//! The sheet metal features after a Sheet metal model (P3I.5; `reference/onshape/sheetmetal/`
//! `simultaneous-sheet-metal.md` SM5, SM7–SM11, SM19.1): **Finish sheet metal model**, **Tab**,
//! **Bend**, **Jog**, **Corner**, **Bend relief** and **Corner break**, all one feature kind
//! ([`crate::FeatureKind::SheetMetalTool`]) so they sit together in the code.
//!
//! Each edits the definition of the active model its picks are on
//! (`cadrs_sheetmetal::edit`, the overrides of `cadrs_sheetmetal::model`), and the rebuild
//! makes the flat pattern and the folded parts again (`rebuild/kernel_ops/sheetmetal/tools.rs`).
//! Finish sheet metal model ends that: the parts become ordinary solids for the features after
//! it, while their flat pattern and table stay (SM10).
//!
//! Lengths are mm, angles in the dialogs degrees (stored as typed and as values).

use cadrs_sheetmetal::edit::{BendAlignment, JogAnchor};
use cadrs_sheetmetal::params::{BendRelief, CornerRelief, CornerReliefKind};
use serde::{Deserialize, Serialize};

use crate::applied::{ChamferMeasurement, ChamferType, EdgeOrFace, FilletMeasurement};
use crate::document::{EdgeRef, FaceRef, RegionRef, VertexRef};
use crate::ids::{FeatureId, PartId};
use crate::sheetmetal::{CurveRef, plain};

/// A face, edge or vertex picked on a sheet metal part (a corner, a bend end): where it was
/// picked is what counts, as the definition is matched by position.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SmPick {
    Face(FaceRef),
    Edge(EdgeRef),
    Vertex(VertexRef),
}

impl SmPick {
    pub fn part(&self) -> PartId {
        match self {
            SmPick::Face(f) => f.part,
            SmPick::Edge(e) => e.part,
            SmPick::Vertex(v) => v.part,
        }
    }

    pub fn seed(&self) -> [f64; 3] {
        match self {
            SmPick::Face(f) => f.seed,
            SmPick::Edge(e) => e.seed,
            SmPick::Vertex(v) => v.point,
        }
    }

    pub fn op(&self) -> cadrs_sketch::OpId {
        match self {
            SmPick::Face(f) => f.face.op,
            SmPick::Edge(e) => e.edge.op(),
            SmPick::Vertex(v) => v.vertex.faces[0].op,
        }
    }

    /// "Face of Bend 1" (…): the kind of entity.
    pub fn what(&self) -> &'static str {
        match self {
            SmPick::Face(_) => "Face",
            SmPick::Edge(_) => "Edge",
            SmPick::Vertex(_) => "Vertex",
        }
    }
}

/// A bend line: a sketch line or a part edge (SM9.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LineRef {
    Sketch(CurveRef),
    Edge(EdgeRef),
}

/// How a Bend's angle is set (SM9.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AngleControl {
    #[default]
    BendAngle,
    /// The bent wall parallel to an edge, face or plane.
    AlignToGeometry,
    /// The bent wall at an angle from an edge, face or plane.
    AngleFromDirection,
}

impl AngleControl {
    pub const ALL: [AngleControl; 3] = [AngleControl::BendAngle, AngleControl::AlignToGeometry, AngleControl::AngleFromDirection];

    pub fn label(self) -> &'static str {
        match self {
            AngleControl::BendAngle => "Bend angle",
            AngleControl::AlignToGeometry => "Align to geometry",
            AngleControl::AngleFromDirection => "Angle from direction",
        }
    }
}

/// **Bend** (SM9; `help/feature-tools/bend-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BendFeature {
    /// "Bend line".
    pub line: Option<LineRef>,
    /// "Sheet metal face to bend" (filled in from the line when there is one face under it).
    pub face: Option<FaceRef>,
    /// The arrow beside the bend line: the other side moves.
    pub hold_opposite: bool,
    pub alignment: BendAlignment,
    pub control: AngleControl,
    /// Bend angle (degrees, 1–359), or the angle from the direction.
    pub angle: f64,
    pub angle_expr: String,
    /// The opposite angle arrow: bend the other way.
    pub opposite: bool,
    /// "Parallel to" / "Direction" (Align to geometry, Angle from direction).
    pub reference: Option<EdgeOrFace>,
    pub use_model_radius: bool,
    pub radius: f64,
    pub radius_expr: String,
    pub use_model_k: bool,
    pub k_factor: f64,
    pub k_expr: String,
}

impl Default for BendFeature {
    fn default() -> Self {
        BendFeature {
            line: None,
            face: None,
            hold_opposite: false,
            alignment: BendAlignment::BendLine,
            control: AngleControl::BendAngle,
            angle: 90.0,
            angle_expr: "90 deg".into(),
            opposite: false,
            reference: None,
            use_model_radius: true,
            radius: 1.0,
            radius_expr: "1 mm".into(),
            use_model_k: true,
            k_factor: 0.45,
            k_expr: "0.45".into(),
        }
    }
}

impl BendFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.line.is_none() {
            return Some("Select a bend line");
        }
        if self.face.is_none() {
            return Some("Select a sheet metal face to bend");
        }
        if self.control != AngleControl::AlignToGeometry && !(self.angle >= 1.0 && self.angle <= 359.0) {
            return Some("The bend angle must be between 1 and 359 degrees");
        }
        if self.control != AngleControl::BendAngle && self.reference.is_none() {
            return Some(if self.control == AngleControl::AlignToGeometry { "Select what to align to" } else { "Select a direction" });
        }
        if !self.use_model_radius && !(self.radius > 0.0) {
            return Some("The bend radius must be greater than 0");
        }
        if !self.use_model_k && !(self.k_factor >= 0.0 && self.k_factor <= 1.0) {
            return Some("The K Factor must be between 0 and 1");
        }
        None
    }

    fn parents(&self, add: &mut dyn FnMut(FeatureId)) {
        match &self.line {
            Some(LineRef::Sketch(c)) => add(c.sketch),
            Some(LineRef::Edge(e)) => add(e.part.feature),
            None => {}
        }
        if let Some(f) = &self.face {
            add(f.part.feature);
        }
        if let Some(r) = &self.reference {
            add(r.part().feature);
        }
    }
}

/// A Jog's Bounding type (SM19.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum JogBounding {
    #[default]
    Blind,
    UpToEntity,
    Thickness,
}

impl JogBounding {
    pub const ALL: [JogBounding; 3] = [JogBounding::Blind, JogBounding::UpToEntity, JogBounding::Thickness];

    pub fn label(self) -> &'static str {
        match self {
            JogBounding::Blind => "Blind",
            JogBounding::UpToEntity => "Up to entity",
            JogBounding::Thickness => "Thickness",
        }
    }
}

/// **Jog** (SM19.1; `help/feature-tools/sm-jog-01.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct JogFeature {
    /// The bend line, face, side, alignment, angle, radius and K factor (as a Bend's).
    pub bend: BendFeature,
    pub bounding: JogBounding,
    /// Jog offset (Blind).
    pub offset: f64,
    pub offset_expr: String,
    /// The entity (Up to entity) and its Offset distance.
    pub up_to: Option<FaceRef>,
    pub up_to_offset_on: bool,
    pub up_to_offset: f64,
    pub up_to_offset_expr: String,
    /// The Thickness factor.
    pub factor: f64,
    pub factor_expr: String,
    pub anchor: JogAnchor,
    pub preserve_material: bool,
}

impl Default for JogFeature {
    fn default() -> Self {
        JogFeature {
            bend: BendFeature::default(),
            bounding: JogBounding::Blind,
            offset: 5.0,
            offset_expr: "5 mm".into(),
            up_to: None,
            up_to_offset_on: false,
            up_to_offset: 0.0,
            up_to_offset_expr: "0 mm".into(),
            factor: 4.0,
            factor_expr: "4".into(),
            anchor: JogAnchor::Inside,
            preserve_material: true,
        }
    }
}

impl JogFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if let Some(p) = self.bend.problem() {
            return Some(p);
        }
        match self.bounding {
            JogBounding::Blind if !(self.offset > 0.0) => Some("The jog offset must be greater than 0"),
            JogBounding::UpToEntity if self.up_to.is_none() => Some("Select the entity to jog up to"),
            JogBounding::Thickness if !(self.factor > 0.0) => Some("The thickness factor must be greater than 0"),
            _ => None,
        }
    }
}

/// **Tab** (SM5; `help/feature-tools/sheetmetaltab-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TabFeature {
    /// "Tab profile": closed sketch regions...
    pub regions: Vec<RegionRef>,
    /// ...and whole sketches.
    pub sketches: Vec<FeatureId>,
    /// "Flange to merge": the walls (faces) the tab is added to; empty: every wall the profile
    /// is parallel to and touches.
    pub flanges: Vec<FaceRef>,
    /// "Subtraction offset" (mm).
    pub offset: f64,
    pub offset_expr: String,
    /// "Subtraction scope": parts a clearance pocket round the tab is cut from.
    pub scope: Vec<PartId>,
}

impl Default for TabFeature {
    fn default() -> Self {
        TabFeature { regions: Vec::new(), sketches: Vec::new(), flanges: Vec::new(), offset: 0.0, offset_expr: "0 mm".into(), scope: Vec::new() }
    }
}

impl TabFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.regions.is_empty() && self.sketches.is_empty() {
            return Some("Select a tab profile");
        }
        if self.offset < 0.0 || self.offset.is_nan() {
            return Some("The subtraction offset must be at least 0");
        }
        None
    }

    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.sketches.clone();
        for r in &self.regions {
            if !v.contains(&r.sketch) {
                v.push(r.sketch);
            }
        }
        v
    }
}

/// **Corner** (SM7; `help/feature-tools/sheetmetalcorner-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CornerFeature {
    /// "Corner": a face, edge or vertex of it.
    pub corner: Option<SmPick>,
    pub relief: CornerRelief,
    pub scale_expr: String,
    pub size_expr: String,
}

impl Default for CornerFeature {
    fn default() -> Self {
        let relief = CornerRelief { kind: CornerReliefKind::RectangleScaled, scale: 1.5, size: 3.0 };
        CornerFeature { corner: None, relief, scale_expr: "1.5".into(), size_expr: "3 mm".into() }
    }
}

impl CornerFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.corner.is_none() {
            return Some("Select a corner");
        }
        let r = &self.relief;
        if r.kind.is_scaled() && !(1.0..=2.0).contains(&r.scale) {
            return Some("The corner relief scale must be between 1 and 2");
        }
        if r.kind.is_sized() && !(r.size > 0.0) {
            return Some("The corner relief size must be greater than 0");
        }
        None
    }
}

/// **Bend relief** (SM8; `help/feature-tools/sheetmetalbendrelief-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BendReliefFeature {
    /// "Bend relief": a face, edge or vertex at the bend's end.
    pub end: Option<SmPick>,
    /// The type, scales, depth and Extend bend relief.
    pub relief: BendRelief,
    pub depth_scale_expr: String,
    pub width_scale_expr: String,
    pub depth_expr: String,
}

impl Default for BendReliefFeature {
    fn default() -> Self {
        let relief = BendRelief { width_scale: 2.0, ..BendRelief::default() };
        BendReliefFeature {
            end: None,
            relief,
            depth_scale_expr: plain(relief.depth_scale),
            width_scale_expr: plain(relief.width_scale),
            depth_expr: format!("{} mm", plain(relief.depth)),
        }
    }
}

impl BendReliefFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.end.is_none() {
            return Some("Select a bend relief");
        }
        let r = &self.relief;
        if r.kind.is_scaled() {
            if !(1.0..=5.0).contains(&r.depth_scale) {
                return Some("The bend relief depth scale must be between 1 and 5");
            }
            if !(0.0625..=2.0).contains(&r.width_scale) {
                return Some("The bend relief width scale must be between 0.0625 and 2");
            }
        }
        if r.kind.is_sized() && !(r.depth > 0.0) {
            return Some("The bend relief depth must be greater than 0");
        }
        None
    }
}

/// **Corner break** (SM11; `help/feature-tools/sm-cornerbreak-01.png`): the Fillet tab (Radius
/// or Width, Distance control) or the Chamfer tab.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CornerBreakFeature {
    /// "Entities to fillet or chamfer": corner edges or vertices.
    pub entities: Vec<SmPick>,
    /// The Chamfer tab (else Fillet).
    pub chamfer: bool,
    pub fillet_measurement: FilletMeasurement,
    /// Radius or Width (mm).
    pub size: f64,
    pub size_expr: String,
    pub chamfer_measurement: ChamferMeasurement,
    pub chamfer_type: ChamferType,
    pub distance: f64,
    pub distance_expr: String,
    pub distance2: f64,
    pub distance2_expr: String,
    /// Degrees.
    pub angle: f64,
    pub angle_expr: String,
    /// The chamfer's opposite direction (its distances swap sides).
    pub flip: bool,
}

impl Default for CornerBreakFeature {
    fn default() -> Self {
        CornerBreakFeature {
            entities: Vec::new(),
            chamfer: false,
            fillet_measurement: FilletMeasurement::Radius,
            size: 5.0,
            size_expr: "5 mm".into(),
            chamfer_measurement: ChamferMeasurement::Offset,
            chamfer_type: ChamferType::EqualDistance,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            distance2: 5.0,
            distance2_expr: "5 mm".into(),
            angle: 45.0,
            angle_expr: "45 deg".into(),
            flip: false,
        }
    }
}

impl CornerBreakFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.entities.is_empty() {
            return Some("Select corners to fillet or chamfer");
        }
        if !self.chamfer && !(self.size > 0.0) {
            return Some("The radius must be greater than 0");
        }
        if self.chamfer {
            if !(self.distance > 0.0) || (self.chamfer_type == ChamferType::TwoDistances && !(self.distance2 > 0.0)) {
                return Some("The distance must be greater than 0");
            }
            if self.chamfer_type == ChamferType::DistanceAngle && !(self.angle > 0.0 && self.angle < 180.0) {
                return Some("The angle must be between 0 and 180 degrees");
            }
        }
        None
    }
}

/// **Finish sheet metal model** (SM10; `help/feature-tools/smm-finish-01.png`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FinishFeature {
    /// "Sheet metal parts".
    pub parts: Vec<PartId>,
}

/// The warning Finish sheet metal model's dialog shows (exercise E4).
pub const FINISH_WARNING: &str = "Features after this one act on the parts as ordinary solids and don't show in the flat pattern";

/// One of the sheet metal features after a Sheet metal model.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SheetMetalTool {
    Finish(FinishFeature),
    Tab(TabFeature),
    Bend(BendFeature),
    Jog(JogFeature),
    Corner(CornerFeature),
    BendRelief(BendReliefFeature),
    CornerBreak(CornerBreakFeature),
}

impl SheetMetalTool {
    /// The feature's base name (and type, in the feature list).
    pub fn label(&self) -> &'static str {
        match self {
            SheetMetalTool::Finish(_) => "Finish sheet metal model",
            SheetMetalTool::Tab(_) => "Tab",
            SheetMetalTool::Bend(_) => "Bend",
            SheetMetalTool::Jog(_) => "Jog",
            SheetMetalTool::Corner(_) => "Corner",
            SheetMetalTool::BendRelief(_) => "Bend relief",
            SheetMetalTool::CornerBreak(_) => "Corner break",
        }
    }

    /// Its icon (icon-rs) and toolbar name.
    pub fn icon(&self) -> &'static str {
        match self {
            SheetMetalTool::Finish(_) => "sheet-metal-finish",
            SheetMetalTool::Tab(_) => "sheet-metal-tab",
            SheetMetalTool::Bend(_) => "sheet-metal-bend",
            SheetMetalTool::Jog(_) => "sheet-metal-jog",
            SheetMetalTool::Corner(_) => "sheet-metal-corner",
            SheetMetalTool::BendRelief(_) => "sheet-metal-bend-relief",
            SheetMetalTool::CornerBreak(_) => "sheet-metal-corner-break",
        }
    }

    /// A new feature for a toolbar name ("sheet-metal-bend", …).
    pub fn for_tool(name: &str) -> Option<SheetMetalTool> {
        Some(match name {
            "sheet-metal-finish" => SheetMetalTool::Finish(FinishFeature::default()),
            "sheet-metal-tab" => SheetMetalTool::Tab(TabFeature::default()),
            "sheet-metal-bend" => SheetMetalTool::Bend(BendFeature::default()),
            "sheet-metal-jog" => SheetMetalTool::Jog(JogFeature::default()),
            "sheet-metal-corner" => SheetMetalTool::Corner(CornerFeature::default()),
            "sheet-metal-bend-relief" => SheetMetalTool::BendRelief(BendReliefFeature::default()),
            "sheet-metal-corner-break" => SheetMetalTool::CornerBreak(CornerBreakFeature::default()),
            _ => return None,
        })
    }

    pub fn problem(&self) -> Option<&'static str> {
        match self {
            SheetMetalTool::Finish(x) => x.parts.is_empty().then_some("Select sheet metal parts"),
            SheetMetalTool::Tab(x) => x.problem(),
            SheetMetalTool::Bend(x) => x.problem(),
            SheetMetalTool::Jog(x) => x.problem(),
            SheetMetalTool::Corner(x) => x.problem(),
            SheetMetalTool::BendRelief(x) => x.problem(),
            SheetMetalTool::CornerBreak(x) => x.problem(),
        }
    }

    /// The sketches it takes profiles of (hidden once used, like an extrude's).
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        match self {
            SheetMetalTool::Tab(x) => x.sketch_ids(),
            _ => Vec::new(),
        }
    }

    /// The features it refers to (PS11).
    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out: Vec<FeatureId> = Vec::new();
        let mut add = |f: FeatureId| {
            if !out.contains(&f) {
                out.push(f);
            }
        };
        match self {
            SheetMetalTool::Finish(x) => x.parts.iter().for_each(|p| add(p.feature)),
            SheetMetalTool::Tab(x) => {
                x.sketch_ids().into_iter().for_each(&mut add);
                x.flanges.iter().for_each(|f| add(f.part.feature));
                x.scope.iter().for_each(|p| add(p.feature));
            }
            SheetMetalTool::Bend(x) => x.parents(&mut add),
            SheetMetalTool::Jog(x) => {
                x.bend.parents(&mut add);
                if let Some(f) = &x.up_to {
                    add(f.part.feature);
                }
            }
            SheetMetalTool::Corner(x) => x.corner.iter().for_each(|p| add(p.part().feature)),
            SheetMetalTool::BendRelief(x) => x.end.iter().for_each(|p| add(p.part().feature)),
            SheetMetalTool::CornerBreak(x) => x.entities.iter().for_each(|p| add(p.part().feature)),
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_the_dialogs_and_round_trip() {
        let b = BendFeature::default();
        assert_eq!((b.angle, b.alignment, b.use_model_radius, b.use_model_k), (90.0, BendAlignment::BendLine, true, true));
        let j = JogFeature::default();
        assert_eq!((j.anchor, j.preserve_material, j.bounding), (JogAnchor::Inside, true, JogBounding::Blind));
        for name in ["sheet-metal-finish", "sheet-metal-tab", "sheet-metal-bend", "sheet-metal-jog", "sheet-metal-corner", "sheet-metal-bend-relief", "sheet-metal-corner-break"] {
            let t = SheetMetalTool::for_tool(name).unwrap();
            assert_eq!(t.icon(), name);
            assert!(t.problem().is_some(), "{name}: nothing picked yet");
            let s = ron::to_string(&t).unwrap();
            assert_eq!(ron::from_str::<SheetMetalTool>(&s).unwrap(), t);
        }
    }
}

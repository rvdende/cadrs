//! The sheet metal features after a Sheet metal model that edit its definition (P3I.4;
//! `reference/onshape/sheetmetal/simultaneous-sheet-metal.md` SM3, SM4, SM6.1–SM6.3):
//! **Flange**, **Hem** and **Make joint**, one [`crate::FeatureKind::SheetMetal`] variant.
//!
//! Each picks free edges (or side faces) of an active model's folded parts. The rebuild
//! (`rebuild/kernel_ops/sheetmetal_features.rs`) finds the model the parts belong to, adds walls
//! and joints to its definition ([`cadrs_sheetmetal::sharp_edit`]) and makes the folded parts and
//! the flat pattern again through the SM1.6 hook (`Rebuilder::edit_sheet_metal`).
//!
//! Lengths are mm; angles are degrees here (as typed) and radians in `cadrs_sheetmetal`.

// NaN-safe checks: `!(x > 0.0)` is true for NaN too, which is what they mean.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use cadrs_sheetmetal::model::{HemAlignment, RipStyle};
use cadrs_sheetmetal::sharp_edit::{FlangeAlignment, HemKind};
use cadrs_sketch::PlaneRef;
use serde::{Deserialize, Serialize};

use crate::applied::EdgeOrFace;
use crate::document::{DirectionRef, EdgeRef, FaceRef, VertexRef};
use crate::ids::FeatureId;

/// One of the sheet metal features that edit an active model.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SheetMetalFeature {
    Flange(FlangeFeature),
    Hem(HemFeature),
    MakeJoint(MakeJointFeature),
}

impl SheetMetalFeature {
    /// The base name ("Flange 1") and the icon.
    pub fn label(&self) -> &'static str {
        match self {
            SheetMetalFeature::Flange(_) => "Flange",
            SheetMetalFeature::Hem(_) => "Hem",
            SheetMetalFeature::MakeJoint(_) => "Make joint",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            SheetMetalFeature::Flange(_) => "sheet-metal-flange",
            SheetMetalFeature::Hem(_) => "sheet-metal-hem",
            SheetMetalFeature::MakeJoint(_) => "sheet-metal-make-joint",
        }
    }

    /// The picked edges or side faces.
    pub fn entities(&self) -> &[EdgeOrFace] {
        match self {
            SheetMetalFeature::Flange(x) => &x.edges,
            SheetMetalFeature::Hem(x) => &x.edges,
            SheetMetalFeature::MakeJoint(x) => &x.edges,
        }
    }

    pub fn problem(&self) -> Option<&'static str> {
        match self {
            SheetMetalFeature::Flange(x) => x.problem(),
            SheetMetalFeature::Hem(x) => x.problem(),
            SheetMetalFeature::MakeJoint(x) => x.problem(),
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
        for e in self.entities() {
            add(e.part().feature);
        }
        if let SheetMetalFeature::Flange(x) = self {
            for t in [x.up_to, x.bound.up_to, x.second.as_ref().and_then(|s| s.up_to)].into_iter().flatten() {
                t.parent().into_iter().for_each(&mut add);
            }
            for d in [&x.parallel_to, &x.direction].into_iter().flatten() {
                crate::document::direction_parent(d).into_iter().for_each(&mut add);
            }
        }
        out
    }

    /// Its typed values, for variables (P3F.4): (label, expression, value, is an angle).
    pub fn exprs_mut(&mut self) -> Vec<(&'static str, &mut String, &mut f64, bool)> {
        let mut v: Vec<(&'static str, &mut String, &mut f64, bool)> = Vec::new();
        match self {
            SheetMetalFeature::Flange(x) => {
                v.push(("Distance", &mut x.distance_expr, &mut x.distance, false));
                v.push(("Offset", &mut x.offset_expr, &mut x.offset, false));
                v.push(("Bend angle", &mut x.angle_expr, &mut x.angle, true));
                v.push(("Angle", &mut x.direction_angle_expr, &mut x.direction_angle, true));
                v.push(("Miter angle", &mut x.miter_angle_expr, &mut x.miter_angle, true));
                v.push(("Bend radius", &mut x.radius_expr, &mut x.radius, false));
                v.push(("First bound", &mut x.bound.distance_expr, &mut x.bound.distance, false));
                if let Some(s) = &mut x.second {
                    v.push(("Second bound", &mut s.distance_expr, &mut s.distance, false));
                }
            }
            SheetMetalFeature::Hem(x) => {
                v.push(("Inner radius", &mut x.radius_expr, &mut x.radius, false));
                v.push(("Total length", &mut x.total_expr, &mut x.total, false));
                v.push(("Angle", &mut x.angle_expr, &mut x.angle, true));
                v.push(("Gap", &mut x.gap_expr, &mut x.gap, false));
            }
            SheetMetalFeature::MakeJoint(x) => {
                v.push(("Bend radius", &mut x.radius_expr, &mut x.radius, false));
            }
        }
        v
    }
}

/// What an Up to entity end goes up to (SM3.3, SM3.7).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SmTarget {
    Face(FaceRef),
    Edge(EdgeRef),
    Vertex(VertexRef),
    Plane(PlaneRef),
}

impl SmTarget {
    pub fn parent(&self) -> Option<FeatureId> {
        match self {
            SmTarget::Face(f) => Some(FeatureId(f.face.op)),
            SmTarget::Edge(e) => Some(FeatureId(e.edge.op())),
            SmTarget::Vertex(v) => Some(v.part.feature),
            SmTarget::Plane(PlaneRef::Feature(f)) => Some(FeatureId(f.feature)),
            SmTarget::Plane(_) => None,
        }
    }
}

/// A flange's end type (SM3.3) and a partial flange's bound type (SM3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FlangeEnd {
    #[default]
    Blind,
    UpToEntity,
    UpToEntityOffset,
}

impl FlangeEnd {
    pub const ALL: [FlangeEnd; 3] = [FlangeEnd::Blind, FlangeEnd::UpToEntity, FlangeEnd::UpToEntityOffset];

    pub fn label(self) -> &'static str {
        match self {
            FlangeEnd::Blind => "Blind",
            FlangeEnd::UpToEntity => "Up to entity",
            FlangeEnd::UpToEntityOffset => "Up to entity with offset",
        }
    }
}

/// How a flange's angle is set (SM3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum AngleControl {
    #[default]
    BendAngle,
    AlignToGeometry,
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

/// A partial flange's Per edge / Per chain (SM3.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChainType {
    #[default]
    PerEdge,
    PerChain,
}

impl ChainType {
    pub const ALL: [ChainType; 2] = [ChainType::PerEdge, ChainType::PerChain];

    pub fn label(self) -> &'static str {
        match self {
            ChainType::PerEdge => "Per edge",
            ChainType::PerChain => "Per chain",
        }
    }
}

/// One end of a partial flange: how far in from the edge's end it starts.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bound {
    pub kind: FlangeEnd,
    pub distance: f64,
    pub distance_expr: String,
    #[serde(default)]
    pub up_to: Option<SmTarget>,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "zero_mm")]
    pub offset_expr: String,
}

impl Default for Bound {
    fn default() -> Self {
        Bound { kind: FlangeEnd::Blind, distance: 25.0, distance_expr: "25 mm".into(), up_to: None, offset: 0.0, offset_expr: zero_mm() }
    }
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn yes() -> bool {
    true
}

/// A **Flange** (SM3; `help/feature-tools/sheetmetalflange-dialog-01.png`, `-03.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlangeFeature {
    /// Edges or side faces to flange.
    pub edges: Vec<EdgeOrFace>,
    #[serde(default)]
    pub alignment: FlangeAlignment,
    #[serde(default)]
    pub end: FlangeEnd,
    /// Blind: from the outer virtual sharp to the flange's tip.
    pub distance: f64,
    pub distance_expr: String,
    #[serde(default)]
    pub up_to: Option<SmTarget>,
    #[serde(default)]
    pub offset: f64,
    #[serde(default = "zero_mm")]
    pub offset_expr: String,
    #[serde(default)]
    pub angle_control: AngleControl,
    /// Bend angle (degrees).
    pub angle: f64,
    pub angle_expr: String,
    /// The opposite direction arrow.
    #[serde(default)]
    pub flip: bool,
    /// Align to geometry's "Parallel to".
    #[serde(default)]
    pub parallel_to: Option<DirectionRef>,
    /// Angle from direction's direction and angle (degrees, 1–359).
    #[serde(default)]
    pub direction: Option<DirectionRef>,
    #[serde(default = "ninety")]
    pub direction_angle: f64,
    #[serde(default = "ninety_deg")]
    pub direction_angle_expr: String,
    #[serde(default = "yes")]
    pub auto_miter: bool,
    #[serde(default = "forty_five")]
    pub miter_angle: f64,
    #[serde(default = "forty_five_deg")]
    pub miter_angle_expr: String,
    #[serde(default = "yes")]
    pub use_model_radius: bool,
    #[serde(default = "one")]
    pub radius: f64,
    #[serde(default = "one_mm")]
    pub radius_expr: String,
    // Partial flange (SM3.7).
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub chain: ChainType,
    #[serde(default)]
    pub flip_sides: bool,
    #[serde(default = "yes")]
    pub hold_adjacent: bool,
    #[serde(default)]
    pub bound: Bound,
    #[serde(default)]
    pub second: Option<Bound>,
}

fn ninety() -> f64 {
    90.0
}
fn ninety_deg() -> String {
    "90 deg".into()
}
fn forty_five() -> f64 {
    45.0
}
fn forty_five_deg() -> String {
    "45 deg".into()
}
fn one() -> f64 {
    1.0
}
fn one_mm() -> String {
    "1 mm".into()
}

impl Default for FlangeFeature {
    fn default() -> Self {
        FlangeFeature {
            edges: Vec::new(),
            alignment: FlangeAlignment::Inner,
            end: FlangeEnd::Blind,
            distance: 25.0,
            distance_expr: "25 mm".into(),
            up_to: None,
            offset: 0.0,
            offset_expr: zero_mm(),
            angle_control: AngleControl::BendAngle,
            angle: 90.0,
            angle_expr: ninety_deg(),
            flip: false,
            parallel_to: None,
            direction: None,
            direction_angle: 90.0,
            direction_angle_expr: ninety_deg(),
            auto_miter: true,
            miter_angle: 45.0,
            miter_angle_expr: forty_five_deg(),
            use_model_radius: true,
            radius: 1.0,
            radius_expr: one_mm(),
            partial: false,
            chain: ChainType::PerEdge,
            flip_sides: false,
            hold_adjacent: true,
            bound: Bound::default(),
            second: None,
        }
    }
}

impl FlangeFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.edges.is_empty() {
            return Some("Select edges or side faces to flange");
        }
        match self.end {
            FlangeEnd::Blind if !(self.distance > 0.0) => return Some("The distance must be greater than zero"),
            FlangeEnd::UpToEntity | FlangeEnd::UpToEntityOffset if self.up_to.is_none() => return Some("Select what the flange goes up to"),
            _ => {}
        }
        match self.angle_control {
            AngleControl::BendAngle if !(self.angle > 0.0 && self.angle < 180.0) => return Some("The bend angle must be between 0 and 180 degrees"),
            AngleControl::AlignToGeometry if self.parallel_to.is_none() => return Some("Select what the flange is parallel to"),
            AngleControl::AngleFromDirection if self.direction.is_none() => return Some("Select the direction the angle is measured from"),
            AngleControl::AngleFromDirection if !(self.direction_angle >= 1.0 && self.direction_angle <= 359.0) => {
                return Some("The angle must be between 1 and 359 degrees");
            }
            _ => {}
        }
        if !self.auto_miter && !(self.miter_angle > 0.0 && self.miter_angle < 180.0) {
            return Some("The miter angle must be between 0 and 180 degrees");
        }
        if !self.use_model_radius && !(self.radius >= 0.0) {
            return Some("The bend radius must be at least 0");
        }
        if self.partial {
            for b in std::iter::once(&self.bound).chain(self.second.as_ref()) {
                if b.kind != FlangeEnd::Blind && b.up_to.is_none() {
                    return Some("Select what the partial flange's bound goes up to");
                }
                if b.kind == FlangeEnd::Blind && !(b.distance >= 0.0) {
                    return Some("A bound's distance must be at least 0");
                }
            }
        }
        None
    }
}

/// A **Hem** (SM4; `help/feature-tools/shmetal-hem-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HemFeature {
    pub edges: Vec<EdgeOrFace>,
    /// The flip arrow: the other side.
    #[serde(default)]
    pub flip: bool,
    #[serde(default)]
    pub kind: HemKind,
    /// Straight: lies flat with the model's minimal gap (else the inner radius).
    #[serde(default = "yes")]
    pub flattened: bool,
    pub radius: f64,
    pub radius_expr: String,
    /// Straight and tear drop: from the hem's outermost point to its end.
    pub total: f64,
    pub total_expr: String,
    /// Rolled: degrees (more than 180).
    pub angle: f64,
    pub angle_expr: String,
    /// Tear drop: the model's minimal gap, else `gap`.
    #[serde(default = "yes")]
    pub use_minimal_gap: bool,
    pub gap: f64,
    pub gap_expr: String,
    #[serde(default)]
    pub alignment: HemAlignment,
    /// Closed corners (else Simple).
    #[serde(default)]
    pub closed: bool,
}

impl Default for HemFeature {
    fn default() -> Self {
        HemFeature {
            edges: Vec::new(),
            flip: false,
            kind: HemKind::Straight,
            flattened: true,
            radius: 1.0,
            radius_expr: one_mm(),
            total: 12.5,
            total_expr: "12.5 mm".into(),
            angle: 270.0,
            angle_expr: "270 deg".into(),
            use_minimal_gap: true,
            gap: 0.5,
            gap_expr: "0.5 mm".into(),
            alignment: HemAlignment::Outer,
            closed: false,
        }
    }
}

impl HemFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.edges.is_empty() {
            return Some("Select edges or side faces to hem");
        }
        let radius_used = !(self.kind == HemKind::Straight && self.flattened);
        if radius_used && !(self.radius >= 0.0) {
            return Some("The inner radius must be at least 0");
        }
        if self.kind == HemKind::Rolled && !(self.angle > 180.0 && self.angle < 360.0) {
            return Some("The angle must be between 180 and 360 degrees");
        }
        if self.kind != HemKind::Rolled && !(self.total > 0.0) {
            return Some("The total length must be greater than zero");
        }
        if self.kind == HemKind::TearDrop && !self.use_minimal_gap && !(self.gap >= 0.0) {
            return Some("The gap must be at least 0");
        }
        None
    }
}

/// Make joint's joint type (SM6.2, SM6.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MakeJointType {
    #[default]
    Rip,
    Bend,
}

impl MakeJointType {
    pub const ALL: [MakeJointType; 2] = [MakeJointType::Rip, MakeJointType::Bend];

    pub fn label(self) -> &'static str {
        match self {
            MakeJointType::Rip => "Rip",
            MakeJointType::Bend => "Bend",
        }
    }
}

/// A **Make joint** (SM6.1–SM6.3; `help/feature-tools/sheetmetalmakejoint-dialog.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MakeJointFeature {
    /// Exactly two edges or side faces.
    pub edges: Vec<EdgeOrFace>,
    #[serde(default)]
    pub kind: MakeJointType,
    #[serde(default)]
    pub style: RipStyle,
    #[serde(default = "yes")]
    pub use_model_radius: bool,
    #[serde(default = "one")]
    pub radius: f64,
    #[serde(default = "one_mm")]
    pub radius_expr: String,
}

impl Default for MakeJointFeature {
    fn default() -> Self {
        MakeJointFeature { edges: Vec::new(), kind: MakeJointType::Rip, style: RipStyle::EdgeJoint, use_model_radius: true, radius: 1.0, radius_expr: one_mm() }
    }
}

impl MakeJointFeature {
    pub fn problem(&self) -> Option<&'static str> {
        match self.edges.len() {
            0 | 1 => Some("Select two edges or side faces to join"),
            2 => (self.kind == MakeJointType::Bend && !self.use_model_radius && !(self.radius >= 0.0)).then_some("The bend radius must be at least 0"),
            _ => Some("Select exactly two edges or side faces"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_onshape_and_round_trip() {
        let f = FlangeFeature::default();
        assert_eq!((f.alignment, f.end, f.angle_control, f.angle, f.auto_miter, f.use_model_radius, f.partial, f.hold_adjacent), (FlangeAlignment::Inner, FlangeEnd::Blind, AngleControl::BendAngle, 90.0, true, true, false, true));
        let h = HemFeature::default();
        assert_eq!((h.kind, h.flattened, h.total, h.alignment, h.closed), (HemKind::Straight, true, 12.5, HemAlignment::Outer, false));
        let j = MakeJointFeature::default();
        assert_eq!((j.kind, j.style), (MakeJointType::Rip, RipStyle::EdgeJoint));
        for x in [SheetMetalFeature::Flange(f), SheetMetalFeature::Hem(h), SheetMetalFeature::MakeJoint(j)] {
            assert!(x.problem().is_some(), "nothing picked");
            let s = ron::to_string(&x).unwrap();
            assert_eq!(ron::from_str::<SheetMetalFeature>(&s).unwrap(), x);
        }
        let old: FlangeFeature = ron::from_str("(edges: [], distance: 10.0, distance_expr: \"10 mm\", angle: 90.0, angle_expr: \"90 deg\")").unwrap();
        assert!(old.auto_miter && old.hold_adjacent);
    }
}

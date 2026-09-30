//! Transform (Onshape's Transform Part Studio feature): parts moved, turned or scaled, or copies
//! of them made there. Its rebuild is in `rebuild/kernel_ops/transform.rs`.
//!
//! The types map one to one onto Onshape's `TransformType` (see [`TransformType::onshape`]):
//!
//! | cadrs | Onshape | dialog | fields used |
//! |---|---|---|---|
//! | [`TransformType::TranslateByLine`] | `TRANSLATION_ENTITY` | Translate by line | `line`, `flip` |
//! | [`TransformType::TranslateByDistance`] | `TRANSLATION_DISTANCE` | Translate by distance | `direction`, `distance`, `flip` |
//! | [`TransformType::TranslateXyz`] | `TRANSLATION_3D` | Translate by XYZ | `dx`, `dy`, `dz` |
//! | [`TransformType::MateConnectors`] | `TRANSFORM_MATE_CONNECTOR` | Transform by mate connectors | `from`, `to`, `flip_primary`, `secondary` |
//! | [`TransformType::Rotate`] | `ROTATION` | Rotate | `axis`, `angle`, `flip` |
//! | [`TransformType::CopyInPlace`] | `COPY` | Copy in place | (none; always copies) |
//! | [`TransformType::ScaleUniformly`] | `SCALE_UNIFORMLY` | Scale uniformly | `scale_point`, `scale` |
//!
//! plus `parts` ("Parts to transform", Onshape's `entities`) and `copy` ("Copy part",
//! `makeCopy`).
//!
//! **Names.** A moved (or scaled) part keeps its [`PartId`], its name, appearance and material,
//! and every face, edge and vertex keeps its persistent name, so later features that refer to
//! them (a fillet, a sketch on a face) still find them. A copy ("Copy part" on, or Copy in
//! place) is a new part, `PartId::new(<transform id>, i)`, whose faces are named as a pattern
//! copy's: `FaceOrigin::Instance { of, face, instance: 1 }` under the Transform's id, and it
//! shows its original's appearance and material (`Part::source`).
//!
//! **In-context parts (P3H.6; PCB7.9, X9).** In a Part Studio made or edited in the context of
//! an assembly (P3B.9, P3H.5), a Transform can also copy the context's parts (they can only be
//! copied: Copy in place, or Copy part on). The context is a snapshot of other Part Studios'
//! parts, so the feature keeps its own snapshot of what it copies ([`ContextCopy`],
//! [`ContextSource`]: the source studios' features when it was made, and where each part was),
//! which the rebuild builds again; [`context_copies`] makes it from the picked parts. The copies
//! keep their Part Studio's colour (`Solid::looks`).
//!
//! **Composite part** ([`CompositeFeature`], P3H.6): groups parts into one part ("Composite
//! part N") without merging them (a kernel compound, `cadrs_kernel::Kernel::compound`): its
//! volume is the sum of theirs and its faces keep their members' names and colours. **Closed**:
//! listed, picked and inserted as one part (its members are left out of `Build::parts`); open:
//! listed with its members.

use serde::{Deserialize, Serialize};

use crate::assembly::Pose;
use crate::document::{AxisRef, DirectionRef, Document, Feature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::mate::ConnectorRef;

/// How the parts move (the dialog's first dropdown; Onshape's `transformType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TransformType {
    /// Along a line (a straight edge or a sketch line) by its length, from its first end to its
    /// second (`TRANSLATION_ENTITY`).
    #[default]
    TranslateByLine,
    /// Along a direction by a distance (`TRANSLATION_DISTANCE`).
    TranslateByDistance,
    /// By X, Y and Z distances (`TRANSLATION_3D`).
    TranslateXyz,
    /// So the `from` mate connector lands on the `to` one (`TRANSFORM_MATE_CONNECTOR`).
    MateConnectors,
    /// About an axis by an angle (`ROTATION`).
    Rotate,
    /// Copies where the parts are (`COPY`).
    CopyInPlace,
    /// Scaled about a point by a factor (`SCALE_UNIFORMLY`).
    ScaleUniformly,
}

impl TransformType {
    /// In the dialog's order (Onshape's).
    pub const ALL: [TransformType; 7] = [
        TransformType::TranslateByLine,
        TransformType::TranslateByDistance,
        TransformType::TranslateXyz,
        TransformType::MateConnectors,
        TransformType::Rotate,
        TransformType::CopyInPlace,
        TransformType::ScaleUniformly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            TransformType::TranslateByLine => "Translate by line",
            TransformType::TranslateByDistance => "Translate by distance",
            TransformType::TranslateXyz => "Translate by XYZ",
            TransformType::MateConnectors => "Transform by mate connectors",
            TransformType::Rotate => "Rotate",
            TransformType::CopyInPlace => "Copy in place",
            TransformType::ScaleUniformly => "Scale uniformly",
        }
    }

    /// Onshape's `TransformType` enum name.
    pub fn onshape(self) -> &'static str {
        match self {
            TransformType::TranslateByLine => "TRANSLATION_ENTITY",
            TransformType::TranslateByDistance => "TRANSLATION_DISTANCE",
            TransformType::TranslateXyz => "TRANSLATION_3D",
            TransformType::MateConnectors => "TRANSFORM_MATE_CONNECTOR",
            TransformType::Rotate => "ROTATION",
            TransformType::CopyInPlace => "COPY",
            TransformType::ScaleUniformly => "SCALE_UNIFORMLY",
        }
    }

    /// The type for Onshape's enum name.
    pub fn from_onshape(name: &str) -> Option<TransformType> {
        TransformType::ALL.into_iter().find(|t| t.onshape() == name)
    }

    /// True for the types that scale (the others are rigid motions).
    pub fn scales(self) -> bool {
        self == TransformType::ScaleUniformly
    }
}

/// Where the destination connector's X axis goes ("Reorient secondary axis"; Onshape's
/// `secondaryAxisType`): the destination turned about its Z by 0°, 90°, 180° or 270°.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SecondaryAxis {
    #[default]
    PlusX,
    PlusY,
    MinusX,
    MinusY,
}

impl SecondaryAxis {
    pub const ALL: [SecondaryAxis; 4] = [SecondaryAxis::PlusX, SecondaryAxis::PlusY, SecondaryAxis::MinusX, SecondaryAxis::MinusY];

    /// Onshape's enum name (`PLUS_X`, …).
    pub fn onshape(self) -> &'static str {
        match self {
            SecondaryAxis::PlusX => "PLUS_X",
            SecondaryAxis::PlusY => "PLUS_Y",
            SecondaryAxis::MinusX => "MINUS_X",
            SecondaryAxis::MinusY => "MINUS_Y",
        }
    }

    pub fn from_onshape(name: &str) -> Option<SecondaryAxis> {
        SecondaryAxis::ALL.into_iter().find(|a| a.onshape() == name)
    }

    /// The turn about the destination's Z (degrees).
    pub fn degrees(self) -> f64 {
        match self {
            SecondaryAxis::PlusX => 0.0,
            SecondaryAxis::PlusY => 90.0,
            SecondaryAxis::MinusX => 180.0,
            SecondaryAxis::MinusY => 270.0,
        }
    }

    /// The next one (the dialog's Reorient button turns it a quarter).
    pub fn next(self) -> SecondaryAxis {
        let i = SecondaryAxis::ALL.iter().position(|a| *a == self).unwrap_or(0);
        SecondaryAxis::ALL[(i + 1) % 4]
    }
}

/// The Transform feature. Distances are mm, angles degrees; each value has the text typed for it
/// (`*_expr`, "25 mm"), as the other features keep theirs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransformFeature {
    #[serde(default)]
    pub transform_type: TransformType,
    /// "Parts to transform" (Onshape's `entities`).
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// "Copy part" (Onshape's `makeCopy`): the parts stay and moved copies are made. Copy in
    /// place always copies.
    #[serde(default)]
    pub copy: bool,
    /// Translate by line: a straight part edge ([`DirectionRef::Edge`]) or a sketch line
    /// ([`DirectionRef::SketchLine`]); the parts move by its length, from its first end to its
    /// second (the other `DirectionRef` kinds are refused).
    #[serde(default)]
    pub line: Option<DirectionRef>,
    /// Translate by distance: an edge, a sketch line, a face's or plane's normal, or a mate
    /// connector's Z axis.
    #[serde(default)]
    pub direction: Option<DirectionRef>,
    #[serde(default = "default_distance")]
    pub distance: f64,
    #[serde(default = "default_distance_expr")]
    pub distance_expr: String,
    /// Opposite direction (Onshape's `oppositeDirection`): Translate by line and by distance go
    /// the other way, Rotate turns the other way.
    #[serde(default)]
    pub flip: bool,
    /// Translate by XYZ (mm, any sign).
    #[serde(default)]
    pub dx: f64,
    #[serde(default = "zero_mm")]
    pub dx_expr: String,
    #[serde(default)]
    pub dy: f64,
    #[serde(default = "zero_mm")]
    pub dy_expr: String,
    #[serde(default)]
    pub dz: f64,
    #[serde(default = "zero_mm")]
    pub dz_expr: String,
    /// Transform by mate connectors: the connector on the parts (Onshape's `baseConnector`) and
    /// where it goes (`destinationConnector`). The parts move so `from`'s frame lands on `to`'s
    /// (after `flip_primary` and `secondary` change `to`).
    #[serde(default)]
    pub from: Option<ConnectorRef>,
    #[serde(default)]
    pub to: Option<ConnectorRef>,
    /// "Flip primary axis" (Onshape's `oppositeDirectionMateAxis`): the destination turned half
    /// a turn about its X axis (its Z and Y reversed).
    #[serde(default)]
    pub flip_primary: bool,
    /// "Reorient secondary axis" (Onshape's `secondaryAxisType`), applied after the flip.
    #[serde(default)]
    pub secondary: SecondaryAxis,
    /// Rotate: a sketch line or circle, a straight or circular edge, a cylindrical (or other
    /// revolved) face, or a mate connector's Z axis; counter-clockwise seen from the axis's tip.
    #[serde(default)]
    pub axis: Option<AxisRef>,
    #[serde(default)]
    pub angle: f64,
    #[serde(default = "zero_deg")]
    pub angle_expr: String,
    /// Scale uniformly: the point it scales about (a vertex, a sketch point or a mate connector,
    /// usually `ConnectorRef::Implicit(ConnectorOrigin::Vertex(..))`); `None` is the origin.
    #[serde(default)]
    pub scale_point: Option<ConnectorRef>,
    /// The scale factor (greater than zero).
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default = "one_expr")]
    pub scale_expr: String,
    /// P3H.6: context parts to copy (a Part Studio in the context of an assembly), and the
    /// snapshot of the Part Studios they come from (see the module docs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context: Vec<ContextCopy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<ContextSource>,
}

fn default_distance() -> f64 {
    25.0
}

fn default_distance_expr() -> String {
    "25 mm".into()
}

fn zero_mm() -> String {
    "0 mm".into()
}

fn zero_deg() -> String {
    "0 deg".into()
}

fn one() -> f64 {
    1.0
}

fn one_expr() -> String {
    "1".into()
}

impl Default for TransformFeature {
    fn default() -> Self {
        Self {
            transform_type: TransformType::TranslateByLine,
            parts: Vec::new(),
            copy: false,
            line: None,
            direction: None,
            distance: default_distance(),
            distance_expr: default_distance_expr(),
            flip: false,
            dx: 0.0,
            dx_expr: zero_mm(),
            dy: 0.0,
            dy_expr: zero_mm(),
            dz: 0.0,
            dz_expr: zero_mm(),
            from: None,
            to: None,
            flip_primary: false,
            secondary: SecondaryAxis::PlusX,
            axis: None,
            angle: 0.0,
            angle_expr: zero_deg(),
            scale_point: None,
            scale: 1.0,
            scale_expr: one_expr(),
            context: Vec::new(),
            sources: Vec::new(),
        }
    }
}

impl TransformFeature {
    /// A Transform of `t` (the rest at their defaults).
    pub fn new(t: TransformType) -> Self {
        Self { transform_type: t, ..Self::default() }
    }

    /// A Translate by XYZ of `parts` by `(dx, dy, dz)` mm.
    pub fn translate_xyz(parts: Vec<PartId>, d: [f64; 3]) -> Self {
        let mm = |v: f64| format!("{} mm", trim(v));
        Self {
            transform_type: TransformType::TranslateXyz,
            parts,
            dx: d[0],
            dx_expr: mm(d[0]),
            dy: d[1],
            dy_expr: mm(d[1]),
            dz: d[2],
            dz_expr: mm(d[2]),
            ..Self::default()
        }
    }

    /// A Rotate of `parts` about `axis` by `degrees`.
    pub fn rotate(parts: Vec<PartId>, axis: AxisRef, degrees: f64) -> Self {
        Self {
            transform_type: TransformType::Rotate,
            parts,
            axis: Some(axis),
            angle: degrees,
            angle_expr: format!("{} deg", trim(degrees)),
            ..Self::default()
        }
    }

    /// True when it makes copies (Copy part, or Copy in place).
    pub fn copies(&self) -> bool {
        self.copy || self.transform_type == TransformType::CopyInPlace
    }

    /// Why it can't be built, if it can't.
    pub fn problem(&self) -> Option<&'static str> {
        if self.parts.is_empty() && self.context.is_empty() {
            return Some("Select parts to transform");
        }
        if !self.context.is_empty() && !self.copies() {
            return Some("Parts of the assembly context can only be copied: check Copy part");
        }
        match self.transform_type {
            TransformType::TranslateByLine if self.line.is_none() => Some("Select a line to translate along"),
            TransformType::TranslateByLine => match self.line {
                Some(DirectionRef::Edge(_) | DirectionRef::SketchLine { .. }) => None,
                _ => Some("Translate by line needs a straight edge or a sketch line"),
            },
            TransformType::TranslateByDistance if self.direction.is_none() => Some("Select a direction"),
            TransformType::TranslateByDistance if !self.distance.is_finite() => Some("The distance must be a number"),
            TransformType::TranslateXyz if ![self.dx, self.dy, self.dz].iter().all(|v| v.is_finite()) => {
                Some("The distances must be numbers")
            }
            TransformType::MateConnectors if self.from.is_none() => Some("Select the mate connector to move from"),
            TransformType::MateConnectors if self.to.is_none() => Some("Select the mate connector to move to"),
            TransformType::Rotate if self.axis.is_none() => Some("Select an axis of rotation"),
            TransformType::Rotate if !self.angle.is_finite() => Some("The angle must be a number"),
            TransformType::ScaleUniformly if !(self.scale.is_finite() && self.scale > 0.0) => {
                Some("The scale must be greater than zero")
            }
            _ => None,
        }
    }
}

impl TransformFeature {
    /// Every part it takes: its own, then the context parts' ids in the studio's view.
    pub fn picked(&self) -> Vec<PartId> {
        self.parts.iter().copied().chain(self.context.iter().map(|c| PartId::new(c.id, 0))).collect()
    }

    /// Sets the parts it takes from `picked` (its own and context parts) in the Part Studio
    /// `studio` of `doc` (see [`context_copies`]).
    pub fn set_picked(&mut self, doc: &Document, studio: ElementId, picked: &[PartId]) {
        let (own, context, sources) = context_copies(doc, studio, picked);
        self.parts = own;
        self.context = context;
        self.sources = sources;
    }
}

/// A number without trailing zeros ("12.5", "3").
fn trim(v: f64) -> String {
    let s = format!("{v:.6}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A Part Studio whose parts a Transform copies from the context, as it was.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSource {
    pub element: ElementId,
    pub features: Vec<Feature>,
}

/// A context part a Transform copies: which part of which [`ContextSource`], and where the
/// context had it (the studio's coordinates).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextCopy {
    /// The context part's id in the studio's view (`assembly::context::context_id`).
    pub id: FeatureId,
    pub source: usize,
    pub part: PartId,
    pub pose: Pose,
    /// Its instance name in the assembly, for the dialog.
    pub name: String,
    /// Its appearance in its Part Studio (the copy keeps it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<crate::appearance::Appearance>,
}

/// Composite part (see the module docs).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct CompositeFeature {
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// Closed: one part (members not listed or picked on their own).
    #[serde(default)]
    pub closed: bool,
}

impl CompositeFeature {
    pub fn problem(&self) -> Option<&'static str> {
        self.parts.is_empty().then_some("Select parts for the composite part")
    }
}

/// A composite part a rebuild made: its part, its members and whether it is closed.
#[derive(Debug, Clone, PartialEq)]
pub struct Composite {
    pub part: PartId,
    pub members: Vec<PartId>,
    pub closed: bool,
}

/// Splits parts picked in the Part Studio `studio` into its own parts and context parts, the
/// latter with their snapshot (see [`TransformFeature::context`]): the source studios' current
/// features and where the context has the parts.
pub fn context_copies(doc: &Document, studio: ElementId, picked: &[PartId]) -> (Vec<PartId>, Vec<ContextCopy>, Vec<ContextSource>) {
    let ctx = doc.element(studio).and_then(|e| e.context.as_ref());
    let (mut own, mut copies, mut sources): (Vec<PartId>, Vec<ContextCopy>, Vec<ContextSource>) = (Vec::new(), Vec::new(), Vec::new());
    for p in picked {
        if !crate::assembly::context::is_context(p.feature) {
            if !own.contains(p) {
                own.push(*p);
            }
            continue;
        }
        let Some(cp) = ctx.and_then(|c| c.parts.iter().find(|c| c.id == p.feature)) else { continue };
        if copies.iter().any(|c: &ContextCopy| c.id == cp.id) {
            continue;
        }
        let source = match sources.iter().position(|s| s.element == cp.element) {
            Some(i) => i,
            None => {
                let Some(e) = doc.element(cp.element) else { continue };
                sources.push(ContextSource { element: cp.element, features: e.active_features() });
                sources.len() - 1
            }
        };
        let appearance = doc.element(cp.element).and_then(|e| e.part_props().iter().find(|q| q.part == cp.part)).and_then(|q| q.appearance);
        copies.push(ContextCopy { id: cp.id, source, part: cp.part, pose: cp.pose, name: cp.name.clone(), appearance });
    }
    (own, copies, sources)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onshape_names_round_trip() {
        for t in TransformType::ALL {
            assert_eq!(TransformType::from_onshape(t.onshape()), Some(t));
        }
        for a in SecondaryAxis::ALL {
            assert_eq!(SecondaryAxis::from_onshape(a.onshape()), Some(a));
        }
        assert_eq!(SecondaryAxis::MinusY.next(), SecondaryAxis::PlusX);
    }

    #[test]
    fn older_documents_load() {
        // Only the type and the parts: everything else takes its default.
        let x: TransformFeature = ron::from_str("(transform_type: TranslateXyz, parts: [])").unwrap();
        assert_eq!(x.scale, 1.0);
        assert_eq!(x.distance_expr, "25 mm");
        assert!(!x.copies());
        let x = TransformFeature::translate_xyz(vec![], [1.5, 0.0, -2.0]);
        assert_eq!((x.dx_expr.as_str(), x.dz_expr.as_str()), ("1.5 mm", "-2 mm"));
        assert_eq!(x.problem(), Some("Select parts to transform"));
    }
}

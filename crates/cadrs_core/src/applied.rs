//! The applied features of Onshape's "Applied features" lesson (P3.6, PS14–PS16): Fillet,
//! Chamfer, Shell and Hole. They act on the parts already built, through the kernel's
//! `fillet_with`, `chamfer_with` and `shell_with`; a hole is a revolved tool subtracted from the
//! parts (see [`crate::hole`]).

use cadrs_sketch::PointId;
use serde::{Deserialize, Serialize};

use crate::document::{EdgeRef, FaceRef, VertexRef};
use crate::hole::HoleSpec;
use crate::ids::{FeatureId, PartId};

/// An edge or a face picked for a fillet or chamfer ("Entities to fillet"): a face stands for
/// all of its edges (PS14.2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum EdgeOrFace {
    Edge(EdgeRef),
    Face(FaceRef),
}

impl EdgeOrFace {
    pub fn part(&self) -> PartId {
        match self {
            EdgeOrFace::Edge(e) => e.part,
            EdgeOrFace::Face(f) => f.part,
        }
    }

    /// The feature that made the entity (its name's operation), for "Edge of Extrude 1".
    pub fn op(&self) -> cadrs_sketch::OpId {
        match self {
            EdgeOrFace::Edge(e) => e.edge.op(),
            EdgeOrFace::Face(f) => f.face.op,
        }
    }

    /// Every feature the entity depends on (P3.11, PS11.2): both faces' features for an edge.
    pub fn ops(&self) -> Vec<cadrs_sketch::OpId> {
        match self {
            EdgeOrFace::Edge(e) => {
                let [a, b] = e.edge.faces;
                if a.op == b.op { vec![a.op] } else { vec![a.op, b.op] }
            }
            EdgeOrFace::Face(f) => vec![f.face.op],
        }
    }
}

/// A fillet's Measurement (PS14.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilletMeasurement {
    #[default]
    Radius,
    /// The chord between the two contact lines, kept constant along the edge.
    Width,
}

impl FilletMeasurement {
    pub const ALL: [FilletMeasurement; 2] = [FilletMeasurement::Radius, FilletMeasurement::Width];

    pub fn label(self) -> &'static str {
        match self {
            FilletMeasurement::Radius => "Radius",
            FilletMeasurement::Width => "Width",
        }
    }
}

/// A fillet's cross section (PS14.4): Distance (circular), Conic (with Rho) or Curvature (G2,
/// with a Magnitude). Conic and Curvature are built on straight edges between flat faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilletControl {
    #[default]
    Distance,
    Conic,
    Curvature,
}

impl FilletControl {
    pub const ALL: [FilletControl; 3] = [FilletControl::Distance, FilletControl::Conic, FilletControl::Curvature];

    pub fn label(self) -> &'static str {
        match self {
            FilletControl::Distance => "Distance",
            FilletControl::Conic => "Conic",
            FilletControl::Curvature => "Curvature",
        }
    }
}

/// The Fillet dialog's tabs (PS14.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum FilletType {
    #[default]
    Edge,
    /// A round replacing a face, tangent to the faces on either side of it.
    FullRound,
}

impl FilletType {
    pub const ALL: [FilletType; 2] = [FilletType::Edge, FilletType::FullRound];

    pub fn label(self) -> &'static str {
        match self {
            FilletType::Edge => "Edge",
            FilletType::FullRound => "Full round",
        }
    }
}

/// The Fillet feature: the Edge tab (PS14.1–14.5) and the Full round tab (PS14.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FilletFeature {
    #[serde(default)]
    pub kind: FilletType,
    /// Entities to fillet, in the order picked.
    pub entities: Vec<EdgeOrFace>,
    /// Full round: the side faces and the face the round replaces.
    #[serde(default)]
    pub side1: Vec<FaceRef>,
    #[serde(default)]
    pub center: Vec<FaceRef>,
    #[serde(default)]
    pub side2: Vec<FaceRef>,
    /// A conic section's Rho (0 < rho < 1).
    #[serde(default = "half")]
    pub rho: f64,
    /// A curvature section's Magnitude (0 < m ≤ 1).
    #[serde(default = "half")]
    pub magnitude: f64,
    #[serde(default)]
    pub measurement: FilletMeasurement,
    #[serde(default)]
    pub control: FilletControl,
    /// The radius or width (mm).
    pub size: f64,
    pub size_expr: String,
    /// Tangent propagation (on by default): the picked edges' tangent chains are filleted too.
    /// OpenCascade always continues a fillet along tangent edges, so with it off a pick whose
    /// chain has unpicked edges fails with that reason.
    #[serde(default = "yes")]
    pub tangent_propagation: bool,
    /// Allow edge overflow (on by default): off refuses a fillet that runs onto a face other
    /// than those meeting at its edge's ends.
    #[serde(default = "yes")]
    pub allow_overflow: bool,
    /// (P3.10, PS14.6) Asymmetric: a second radius on the edges' other face (`flip_asymmetric`
    /// swaps the faces). Straight edges between flat faces (the kernel's conic sections).
    #[serde(default)]
    pub asymmetric: bool,
    #[serde(default = "five")]
    pub second: f64,
    #[serde(default = "five_mm")]
    pub second_expr: String,
    #[serde(default)]
    pub flip_asymmetric: bool,
    /// (P3.10, PS14.6) Variable fillet: radii at picked vertices and at points along edges; the
    /// fillet's radius elsewhere follows them (`size` where nothing is set). Smooth transition
    /// blends the radii smoothly; off, the radius changes linearly between them.
    #[serde(default)]
    pub variable: bool,
    #[serde(default)]
    pub vertices: Vec<VertexRadius>,
    #[serde(default)]
    pub edge_points: Vec<EdgePoint>,
    #[serde(default = "yes")]
    pub smooth_transition: bool,
    /// (P3.11, PS14.6) Partial fillet: only the part of the (one) edge between the First and
    /// Second bound, measured from the edge's start (its end with `flip_partial`) as a fraction
    /// of its length (Parameter) or a distance along it (Length). The fillet stops at a flat end
    /// face square to the edge at each bound.
    #[serde(default)]
    pub partial: bool,
    #[serde(default)]
    pub partial_bound: PartialBound,
    #[serde(default = "quarter")]
    pub partial_first: f64,
    #[serde(default = "quarter_text")]
    pub partial_first_expr: String,
    #[serde(default = "three_quarters")]
    pub partial_second: f64,
    #[serde(default = "three_quarters_text")]
    pub partial_second_expr: String,
    #[serde(default)]
    pub flip_partial: bool,
    /// (Final, PS14.6) Smooth fillet corners: where three or more of the fillet's edges meet,
    /// the corner is set back [`SMOOTH_SETBACK`] × the radius along the fillets and blended
    /// with one patch tangent to every face round it (kernel `fillet_smooth`).
    /// Left out when off, so documents (and their history hashes) from before stay the same.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub smooth_corners: bool,
}

/// How far a smooth fillet corner is set back from its vertex along the fillets' contact
/// lines, in fillet radii (Final, PS14.6).
pub const SMOOTH_SETBACK: f64 = 1.5;

/// How a partial fillet's bounds are given (P3.11, PS14.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PartialBound {
    /// A fraction of the edge's length, 0 to 1.
    #[default]
    Parameter,
    /// A distance along the edge (mm).
    Length,
}

impl PartialBound {
    pub const ALL: [PartialBound; 2] = [PartialBound::Parameter, PartialBound::Length];

    pub fn label(self) -> &'static str {
        match self {
            PartialBound::Parameter => "Parameter",
            PartialBound::Length => "Length",
        }
    }
}

/// A variable fillet's radius at a vertex (P3.10, PS14.6).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VertexRadius {
    pub vertex: VertexRef,
    pub radius: f64,
    pub expr: String,
}

/// A variable fillet's point on an edge: its location (0–1 along the edge from its start) and
/// radius (P3.10, PS14.6 "Points on edge").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EdgePoint {
    pub edge: EdgeRef,
    pub location: f64,
    pub radius: f64,
    pub expr: String,
}

fn five() -> f64 {
    5.0
}

fn five_mm() -> String {
    "5 mm".into()
}

fn yes() -> bool {
    true
}

fn half() -> f64 {
    0.5
}

fn quarter() -> f64 {
    0.25
}

fn quarter_text() -> String {
    "0.25".into()
}

fn three_quarters() -> f64 {
    0.75
}

fn three_quarters_text() -> String {
    "0.75".into()
}

impl Default for FilletFeature {
    fn default() -> Self {
        Self {
            kind: FilletType::Edge,
            entities: Vec::new(),
            side1: Vec::new(),
            center: Vec::new(),
            side2: Vec::new(),
            rho: 0.5,
            magnitude: 0.5,
            measurement: FilletMeasurement::Radius,
            control: FilletControl::Distance,
            size: 5.0,
            size_expr: "5 mm".into(),
            tangent_propagation: true,
            allow_overflow: true,
            asymmetric: false,
            second: 5.0,
            second_expr: "5 mm".into(),
            flip_asymmetric: false,
            variable: false,
            vertices: Vec::new(),
            edge_points: Vec::new(),
            smooth_transition: true,
            partial: false,
            partial_bound: PartialBound::Parameter,
            partial_first: 0.25,
            partial_first_expr: quarter_text(),
            partial_second: 0.75,
            partial_second_expr: three_quarters_text(),
            flip_partial: false,
            smooth_corners: false,
        }
    }
}

impl FilletFeature {
    /// A partial fillet's bounds as fractions of an edge of `length`, from its start, lower
    /// first; an error if they don't lie on it.
    pub fn partial_range(&self, length: f64) -> Result<(f64, f64), String> {
        let frac = |v: f64| match self.partial_bound {
            PartialBound::Parameter => v,
            PartialBound::Length => v / length.max(1e-12),
        };
        let (mut a, mut b) = (frac(self.partial_first), frac(self.partial_second));
        if self.flip_partial {
            (a, b) = (1.0 - a, 1.0 - b);
        }
        let (lo, hi) = (a.min(b), a.max(b));
        if lo < -1e-9 || hi > 1.0 + 1e-9 {
            return Err(format!("A partial fillet's bounds must lie on the edge (it is {length:.3} mm long)"));
        }
        Ok((lo.max(0.0), hi.min(1.0)))
    }

    /// Switches how the bounds are given, keeping where they are on an edge of `length` (if
    /// known).
    pub fn set_partial_bound(&mut self, bound: PartialBound, length: Option<f64>) {
        if bound == self.partial_bound {
            return;
        }
        let conv = |v: f64| match (bound, length) {
            (PartialBound::Length, Some(l)) => (v * l * 1000.0).round() / 1000.0,
            (PartialBound::Parameter, Some(l)) if l > 0.0 => ((v / l) * 1e4).round() / 1e4,
            (PartialBound::Length, None) => v * 20.0,
            (PartialBound::Parameter, _) => (v / 20.0).clamp(0.0, 1.0),
        };
        self.partial_first = conv(self.partial_first);
        self.partial_second = conv(self.partial_second);
        let text = |v: f64| {
            let s = format!("{v:.4}");
            let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
            if bound == PartialBound::Length { format!("{s} mm") } else { s }
        };
        self.partial_first_expr = text(self.partial_first);
        self.partial_second_expr = text(self.partial_second);
        self.partial_bound = bound;
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.kind == FilletType::FullRound {
            if self.side1.is_empty() || self.center.is_empty() || self.side2.is_empty() {
                return Some("Select the side faces and the center face");
            }
            if self.side1.len() > 1 || self.center.len() > 1 || self.side2.len() > 1 {
                return Some("A full round takes one face for each side and one for the center");
            }
            return None;
        }
        if self.entities.is_empty() {
            return Some("Select edges or faces to fillet");
        }
        if !(self.size > 0.0 && self.size.is_finite()) {
            return Some(match self.measurement {
                FilletMeasurement::Radius => "The radius must be greater than zero",
                FilletMeasurement::Width => "The width must be greater than zero",
            });
        }
        if self.asymmetric && !(self.second > 0.0 && self.second.is_finite()) {
            return Some("The second radius must be greater than zero");
        }
        let bad = |r: f64| r.is_nan() || r <= 0.0;
        if self.variable && (self.vertices.iter().any(|v| bad(v.radius)) || self.edge_points.iter().any(|p| bad(p.radius))) {
            return Some("Every radius of a variable fillet must be greater than zero");
        }
        if self.partial {
            if self.variable {
                return Some("A partial fillet can't be a variable fillet too");
            }
            if !self.entities.iter().all(|e| matches!(e, EdgeOrFace::Edge(_))) || self.entities.len() != 1 {
                return Some("A partial fillet takes one edge");
            }
            let ok = |v: f64| match self.partial_bound {
                PartialBound::Parameter => (0.0..=1.0).contains(&v),
                PartialBound::Length => v.is_finite() && v >= 0.0,
            };
            if !ok(self.partial_first) || !ok(self.partial_second) {
                return Some(match self.partial_bound {
                    PartialBound::Parameter => "A partial fillet's bounds must be between 0 and 1",
                    PartialBound::Length => "A partial fillet's bounds can't be negative",
                });
            }
            if (self.partial_first - self.partial_second).abs() < 1e-9 {
                return Some("A partial fillet's bounds must differ");
            }
        }
        if self.variable && self.edge_points.iter().any(|p| !(0.0..=1.0).contains(&p.location)) {
            return Some("A point's location must be between 0 and 1");
        }
        match self.control {
            FilletControl::Conic if !(self.rho > 0.0 && self.rho < 1.0) => Some("Rho must be between 0 and 1"),
            FilletControl::Curvature if !(self.magnitude > 0.0 && self.magnitude <= 1.0) => {
                Some("The magnitude must be between 0 and 1")
            }
            _ => None,
        }
    }
}

/// A chamfer's Measurement (PS14.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChamferMeasurement {
    #[default]
    Offset,
    Tangent,
}

impl ChamferMeasurement {
    pub const ALL: [ChamferMeasurement; 2] = [ChamferMeasurement::Offset, ChamferMeasurement::Tangent];

    pub fn label(self) -> &'static str {
        match self {
            ChamferMeasurement::Offset => "Offset",
            ChamferMeasurement::Tangent => "Tangent",
        }
    }
}

/// The chamfer type (PS14.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChamferType {
    #[default]
    EqualDistance,
    TwoDistances,
    DistanceAngle,
}

impl ChamferType {
    pub const ALL: [ChamferType; 3] = [ChamferType::EqualDistance, ChamferType::TwoDistances, ChamferType::DistanceAngle];

    pub fn label(self) -> &'static str {
        match self {
            ChamferType::EqualDistance => "Equal distance",
            ChamferType::TwoDistances => "Two distances",
            ChamferType::DistanceAngle => "Distance and angle",
        }
    }
}

/// The Chamfer feature (PS14.7–14.9).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChamferFeature {
    pub entities: Vec<EdgeOrFace>,
    #[serde(default)]
    pub measurement: ChamferMeasurement,
    #[serde(default)]
    pub kind: ChamferType,
    /// Distance (mm; Distance 1 for two distances).
    pub distance: f64,
    pub distance_expr: String,
    /// Distance 2 (mm).
    pub distance2: f64,
    pub distance2_expr: String,
    /// The angle (degrees) of Distance and angle.
    pub angle: f64,
    pub angle_expr: String,
    /// The opposite direction for all edges (which face takes Distance 1 or the angle).
    #[serde(default)]
    pub flip: bool,
    /// Direction overrides: edges flipped on their own.
    #[serde(default)]
    pub overrides: Vec<EdgeRef>,
    #[serde(default = "yes")]
    pub tangent_propagation: bool,
}

impl Default for ChamferFeature {
    fn default() -> Self {
        Self {
            entities: Vec::new(),
            measurement: ChamferMeasurement::Offset,
            kind: ChamferType::EqualDistance,
            distance: 5.0,
            distance_expr: "5 mm".into(),
            distance2: 5.0,
            distance2_expr: "5 mm".into(),
            angle: 45.0,
            angle_expr: "45 deg".into(),
            flip: false,
            overrides: Vec::new(),
            tangent_propagation: true,
        }
    }
}

impl ChamferFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.entities.is_empty() {
            return Some("Select edges or faces to chamfer");
        }
        if !(self.distance > 0.0 && self.distance.is_finite()) {
            return Some("The distance must be greater than zero");
        }
        if self.kind == ChamferType::TwoDistances && !(self.distance2 > 0.0 && self.distance2.is_finite()) {
            return Some("The distance must be greater than zero");
        }
        if self.kind == ChamferType::DistanceAngle && !(self.angle > 0.0 && self.angle < 90.0) {
            return Some("The angle must be between 0 and 90°");
        }
        None
    }
}

/// The Shell feature (PS16).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellFeature {
    /// Faces to remove (the openings).
    pub faces: Vec<FaceRef>,
    /// Hollow: a closed hollow part, picked as parts instead of faces (PS16.3).
    #[serde(default)]
    pub hollow: bool,
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// Shell thickness (mm).
    pub thickness: f64,
    pub thickness_expr: String,
    /// The opposite direction: the walls grow outward (PS16.2).
    #[serde(default)]
    pub outward: bool,
}

impl Default for ShellFeature {
    fn default() -> Self {
        Self {
            faces: Vec::new(),
            hollow: false,
            parts: Vec::new(),
            thickness: 2.0,
            thickness_expr: "2 mm".into(),
            outward: false,
        }
    }
}

impl ShellFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.hollow && self.parts.is_empty() {
            return Some("Select the parts to hollow");
        }
        if !self.hollow && self.faces.is_empty() {
            return Some("Select the faces to remove");
        }
        if !(self.thickness > 0.0 && self.thickness.is_finite()) {
            return Some("The thickness must be greater than zero");
        }
        None
    }
}

/// A point a hole is placed at (PS15.1): a sketch point (a standalone point, a line's or arc's
/// end, or a circle's or arc's centre).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HolePoint {
    pub sketch: FeatureId,
    pub point: PointId,
}

/// The Hole feature (PS15.1, 15.3–15.7, 15.10).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HoleFeature {
    /// Sketch points to place holes, picked one by one...
    pub points: Vec<HolePoint>,
    /// ...and whole sketches (every non-construction vertex in them)...
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// ...and mate connectors (P3.8, PS15.2): a hole at each, drilled along its −Z axis.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connectors: Vec<crate::mate::ConnectorRef>,
    /// Merge scope: the parts to cut (empty: every part a hole reaches).
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    /// The hole's shape and size.
    pub spec: HoleSpec,
    /// Drill against the hole direction (the opposite of into the sketch plane's back).
    #[serde(default)]
    pub flip: bool,
    /// (P3.10, PS15.6) Start from selected plane: the Hole start plane (a plane, a flat face or
    /// a mate connector's XY plane).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start_plane: Option<crate::pattern::MirrorPlane>,
    /// (P3.10, PS15.7) Up to entity: the face or plane the full diameter reaches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_to: Option<crate::pattern::MirrorPlane>,
    /// The user renamed the feature: its name no longer follows the callout.
    #[serde(default)]
    pub renamed: bool,
}

impl HoleFeature {
    pub fn problem(&self) -> Option<&'static str> {
        if self.points.is_empty() && self.sketches.is_empty() && self.connectors.is_empty() {
            return Some("Select sketch points to place holes");
        }
        if self.spec.start == crate::hole::HoleStart::SelectedPlane && self.start_plane.is_none() {
            return Some("Select a hole start plane");
        }
        if self.spec.end == crate::hole::HoleEnd::UpToEntity && self.up_to.is_none() {
            return Some("Select a face or plane to go up to");
        }
        if self.spec.hole_type == crate::hole::HoleType::Tapped && self.spec.tap_type == crate::hole::TapType::Tapered {
            return Some("Tapered taps aren't available; use Straight tap");
        }
        self.spec.problem()
    }

    /// The sketches it takes points of.
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.points.iter().map(|p| p.sketch).collect();
        v.extend(self.sketches.iter().copied());
        v.sort();
        v.dedup();
        v
    }
}

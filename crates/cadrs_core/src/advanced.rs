//! The advanced features of P3.7: **Sweep** (PS19), **Loft** (PS20) and **Split** (PS18.5).
//! Their rebuild is in `rebuild/kernel_ops/advanced.rs`.

use cadrs_sketch::{CurveId, PlaneRef, PointId};
use serde::{Deserialize, Serialize};

use crate::document::{BodyType, BooleanOp, DirectionRef, EdgeRef, FaceRef, RegionRef, ThinWall, VertexRef};
use crate::ids::{FeatureId, PartId};

/// One piece of a sweep path (PS19.2): a part edge, a sketch curve, or every curve of a sketch.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum PathRef {
    Edge(EdgeRef),
    SketchCurve { sketch: FeatureId, curve: CurveId },
    Sketch(FeatureId),
    /// A curve feature's curve (a Helix, `crate::surfacing`).
    Curve(FeatureId),
}

/// How the profile follows the path (the Sweep dialog's profile control, PS19.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ProfileControl {
    /// The profile keeps its angle to the path.
    #[default]
    None,
    /// The profile keeps its orientation in space.
    KeepOrientation,
    /// The profile's plane keeps containing a direction.
    LockDirection,
}

impl ProfileControl {
    pub const ALL: [ProfileControl; 3] = [ProfileControl::None, ProfileControl::KeepOrientation, ProfileControl::LockDirection];

    pub fn label(self) -> &'static str {
        match self {
            ProfileControl::None => "None",
            ProfileControl::KeepOrientation => "Keep profile orientation",
            ProfileControl::LockDirection => "Lock profile direction",
        }
    }
}

/// A Sweep (P3.7, PS19; the dialog in `ex4-step15.png`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct SweepFeature {
    /// "Faces and sketch regions to sweep": regions...
    pub regions: Vec<RegionRef>,
    /// ...whole sketches (for surfaces and thin walls, their curves)...
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// ...and planar part faces.
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// The "Sweep path".
    pub path: Vec<PathRef>,
    #[serde(default)]
    pub body: BodyType,
    #[serde(default)]
    pub thin: ThinWall,
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    #[serde(default)]
    pub control: ProfileControl,
    /// The direction Lock profile direction keeps.
    #[serde(default)]
    pub lock_direction: Option<DirectionRef>,
}

impl SweepFeature {
    /// The sketches it sweeps regions of (or whole) and takes path curves from.
    pub fn sketches(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = self.regions.iter().map(|r| r.sketch).collect();
        for s in self.sketches.iter().copied().chain(self.path.iter().filter_map(|p| match p {
            PathRef::SketchCurve { sketch, .. } | PathRef::Sketch(sketch) => Some(*sketch),
            PathRef::Edge(_) | PathRef::Curve(_) => None,
        })) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
        v
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.regions.is_empty() && self.sketches.is_empty() && self.faces.is_empty() {
            return Some("Select a profile to sweep");
        }
        if self.path.is_empty() {
            return Some("Select a sweep path");
        }
        if self.control == ProfileControl::LockDirection && self.lock_direction.is_none() {
            return Some("Select a direction to lock");
        }
        None
    }
}

/// One profile of a loft, in order (PS20.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LoftProfile {
    /// Regions of one sketch (joined into one contour: "Faces of Sketch 2").
    Regions { sketch: FeatureId, regions: Vec<RegionRef> },
    /// A whole sketch.
    Sketch(FeatureId),
    /// A planar part face.
    Face(FaceRef),
    /// A point (first or last only): a sketch point...
    SketchPoint { sketch: FeatureId, point: PointId },
    /// ...or a vertex.
    Vertex(VertexRef),
}

impl LoftProfile {
    /// The sketch it comes from, if any.
    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            LoftProfile::Regions { sketch, .. } | LoftProfile::Sketch(sketch) | LoftProfile::SketchPoint { sketch, .. } => {
                Some(*sketch)
            }
            _ => None,
        }
    }
}

/// A loft end's condition (PS20.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LoftCondition {
    #[default]
    None,
    NormalToProfile,
    TangentToProfile,
    /// Continue the faces next to a face profile (P3.10): tangent (G1) or with their curvature
    /// too (G2, two profiles). Only for a face as the first or last profile.
    MatchTangent,
    MatchCurvature,
    /// P3.11 (PS20.4): as Normal to profile, along a picked direction (the end's Direction
    /// field: an edge, a sketch line, a face's or a plane's normal, a mate connector's Z).
    NormalDirection,
    /// P3.11: as Tangent to profile, in the plane normal to a picked direction.
    TangentDirection,
}

impl LoftCondition {
    pub const ALL: [LoftCondition; 7] = [
        LoftCondition::None,
        LoftCondition::NormalToProfile,
        LoftCondition::TangentToProfile,
        LoftCondition::MatchTangent,
        LoftCondition::MatchCurvature,
        LoftCondition::NormalDirection,
        LoftCondition::TangentDirection,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LoftCondition::None => "None",
            LoftCondition::NormalToProfile => "Normal to profile",
            LoftCondition::TangentToProfile => "Tangent to profile",
            LoftCondition::MatchTangent => "Match tangent",
            LoftCondition::MatchCurvature => "Match curvature",
            LoftCondition::NormalDirection => "Normal direction",
            LoftCondition::TangentDirection => "Tangent direction",
        }
    }

    /// Conditions that take a picked direction.
    pub fn takes_direction(self) -> bool {
        matches!(self, LoftCondition::NormalDirection | LoftCondition::TangentDirection)
    }

    /// Conditions cadrs builds (all of them since P3.10; the Match ones need a face profile at
    /// that end, which the rebuild checks).
    pub fn available(self) -> bool {
        true
    }

    /// Conditions with a magnitude.
    pub fn has_magnitude(self) -> bool {
        self != LoftCondition::None
    }
}

/// A Loft (P3.7, PS20; the dialog in `ex4-step9.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LoftFeature {
    pub profiles: Vec<LoftProfile>,
    #[serde(default)]
    pub body: BodyType,
    #[serde(default)]
    pub thin: ThinWall,
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    #[serde(default)]
    pub start: LoftCondition,
    pub start_magnitude: f64,
    pub start_magnitude_expr: String,
    #[serde(default)]
    pub end: LoftCondition,
    pub end_magnitude: f64,
    pub end_magnitude_expr: String,
    /// P3.11: the Normal direction / Tangent direction conditions' directions.
    #[serde(default)]
    pub start_direction: Option<DirectionRef>,
    #[serde(default)]
    pub end_direction: Option<DirectionRef>,
}

impl Default for LoftFeature {
    fn default() -> Self {
        Self {
            profiles: Vec::new(),
            body: BodyType::Solid,
            thin: ThinWall::default(),
            op: BooleanOp::New,
            merge_all: false,
            merge_scope: Vec::new(),
            start: LoftCondition::None,
            start_magnitude: 1.0,
            start_magnitude_expr: "1".into(),
            end: LoftCondition::None,
            end_magnitude: 1.0,
            end_magnitude_expr: "1".into(),
            start_direction: None,
            end_direction: None,
        }
    }
}

impl LoftFeature {
    pub fn sketches(&self) -> Vec<FeatureId> {
        let mut v = Vec::new();
        for s in self.profiles.iter().filter_map(LoftProfile::sketch) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
        v
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.profiles.len() < 2 {
            return Some("Select at least two profiles");
        }
        let n = self.profiles.len();
        let point = |p: &LoftProfile| matches!(p, LoftProfile::SketchPoint { .. } | LoftProfile::Vertex(_));
        if self.profiles.iter().enumerate().any(|(i, p)| point(p) && i != 0 && i + 1 != n) {
            return Some("A point can only be the first or last profile");
        }
        for (c, m) in [(self.start, self.start_magnitude), (self.end, self.end_magnitude)] {
            if !c.available() {
                return Some("Match tangent and Match curvature need a face profile's neighbours (not available yet)");
            }
            if c.has_magnitude() && !(m > 0.0 && m.is_finite()) {
                return Some("The magnitude must be greater than zero");
            }
        }
        if (self.start.takes_direction() && self.start_direction.is_none()) || (self.end.takes_direction() && self.end_direction.is_none()) {
            return Some("Select a direction for the profile condition");
        }
        None
    }
}

/// What splits the parts (PS18.5).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SplitToolRef {
    /// A default plane or a Plane feature.
    Plane(PlaneRef),
    /// A part face: a planar face splits along its whole plane, another face by itself.
    Face(FaceRef),
    /// A sketch: its curves extruded both ways through the parts.
    Sketch(FeatureId),
}

/// What a Split cuts (P3.8, as Onshape's Split: "Part" and "Face").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SplitType {
    /// Parts cut in pieces, each piece a part.
    #[default]
    Part,
    /// Faces cut where the tool crosses them; the part stays one part, with more faces.
    Face,
}

impl SplitType {
    pub const ALL: [SplitType; 2] = [SplitType::Part, SplitType::Face];

    pub fn label(self) -> &'static str {
        match self {
            SplitType::Part => "Part",
            SplitType::Face => "Face",
        }
    }
}

fn yes() -> bool {
    true
}

/// A Split (P3.7, PS18.5): parts cut in pieces by a plane, a face or a sketch. The largest
/// piece of each part keeps it; the others are new parts. P3.8 adds Onshape's other options: the
/// Face type (faces split, the part kept whole), Keep tools (a surface split with stays),
/// Trim to face boundaries (a planar face splits only within its edges, not along its whole
/// plane) and Keep both sides (off: only the pieces on the tool's front, or with the flip its
/// back, stay).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SplitFeature {
    pub parts: Vec<PartId>,
    pub tool: Option<SplitToolRef>,
    #[serde(default)]
    pub split_type: SplitType,
    /// The faces to split (the Face type).
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    #[serde(default)]
    pub keep_tools: bool,
    #[serde(default)]
    pub trim: bool,
    #[serde(default = "yes")]
    pub keep_both: bool,
    /// Keep the pieces behind the tool instead (Keep both sides off).
    #[serde(default)]
    pub flip: bool,
}

impl Default for SplitFeature {
    fn default() -> Self {
        Self {
            parts: Vec::new(),
            tool: None,
            split_type: SplitType::Part,
            faces: Vec::new(),
            keep_tools: false,
            trim: false,
            keep_both: true,
            flip: false,
        }
    }
}

impl SplitFeature {
    pub fn problem(&self) -> Option<&'static str> {
        match self.split_type {
            SplitType::Part if self.parts.is_empty() => return Some("Select parts to split"),
            SplitType::Face if self.faces.is_empty() => return Some("Select faces to split"),
            _ => {}
        }
        if self.tool.is_none() {
            return Some("Select a plane, face or sketch to split with");
        }
        None
    }
}

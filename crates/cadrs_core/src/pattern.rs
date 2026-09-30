//! Patterns and Mirror (P3.8, PS22–PS26): the **Linear**, **Circular** and **Curve** patterns
//! and **Mirror**, each of **parts**, **features** or **faces** (Onshape's Pattern type and
//! Mirror type). Their rebuild is in `rebuild/kernel_ops/pattern.rs`.
//!
//! - **Part pattern / mirror**: copies of the parts, New (separate parts that keep the seed's
//!   appearance and material, PS9.6) or Add/Remove/Intersect with the merge scope, as an
//!   extrude's result.
//! - **Feature pattern / mirror**: the features' effect copied. With **Reapply features**
//!   (PS22.3, rigid patterns only) each instance is regenerated with its sketches moved, so
//!   "Up to" ends stop at their own targets (PS27.5); without it, the material the features
//!   removed and added is copied.
//! - **Face pattern / mirror**: the pocket or boss the faces bound (their free edges capped,
//!   `Kernel::face_tool`) copied and cut or added.
//! - Instances are numbered on a grid `(i, j)`: `i` along the first direction (or round the
//!   axis, or along the path), `j` along the second; `(0, 0)` is the seed. **Skip instances**
//!   (PS22.5) lists the ones left out.

use cadrs_sketch::Vec3;
use serde::{Deserialize, Serialize};

use crate::advanced::PathRef;
use crate::document::{AxisRef, BooleanOp, DirectionRef, FaceRef};
use crate::ids::{FeatureId, PartId};
use crate::mate::ConnectorRef;

/// Which pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatternKind {
    #[default]
    Linear,
    Circular,
    Curve,
}

impl PatternKind {
    pub fn label(self) -> &'static str {
        match self {
            PatternKind::Linear => "Linear pattern",
            PatternKind::Circular => "Circular pattern",
            PatternKind::Curve => "Curve pattern",
        }
    }
}

/// What a pattern or mirror copies (PS22.2, PS26.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatternType {
    #[default]
    Part,
    Feature,
    Face,
}

impl PatternType {
    pub const ALL: [PatternType; 3] = [PatternType::Part, PatternType::Feature, PatternType::Face];

    pub fn label(self) -> &'static str {
        match self {
            PatternType::Part => "Part pattern",
            PatternType::Feature => "Feature pattern",
            PatternType::Face => "Face pattern",
        }
    }

    pub fn mirror_label(self) -> &'static str {
        match self {
            PatternType::Part => "Part mirror",
            PatternType::Feature => "Feature mirror",
            PatternType::Face => "Face mirror",
        }
    }
}

/// One direction of a linear pattern: where, how far apart and how many (with the seed).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearDirection {
    /// An edge, a sketch line, a face's or plane's normal, or a mate connector's Z axis
    /// (PS23.1).
    pub direction: Option<DirectionRef>,
    /// mm between instances.
    pub distance: f64,
    pub distance_expr: String,
    pub count: u32,
    /// The count as typed when it names a variable (P3F.4, `#bolts`); empty for a plain count.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub count_expr: String,
    #[serde(default)]
    pub flip: bool,
    /// Spread about the seed (PS23.2).
    #[serde(default)]
    pub centered: bool,
}

impl Default for LinearDirection {
    fn default() -> Self {
        Self { direction: None, distance: 25.0, distance_expr: "25 mm".into(), count: 2, count_expr: String::new(), flip: false, centered: false }
    }
}

/// A Linear, Circular or Curve pattern (P3.8; the dialogs in `ex5-step5.png`, `ex5-step9.png`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternFeature {
    pub kind: PatternKind,
    #[serde(default)]
    pub pattern_type: PatternType,
    /// "Entities to pattern": parts...
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// ...features...
    #[serde(default)]
    pub features: Vec<FeatureId>,
    /// ...or faces.
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// Linear: the first direction.
    #[serde(default)]
    pub first: LinearDirection,
    /// Linear: the second direction (PS23.3), when on.
    #[serde(default)]
    pub second_on: bool,
    #[serde(default)]
    pub second: LinearDirection,
    /// Circular: the axis (a circular edge, a cylindrical face, a sketch circle or line, or a
    /// mate connector's Z axis, PS24.1), the angle (degrees); the count, flip and Centered are
    /// `first`'s.
    #[serde(default)]
    pub axis: Option<AxisRef>,
    #[serde(default = "full_turn")]
    pub angle: f64,
    #[serde(default = "full_turn_expr")]
    pub angle_expr: String,
    /// Curve: the path (PS25.1) and the orientation (Tangent to curve, PS25.3).
    #[serde(default)]
    pub path: Vec<PathRef>,
    #[serde(default = "yes")]
    pub tangent_to_curve: bool,
    /// Circular and Curve: the instances fill the angle or the path evenly (PS24.2, PS25.2).
    #[serde(default = "yes")]
    pub equal_spacing: bool,
    /// Feature pattern: regenerate each instance (PS22.3).
    #[serde(default)]
    pub reapply: bool,
    /// Skip instances (PS22.5): on, and the instances left out, by grid index.
    #[serde(default)]
    pub skip_on: bool,
    #[serde(default)]
    pub skipped: Vec<[u32; 2]>,
    /// Part pattern: New, Add, Remove, Intersect, and the merge scope (PS22.6).
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
}

fn full_turn() -> f64 {
    360.0
}

fn full_turn_expr() -> String {
    "360 deg".into()
}

fn yes() -> bool {
    true
}

impl Default for PatternFeature {
    fn default() -> Self {
        Self {
            kind: PatternKind::Linear,
            pattern_type: PatternType::Part,
            parts: Vec::new(),
            features: Vec::new(),
            faces: Vec::new(),
            first: LinearDirection::default(),
            second_on: false,
            second: LinearDirection::default(),
            axis: None,
            angle: 360.0,
            angle_expr: full_turn_expr(),
            path: Vec::new(),
            tangent_to_curve: true,
            equal_spacing: true,
            reapply: false,
            skip_on: false,
            skipped: Vec::new(),
            op: BooleanOp::New,
            merge_all: false,
            merge_scope: Vec::new(),
        }
    }
}

impl PatternFeature {
    /// A new pattern of `kind` (Onshape's defaults: 2 instances 25 mm apart; 4 round 360°).
    pub fn new(kind: PatternKind) -> Self {
        let mut p = Self { kind, ..Self::default() };
        if kind == PatternKind::Circular {
            p.first.count = 4;
        }
        if kind == PatternKind::Curve {
            p.first.count = 4;
        }
        p
    }

    /// Nothing to pattern yet.
    pub fn seeds_empty(&self) -> bool {
        match self.pattern_type {
            PatternType::Part => self.parts.is_empty(),
            PatternType::Feature => self.features.is_empty(),
            PatternType::Face => self.faces.is_empty(),
        }
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.seeds_empty() {
            return Some(match self.pattern_type {
                PatternType::Part => "Select parts to pattern",
                PatternType::Feature => "Select features to pattern",
                PatternType::Face => "Select faces to pattern",
            });
        }
        match self.kind {
            PatternKind::Linear => {
                if self.first.direction.is_none() {
                    return Some("Select a direction");
                }
                if self.second_on && self.second.direction.is_none() {
                    return Some("Select a second direction");
                }
            }
            PatternKind::Circular => {
                if self.axis.is_none() {
                    return Some("Select an axis of pattern");
                }
            }
            PatternKind::Curve => {
                if self.path.is_empty() {
                    return Some("Select a path to pattern along");
                }
            }
        }
        if self.first.count < 1 || (self.second_on && self.second.count < 1) {
            return Some("The instance count must be at least 1");
        }
        None
    }

    /// Whether instance `(i, j)` is left out.
    pub fn is_skipped(&self, i: u32, j: u32) -> bool {
        self.skip_on && self.skipped.contains(&[i, j])
    }

    /// The instances' grid indices and their offsets along the first and second directions (in
    /// units of the distance or angle step), the seed `(0, 0)` first. Centered spreads them
    /// about the seed: `i` still counts from the first instance, so the seed is in the middle.
    pub fn grid(&self) -> Vec<([u32; 2], [f64; 2])> {
        // Centered: the seed in the middle (with an even count, one more on the far side).
        let axis_offsets = |d: &LinearDirection| -> Vec<f64> {
            let n = d.count.max(1);
            let shift = if d.centered { (n - 1) / 2 } else { 0 };
            (0..n).map(|i| f64::from(i) - f64::from(shift)).collect()
        };
        let a = axis_offsets(&self.first);
        let b = if self.kind == PatternKind::Linear && self.second_on { axis_offsets(&self.second) } else { vec![0.0] };
        let mut seed = Vec::new();
        let mut out = Vec::new();
        for (j, y) in b.iter().enumerate() {
            for (i, x) in a.iter().enumerate() {
                let item = ([i as u32, j as u32], [*x, *y]);
                if *x == 0.0 && *y == 0.0 { seed.push(item) } else { out.push(item) }
            }
        }
        seed.extend(out);
        seed
    }

    /// The step of a circular pattern (degrees): with Equal spacing the angle is shared out
    /// (a full turn by the count, else by the gaps between them), otherwise it is the step.
    pub fn angle_step(&self) -> f64 {
        let n = f64::from(self.first.count.max(1));
        let a = if self.first.flip { -self.angle } else { self.angle };
        if !self.equal_spacing {
            return a;
        }
        if (self.angle.abs() - 360.0).abs() < 1e-9 {
            a / n
        } else if n > 1.0 {
            a / (n - 1.0)
        } else {
            0.0
        }
    }

    /// The feature ids it copies (Feature pattern).
    pub fn feature_seeds(&self) -> &[FeatureId] {
        if self.pattern_type == PatternType::Feature { &self.features } else { &[] }
    }
}

/// The plane a mirror reflects in (PS26.1): a default plane or a Plane feature, a planar face,
/// or a mate connector's XY plane.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MirrorPlane {
    Plane(cadrs_sketch::PlaneRef),
    Face(FaceRef),
    Connector(ConnectorRef),
}

/// Mirror (P3.8, PS26; the dialog in `ex5-step12.png`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MirrorFeature {
    #[serde(default)]
    pub mirror_type: PatternType,
    #[serde(default)]
    pub parts: Vec<PartId>,
    #[serde(default)]
    pub features: Vec<FeatureId>,
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    pub plane: Option<MirrorPlane>,
    /// Part mirror: New, Add (the usual: one symmetric part, PS26.2), Remove, Intersect.
    #[serde(default)]
    pub op: BooleanOp,
    #[serde(default)]
    pub merge_all: bool,
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
}

impl MirrorFeature {
    pub fn seeds_empty(&self) -> bool {
        match self.mirror_type {
            PatternType::Part => self.parts.is_empty(),
            PatternType::Feature => self.features.is_empty(),
            PatternType::Face => self.faces.is_empty(),
        }
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.seeds_empty() {
            return Some(match self.mirror_type {
                PatternType::Part => "Select parts to mirror",
                PatternType::Feature => "Select features to mirror",
                PatternType::Face => "Select faces to mirror",
            });
        }
        self.plane.is_none().then_some("Select a mirror plane")
    }
}

/// Where each instance of a pattern is shown for Skip instances (PS22.5): its grid index,
/// where its dot goes, and whether it is skipped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InstanceDot {
    pub index: [u32; 2],
    pub at: Vec3,
    pub skipped: bool,
}

/// A skipped instance as the dialog lists it: "(2, 0)".
pub fn index_label(i: [u32; 2]) -> String {
    format!("({}, {})", i[0], i[1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_and_steps() {
        let mut p = PatternFeature::new(PatternKind::Linear);
        p.first.count = 3;
        let g = p.grid();
        assert_eq!(g[0], ([0, 0], [0.0, 0.0]));
        assert_eq!(g.len(), 3);
        p.second_on = true;
        p.second.count = 2;
        assert_eq!(p.grid().len(), 6);
        p.first.centered = true;
        let g = p.grid();
        assert_eq!(g[0].0, [1, 0], "the middle instance is the seed");
        let mut c = PatternFeature::new(PatternKind::Circular);
        c.first.count = 4;
        assert_eq!(c.angle_step(), 90.0);
        c.angle = 60.0;
        c.first.count = 3;
        assert_eq!(c.angle_step(), 30.0, "3 over 60° with equal spacing: a 30° step (PS24.2)");
        c.equal_spacing = false;
        assert_eq!(c.angle_step(), 60.0, "3 × 60° spans 120°");
        assert_eq!(index_label([2, 0]), "(2, 0)");
    }
}

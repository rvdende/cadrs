//! Onshape's **Sheet metal loft** feature (P3I.9, SM19.2; `reference/onshape/sheetmetal/raw/`
//! `help-sheet_metal_loft.txt`, dialog `help/feature-tools/sheet-metal-loft-dialog-01.png`):
//!
//! - **New | Add**: a new sheet metal model, or walls added to an active one (the *Merge scope*
//!   takes the parts of a single active model).
//! - **Profile 1**, **Profile 2**: a sketch region, a face, edges (sketch curves or part edges,
//!   chained) or a point.
//! - **Connections** (a checkbox opening the *Match connections* list): each connection joins a
//!   point of profile 1 to a point of profile 2. It is kept as a position along each profile
//!   (0..1 of its length from its start), so the view's manipulators drag it along the profiles;
//!   picking a vertex or an edge of a profile places it there. **Rip** cuts the sheet at it.
//! - **Chordal tolerance** (1 mm): how far the tessellated walls may stray from the loft surface.
//! - For New, the **General**, **Material** and **Relief** sections of the Sheet metal model.
//!
//! The rebuild (`rebuild/kernel_ops/sheetmetal_loft.rs`) lays the loft out with
//! [`cadrs_sheetmetal::loft`] (planar walls along the tessellation, facet joints, a closed loft
//! ripped at its first connection) and folds it into a part with mitred walls.

use cadrs_sheetmetal::Params;
use serde::{Deserialize, Serialize};

use crate::document::{EdgeRef, FaceRef, RegionRef, VertexRef};
use crate::ids::{FeatureId, PartId};
use crate::sheetmetal::{CurveRef, SheetMetalExprs, SheetMetalModelFeature, plain};

/// One entity of a loft profile.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum LoftItem {
    Region(RegionRefKey),
    Face(FaceRef),
    Curve(CurveRef),
    Edge(EdgeRef),
    SketchPoint { sketch: FeatureId, point: cadrs_sketch::PointId },
    Vertex(VertexRef),
}

/// A region kept by its sketch and seed (the region's curves are looked up again, like a
/// [`RegionRef`]; `Copy` so profiles stay cheap to compare).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RegionRefKey {
    pub sketch: FeatureId,
    pub seed: cadrs_sketch::Vec2,
}

impl RegionRefKey {
    pub fn of(r: &RegionRef) -> Self {
        RegionRefKey { sketch: r.sketch, seed: r.seed }
    }
}

impl LoftItem {
    /// The feature it depends on.
    pub fn parent(&self) -> FeatureId {
        match self {
            LoftItem::Region(r) => r.sketch,
            LoftItem::Face(f) => FeatureId(f.face.op),
            LoftItem::Curve(c) => c.sketch,
            LoftItem::Edge(e) => FeatureId(e.edge.op()),
            LoftItem::SketchPoint { sketch, .. } => *sketch,
            LoftItem::Vertex(v) => v.part.feature,
        }
    }

    pub fn sketch(&self) -> Option<FeatureId> {
        match self {
            LoftItem::Region(r) => Some(r.sketch),
            LoftItem::Curve(c) => Some(c.sketch),
            LoftItem::SketchPoint { sketch, .. } => Some(*sketch),
            _ => None,
        }
    }
}

/// A connection: where it meets each profile (0..1 along it) and whether it rips.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoftConnection {
    pub t1: f64,
    pub t2: f64,
    #[serde(default)]
    pub rip: bool,
}

/// New or Add (the dialog's tabs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SmLoftOp {
    #[default]
    New,
    Add,
}

impl SmLoftOp {
    pub const ALL: [SmLoftOp; 2] = [SmLoftOp::New, SmLoftOp::Add];

    pub fn label(self) -> &'static str {
        match self {
            SmLoftOp::New => "New",
            SmLoftOp::Add => "Add",
        }
    }
}

/// A Sheet metal loft feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetMetalLoftFeature {
    #[serde(default)]
    pub op: SmLoftOp,
    /// Add: the parts of the active model to add to.
    #[serde(default)]
    pub merge_scope: Vec<PartId>,
    #[serde(default)]
    pub profile1: Vec<LoftItem>,
    #[serde(default)]
    pub profile2: Vec<LoftItem>,
    #[serde(default)]
    pub connections_on: bool,
    #[serde(default)]
    pub connections: Vec<LoftConnection>,
    pub chordal_tolerance: f64,
    pub chordal_tolerance_expr: String,
    /// New: the model's settings (Add uses the active model's).
    #[serde(default = "SheetMetalModelFeature::default_params")]
    pub params: Params,
    #[serde(default)]
    pub flip_thickness: bool,
    #[serde(default)]
    pub exprs: SheetMetalExprs,
}

impl Default for SheetMetalLoftFeature {
    fn default() -> Self {
        let params = SheetMetalModelFeature::default_params();
        SheetMetalLoftFeature {
            op: SmLoftOp::New,
            merge_scope: Vec::new(),
            profile1: Vec::new(),
            profile2: Vec::new(),
            connections_on: false,
            connections: Vec::new(),
            chordal_tolerance: cadrs_sheetmetal::loft::DEFAULT_CHORDAL_TOLERANCE,
            chordal_tolerance_expr: format!("{} mm", plain(cadrs_sheetmetal::loft::DEFAULT_CHORDAL_TOLERANCE)),
            params,
            flip_thickness: false,
            exprs: SheetMetalExprs::of(&params),
        }
    }
}

impl SheetMetalLoftFeature {
    pub fn is_empty(&self) -> bool {
        self.profile1.is_empty() || self.profile2.is_empty()
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.profile1.is_empty() {
            return Some("Select the first profile");
        }
        if self.profile2.is_empty() {
            return Some("Select the second profile");
        }
        if self.op == SmLoftOp::New && !self.params.validate().is_empty() {
            return Some("A value is out of range");
        }
        if self.chordal_tolerance.is_nan() || self.chordal_tolerance <= 0.0 || self.chordal_tolerance.is_infinite() {
            return Some("The chordal tolerance must be greater than 0");
        }
        if self.op == SmLoftOp::Add && self.merge_scope.is_empty() {
            return Some("Select the sheet metal part to add to");
        }
        None
    }

    /// The sketches its profiles come from (hidden once used, like a loft's).
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v = Vec::new();
        for s in self.profile1.iter().chain(&self.profile2).filter_map(LoftItem::sketch) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
        v
    }

    pub fn parents(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = Vec::new();
        for f in self.profile1.iter().chain(&self.profile2).map(LoftItem::parent).chain(self.merge_scope.iter().map(|p| p.feature)) {
            if !v.contains(&f) {
                v.push(f);
            }
        }
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_and_problems() {
        let mut f = SheetMetalLoftFeature::default();
        assert_eq!(f.chordal_tolerance, 1.0);
        assert_eq!(f.chordal_tolerance_expr, "1 mm");
        assert_eq!(f.problem(), Some("Select the first profile"));
        let p = LoftItem::SketchPoint { sketch: FeatureId::new(), point: Default::default() };
        f.profile1.push(p);
        f.profile2.push(p);
        assert_eq!(f.problem(), None);
        f.op = SmLoftOp::Add;
        assert_eq!(f.problem(), Some("Select the sheet metal part to add to"));
        let s = ron::to_string(&f).unwrap();
        assert_eq!(ron::from_str::<SheetMetalLoftFeature>(&s).unwrap(), f);
    }
}

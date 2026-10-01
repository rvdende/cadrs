//! Onshape's **Sheet metal model** feature (P3I.2; `reference/onshape/sheetmetal/`
//! `simultaneous-sheet-metal.md`, SM1–SM2): sheet metal parts made by **Convert** (the faces of
//! parts become walls), **Extrude** (sketch curves swept into walls, arcs rolled or bent) or
//! **Thicken** (faces and sketch regions), with the model's **General**, **Material** and
//! **Relief** settings ([`cadrs_sheetmetal::Params`]).
//!
//! The rebuild (`rebuild/kernel_ops/sheetmetal.rs`) builds the sheet metal definition
//! ([`cadrs_sheetmetal::Model`]) with [`cadrs_sheetmetal::construct`], lays it flat
//! ([`cadrs_sheetmetal::flatten`]: the collision check, SM1.5) and makes the **folded solid**
//! through the kernel: each wall thickened on its material side, each bend a cylindrical shell,
//! the relief cuts taken out, fused into one part per flat-pattern part. The definition and its
//! flat pattern stay in the rebuild as the model's [`SheetMetalContext`] (what the table and flat
//! view panel shows, P3I.3).

use cadrs_sheetmetal::{FlatPattern, JointId, Model, Params, WallId};
use cadrs_sketch::CurveId;
use serde::{Deserialize, Serialize};

use crate::applied::EdgeOrFace;
use crate::document::{EndCondition, EndType, FaceRef, RegionRef, UpTo};
use crate::ids::{FeatureId, PartId};

/// The dialog's tabs (SM2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SheetMetalOp {
    #[default]
    Convert,
    Extrude,
    Thicken,
}

impl SheetMetalOp {
    pub const ALL: [SheetMetalOp; 3] = [SheetMetalOp::Convert, SheetMetalOp::Extrude, SheetMetalOp::Thicken];

    pub fn label(self) -> &'static str {
        match self {
            SheetMetalOp::Convert => "Convert",
            SheetMetalOp::Extrude => "Extrude",
            SheetMetalOp::Thicken => "Thicken",
        }
    }
}

/// The end types a sheet metal Extrude offers (SM2.3): the extrude's, less Through all.
pub const EXTRUDE_ENDS: [EndType; 5] = [EndType::Blind, EndType::UpToNext, EndType::UpToFace, EndType::UpToPart, EndType::UpToVertex];

/// A sketch curve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurveRef {
    pub sketch: FeatureId,
    pub curve: CurveId,
}

/// The settings as typed (expressions may name variables, P3F.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SheetMetalExprs {
    pub thickness: String,
    pub bend_radius: String,
    pub k_factor: String,
    pub rolled_k_factor: String,
    pub bend_allowance: String,
    pub bend_deduction: String,
    pub minimal_gap: String,
    pub corner_relief_scale: String,
    pub corner_relief_size: String,
    pub bend_relief_depth_scale: String,
    pub bend_relief_width_scale: String,
}

impl SheetMetalExprs {
    /// The expressions of these settings, in mm.
    pub fn of(p: &Params) -> Self {
        let mm = |v: f64| format!("{} mm", plain(v));
        SheetMetalExprs {
            thickness: mm(p.thickness),
            bend_radius: mm(p.bend_radius),
            k_factor: plain(p.k_factor),
            rolled_k_factor: plain(p.rolled_k_factor),
            bend_allowance: mm(p.bend_allowance),
            bend_deduction: mm(p.bend_deduction),
            minimal_gap: mm(p.minimal_gap),
            corner_relief_scale: plain(p.corner_relief.scale),
            corner_relief_size: mm(p.corner_relief.size),
            bend_relief_depth_scale: plain(p.bend_relief.depth_scale),
            bend_relief_width_scale: plain(p.bend_relief.width_scale),
        }
    }
}

impl Default for SheetMetalExprs {
    fn default() -> Self {
        SheetMetalExprs::of(&SheetMetalModelFeature::default_params())
    }
}

/// A number without trailing zeros.
pub fn plain(v: f64) -> String {
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" { "0".into() } else { s.into() }
}

/// A Sheet metal model feature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetMetalModelFeature {
    #[serde(default)]
    pub operation: SheetMetalOp,
    // Convert (SM2.2)
    /// Parts and surfaces to convert.
    #[serde(default)]
    pub parts: Vec<PartId>,
    /// Faces to exclude (no wall for them).
    #[serde(default)]
    pub exclude: Vec<FaceRef>,
    /// Edges or cylinders to bend (Convert and Thicken), in the order they were picked: the
    /// order decides which walls bends join (SM2.2).
    #[serde(default)]
    pub bends: Vec<EdgeOrFace>,
    /// Clearance from input (mm).
    #[serde(default)]
    pub clearance: f64,
    #[serde(default = "zero_mm")]
    pub clearance_expr: String,
    /// The clearance includes the bends ("Include bends"; Thicken's "Clearance includes bends").
    #[serde(default)]
    pub include_bends: bool,
    /// Keep input parts (else Convert consumes them).
    #[serde(default)]
    pub keep_input: bool,
    // Extrude (SM2.3)
    /// Sketch curves to extrude...
    #[serde(default)]
    pub curves: Vec<CurveRef>,
    /// ...and whole sketches picked in the feature list.
    #[serde(default)]
    pub sketches: Vec<FeatureId>,
    /// Arcs to extrude as bends.
    #[serde(default)]
    pub arcs_as_bends: Vec<CurveRef>,
    #[serde(default)]
    pub end: EndType,
    pub depth: f64,
    pub depth_expr: String,
    #[serde(default)]
    pub up_to: Option<UpTo>,
    /// The extrude's opposite direction arrow.
    #[serde(default)]
    pub flip_extrude: bool,
    #[serde(default)]
    pub symmetric: bool,
    /// Second end position.
    #[serde(default)]
    pub second: Option<EndCondition>,
    // Thicken (SM2.4)
    /// Faces to thicken...
    #[serde(default)]
    pub faces: Vec<FaceRef>,
    /// ...sketch regions...
    #[serde(default)]
    pub regions: Vec<RegionRef>,
    /// ...and whole sketches' regions.
    #[serde(default)]
    pub region_sketches: Vec<FeatureId>,
    #[serde(default)]
    pub tangent_propagation: bool,
    // General, Material, Relief (SM2.5–SM2.7)
    #[serde(default = "SheetMetalModelFeature::default_params")]
    pub params: Params,
    /// The thickness's opposite direction arrow: the material on the other side.
    #[serde(default)]
    pub flip_thickness: bool,
    #[serde(default)]
    pub exprs: SheetMetalExprs,
}

fn zero_mm() -> String {
    "0 mm".into()
}

impl Default for SheetMetalModelFeature {
    fn default() -> Self {
        let params = Self::default_params();
        SheetMetalModelFeature {
            operation: SheetMetalOp::Convert,
            parts: Vec::new(),
            exclude: Vec::new(),
            bends: Vec::new(),
            clearance: 0.0,
            clearance_expr: zero_mm(),
            include_bends: false,
            keep_input: false,
            curves: Vec::new(),
            sketches: Vec::new(),
            arcs_as_bends: Vec::new(),
            end: EndType::Blind,
            depth: crate::document::DEFAULT_DEPTH,
            depth_expr: "25 mm".into(),
            up_to: None,
            flip_extrude: false,
            symmetric: false,
            second: None,
            faces: Vec::new(),
            regions: Vec::new(),
            region_sketches: Vec::new(),
            tangent_propagation: false,
            params,
            flip_thickness: false,
            exprs: SheetMetalExprs::of(&params),
        }
    }
}

impl SheetMetalModelFeature {
    /// The settings a new model starts with: Onshape's (K Factor 0.45, rolled 0.5, Simple corner
    /// reliefs, Obround – Scaled bend reliefs with depth and width scales of 2, as the help's
    /// dialogs show them), with round millimetre lengths.
    pub fn default_params() -> Params {
        let mut p = Params::default();
        p.bend_relief.width_scale = 2.0;
        p
    }

    /// Nothing picked for the current operation.
    pub fn is_empty(&self) -> bool {
        match self.operation {
            SheetMetalOp::Convert => self.parts.is_empty(),
            SheetMetalOp::Extrude => self.curves.is_empty() && self.sketches.is_empty(),
            SheetMetalOp::Thicken => self.faces.is_empty() && self.regions.is_empty() && self.region_sketches.is_empty(),
        }
    }

    pub fn problem(&self) -> Option<&'static str> {
        if self.is_empty() {
            return Some(match self.operation {
                SheetMetalOp::Convert => "Select parts or surfaces to convert",
                SheetMetalOp::Extrude => "Select sketch curves to extrude",
                SheetMetalOp::Thicken => "Select faces or sketch regions to thicken",
            });
        }
        if !self.params.validate().is_empty() {
            return Some("A value is out of range");
        }
        if !(self.clearance >= 0.0 && self.clearance.is_finite()) {
            return Some("The clearance must be at least 0");
        }
        if self.operation == SheetMetalOp::Extrude {
            if self.end == EndType::Blind && !(self.depth > 0.0) {
                return Some("The depth must be greater than zero");
            }
            if self.end.needs_target() && self.up_to.is_none() {
                return Some("Select what the extrude goes up to");
            }
        }
        None
    }

    /// The sketches its curves and regions come from.
    pub fn sketch_ids(&self) -> Vec<FeatureId> {
        let mut v: Vec<FeatureId> = Vec::new();
        let mut add = |f: FeatureId| {
            if !v.contains(&f) {
                v.push(f);
            }
        };
        match self.operation {
            SheetMetalOp::Extrude => {
                self.curves.iter().chain(&self.arcs_as_bends).for_each(|c| add(c.sketch));
                self.sketches.iter().for_each(|s| add(*s));
            }
            SheetMetalOp::Thicken => {
                self.regions.iter().for_each(|r| add(r.sketch));
                self.region_sketches.iter().for_each(|s| add(*s));
            }
            SheetMetalOp::Convert => {}
        }
        v
    }

    /// The features it refers to (PS11).
    pub fn parents(&self) -> Vec<FeatureId> {
        let mut out = self.sketch_ids();
        let mut add = |f: FeatureId| {
            if !out.contains(&f) {
                out.push(f);
            }
        };
        match self.operation {
            SheetMetalOp::Convert => {
                self.parts.iter().for_each(|p| add(p.feature));
                self.exclude.iter().for_each(|f| add(FeatureId(f.face.op)));
                self.bends.iter().for_each(|b| add(b.part().feature));
            }
            SheetMetalOp::Thicken => {
                self.faces.iter().for_each(|f| add(FeatureId(f.face.op)));
                self.bends.iter().for_each(|b| add(b.part().feature));
            }
            SheetMetalOp::Extrude => match &self.up_to {
                Some(UpTo::Face(f)) => add(FeatureId(f.face.op)),
                Some(UpTo::Part(p)) => add(p.feature),
                Some(UpTo::Vertex(v)) => add(v.part.feature),
                None => {}
            },
        }
        out
    }
}

/// A sheet metal model as the rebuild keeps it (the "sheet metal context", SM1.3): its
/// definition, its flat pattern and the parts it made, for the table and flat view and for the
/// sheet metal features after it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SheetMetalContext {
    /// The Sheet metal model feature (its name names the context).
    pub feature: FeatureId,
    pub model: Model,
    pub flat: FlatPattern,
    /// Each part it made with the walls in it (in flat-pattern part order).
    pub parts: Vec<(PartId, Vec<WallId>)>,
    /// Still active: features after it act on it as sheet metal (SM1.6). Finish sheet metal
    /// model (P3I.5) ends that.
    pub active: bool,
    /// Where each wall and joint came from (a face, edge, curve or cylinder's key), so later
    /// features and the views can find them.
    pub wall_keys: Vec<(u64, WallId)>,
    pub joint_keys: Vec<(u64, JointId)>,
}

impl SheetMetalContext {
    /// The part a wall is in.
    pub fn part_of_wall(&self, w: WallId) -> Option<PartId> {
        self.parts.iter().find(|(_, ws)| ws.contains(&w)).map(|(p, _)| *p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_follow_onshape_and_round_trip() {
        let f = SheetMetalModelFeature::default();
        assert_eq!(f.operation, SheetMetalOp::Convert);
        assert_eq!(f.params.k_factor, 0.45);
        assert_eq!(f.params.rolled_k_factor, 0.5);
        assert_eq!(f.params.bend_relief.depth_scale, 2.0);
        assert_eq!(f.params.bend_relief.width_scale, 2.0);
        assert_eq!(f.exprs.k_factor, "0.45");
        assert_eq!(f.exprs.thickness, "1 mm");
        assert_eq!(f.problem(), Some("Select parts or surfaces to convert"));
        let s = ron::to_string(&f).unwrap();
        let back: SheetMetalModelFeature = ron::from_str(&s).unwrap();
        assert_eq!(back, f);
        // An old document without the new fields reads with the defaults.
        let old: SheetMetalModelFeature = ron::from_str("(depth: 10.0, depth_expr: \"10 mm\")").unwrap();
        assert_eq!(old.params, SheetMetalModelFeature::default_params());
    }

    #[test]
    fn out_of_range_settings_are_a_problem() {
        let mut f = SheetMetalModelFeature { parts: vec![PartId::new(FeatureId::new(), 0)], ..Default::default() };
        assert_eq!(f.problem(), None);
        f.params.k_factor = 1.5;
        assert_eq!(f.problem(), Some("A value is out of range"));
    }
}

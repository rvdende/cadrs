//! The legacy sheet metal stand-in (P3I.8, SM17; lesson 17 "Importing legacy sheet metal"): a
//! folded part as another CAD system would hand it over, a plain solid in a STEP file, for an
//! import to be made active sheet metal again with **Sheet metal model → Thicken**, **Tangent
//! propagation** and the bend cylinders picked as **Edges or cylinders to bend**.
//!
//! Two parts, made with cadrs's own sheet metal (so the expected flat and mass are known), each
//! 2 mm thick, inner bend radius 3 mm, K 0.45:
//!
//! - a **C-channel**: a Front-plane chain of four lines (lips 10, sides 40, base 60) extruded
//!   100 mm, the material inside the C ([`channel`]);
//! - a **Case** like the lesson's (`t0043.9.png`): an open box, a [`CASE`] block converted with
//!   its top excluded and its four bottom edges bent, its corners left open: each wall stops
//!   short of its neighbours by the minimal gap, the bends' ends relieved ([`case`]). Its walls
//!   meet the base only through the bends: Tangent propagation has to go round the corner gaps,
//!   base → bend → wall, to take the whole skin. (Round corner reliefs on the import leave
//!   slivers when it is thickened again: see the gaps doc.)
//!
//! [`step`] / [`case_step`] export the folded solid as STEP: an imported copy has no sheet metal
//! left in it.

use std::sync::Arc;

use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

use crate::applied::EdgeOrFace;
use crate::document::{EdgeRef, ExtrudeFeature, Feature, FeatureKind, RegionRef, SketchFeature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::rebuild::Rebuilder;
use crate::rebuild::exchange::{ExportItem, ExportRequest, ModelFormat};
use crate::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};

pub const THICKNESS: f64 = 2.0;
pub const RADIUS: f64 = 3.0;
pub const DEPTH: f64 = 100.0;
/// The chain on Front (x, z): lip, side, base, side, lip.
pub const CHAIN: [(f64, f64); 6] = [(10.0, 40.0), (0.0, 40.0), (0.0, 0.0), (60.0, 0.0), (60.0, 40.0), (50.0, 40.0)];

pub const SKETCH: FeatureId = FeatureId::from_u128(0x5317_0000_0000_0000_0000_0000_0000_0001);
pub const MODEL: FeatureId = FeatureId::from_u128(0x5317_0000_0000_0000_0000_0000_0000_0002);
pub const PART: PartId = PartId::new(MODEL, 0);

/// The model's settings.
pub fn params() -> cadrs_sheetmetal::Params {
    cadrs_sheetmetal::Params { thickness: THICKNESS, bend_radius: RADIUS, k_factor: 0.45, ..SheetMetalModelFeature::default_params() }
}

/// The channel's features: its sketch and its Sheet metal model (Extrude).
pub fn channel() -> Vec<Feature> {
    let mut g = Sketch::new();
    SketchOp::AddPolyline { points: CHAIN.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: false, construction: false, label: "Add line" }
        .apply(&mut g)
        .expect("a chain");
    let sketch = Feature { id: SKETCH, name: "Sketch 1".into(), kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Front), disable_imprinting: false, geometry: g }) };
    let p = params();
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Extrude,
        sketches: vec![SKETCH],
        depth: DEPTH,
        depth_expr: format!("{DEPTH} mm"),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    let model = Feature { id: MODEL, name: "Sheet metal model 1".into(), kind: FeatureKind::SheetMetalModel(x) };
    vec![sketch, model]
}

/// The channel's folded solid as a STEP file (built with `r`, the rebuild session): a plain
/// part named "Channel".
pub fn step(r: &mut Rebuilder) -> Result<Vec<u8>, String> {
    let features = Arc::new(channel());
    let el = ElementId::from_u128(0x5317);
    let item = ExportItem { features, part: PART, name: "Channel".into(), pose: None, source: (el, PART), source_name: "Channel".into() };
    let req = ExportRequest::new(ModelFormat::Step, "Channel", vec![item]);
    let files = r.export_files(&req)?;
    files.into_iter().next().map(|f| f.bytes).ok_or_else(|| "no file".into())
}

pub const CASE_SKETCH: FeatureId = FeatureId::from_u128(0x5317_0000_0000_0000_0000_0000_0000_0011);
pub const CASE_BLOCK: FeatureId = FeatureId::from_u128(0x5317_0000_0000_0000_0000_0000_0000_0012);
pub const CASE_MODEL: FeatureId = FeatureId::from_u128(0x5317_0000_0000_0000_0000_0000_0000_0013);
pub const CASE_PART: PartId = PartId::new(CASE_MODEL, 0);
/// The Case's block on Top: width (X), depth (Y), height (Z).
pub const CASE: (f64, f64, f64) = (80.0, 60.0, 40.0);

/// The Case's settings: [`params`] (Simple corners: the corner gaps the bends leave).
pub fn case_params() -> cadrs_sheetmetal::Params {
    params()
}

/// The Case's features: Sketch 1 (the block's rectangle on Top), Extrude 1 and its Sheet metal
/// model (Convert, the top excluded, the four bottom edges bent).
pub fn case() -> Vec<Feature> {
    let (w, d, h) = CASE;
    let mut g = Sketch::new();
    SketchOp::AddPolyline { points: vec![Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, d), Vec2::new(0.0, d)], closed: true, construction: false, label: "Add rectangle" }
        .apply(&mut g)
        .expect("a rectangle");
    let region = cadrs_sketch::region::regions(&g).into_iter().next().expect("the rectangle's region");
    let sketch = Feature { id: CASE_SKETCH, name: "Sketch 1".into(), kind: FeatureKind::Sketch(SketchFeature { plane: Some(PlaneRef::Top), disable_imprinting: false, geometry: g }) };
    let block = Feature {
        id: CASE_BLOCK,
        name: "Extrude 1".into(),
        kind: FeatureKind::Extrude(ExtrudeFeature { regions: vec![RegionRef::new(CASE_SKETCH, &region)], depth: h, depth_expr: format!("{h} mm"), ..Default::default() }),
    };
    // A session of its own: `case_step` runs this on the rebuild worker, which
    // `rebuild::build` would wait on.
    let b = Rebuilder::new().rebuild(&[sketch.clone(), block.clone()]);
    let part = b.parts.first().expect("the block");
    let s = &part.solid;
    let top = (0..s.faces.len()).find(|i| s.faces[*i].plane.is_some_and(|p| p.normal()[2] > 0.999)).expect("the top");
    let bottom = (0..s.faces.len()).find(|i| s.faces[*i].plane.is_some_and(|p| p.normal()[2] < -0.999)).expect("the bottom");
    let top = crate::document::FaceRef { part: part.id, face: s.faces[top].name, seed: s.faces[top].center.unwrap_or([w / 2.0, d / 2.0, h]) };
    let bends = s
        .face_edges(&s.faces[bottom].name)
        .into_iter()
        .filter_map(|e| s.edge(&e))
        .map(|e| EdgeOrFace::Edge(EdgeRef { part: part.id, edge: e.name, seed: e.points[e.points.len() / 2] }))
        .collect();
    let p = case_params();
    let x = SheetMetalModelFeature { operation: SheetMetalOp::Convert, parts: vec![part.id], exclude: vec![top], bends, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    let model = Feature { id: CASE_MODEL, name: "Sheet metal model 1".into(), kind: FeatureKind::SheetMetalModel(x) };
    vec![sketch, block, model]
}

/// The Case's folded solid as a STEP file: a plain part named "Case".
pub fn case_step(r: &mut Rebuilder) -> Result<Vec<u8>, String> {
    let features = Arc::new(case());
    let el = ElementId::from_u128(0x5317);
    let item = ExportItem { features, part: CASE_PART, name: "Case".into(), pose: None, source: (el, CASE_PART), source_name: "Case".into() };
    let req = ExportRequest::new(ModelFormat::Step, "Case", vec![item]);
    let files = r.export_files(&req)?;
    files.into_iter().next().map(|f| f.bytes).ok_or_else(|| "no file".into())
}

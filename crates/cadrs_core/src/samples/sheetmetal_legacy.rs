//! The legacy sheet metal stand-in (P3I.8, SM17; lesson 17 "Importing legacy sheet metal"): a
//! folded part as another CAD system would hand it over, a plain solid in a STEP file, for an
//! import to be made active sheet metal again with **Sheet metal model → Thicken**, **Tangent
//! propagation** and the bend cylinders picked as **Edges or cylinders to bend**.
//!
//! The part is a C-channel made with cadrs's own sheet metal (so the expected flat and mass are
//! known): a Front-plane chain of four lines (lips 10, sides 40, base 60) extruded 100 mm, 2 mm
//! thick, inner bend radius 3 mm, K 0.45, the material inside the C. [`step`] exports its folded
//! solid as STEP: an imported copy has no sheet metal left in it.

use std::sync::Arc;

use cadrs_sketch::{PlaneRef, Sketch, SketchOp, Vec2};

use crate::document::{Feature, FeatureKind, SketchFeature};
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

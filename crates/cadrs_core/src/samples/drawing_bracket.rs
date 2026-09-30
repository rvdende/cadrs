//! A bracket for the drawing-view scenarios and tests (P3C.2): a part with through holes, rounded
//! corners (tangent edges) and faces at several depths, so its views show hidden lines and
//! tangent edges. It is our own part, in mm:
//!
//! - **Sketch 1 / Extrude 1** (the base): a 120 × 80 plate on Top centred on the origin, corners
//!   R10, with four Ø10 holes at (±45, ±25); 12 mm up.
//! - **Sketch 2 / Extrude 2** (the upright): on Front, x −40..40 from the plate's top (z 12) to
//!   z 70, its top right corner R15 and its top left corner chamfered 20 × 20 (a 45° face, for
//!   auxiliary views), with a Ø24 hole centred at z 48; 16 mm thick symmetric about the Front
//!   plane, Add.
//!
//! The part is named "Bracket".

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::command::CommandError;
use crate::commands::{AddExtrude, AddSketch, EditSketch, RenamePart, SetExtrude};
use crate::document::{BooleanOp, ExtrudeFeature};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::samples::gear_cover::Studio;

pub const SKETCH_1: FeatureId = FeatureId::from_u128(0xb7ac_e700_0000_0000_0000_0000_0000_0001);
pub const EXTRUDE_1: FeatureId = FeatureId::from_u128(0xb7ac_e700_0000_0000_0000_0000_0000_0002);
pub const SKETCH_2: FeatureId = FeatureId::from_u128(0xb7ac_e700_0000_0000_0000_0000_0000_0003);
pub const EXTRUDE_2: FeatureId = FeatureId::from_u128(0xb7ac_e700_0000_0000_0000_0000_0000_0004);
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

/// Plate half sizes, thickness and corner radius.
pub const PLATE_X: f64 = 60.0;
pub const PLATE_Y: f64 = 40.0;
pub const PLATE_T: f64 = 12.0;
pub const PLATE_R: f64 = 10.0;
/// Plate holes: centres (±HOLE_X, ±HOLE_Y), radius.
pub const HOLE_X: f64 = 45.0;
pub const HOLE_Y: f64 = 25.0;
pub const HOLE_R: f64 = 5.0;
/// Upright: half width, top, thickness, corner radius, hole height and radius.
pub const UP_X: f64 = 40.0;
pub const UP_TOP: f64 = 70.0;
pub const UP_T: f64 = 16.0;
pub const UP_R: f64 = 15.0;
/// The chamfer on the upright's top left corner.
pub const UP_CHAMFER: f64 = 20.0;
pub const UP_HOLE_Z: f64 = 48.0;
pub const UP_HOLE_R: f64 = 12.0;

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn fillet(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, corner: Vec2, radius: f64) -> Result<(), CommandError> {
    let p = {
        let g = &s
            .document()
            .element(el)
            .and_then(|e| e.feature(sketch))
            .and_then(|f| f.sketch())
            .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?
            .geometry;
        g.point_at(corner, 1e-9).ok_or_else(|| CommandError::Invalid("corner not found".into()))?
    };
    s.run(&EditSketch {
        element: el,
        feature: sketch,
        op: SketchOp::Fillet { corner: p, radius, equal_to: None },
    })
}

/// An extrude of the regions of `sketch` under `seeds`.
pub fn extrude(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    feature: FeatureId,
    seeds: &[Vec2],
    set: impl FnOnce(&mut ExtrudeFeature),
) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, seeds);
    if regions.len() != seeds.len() {
        return Err(CommandError::Invalid("a region of the sample is missing".into()));
    }
    let mut e = super::extrude_of(regions, 10.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// Builds the bracket in Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_1, op: rect(-PLATE_X, -PLATE_Y, PLATE_X, PLATE_Y) })?;
    for c in [v(-PLATE_X, -PLATE_Y), v(PLATE_X, -PLATE_Y), v(PLATE_X, PLATE_Y), v(-PLATE_X, PLATE_Y)] {
        fillet(s, el, SKETCH_1, c, PLATE_R)?;
    }
    for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
        s.run(&EditSketch {
            element: el,
            feature: SKETCH_1,
            op: SketchOp::AddCircle { center: v(sx * HOLE_X, sy * HOLE_Y), radius: HOLE_R, construction: false },
        })?;
    }
    extrude(s, el, SKETCH_1, EXTRUDE_1, &[v(0.0, 0.0)], |e| {
        e.depth = PLATE_T;
        e.depth_expr = format!("{PLATE_T} mm");
    })?;
    // The upright on Front: sketch x = model X, sketch y = model Z.
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Front) })?;
    s.run(&EditSketch { element: el, feature: SKETCH_2, op: rect(-UP_X, PLATE_T, UP_X, UP_TOP) })?;
    fillet(s, el, SKETCH_2, v(UP_X, UP_TOP), UP_R)?;
    let corner = s
        .document()
        .element(el)
        .and_then(|e| e.feature(SKETCH_2))
        .and_then(|f| f.sketch())
        .and_then(|sk| sk.geometry.point_at(v(-UP_X, UP_TOP), 1e-9))
        .ok_or_else(|| CommandError::Invalid("corner not found".into()))?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::Chamfer { corner, d1: UP_CHAMFER, d2: UP_CHAMFER, equal_to: None },
    })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_2,
        op: SketchOp::AddCircle { center: v(0.0, UP_HOLE_Z), radius: UP_HOLE_R, construction: false },
    })?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, &[v(0.0, PLATE_T + 4.0)], |e| {
        e.op = BooleanOp::Add;
        e.symmetric = true;
        e.depth = UP_T;
        e.depth_expr = format!("{UP_T} mm");
    })?;
    s.run(&RenamePart { element: el, part: PART, name: "Bracket".into() })?;
    Ok(())
}

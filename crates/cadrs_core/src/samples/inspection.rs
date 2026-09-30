//! A small broken Part Studio for the Inspection and Repair lessons (P3D.1, P3D.2), shaped
//! after the course's Conrod exercise (`reference/onshape/training/inspection-and-repair.md`
//! IR6.1–IR6.8) but built from our own features (mm):
//!
//! - **Sketch 1 / Extrude 1**: a ring on Top, Ø30 outside and Ø18 inside, 10 mm (New);
//! - **Fillet 1**: R1 on the ring's top outer edge;
//! - **Sketch 2 / Extrude 2**: a 20 × 10 tab beside the ring (x 16..36, y −5..5), 6 mm (New).
//!
//! Then Sketch 2 is broken the way the course's Sketch 2 is:
//! - a spur line off the tab's top-right corner, whose far end joins nothing (a "Loose end");
//! - the tab's left side redrawn 0.02 in (0.508 mm) short of the bottom corner, so the tab no
//!   longer closes (a "Loose ends (2)" pair) and Extrude 2's region is gone;
//! - the ring's outer bottom edge used (projected, S20) into the sketch (Ø30, fixed), and a
//!   circle over the bore dimensioned Ø18, concentric with it and made **Equal** to it: the
//!   Equal and the Ø18 can't both hold, so the sketch can't be solved and both are in the
//!   conflicting set (the course's Equal 1 on Extrude geometry, `ex1-step5.png`).
//!
//! Sketch 2 and Extrude 2 are red. Deleting Equal 1 and closing the gap with Coincident repairs
//! both. Every id is fixed.

use cadrs_sketch::projection::Projected;
use cadrs_sketch::{ConstraintOf, CurveRef, Dimension, DimensionKind, Link, PlaneRef, SketchOp, Vec2};

use super::gear_cover::Studio;
use crate::applied::{EdgeOrFace, FilletFeature};
use crate::command::CommandError;
use crate::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use crate::document::{BooleanOp, EdgeRef, ExtrudeFeature};
use crate::ids::{ElementId, FeatureId, PartId};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0x1a5e_c700_0000_0000_0000_0000_0000_0000 | n)
}

pub const SKETCH_1: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const FILLET_1: FeatureId = id(3);
pub const SKETCH_2: FeatureId = id(4);
pub const EXTRUDE_2: FeatureId = id(5);
pub const RING: PartId = PartId::new(EXTRUDE_1, 0);

/// The ring's radii and height, the tab's box and height (mm).
pub const RING_OUT: f64 = 15.0;
pub const RING_IN: f64 = 9.0;
pub const RING_H: f64 = 10.0;
pub const TAB: (f64, f64, f64, f64) = (16.0, -5.0, 36.0, 5.0);
pub const TAB_H: f64 = 6.0;
/// The gap left in the tab's outline: 0.02 in.
pub const GAP: f64 = 0.508;

/// Adds the stand-in (healthy, then Sketch 2 broken) to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    let edit = |s: &mut dyn Studio, feature: FeatureId, op: SketchOp| s.run(&EditSketch { element: el, feature, op });
    // The ring.
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    for r in [RING_OUT, RING_IN] {
        edit(s, SKETCH_1, SketchOp::AddCircle { center: v(0.0, 0.0), radius: r, construction: false })?;
    }
    extrude(s, el, SKETCH_1, EXTRUDE_1, v((RING_OUT + RING_IN) / 2.0, 0.0), RING_H)?;
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let build = crate::rebuild::build(&features);
    let ring = build.part(RING).ok_or_else(|| CommandError::Invalid("the ring didn't build".into()))?;
    let p = [RING_OUT, 0.0, RING_H];
    let edge = ring
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .filter(|e| e.distance(p) < 1e-3)
        .ok_or_else(|| CommandError::Invalid("no top outer edge on the ring".into()))?;
    let entities = vec![EdgeOrFace::Edge(EdgeRef { part: ring.id, edge: edge.name, seed: p })];
    s.run(&AddFeature::fillet(el, FILLET_1, FilletFeature { entities, size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() }))?;
    // The tab, healthy: the spur first (its loose end is the first row, as in `ex1-step7.png`),
    // then the outline.
    let (x0, y0, x1, y1) = TAB;
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Top) })?;
    edit(s, SKETCH_2, SketchOp::AddPolyline { points: vec![v(x1, y1), v(x1 + 4.0, y1 + 4.0)], closed: false, construction: false, label: "Add line" })?;
    edit(
        s,
        SKETCH_2,
        SketchOp::AddPolyline {
            points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    )?;
    extrude(s, el, SKETCH_2, EXTRUDE_2, v((x0 + x1) / 2.0, 0.0), TAB_H)?;
    // Broken: the left side redrawn short of the bottom corner.
    let g = sketch_of(s, el, SKETCH_2)?;
    let left = g
        .curves
        .keys()
        .find(|k| {
            g.curve_ends(*k).is_some_and(|(a, b)| {
                let (a, b) = (g.pos(a), g.pos(b));
                (a.x - x0).abs() < 1e-9 && (b.x - x0).abs() < 1e-9
            })
        })
        .ok_or_else(|| CommandError::Invalid("no left side on the tab".into()))?;
    edit(s, SKETCH_2, SketchOp::Delete { curves: vec![left], points: vec![], dimensions: vec![], constraints: vec![] })?;
    edit(s, SKETCH_2, SketchOp::AddPolyline { points: vec![v(x0, y1), v(x0, y0 + GAP)], closed: false, construction: false, label: "Add line" })?;
    // The ring's outer bottom edge used in Sketch 2 (external geometry, fixed where the ring
    // puts it), a circle over its bore dimensioned Ø18, the two concentric, then made equal:
    // the Equal and the Ø18 can't both hold (the course's Equal 1 on Extrude geometry).
    let p = [RING_OUT, 0.0, 0.0];
    let e = ring
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .filter(|e| e.distance(p) < 1e-3)
        .ok_or_else(|| CommandError::Invalid("no bottom edge on the ring".into()))?;
    let items = vec![(Projected::Circle(v(0.0, 0.0), RING_OUT), Link::Edge { feature: EXTRUDE_1.0, edge: e.name })];
    edit(s, SKETCH_2, SketchOp::Use { items })?;
    let used = circle_ids(&sketch_of(s, el, SKETCH_2)?);
    edit(s, SKETCH_2, SketchOp::AddCircle { center: v(0.0, 0.0), radius: RING_IN, construction: false })?;
    let bore = circle_ids(&sketch_of(s, el, SKETCH_2)?).into_iter().find(|k| !used.contains(k));
    if let (Some(&a), Some(b)) = (used.first(), bore) {
        edit(
            s,
            SKETCH_2,
            SketchOp::SetDimension {
                dimension: Dimension::new(DimensionKind::Diameter { curve: b }, 2.0 * RING_IN, std::f64::consts::FRAC_PI_4),
                moves: vec![],
                radii: vec![],
            },
        )?;
        let (a, b) = (CurveRef::Curve(a), CurveRef::Curve(b));
        edit(s, SKETCH_2, SketchOp::AddConstraint { constraints: vec![ConstraintOf::Concentric(a, b)], label: "Add concentric" })?;
        edit(s, SKETCH_2, SketchOp::AddConstraint { constraints: vec![ConstraintOf::Equal(a, b)], label: "Add equal" })?;
    }
    Ok(())
}

fn circle_ids(g: &cadrs_sketch::Sketch) -> Vec<cadrs_sketch::CurveId> {
    g.curves.iter().filter(|(_, c)| matches!(c.kind, cadrs_sketch::CurveKind::Circle { .. })).map(|(k, _)| k).collect()
}

fn sketch_of(s: &dyn Studio, el: ElementId, sketch: FeatureId) -> Result<cadrs_sketch::Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

fn extrude(s: &mut dyn Studio, el: ElementId, sketch: FeatureId, feature: FeatureId, seed: Vec2, depth: f64) -> Result<(), CommandError> {
    let g = sketch_of(s, el, sketch)?;
    let regions = super::region_refs(sketch, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the stand-in is missing".into()));
    }
    let e = ExtrudeFeature { op: BooleanOp::New, ..super::extrude_of(regions, depth) };
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

//! A bracket with a long feature list (P3.9, PS3): the course's filter and folder lessons work
//! on a Part Studio of 43 features that isn't public, so their scenarios use this one, built
//! from our own features (mm):
//!
//! - **Sketch 1 / Extrude 1**: a 120 × 80 plate on Top (x −60..60, y −40..40), 10 mm;
//! - **Sketch 2 / Extrude 2 … Extrude 11**: ten Ø12 bosses on the plate's top (x −40..40 by
//!   20, y ±18), one extrude each (Add, from a starting offset of 10 mm), 5 to 14 mm high; these
//!   eleven sit in a closed folder, "Bosses";
//! - **Fillet 1**: R5 on the plate's four vertical corner edges;
//! - **Fillet 2**: R1 on the plate's top rim (one edge picked, its tangent chain rounded).
//!
//! So "Extrude 1" (unquoted) matches Extrude 1, 10 and 11, and `"Extrude 1"` only the first.
//! Every id is fixed.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use super::gear_cover::Studio;
use crate::applied::{EdgeOrFace, FilletFeature};
use crate::command::CommandError;
use crate::commands::{AddExtrude, AddFeature, AddSketch, CreateFolder, EditSketch, RenamePart, SetExtrude};
use crate::document::{BooleanOp, EdgeRef, ExtrudeFeature, Offset};
use crate::ids::{ElementId, FeatureId, PartId};

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0xb7ac_4e70_0000_0000_0000_0000_0000_0000 | n)
}

pub const SKETCH_1: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const SKETCH_2: FeatureId = id(3);
/// Extrude 2 … Extrude 11.
pub const BOSSES: [FeatureId; 10] = [id(10), id(11), id(12), id(13), id(14), id(15), id(16), id(17), id(18), id(19)];
pub const FILLET_1: FeatureId = id(4);
pub const FILLET_2: FeatureId = id(5);
pub const FOLDER: FeatureId = id(6);
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

pub const HALF_X: f64 = 60.0;
pub const HALF_Y: f64 = 40.0;
pub const PLATE: f64 = 10.0;
pub const BOSS_R: f64 = 6.0;

/// Where the bosses go, in order (Extrude 2 first), and their heights above the plate.
pub fn bosses() -> Vec<(Vec2, f64)> {
    let mut out = Vec::new();
    for (k, (x, y)) in [-40.0, -20.0, 0.0, 20.0, 40.0].iter().flat_map(|x| [(*x, -18.0), (*x, 18.0)]).enumerate() {
        out.push((Vec2::new(x, y), 5.0 + k as f64));
    }
    out
}

/// Adds the bracket to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_1,
        op: SketchOp::AddPolyline {
            points: vec![v(-HALF_X, -HALF_Y), v(HALF_X, -HALF_Y), v(HALF_X, HALF_Y), v(-HALF_X, HALF_Y)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    extrude(s, el, SKETCH_1, EXTRUDE_1, v(0.0, 0.0), |e| {
        e.depth = PLATE;
        e.depth_expr = "10 mm".into();
    })?;
    s.run(&AddSketch { element: el, feature: SKETCH_2, plane: Some(PlaneRef::Top) })?;
    for (c, _) in bosses() {
        s.run(&EditSketch { element: el, feature: SKETCH_2, op: SketchOp::AddCircle { center: c, radius: BOSS_R, construction: false } })?;
    }
    for ((c, h), f) in bosses().into_iter().zip(BOSSES) {
        extrude(s, el, SKETCH_2, f, c, |e| {
            e.op = BooleanOp::Add;
            e.depth = h;
            e.depth_expr = format!("{h} mm");
            e.start_offset = Some(Offset { value: PLATE, expr: "10 mm".into(), flip: false });
        })?;
    }
    // The fillets, on the edges as they are built.
    let corners: Vec<[f64; 3]> =
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].iter().map(|(x, y)| [x * HALF_X, y * HALF_Y, PLATE / 2.0]).collect();
    let entities = edges(s, el, &corners)?;
    s.run(&AddFeature::fillet(el, FILLET_1, FilletFeature { entities, size: 5.0, size_expr: "5 mm".into(), ..FilletFeature::default() }))?;
    let entities = edges(s, el, &[[0.0, -HALF_Y, PLATE]])?;
    s.run(&AddFeature::fillet(el, FILLET_2, FilletFeature { entities, size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() }))?;
    s.run(&RenamePart { element: el, part: PART, name: "Bracket".into() })?;
    let mut folder = vec![SKETCH_2];
    folder.extend(BOSSES);
    s.run(&CreateFolder { element: el, folder: FOLDER, name: Some("Bosses".into()), features: folder })?;
    Ok(())
}

/// The part's edges nearest `points` (each within 1 µm).
fn edges(s: &dyn Studio, el: ElementId, points: &[[f64; 3]]) -> Result<Vec<EdgeOrFace>, CommandError> {
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let build = crate::rebuild::build(&features);
    let part = build.part(PART).ok_or_else(|| CommandError::Invalid("the bracket didn't build".into()))?;
    points
        .iter()
        .map(|p| {
            let e = part
                .solid
                .edges
                .iter()
                .min_by(|a, b| a.distance(*p).total_cmp(&b.distance(*p)))
                .filter(|e| e.distance(*p) < 1e-3)
                .ok_or_else(|| CommandError::Invalid(format!("no edge at {p:?}")))?;
            Ok(EdgeOrFace::Edge(EdgeRef { part: part.id, edge: e.name, seed: *p }))
        })
        .collect()
}

fn extrude(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    feature: FeatureId,
    seed: Vec2,
    set: impl FnOnce(&mut ExtrudeFeature),
) -> Result<(), CommandError> {
    let g = s
        .document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?;
    let regions = super::region_refs(sketch, &g, &[seed]);
    if regions.len() != 1 {
        return Err(CommandError::Invalid("a region of the bracket is missing".into()));
    }
    let mut e = super::extrude_of(regions, 10.0);
    set(&mut e);
    s.run(&AddExtrude { element: el, feature, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude { element: el, feature, extrude: e, label: "Extrude".into() })?;
    Ok(())
}

/// A row of `n` separate 10 mm blocks along x, each its own sketch on Top and New extrude: a
/// Part Studio with a long feature list (2n features) and Parts list (n parts), for scrolling.
pub fn blocks_in(s: &mut dyn Studio, el: ElementId, n: usize) -> Result<(), CommandError> {
    let v = Vec2::new;
    for k in 0..n {
        let (sketch, feature) = (id(0x1000 + 2 * k as u128), id(0x1001 + 2 * k as u128));
        let x = k as f64 * 15.0;
        s.run(&AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) })?;
        s.run(&EditSketch {
            element: el,
            feature: sketch,
            op: SketchOp::AddPolyline {
                points: vec![v(x, 0.0), v(x + 10.0, 0.0), v(x + 10.0, 10.0), v(x, 10.0)],
                closed: true,
                construction: false,
                label: "Add rectangle",
            },
        })?;
        extrude(s, el, sketch, feature, v(x + 5.0, 5.0), |e| {
            e.op = BooleanOp::New;
            e.depth = 10.0;
            e.depth_expr = "10 mm".into();
        })?;
    }
    Ok(())
}

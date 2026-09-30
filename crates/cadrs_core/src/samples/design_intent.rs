//! The design-intent model of the parametric CAD course (P3F.4, `intro-to-parametric-cad.md`
//! P5, `course_pcad_design_intent`): a hydraulic cylinder body driven by two variables, built
//! from our own features (mm).
//!
//! - **#piston_d** = 40 mm and **#clearance** = 0.5 mm (Variable features, at the top).
//! - **Master sketch** on Front (sketch x = model X, sketch y = model Z): the body's half
//!   section, a rectangle from the bore to the outside, 100 mm long, its bore dimensioned
//!   diametrally (Ø to the sketch's Y axis) as `#piston_d + #clearance`, the wall 5 mm (so
//!   OD = ID + 10), the top 100 mm above the X axis.
//! - **Body** (Revolve 1): the section revolved a full turn about Z (New).
//! - **Groove sketch** on Front: one O-ring groove, 2 mm deep into the bore wall and 2 mm wide
//!   (20 mm up); its inner side reaches 1 mm into the bore, so the cut never lies on the bore
//!   face: Ø`#piston_d + #clearance - 2 mm`, 3 mm across.
//! - **Groove** (Revolve 2, Remove) and **Groove pattern** (a Linear feature pattern ×3, 15 mm
//!   apart along Z).
//! - **Piston** (Sketch 3 on Top, Ø`#piston_d`; Extrude 1, 30 mm, New).
//! - **Clamp** (Sketch 4 on Top: an 80 × 80 square round the piston's bottom edge, projected
//!   with Use; Extrude 2, 10 mm down, New): its bore follows the piston.
//!
//! The body's volume in closed form, with ID = `#piston_d + #clearance` and OD = ID + 10:
//! V = π/4·(OD² − ID²)·100 − 3·π/4·((ID + 4)² − ID²)·2 = 69 869.021 mm³ at 40 mm and
//! 85 199.993 mm³ at 50 mm ([`body_volume`]). Every id is fixed.

use cadrs_sketch::constraint::{PointRef, rectangle_constraints};
use cadrs_sketch::projection::Projected;
use cadrs_sketch::{CurveRef, Dimension, DimensionKind, Link, PlaneRef, Sketch, SketchOp, Vec2};

use super::gear_cover::Studio;
use crate::command::CommandError;
use crate::commands::{AddExtrude, AddFeature, AddRevolve, AddSketch, EditSketch, RenameFeature, RenamePart, SetExtrude};
use crate::document::{AxisRef, BooleanOp, DirectionRef, ExtrudeFeature, FeatureKind, RevolveFeature, RevolveType};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::mate::{ConnectorOrigin, ConnectorRef};
use crate::pattern::{PatternFeature, PatternKind, PatternType};
use crate::variables::VariableFeature;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0xde51_9e00_0000_0000_0000_0000_0000_0000 | n)
}

pub const VAR_PISTON: FeatureId = id(1);
pub const VAR_CLEARANCE: FeatureId = id(2);
pub const MASTER_SKETCH: FeatureId = id(3);
pub const BODY: FeatureId = id(4);
pub const GROOVE_SKETCH: FeatureId = id(5);
pub const GROOVE: FeatureId = id(6);
pub const GROOVE_PATTERN: FeatureId = id(7);
pub const PISTON_SKETCH: FeatureId = id(8);
pub const PISTON: FeatureId = id(9);
pub const CLAMP_SKETCH: FeatureId = id(10);
pub const CLAMP: FeatureId = id(11);

/// The body, the piston and the clamp.
pub const BODY_PART: PartId = PartId::new(BODY, 0);
pub const PISTON_PART: PartId = PartId::new(PISTON, 0);
pub const CLAMP_PART: PartId = PartId::new(CLAMP, 0);

pub const PISTON_D: f64 = 40.0;
pub const CLEARANCE: f64 = 0.5;
pub const WALL: f64 = 5.0;
pub const LENGTH: f64 = 100.0;
pub const GROOVE_DEPTH: f64 = 2.0;
pub const GROOVE_WIDTH: f64 = 2.0;
pub const GROOVE_Z: f64 = 20.0;
pub const GROOVE_PITCH: f64 = 15.0;
pub const GROOVES: u32 = 3;
pub const PISTON_LENGTH: f64 = 30.0;
pub const CLAMP_SIDE: f64 = 80.0;
pub const CLAMP_THICKNESS: f64 = 10.0;

/// The body's volume for a piston diameter and clearance (mm³), in closed form.
pub fn body_volume(piston_d: f64, clearance: f64) -> f64 {
    let id = piston_d + clearance;
    let od = id + 2.0 * WALL;
    let q = std::f64::consts::FRAC_PI_4;
    q * (od * od - id * id) * LENGTH - GROOVES as f64 * q * ((id + 2.0 * GROOVE_DEPTH).powi(2) - id * id) * GROOVE_WIDTH
}

/// The clamp's volume (mm³): the square less the piston's section, 10 mm thick.
pub fn clamp_volume(piston_d: f64) -> f64 {
    (CLAMP_SIDE * CLAMP_SIDE - std::f64::consts::FRAC_PI_4 * piston_d * piston_d) * CLAMP_THICKNESS
}

fn sketch_of(s: &dyn Studio, el: ElementId, sketch: FeatureId) -> Result<Sketch, CommandError> {
    s.document()
        .element(el)
        .and_then(|e| e.feature(sketch))
        .and_then(|f| f.sketch())
        .map(|s| s.geometry.clone())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))
}

/// A rectangle (with its constraints) and dimensions on its corners, `dims(corners)` given the
/// corners' point ids (from the first corner, counter-clockwise).
fn rect_with(
    s: &mut dyn Studio,
    el: ElementId,
    sketch: FeatureId,
    corners: [Vec2; 4],
    dims: impl Fn([cadrs_sketch::PointId; 4]) -> Vec<(DimensionKind, f64, Option<&'static str>)>,
) -> Result<(), CommandError> {
    let edit = |s: &mut dyn Studio, op: SketchOp| s.run(&EditSketch { element: el, feature: sketch, op });
    edit(s, SketchOp::AddPolyline { points: corners.to_vec(), closed: true, construction: false, label: "Add rectangle" })?;
    edit(s, SketchOp::AddConstraints(rectangle_constraints(corners)))?;
    let g = sketch_of(s, el, sketch)?;
    let mut ids = [cadrs_sketch::PointId::default(); 4];
    for (i, c) in corners.iter().enumerate() {
        ids[i] = g.point_at(*c, 1e-6).ok_or_else(|| CommandError::Invalid("a corner is missing".into()))?;
    }
    for (kind, value, expr) in dims(ids) {
        edit(s, SketchOp::SetDimension { dimension: Dimension::new(kind, value, 8.0), moves: vec![], radii: vec![] })?;
        if let Some(e) = expr {
            let g = sketch_of(s, el, sketch)?;
            let id = g.dimensions.iter().find(|(_, d)| d.kind == kind).map(|(id, _)| id).ok_or_else(|| CommandError::Invalid("a dimension is missing".into()))?;
            edit(s, SketchOp::SetDimensionExpr { id, expr: Some(e.into()) })?;
        }
    }
    Ok(())
}

/// Adds the model to the Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    let z_axis = || Some(AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin)));
    // The variables.
    s.run(&AddFeature::variable(el, VAR_PISTON, VariableFeature {
        description: "Piston diameter".into(),
        ..VariableFeature::length("piston_d", "40 mm")
    }))?;
    s.run(&AddFeature::variable(el, VAR_CLEARANCE, VariableFeature {
        description: "Diametral clearance of the bore".into(),
        ..VariableFeature::length("clearance", "0.5 mm")
    }))?;
    let (ri, ro) = ((PISTON_D + CLEARANCE) / 2.0, (PISTON_D + CLEARANCE) / 2.0 + WALL);
    // The master sketch and the body.
    s.run(&AddSketch { element: el, feature: MASTER_SKETCH, plane: Some(PlaneRef::Front) })?;
    s.run(&RenameFeature { element: el, feature: MASTER_SKETCH, name: "Master sketch".into() })?;
    rect_with(s, el, MASTER_SKETCH, [v(ri, 0.0), v(ro, 0.0), v(ro, LENGTH), v(ri, LENGTH)], |p| {
        vec![
            (DimensionKind::Diametral { p: PointRef::Point(p[0]), line: CurveRef::YAxis }, 2.0 * ri, Some("#piston_d + #clearance")),
            (DimensionKind::Horizontal { a: p[0], b: p[1] }, WALL, None),
            (DimensionKind::Vertical { a: p[1], b: p[2] }, LENGTH, None),
            (DimensionKind::PointLine { p: PointRef::Point(p[2]), line: CurveRef::XAxis }, LENGTH, None),
        ]
    })?;
    let g = sketch_of(s, el, MASTER_SKETCH)?;
    let regions = super::region_refs(MASTER_SKETCH, &g, &[v((ri + ro) / 2.0, LENGTH / 2.0)]);
    s.run(&AddRevolve {
        element: el,
        feature: BODY,
        revolve: RevolveFeature { regions, axis: z_axis(), kind: RevolveType::Full, op: BooleanOp::New, ..RevolveFeature::default() },
    })?;
    s.run(&RenameFeature { element: el, feature: BODY, name: "Body".into() })?;
    // One groove, cut, then patterned.
    let (gi, go) = (ri - 1.0, ri + GROOVE_DEPTH);
    s.run(&AddSketch { element: el, feature: GROOVE_SKETCH, plane: Some(PlaneRef::Front) })?;
    s.run(&RenameFeature { element: el, feature: GROOVE_SKETCH, name: "Groove sketch".into() })?;
    rect_with(s, el, GROOVE_SKETCH, [v(gi, GROOVE_Z), v(go, GROOVE_Z), v(go, GROOVE_Z + GROOVE_WIDTH), v(gi, GROOVE_Z + GROOVE_WIDTH)], |p| {
        vec![
            (DimensionKind::Diametral { p: PointRef::Point(p[0]), line: CurveRef::YAxis }, 2.0 * gi, Some("#piston_d + #clearance - 2 mm")),
            (DimensionKind::Horizontal { a: p[0], b: p[1] }, go - gi, None),
            (DimensionKind::Vertical { a: p[1], b: p[2] }, GROOVE_WIDTH, None),
            (DimensionKind::PointLine { p: PointRef::Point(p[0]), line: CurveRef::XAxis }, GROOVE_Z, None),
        ]
    })?;
    let g = sketch_of(s, el, GROOVE_SKETCH)?;
    let regions = super::region_refs(GROOVE_SKETCH, &g, &[v((gi + go) / 2.0, GROOVE_Z + GROOVE_WIDTH / 2.0)]);
    s.run(&AddRevolve {
        element: el,
        feature: GROOVE,
        revolve: RevolveFeature {
            regions,
            axis: z_axis(),
            kind: RevolveType::Full,
            op: BooleanOp::Remove,
            merge_all: true,
            ..RevolveFeature::default()
        },
    })?;
    s.run(&RenameFeature { element: el, feature: GROOVE, name: "Groove".into() })?;
    let mut p = PatternFeature::new(PatternKind::Linear);
    p.pattern_type = PatternType::Feature;
    p.features = vec![GROOVE];
    p.first.direction = Some(DirectionRef::PlaneNormal(PlaneRef::Top));
    p.first.distance = GROOVE_PITCH;
    p.first.distance_expr = format!("{GROOVE_PITCH} mm");
    p.first.count = GROOVES;
    s.run(&AddFeature { element: el, feature: GROOVE_PATTERN, base_name: "Linear pattern".into(), kind: FeatureKind::Pattern(p) })?;
    // The piston.
    s.run(&AddSketch { element: el, feature: PISTON_SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&RenameFeature { element: el, feature: PISTON_SKETCH, name: "Piston sketch".into() })?;
    let r = PISTON_D / 2.0;
    s.run(&EditSketch { element: el, feature: PISTON_SKETCH, op: SketchOp::AddCircle { center: v(0.0, 0.0), radius: r, construction: false } })?;
    let g = sketch_of(s, el, PISTON_SKETCH)?;
    let circle = g.curves.keys().next().ok_or_else(|| CommandError::Invalid("no piston circle".into()))?;
    let kind = DimensionKind::Diameter { curve: circle };
    s.run(&EditSketch {
        element: el,
        feature: PISTON_SKETCH,
        op: SketchOp::SetDimension { dimension: Dimension::new(kind, PISTON_D, std::f64::consts::FRAC_PI_4), moves: vec![], radii: vec![] },
    })?;
    let g = sketch_of(s, el, PISTON_SKETCH)?;
    let dim = g.dimensions.keys().next().ok_or_else(|| CommandError::Invalid("no piston dimension".into()))?;
    s.run(&EditSketch { element: el, feature: PISTON_SKETCH, op: SketchOp::SetDimensionExpr { id: dim, expr: Some("#piston_d".into()) } })?;
    let regions = super::region_refs(PISTON_SKETCH, &g, &[v(0.0, 0.0)]);
    s.run(&AddExtrude { element: el, feature: PISTON, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude {
        element: el,
        feature: PISTON,
        extrude: ExtrudeFeature { op: BooleanOp::New, ..super::extrude_of(regions, PISTON_LENGTH) },
        label: "Extrude".into(),
    })?;
    s.run(&RenameFeature { element: el, feature: PISTON, name: "Piston".into() })?;
    // The clamp: its bore uses the piston's bottom edge.
    let features = s.document().element(el).map(|e| e.features().to_vec()).unwrap_or_default();
    let build = crate::rebuild::build(&features);
    let piston = build.part(PISTON_PART).ok_or_else(|| CommandError::Invalid("the piston didn't build".into()))?;
    let at = [r, 0.0, 0.0];
    let edge = piston
        .solid
        .edges
        .iter()
        .filter(|e| e.circle.is_some())
        .min_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
        .filter(|e| e.distance(at) < 1e-3)
        .ok_or_else(|| CommandError::Invalid("no bottom edge on the piston".into()))?;
    s.run(&AddSketch { element: el, feature: CLAMP_SKETCH, plane: Some(PlaneRef::Top) })?;
    s.run(&RenameFeature { element: el, feature: CLAMP_SKETCH, name: "Clamp sketch".into() })?;
    let items = vec![(Projected::Circle(v(0.0, 0.0), r), Link::Edge { feature: PISTON.0, edge: edge.name })];
    s.run(&EditSketch { element: el, feature: CLAMP_SKETCH, op: SketchOp::Use { items } })?;
    let h = CLAMP_SIDE / 2.0;
    s.run(&EditSketch {
        element: el,
        feature: CLAMP_SKETCH,
        op: SketchOp::AddPolyline { points: vec![v(-h, -h), v(h, -h), v(h, h), v(-h, h)], closed: true, construction: false, label: "Add rectangle" },
    })?;
    let g = sketch_of(s, el, CLAMP_SKETCH)?;
    let regions = super::region_refs(CLAMP_SKETCH, &g, &[v(h - 2.0, h - 2.0)]);
    s.run(&AddExtrude { element: el, feature: CLAMP, extrude: ExtrudeFeature::default() })?;
    s.run(&SetExtrude {
        element: el,
        feature: CLAMP,
        extrude: ExtrudeFeature { op: BooleanOp::New, flip: true, ..super::extrude_of(regions, CLAMP_THICKNESS) },
        label: "Extrude".into(),
    })?;
    s.run(&RenameFeature { element: el, feature: CLAMP, name: "Clamp".into() })?;
    for (part, name) in [(BODY_PART, "Body"), (PISTON_PART, "Piston"), (CLAMP_PART, "Clamp")] {
        s.run(&RenamePart { element: el, part, name: name.into() })?;
    }
    Ok(())
}

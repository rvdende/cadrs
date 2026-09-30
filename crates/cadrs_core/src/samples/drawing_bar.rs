//! A long bar for the P3C.8 view and annotation scenarios and tests (our own part, in mm):
//!
//! - **Sketch 1 / Extrude 1**: a 200 × 20 rectangle on Top (x 0..200, y −10..10), 10 mm down
//!   (z −10..0), so the holes are drilled from its top face on the Top plane.
//! - **Hole 1**: a Ø6 drilled hole through all at (30, 0).
//! - **Hole 2**: an M6×1 tapped hole through all at (170, 0), tapped 8 mm (Show threads).
//! - **Chamfer 1**: 1 mm equal distance on the two vertical edges at x = 0 (45° corners in the
//!   top view: the chamfer dimension).
//! - **Fillet 1**: R8 on the two vertical edges at x = 200 (quarter arcs in the top view: the
//!   arc length dimension).
//!
//! [`drawing`] makes its drawing: ANSI A in mm, third angle, Front at 1:1 with Top above it and
//! Right beside it, hidden lines on in Front, and the overall length 200 dimensioned in Front.

use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

use crate::applied::{ChamferFeature, ChamferType, EdgeOrFace, FilletFeature, HoleFeature, HolePoint};
use crate::command::CommandError;
use crate::commands::{AddFeature, AddSketch, EditSketch, RenameFeature, RenamePart};
use crate::document::{Document, EdgeRef};
use crate::hole::{HoleEnd, HoleSpec, HoleStandard, HoleType, Length};
use crate::ids::{ElementId, FeatureId, PartId};
use crate::samples::drawing_bracket::extrude;
use crate::samples::gear_cover::Studio;

const fn id(n: u128) -> FeatureId {
    FeatureId::from_u128(0xba40_0000_0000_0000_0000_0000_0000_0000 | n)
}

pub const SKETCH_1: FeatureId = id(1);
pub const EXTRUDE_1: FeatureId = id(2);
pub const SKETCH_2: FeatureId = id(3);
pub const HOLE_1: FeatureId = id(4);
pub const SKETCH_3: FeatureId = id(5);
pub const HOLE_2: FeatureId = id(6);
pub const CHAMFER_1: FeatureId = id(7);
pub const FILLET_1: FeatureId = id(8);
pub const PART: PartId = PartId::new(EXTRUDE_1, 0);

pub const LENGTH: f64 = 200.0;
pub const WIDTH: f64 = 20.0;
pub const THICK: f64 = 10.0;
pub const HOLE_X: f64 = 30.0;
pub const HOLE_D: f64 = 6.0;
pub const TAP_X: f64 = 170.0;
pub const TAP_DEPTH: f64 = 8.0;
pub const CHAMFER: f64 = 1.0;
pub const FILLET_R: f64 = 8.0;

pub const STUDIO_NAME: &str = "Bar";
pub const PART_NAME: &str = "Bar";
pub const DRAWING_NAME: &str = "Bar Drawing";

fn points_of(s: &dyn Studio, el: ElementId, f: FeatureId) -> Result<Vec<HolePoint>, CommandError> {
    let g = &s
        .document()
        .element(el)
        .and_then(|e| e.feature(f))
        .and_then(|f| f.sketch())
        .ok_or_else(|| CommandError::Invalid("sketch not found".into()))?
        .geometry;
    Ok(g.points.keys().map(|point| HolePoint { sketch: f, point }).collect())
}

/// The edge of the built bar nearest `p`.
fn edge_at(s: &dyn Studio, el: ElementId, p: [f64; 3]) -> Result<EdgeRef, CommandError> {
    let features = s
        .document()
        .element(el)
        .map(|e| e.features().to_vec())
        .ok_or_else(|| CommandError::Invalid("studio not found".into()))?;
    let build = crate::rebuild::build(&features);
    let part = build
        .parts
        .iter()
        .find(|q| q.id == PART)
        .ok_or_else(|| CommandError::Invalid(format!("the bar did not build: {:?}", build.errors)))?;
    let e = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .filter(|e| e.distance(p) < 1e-3)
        .ok_or_else(|| CommandError::Invalid(format!("no edge at {p:?}")))?;
    Ok(EdgeRef { part: PART, edge: e.name, seed: p })
}

/// The Ø6 drilled hole's spec.
pub fn hole_spec() -> HoleSpec {
    let mut s = HoleSpec::new("6 mm".into());
    s.end = HoleEnd::ThroughAll;
    s.apply_table();
    s
}

/// The M6 tapped hole's spec.
pub fn tap_spec() -> HoleSpec {
    let mut s = HoleSpec { standard: HoleStandard::Iso, hole_type: HoleType::Tapped, size: "M6".into(), pitch: "M6x1".into(), ..HoleSpec::default() };
    s.end = HoleEnd::ThroughAll;
    s.apply_table();
    s.tapped_depth = Length::mm(TAP_DEPTH);
    s
}

/// Builds the bar in Part Studio `el`.
pub fn build_in(s: &mut dyn Studio, el: ElementId) -> Result<(), CommandError> {
    let v = Vec2::new;
    let h = WIDTH / 2.0;
    s.run(&AddSketch { element: el, feature: SKETCH_1, plane: Some(PlaneRef::Top) })?;
    s.run(&EditSketch {
        element: el,
        feature: SKETCH_1,
        op: SketchOp::AddPolyline {
            points: vec![v(0.0, -h), v(LENGTH, -h), v(LENGTH, h), v(0.0, h)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        },
    })?;
    extrude(s, el, SKETCH_1, EXTRUDE_1, &[v(100.0, 0.0)], |e| {
        e.depth = THICK;
        e.depth_expr = format!("{THICK} mm");
        e.flip = true;
    })?;
    for (sk, hole, x, spec, name) in [
        (SKETCH_2, HOLE_1, HOLE_X, hole_spec(), "Hole points"),
        (SKETCH_3, HOLE_2, TAP_X, tap_spec(), "Tap points"),
    ] {
        s.run(&AddSketch { element: el, feature: sk, plane: Some(PlaneRef::Top) })?;
        s.run(&EditSketch { element: el, feature: sk, op: SketchOp::AddPoint { pos: v(x, 0.0) } })?;
        s.run(&RenameFeature { element: el, feature: sk, name: name.into() })?;
        let points = points_of(s, el, sk)?;
        // Drilled from the top face (z = 0) down.
        s.run(&AddFeature::hole(el, hole, HoleFeature { points, merge_scope: vec![PART], spec, ..HoleFeature::default() }))?;
    }
    let chamfer_edges = vec![
        EdgeOrFace::Edge(edge_at(s, el, [0.0, -h, -THICK / 2.0])?),
        EdgeOrFace::Edge(edge_at(s, el, [0.0, h, -THICK / 2.0])?),
    ];
    s.run(&AddFeature::chamfer(
        el,
        CHAMFER_1,
        ChamferFeature {
            entities: chamfer_edges,
            kind: ChamferType::EqualDistance,
            distance: CHAMFER,
            distance_expr: format!("{CHAMFER} mm"),
            ..ChamferFeature::default()
        },
    ))?;
    let fillet_edges = vec![
        EdgeOrFace::Edge(edge_at(s, el, [LENGTH, -h, -THICK / 2.0])?),
        EdgeOrFace::Edge(edge_at(s, el, [LENGTH, h, -THICK / 2.0])?),
    ];
    s.run(&AddFeature::fillet(
        el,
        FILLET_1,
        FilletFeature { entities: fillet_edges, size: FILLET_R, size_expr: format!("{FILLET_R} mm"), ..FilletFeature::default() },
    ))?;
    s.run(&RenamePart { element: el, part: PART, name: PART_NAME.into() })?;
    Ok(())
}

pub mod views {
    //! The bar's drawing.
    use cadrs_drawing::annotation::{Annotation, AnnotationKind, DimTool, EdgeRef, Pick, Shape, propose, resolve};
    use cadrs_drawing::style::TangentEdges;
    use cadrs_drawing::view::{Placement, projected_view};
    use cadrs_drawing::{Drawing, DrawingOp, NamedView, ObjectRef, Projection, Scale, SheetId, View, ViewId, template};

    use super::*;
    use crate::drawing_source::{StudioState, part_key, source_of};

    pub const SHEET: SheetId = SheetId::from_u128(0xba40_0000_5000_0000_0000_0000_0000_0001);
    pub const FRONT: ViewId = ViewId::from_u128(0xba40_0000_7000_0000_0000_0000_0000_0001);
    pub const TOP: ViewId = ViewId::from_u128(0xba40_0000_7000_0000_0000_0000_0000_0002);
    pub const RIGHT: ViewId = ViewId::from_u128(0xba40_0000_7000_0000_0000_0000_0000_0003);

    /// The model origins of the views on the sheet (mm, 1:1).
    pub const FRONT_AT: [f64; 2] = [30.0, 112.0];
    pub const TOP_AT: [f64; 2] = [30.0, 158.0];
    pub const RIGHT_AT: [f64; 2] = [254.0, 112.0];

    fn bad(what: &str) -> CommandError {
        CommandError::Invalid(format!("Bar drawing: {what}"))
    }

    /// Projects `v` of the bar (tests and the sample).
    pub fn project(features: &[crate::Feature], v: &View) -> Result<std::sync::Arc<crate::views::ViewGeometry>, CommandError> {
        crate::views::project(features, crate::drawing_source::view_request(&StudioState { features: features.to_vec(), ..Default::default() }, v))
            .map_err(|e| bad(&e))
    }

    /// The drawing of the bar in Part Studio `studio` of `doc`.
    pub fn drawing(doc: &Document, studio: ElementId) -> Result<Drawing, CommandError> {
        let t = template::builtin("ANSI_A_MM.dwt").ok_or_else(|| bad("no ANSI_A_MM template"))?;
        let el = doc.element(studio).ok_or_else(|| bad("no studio"))?;
        let state = StudioState::of(el).ok_or_else(|| bad("not a Part Studio"))?;
        let build = crate::rebuild::build(&state.features);
        let source = source_of(studio, &state, &build);
        let part = ObjectRef { element: studio.0, part: part_key(Some(PART)) };
        let hash = source.hash_of(part.part);
        let mut d = Drawing::from_template(&t, None);
        d.sources = vec![source];
        d.sheets[0].id = SHEET;
        let place = |d: &mut Drawing, mut v: View, id: ViewId| -> Result<View, CommandError> {
            v.id = id;
            v.source_hash = hash;
            v.tangent_edges = TangentEdges::Solid;
            d.apply(&DrawingOp::InsertView { sheet: SHEET, view: v.clone() }).map_err(|e| bad(&e))?;
            Ok(v)
        };
        let mut front = View::base(part, NamedView::Front, Scale::new(1, 1), FRONT_AT);
        front.hidden_lines = true;
        let front = place(&mut d, front, FRONT)?;
        let top = place(&mut d, projected_view(&front, Placement::Ortho([0.0, 1.0]), Projection::Third, TOP_AT, None), TOP)?;
        let mut right = projected_view(&front, Placement::Ortho([1.0, 0.0]), Projection::Third, RIGHT_AT, None);
        right.hidden_lines = true;
        place(&mut d, right, RIGHT)?;
        let _ = top;
        // The overall length in Front: its two end faces.
        let g = project(&state.features, &front)?;
        let end = |x: f64| -> Result<EdgeRef, CommandError> {
            g.projection
                .edges
                .iter()
                .map(EdgeRef::of)
                .filter(|r| r.edge.is_some() || r.face.is_some())
                .find(|r| {
                    let s = resolve(&front, &*g, r).shape;
                    matches!(s, Shape::Line { a, b } if (a[0] - x).abs() < 1e-6 && (b[0] - x).abs() < 1e-6 && (a[1] - b[1]).abs() > THICK - 1e-6)
                })
                .ok_or_else(|| bad(&format!("no end at x = {x}")))
        };
        let dim = propose(DimTool::Smart, &front, &*g, &[Pick::Edge(end(0.0)?), Pick::Edge(end(LENGTH)?)], [LENGTH / 2.0, -THICK - 14.0])
            .ok_or_else(|| bad("the length doesn't measure"))?;
        d.apply(&DrawingOp::AddAnnotation { view: FRONT, annotation: Annotation::new(AnnotationKind::Dimension(dim)) })
            .map_err(|e| bad(&e))?;
        Ok(d)
    }
}

/// A new document with the bar ("Bar", in mm) and its drawing tab (tests).
pub fn document() -> Result<(Document, ElementId, ElementId), CommandError> {
    let mut doc = Document::empty("Bar (P3C.8)");
    let el = crate::document::Element::part_studio(STUDIO_NAME);
    let id = el.id;
    doc.elements.push(el);
    let mut h = crate::command::History::default();
    build_in(&mut crate::samples::gear_cover::DocHistory(&mut doc, &mut h), id)?;
    let d = views::drawing(&doc, id)?;
    let del = crate::document::Element::drawing(DRAWING_NAME, d);
    let did = del.id;
    doc.elements.push(del);
    Ok((doc, id, did))
}

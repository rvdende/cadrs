//! Flat pattern drawing views (P3I.7, SM16) through the document: a sheet metal box's flat in a
//! drawing, its bend notes, its DXF export and its update when the model changes.
#![cfg(feature = "occt")]

use std::collections::HashMap;

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::sheetmetal::{SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::{ElementId, FeatureId, History, PartId, rebuild};
use cadrs_drawing::annotation::ViewModel;
use cadrs_drawing::export::{Item, Layer, PageContext, ViewInput, sheet_page};
use cadrs_drawing::{Drawing, NamedView, ObjectRef, Scale, View};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
    extrude: FeatureId,
    sketch: FeatureId,
}

/// A 100 × 60 × 40 block converted into an open box: its bottom edges bent, its top left out
/// (2 mm sheet, R3 bends).
fn open_box() -> (Studio, PartId) {
    let mut d = Document::new("Flat drawing");
    let el = d.elements[0].id;
    let mut h = History::default();
    let sketch = FeatureId::new();
    h.execute(&mut d, &AddSketch { element: el, feature: sketch, plane: Some(PlaneRef::Top) }).unwrap();
    let points = vec![Vec2::new(0.0, 0.0), Vec2::new(100.0, 0.0), Vec2::new(100.0, 60.0), Vec2::new(0.0, 60.0)];
    h.execute(&mut d, &EditSketch { element: el, feature: sketch, op: SketchOp::AddPolyline { points, closed: true, construction: false, label: "rect" } })
        .unwrap();
    let extrude = FeatureId::new();
    h.execute(&mut d, &AddExtrude { element: el, feature: extrude, extrude: ExtrudeFeature::default() }).unwrap();
    let e = ExtrudeFeature { sketches: vec![sketch], depth: 40.0, depth_expr: "40 mm".into(), ..Default::default() };
    h.execute(&mut d, &SetExtrude { element: el, feature: extrude, extrude: e, label: "Extrude".into() }).unwrap();
    let b = rebuild::build(d.element(el).unwrap().features());
    let block = b.parts[0].clone();
    let edge = |p: [f64; 3]| {
        let e = block.solid.edges.iter().min_by(|a, c| a.distance(p).total_cmp(&c.distance(p))).unwrap();
        EdgeOrFace::Edge(EdgeRef { part: block.id, edge: e.name, seed: p })
    };
    let top = block
        .solid
        .faces
        .iter()
        .filter(|f| f.center.is_some_and(|c| (c[2] - 40.0).abs() < 1e-6))
        .map(|f| FaceRef { part: block.id, face: f.name, seed: f.center.unwrap() })
        .next()
        .unwrap();
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 2.0;
    p.bend_radius = 3.0;
    let x = SheetMetalModelFeature {
        operation: SheetMetalOp::Convert,
        parts: vec![block.id],
        exclude: vec![top],
        bends: [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]].into_iter().map(edge).collect(),
        params: p,
        exprs: SheetMetalExprs::of(&p),
        ..Default::default()
    };
    h.execute(&mut d, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(x) })
        .unwrap();
    let b = rebuild::build(d.element(el).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let part = cadrs_core::flat_drawing::flat_parts(&b.sheet_metal)[0];
    (Studio { d, h, el, extrude, sketch }, part)
}

fn drawing_of(st: &Studio, part: PartId) -> (Drawing, View) {
    let r = ObjectRef { element: st.el.0, part: Some((part.feature.0, part.index)) };
    let mut d = Drawing::from_template(&cadrs_drawing::template::builtin("ANSI_A_MM.dwt").unwrap(), Some(r));
    let mut v = View::flat_pattern(r, NamedView::Top, Scale::new(1, 2), [140.0, 110.0]);
    let src = cadrs_core::drawing_source::live_source(&st.d, st.el).unwrap();
    v.source_hash = src.hash_of(r.part);
    d.sources.push(src);
    d.sheets[0].views.push(v.clone());
    (d, v)
}

#[test]
fn a_flat_view_shows_the_flat_its_notes_exports_and_updates() {
    let (mut st, part) = open_box();
    let (mut d, v) = drawing_of(&st, part);
    let g = cadrs_core::drawing_export::view_geometry(&st.d, &d, &v).unwrap();
    let flat = g.flat.as_ref().expect("a flat view's bends");
    assert!(flat.face_on);
    assert_eq!(flat.bends.len(), 4);
    // The flat's width: the base 100 − 2·(R + T) = 90 plus two bend allowances and two walls.
    let (lo, hi) = g.bounds.unwrap();
    let b = rebuild::build(st.d.element(st.el).unwrap().features());
    let ctx = cadrs_core::flat_drawing::context_of(&b.sheet_metal, part).unwrap();
    let (flo, fhi) = ctx.flat.parts[0].bounds().unwrap();
    assert!(((hi[0] - lo[0]) - (fhi.x - flo.x)).abs() < 1e-9);
    // Closed form: the faces are the sheet's inside, so the base is 100 − 2R and each wall
    // 40 − R from its bend's tangent line, plus two 90° bend allowances π/2 · (R + K·T).
    let (t, r, k) = (2.0, 3.0, cadrs_core::sheetmetal::SheetMetalModelFeature::default_params().k_factor);
    let ba = std::f64::consts::FRAC_PI_2 * (r + k * t);
    for (got, base) in [(hi[0] - lo[0], 100.0), (hi[1] - lo[1], 60.0)] {
        let want = (base - 2.0 * r) + 2.0 * (40.0 - r) + 2.0 * ba;
        assert!((got - want).abs() < 1e-3, "{got} vs {want}");
    }
    // Notes: "DOWN 90.0° R3" (the box's material is outside, so its walls bend down).
    let notes = cadrs_drawing::flat_view::bend_notes(&d.style, &v, flat);
    assert_eq!(notes.len(), 4);
    for n in &notes {
        assert!(n.text.text == "DOWN 90.0° R3" || n.text.text == "UP 90.0° R3", "{}", n.text.text);
    }
    // Every edge has a name the model data resolves (dimensions measure them).
    for e in &g.projection.edges {
        let name = e.source.unwrap().edge_name.unwrap();
        assert!(g.model_edge(&name).is_some());
    }
    // The DXF has the bend lines on their layer and the notes as text.
    let r = cadrs_drawing::ReferenceProps::default();
    let f = cadrs_drawing::rich::DrawingContext::default();
    let mut views = HashMap::new();
    views.insert(v.id, ViewInput { model: &*g, shaded: Vec::new(), sketches: Vec::new() });
    let page = sheet_page(&d, 0, &PageContext { reference: &r, fields: &f, views });
    let bend_lines = page.items.iter().filter(|i| matches!(i, Item::Stroke(_, p) if matches!(p.layer, Layer::BendUp | Layer::BendDown))).count();
    assert_eq!(bend_lines, flat.lines.iter().map(|(_, l)| l.len()).sum::<usize>());
    let dxf = cadrs_drawing::dxf::write_dxf(&page);
    assert!(dxf.contains("BEND_DOWN") || dxf.contains("BEND_UP"));
    assert!(dxf.contains("90.0%%d R3"));
    // The model changes (the walls 10 mm taller): the view is out of date until updated, then
    // the flat is 20 mm longer.
    assert!(cadrs_core::drawing_source::out_of_date(&st.d, &d).is_empty());
    let e = ExtrudeFeature { sketches: vec![st.sketch], depth: 50.0, depth_expr: "50 mm".into(), ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: st.extrude, extrude: e, label: "Extrude".into() }).unwrap();
    assert_eq!(cadrs_core::drawing_source::out_of_date(&st.d, &d), vec![v.id]);
    let op = cadrs_core::drawing_source::update_now(&st.d, &d).expect("an update");
    d.apply(&op).unwrap();
    assert!(cadrs_core::drawing_source::out_of_date(&st.d, &d).is_empty());
    let g2 = cadrs_core::drawing_export::view_geometry(&st.d, &d, &d.sheets[0].views[0]).unwrap();
    let (lo2, hi2) = g2.bounds.unwrap();
    assert!(((hi2[1] - lo2[1]) - (hi[1] - lo[1]) - 20.0).abs() < 1e-6, "{} vs {}", hi2[1] - lo2[1], hi[1] - lo[1]);
}

/// SM16.3: a plate with two louvers (Form) and a counterbored hole through it: the flat view
/// shows the louvers' outlines, the hole and its counterbore's outer diameter, and a centermark
/// at each.
#[test]
fn forms_holes_and_counterbores_in_a_flat_view() {
    use cadrs_core::applied::{HoleFeature, HolePoint};
    use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, HoleStyle, Length};
    use cadrs_core::sheetmetal_form::{FormFeature, FormLocation, FormPick, FormSource, LIBRARY_NAME, LibraryForm};
    use cadrs_drawing::flat_view::{FlatEdgeKind, kind_of};
    let mut d = Document::new("Flat drawing forms");
    let el = d.elements[0].id;
    let mut h = History::default();
    let sketch = |d: &mut Document, h: &mut History, ops: Vec<SketchOp>| {
        let f = FeatureId::new();
        h.execute(d, &AddSketch { element: el, feature: f, plane: Some(PlaneRef::Top) }).unwrap();
        h.execute(d, &EditSketch { element: el, feature: f, op: SketchOp::Batch(ops) }).unwrap();
        f
    };
    let rect = vec![Vec2::new(0.0, 0.0), Vec2::new(120.0, 0.0), Vec2::new(120.0, 80.0), Vec2::new(0.0, 80.0)];
    let s = sketch(&mut d, &mut h, vec![SketchOp::AddPolyline { points: rect, closed: true, construction: false, label: "rect" }]);
    let g = d.element(el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let region = cadrs_core::samples::region_refs(s, &g, &[Vec2::new(60.0, 40.0)]).pop().unwrap();
    let mut p = SheetMetalModelFeature::default_params();
    p.thickness = 1.5;
    p.bend_radius = 2.0;
    let sm = SheetMetalModelFeature { operation: SheetMetalOp::Thicken, regions: vec![region], params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    let model = FeatureId::new();
    h.execute(&mut d, &AddFeature { element: el, feature: model, base_name: "Sheet metal model".into(), kind: FeatureKind::SheetMetalModel(sm) }).unwrap();
    let b = rebuild::build(d.element(el).unwrap().features());
    let part = b.parts.iter().find(|q| q.id.feature == model).unwrap().clone();
    let top = {
        let f = part.solid.faces.iter().find(|f| f.center.is_some_and(|c| (c[2] - 1.5).abs() < 1e-6)).unwrap();
        FaceRef { part: part.id, face: f.name, seed: f.center.unwrap() }
    };
    // Two louvers.
    let pts = sketch(&mut d, &mut h, [(30.0, 25.0), (30.0, 55.0)].iter().map(|(x, y)| SketchOp::AddPoint { pos: Vec2::new(*x, *y) }).collect());
    let form = FormFeature {
        form: Some(FormPick { source: FormSource::Library(LibraryForm::Louver), name: "Louver".into(), document_name: LIBRARY_NAME.into(), studio: vec![] }),
        variables: LibraryForm::Louver.variables(),
        locations: vec![FormLocation::SketchPoints(pts)],
        targets: vec![top],
        flip: false,
    };
    h.execute(&mut d, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Form".into(), kind: FeatureKind::Form(form) }).unwrap();
    // A Ø6.6 hole counterbored Ø11, through all.
    let hp = sketch(&mut d, &mut h, vec![SketchOp::AddPoint { pos: Vec2::new(90.0, 40.0) }]);
    let point = d.element(el).unwrap().feature(hp).unwrap().sketch().unwrap().geometry.points.keys().next().unwrap();
    let spec = HoleSpec {
        style: HoleStyle::Counterbore,
        diameter: Length::mm(6.6),
        cbore_diameter: Length::mm(11.0),
        cbore_depth: Length::mm(0.5),
        end: HoleEnd::ThroughAll,
        start: HoleStart::Part,
        ..HoleSpec::default()
    };
    let hole = HoleFeature { points: vec![HolePoint { sketch: hp, point }], spec, ..HoleFeature::default() };
    h.execute(&mut d, &AddFeature { element: el, feature: FeatureId::new(), base_name: "Hole".into(), kind: FeatureKind::Hole(hole) }).unwrap();
    let b = rebuild::build(d.element(el).unwrap().features());
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    let ctx = cadrs_core::flat_drawing::context_of(&b.sheet_metal, part.id).unwrap();
    assert_eq!(ctx.hole_marks.len(), 1);
    // The hole is in the flat (the sheet's area less the hole's).
    let area = ctx.flat.parts[0].area();
    assert!(area < 120.0 * 80.0 - 30.0, "{area}");
    let st = Studio { d, h, el, extrude: FeatureId::new(), sketch: FeatureId::new() };
    let (dr, v) = drawing_of(&st, part.id);
    let g = cadrs_core::drawing_export::view_geometry(&st.d, &dr, &v).unwrap();
    let kind = |e: &cadrs_kernel::ProjEdge| kind_of(e.source.as_ref().unwrap().edge_name.as_ref().unwrap());
    let forms = g.projection.edges.iter().filter(|e| kind(e) == Some(FlatEdgeKind::Form)).count();
    assert!(forms >= 2, "{forms} form edges");
    let circle = |k: FlatEdgeKind| {
        g.projection.edges.iter().filter(|e| kind(e) == Some(k)).find_map(|e| match e.curve {
            cadrs_kernel::ProjCurve::Arc { radius, full: true, center, .. } => Some((radius, center)),
            _ => None,
        })
    };
    let (hr, hc) = circle(FlatEdgeKind::Hole).expect("the hole");
    let (mr, mc) = circle(FlatEdgeKind::HoleMark).expect("the counterbore");
    assert!((hr - 3.3).abs() < 1e-6 && (mr - 5.5).abs() < 1e-9, "{hr} {mr}");
    assert!((hc - mc).norm() < 1e-6);
    // Centermarks: the two louvers and the hole.
    let flat = g.flat.as_ref().unwrap();
    assert_eq!(flat.centers.len(), 3);
    assert_eq!(cadrs_drawing::flat_view::centermarks(&dr.style, &v, flat).len(), 6);
    // Every edge resolves (dimensions measure them), the counterbore as a circle.
    for e in &g.projection.edges {
        assert!(g.model_edge(&e.source.unwrap().edge_name.unwrap()).is_some());
    }
}

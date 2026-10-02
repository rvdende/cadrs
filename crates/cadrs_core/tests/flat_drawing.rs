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

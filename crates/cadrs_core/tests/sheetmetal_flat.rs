//! Modelling in the flat (P3I.6, SM14) through the document's commands: a sketch on the flat
//! pattern plane, extruded Remove across a bend (the cut keeps its exact flat size and the folded
//! part loses exactly that material) and Add (a tab appears folded); an ordinary Extrude of a
//! flat-pattern sketch fails (SM14.3).
#![cfg(feature = "occt")]

use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{Document, ExtrudeFeature, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_flat::{FlatExtrudeFeature, MODEL_SPACE, flat_plane_id};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::Params;
use cadrs_sheetmetal::flat::PieceSource;
use cadrs_sheetmetal::poly::{P2, perp};
use cadrs_sketch::{FeaturePlane, PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Flat");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind }).unwrap();
        feature
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn ok(&self) -> std::sync::Arc<Build> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    /// A U channel: a sheet metal Extrude of an open U (30 up, 60 across, 30 up) on Front, 100
    /// deep, 2 thick, bend radius 3.
    fn channel(&mut self) -> FeatureId {
        let s = self.sketch(PlaneRef::Front, vec![poly(&[(0.0, 30.0), (0.0, 0.0), (60.0, 0.0), (60.0, 30.0)], false)]);
        let p = Params { thickness: 2.0, bend_radius: 3.0, ..SheetMetalModelFeature::default_params() };
        let x = SheetMetalModelFeature {
            operation: SheetMetalOp::Extrude,
            sketches: vec![s],
            depth: 100.0,
            depth_expr: "100 mm".into(),
            params: p,
            exprs: cadrs_core::sheetmetal::SheetMetalExprs::of(&p),
            ..Default::default()
        };
        self.add("Sheet metal model", FeatureKind::SheetMetalModel(x))
    }

    /// A sketch on the model's first flat part (its plane as the rebuild has it).
    fn flat_sketch(&mut self, model: FeatureId, ops: Vec<SketchOp>) -> FeatureId {
        let b = self.ok();
        let id = flat_plane_id(model, 0);
        let frame = *b.planes.get(&FeatureId(id)).expect("the flat pattern plane");
        self.sketch(PlaneRef::Feature(FeaturePlane::new(id, frame)), ops)
    }
}

fn poly(pts: &[(f64, f64)], closed: bool) -> SketchOp {
    SketchOp::AddPolyline { points: pts.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed, construction: false, label: "Add line" }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

/// The folded volume the flat predicts: walls' flat area × T, bend regions scaled from the neutral
/// to the mid-thickness radius.
fn predicted(b: &Build) -> f64 {
    let ctx = &b.sheet_metal[0];
    let p = &ctx.model.params;
    let t = p.thickness;
    ctx.flat.parts[0]
        .pieces
        .iter()
        .map(|piece| {
            let area: f64 = piece.cut.iter().map(|c| c.area()).sum();
            match piece.source {
                PieceSource::Wall(_) => area * t,
                PieceSource::Bend(j) => {
                    let bend = ctx.model.joint(j).unwrap().bend().unwrap();
                    area * t * (bend.radius + t / 2.0) / (bend.radius + p.k_factor * t)
                }
            }
        })
        .sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

#[test]
fn a_flat_cut_across_a_bend_keeps_its_flat_size_folded() {
    let mut st = Studio::new();
    let model = st.channel();
    let b = st.ok();
    let v0 = volume(&b.parts[0]);
    let area0 = b.sheet_metal[0].flat.parts[0].area();
    // The lesson's slot: 0.5 × 6.0 in (12.7 × 152.4 mm would be longer than the part: 12.7 × 50
    // here), across the first bend, square to it, centred on its centreline.
    let bend = b.sheet_metal[0].flat.parts[0].bends[0].clone();
    let c = P2::from((bend.center.a.coords + bend.center.b.coords) / 2.0);
    let (d, n) = (bend.center.dir(), perp(bend.center.dir()));
    let (w, l) = (12.7, 50.0);
    let corners = [c - d * (w / 2.0) - n * (l / 2.0), c + d * (w / 2.0) - n * (l / 2.0), c + d * (w / 2.0) + n * (l / 2.0), c - d * (w / 2.0) + n * (l / 2.0)];
    let s = st.flat_sketch(model, vec![poly(&corners.map(|q| (q.x, q.y)), true)]);
    st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: true, sketches: vec![s], ..Default::default() }));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    let flat = &b.sheet_metal[0].flat.parts[0];
    // The flat loses exactly the slot, as one hole w × l.
    assert!((area0 - flat.area() - w * l).abs() < 1e-6, "{}", area0 - flat.area());
    assert_eq!(flat.outline.len(), 1);
    let holes: Vec<&Vec<P2>> = flat.outline.iter().flat_map(|o| o.holes.iter()).collect();
    assert_eq!(holes.len(), 1);
    let along: Vec<f64> = holes[0].iter().map(|q| (q - c).dot(&d)).collect();
    let across: Vec<f64> = holes[0].iter().map(|q| (q - c).dot(&n)).collect();
    let span = |v: &[f64]| v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min);
    assert!((span(&along) - w).abs() < 1e-6 && (span(&across) - l).abs() < 1e-6, "{} × {}", span(&along), span(&across));
    // Folded: the part loses that material, wrapped round the bend (walls flat, the bend region
    // at its mid-thickness radius).
    let v1 = volume(&b.parts[0]);
    assert!(close(v1, predicted(&b), 1e-6), "{v1} vs {}", predicted(&b));
    assert!(v0 - v1 > w * l * 2.0 * 0.9, "{v0} → {v1}");
    // The part keeps its id and name.
    assert_eq!(b.parts[0].name, "Part 1");
}

#[test]
fn a_tab_added_in_the_flat_appears_folded() {
    let mut st = Studio::new();
    let model = st.channel();
    let b = st.ok();
    let v0 = volume(&b.parts[0]);
    let flat = &b.sheet_metal[0].flat.parts[0];
    let (lo, hi) = flat.bounds().unwrap();
    // A 20 × 8 tab on the flat's edge at its lowest y, in the middle of x.
    let mx = (lo.x + hi.x) / 2.0;
    let s = st.flat_sketch(model, vec![poly(&[(mx - 10.0, lo.y - 8.0), (mx + 10.0, lo.y - 8.0), (mx + 10.0, lo.y), (mx - 10.0, lo.y)], true)]);
    st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: false, sketches: vec![s], ..Default::default() }));
    let b = st.ok();
    let v1 = volume(&b.parts[0]);
    assert!(close(v1 - v0, 20.0 * 8.0 * 2.0, 1e-6), "{}", v1 - v0);
    assert!(close(v1, predicted(&b), 1e-6));
}

#[test]
fn an_ordinary_extrude_of_a_flat_sketch_fails() {
    let mut st = Studio::new();
    let model = st.channel();
    let (lo, hi) = st.ok().sheet_metal[0].flat.parts[0].bounds().unwrap();
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    let s = st.flat_sketch(model, vec![poly(&[(cx - 5.0, cy - 5.0), (cx + 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), (cx - 5.0, cy + 5.0)], true)]);
    let f = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
    let e = ExtrudeFeature { sketches: vec![s], depth: 5.0, depth_expr: "5 mm".into(), ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
    let b = st.build();
    assert_eq!(b.errors.iter().find(|(id, _)| *id == f).map(|(_, e)| e.as_str()), Some(MODEL_SPACE), "{:?}", b.errors);
    // A flat pattern extrude of the same sketch rebuilds.
    let g = st.add("Extrude", FeatureKind::FlatExtrude(FlatExtrudeFeature { remove: true, sketches: vec![s], ..Default::default() }));
    let b = st.build();
    assert!(!b.errors.iter().any(|(id, _)| *id == g), "{:?}", b.errors);
}

#[test]
fn a_flat_sketch_uses_the_flat_lines_and_they_follow_the_flat() {
    use cadrs_core::sheetmetal_flat::flat_lines;
    use cadrs_sketch::Link;
    use cadrs_sketch::projection::Projected;
    let mut st = Studio::new();
    let model = st.channel();
    let b = st.ok();
    let flat = b.sheet_metal[0].flat.parts[0].clone();
    // The two bend centre lines, and the outline's edge furthest along +x (the free edge of the
    // second wall).
    let lines = flat_lines(&flat);
    let mut items: Vec<(Projected, Link)> = lines.iter().filter(|(_, j)| j.is_some()).map(|((a, b), j)| (Projected::Line(*a, *b), Link::FlatLine { model: model.0, part: 0, bend: *j })).collect();
    assert_eq!(items.len(), 2);
    let ((ea, eb), _) = *lines.iter().filter(|(_, j)| j.is_none()).max_by(|x, y| (x.0.0.x + x.0.1.x).total_cmp(&(y.0.0.x + y.0.1.x))).unwrap();
    items.push((Projected::Line(ea, eb), Link::FlatLine { model: model.0, part: 0, bend: None }));
    let s = st.flat_sketch(model, vec![SketchOp::UseConstruction { items }]);
    let sketch = |st: &Studio| st.features().into_iter().find(|f| f.id == s).unwrap().sketch().unwrap().geometry.clone();
    let segs = |g: &cadrs_sketch::Sketch| -> Vec<(Vec2, Vec2)> {
        g.curves.values().filter_map(|c| match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => Some((g.pos(a), g.pos(b))),
            _ => None,
        }).collect()
    };
    let same = |p: (Vec2, Vec2), q: (Vec2, Vec2)| (p.0.distance(q.0) < 1e-6 && p.1.distance(q.1) < 1e-6) || (p.0.distance(q.1) < 1e-6 && p.1.distance(q.0) < 1e-6);
    let g = sketch(&st);
    assert!(g.broken.is_empty() && g.curves.values().all(|c| c.construction));
    assert_eq!(segs(&g).len(), 3);
    // A larger bend radius: the flat changes and the used lines go with it.
    let mut f = st.features().into_iter().find(|f| f.id == model).unwrap();
    let FeatureKind::SheetMetalModel(x) = &mut f.kind else { unreachable!() };
    x.params.bend_radius = 8.0;
    x.exprs = cadrs_core::sheetmetal::SheetMetalExprs::of(&x.params);
    st.h.execute(&mut st.d, &cadrs_core::commands::ReplaceFeature { element: st.el, feature: f, label: "Edit".into() }).unwrap();
    let b = st.ok();
    let now = flat_lines(&b.sheet_metal[0].flat.parts[0]);
    let g = sketch(&st);
    assert!(g.broken.is_empty(), "{:?}", g.broken);
    let mine = segs(&g);
    for ((a, b), j) in &now {
        if j.is_some() {
            assert!(mine.iter().any(|m| same(*m, (*a, *b))), "bend {j:?} at {a:?}–{b:?} not followed: {mine:?}");
        }
    }
    let ((na, nb), _) = *now.iter().filter(|(_, j)| j.is_none()).max_by(|x, y| (x.0.0.x + x.0.1.x).total_cmp(&(y.0.0.x + y.0.1.x))).unwrap();
    assert!((na.x - ea.x).abs() > 1e-3, "the free edge moved");
    assert!(mine.iter().any(|m| same(*m, (na, nb))), "the free edge not followed: {mine:?}");
}

#[test]
fn visible_flat_sketches_reach_the_dxf_on_their_layer() {
    use cadrs_core::flat_export::{FlatExportOptions, flat_parts, part_page};
    use cadrs_drawing::dxf::{DxfVersion, read_dxf, write_dxf_version};
    use cadrs_drawing::sheet_sketch::Entity;
    let mut st = Studio::new();
    let model = st.channel();
    let (lo, hi) = st.ok().sheet_metal[0].flat.parts[0].bounds().unwrap();
    let (cx, cy) = ((lo.x + hi.x) / 2.0, (lo.y + hi.y) / 2.0);
    // A shown sketch: a 10 × 10 square and a spline; a hidden one: a circle.
    let shown = st.flat_sketch(
        model,
        vec![
            poly(&[(cx - 5.0, cy - 5.0), (cx + 5.0, cy - 5.0), (cx + 5.0, cy + 5.0), (cx - 5.0, cy + 5.0)], true),
            SketchOp::AddSpline { points: vec![Vec2::new(cx - 20.0, cy), Vec2::new(cx - 15.0, cy + 6.0), Vec2::new(cx - 10.0, cy)], periodic: false, start_tangent: None, end_tangent: None, construction: false },
        ],
    );
    let hidden = st.flat_sketch(model, vec![SketchOp::AddCircle { center: Vec2::new(cx, cy + 20.0), radius: 3.0, construction: false }]);
    let _ = hidden;
    let b = st.ok();
    let features = st.features();
    let r = flat_parts(&b).into_iter().next().unwrap();
    let is_shown = |id: FeatureId| id == shown;
    let read = |o: &FlatExportOptions, v: DxfVersion| {
        let page = part_page(&b, &features, &r, o, &is_shown).unwrap();
        let text = write_dxf_version(&page, v);
        (text.clone(), read_dxf(&text).unwrap())
    };
    for v in DxfVersion::WRITTEN {
        let (text, d) = read(&FlatExportOptions::default(), v);
        assert!(text.contains(v.acadver()));
        let on: Vec<&Entity> = d.entities.iter().zip(&d.layers).filter(|(_, l)| *l == "FLAT_SKETCH").map(|(e, _)| e).collect();
        // The square's four lines and the spline as a SPLINE; nothing of the hidden sketch.
        assert_eq!(on.iter().filter(|e| matches!(e, Entity::Line { .. })).count(), 4, "{v:?}");
        assert_eq!(on.iter().filter(|e| matches!(e, Entity::Spline { .. })).count(), 1, "{v:?}");
        assert!(!on.iter().any(|e| matches!(e, Entity::Circle { .. })));
        // The spline reads back through its fit points.
        let Some(Entity::Spline { control, knots, .. }) = on.iter().find(|e| matches!(e, Entity::Spline { .. })) else { unreachable!() };
        assert_eq!(knots.len(), control.len() + 4);
        // The layer table: the flat's own layers in use, none of the drawings'.
        let start = text.find("  2\nLAYER\n").unwrap();
        let table = &text[start..start + text[start..].find("ENDTAB").unwrap()];
        for l in ["OUTLINE", "BEND_UP", "FLAT_SKETCH"] {
            assert!(table.contains(&format!("\n{l}\n")), "{l} missing from the layer table");
        }
        for l in ["BORDER", "VISIBLE", "HIDDEN", "VIEW_SKETCH", "ANNOTATION", "HATCH", "CUTOUTS", "BEND_TANGENT"] {
            assert!(!table.contains(&format!("\n{l}\n")), "{l} in the layer table");
        }
    }
    // Splines as polylines.
    let (_, d) = read(&FlatExportOptions { splines_as_polylines: true, ..Default::default() }, DxfVersion::R2000);
    let on: Vec<&Entity> = d.entities.iter().zip(&d.layers).filter(|(_, l)| *l == "FLAT_SKETCH").map(|(e, _)| e).collect();
    assert!(!on.iter().any(|e| matches!(e, Entity::Spline { .. })));
    assert!(on.iter().any(|e| matches!(e, Entity::Polyline { .. })));
    // Without visible sketches: nothing on the layer.
    let (_, d) = read(&FlatExportOptions { sketches: false, ..Default::default() }, DxfVersion::R2013);
    assert!(!d.layers.iter().any(|l| l == "FLAT_SKETCH"));
}

#[test]
fn a_renamed_part_names_its_flat_export() {
    use cadrs_core::flat_export::{FlatScope, default_file_name, flat_parts_named, scope_parts};
    let mut st = Studio::new();
    st.channel();
    let b = st.ok();
    let part = b.parts[0].id;
    st.h.execute(&mut st.d, &cadrs_core::commands::RenamePart { element: st.el, part, name: "Sheet Metal Box".into() }).unwrap();
    let props = st.d.element(st.el).unwrap().part_props().to_vec();
    let b = st.ok();
    let all = flat_parts_named(&b, &props);
    assert_eq!(all[0].name, "Sheet Metal Box");
    assert_eq!(scope_parts(&b, &props, part, FlatScope::Single)[0].name, "Sheet Metal Box");
    assert_eq!(default_file_name("Doc", &all[0].name), "Doc - Flat pattern of Sheet Metal Box");
}

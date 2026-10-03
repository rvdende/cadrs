//! Sheet metal features combined and reordered (SM1.6): every feature after a Sheet metal model
//! changes one definition (`cadrs_sheetmetal::definition`) and refolds through one pipeline, so
//! a Flange's bend can be modified, a Bend made on a flange and its corners broken, a Tab added
//! after a Hem (and the other way round), a Make joint and a Bend in either order, the table reordered with later features' bends, a form kept through a later
//! refold, and the features moved, undone and redone, with the parts keeping their ids. Folded
//! volumes are checked against the flat pattern as in `tests/sheetmetal.rs`.
#![cfg(feature = "occt")]

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddFeature, AddSketch, EditSketch, MoveFeature};
use cadrs_core::document::{Document, EdgeRef, FaceRef, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{CurveRef, SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_features::{FlangeFeature, HemFeature, MakeJointFeature, MakeJointType, SheetMetalFeature};
use cadrs_core::sheetmetal_form::{FormFeature, FormLocation, FormPick, FormSource, LIBRARY_NAME, LibraryForm};
use cadrs_core::sheetmetal_joint::{PutModifyJoint, SetTableOrder, TableEdit, modify_joint_of, table_edit};
use cadrs_core::sheetmetal_tools::{BendFeature, CornerBreakFeature, LineRef, SheetMetalTool, SmPick, TabFeature};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::model_edit::BendAlignment;
use cadrs_sheetmetal::{JointKind, Params};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Sheet metal combined");
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

    /// A `w` × `h` plate, 2 thick, made by a Thicken of a Top-plane rectangle (material up).
    fn plate(&mut self, w: f64, h: f64) -> (FeatureId, Part) {
        let s = self.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, w, h)]);
        let p = params();
        let x = SheetMetalModelFeature { operation: SheetMetalOp::Thicken, region_sketches: vec![s], params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
        let f = self.add("Sheet metal model", FeatureKind::SheetMetalModel(x));
        let part = self.ok().parts[0].clone();
        (f, part)
    }

    /// A flange of `distance` on the plate edge through `at`.
    fn flange(&mut self, part: &Part, at: [f64; 3], distance: f64) -> FeatureId {
        let e = edge_near(part, at);
        let fl = FlangeFeature { edges: vec![EdgeOrFace::Edge(e)], distance, distance_expr: format!("{distance} mm"), ..Default::default() };
        self.add("Flange", FeatureKind::SheetMetal(SheetMetalFeature::Flange(fl)))
    }

    /// A table edit, as the panel makes it: the joint's Modify joint edited, or a new one after
    /// the model's last editor.
    fn table(&mut self, model: FeatureId, joint: cadrs_sheetmetal::JointId, edit: TableEdit) -> FeatureId {
        let b = self.ok();
        let ctx = b.sheet_metal.iter().find(|c| c.feature == model).expect("context");
        let j = ctx.model.joint(joint).expect("the joint").clone();
        let features = self.features();
        let existing = modify_joint_of(&features, model, joint);
        let prev = existing.and_then(|f| match &f.kind {
            FeatureKind::ModifyJoint(x) => Some(x.clone()),
            _ => None,
        });
        let feature = existing.map_or_else(FeatureId::new, |f| f.id);
        let x = table_edit(model, prev.as_ref(), &j, &ctx.model.params, &edit);
        let label = edit.label(&j.name);
        self.h.execute(&mut self.d, &PutModifyJoint { element: self.el, feature, joint: x, after: ctx.editors.clone(), label }).unwrap();
        feature
    }

    fn position(&self, f: FeatureId) -> usize {
        self.features().iter().position(|x| x.id == f).expect("in the list")
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn params() -> Params {
    Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..SheetMetalModelFeature::default_params() }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn edge_near(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

fn face_near(part: &Part, p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let d = |c: [f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let i = (0..s.faces.len()).min_by(|a, b| d(s.faces[*a].center.unwrap()).total_cmp(&d(s.faces[*b].center.unwrap()))).unwrap();
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.faces[i].center.unwrap() }
}

fn bounds(p: &Part) -> ([f64; 3], [f64; 3]) {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for q in &p.solid.positions {
        for i in 0..3 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    }
    (lo, hi)
}

/// The folded volume the flat pattern predicts for one context part (see `tests/sheetmetal.rs`).
fn predicted(b: &Build, part: &Part) -> f64 {
    use cadrs_sheetmetal::flat::PieceSource;
    let ctx = b.sheet_metal.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part.id)).expect("a sheet metal part");
    let walls = &ctx.parts.iter().find(|(p, _)| *p == part.id).unwrap().1;
    let flat = ctx.flat.parts.iter().find(|f| f.walls.iter().any(|w| walls.contains(w))).expect("its flat");
    let p = &ctx.model.params;
    let t = p.thickness;
    let mut v = 0.0;
    for piece in &flat.pieces {
        let area: f64 = piece.cut.iter().map(|c| c.area()).sum();
        v += match piece.source {
            PieceSource::Wall(_) => area * t,
            PieceSource::Bend(j) => {
                let bend = ctx.model.joint(j).unwrap().bend().unwrap();
                let k = match bend.value_or_model(p) {
                    cadrs_sheetmetal::BendValue::KFactor(k) => k,
                    _ => p.k_factor,
                };
                area * t * (bend.radius + t / 2.0) / (bend.radius + k * t)
            }
        };
    }
    v
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

fn all_match(b: &Build) {
    for p in &b.parts {
        assert!(close(volume(p), predicted(b, p), 1e-6), "{}: {} vs {}", p.name, volume(p), predicted(b, p));
    }
}

fn bends(b: &Build) -> Vec<(cadrs_sheetmetal::JointId, f64)> {
    b.sheet_metal[0].model.joints.iter().filter_map(|j| Some((j.id, j.bend()?.radius))).collect()
}

#[test]
fn a_flange_then_a_modify_joint_on_its_bend_then_a_rip_undone_and_redone() {
    let mut st = Studio::new();
    let (model, plate) = st.plate(100.0, 60.0);
    let flange = st.flange(&plate, [100.0, 30.0, 2.0], 30.0);
    let b = st.ok();
    let [(bend, r)] = bends(&b)[..] else { panic!("one bend: {:?}", bends(&b)) };
    assert_eq!(r, 3.0);
    let v0 = volume(&b.parts[0]);
    // The table's radius edit: a Modify joint after the Flange that made the bend.
    let mj = st.table(model, bend, TableEdit::Radius(6.0, "6 mm".into()));
    assert_eq!(st.position(mj), st.position(flange) + 1, "after the Flange");
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    assert_eq!(b.parts[0].id, plate.id, "the part keeps its id");
    assert_eq!(bends(&b), vec![(bend, 6.0)], "the flange is still there, its bend now R6");
    assert_eq!(b.sheet_metal[0].model.walls.len(), 2);
    all_match(&b);
    assert!((volume(&b.parts[0]) - v0).abs() > 1.0);
    // Made a rip: the flange comes off as a part of its own; the plate keeps its id.
    st.table(model, bend, TableEdit::ConvertToRip);
    let b = st.ok();
    assert_eq!(b.parts.len(), 2);
    assert!(b.parts.iter().any(|p| p.id == plate.id));
    assert!(matches!(b.sheet_metal[0].model.joint(bend).unwrap().kind, JointKind::Rip { .. }));
    all_match(&b);
    // Undo: the bend again (R6), then R3; redo: R6.
    st.h.undo(&mut st.d).unwrap();
    let b = st.ok();
    assert_eq!((b.parts.len(), bends(&b)), (1, vec![(bend, 6.0)]));
    st.h.undo(&mut st.d).unwrap();
    let b = st.ok();
    assert_eq!(bends(&b), vec![(bend, 3.0)]);
    assert!(close(volume(&b.parts[0]), v0, 1e-9));
    st.h.redo(&mut st.d).unwrap();
    assert_eq!(bends(&st.ok()), vec![(bend, 6.0)]);
}

#[test]
fn a_bend_on_a_flange_then_a_corner_break_and_a_modify_joint_on_the_bend() {
    let mut st = Studio::new();
    let (model, plate) = st.plate(100.0, 60.0);
    st.flange(&plate, [100.0, 30.0, 2.0], 30.0);
    let b = st.ok();
    let part = b.parts[0].clone();
    // The flange stands at x = 100..102, z 0..30. A line across it at z = 22 (a Right-plane
    // sketch, projected onto the flange): the 8 mm above it turns out, away from the plate.
    let s = st.sketch(PlaneRef::Right, vec![SketchOp::AddPolyline { points: vec![Vec2::new(-10.0, 22.0), Vec2::new(70.0, 22.0)], closed: false, construction: false, label: "Add line" }]);
    let g = &st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry;
    let (curve, _) = g.curves.iter().next().unwrap();
    let line = LineRef::Sketch(CurveRef { sketch: s, curve });
    let outer = face_near(&part, [102.0, 30.0, 17.5]);
    let bf = BendFeature { line: Some(line), face: Some(outer), alignment: BendAlignment::BendLine, ..Default::default() };
    st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bf)));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    assert_eq!(b.parts[0].id, plate.id);
    assert_eq!(bends(&b).len(), 2, "the flange's bend and the Bend's");
    let flat0 = b.sheet_metal[0].flat.parts[0].bounds().unwrap();
    let (_, hi) = bounds(&b.parts[0]);
    assert!(hi[0] > 105.0, "the top of the flange turned out: {hi:?}");
    all_match(&b);
    // A corner break on the plate's far corner.
    let corner = edge_near(&b.parts[0], [0.0, 60.0, 1.0]);
    let cb = CornerBreakFeature { entities: vec![SmPick::Edge(corner)], size: 10.0, ..Default::default() };
    st.add("Corner break", FeatureKind::SheetMetalTool(SheetMetalTool::CornerBreak(cb)));
    let b = st.ok();
    assert!(b.sheet_metal[0].corner_broken);
    assert_eq!(bends(&b).len(), 2);
    all_match(&b);
    let area = b.sheet_metal[0].flat.parts[0].area();
    // The Bend's bend from the table: R5 goes into the Bend's step; the flat keeps its size.
    // The flange's bend is in the base, the Bend's comes after it.
    let bent = bends(&b)[1].0;
    st.table(model, bent, TableEdit::Radius(5.0, "5 mm".into()));
    let b = st.ok();
    assert!(bends(&b).contains(&(bent, 5.0)), "{:?}", bends(&b));
    assert!((b.sheet_metal[0].flat.parts[0].area() - area).abs() < 1e-6, "a Bend never changes the flat");
    let flat1 = b.sheet_metal[0].flat.parts[0].bounds().unwrap();
    assert!((flat1.1 - flat1.0 - (flat0.1 - flat0.0)).norm() < 1e-6);
    all_match(&b);
    // Undo all three, one at a time: each state rebuilds.
    for _ in 0..3 {
        st.h.undo(&mut st.d).unwrap();
        let b = st.ok();
        assert_eq!(b.parts[0].id, plate.id);
        all_match(&b);
    }
    for _ in 0..3 {
        st.h.redo(&mut st.d).unwrap();
    }
    assert!(bends(&st.ok()).contains(&(bent, 5.0)));
}

#[test]
fn a_hem_then_a_tab() {
    let mut st = Studio::new();
    let (_, plate) = st.plate(100.0, 60.0);
    let e = edge_near(&plate, [100.0, 30.0, 2.0]);
    let h = HemFeature { edges: vec![EdgeOrFace::Edge(e)], radius: 3.0, radius_expr: "3 mm".into(), ..Default::default() };
    st.add("Hem", FeatureKind::SheetMetal(SheetMetalFeature::Hem(h)));
    let b = st.ok();
    let hem_area = b.sheet_metal[0].flat.parts[0].area();
    // A 20 × 15 tongue off the y = 60 edge.
    let s = st.sketch(PlaneRef::Top, vec![rect(40.0, 50.0, 60.0, 75.0)]);
    st.add("Tab", FeatureKind::SheetMetalTool(SheetMetalTool::Tab(TabFeature { sketches: vec![s], ..Default::default() })));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    assert_eq!(b.parts[0].id, plate.id);
    let ctx = &b.sheet_metal[0];
    assert!(ctx.model.joints.iter().any(|j| j.bend().is_some_and(|x| x.hem)), "the hem stays");
    let area = ctx.flat.parts[0].area();
    assert!((area - (hem_area + 20.0 * 15.0)).abs() < 1e-3, "{area} vs {hem_area} + 300");
    all_match(&b);
    st.h.undo(&mut st.d).unwrap();
    let b = st.ok();
    assert!((b.sheet_metal[0].flat.parts[0].area() - hem_area).abs() < 1e-6);
    st.h.redo(&mut st.d).unwrap();
    assert!((st.ok().sheet_metal[0].flat.parts[0].area() - area).abs() < 1e-6);
}

#[test]
fn the_table_order_holds_flange_bends_and_later_edits() {
    let mut st = Studio::new();
    let (model, plate) = st.plate(100.0, 60.0);
    st.flange(&plate, [100.0, 30.0, 2.0], 30.0);
    let b = st.ok();
    st.flange(&b.parts[0].clone(), [50.0, 0.0, 2.0], 25.0);
    let b = st.ok();
    let names = |b: &Build| b.sheet_metal[0].model.joints.iter().filter(|j| j.bend().is_some()).map(|j| j.name.clone()).collect::<Vec<_>>();
    let before = names(&b);
    assert_eq!(before.len(), 2);
    let second = b.sheet_metal[0].model.joints.iter().filter(|j| j.bend().is_some()).nth(1).unwrap().id;
    let order = cadrs_sheetmetal::joint_edit::moved(&b.sheet_metal[0].model, second, -1).expect("it can move up");
    st.h.execute(&mut st.d, &SetTableOrder { element: st.el, model, order, label: "Move up".into() }).unwrap();
    let b = st.ok();
    let after = names(&b);
    assert_eq!(after, vec![before[1].clone(), before[0].clone()]);
    // A later Hem keeps the order.
    let e = edge_near(&b.parts[0], [0.0, 30.0, 2.0]);
    st.add("Hem", FeatureKind::SheetMetal(SheetMetalFeature::Hem(HemFeature { edges: vec![EdgeOrFace::Edge(e)], ..Default::default() })));
    let b = st.ok();
    assert_eq!(names(&b)[..2], after[..]);
    all_match(&b);
    // Undo the hem and the move: the order as made.
    st.h.undo(&mut st.d).unwrap();
    st.h.undo(&mut st.d).unwrap();
    assert_eq!(names(&st.ok()), before);
}

#[test]
fn a_flange_and_a_bend_in_either_order_make_the_same_part() {
    let run = |bend_first: bool| {
        let mut st = Studio::new();
        let (_, plate) = st.plate(100.0, 60.0);
        let flange = st.flange(&plate, [100.0, 30.0, 2.0], 30.0);
        let s = st.sketch(PlaneRef::Top, vec![SketchOp::AddPolyline { points: vec![Vec2::new(20.0, -10.0), Vec2::new(20.0, 70.0)], closed: false, construction: false, label: "Add line" }]);
        let g = &st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry;
        let (curve, _) = g.curves.iter().next().unwrap();
        let line = LineRef::Sketch(CurveRef { sketch: s, curve });
        let top = face_near(&st.ok().parts[0], [50.0, 30.0, 2.0]);
        let bend = st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(BendFeature { line: Some(line), face: Some(top), alignment: BendAlignment::Inner, ..Default::default() })));
        if bend_first {
            // The Bend moved up above the Flange (and its line's sketch with it).
            let to = st.position(flange);
            st.h.execute(&mut st.d, &MoveFeature { element: st.el, feature: s, to, label: "Move".into() }).unwrap();
            st.h.execute(&mut st.d, &MoveFeature { element: st.el, feature: bend, to: to + 1, label: "Move".into() }).unwrap();
            assert!(st.position(bend) < st.position(flange));
        }
        let b = st.ok();
        assert_eq!(b.parts.len(), 1);
        assert_eq!(b.parts[0].id, plate.id);
        all_match(&b);
        (volume(&b.parts[0]), b.sheet_metal[0].flat.parts[0].area(), bounds(&b.parts[0]))
    };
    let (v1, a1, b1) = run(false);
    let (v2, a2, b2) = run(true);
    assert!(close(v1, v2, 1e-9) && close(a1, a2, 1e-9), "{v1} {v2} {a1} {a2}");
    for i in 0..3 {
        assert!((b1.0[i] - b2.0[i]).abs() < 1e-6 && (b1.1[i] - b2.1[i]).abs() < 1e-6, "{b1:?} {b2:?}");
    }
}

/// Volume, flat area and bounds of the sole part, for comparing orders.
type Made = (f64, f64, ([f64; 3], [f64; 3]));

fn same_part(a: Made, b: Made) {
    assert!(close(a.0, b.0, 1e-9) && close(a.1, b.1, 1e-9), "{} {} {} {}", a.0, b.0, a.1, b.1);
    for i in 0..3 {
        assert!((a.2.0[i] - b.2.0[i]).abs() < 1e-6 && (a.2.1[i] - b.2.1[i]).abs() < 1e-6, "{:?} {:?}", a.2, b.2);
    }
}

#[test]
fn a_hem_and_a_tab_in_either_order_make_the_same_part() {
    let run = |tab_first: bool| -> Made {
        let mut st = Studio::new();
        let (_, plate) = st.plate(100.0, 60.0);
        let e = edge_near(&plate, [100.0, 30.0, 2.0]);
        let h = HemFeature { edges: vec![EdgeOrFace::Edge(e)], radius: 3.0, radius_expr: "3 mm".into(), ..Default::default() };
        let hem = st.add("Hem", FeatureKind::SheetMetal(SheetMetalFeature::Hem(h)));
        let s = st.sketch(PlaneRef::Top, vec![rect(40.0, 50.0, 60.0, 75.0)]);
        let tab = st.add("Tab", FeatureKind::SheetMetalTool(SheetMetalTool::Tab(TabFeature { sketches: vec![s], ..Default::default() })));
        if tab_first {
            // The Tab (and its sketch) moved up above the Hem.
            let to = st.position(hem);
            st.h.execute(&mut st.d, &MoveFeature { element: st.el, feature: s, to, label: "Move".into() }).unwrap();
            st.h.execute(&mut st.d, &MoveFeature { element: st.el, feature: tab, to: to + 1, label: "Move".into() }).unwrap();
            assert!(st.position(tab) < st.position(hem));
        }
        let b = st.ok();
        assert_eq!(b.parts.len(), 1);
        assert_eq!(b.parts[0].id, plate.id);
        assert!(b.sheet_metal[0].model.joints.iter().any(|j| j.bend().is_some_and(|x| x.hem)), "the hem is there");
        all_match(&b);
        (volume(&b.parts[0]), b.sheet_metal[0].flat.parts[0].area(), bounds(&b.parts[0]))
    };
    same_part(run(false), run(true));
}

#[test]
fn a_make_joint_and_a_bend_in_either_order_make_the_same_part() {
    // Two walls standing on Top, apart (an extruded line along x, 0..50, and one along y at
    // x = 52, 5..30): Make joint (Bend) joins them at the corner; a Bend on the first wall's
    // face along x = 20 folds its short end. Made in either order, each pick taken on the part
    // as it is at that point (as a user would).
    let run = |bend_first: bool| -> Made {
        let mut st = Studio::new();
        let line = |a: Vec2, b: Vec2| SketchOp::AddPolyline { points: vec![a, b], closed: false, construction: false, label: "Add line" };
        let s = st.sketch(PlaneRef::Top, vec![line(Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0)), line(Vec2::new(52.0, 5.0), Vec2::new(52.0, 30.0))]);
        let p = params();
        let x = SheetMetalModelFeature { operation: SheetMetalOp::Extrude, sketches: vec![s], depth: 40.0, depth_expr: "40 mm".into(), params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
        st.add("Sheet metal model", FeatureKind::SheetMetalModel(x));
        assert_eq!(st.ok().parts.len(), 2);
        let ls = st.sketch(PlaneRef::Front, vec![line(Vec2::new(20.0, -10.0), Vec2::new(20.0, 50.0))]);
        let joint = |st: &mut Studio| {
            let b = st.ok();
            let find = |p: [f64; 3]| {
                let part = b.parts.iter().find(|q| q.solid.edges.iter().any(|e| e.distance(p) < 1e-3)).expect("an edge there");
                EdgeOrFace::Edge(edge_near(part, p))
            };
            let edges = vec![find([50.0, 0.0, 30.0]), find([52.0, 5.0, 30.0])];
            st.add("Make joint", FeatureKind::SheetMetal(SheetMetalFeature::MakeJoint(MakeJointFeature { edges, kind: MakeJointType::Bend, ..Default::default() })));
        };
        let bend = |st: &mut Studio| {
            let b = st.ok();
            // The first wall's broad face on y = 0.
            let (part, i) = b
                .parts
                .iter()
                .flat_map(|q| (0..q.solid.faces.len()).map(move |i| (q, i)))
                .filter(|(q, i)| q.solid.faces[*i].plane.is_some_and(|p| p.normal()[1].abs() > 0.999 && p.origin[1].abs() < 1e-6))
                .max_by(|(q, a), (r, b)| q.solid.faces[*a].area.unwrap_or(0.0).total_cmp(&r.solid.faces[*b].area.unwrap_or(0.0)))
                .expect("the first wall's face");
            let face = FaceRef { part: part.id, face: part.solid.faces[i].name, seed: [35.0, 0.0, 20.0] };
            let g = &st.d.element(st.el).unwrap().feature(ls).unwrap().sketch().unwrap().geometry;
            let (curve, _) = g.curves.iter().next().unwrap();
            let bl = LineRef::Sketch(CurveRef { sketch: ls, curve });
            st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(BendFeature { line: Some(bl), face: Some(face), alignment: BendAlignment::Inner, ..Default::default() })));
        };
        if bend_first {
            bend(&mut st);
            joint(&mut st);
        } else {
            joint(&mut st);
            bend(&mut st);
        }
        let b = st.ok();
        assert_eq!(b.parts.len(), 1, "joined");
        assert_eq!(b.sheet_metal[0].model.joints.iter().filter(|j| j.bend().is_some()).count(), 2, "the joint's bend and the Bend");
        all_match(&b);
        (volume(&b.parts[0]), b.sheet_metal[0].flat.parts[0].area(), bounds(&b.parts[0]))
    };
    same_part(run(false), run(true));
}

#[test]
fn forms_stay_on_their_wall_through_a_later_refold() {
    let mut st = Studio::new();
    let (sm, plate) = st.plate(120.0, 80.0);
    let pts = st.sketch(PlaneRef::Top, [(40.0, 25.0), (80.0, 55.0)].iter().map(|(x, y)| SketchOp::AddPoint { pos: Vec2::new(*x, *y) }).collect());
    let top = face_near(&plate, [60.0, 40.0, 2.0]);
    let form = FormFeature {
        form: Some(FormPick { source: FormSource::Library(LibraryForm::Louver), name: "Louver".into(), document_name: LIBRARY_NAME.into(), studio: vec![] }),
        variables: LibraryForm::Louver.variables(),
        locations: vec![FormLocation::SketchPoints(pts)],
        targets: vec![top],
        flip: false,
    };
    st.add("Form", FeatureKind::Form(form));
    let b = st.ok();
    let formed = volume(&b.parts[0]);
    let raised = |b: &Build| b.parts[0].solid.positions.iter().map(|p| p[2]).fold(f64::MIN, f64::max);
    assert!((raised(&b) - (2.0 + 4.0)).abs() < 0.05, "{}", raised(&b));
    let forms = |b: &Build| b.sheet_metal.iter().find(|c| c.feature == sm).unwrap().flat.parts.iter().map(|p| p.forms.len()).sum::<usize>();
    assert_eq!(forms(&b), 2);
    // A flange refolds the model: the louvers are applied again, on the flat too.
    let part = b.parts[0].clone();
    st.flange(&part, [60.0, 0.0, 2.0], 20.0);
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    assert_eq!(b.parts[0].id, plate.id);
    assert_eq!(forms(&b), 2, "the forms are still in the flat");
    assert!(raised(&b) > 5.9, "and still raised: {}", raised(&b));
    // The flange adds exactly its sheet: the louvers' change of volume is the same as before.
    let plain = predicted(&b, &b.parts[0]);
    let louvers = formed - 120.0 * 80.0 * 2.0;
    assert!((volume(&b.parts[0]) - plain - louvers).abs() < 1e-3 * louvers.abs().max(1.0), "{} vs {} + {louvers}", volume(&b.parts[0]), plain);
}

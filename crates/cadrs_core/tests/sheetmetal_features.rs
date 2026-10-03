//! Flange, Hem and Make joint through the document's commands (P3I.4): the edited model's folded
//! parts from the kernel against the flat pattern and closed forms (see `tests/sheetmetal.rs`
//! for the volume rule), and undo.
#![cfg(feature = "occt")]
#![allow(dead_code)]

use std::f64::consts::{FRAC_PI_2, PI};

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::document::{Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_features::{FlangeFeature, HemFeature, MakeJointFeature, MakeJointType, SheetMetalFeature};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::model::RipStyle;
use cadrs_sheetmetal::sharp_edit::{FlangeAlignment, HemKind};
use cadrs_sheetmetal::{JointKind, Params};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Sheet metal");
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

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
        self.h.execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
        f
    }

    fn add(&mut self, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: "Sheet metal model".into(), kind }).unwrap();
        feature
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature, kind, label: "Edit".into() }).unwrap();
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn ok(&self) -> std::sync::Arc<Build> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b
    }

    /// A 100 × 60 × 40 block from the origin.
    fn block(&mut self) -> Part {
        let s = self.sketch(PlaneRef::Top, vec![rect(100.0, 60.0)]);
        self.extrude(ExtrudeFeature { sketches: vec![s], depth: 40.0, depth_expr: "40 mm".into(), ..Default::default() });
        self.ok().parts[0].clone()
    }
}

fn rect(w: f64, h: f64) -> SketchOp {
    SketchOp::AddPolyline {
        points: vec![Vec2::new(0.0, 0.0), Vec2::new(w, 0.0), Vec2::new(w, h), Vec2::new(0.0, h)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn params() -> Params {
    Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..SheetMetalModelFeature::default_params() }
}

fn feature(op: SheetMetalOp) -> SheetMetalModelFeature {
    let p = params();
    SheetMetalModelFeature { operation: op, params: p, exprs: cadrs_core::sheetmetal::SheetMetalExprs::of(&p), ..Default::default() }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
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

/// The folded volume the flat pattern predicts (see the module docs) for one context part: its
/// walls' flat area (after relief cuts) × T, and each bend region's flat area scaled from the
/// neutral radius to the mid-thickness one.
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


/// A 50 × 40 plate on Top, 2 thick, material up (a Thicken of a sketch region).
fn plate(st: &mut Studio) -> (FeatureId, Part) {
    let s = st.sketch(PlaneRef::Top, vec![rect(50.0, 40.0)]);
    let mut x = feature(SheetMetalOp::Thicken);
    x.region_sketches = vec![s];
    let f = st.add(FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    (f, b.parts[0].clone())
}

fn sm(st: &mut Studio, name: &str, x: SheetMetalFeature) -> FeatureId {
    let feature = FeatureId::new();
    st.h.execute(&mut st.d, &AddFeature { element: st.el, feature, base_name: name.into(), kind: FeatureKind::SheetMetal(x) }).unwrap();
    feature
}

fn one_part(b: &Build) -> &Part {
    assert_eq!(b.parts.len(), 1, "{:?}", b.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
    &b.parts[0]
}

#[test]
fn flange_on_a_plate_edge() {
    let mut st = Studio::new();
    let (_, plate) = plate(&mut st);
    let e = edge_near(&plate, [50.0, 20.0, 2.0]);
    let fl = FlangeFeature { edges: vec![EdgeOrFace::Edge(e)], distance: 30.0, distance_expr: "30 mm".into(), ..Default::default() };
    let f = sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
    let b = st.ok();
    let p = one_part(&b);
    assert_eq!(p.name, "Part 1", "the model's part keeps its id and name");
    // Inner: the flange's inside face on the edge, outside at x = 52; 30 high from the bottom.
    let (lo, hi) = bounds(p);
    assert!((hi[0] - 52.0).abs() < 1e-6 && (hi[2] - 30.0).abs() < 1e-6 && lo[2].abs() < 1e-6, "{lo:?} {hi:?}");
    assert!(close(volume(p), predicted(&b, p), 1e-6), "{} vs {}", volume(p), predicted(&b, p));
    // Closed form: base 52 − 5 = 47, flange 30 − 5 = 25, a quarter shell of radii 3..5, all 40 long.
    let want = 47.0 * 40.0 * 2.0 + 25.0 * 40.0 * 2.0 + FRAC_PI_2 / 2.0 * (25.0 - 9.0) * 40.0;
    assert!(close(volume(p), want, 1e-6), "{} vs {want}", volume(p));
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 1);
    // The flange's faces are named after it.
    assert!(p.solid.faces.iter().any(|x| x.name.op == f.0));
    // Undo takes it away again.
    st.h.undo(&mut st.d).expect("undone");
    let b = st.ok();
    assert!(close(volume(one_part(&b)), 50.0 * 40.0 * 2.0, 1e-9));
}

#[test]
fn flanges_on_two_plate_edges_are_mitred() {
    let mut st = Studio::new();
    let (_, plate) = plate(&mut st);
    let edges = vec![EdgeOrFace::Edge(edge_near(&plate, [50.0, 20.0, 2.0])), EdgeOrFace::Edge(edge_near(&plate, [25.0, 40.0, 2.0]))];
    let fl = FlangeFeature { edges, distance: 20.0, distance_expr: "20 mm".into(), ..Default::default() };
    sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
    let b = st.ok();
    let p = one_part(&b);
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count(), 1);
    assert!(close(volume(p), predicted(&b, p), 1e-6), "{} vs {}", volume(p), predicted(&b, p));
}

#[test]
fn partial_flange_and_flange_outer_alignment() {
    let mut st = Studio::new();
    let (_, plate) = plate(&mut st);
    let e = edge_near(&plate, [50.0, 20.0, 2.0]);
    let mut fl = FlangeFeature { edges: vec![EdgeOrFace::Edge(e)], distance: 30.0, distance_expr: "30 mm".into(), alignment: FlangeAlignment::Outer, partial: true, ..Default::default() };
    fl.bound.distance = 10.0;
    fl.second = Some(cadrs_core::sheetmetal_features::Bound { distance: 5.0, ..Default::default() });
    sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
    let b = st.ok();
    let p = one_part(&b);
    assert!(close(volume(p), predicted(&b, p), 1e-6), "{} vs {}", volume(p), predicted(&b, p));
    let (_, hi) = bounds(p);
    assert!((hi[0] - 50.0).abs() < 1e-6, "Outer: the flange's outside on the edge: {hi:?}");
}

#[test]
fn hems_straight_rolled_and_tear_drop() {
    for kind in HemKind::ALL {
        let mut st = Studio::new();
        let (_, plate) = plate(&mut st);
        let e = edge_near(&plate, [50.0, 20.0, 2.0]);
        let h = HemFeature { edges: vec![EdgeOrFace::Edge(e)], kind, flattened: false, radius: 3.0, radius_expr: "3 mm".into(), ..Default::default() };
        sm(&mut st, "Hem", SheetMetalFeature::Hem(h));
        let b = st.ok();
        let p = one_part(&b);
        assert!(close(volume(p), predicted(&b, p), 1e-6), "{kind:?}: {} vs {}", volume(p), predicted(&b, p));
        // Outer: the hem's outside on the edge (the mesh's vertices on the round, within the
        // tessellation's chord error).
        let (_, hi) = bounds(p);
        assert!(hi[0] <= 50.0 + 1e-6 && hi[0] > 49.9, "{kind:?}: {hi:?}");
        if kind == HemKind::Straight {
            // Base 45, hem 12.5 − 5 = 7.5, a half shell of radii 3..5, 40 long.
            let want = 45.0 * 40.0 * 2.0 + 7.5 * 40.0 * 2.0 + PI / 2.0 * (25.0 - 9.0) * 40.0;
            assert!(close(volume(p), want, 1e-6), "{} vs {want}", volume(p));
        }
    }
}

/// Two walls apart: an extruded line along x (0..50) and one along y at x = 52 (y 5..30).
fn two_walls(st: &mut Studio) -> Part {
    let line = |a: Vec2, b: Vec2| SketchOp::AddPolyline { points: vec![a, b], closed: false, construction: false, label: "Add line" };
    let s = st.sketch(PlaneRef::Top, vec![line(Vec2::new(0.0, 0.0), Vec2::new(50.0, 0.0)), line(Vec2::new(52.0, 5.0), Vec2::new(52.0, 30.0))]);
    let mut x = feature(SheetMetalOp::Extrude);
    x.sketches = vec![s];
    x.depth = 40.0;
    x.depth_expr = "40 mm".into();
    st.add(FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    assert_eq!(b.parts.len(), 2);
    b.parts[0].clone()
}

#[test]
fn make_joint_bend_and_butt_rip() {
    for (kind, style) in [(MakeJointType::Bend, RipStyle::EdgeJoint), (MakeJointType::Rip, RipStyle::EdgeJoint), (MakeJointType::Rip, RipStyle::ButtDirection1)] {
        let mut st = Studio::new();
        two_walls(&mut st);
        let b = st.ok();
        let find = |p: [f64; 3]| {
            let part = b.parts.iter().find(|q| q.solid.edges.iter().any(|e| e.distance(p) < 1e-3)).expect("an edge there");
            EdgeOrFace::Edge(edge_near(part, p))
        };
        let edges = vec![find([50.0, 0.0, 20.0]), find([52.0, 5.0, 20.0])];
        sm(&mut st, "Make joint", SheetMetalFeature::MakeJoint(MakeJointFeature { edges, kind, style, ..Default::default() }));
        let b = st.ok();
        let ctx = &b.sheet_metal[0];
        match kind {
            MakeJointType::Bend => assert_eq!(b.parts.len(), 1, "one part now"),
            MakeJointType::Rip => assert_eq!(b.parts.len(), 2),
        }
        assert!(ctx.model.joints.iter().any(|j| j.name.starts_with(if kind == MakeJointType::Bend { "Bend" } else { "Joint" })));
        for p in &b.parts {
            assert!(close(volume(p), predicted(&b, p), 1e-6), "{kind:?} {style:?}: {} vs {}", volume(p), predicted(&b, p));
        }
    }
}

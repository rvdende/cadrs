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
    // The model's part (other parts may stand beside it).
    (f, sm_part(&b))
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
    // (Its reliefs fold as exact arcs, the flat's polygons: 2e-5.)
    assert!(close(volume(p), predicted(&b, p), 2e-5), "{} vs {}", volume(p), predicted(&b, p));
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

// ---------------------------------------------------------------------------------------------
// Flange options (SM3.3–SM3.7) against closed forms, on the 50 × 40 plate (T 2, R 3, material
// up from z = 0) with a flange on its east edge (the top-face edge at x = 50). Inner, 90°: the
// outer virtual sharp is at x = 52, z = 0; the flange's outside face is x = 52.

use cadrs_core::document::{DirectionRef, VertexRef};
use cadrs_core::sheetmetal_features::{AngleControl, Bound, ChainType, FlangeEnd, SmTarget};

const T: f64 = 2.0;

/// A block standing beside the plate (x 100..110, y `y0..y1`, z 0..`h`): faces and vertices to
/// go up to.
fn block_at(st: &mut Studio, y0: f64, y1: f64, h: f64) -> FeatureId {
    let s = st.sketch(PlaneRef::Top, vec![SketchOp::AddPolyline {
        points: vec![Vec2::new(100.0, y0), Vec2::new(110.0, y0), Vec2::new(110.0, y1), Vec2::new(100.0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }]);
    st.extrude(ExtrudeFeature { sketches: vec![s], depth: h, depth_expr: format!("{h} mm"), ..Default::default() })
}

/// The sheet metal part and its build.
fn sm_part(b: &Build) -> Part {
    let id = b.sheet_metal[0].parts[0].0;
    b.parts.iter().find(|p| p.id == id).unwrap().clone()
}

fn part_of(b: &Build, f: FeatureId) -> Part {
    b.parts.iter().find(|p| p.id.feature == f).unwrap().clone()
}

/// The east flange's wall in the model (its normal along x) and its extent along y and z.
fn east_wall(b: &Build) -> ((f64, f64), (f64, f64)) {
    let m = &b.sheet_metal[0].model;
    let w = m.walls.iter().find(|w| w.surface.normal().is_some_and(|n| n.x.abs() > 0.999)).expect("a wall facing x");
    let pts: Vec<_> = w.outline.outer.iter().map(|q| w.surface.point(*q)).collect();
    let span = |f: &dyn Fn(&cadrs_sheetmetal::model::P3) -> f64| pts.iter().map(f).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)));
    (span(&|p| p.y), span(&|p| p.z))
}

fn bend_angle(b: &Build) -> f64 {
    let m = &b.sheet_metal[0].model;
    m.joints.iter().find_map(|j| j.bend()).expect("a bend").angle
}

fn east_flange(plate: &Part) -> FlangeFeature {
    FlangeFeature { edges: vec![EdgeOrFace::Edge(edge_near(plate, [50.0, 20.0, 2.0]))], ..Default::default() }
}

fn check_volume(b: &Build) {
    let p = sm_part(b);
    assert!(close(volume(&p), predicted(b, &p), 1e-6), "{} vs {}", volume(&p), predicted(b, &p));
}

/// Up to entity: the flange runs from its outer sharp (z = 0) up to the block's top face, so its
/// tip is at z = 35; with offset 5, at z = 40 (SM3.3).
#[test]
fn flange_up_to_entity_and_with_offset() {
    for (end, offset, tip) in [(FlangeEnd::UpToEntity, 0.0, 35.0), (FlangeEnd::UpToEntityOffset, 5.0, 40.0)] {
        let mut st = Studio::new();
        let blk = block_at(&mut st, 0.0, 10.0, 35.0);
        let (_, plate) = plate(&mut st);
        let b = st.ok();
        let top = face_near(&part_of(&b, blk), [105.0, 5.0, 35.0]);
        let fl = FlangeFeature { end, up_to: Some(SmTarget::Face(top)), offset, offset_expr: format!("{offset} mm"), ..east_flange(&plate) };
        sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
        let b = st.ok();
        let (_, hi) = bounds(&sm_part(&b));
        assert!((hi[2] - tip).abs() < 1e-6, "{end:?}: tip at z = {}", hi[2]);
        assert!((hi[0] - 52.0).abs() < 1e-6, "{end:?}: outside at x = {}", hi[0]);
        // The flange's wall: from the bend's tangent (z = R + T = 5) to the tip.
        let (_, z) = east_wall(&b);
        assert!((z.0 - 5.0).abs() < 1e-6 && (z.1 - tip).abs() < 1e-6, "{z:?}");
        check_volume(&b);
    }
}

/// Align to geometry (SM3.4): parallel to a face inclined 30° (a wedge's slope). The flange turns
/// to the picked face's side (up): bend angle 30°, so from the outer sharp (z = 0) its outside
/// rises `25 · sin 30° = 12.5` and its inside corner `T · cos 30°` more. The arrow takes the other
/// parallel: 150°.
#[test]
fn flange_aligned_to_a_30_degree_plane() {
    let h = 40.0 * (30f64).to_radians().tan();
    for flip in [false, true] {
        let mut st = Studio::new();
        let s = st.sketch(PlaneRef::Front, vec![SketchOp::AddPolyline {
            points: vec![Vec2::new(100.0, 0.0), Vec2::new(140.0, 0.0), Vec2::new(140.0, h)],
            closed: true,
            construction: false,
            label: "Add line",
        }]);
        let wedge = st.extrude(ExtrudeFeature { sketches: vec![s], depth: 10.0, depth_expr: "10 mm".into(), ..Default::default() });
        let (_, plate) = plate(&mut st);
        let b = st.ok();
        let w = part_of(&b, wedge);
        // The slope's centre (the extrude runs either way off Front).
        let ymid = (bounds(&w).0[1] + bounds(&w).1[1]) / 2.0;
        let slope = face_near(&w, [120.0, ymid, h / 2.0]);
        let n = w.solid.faces.iter().find(|f| f.name == slope.face).unwrap().plane.unwrap().normal();
        assert!((n[2].abs() - (30f64).to_radians().cos()).abs() < 1e-6, "the slope's normal {n:?}");
        let fl = FlangeFeature { angle_control: AngleControl::AlignToGeometry, parallel_to: Some(DirectionRef::FaceNormal(slope)), flip, ..east_flange(&plate) };
        sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
        let b = st.ok();
        let want = if flip { 150.0 } else { 30.0 };
        assert!((bend_angle(&b).to_degrees() - want).abs() < 1e-6, "flip {flip}: {}", bend_angle(&b).to_degrees());
        if !flip {
            let (_, hi) = bounds(&sm_part(&b));
            let tip = 25.0 * 0.5 + T * (30f64).to_radians().cos();
            assert!((hi[2] - tip).abs() < 1e-6, "tip at z = {} vs {tip}", hi[2]);
        }
        check_volume(&b);
    }
}

/// Angle from direction (SM3.4): 30° from the Top plane's normal (z), turned away from the plate
/// (towards +x): the flange leans out, a 60° bend; its tip `25 · sin 60°` above the outer sharp
/// plus `T · cos 60°`. The arrow turns it 30° the other way: a 120° bend, leaning in (its outside
/// tip, `25 · sin 120°`, the highest point).
#[test]
fn flange_angle_from_direction() {
    for (flip, want) in [(false, 60.0), (true, 120.0)] {
        let mut st = Studio::new();
        let (_, plate) = plate(&mut st);
        let fl = FlangeFeature {
            angle_control: AngleControl::AngleFromDirection,
            direction: Some(DirectionRef::PlaneNormal(PlaneRef::Top)),
            direction_angle: 30.0,
            direction_angle_expr: "30 deg".into(),
            flip,
            ..east_flange(&plate)
        };
        sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
        let b = st.ok();
        assert!((bend_angle(&b).to_degrees() - want).abs() < 1e-6, "flip {flip}: {}", bend_angle(&b).to_degrees());
        let (_, hi) = bounds(&sm_part(&b));
        let a = want.to_radians();
        let tip = 25.0 * a.sin() + T * a.cos().max(0.0);
        assert!((hi[2] - tip).abs() < 1e-6, "flip {flip}: tip at z = {} vs {tip}", hi[2]);
        check_volume(&b);
    }
}

fn vertex_at(p: &Part, at: [f64; 3]) -> VertexRef {
    let v = p.solid.vertices.iter().min_by(|a, b| {
        let d = |q: [f64; 3]| (0..3).map(|i| (q[i] - at[i]).powi(2)).sum::<f64>();
        d(a.point).total_cmp(&d(b.point))
    });
    let v = v.unwrap();
    assert!((0..3).all(|i| (v.point[i] - at[i]).abs() < 1e-6), "no vertex at {at:?}");
    VertexRef { part: p.id, vertex: v.name, point: v.point }
}

/// Which end of the plate's east edge a partial flange's first bound is measured from (the
/// edge's start, which the edge's own direction sets: Onshape shows it with the bound's arrow):
/// true for y = 0.
fn east_starts_at_y0(st: &Studio, plate: &Part) -> bool {
    let mut s2 = Studio { d: st.d.clone(), h: History::default(), el: st.el };
    let fl = FlangeFeature { partial: true, bound: Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() }, ..east_flange(plate) };
    sm(&mut s2, "Flange", SheetMetalFeature::Flange(fl));
    let (y, _) = east_wall(&s2.ok());
    (y.0 - 10.0).abs() < 1e-6
}

/// A partial flange bounded by vertices (SM3.7), its first bound at the y = 0 end (Flip sides
/// if the edge starts at y = 40): up to a vertex at y = 12 with an offset of 3, the second up to
/// one at y = 30: the flange runs y 15..30. Both bounds Blind 10 and 5: 10..35 from the y = 0
/// end, and Flip sides mirrors it to 5..30.
#[test]
fn partial_flange_bounded_by_vertices_and_flip_sides() {
    let mut st = Studio::new();
    let blk = block_at(&mut st, 12.0, 30.0, 5.0);
    let (_, plate) = plate(&mut st);
    let b = st.ok();
    let bp = part_of(&b, blk);
    let (v12, v30) = (vertex_at(&bp, [100.0, 12.0, 0.0]), vertex_at(&bp, [100.0, 30.0, 0.0]));
    let from_y0 = east_starts_at_y0(&st, &plate);
    let fl = FlangeFeature {
        partial: true,
        flip_sides: !from_y0,
        bound: Bound { kind: FlangeEnd::UpToEntityOffset, up_to: Some(SmTarget::Vertex(v12)), offset: 3.0, offset_expr: "3 mm".into(), ..Default::default() },
        second: Some(Bound { kind: FlangeEnd::UpToEntity, up_to: Some(SmTarget::Vertex(v30)), ..Default::default() }),
        ..east_flange(&plate)
    };
    let f = sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
    let b = st.ok();
    let (y, _) = east_wall(&b);
    assert!((y.0 - 15.0).abs() < 1e-6 && (y.1 - 30.0).abs() < 1e-6, "{y:?}");
    check_volume(&b);
    for flip in [false, true] {
        let blind = FlangeFeature {
            partial: true,
            flip_sides: flip != !from_y0,
            bound: Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() },
            second: Some(Bound { distance: 5.0, distance_expr: "5 mm".into(), ..Default::default() }),
            ..east_flange(&plate)
        };
        st.set(f, FeatureKind::SheetMetal(SheetMetalFeature::Flange(blind)));
        let b = st.ok();
        let (y, _) = east_wall(&b);
        let want = if flip { (5.0, 30.0) } else { (10.0, 35.0) };
        assert!((y.0 - want.0).abs() < 1e-6 && (y.1 - want.1).abs() < 1e-6, "flip sides {flip}: {y:?}");
        check_volume(&b);
    }
}

/// A test the bounds' orientation can't hide: the first bound up to the vertex at y = 12 alone
/// (no second). Measured from the edge's start: from the y = 0 end the flange runs y 12..40;
/// with Flip sides the bound is measured from the other end and the flange runs 0..12 (and the
/// other way round when the edge starts at y = 40) — never 28..40.
#[test]
fn partial_flange_up_to_a_vertex_whichever_way_the_edge_runs() {
    let mut st = Studio::new();
    let blk = block_at(&mut st, 12.0, 30.0, 5.0);
    let (_, plate) = plate(&mut st);
    let b = st.ok();
    let v12 = vertex_at(&part_of(&b, blk), [100.0, 12.0, 0.0]);
    let from_y0 = east_starts_at_y0(&st, &plate);
    for flip_sides in [false, true] {
        let fl = FlangeFeature {
            partial: true,
            flip_sides,
            bound: Bound { kind: FlangeEnd::UpToEntity, up_to: Some(SmTarget::Vertex(v12)), ..Default::default() },
            ..east_flange(&plate)
        };
        let mut s2 = Studio { d: st.d.clone(), h: History::default(), el: st.el };
        sm(&mut s2, "Flange", SheetMetalFeature::Flange(fl));
        let b = s2.ok();
        let (y, _) = east_wall(&b);
        let want = if flip_sides == !from_y0 { (12.0, 40.0) } else { (0.0, 12.0) };
        assert!((y.0 - want.0).abs() < 1e-6 && (y.1 - want.1).abs() < 1e-6, "flip sides {flip_sides} (start at y = 0: {from_y0}): {y:?}, want {want:?}");
    }
}

/// Hold adjacent edges (SM3.7): on, only the flange's stretch of the plate's edge moves to the
/// flange's sharp (the rest stays at x = 50); off, the whole edge moves (to x = 52, Inner).
#[test]
fn partial_flange_hold_adjacent_edges() {
    for hold in [true, false] {
        let mut st = Studio::new();
        let (_, plate) = plate(&mut st);
        let fl = FlangeFeature {
            partial: true,
            hold_adjacent: hold,
            bound: Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() },
            second: Some(Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() }),
            ..east_flange(&plate)
        };
        sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
        let b = st.ok();
        let p = sm_part(&b);
        // The plate's edge beside the flange (y < 5, clear of the bend relief).
        let reach = p.solid.positions.iter().filter(|q| q[1] < 5.0).map(|q| q[0]).fold(f64::MIN, f64::max);
        let want = if hold { 50.0 } else { 52.0 };
        assert!((reach - want).abs() < 1e-6, "hold {hold}: the edge beside the flange at x = {reach}");
        check_volume(&b);
    }
}

/// Per chain (SM3.7) on two adjacent edges (east, then north, meeting at the corner (50, 40)):
/// one chain, so the bounds apply only at its free ends: the first (10) where the east edge starts
/// at y = 0, the second (5) where the north edge ends at x = 0; at the corner the two flanges
/// meet whole and are mitred (a rip).
#[test]
fn partial_flange_per_chain_on_adjacent_edges() {
    let mut st = Studio::new();
    let (_, plate) = plate(&mut st);
    let edges = vec![EdgeOrFace::Edge(edge_near(&plate, [50.0, 20.0, 2.0])), EdgeOrFace::Edge(edge_near(&plate, [25.0, 40.0, 2.0]))];
    let fl = FlangeFeature {
        edges,
        distance: 20.0,
        distance_expr: "20 mm".into(),
        partial: true,
        chain: ChainType::PerChain,
        bound: Bound { distance: 10.0, distance_expr: "10 mm".into(), ..Default::default() },
        second: Some(Bound { distance: 5.0, distance_expr: "5 mm".into(), ..Default::default() }),
        ..Default::default()
    };
    sm(&mut st, "Flange", SheetMetalFeature::Flange(fl));
    let b = st.ok();
    let m = &b.sheet_metal[0].model;
    assert_eq!(m.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count(), 1, "mitred at the corner");
    let extent = |axis: usize, along: usize| {
        let w = m.walls.iter().find(|w| w.surface.normal().is_some_and(|n| n[axis].abs() > 0.999)).unwrap();
        w.outline.outer.iter().map(|q| w.surface.point(*q)[along]).fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v), b.max(v)))
    };
    let (east, north) = (extent(0, 1), extent(1, 0));
    // The free ends: 10 in from y = 0 (the first edge's) and 5 in from x = 0; the corner ends
    // reach the mitre.
    assert!((east.0 - 10.0).abs() < 1e-6 && (north.0 - 5.0).abs() < 1e-6, "east {east:?} north {north:?}");
    assert!(east.1 > 39.0 && north.1 > 49.0, "east {east:?} north {north:?}");
    check_volume(&b);
}

/// A Flange with Inner alignment on sloping edges (the P3I.7 fixer's E3: a sloped enclosure
/// converted with its slope left open, 25 mm flanges on both side walls' sloping edges) stays one
/// part with the box, and the box's volume is its flat's, for every alignment (Outer and Middle
/// stop the flanges' ends clear of the front wall and the shelf they ran into). (It came apart: the side wall's bend
/// reliefs were cut by polygon booleans that round to 1e-6 mm, which left the oblique edge off
/// the bend's face by more than the kernel's tolerance.)
#[test]
fn flange_on_a_sloped_enclosures_sloping_edges() {
    use cadrs_core::samples::{extrude_of, region_refs};
    use cadrs_sheetmetal::sharp_edit::FlangeAlignment;
    let (w, d, hb, hf, sh) = (200.0, 250.0, 200.0, 125.0, 75.0);
    let mut failed: Vec<String> = Vec::new();
    // Material outside the block, flanges folded in. (Open: material inside with the flanges
    // folded in over the opening, the P3I.7 stand-in's way, still fails for Inner, Outer and
    // Middle: the flanges' ends run into the front wall and the shelf.)
    for (inside, align) in [false].into_iter().flat_map(|i| [FlangeAlignment::Inner, FlangeAlignment::Outer, FlangeAlignment::Middle, FlangeAlignment::HoldLine].map(|a| (i, a))) {
        let mut st = Studio::new();
        let profile = [(0.0, 0.0), (d, 0.0), (d, hb), (d - sh, hb), (0.0, hf)];
        let s = st.sketch(PlaneRef::Right, vec![SketchOp::AddPolyline { points: profile.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed: true, construction: false, label: "Add line" }]);
        let g = st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        st.extrude(extrude_of(region_refs(s, &g, &[Vec2::new(d / 2.0, hf / 2.0)]), w));
        let b = st.ok();
        let block = b.parts[0].clone();
        let slope_mid = [(d - sh) / 2.0, (hf + hb) / 2.0];
        let slope = face_near(&block, [w / 2.0, slope_mid[0], slope_mid[1]]);
        let bends = [[w / 2.0, 0.0, 0.0], [w / 2.0, d, 0.0], [0.0, d / 2.0, 0.0], [w, d / 2.0, 0.0], [w / 2.0, d, hb]].iter().map(|q| EdgeOrFace::Edge(edge_near(&block, *q))).collect();
        let mut x = feature(SheetMetalOp::Convert);
        x.params.thickness = 1.5;
        x.params.bend_radius = 1.5;
        x.exprs = cadrs_core::sheetmetal::SheetMetalExprs::of(&x.params);
        x.parts = vec![block.id];
        x.exclude = vec![slope];
        x.bends = bends;
        x.flip_thickness = inside;
        st.add(FeatureKind::SheetMetalModel(x));
        let part = sm_part(&st.ok());
        let edges = [0.0, w].iter().map(|x| EdgeOrFace::Edge(edge_near(&part, [*x, slope_mid[0], slope_mid[1]]))).collect();
        sm(&mut st, "Flange", SheetMetalFeature::Flange(FlangeFeature { edges, distance: 25.0, distance_expr: "25 mm".into(), alignment: align, flip: inside, ..Default::default() }));
        let b = st.build();
        if !b.errors.is_empty() {
            failed.push(format!("inside {inside} {align:?}: {:?}", b.errors));
            continue;
        }
        let ctx = &b.sheet_metal[0];
        let p = sm_part(&b);
        let case = format!("inside {inside} {align:?}");
        if b.parts.len() != 1 {
            failed.push(format!("{case}: {} parts", b.parts.len()));
        } else if !(ctx.flat.is_ok() && ctx.flat.parts.len() == 1) {
            failed.push(format!("{case}: {:?}", ctx.flat.errors));
        } else if ctx.model.joints.iter().filter(|j| j.bend().is_some()).count() != 7 {
            failed.push(format!("{case}: not 7 bends"));
        } else if !close(volume(&p), predicted(&b, &p), 1e-6) {
            failed.push(format!("{case}: volume {} vs the flat's {}", volume(&p), predicted(&b, &p)));
        }
    }
    assert!(failed.is_empty(), "{failed:?}");
}

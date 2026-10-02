//! The Sheet metal model feature through the document's commands (P3I.2): the folded solid's
//! numbers from the kernel against the flat pattern and closed forms.
//!
//! The folded volume of a model is its flat area × thickness, except in the bends: a bend region
//! is `θ·(R + K·T)` wide flat but holds `θ·T·(R + T/2)` of material per unit length folded, so
//! `volume = area·T + Σ θ·L·T²·(½ − K)` over the bends (L their lengths).
#![cfg(feature = "occt")]

use std::f64::consts::FRAC_PI_2;

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, EndType, ExtrudeFeature, FaceRef, FeatureKind, RegionRef};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{CurveRef, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::{JointKind, Params};
use cadrs_sketch::{CurveKind, PlaneRef, SketchOp, Vec2};

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

#[test]
fn convert_a_block_bending_its_bottom_edges() {
    let mut st = Studio::new();
    let block = st.block();
    let bottom = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]];
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![block.id];
    x.bends = bottom.iter().map(|p| EdgeOrFace::Edge(edge_near(&block, *p))).collect();
    let f = st.add(FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    // The block is consumed; an open box (bottom and four sides) and the separate top.
    assert_eq!(b.parts.len(), 2, "{:?}", b.parts.iter().map(|p| &p.name).collect::<Vec<_>>());
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).expect("context");
    assert_eq!(ctx.model.walls.len(), 6);
    let bends: Vec<&str> = ctx.model.joints.iter().filter(|j| j.bend().is_some()).map(|j| j.name.as_str()).collect();
    assert_eq!(bends, ["Bend A", "Bend B", "Bend C", "Bend D"]);
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Rip { .. })).count(), 8);
    assert!(ctx.flat.is_ok());
    let open_box = b.parts.iter().max_by(|a, c| volume(a).total_cmp(&volume(c))).unwrap();
    let top = b.parts.iter().min_by(|a, c| volume(a).total_cmp(&volume(c))).unwrap();
    // Volumes against the flat pattern.
    for p in [open_box, top] {
        let v = volume(p);
        let want = predicted(&b, p);
        assert!(close(v, want, 1e-6), "{}: {v} vs {want}", p.name);
    }
    // The top: a flat 100 × 60 plate less the 0.1 gap at each ripped edge, 2 thick.
    assert!(close(volume(top), 99.8 * 59.8 * 2.0, 1e-9), "{}", volume(top));
    // The box outside the block (material outward): from −2 below to 0.1 short of the top.
    let (lo, hi) = bounds(open_box);
    for (a, e) in lo.iter().zip([-2.0, -2.0, -2.0]) {
        assert!((a - e).abs() < 1e-6, "{lo:?}");
    }
    for (a, e) in hi.iter().zip([102.0, 62.0, 39.9]) {
        assert!((a - e).abs() < 1e-6, "{hi:?}");
    }
    // Closed form: the base 100 × 60 less the inside setback (3) at each bend: 94 × 54. The
    // walls are ripped 0.1 short of each other at the corners and of the top: 0.2 shorter than
    // their base edge, 40 − 3 − 0.1 = 36.9 high. The bends are quarter shells of radii 3..5,
    // which the (Simple) corner reliefs stop where the base stops: 94 and 54 long.
    let walls = 94.0 * 54.0 * 2.0 + 2.0 * (99.8 + 59.8) * 36.9 * 2.0;
    let bends = 2.0 * (94.0 + 54.0) * FRAC_PI_2 / 2.0 * (25.0 - 9.0);
    let want = walls + bends;
    assert!((volume(open_box) - want).abs() < 1e-6 * want, "{} vs {want}", volume(open_box));
}

#[test]
fn bend_pick_order_decides_the_folded_parts() {
    let mut st = Studio::new();
    let block = st.block();
    let e = |p: [f64; 3]| EdgeOrFace::Edge(edge_near(&block, p));
    // Bottom–south, south–east, bottom–east: the last closes a loop and stays a rip.
    let order_a = vec![e([50.0, 0.0, 0.0]), e([100.0, 0.0, 20.0]), e([100.0, 30.0, 0.0])];
    // Bottom–east first, then bottom–south: now south–east is the rip.
    let order_b = vec![e([100.0, 30.0, 0.0]), e([50.0, 0.0, 0.0]), e([100.0, 0.0, 20.0])];
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![block.id];
    x.bends = order_a;
    let f = st.add(FeatureKind::SheetMetalModel(x.clone()));
    let a = st.ok();
    x.bends = order_b;
    st.set(f, FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    let joined = |b: &Build| {
        let ctx = &b.sheet_metal[0];
        let mut v: Vec<usize> = ctx.parts.iter().map(|(_, w)| w.len()).collect();
        v.sort();
        let big = b.parts.iter().map(volume).fold(0.0, f64::max);
        (v, big, ctx.flat.parts.iter().map(|p| p.bounds().map(|(lo, hi)| ((hi - lo).norm() * 1e3).round() as i64).unwrap_or(0)).max())
    };
    let (ja, jb) = (joined(&a), joined(&b));
    // Both: one part of three walls (bottom, south, east) and three single walls.
    assert_eq!(ja.0, vec![1, 1, 1, 3]);
    assert_eq!(jb.0, vec![1, 1, 1, 3]);
    // But a different flat (the east wall hangs off the south wall, or off the bottom) and a
    // different folded part (a bend at a different edge).
    assert_ne!(ja.2, jb.2);
    assert!((ja.1 - jb.1).abs() > 1.0, "{} vs {}", ja.1, jb.1);
    for p in &b.parts {
        assert!(close(volume(p), predicted(&b, p), 1e-6));
    }
}

#[test]
fn convert_keeps_the_input_when_asked_and_moves_out_by_the_clearance() {
    let mut st = Studio::new();
    let block = st.block();
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![block.id];
    x.keep_input = true;
    x.clearance = 1.0;
    x.clearance_expr = "1 mm".into();
    st.add(FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    assert_eq!(b.parts.len(), 7, "the block and six walls");
    // The bottom wall 1 below the block, 102 × 62 less its rips.
    let bottom = b.parts.iter().filter(|p| p.id != block.id).min_by(|a, c| bounds(a).0[2].total_cmp(&bounds(c).0[2])).unwrap();
    let (lo, hi) = bounds(bottom);
    assert!((lo[2] + 3.0).abs() < 1e-6 && (hi[2] + 1.0).abs() < 1e-6, "{lo:?} {hi:?}");
}

/// An open sketch: a 40 line, a quarter arc of radius 10 turning left, a 30 line.
fn hook(st: &mut Studio) -> FeatureId {
    st.sketch(
        PlaneRef::Front,
        vec![
            SketchOp::AddPolyline { points: vec![Vec2::new(0.0, 0.0), Vec2::new(40.0, 0.0)], closed: false, construction: false, label: "Add line" },
            SketchOp::AddArc { center: Vec2::new(40.0, 10.0), start: Vec2::new(40.0, 0.0), end: Vec2::new(50.0, 10.0), construction: false },
            SketchOp::AddPolyline { points: vec![Vec2::new(50.0, 10.0), Vec2::new(50.0, 40.0)], closed: false, construction: false, label: "Add line" },
        ],
    )
}

#[test]
fn extrude_an_open_sketch_with_its_arc_rolled_or_bent() {
    let mut st = Studio::new();
    let s = hook(&mut st);
    let mut x = feature(SheetMetalOp::Extrude);
    x.sketches = vec![s];
    x.depth = 20.0;
    x.depth_expr = "20 mm".into();
    let f = st.add(FeatureKind::SheetMetalModel(x.clone()));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.walls.len(), 3);
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 2);
    // Rolled at K 0.5: the folded volume is the flat area × T exactly. The material is inside
    // the turn: a strip (40 + 30 + π/2·9) × 20, 2 thick.
    let rolled = volume(&b.parts[0]);
    let want = (70.0 + FRAC_PI_2 * 9.0) * 20.0 * 2.0;
    assert!(close(rolled, want, 1e-6), "{rolled} vs {want}");
    // The arc as a bend (inner radius 10 − 2 = 8): a bend's K 0.45 flat.
    let sketch = st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let arc = sketch.curves.iter().find(|(_, c)| matches!(c.kind, CurveKind::Arc { .. })).map(|(id, _)| id).unwrap();
    x.arcs_as_bends = vec![CurveRef { sketch: s, curve: arc }];
    st.set(f, FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.walls.len(), 2);
    let bend = ctx.model.joints.iter().find_map(|j| j.bend()).unwrap();
    assert!((bend.radius - 8.0).abs() < 1e-9);
    let bent = volume(&b.parts[0]);
    // Same material folded: the walls 40 + 30 and the quarter shell of radii 8..10.
    let want = (70.0 * 2.0 + FRAC_PI_2 / 2.0 * (100.0 - 64.0)) * 20.0;
    assert!(close(bent, want, 1e-6), "{bent} vs {want}");
    assert!(close(bent, predicted(&b, &b.parts[0]), 1e-6));
    assert!(close(bent, rolled, 1e-9), "the same sheet either way");
}

#[test]
fn thicken_sketch_regions() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![rect(50.0, 30.0)]);
    let mut x = feature(SheetMetalOp::Thicken);
    x.region_sketches = vec![s];
    st.add(FeatureKind::SheetMetalModel(x.clone()));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1);
    assert!(close(volume(&b.parts[0]), 50.0 * 30.0 * 2.0, 1e-9));
    let (lo, hi) = bounds(&b.parts[0]);
    assert!((lo[2]).abs() < 1e-9 && (hi[2] - 2.0).abs() < 1e-9, "material along the sketch normal");
    // A picked region does the same; the opposite direction puts the sheet below.
    let sk = st.d.element(st.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let region = cadrs_sketch::region::regions(&sk).into_iter().next().unwrap();
    x.region_sketches.clear();
    x.regions = vec![RegionRef::new(s, &region)];
    x.flip_thickness = true;
    let f = st.features().last().unwrap().id;
    st.set(f, FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    let (lo, hi) = bounds(&b.parts[0]);
    assert!((lo[2] + 2.0).abs() < 1e-9 && hi[2].abs() < 1e-9);
}

#[test]
fn thicken_faces_bent_at_their_edge() {
    let mut st = Studio::new();
    let block = st.block();
    let mut x = feature(SheetMetalOp::Thicken);
    // The top and the east faces, bent where they meet; the block stays.
    x.faces = vec![face_near(&block, [50.0, 30.0, 40.0]), face_near(&block, [100.0, 30.0, 20.0])];
    x.bends = vec![EdgeOrFace::Edge(edge_near(&block, [100.0, 30.0, 40.0]))];
    st.add(FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    assert_eq!(b.parts.len(), 2, "the block and the bent sheet");
    let sheet = b.parts.iter().find(|p| p.id != block.id).unwrap();
    assert!(close(volume(sheet), predicted(&b, sheet), 1e-6));
}

#[test]
fn an_l_shaped_part_whose_inner_walls_fold_onto_each_other_collides() {
    let mut st = Studio::new();
    let s = st.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddPolyline {
            points: vec![
                Vec2::new(0.0, 0.0),
                Vec2::new(60.0, 0.0),
                Vec2::new(60.0, 30.0),
                Vec2::new(30.0, 30.0),
                Vec2::new(30.0, 60.0),
                Vec2::new(0.0, 60.0),
            ],
            closed: true,
            construction: false,
            label: "Add polyline",
        }],
    );
    st.extrude(ExtrudeFeature { sketches: vec![s], depth: 40.0, depth_expr: "40 mm".into(), ..Default::default() });
    let part = st.ok().parts[0].clone();
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![part.id];
    x.bends = vec![EdgeOrFace::Edge(edge_near(&part, [45.0, 30.0, 0.0])), EdgeOrFace::Edge(edge_near(&part, [30.0, 45.0, 0.0]))];
    let f = st.add(FeatureKind::SheetMetalModel(x));
    let b = st.build();
    assert_eq!(b.error(f), Some("Collision in sheet metal flat pattern"));
    // The part isn't consumed, and the context stays for the flat view.
    assert_eq!(b.parts.len(), 1);
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    assert!(ctx.flat.errors.iter().any(|e| matches!(e, cadrs_sheetmetal::FlatError::Collision { .. })));
}

#[test]
fn out_of_range_settings_fail_with_the_range() {
    let mut st = Studio::new();
    let s = st.sketch(PlaneRef::Top, vec![rect(50.0, 30.0)]);
    let mut x = feature(SheetMetalOp::Thicken);
    x.region_sketches = vec![s];
    x.params.bend_relief.depth_scale = 7.0;
    let f = st.add(FeatureKind::SheetMetalModel(x));
    assert_eq!(st.build().error(f), Some("Bend relief depth scale must be between 1 and 5"));
}

#[test]
fn undo_and_redo_the_model_and_cut_the_folded_part_after_it() {
    let mut st = Studio::new();
    let block = st.block();
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![block.id];
    x.bends = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]].iter().map(|p| EdgeOrFace::Edge(edge_near(&block, *p))).collect();
    st.add(FeatureKind::SheetMetalModel(x));
    let with = st.ok();
    assert_eq!(with.parts.len(), 2);
    st.h.undo(&mut st.d).expect("undo");
    let without = st.ok();
    assert_eq!(without.parts.len(), 1);
    assert!(without.sheet_metal.is_empty());
    st.h.redo(&mut st.d).expect("redo");
    let again = st.ok();
    assert_eq!(again.parts.len(), 2);
    assert_eq!(again.sheet_metal[0].model, with.sheet_metal[0].model, "the same walls and joints, ids and all");
    // An ordinary feature after it works on the folded part: a 20 × 10 cut through the bottom.
    let box_part = again.parts.iter().max_by(|a, c| volume(a).total_cmp(&volume(c))).unwrap().clone();
    let s = st.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddPolyline {
            points: vec![Vec2::new(40.0, 20.0), Vec2::new(60.0, 20.0), Vec2::new(60.0, 30.0), Vec2::new(40.0, 30.0)],
            closed: true,
            construction: false,
            label: "Add rectangle",
        }],
    );
    st.extrude(ExtrudeFeature {
        sketches: vec![s],
        op: BooleanOp::Remove,
        end: EndType::ThroughAll,
        symmetric: true,
        merge_scope: vec![box_part.id],
        ..Default::default()
    });
    let cut = st.ok();
    let after = cut.parts.iter().find(|p| p.id == box_part.id).unwrap();
    assert!(close(volume(&box_part) - volume(after), 20.0 * 10.0 * 2.0, 1e-6), "{}", volume(&box_part) - volume(after));
    assert!(cut.sheet_metal[0].active);
}

#[test]
fn convert_a_filleted_block_rolls_or_bends_the_round() {
    let mut st = Studio::new();
    let block = st.block();
    // A radius 8 round on the top front edge.
    let fillet = cadrs_core::applied::FilletFeature {
        entities: vec![EdgeOrFace::Edge(edge_near(&block, [50.0, 0.0, 40.0]))],
        size: 8.0,
        size_expr: "8 mm".into(),
        ..Default::default()
    };
    let fid = FeatureId::new();
    st.h.execute(&mut st.d, &AddFeature { element: st.el, feature: fid, base_name: "Fillet".into(), kind: FeatureKind::Fillet(fillet) }).unwrap();
    let rounded = st.ok().parts[0].clone();
    // The round's middle: its axis runs along x at y = 8, z = 32.
    let h = 8.0 * std::f64::consts::FRAC_1_SQRT_2;
    let round = face_near(&rounded, [50.0, 8.0 - h, 32.0 + h]);
    let mut x = feature(SheetMetalOp::Convert);
    x.parts = vec![rounded.id];
    let f = st.add(FeatureKind::SheetMetalModel(x.clone()));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    // Not picked: a rolled wall joined to the top and the front by tangent joints.
    assert_eq!(ctx.model.walls.len(), 7, "{:?}", ctx.model.walls.iter().map(|w| w.id).collect::<Vec<_>>());
    assert_eq!(ctx.model.joints.iter().filter(|j| matches!(j.kind, JointKind::Tangent { .. })).count(), 2);
    for p in &b.parts {
        assert!(close(volume(p), predicted(&b, p), 1e-6), "{}: {} vs {}", p.name, volume(p), predicted(&b, p));
    }
    // Picked to bend: a bend of the round's radius between the top and the front.
    x.bends = vec![EdgeOrFace::Face(round)];
    st.set(f, FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    let ctx = b.sheet_metal.iter().find(|c| c.feature == f).unwrap();
    assert_eq!(ctx.model.walls.len(), 6);
    let bend = ctx.model.joints.iter().find_map(|j| j.bend()).expect("a bend");
    assert!((bend.radius - 8.0).abs() < 1e-6, "{}", bend.radius);
    for p in &b.parts {
        assert!(close(volume(p), predicted(&b, p), 1e-6), "{}: {} vs {}", p.name, volume(p), predicted(&b, p));
    }
}

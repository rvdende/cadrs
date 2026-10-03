//! The sheet metal features after a Sheet metal model (P3I.5) through the document's commands:
//! Bend (the flat keeps its size), Jog, Corner, Bend relief, Corner break, Tab, Finish, and the
//! ordinary features that act on an active model as sheet metal (perpendicular cuts, corner
//! fillets, face mirrors). Folded volumes are checked against the flat pattern as in
//! `tests/sheetmetal.rs`.
#![cfg(feature = "occt")]

use cadrs_core::applied::EdgeOrFace;
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, EndType, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::rebuild::Build;
use cadrs_core::sheetmetal::{CurveRef, SheetMetalExprs, SheetMetalModelFeature, SheetMetalOp};
use cadrs_core::sheetmetal_tools::*;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, rebuild};
use cadrs_sheetmetal::Params;
use cadrs_sheetmetal::model_edit::{BendAlignment, JogAnchor};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = Document::new("Sheet metal tools");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn sketch(&mut self, plane: PlaneRef, points: &[(f64, f64)], closed: bool) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        let op = SketchOp::AddPolyline { points: points.iter().map(|(x, y)| Vec2::new(*x, *y)).collect(), closed, construction: false, label: "Add polyline" };
        self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        f
    }

    fn first_curve(&self, sketch: FeatureId) -> CurveRef {
        let g = &self.d.element(self.el).unwrap().feature(sketch).unwrap().sketch().unwrap().geometry;
        let (curve, _) = g.curves.iter().next().unwrap();
        CurveRef { sketch, curve }
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h.execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind }).unwrap();
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

    /// A 100 × 60 plate, 2 thick, made by a Thicken of a Top-plane rectangle (material up).
    fn plate(&mut self) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)], true);
        let p = params();
        let x = SheetMetalModelFeature { operation: SheetMetalOp::Thicken, region_sketches: vec![s], params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
        self.add("Sheet metal model", FeatureKind::SheetMetalModel(x))
    }

    /// A line across the plate at `x` (a Top-plane sketch).
    fn line_at(&mut self, x: f64) -> LineRef {
        let s = self.sketch(PlaneRef::Top, &[(x, -10.0), (x, 70.0)], false);
        LineRef::Sketch(self.first_curve(s))
    }
}

fn params() -> Params {
    Params { thickness: 2.0, bend_radius: 3.0, k_factor: 0.45, minimal_gap: 0.2, ..SheetMetalModelFeature::default_params() }
}

fn volume(p: &Part) -> f64 {
    p.mass.as_ref().expect("kernel mass").volume
}

fn face_near(part: &Part, p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let d = |c: [f64; 3]| (0..3).map(|i| (c[i] - p[i]).powi(2)).sum::<f64>();
    let i = (0..s.faces.len()).min_by(|a, b| d(s.faces[*a].center.unwrap()).total_cmp(&d(s.faces[*b].center.unwrap()))).unwrap();
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.faces[i].center.unwrap() }
}

fn edge_near(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    EdgeRef { part: part.id, edge: e.name, seed: e.points[e.points.len() / 2] }
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

/// The folded volume the flat predicts (walls × T; bend regions scaled to the mid radius).
fn predicted(b: &Build, part: &Part) -> f64 {
    use cadrs_sheetmetal::flat::PieceSource;
    let ctx = b.sheet_metal.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part.id)).expect("a sheet metal part");
    let walls = &ctx.parts.iter().find(|(p, _)| *p == part.id).unwrap().1;
    let flat = ctx.flat.parts.iter().find(|f| f.walls.iter().any(|w| walls.contains(w))).expect("its flat");
    let p = &ctx.model.params;
    let t = p.thickness;
    flat.pieces
        .iter()
        .map(|piece| {
            let area: f64 = piece.cut.iter().map(|c| c.area()).sum();
            match piece.source {
                PieceSource::Wall(_) => area * t,
                PieceSource::Bend(j) => {
                    let bend = ctx.model.joint(j).unwrap().bend().unwrap();
                    let k = match bend.value_or_model(p) {
                        cadrs_sheetmetal::BendValue::KFactor(k) => k,
                        _ => p.k_factor,
                    };
                    area * t * (bend.radius + t / 2.0) / (bend.radius + k * t)
                }
            }
        })
        .sum()
}

fn close(a: f64, b: f64, rel: f64) -> bool {
    (a - b).abs() <= rel * b.abs().max(1e-9)
}

fn flat_size(b: &Build) -> (f64, f64) {
    let (lo, hi) = b.sheet_metal[0].flat.parts[0].bounds().unwrap();
    (hi.x - lo.x, hi.y - lo.y)
}

fn bend(line: LineRef, face: FaceRef, alignment: BendAlignment) -> BendFeature {
    BendFeature { line: Some(line), face: Some(face), alignment, ..Default::default() }
}

#[test]
fn bends_on_a_flat_plate_keep_the_flat_size() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let top = face_near(&plate, [50.0, 30.0, 2.0]);
    // Exercise E1's way: Inner alignment, the line on the top face.
    let l1 = st.line_at(80.0);
    let b1 = st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bend(l1, top, BendAlignment::Inner))));
    let b = st.ok();
    assert_eq!(b.parts.len(), 1, "still one part");
    let part = &b.parts[0];
    assert_eq!(part.id, plate.id, "the part keeps its id");
    let (w, h) = flat_size(&b);
    assert!((w - 100.0).abs() < 1e-3 && (h - 60.0).abs() < 1e-3, "flat {w} × {h}");
    assert!(close(volume(part), predicted(&b, part), 1e-6), "{} vs {}", volume(part), predicted(&b, part));
    // Bent up towards the picked (top) face: the wall stands at x = 80 with its inside face on the
    // line.
    let (lo, hi) = bounds(part);
    assert!(hi[2] > 10.0, "{hi:?}");
    assert!((hi[0] - 80.0 - 2.0).abs() < 1e-3, "outer face at x = 82, {hi:?}");
    assert!(lo[0].abs() < 1e-6);
    // A second bend on the other end (held opposite: the small side still moves).
    let b_now = st.ok();
    let top2 = face_near(&b_now.parts[0], [40.0, 30.0, 2.0]);
    let l2 = st.line_at(15.0);
    st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(BendFeature { opposite: true, ..bend(l2, top2, BendAlignment::BendLine) })));
    let b = st.ok();
    let (w, h) = flat_size(&b);
    assert!((w - 100.0).abs() < 1e-3 && (h - 60.0).abs() < 1e-3, "flat {w} × {h}");
    let part = &b.parts[0];
    assert!(close(volume(part), predicted(&b, part), 1e-6));
    assert_eq!(b.sheet_metal[0].model.joints.len(), 2);
    let (lo, _) = bounds(part);
    assert!(lo[2] < -5.0, "the second bend went down (opposite angle): {lo:?}");
    // A custom radius and K factor change the folded part, not the flat.
    let face = face_near(&plate, [50.0, 30.0, 2.0]);
    st.set(b1, FeatureKind::SheetMetalTool(SheetMetalTool::Bend(BendFeature { use_model_radius: false, radius: 6.0, use_model_k: false, k_factor: 0.3, ..bend(l1, face, BendAlignment::Inner) })));
    let b = st.ok();
    let (w, _) = flat_size(&b);
    assert!((w - 100.0).abs() < 1e-3);
    assert!(close(volume(&b.parts[0]), predicted(&b, &b.parts[0]), 1e-6));
}

#[test]
fn a_jog_offsets_the_far_end() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let top = face_near(&plate, [50.0, 30.0, 2.0]);
    let line = st.line_at(70.0);
    let j = JogFeature { bend: bend(line, top, BendAlignment::BendLine), offset: 10.0, anchor: JogAnchor::Inside, ..Default::default() };
    let f = st.add("Jog", FeatureKind::SheetMetalTool(SheetMetalTool::Jog(j.clone())));
    let b = st.ok();
    let part = &b.parts[0];
    let (_, hi) = bounds(part);
    // Inside: 10 from the top face (z = 2) to the far wall's lower face: its top at 14.
    assert!((hi[2] - 14.0).abs() < 1e-3, "{hi:?}");
    let (w, _) = flat_size(&b);
    assert!((w - 100.0).abs() < 1e-3, "Preserve material: the flat keeps its size");
    assert!(close(volume(part), predicted(&b, part), 1e-6));
    // Without Preserve material the far end stays at x = 100.
    st.set(f, FeatureKind::SheetMetalTool(SheetMetalTool::Jog(JogFeature { preserve_material: false, ..j })));
    let b = st.ok();
    let (_, hi) = bounds(&b.parts[0]);
    assert!((hi[0] - 100.0).abs() < 1e-3, "{hi:?}");
    assert!(flat_size(&b).0 > 100.5);
}

#[test]
fn corner_and_bend_relief_overrides() {
    let mut st = Studio::new();
    // An open box by Extrude: a U sketch on Front extruded 60 gives a channel; Convert of a block
    // gives the corners. Use Convert of a 100 × 60 × 20 block with four bends.
    let s = st.sketch(PlaneRef::Top, &[(0.0, 0.0), (100.0, 0.0), (100.0, 60.0), (0.0, 60.0)], true);
    let e = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: e, extrude: ExtrudeFeature::default() }).unwrap();
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: e, extrude: ExtrudeFeature { sketches: vec![s], depth: 20.0, depth_expr: "20 mm".into(), ..Default::default() }, label: "Extrude".into() }).unwrap();
    let block = st.ok().parts[0].clone();
    let p = params();
    let mut x = SheetMetalModelFeature { operation: SheetMetalOp::Convert, params: p, exprs: SheetMetalExprs::of(&p), ..Default::default() };
    x.parts = vec![block.id];
    x.exclude = vec![face_near(&block, [50.0, 30.0, 20.0])];
    x.bends = [[50.0, 0.0, 0.0], [100.0, 30.0, 0.0], [50.0, 60.0, 0.0], [0.0, 30.0, 0.0]].iter().map(|q| EdgeOrFace::Edge(edge_near(&block, *q))).collect();
    st.add("Sheet metal model", FeatureKind::SheetMetalModel(x));
    let b = st.ok();
    let sheet = b.parts.iter().find(|p| p.id != block.id).unwrap().clone();
    let corners_before = b.sheet_metal[0].flat.parts[0].corners.len();
    assert!(corners_before >= 4);
    let area_before = b.sheet_metal[0].flat.parts[0].area();
    // A Corner at (100, 60, 0): Round – Sized 3.3 (exercise E2).
    let mut c = CornerFeature::default();
    c.relief.kind = cadrs_sheetmetal::CornerReliefKind::RoundSized;
    c.relief.size = 3.3 * 4.0;
    let v = sheet.solid.vertices.iter().min_by(|a, b| {
        let d = |p: [f64; 3]| (p[0] - 102.0).powi(2) + (p[1] - 62.0).powi(2) + (p[2] + 2.0).powi(2);
        d(a.point).total_cmp(&d(b.point))
    });
    let pick = match v {
        Some(v) => SmPick::Vertex(cadrs_core::document::VertexRef { part: sheet.id, vertex: v.name, point: v.point }),
        None => SmPick::Face(face_near(&sheet, [100.0, 60.0, 0.0])),
    };
    c.corner = Some(pick);
    st.add("Corner", FeatureKind::SheetMetalTool(SheetMetalTool::Corner(c)));
    let b = st.ok();
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.corner_overrides.len(), 1);
    let fc = ctx.flat.parts[0].corners.iter().filter(|k| k.relief.kind == cadrs_sheetmetal::CornerReliefKind::RoundSized).count();
    assert_eq!(fc, 1, "one corner overridden");
    assert!(ctx.flat.parts[0].area() < area_before, "the round relief takes more material");
    let part = b.parts.iter().find(|p| p.id == sheet.id).unwrap();
    // (Round reliefs on a bend region are cut as wedges: close, not exact.)
    assert!(close(volume(part), predicted(&b, part), 1e-3), "{} vs {}", volume(part), predicted(&b, part));
}

#[test]
fn corner_breaks_lock_the_table_and_show_in_the_flat() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let corner = edge_near(&plate, [100.0, 60.0, 1.0]);
    let cb = CornerBreakFeature { entities: vec![SmPick::Edge(corner)], size: 10.0, ..Default::default() };
    let f = st.add("Corner break", FeatureKind::SheetMetalTool(SheetMetalTool::CornerBreak(cb.clone())));
    let b = st.ok();
    let area = b.sheet_metal[0].flat.parts[0].area();
    let want = 6000.0 - (1.0 - std::f64::consts::PI / 4.0) * 100.0;
    assert!((area - want).abs() < 0.3, "{area} vs {want}");
    assert!(b.sheet_metal[0].corner_broken);
    assert!(close(volume(&b.parts[0]), area * 2.0, 1e-6));
    // The chamfer tab: 5 × 5 off the corner.
    st.set(f, FeatureKind::SheetMetalTool(SheetMetalTool::CornerBreak(CornerBreakFeature { chamfer: true, distance: 5.0, ..cb })));
    let b = st.ok();
    assert!((b.sheet_metal[0].flat.parts[0].area() - (6000.0 - 12.5)).abs() < 1e-3);
}

#[test]
fn finish_makes_later_features_ordinary_and_rolls_back() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let fin = st.add("Finish sheet metal model", FeatureKind::SheetMetalTool(SheetMetalTool::Finish(FinishFeature { parts: vec![plate.id] })));
    let b = st.ok();
    assert!(!b.sheet_metal[0].active, "finished");
    let flat_before = b.sheet_metal[0].flat.clone();
    // A fillet of a corner edge after Finish is an ordinary fillet: the flat doesn't change.
    let corner = edge_near(&plate, [100.0, 60.0, 1.0]);
    let fillet = cadrs_core::applied::FilletFeature { entities: vec![EdgeOrFace::Edge(corner)], size: 10.0, size_expr: "10 mm".into(), ..Default::default() };
    let fl = st.add("Fillet", FeatureKind::Fillet(fillet));
    let b = st.ok();
    assert_eq!(b.sheet_metal[0].flat, flat_before, "the fillet isn't in the flat");
    assert!(volume(&b.parts[0]) < 6000.0 * 2.0 - 1.0, "but it is on the part");
    // A Bend now fails: the model is finished.
    let top = face_near(&plate, [50.0, 30.0, 2.0]);
    let line = st.line_at(80.0);
    let bf = st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bend(line, top, BendAlignment::BendLine))));
    let b = st.build();
    assert!(b.error(bf).is_some_and(|e| e.contains("finished")), "{:?}", b.error(bf));
    // Without the Finish (suppressed or deleted), the model is active again: the fillet is a
    // corner break and shows in the flat, and the bend builds.
    st.h.execute(&mut st.d, &cadrs_core::commands::DeleteFeature { element: st.el, feature: fin, label: "Delete".into() }).unwrap();
    let b = st.ok();
    assert!(b.sheet_metal[0].active);
    assert!(b.sheet_metal[0].flat.parts[0].area() < 6000.0 - 1.0, "the fillet is a corner break in the flat");
    let _ = fl;
}

#[test]
fn a_cut_through_active_sheet_metal_is_perpendicular_and_in_the_flat() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let top = face_near(&plate, [50.0, 30.0, 2.0]);
    let line = st.line_at(70.0);
    st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bend(line, top, BendAlignment::HoldLine))));
    let bent = st.ok();
    let area0 = bent.sheet_metal[0].flat.parts[0].area();
    // A 10 × 10 square on Top, cut through all: a square hole in the base.
    let s = st.sketch(PlaneRef::Top, &[(20.0, 20.0), (30.0, 20.0), (30.0, 30.0), (20.0, 30.0)], true);
    let e = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: e, extrude: ExtrudeFeature::default() }).unwrap();
    let cut = ExtrudeFeature { sketches: vec![s], op: BooleanOp::Remove, end: EndType::ThroughAll, symmetric: true, ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: e, extrude: cut, label: "Extrude".into() }).unwrap();
    let b = st.ok();
    let area = b.sheet_metal[0].flat.parts[0].area();
    assert!((area0 - area - 100.0).abs() < 1e-3, "{area0} − {area}");
    assert!(close(volume(&b.parts[0]), predicted(&b, &b.parts[0]), 1e-6));
}

#[test]
fn a_face_mirror_copies_a_flange_with_its_bend() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    let top = face_near(&plate, [50.0, 30.0, 2.0]);
    let line = st.line_at(85.0);
    st.add("Bend", FeatureKind::SheetMetalTool(SheetMetalTool::Bend(bend(line, top, BendAlignment::HoldLine))));
    let b = st.ok();
    let part = b.parts[0].clone();
    let (_, hi) = bounds(&part);
    // The flange's outer face, mirrored in the Right plane moved to x = 50.
    let flange_face = face_near(&part, [hi[0], 30.0, hi[2] / 2.0 + 1.0]);
    // A block whose face at x = 50 is the mirror plane.
    let bs = st.sketch(PlaneRef::Top, &[(50.0, 100.0), (60.0, 100.0), (60.0, 110.0), (50.0, 110.0)], true);
    let e = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: e, extrude: ExtrudeFeature::default() }).unwrap();
    let blk = ExtrudeFeature { sketches: vec![bs], depth: 10.0, depth_expr: "10 mm".into(), op: BooleanOp::New, ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: e, extrude: blk, label: "Extrude".into() }).unwrap();
    let b = st.ok();
    let block = b.parts.iter().find(|p| p.id != part.id).unwrap().clone();
    let mirror_face = face_near(&block, [50.0, 105.0, 5.0]);
    let mirror = cadrs_core::pattern::MirrorFeature {
        mirror_type: cadrs_core::pattern::PatternType::Face,
        faces: vec![flange_face],
        plane: Some(cadrs_core::pattern::MirrorPlane::Face(mirror_face)),
        ..Default::default()
    };
    st.add("Mirror", FeatureKind::Mirror(mirror));
    let b = st.ok();
    let ctx = &b.sheet_metal[0];
    assert_eq!(ctx.model.joints.iter().filter(|j| j.bend().is_some()).count(), 2, "the copy came with its bend");
    let sheet = b.parts.iter().find(|p| p.id == part.id).unwrap();
    let (lo, hi) = bounds(sheet);
    assert!(lo[2] > -1e-6 && hi[2] > 10.0 && (lo[0] - (100.0 - hi[0])).abs() < 1e-3, "{lo:?} {hi:?}");
    assert!(close(volume(sheet), predicted(&b, sheet), 1e-6));
}

#[test]
fn a_tab_adds_to_its_wall_and_cuts_its_subtraction_scope() {
    let mut st = Studio::new();
    st.plate();
    let plate = st.ok().parts[0].clone();
    // A 20 × 15 tongue off the y = 60 edge, sketched on the plate's top.
    let s = st.sketch(PlaneRef::Top, &[(40.0, 50.0), (60.0, 50.0), (60.0, 75.0), (40.0, 75.0)], true);
    // A block the tongue runs into.
    let bs = st.sketch(PlaneRef::Top, &[(30.0, 65.0), (70.0, 65.0), (70.0, 90.0), (30.0, 90.0)], true);
    let e = FeatureId::new();
    st.h.execute(&mut st.d, &AddExtrude { element: st.el, feature: e, extrude: ExtrudeFeature::default() }).unwrap();
    let blk = ExtrudeFeature { sketches: vec![bs], depth: 10.0, depth_expr: "10 mm".into(), symmetric: true, op: BooleanOp::New, ..Default::default() };
    st.h.execute(&mut st.d, &SetExtrude { element: st.el, feature: e, extrude: blk, label: "Extrude".into() }).unwrap();
    let b = st.ok();
    let block = b.parts.iter().find(|p| p.id != plate.id).unwrap().clone();
    let v_block = volume(&block);
    let tab = TabFeature { sketches: vec![s], offset: 0.5, scope: vec![block.id], ..Default::default() };
    st.add("Tab", FeatureKind::SheetMetalTool(SheetMetalTool::Tab(tab)));
    let b = st.ok();
    let area = b.sheet_metal[0].flat.parts[0].area();
    assert!((area - (6000.0 + 20.0 * 15.0)).abs() < 1e-3, "{area}");
    let sheet = b.parts.iter().find(|p| p.id == plate.id).unwrap();
    assert!(close(volume(sheet), area * 2.0, 1e-6));
    let block_after = b.parts.iter().find(|p| p.id == block.id).unwrap();
    // The pocket: (20 + 1) × (75 − 65 + 0.5) ± (thickness + offset) about the profile (−2.5..2.5,
    // inside the block's −5..5).
    let pocket = 21.0 * 10.5 * 5.0;
    assert!((v_block - volume(block_after) - pocket).abs() < 2.0, "{}", v_block - volume(block_after));
}


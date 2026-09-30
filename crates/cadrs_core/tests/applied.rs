//! P3.6: the applied features through the feature list (Fillet, Chamfer, Shell, Hole), reorder
//! and folders. Every expected value is derived by hand in the test's comment.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::applied::{
    ChamferFeature, ChamferType, EdgeOrFace, FilletControl, FilletFeature, FilletMeasurement, FilletType, HoleFeature,
    HolePoint, ShellFeature,
};
use cadrs_core::commands::{
    AddExtrude, AddFeature, AddSketch, CreateFolder, EditSketch, MoveFeatures, RenameFeature, SetExtrude, SetFeature,
    SetFolder, UnpackFolder,
};
use cadrs_core::document::{BooleanOp, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, HoleStyle, HoleType, Length};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::samples;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part};
use cadrs_sketch::{PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3.6");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn build(&self) -> std::sync::Arc<Build> {
        rebuild::build(&self.features())
    }

    fn parts(&self) -> Vec<Part> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }

    fn volume(&self) -> f64 {
        self.parts().iter().map(|p| p.mass.unwrap().volume).sum()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    /// A box x0..x1 × y0..y1 on Top, `h` high (New).
    fn block(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, vec![rect(x0, y0, x1, y1)]);
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, &[Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)]);
        let e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, h) };
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn add(&mut self, a: AddFeature) -> FeatureId {
        let id = a.feature;
        self.h.execute(&mut self.d, &a).unwrap();
        id
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline {
        points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)],
        closed: true,
        construction: false,
        label: "Add rectangle",
    }
}

fn face_at(part: &Part, n: [f64; 3], p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let i = (0..s.faces.len())
        .find(|&i| {
            s.faces[i].plane.is_some_and(|pl| {
                let m = pl.normal();
                let l = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                (m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / l > 0.999
            }) && s.face_contains(i, p)
        })
        .expect("a face there");
    FaceRef { part: part.id, face: s.faces[i].name, seed: p }
}

fn edge_at(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

#[test]
fn fillet_cube_all_twelve_edges() {
    // A 100³ cube with R10 on all 12 edges, picked as its six faces (a face stands for its
    // edges): V = 1e6 − (4 − π)·100·12·100/4 + 8[3(1 − π/4) − (1 − π/6)]·10³ (each corner cube
    // was counted three times by the edges and keeps a sphere octant) = 975 587.01.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 100.0, 100.0);
    let cube = d.parts()[0].clone();
    let c = [50.0, 50.0, 50.0];
    let faces: Vec<EdgeOrFace> = [[1.0, 0.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, 1.0], [0.0, 0.0, -1.0]]
        .iter()
        .map(|n: &[f64; 3]| EdgeOrFace::Face(face_at(&cube, *n, [c[0] + 50.0 * n[0], c[1] + 50.0 * n[1], c[2] + 50.0 * n[2]])))
        .collect();
    d.add(AddFeature::fillet(
        d.el,
        FeatureId::new(),
        FilletFeature { entities: faces, size: 10.0, size_expr: "10 mm".into(), ..FilletFeature::default() },
    ));
    let corner = 8.0 * (3.0 * (1.0 - PI / 4.0) - (1.0 - PI / 6.0)) * 1000.0;
    close(d.volume(), 1e6 - (4.0 - PI) * 100.0 * 12.0 * 100.0 / 4.0 + corner, 1e-4);
    assert_eq!(d.features().last().unwrap().name, "Fillet 1");
}

#[test]
fn fillet_width_and_overflow() {
    // Width 3 on a box's vertical edge (90°): r = 3/√2, removing (1 − π/4)·4.5 per mm over 30.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let e = edge_at(&part, [10.0, 20.0, 15.0]);
    let f = d.add(AddFeature::fillet(
        d.el,
        FeatureId::new(),
        FilletFeature {
            entities: vec![EdgeOrFace::Edge(e)],
            measurement: FilletMeasurement::Width,
            size: 3.0,
            size_expr: "3 mm".into(),
            ..FilletFeature::default()
        },
    ));
    close(d.volume(), 6000.0 - (1.0 - PI / 4.0) * 4.5 * 30.0, 1e-6);
    // An R8 fillet on the 10 mm edge's neighbour is fine; with overflow off, an R8 along the
    // 20 mm top edge of the 10-wide face: its contact line stays on the face (8 < 10) — no
    // overflow either. (The overflow refusal itself is the kernel's `fillet_overflow` case.)
    let mut x = d.features().last().unwrap().fillet().unwrap().clone();
    x.allow_overflow = false;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: f, kind: FeatureKind::Fillet(x), label: "Overflow".into() })
        .unwrap();
    assert!(d.build().errors.is_empty());
}

#[test]
fn tangent_propagation_off_is_refused_on_a_chain() {
    // A 10 × 20 × 30 block with its vertical edge at (10, 20) rounded R5: the top face's edge
    // y = 20 now runs tangentially into the arc and on into x = 10. Filleting that one edge with
    // Tangent propagation off would have to stop at the arc, which OpenCascade can't: an error
    // naming the cause. On, the whole chain (2 lines + the arc) is rounded.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let vertical = edge_at(&part, [10.0, 20.0, 15.0]);
    let fillet = |entities, size: f64| FilletFeature { entities, size, size_expr: format!("{size} mm"), ..FilletFeature::default() };
    d.add(AddFeature::fillet(d.el, FeatureId::new(), fillet(vec![EdgeOrFace::Edge(vertical)], 5.0)));
    let part = d.parts()[0].clone();
    let top_edge = edge_at(&part, [2.0, 20.0, 30.0]);
    let off = FilletFeature { tangent_propagation: false, ..fillet(vec![EdgeOrFace::Edge(top_edge)], 1.0) };
    let f = d.add(AddFeature::fillet(d.el, FeatureId::new(), off.clone()));
    let errors = d.build().errors.clone();
    assert_eq!(errors.len(), 1, "{errors:?}");
    let message = format!("{:?}", errors[0]);
    assert!(message.contains("Tangent propagation") && message.contains("2 tangent edges"), "{message}");
    let on = FilletFeature { tangent_propagation: true, ..off };
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: f, kind: FeatureKind::Fillet(on), label: "On".into() })
        .unwrap();
    assert!(d.build().errors.is_empty());
}

#[test]
fn full_round_and_conic_sections() {
    // Full round of the 10 × 20 × 30 block's top between its x sides: r 5, removing the two
    // corner slivers (1 − π/4)·25 each along 20.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let round = FilletFeature {
        kind: FilletType::FullRound,
        side1: vec![face_at(&part, [-1.0, 0.0, 0.0], [0.0, 10.0, 15.0])],
        center: vec![face_at(&part, [0.0, 0.0, 1.0], [5.0, 10.0, 30.0])],
        side2: vec![face_at(&part, [1.0, 0.0, 0.0], [10.0, 10.0, 15.0])],
        ..FilletFeature::default()
    };
    let f = d.add(AddFeature::fillet(d.el, FeatureId::new(), round));
    close(d.volume(), 6000.0 - (2.0 - PI / 2.0) * 25.0 * 20.0, 1e-6);
    // Switched to an Edge fillet, Conic with Rho 0.5 (a parabola), r 3 on the vertical edge at
    // (10, 20): the section left is 1/3 of the contact triangle 9/2, along 30.
    let conic = FilletFeature {
        entities: vec![EdgeOrFace::Edge(edge_at(&part, [10.0, 20.0, 15.0]))],
        control: FilletControl::Conic,
        rho: 0.5,
        size: 3.0,
        size_expr: "3 mm".into(),
        ..FilletFeature::default()
    };
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: f, kind: FeatureKind::Fillet(conic), label: "Conic".into() })
        .unwrap();
    close(d.volume(), 6000.0 - 1.5 * 30.0, 1e-6);
}

#[test]
fn chamfer_two_by_45_removes_two_per_mm() {
    // 2 mm × 45° (Distance and angle) on a 30 mm edge removes 2·2/2·30 = 60 mm³.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let e = edge_at(&part, [10.0, 20.0, 15.0]);
    d.add(AddFeature::chamfer(
        d.el,
        FeatureId::new(),
        ChamferFeature {
            entities: vec![EdgeOrFace::Edge(e)],
            kind: ChamferType::DistanceAngle,
            distance: 2.0,
            distance_expr: "2 mm".into(),
            angle: 45.0,
            ..ChamferFeature::default()
        },
    ));
    close(d.volume(), 6000.0 - 2.0 * 2.0 / 2.0 * 30.0, 1e-6);
}

#[test]
fn shell_open_box_and_failure() {
    // 100 × 60 × 40, the bottom removed, 4 mm: V = 100·60·40 − 92·52·36 = 67 776.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 60.0, 40.0);
    let part = d.parts()[0].clone();
    let bottom = face_at(&part, [0.0, 0.0, -1.0], [50.0, 30.0, 0.0]);
    let s = d.add(AddFeature::shell(
        d.el,
        FeatureId::new(),
        ShellFeature { faces: vec![bottom], thickness: 4.0, thickness_expr: "4 mm".into(), ..ShellFeature::default() },
    ));
    close(d.volume(), 100.0 * 60.0 * 40.0 - 92.0 * 52.0 * 36.0, 1e-6);
    // Hollow (PS16.3): the part picked instead of faces: a 92 × 52 × 32 void.
    let mut x = d.features().last().unwrap().shell().unwrap().clone();
    x.hollow = true;
    x.parts = vec![part.id];
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: s, kind: FeatureKind::Shell(x.clone()), label: "Hollow".into() })
        .unwrap();
    close(d.volume(), 240_000.0 - 92.0 * 52.0 * 32.0, 1e-6);
    // PS16.4: 35 mm walls would cross (the box is 60 wide): the shell fails, the part stays as
    // it was, and the reason names the fix.
    x.hollow = false;
    x.thickness = 35.0;
    x.thickness_expr = "35 mm".into();
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: s, kind: FeatureKind::Shell(x), label: "Thickness".into() })
        .unwrap();
    let b = d.build();
    let why = b.error(s).expect("the shell fails");
    assert!(why.contains("reduce the thickness"), "{why}");
    close(b.parts[0].mass.unwrap().volume, 240_000.0, 1e-6);
}

/// A hole at the middle of a 60 × 60 × 30 block, sketched on Top with the block below it, its
/// top `above` mm under the sketch plane (an extrude down from Top with a starting offset).
fn hole_doc(above: f64) -> (Doc, FeatureId) {
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 60.0, 60.0)]);
    let g = d.d.element(d.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let regions = samples::region_refs(s, &g, &[Vec2::new(30.0, 30.0)]);
    let e = ExtrudeFeature {
        flip: true,
        start_offset: (above > 0.0).then(|| cadrs_core::Offset { value: above, expr: format!("{above} mm"), flip: false }),
        ..samples::extrude_of(regions, 30.0)
    };
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddExtrude { element: d.el, feature: f, extrude: ExtrudeFeature::default() }).unwrap();
    d.h.execute(&mut d.d, &SetExtrude { element: d.el, feature: f, extrude: e, label: "Extrude".into() }).unwrap();
    let p = d.sketch(PlaneRef::Top, vec![SketchOp::AddPoint { pos: Vec2::new(30.0, 30.0) }]);
    (d, p)
}

fn point_of(d: &Doc, s: FeatureId) -> HolePoint {
    let g = &d.d.element(d.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry;
    HolePoint { sketch: s, point: g.points.keys().next().unwrap() }
}

#[test]
fn hole_whole_sketch_merge_scope_and_parent_below() {
    // Two 60 × 60 × 30 blocks (x 0..60 and 100..160) and a sketch on Top with three points,
    // two under the first block and one under the second. PS15.1: the whole sketch picked puts
    // a hole at each of its points: Ø10 through all, up from Top, removes 3·π·5²·30.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 60.0, 60.0, 30.0);
    d.block(100.0, 0.0, 160.0, 60.0, 30.0);
    let v = Vec2::new;
    let points = [v(20.0, 30.0), v(40.0, 30.0), v(130.0, 30.0)];
    let s = d.sketch(PlaneRef::Top, points.iter().map(|p| SketchOp::AddPoint { pos: *p }).collect());
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(10.0);
    spec.end = HoleEnd::ThroughAll;
    spec.start = HoleStart::SketchPlane;
    let x = HoleFeature { sketches: vec![s], flip: true, spec, ..HoleFeature::default() };
    let hole = d.add(AddFeature::hole(d.el, FeatureId::new(), x.clone()));
    let hole_volume = PI * 25.0 * 30.0;
    close(d.volume(), 2.0 * 108_000.0 - 3.0 * hole_volume, 1e-6);
    // PS15.3: the merge scope holds only the first block: the second is left whole.
    let first = d.parts().into_iter().find(|p| p.mass.unwrap().center_of_mass.x < 60.0).unwrap().id;
    let scoped = HoleFeature { merge_scope: vec![first], ..x };
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(scoped), label: "Scope".into() })
        .unwrap();
    close(d.volume(), 2.0 * 108_000.0 - 2.0 * hole_volume, 1e-6);
    // Start from part: the block's point (off the scope) gets no hole rather than an error, and
    // the Top plane lying on the blocks' bottom faces still starts the others there.
    let mut from_part = d.features().iter().find(|f| f.id == hole).unwrap().hole().unwrap().clone();
    from_part.spec.start = HoleStart::Part;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(from_part), label: "Start".into() })
        .unwrap();
    close(d.volume(), 2.0 * 108_000.0 - 2.0 * hole_volume, 1e-6);
    // PS11.3: the sketch dragged below the hole: the hole fails, saying why. (A build of the
    // list without the sketch first: its cached "no longer exist" must not stand for it.)
    let without: Vec<_> = d.features().into_iter().filter(|f| f.id != s).collect();
    let lost = rebuild::build(&without);
    assert!(lost.error(hole).is_some_and(|w| w.contains("no longer exist")));
    let end = d.features().len() - 1;
    d.h.execute(&mut d.d, &MoveFeatures { element: d.el, features: vec![s], to: end, folder: None, label: "Reorder".into() })
        .unwrap();
    let b = d.build();
    let why = b.error(hole).expect("the hole fails");
    assert!(why.contains("Sketch") && why.contains("below this feature"), "{why}");
}

#[test]
fn counterbore_hole_closed_form() {
    // A blind counterbored hole Ø5 × 20 with a Ø10 × 4 counterbore and the 118° drill point:
    // V = π·5²·4 + π·2.5²·(20 − 4) + π·2.5²·h/3, h = 2.5 / tan 59°.
    let (mut d, s) = hole_doc(0.0);
    let mut spec = HoleSpec::default();
    spec.style = HoleStyle::Counterbore;
    spec.diameter = Length::mm(5.0);
    spec.depth = Length::mm(20.0);
    spec.cbore_diameter = Length::mm(10.0);
    spec.cbore_depth = Length::mm(4.0);
    let hole = d.add(AddFeature::hole(
        d.el,
        FeatureId::new(),
        HoleFeature { points: vec![point_of(&d, s)], spec: spec.clone(), ..HoleFeature::default() },
    ));
    let tip = 2.5 / 59f64.to_radians().tan();
    let removed = PI * 25.0 * 4.0 + PI * 6.25 * 16.0 + PI * 6.25 * tip / 3.0;
    close(d.volume(), 108_000.0 - removed, 1e-6);
    // The feature is named by its callout (PS15.10).
    assert_eq!(d.features().last().unwrap().name, "Ø 5 mm ↧ 20 mm | ⌴Ø 10 mm ↧ 4 mm");
    // Through all: no drill point, π·5²·4 + π·2.5²·26.
    let mut h = d.features().last().unwrap().hole().unwrap().clone();
    h.spec.end = HoleEnd::ThroughAll;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(h.clone()), label: "Through all".into() })
        .unwrap();
    close(d.volume(), 108_000.0 - PI * 100.0 - PI * 6.25 * 26.0, 1e-6);
    assert_eq!(d.features().last().unwrap().name, "Ø 5 mm THRU | ⌴Ø 10 mm ↧ 4 mm");
    // A rename sticks: the name no longer follows the callout.
    d.h.execute(&mut d.d, &RenameFeature { element: d.el, feature: hole, name: "Bolt hole".into() }).unwrap();
    h.spec.cbore_depth = Length::mm(5.0);
    h.renamed = true;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(h), label: "Depth".into() })
        .unwrap();
    assert_eq!(d.features().last().unwrap().name, "Bolt hole");
}

#[test]
fn hole_start_plane_and_up_to_next() {
    // PS15.6: the sketch plane 10 mm above the block. Start from part: the counterbore (Ø10 × 4)
    // starts at the block's top; start from sketch plane: the counterbore's 4 mm are all in the
    // air above the block, so only the Ø5 hole is cut (Through all: π·2.5²·30).
    let (mut d, s) = hole_doc(10.0);
    let mut spec = HoleSpec::default();
    spec.style = HoleStyle::Counterbore;
    spec.diameter = Length::mm(5.0);
    spec.cbore_diameter = Length::mm(10.0);
    spec.cbore_depth = Length::mm(4.0);
    spec.end = HoleEnd::ThroughAll;
    spec.start = HoleStart::Part;
    let hole = d.add(AddFeature::hole(
        d.el,
        FeatureId::new(),
        HoleFeature { points: vec![point_of(&d, s)], spec: spec.clone(), ..HoleFeature::default() },
    ));
    close(d.volume(), 108_000.0 - PI * 100.0 - PI * 6.25 * 26.0, 1e-6);
    let mut h = d.features().last().unwrap().hole().unwrap().clone();
    h.spec.start = HoleStart::SketchPlane;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(h.clone()), label: "Start".into() })
        .unwrap();
    close(d.volume(), 108_000.0 - PI * 6.25 * 30.0, 1e-6);
    // Up to next (a simple Ø5 hole): the full diameter reaches the block's bottom (30 deep from
    // the part), the drill point goes past it and cuts nothing more: π·2.5²·30.
    h.spec.style = HoleStyle::Simple;
    h.spec.start = HoleStart::Part;
    h.spec.end = HoleEnd::UpToNext;
    d.h.execute(&mut d.d, &SetFeature { element: d.el, feature: hole, kind: FeatureKind::Hole(h), label: "Up to next".into() })
        .unwrap();
    close(d.volume(), 108_000.0 - PI * 6.25 * 30.0, 1e-6);
}

#[test]
fn tapped_and_clearance_tables() {
    // Tapped M10×1.5 drills Ø8.5 (Blind 20 with the point): π·4.25²·20 + π·4.25²·h/3.
    let (mut d, s) = hole_doc(0.0);
    let mut spec = HoleSpec::default();
    spec.hole_type = HoleType::Tapped;
    spec.size = "M10".into();
    spec.apply_table();
    spec.depth = Length::mm(20.0);
    spec.tapped_depth = Length::mm(15.0);
    d.add(AddFeature::hole(d.el, FeatureId::new(), HoleFeature { points: vec![point_of(&d, s)], spec, ..HoleFeature::default() }));
    let r: f64 = 4.25;
    let tip = r / 59f64.to_radians().tan();
    close(d.volume(), 108_000.0 - PI * r * r * 20.0 - PI * r * r * tip / 3.0, 1e-6);
    // P3.10 (the P3.8 judge): the name gives the hole's depth, as the course's "M10x1.50 ↧ 20 mm".
    assert_eq!(d.features().last().unwrap().name, "M10x1.50 ↧ 20 mm");
}

#[test]
fn order_changes_the_result() {
    // PS11.4: an R1 fillet on the top face's edges placed after a hole also rounds the hole's
    // rim; dragged above the hole it doesn't. The difference is the rim's own fillet.
    let (mut d, s) = hole_doc(0.0);
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(10.0);
    spec.end = HoleEnd::ThroughAll;
    let hole = d.add(AddFeature::hole(d.el, FeatureId::new(), HoleFeature { points: vec![point_of(&d, s)], spec, ..HoleFeature::default() }));
    let part = d.parts()[0].clone();
    let top = face_at(&part, [0.0, 0.0, 1.0], [10.0, 10.0, 0.0]);
    let fillet = d.add(AddFeature::fillet(
        d.el,
        FeatureId::new(),
        FilletFeature { entities: vec![EdgeOrFace::Face(top)], size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() },
    ));
    let after = d.volume();
    // Dragged above the hole: the face then has only its four outer edges.
    let i = d.features().iter().position(|f| f.id == hole).unwrap();
    d.h.execute(&mut d.d, &MoveFeatures { element: d.el, features: vec![fillet], to: i, folder: None, label: "Reorder".into() })
        .unwrap();
    let before = d.volume();
    // After the hole the fillet also rounds the rim (a convex edge), removing more.
    assert!(before > after + 1.0, "{before} vs {after}");
    // Above the hole it rounds only the four outer edges: the same as on a block without the
    // hole, less the through hole π·5²·30.
    let (mut e, _) = hole_doc(0.0);
    let part = e.parts()[0].clone();
    let top = face_at(&part, [0.0, 0.0, 1.0], [10.0, 10.0, 0.0]);
    e.add(AddFeature::fillet(
        e.el,
        FeatureId::new(),
        FilletFeature { entities: vec![EdgeOrFace::Face(top)], size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() },
    ));
    let outer_only = 108_000.0 - e.volume();
    close(108_000.0 - PI * 25.0 * 30.0 - before, outer_only, 1e-6);
    // The rim of a Ø10 hole rounded by r 1 (Pappus): the removed section, the unit square less
    // the quarter disc about its far corner, has area 1 − π/4 and its centroid
    // (1/2 − (1 − 4/(3π))·π/4)/(1 − π/4) = (10 − 3π)/(3(4 − π)) = 0.2234 out from the corner,
    // so at radius 5.2234: 2π·5.2234·(1 − π/4) = 7.0431.
    let rim = 2.0 * PI * (5.0 + (10.0 - 3.0 * PI) / (3.0 * (4.0 - PI))) * (1.0 - PI / 4.0);
    close(before - after, rim, 1e-6);
}

#[test]
fn folders_hold_runs_of_features() {
    let mut d = Doc::new();
    let a = d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    let b = d.block(20.0, 0.0, 30.0, 10.0, 10.0);
    let feats: Vec<FeatureId> = d.features().iter().map(|f| f.id).collect();
    assert_eq!(feats.len(), 4);
    let folder = FeatureId::new();
    d.h.execute(&mut d.d, &CreateFolder { element: d.el, folder, name: None, features: feats[..2].to_vec() }).unwrap();
    let el = d.d.element(d.el).unwrap();
    assert_eq!(el.folders()[0].name, "Folder 1");
    assert_eq!(el.folder_of(a).map(|f| f.id), Some(folder));
    // Dragging Extrude 2 into the folder (between its features) makes it a member, and the
    // folder's features stay together.
    d.h.execute(&mut d.d, &MoveFeatures { element: d.el, features: vec![b], to: 1, folder: Some(folder), label: "Reorder".into() })
        .unwrap();
    let el = d.d.element(d.el).unwrap();
    assert_eq!(el.folders()[0].features.len(), 3);
    assert!(el.folders()[0].features.contains(&b));
    d.h.execute(&mut d.d, &SetFolder { element: d.el, folder, open: Some(true), name: Some("Base Features".into()) }).unwrap();
    assert!(d.d.element(d.el).unwrap().folders()[0].open);
    // Unpack: the features stay, the folder goes.
    d.h.execute(&mut d.d, &UnpackFolder { element: d.el, folder }).unwrap();
    assert!(d.d.element(d.el).unwrap().folders().is_empty());
    assert_eq!(d.features().len(), 4);
    d.h.undo(&mut d.d).unwrap();
    assert_eq!(d.d.element(d.el).unwrap().folders().len(), 1);
}

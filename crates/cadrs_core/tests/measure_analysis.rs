//! P3E.3 (TD6.6, PS2.11): the Measure tool and the draft analysis on parts the kernel built,
//! not on hand-made meshes. Every expected value is the geometry's own: a 100 × 60 × 25 box
//! reads 100 / 60 / 25, its top 6000 mm² and two neighbouring faces 90°; two boxes 15 apart read
//! 15; a Ø40 cylinder reads radius 20; a face drafted 5° by the Draft feature falls in the
//! draft analysis's 3°–6° band.
#![cfg(feature = "occt")]

use cadrs_core::analysis::{DraftBand, draft_angle};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{BooleanOp, Document, ExtrudeFeature, FaceRef};
use cadrs_core::draft::DraftFeature;
use cadrs_core::measure::{self, Entity, Mode};
use cadrs_core::pattern::MirrorPlane;
use cadrs_core::rebuild;
use cadrs_core::samples;
use cadrs_core::{ElementId, FeatureId, History, Part};
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
        let d = Document::new("P3E.3 measure");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn parts(&self) -> Vec<Part> {
        let b = rebuild::build(self.d.element(self.el).unwrap().features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }

    fn extrude(&mut self, ops: Vec<SketchOp>, seed: Vec2, depth: f64) -> FeatureId {
        let s = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: s, plane: Some(PlaneRef::Top) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: s, op }).unwrap();
        }
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, &[seed]);
        let e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, depth) };
        let id = FeatureId::new();
        self.h.execute(&mut self.d, &AddExtrude { element: self.el, feature: id, extrude: ExtrudeFeature::default() }).unwrap();
        self.h.execute(&mut self.d, &SetExtrude { element: self.el, feature: id, extrude: e, label: "Extrude".into() }).unwrap();
        id
    }

    fn block(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let v = Vec2::new;
        let rect = SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" };
        self.extrude(vec![rect], v((x0 + x1) / 2.0, (y0 + y1) / 2.0), h)
    }
}

/// The index of the planar face of `part` with outward normal `n` through `p`.
fn face_index(part: &Part, n: [f64; 3], p: [f64; 3]) -> usize {
    let s = &part.solid;
    (0..s.faces.len())
        .find(|&i| s.face_normal(i).is_some_and(|m| m[0] * n[0] + m[1] * n[1] + m[2] * n[2] > 0.999) && s.face_contains(i, p))
        .expect("a face there")
}

fn face(part: &Part, n: [f64; 3], p: [f64; 3]) -> Entity {
    let i = face_index(part, n, p);
    Entity::face(&part.solid, &part.solid.faces[i])
}

/// The straight edge of `part` from `a` to `b` (either way round).
fn edge(part: &Part, a: [f64; 3], b: [f64; 3]) -> Entity {
    let near = |p: [f64; 3], q: [f64; 3]| (0..3).all(|k| (p[k] - q[k]).abs() < 1e-6);
    let e = part
        .solid
        .edges
        .iter()
        .find(|e| {
            let (f, l) = (e.points[0], *e.points.last().unwrap());
            (near(f, a) && near(l, b)) || (near(f, b) && near(l, a))
        })
        .expect("an edge there");
    Entity::edge(e)
}

#[test]
fn a_100_by_60_by_25_box() {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 60.0, 25.0);
    let p = &d.parts()[0];
    for (a, b, l) in [([0.0, 0.0, 0.0], [100.0, 0.0, 0.0], 100.0), ([0.0, 0.0, 0.0], [0.0, 60.0, 0.0], 60.0), ([0.0, 0.0, 0.0], [0.0, 0.0, 25.0], 25.0)] {
        close(measure::measure(&[edge(p, a, b)], Mode::Minimum).length.unwrap(), l, 1e-6);
    }
    let top = face(p, [0.0, 0.0, 1.0], [50.0, 30.0, 25.0]);
    close(measure::measure(std::slice::from_ref(&top), Mode::Minimum).area.unwrap(), 6000.0, 1e-6);
    // Top and front meet at 90°; top to bottom is the 25 height.
    let front = face(p, [0.0, -1.0, 0.0], [50.0, 0.0, 12.0]);
    close(measure::measure(&[top.clone(), front], Mode::Minimum).angle.unwrap(), 90.0, 1e-6);
    let bottom = face(p, [0.0, 0.0, -1.0], [50.0, 30.0, 0.0]);
    close(measure::measure(&[top, bottom], Mode::Minimum).distance.unwrap().value, 25.0, 1e-6);
}

#[test]
fn two_boxes_15_apart() {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 20.0, 20.0, 20.0);
    d.block(35.0, 5.0, 55.0, 25.0, 10.0);
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    let (a, b) = (Entity::part(&parts[0].solid), Entity::part(&parts[1].solid));
    let m = measure::measure(&[a, b], Mode::Minimum);
    close(m.distance.unwrap().value, 15.0, 1e-6);
    close(m.distance.unwrap().components()[0].abs(), 15.0, 1e-6);
}

#[test]
fn a_40_diameter_cylinder() {
    let mut d = Doc::new();
    let circle = SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 20.0, construction: false };
    d.extrude(vec![circle], Vec2::new(0.0, 0.0), 30.0);
    let p = &d.parts()[0];
    let i = (0..p.solid.faces.len()).find(|&i| p.solid.faces[i].plane.is_none()).expect("the cylinder's side");
    let m = measure::measure(&[Entity::face(&p.solid, &p.solid.faces[i])], Mode::Minimum);
    close(m.radius.unwrap(), 20.0, 1e-6);
}

#[test]
fn a_5_degree_drafted_face_falls_in_the_3_to_6_band() {
    // A 100 mm cube's +X side drafted 5° about Top: the pull direction is +Z, and the side
    // leans in as it rises, so its outward normal tips 5° up.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 100.0, 100.0);
    let cube = d.parts()[0].clone();
    let i = face_index(&cube, [1.0, 0.0, 0.0], [100.0, 50.0, 50.0]);
    let side = FaceRef { part: cube.id, face: cube.solid.faces[i].name, seed: [100.0, 50.0, 50.0] };
    let x = DraftFeature { neutral: Some(MirrorPlane::Plane(PlaneRef::Top)), faces: vec![side], angle: 5.0, angle_expr: "5 deg".into(), ..DraftFeature::default() };
    d.h.execute(&mut d.d, &AddFeature::draft(d.el, FeatureId::new(), x)).unwrap();
    let drafted = d.parts()[0].clone();
    let f = (0..drafted.solid.faces.len()).find(|&k| drafted.solid.faces[k].name == side.face).expect("the drafted face keeps its name");
    let n = drafted.solid.face_normal(f).unwrap();
    let a = draft_angle(n, [0.0, 0.0, 1.0]);
    close(a, 5.0, 1e-6);
    assert_eq!(DraftBand::of(a, 3.0), DraftBand::Positive);
    assert_eq!(DraftBand::of(a, 3.0).label(3.0), "3° to 6°");
    // The undrafted −X side is vertical: 0°, not enough draft.
    let j = face_index(&drafted, [-1.0, 0.0, 0.0], [0.0, 50.0, 50.0]);
    let b = draft_angle(drafted.solid.face_normal(j).unwrap(), [0.0, 0.0, 1.0]);
    close(b, 0.0, 1e-6);
    assert_eq!(DraftBand::of(b, 3.0), DraftBand::InsufficientPositive);
}

/// A1.9 in an assembly: the draft analysis reads the instance's faces as placed. With the
/// drafted cube's instance turned 90° about X (its +Z now −Y… its top facing −Y) and moved, the
/// pull direction taken from the instance's top face and the drafted side's normal are both
/// carried by the pose: the side still reads +5° against that pull, and not against world +Z.
#[test]
fn a_rotated_instance_carries_its_pull_direction_and_normals() {
    use cadrs_core::assembly::{Pose, transform_solid};
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 100.0, 100.0);
    let cube = d.parts()[0].clone();
    let i = face_index(&cube, [1.0, 0.0, 0.0], [100.0, 50.0, 50.0]);
    let side = FaceRef { part: cube.id, face: cube.solid.faces[i].name, seed: [100.0, 50.0, 50.0] };
    let x = DraftFeature { neutral: Some(MirrorPlane::Plane(PlaneRef::Top)), faces: vec![side], angle: 5.0, angle_expr: "5 deg".into(), ..DraftFeature::default() };
    d.h.execute(&mut d.d, &AddFeature::draft(d.el, FeatureId::new(), x)).unwrap();
    let drafted = d.parts()[0].clone();
    let top = face_index(&drafted, [0.0, 0.0, 1.0], [50.0, 50.0, 100.0]);
    let f = (0..drafted.solid.faces.len()).find(|&k| drafted.solid.faces[k].name == side.face).unwrap();
    let pose = Pose::rotation_about([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], std::f64::consts::FRAC_PI_2).then(&Pose::translation([40.0, 10.0, 5.0]));
    let placed = transform_solid(&drafted.solid, &pose);
    // The pull from the placed top face: the rotated +Z.
    let pull = placed.face_normal(top).unwrap();
    let want = pose.rotate([0.0, 0.0, 1.0]);
    for k in 0..3 {
        close(pull[k], want[k], 1e-9);
    }
    assert!(pull[2].abs() < 1e-9, "the top no longer faces +Z: {pull:?}");
    // The drafted side against it: still +5°, the 3°–6° band.
    let n = placed.face_normal(f).unwrap();
    let a = draft_angle(n, pull);
    close(a, 5.0, 1e-6);
    assert_eq!(DraftBand::of(a, 3.0), DraftBand::Positive);
    // Against world +Z the turned side reads otherwise (the analysis must use the placed pull).
    assert!((draft_angle(n, [0.0, 0.0, 1.0]) - 5.0).abs() > 1.0);
    // The per-vertex normals the shader colours by are turned too.
    let tri = placed.faces[f].first_triangle;
    let vi = placed.indices[3 * tri] as usize;
    close(draft_angle(placed.normals[vi], pull), 5.0, 1e-3);
}

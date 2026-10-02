//! P3.3: extrude end types and booleans, whole-sketch input, the Boolean feature, parts and
//! their mass properties, through the document's commands and the rebuild. Every expected value
//! is derived by hand in the test's comment.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::commands::{AddExtrude, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{
    BodyType, BooleanFeature, BooleanKind, BooleanOp, DeletePartFeature, Document, EndType,
    ExtrudeFeature, FaceRef, Offset, UpTo,
};
use cadrs_core::parts::{PartKind, combined_mass};
use cadrs_core::rebuild;
use cadrs_core::samples::{self, EXTRUDE_1_SEEDS, EXTRUDE_2_SEEDS};
use cadrs_core::{ElementId, Feature, FeatureId, FeatureKind, History, Part, PartId};
use cadrs_sketch::{FaceOrigin, PlaneRef, Sketch, SketchOp, Vec2};

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3.3");
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn g(&self, f: FeatureId) -> Sketch {
        self.d.element(self.el).unwrap().feature(f).unwrap().sketch().unwrap().geometry.clone()
    }

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    /// An extrude of the regions of `sketch` under `seeds`, set up by `edit`.
    fn extrude(&mut self, sketch: FeatureId, seeds: &[Vec2], edit: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let refs = samples::region_refs(sketch, &self.g(sketch), seeds);
        assert_eq!(refs.len(), seeds.len(), "every seed is in a region");
        let mut e = ExtrudeFeature { regions: refs, ..ExtrudeFeature::default() };
        edit(&mut e);
        self.add_extrude(e)
    }

    fn add_extrude(&mut self, extrude: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude, label: "Extrude".into() })
            .unwrap();
        f
    }

    /// Appends a feature of any kind (the Boolean and Delete part features).
    fn push(&mut self, name: &str, kind: FeatureKind) -> FeatureId {
        let f = FeatureId::new();
        let el = self.d.element_mut(self.el).unwrap();
        el.features_mut().unwrap().push(Feature { id: f, name: name.into(), kind, suppress_by: None });
        f
    }

    fn build(&self) -> std::sync::Arc<rebuild::Build> {
        rebuild::build(&self.features())
    }

    fn parts(&self) -> Vec<Part> {
        let b = self.build();
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
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

fn circle(x: f64, y: f64, r: f64) -> SketchOp {
    SketchOp::AddCircle { center: Vec2::new(x, y), radius: r, construction: false }
}

fn volume(p: &Part) -> f64 {
    p.mass.unwrap().volume
}

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

fn depth(d: f64) -> impl FnOnce(&mut ExtrudeFeature) {
    move |e| {
        e.depth = d;
        e.depth_expr = format!("{d} mm");
    }
}

/// PS6: the Control Arm as the course builds it: Extrude 1 New (hub ring, right web, right eye
/// ring) 40 mm, Extrude 2 **Add** (left web, left eye ring) 25 mm: one part. The course's
/// self-check reads Volume 368 749.705 mm³ and Surface area 50 179.71 mm² (the area: bottom
/// 10 704.256, the tops the same in two levels, the outer and hole walls, and the 15 mm step
/// wall where the hub meets the left web; the OCCT spike found 50 179.711). Both to 0.01.
#[test]
fn control_arm_self_check() {
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![samples::control_arm_geometry()]);
    let e1 = doc.extrude(s1, &EXTRUDE_1_SEEDS, depth(samples::EXTRUDE_1_DEPTH));
    // As New first: its body touches Part 1, so the dialog picks Add (PS5.2).
    let e2 = doc.extrude(s1, &EXTRUDE_2_SEEDS, depth(samples::EXTRUDE_2_DEPTH));
    let b = doc.build();
    assert_eq!(b.parts.len(), 2);
    let touches = &b.contacts[&e2].touches;
    assert_eq!(touches, &vec![PartId::new(e1, 0)], "Extrude 2 touches Part 1");
    assert!(b.contacts[&e2].overlaps.is_empty(), "only along faces, no overlap");
    // Add.
    let mut e = doc.features().iter().find(|f| f.id == e2).unwrap().extrude().unwrap().clone();
    e.op = BooleanOp::Add;
    doc.h.execute(&mut doc.d, &SetExtrude { element: doc.el, feature: e2, extrude: e, label: "Add".into() }).unwrap();
    let ps = doc.parts();
    assert_eq!(ps.len(), 1, "one part");
    let arm = &ps[0];
    assert_eq!(arm.name, "Part 1");
    assert_eq!(arm.id, PartId::new(e1, 0));
    assert_eq!(arm.features, vec![e1, e2]);
    let m = arm.mass.unwrap();
    close(m.volume, 368_749.705, 0.01);
    close(m.surface_area, 50_179.71, 0.01);
    // Symmetric about the sketch's centreline in x apart from the thicknesses: the centre of mass
    // lies on y = 0, towards the thicker right half, between the two heights.
    close(m.center_of_mass.y, 0.0, 1e-6);
    assert!(m.center_of_mass.x > 0.0);
    assert!(m.center_of_mass.z > 25.0 / 2.0 && m.center_of_mass.z < 40.0 / 2.0);
    // The panel's "Parts to measure" with the one part: the same numbers.
    let c = combined_mass(ps.iter()).unwrap();
    close(c.volume, m.volume, 1e-9);
}

/// PS5.3 on two boxes: A = 100 × 60 × 40 at the origin, B = (50..150) × (20..80) × 25.
/// Their overlap is 50 · 40 · 25 = 50 000. Remove: 100·60·40 − 50 000 = 190 000.
/// Intersect: 50 000. Add: 240 000 + 100·60·25 − 50 000 = 340 000, one part.
#[test]
fn remove_intersect_add_two_boxes() {
    for (op, expected, count) in [
        (BooleanOp::Remove, 190_000.0, 1),
        (BooleanOp::Intersect, 50_000.0, 1),
        (BooleanOp::Add, 340_000.0, 1),
        (BooleanOp::New, 240_000.0, 2),
    ] {
        let mut doc = Doc::new();
        let sa = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 60.0)]);
        let ea = doc.extrude(sa, &[Vec2::new(10.0, 10.0)], depth(40.0));
        let sb = doc.sketch(PlaneRef::Top, vec![rect(50.0, 20.0, 150.0, 80.0)]);
        let eb = doc.extrude(sb, &[Vec2::new(120.0, 50.0)], |e| {
            e.depth = 25.0;
            e.op = op;
        });
        let b = doc.build();
        assert_eq!(b.contacts[&eb].overlaps, vec![PartId::new(ea, 0)]);
        let ps = doc.parts();
        assert_eq!(ps.len(), count, "{op:?}");
        close(volume(&ps[0]), expected, 1e-6);
        assert_eq!(ps[0].name, "Part 1", "{op:?}: A keeps its identity");
    }
}

/// A Remove that splits a part: a 100 × 60 × 40 box cut by a slot x = 30..50 through it leaves
/// 30·60·40 = 72 000 and 50·60·40 = 120 000. The larger piece keeps "Part 1", the other is a new
/// "Part 2".
#[test]
fn a_remove_that_splits_a_part() {
    let mut doc = Doc::new();
    let sa = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 60.0)]);
    let ea = doc.extrude(sa, &[Vec2::new(10.0, 10.0)], depth(40.0));
    let ss = doc.sketch(PlaneRef::Top, vec![rect(30.0, -10.0, 50.0, 70.0)]);
    let es = doc.extrude(ss, &[Vec2::new(40.0, 0.0)], |e| {
        e.op = BooleanOp::Remove;
        e.end = EndType::ThroughAll;
    });
    let ps = doc.parts();
    assert_eq!(ps.len(), 2);
    assert_eq!((ps[0].id, ps[0].name.as_str()), (PartId::new(ea, 0), "Part 1"));
    close(volume(&ps[0]), 120_000.0, 1e-6);
    assert_eq!((ps[1].id, ps[1].name.as_str()), (PartId::new(es, 0), "Part 2"));
    close(volume(&ps[1]), 72_000.0, 1e-6);
}

/// PS1.1: a whole sketch is its outer boundary less the loops inside it: a 100 × 60 plate with
/// a Ø20 circle inside, and a Ø8 circle inside that (a boss in the hole, even-odd), 10 deep:
/// V = (6000 − π·10² + π·4²)·10.
#[test]
fn whole_sketch_input() {
    let mut doc = Doc::new();
    let s = doc.sketch(
        PlaneRef::Top,
        vec![rect(0.0, 0.0, 100.0, 60.0), circle(50.0, 30.0, 10.0), circle(50.0, 30.0, 4.0)],
    );
    let e = ExtrudeFeature {
        sketches: vec![s],
        depth: 10.0,
        depth_expr: "10 mm".into(),
        ..ExtrudeFeature::default()
    };
    doc.add_extrude(e);
    let ps = doc.parts();
    assert_eq!(ps.len(), 2, "the plate with its hole, and the boss in the hole");
    let total: f64 = ps.iter().map(volume).sum();
    close(total, (6000.0 - PI * 100.0 + PI * 16.0) * 10.0, 1e-6);
}

/// End types through the feature list (PS4.3, PS4.7):
/// - a plate z = 30..40 (a Starting offset of 30, depth 10): 100·60·10 = 60 000;
/// - a 20 × 20 square up to the plate's underside (Up to face): 400·30 = 12 000;
/// - with an offset of 5 stopping short: 400·25 = 10 000;
/// - Symmetric 30 of a Ø10 circle: from z = −15 to 15, π·25·30;
/// - a Ø10 hole Through all the plate (Remove): 60 000 − π·25·10.
#[test]
fn end_types() {
    let mut doc = Doc::new();
    let sp = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 60.0)]);
    let plate = doc.extrude(sp, &[Vec2::new(10.0, 10.0)], |e| {
        e.depth = 10.0;
        e.start_offset = Some(Offset { value: 30.0, expr: "30 mm".into(), flip: false });
    });
    let ps = doc.parts();
    close(volume(&ps[0]), 60_000.0, 1e-6);
    // The plate's underside: its start cap, at z = 30.
    let under = ps[0]
        .solid
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| (p.origin[2] - 30.0).abs() < 1e-9))
        .unwrap();
    assert!(matches!(under.name.origin, FaceOrigin::Cap { end: false, .. }));
    let under = under.name;
    let seed = [10.0, 10.0, 30.0];
    let face = FaceRef { part: PartId::new(plate, 0), face: under, seed };
    let ss = doc.sketch(PlaneRef::Top, vec![rect(40.0, 20.0, 60.0, 40.0)]);
    let up = doc.extrude(ss, &[Vec2::new(50.0, 30.0)], |e| {
        e.end = EndType::UpToFace;
        e.up_to = Some(UpTo::Face(face));
    });
    let ps = doc.parts();
    assert_eq!(ps.len(), 2, "New: a separate part");
    close(volume(&ps[1]), 12_000.0, 1e-6);
    // An offset of 5 stops short.
    let mut e = doc.features().iter().find(|f| f.id == up).unwrap().extrude().unwrap().clone();
    e.offset = Some(Offset { value: 5.0, expr: "5 mm".into(), flip: false });
    doc.h.execute(&mut doc.d, &SetExtrude { element: doc.el, feature: up, extrude: e, label: "Offset".into() }).unwrap();
    close(volume(&doc.parts()[1]), 10_000.0, 1e-6);
    // Symmetric.
    let sc = doc.sketch(PlaneRef::Top, vec![circle(200.0, 0.0, 5.0)]);
    doc.extrude(sc, &[Vec2::new(200.0, 0.0)], |e| {
        e.depth = 30.0;
        e.symmetric = true;
    });
    let ps = doc.parts();
    close(volume(&ps[2]), PI * 25.0 * 30.0, 1e-6);
    // Through all, Remove: a hole through the plate from the Top plane.
    let sh = doc.sketch(PlaneRef::Top, vec![circle(20.0, 45.0, 5.0)]);
    doc.extrude(sh, &[Vec2::new(20.0, 45.0)], |e| {
        e.op = BooleanOp::Remove;
        e.end = EndType::ThroughAll;
    });
    let ps = doc.parts();
    close(volume(&ps[0]), 60_000.0 - PI * 25.0 * 10.0, 1e-6);
}

/// PS5.4: a bar that bridges two boxes. Add with the automatic scope joins all three into one
/// part (PS5.3); with the scope set to the first box only, the second stays separate.
/// A = 0..40 × 0..40, C = 60..100 × 0..40, bar = 30..70 × 10..30, all 10 deep:
/// 1600·10 + 1600·10 + (40·20 − 2·10·20)·10 = 36 000 in all.
#[test]
fn merge_scope() {
    for explicit in [false, true] {
        let mut doc = Doc::new();
        let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 40.0, 40.0)]);
        let a = doc.extrude(s, &[Vec2::new(5.0, 5.0)], depth(10.0));
        let s = doc.sketch(PlaneRef::Top, vec![rect(60.0, 0.0, 100.0, 40.0)]);
        doc.extrude(s, &[Vec2::new(90.0, 5.0)], depth(10.0));
        let s = doc.sketch(PlaneRef::Top, vec![rect(30.0, 10.0, 70.0, 30.0)]);
        doc.extrude(s, &[Vec2::new(50.0, 20.0)], |e| {
            e.depth = 10.0;
            e.op = BooleanOp::Add;
            if explicit {
                e.merge_scope = vec![PartId::new(a, 0)];
            }
        });
        let ps = doc.parts();
        assert_eq!(ps.len(), if explicit { 2 } else { 1 });
        let total: f64 = ps.iter().map(volume).sum();
        // Explicit: the bar overlaps C by 10·20·10 counted twice.
        let want = if explicit { 36_000.0 + 2000.0 } else { 36_000.0 };
        close(total, want, 1e-6);
    }
}

/// PS5.5: the Boolean feature on two overlapping boxes A (Part 1) and B (Part 2), the same as in
/// `remove_intersect_add_two_boxes` (overlap 50 000; A 240 000, B 150 000):
/// - Union of B then A: one part, 340 000, taking the first tool's identity (Part 2);
/// - Subtract B from A, keeping the tools: A 190 000 and B 150 000;
/// - Intersect: 50 000 as Part 1.
///   Then Delete part removes a part.
#[test]
fn boolean_feature() {
    let make = || {
        let mut doc = Doc::new();
        let sa = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 60.0)]);
        let a = doc.extrude(sa, &[Vec2::new(10.0, 10.0)], depth(40.0));
        let sb = doc.sketch(PlaneRef::Top, vec![rect(50.0, 20.0, 150.0, 80.0)]);
        let b = doc.extrude(sb, &[Vec2::new(120.0, 50.0)], depth(25.0));
        (doc, PartId::new(a, 0), PartId::new(b, 0))
    };
    let (mut doc, a, b) = make();
    doc.push(
        "Boolean 1",
        FeatureKind::Boolean(BooleanFeature { op: BooleanKind::Union, tools: vec![b, a], targets: vec![], keep_tools: false, offset: None }),
    );
    let ps = doc.parts();
    assert_eq!(ps.len(), 1);
    assert_eq!((ps[0].id, ps[0].name.as_str()), (b, "Part 2"));
    close(volume(&ps[0]), 340_000.0, 1e-6);

    let (mut doc, a, b) = make();
    doc.push(
        "Boolean 1",
        FeatureKind::Boolean(BooleanFeature { op: BooleanKind::Subtract, tools: vec![b], targets: vec![a], keep_tools: true, offset: None }),
    );
    let ps = doc.parts();
    assert_eq!(ps.len(), 2);
    close(volume(&ps[0]), 190_000.0, 1e-6);
    close(volume(&ps[1]), 150_000.0, 1e-6);

    let (mut doc, a, b) = make();
    doc.push(
        "Boolean 1",
        FeatureKind::Boolean(BooleanFeature { op: BooleanKind::Intersect, tools: vec![a, b], targets: vec![], keep_tools: false, offset: None }),
    );
    let ps = doc.parts();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].id, a);
    close(volume(&ps[0]), 50_000.0, 1e-6);

    let (mut doc, a, _) = make();
    doc.push("Delete part 1", FeatureKind::DeletePart(DeletePartFeature { parts: vec![a] }));
    let ps = doc.parts();
    assert_eq!(ps.len(), 1);
    assert_eq!(ps[0].name, "Part 2");
}

/// PS4.10, PS4.11: a surface extrude of a 20 × 10 rectangle 5 deep is "Surface 1": area
/// 2·(20 + 10)·5 = 300, no volume. A thin extrude of a 20 × 20 square, 2 mm inside, 10 deep:
/// (20² − 16²)·10 = 1440; with Mid plane (1 each side): (22² − 18²)·10 = 1600.
#[test]
fn surface_and_thin() {
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 20.0, 10.0)]);
    doc.extrude(s, &[Vec2::new(5.0, 5.0)], |e| {
        e.depth = 5.0;
        e.body = BodyType::Surface;
    });
    let ps = doc.parts();
    assert_eq!((ps[0].kind, ps[0].name.as_str()), (PartKind::Surface, "Surface 1"));
    let m = ps[0].mass.unwrap();
    close(m.surface_area, 300.0, 1e-6);
    close(m.volume, 0.0, 1e-9);

    for (mid, want) in [(false, 1440.0), (true, 1600.0)] {
        let mut doc = Doc::new();
        let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 20.0, 20.0)]);
        doc.extrude(s, &[Vec2::new(5.0, 5.0)], |e| {
            e.depth = 10.0;
            e.body = BodyType::Thin;
            e.thin.thickness1 = 2.0;
            e.thin.mid_plane = mid;
        });
        let ps = doc.parts();
        assert_eq!(ps[0].kind, PartKind::Solid);
        close(volume(&ps[0]), want, 1e-6);
    }
}

/// PS4.2: a planar part face as input. The top face of a 100 × 60 × 40 box extruded 10 up, Add:
/// one part of 100·60·50 = 300 000.
#[test]
fn a_face_as_input() {
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 60.0)]);
    let a = doc.extrude(s, &[Vec2::new(10.0, 10.0)], depth(40.0));
    let ps = doc.parts();
    let top = ps[0]
        .solid
        .faces
        .iter()
        .find(|f| f.plane.is_some_and(|p| (p.origin[2] - 40.0).abs() < 1e-9))
        .unwrap()
        .name;
    let e = ExtrudeFeature {
        faces: vec![FaceRef { part: PartId::new(a, 0), face: top, seed: [50.0, 30.0, 40.0] }],
        depth: 10.0,
        depth_expr: "10 mm".into(),
        op: BooleanOp::Add,
        ..ExtrudeFeature::default()
    };
    doc.add_extrude(e);
    let ps = doc.parts();
    assert_eq!(ps.len(), 1);
    close(volume(&ps[0]), 300_000.0, 1e-6);
}

/// A feature that fails leaves the parts as they were: Remove with nothing to cut.
#[test]
fn a_failed_boolean_changes_nothing() {
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0)]);
    doc.extrude(s, &[Vec2::new(5.0, 5.0)], depth(10.0));
    let s = doc.sketch(PlaneRef::Top, vec![rect(50.0, 50.0, 60.0, 60.0)]);
    let r = doc.extrude(s, &[Vec2::new(55.0, 55.0)], |e| e.op = BooleanOp::Remove);
    let b = doc.build();
    assert!(b.error(r).is_some());
    assert_eq!(b.parts.len(), 1);
    close(volume(&b.parts[0]), 1000.0, 1e-6);
}

/// Mass properties of several parts together: sums and the volume-weighted centre. Two cubes of
/// 10 at x = 0..10 and 20..40 (the second 20 × 10 × 10): V = 1000 + 2000, x̄ = (5·1000 +
/// 30·2000)/3000 = 21.667.
#[test]
fn combined_mass_properties() {
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 40.0, 10.0)]);
    doc.extrude(s, &[Vec2::new(5.0, 5.0), Vec2::new(30.0, 5.0)], depth(10.0));
    let ps = doc.parts();
    assert_eq!(ps.len(), 2, "two separate regions make two parts");
    let m = combined_mass(ps.iter()).unwrap();
    close(m.volume, 3000.0, 1e-6);
    close(m.surface_area, 600.0 + 1000.0, 1e-6);
    close(m.center_of_mass.x, (5.0 * 1000.0 + 30.0 * 2000.0) / 3000.0, 1e-9);
}

/// The Control Arm sketch with `samples::control_arm_constraints` is fully defined (the course
/// accepts it that way, PS6.2), and solving doesn't move it: the regions are the same.
#[test]
fn control_arm_sketch_is_fully_defined() {
    let mut g = Sketch::new();
    samples::control_arm_geometry().apply(&mut g).unwrap();
    let before = cadrs_sketch::region::regions(&g).len();
    let op = samples::control_arm_constraints(&g);
    op.apply(&mut g).unwrap();
    cadrs_sketch::solve::solve(&mut g);
    let a = cadrs_sketch::solve::analyze(&g);
    assert!(!a.has_conflicts(), "conflicts: {:?} {:?}", a.conflicting, a.conflicting_dimensions);
    assert!(a.fully_constrained(), "{} degrees of freedom left", a.dof);
    assert_eq!(cadrs_sketch::region::regions(&g).len(), before);
}

/// The P3.3 commands (part rename and hide, the sketch eye, a Boolean, a Delete part) undo to
/// exactly the state before and redo to the state after, and a document with every new kind of
/// state (the extrude options, the Boolean, part settings, sketch visibility) survives
/// save → reload unchanged.
#[test]
fn new_commands_undo_and_documents_round_trip() {
    use cadrs_core::commands::{AddFeature, RenamePart, SetPartsHidden, SetSketchVisibility};
    use cadrs_core::{Command, DocumentMeta, Store};
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 30.0, 10.0)]);
    let e = doc.extrude(s, &[Vec2::new(5.0, 5.0), Vec2::new(25.0, 5.0)], |e| {
        e.depth = 10.0;
        e.end = EndType::Blind;
        e.symmetric = true;
        e.start_offset = Some(Offset::default());
        e.thin.mid_plane = true;
        e.merge_all = true;
    });
    let (a, b) = (PartId::new(e, 0), PartId::new(e, 1));
    let el = doc.el;
    let check = |doc: &mut Doc, cmd: &dyn Command| {
        let before = doc.d.clone();
        doc.h.execute(&mut doc.d, cmd).unwrap();
        let after = doc.d.clone();
        assert_ne!(before, after, "{} changed nothing", cmd.label());
        doc.h.undo(&mut doc.d).unwrap();
        assert_eq!(doc.d, before, "undo of {}", cmd.label());
        doc.h.redo(&mut doc.d).unwrap();
        assert_eq!(doc.d, after, "redo of {}", cmd.label());
    };
    check(&mut doc, &RenamePart { element: el, part: a, name: "Control Arm".into() });
    check(&mut doc, &SetPartsHidden { element: el, parts: vec![b], hidden: true });
    check(&mut doc, &SetSketchVisibility { element: el, sketch: s, visible: Some(true) });
    check(
        &mut doc,
        &AddFeature::boolean(
            el,
            FeatureId::new(),
            BooleanFeature { op: BooleanKind::Subtract, tools: vec![b], targets: vec![a], keep_tools: true, offset: None },
        ),
    );
    check(&mut doc, &AddFeature::delete_parts(el, FeatureId::new(), vec![b]));
    let names: Vec<String> = doc.features().iter().map(|f| f.name.clone()).collect();
    assert_eq!(names, ["Sketch 1", "Extrude 1", "Boolean 1", "Delete part 1"]);
    let props = doc.d.element(el).unwrap().part_props().to_vec();
    assert_eq!(props.len(), 2);
    assert_eq!(doc.d.element(el).unwrap().sketch_visibility(s), Some(true));

    let dir = std::env::temp_dir().join(format!("cadrs-p33-roundtrip-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::new(&dir);
    let meta = DocumentMeta::new("me", 1_000);
    store.create(&doc.d, &meta).unwrap();
    let back = store.load(doc.d.id).unwrap();
    assert_eq!(back.document, doc.d);
    let _ = std::fs::remove_dir_all(dir);
}

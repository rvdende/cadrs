//! P3.10: the Part Studios remainder through the feature list: Draft (PS4.9) and the extrude's
//! Draft option (X3), the hole's start plane, Up to entity and warnings (PS15.6–15.7, PS11.1),
//! variable and asymmetric fillets (PS14.6), a fillet on several parts (PS18.3), the Boolean's
//! offset and tool order (PS5.5), Loft Match tangent from a face (PS20.4), a sweep of a whole
//! sketch (PS1.6) and the Mass properties options (X7). Every expected value is derived by hand
//! in the test's comment, from the geometry, not from a run.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::advanced::{LoftCondition, LoftFeature, LoftProfile, PathRef, SweepFeature};
use cadrs_core::applied::{EdgeOrFace, EdgePoint, FilletFeature, HoleFeature, HolePoint, PartialBound, VertexRadius};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::document::{BooleanFeature, BooleanKind, BooleanOffset, BooleanOp, Document, EdgeRef, ExtrudeFeature, FaceRef, FeatureKind, VertexRef};
use cadrs_core::draft::{DraftFeature, ExtrudeDraft};
use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, Length};
use cadrs_core::parts::{MassOptions, mass_report_with};
use cadrs_core::pattern::MirrorPlane;
use cadrs_core::plane::{PlaneEntity, PlaneFeature, PlaneType};
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
        let d = Document::new("P3.10");
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

    fn geometry(&self, s: FeatureId) -> cadrs_sketch::Sketch {
        self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone()
    }

    /// An extrude of the regions of sketch `s` around `seeds`, with `f` applied to it.
    fn extrude(&mut self, s: FeatureId, seeds: &[Vec2], depth: f64, f: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let g = self.geometry(s);
        let regions = samples::region_refs(s, &g, seeds);
        let mut e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, depth) };
        f(&mut e);
        let id = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: id, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: id, extrude: e, label: "Extrude".into() })
            .unwrap();
        id
    }

    /// A box x0..x1 × y0..y1 on Top, `h` high (New).
    fn block(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, vec![rect(x0, y0, x1, y1)]);
        self.extrude(s, &[Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)], h, |_| {})
    }

    fn add(&mut self, a: AddFeature) -> FeatureId {
        let id = a.feature;
        self.h.execute(&mut self.d, &a).unwrap();
        id
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature, kind, label: "Edit".into() }).unwrap();
    }

    fn part_at(&self, p: [f64; 3]) -> Part {
        self.parts()
            .into_iter()
            .find(|x| x.solid.faces.iter().enumerate().any(|(i, _)| x.solid.face_contains(i, p)))
            .expect("a part there")
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
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

fn curved_face(part: &Part) -> FaceRef {
    let s = &part.solid;
    let i = (0..s.faces.len()).find(|&i| s.faces[i].plane.is_none()).expect("a curved face");
    let seed = s.face_point(i).unwrap();
    FaceRef { part: part.id, face: s.faces[i].name, seed }
}

fn edge_at(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part.solid.edges.iter().min_by(|a, b| a.distance(p).total_cmp(&b.distance(p))).unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

fn vertex_at(part: &Part, p: [f64; 3]) -> VertexRef {
    let v = part
        .solid
        .vertices
        .iter()
        .find(|v| (0..3).all(|k| (v.point[k] - p[k]).abs() < 1e-6))
        .expect("a vertex there");
    VertexRef { part: part.id, vertex: v.name, point: p }
}

/// The frustum between squares of sides `a` and `b`, `h` apart: h(a² + ab + b²)/3.
fn square_frustum(a: f64, b: f64, h: f64) -> f64 {
    h * (a * a + a * b + b * b) / 3.0
}

fn tan5() -> f64 {
    5f64.to_radians().tan()
}

#[test]
fn draft_feature_on_a_cube() {
    // PS4.9: a 100³ cube on Top, its four sides drafted 5° with the Top plane as the neutral
    // plane (pull +Z): the sides lean in as they rise, the top square's side 100 − 2·100·tan 5°;
    // the cube becomes the frustum h(a² + ab + b²)/3.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 100.0, 100.0);
    let cube = d.parts()[0].clone();
    let sides: Vec<FaceRef> = [([1.0, 0.0, 0.0], [100.0, 50.0, 50.0]), ([-1.0, 0.0, 0.0], [0.0, 50.0, 50.0]), ([0.0, 1.0, 0.0], [50.0, 100.0, 50.0]), ([0.0, -1.0, 0.0], [50.0, 0.0, 50.0])]
        .iter()
        .map(|(n, p)| face_at(&cube, *n, *p))
        .collect();
    let x = DraftFeature { neutral: Some(MirrorPlane::Plane(PlaneRef::Top)), faces: sides.clone(), angle: 5.0, angle_expr: "5 deg".into(), ..DraftFeature::default() };
    let f = d.add(AddFeature::draft(d.el, FeatureId::new(), x.clone()));
    let t = tan5();
    close(d.volume(), square_frustum(100.0, 100.0 - 200.0 * t, 100.0), 1e-4);
    assert_eq!(d.features().last().unwrap().name, "Draft 1");
    // P3.11: the drafted faces keep their names (the dialog tints them as the faces to draft).
    let drafted = d.parts()[0].clone();
    for s in &sides {
        assert!(drafted.solid.face(&s.face).is_some(), "{:?} lost its name", s.face);
    }
    // The cube's bottom face as the neutral plane: its outward normal is −Z, so the pull runs
    // down and the sides lean in going down, i.e. out going up: 100 + 2·100·tan 5° at the top.
    let bottom = MirrorPlane::Face(face_at(&cube, [0.0, 0.0, -1.0], [50.0, 50.0, 0.0]));
    d.set(f, FeatureKind::Draft(DraftFeature { neutral: Some(bottom), ..x.clone() }));
    close(d.volume(), square_frustum(100.0, 100.0 + 200.0 * t, 100.0), 1e-4);
    // Flipped (Opposite direction): as with the Top plane.
    d.set(f, FeatureKind::Draft(DraftFeature { neutral: Some(bottom), flip: true, ..x.clone() }));
    close(d.volume(), square_frustum(100.0, 100.0 - 200.0 * t, 100.0), 1e-4);
    // PS11.1: one face to draft gone (its seed moved off the part, its name unknown): the other
    // three are drafted, with a warning. Three sides in, one straight: the top is
    // (100 − 2·100·t) × (100 − 100·t) and the solid is ∫ (100 − 2tz)(100 − tz) dz over 0..100 =
    // 10⁶ − 1.5·10⁶·t + (2/3)·10⁶·t².
    let mut gone = sides.clone();
    gone[0].face = cadrs_sketch::FaceName { op: uuid::Uuid::nil(), ..gone[0].face };
    gone[0].seed = [500.0, 500.0, 500.0];
    d.set(f, FeatureKind::Draft(DraftFeature { faces: gone, ..x }));
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert!(b.warning(f).is_some_and(|w| w.contains("no longer exists")), "{:?}", b.warnings);
    close(d.volume(), 1e6 - 1.5e6 * t + 2.0 / 3.0 * 1e6 * t * t, 1e-4);
}

#[test]
fn extrude_with_draft() {
    // X3 / PS4.9: a 100 × 100 square extruded 100 with Draft 5°: the same frustum as the Draft
    // feature's, h(a² + ab + b²)/3 with b = 100 − 2·100·tan 5°. Flipped, the sides lean out.
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 100.0, 100.0)]);
    let draft = ExtrudeDraft { angle: 5.0, expr: "5 deg".into(), flip: false };
    let e = d.extrude(s, &[Vec2::new(50.0, 50.0)], 100.0, |e| e.draft = Some(draft.clone()));
    let t = tan5();
    close(d.volume(), square_frustum(100.0, 100.0 - 200.0 * t, 100.0), 1e-4);
    let set = |d: &mut Doc, f: &dyn Fn(&mut ExtrudeFeature)| {
        let mut x = d.d.element(d.el).unwrap().feature(e).unwrap().extrude().unwrap().clone();
        f(&mut x);
        d.h.execute(&mut d.d, &SetExtrude { element: d.el, feature: e, extrude: x, label: "Edit".into() }).unwrap();
    };
    set(&mut d, &|x| x.draft.as_mut().unwrap().flip = true);
    close(d.volume(), square_frustum(100.0, 100.0 + 200.0 * t, 100.0), 1e-4);
    // Symmetric 100: 50 each way, both ends narrowing away from the sketch plane: two frustums
    // 100 → 100 − 2·50·tan 5°, 50 high.
    set(&mut d, &|x| {
        x.draft.as_mut().unwrap().flip = false;
        x.symmetric = true;
    });
    close(d.volume(), 2.0 * square_frustum(100.0, 100.0 - 100.0 * t, 50.0), 1e-4);
    assert_eq!(d.parts().len(), 1);
    // A Ø40 circle extruded 30 with Draft 5°: the cone frustum πh(R² + Rr + r²)/3, r = 20 − 30·tan 5°.
    let mut d = Doc::new();
    let c = d.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 20.0, construction: false }]);
    d.extrude(c, &[Vec2::new(0.0, 0.0)], 30.0, |e| e.draft = Some(draft.clone()));
    let r = 20.0 - 30.0 * t;
    close(d.volume(), PI * 30.0 * (400.0 + 20.0 * r + r * r) / 3.0, 1e-3);
}

/// Two blocks: A (0..60)² × 0..30 and B (100..160) × (0..60) × 0..20, and a sketch point on A's
/// top face at (30, 30, 30).
fn hole_blocks() -> (Doc, HolePoint, Part, Part) {
    let mut d = Doc::new();
    d.block(0.0, 0.0, 60.0, 60.0, 30.0);
    d.block(100.0, 0.0, 160.0, 60.0, 20.0);
    let a = d.part_at([30.0, 30.0, 30.0]);
    let b = d.part_at([130.0, 30.0, 20.0]);
    let top = face_at(&a, [0.0, 0.0, 1.0], [30.0, 30.0, 30.0]);
    let plane = cadrs_core::parts::face_plane(&d.features(), a.feature, top.face).expect("a sketch plane on A's top");
    // (30, 30, 30) in the face plane's coordinates.
    let f = plane.frame();
    let w = [30.0 - f.origin[0], 30.0 - f.origin[1], 30.0 - f.origin[2]];
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let s = d.sketch(plane, vec![SketchOp::AddPoint { pos: Vec2::new(dot(w, f.u), dot(w, f.v)) }]);
    let g = d.geometry(s);
    let p = HolePoint { sketch: s, point: g.points.keys().next().unwrap() };
    (d, p, a, b)
}

#[test]
fn hole_start_from_selected_plane_and_up_to_entity() {
    let (mut d, p, a, b) = hole_blocks();
    let v0 = 108_000.0 + 72_000.0;
    let tip = |r: f64| r / 59f64.to_radians().tan();
    // PS15.6: a Ø10 blind 5 hole from A's top, started from the selected plane B's top face
    // (z = 20, inside A): it starts buried at z = 20 and cuts nothing above it:
    // π·25·5 + π·25·h/3 with the 118° point h = 5 / tan 59°.
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(10.0);
    spec.end = HoleEnd::Blind;
    spec.depth = Length::mm(5.0);
    spec.start = HoleStart::SelectedPlane;
    let b_top = MirrorPlane::Face(face_at(&b, [0.0, 0.0, 1.0], [130.0, 30.0, 20.0]));
    let x = HoleFeature { points: vec![p], spec: spec.clone(), start_plane: Some(b_top), merge_scope: vec![a.id], ..HoleFeature::default() };
    let h = d.add(AddFeature::hole(d.el, FeatureId::new(), x.clone()));
    close(d.volume(), v0 - PI * 25.0 * 5.0 - PI * 25.0 * tip(5.0) / 3.0, 1e-4);
    // The buried hole leaves A's top face whole: its area is still 60².
    let a_now = d.part_at([5.0, 5.0, 30.0]);
    let top_area = a_now.solid.faces.iter().filter(|f| f.plane.is_some_and(|p| p.normal()[2] > 0.999 && (p.origin[2] - 30.0).abs() < 1e-9)).filter_map(|f| f.area).sum::<f64>();
    close(top_area, 3600.0, 1e-6);
    // Start from sketch plane instead: from A's top (z = 30), the same removed volume, but it
    // opens the top face: 60² − π·25.
    let mut from_sketch = x.clone();
    from_sketch.spec.start = HoleStart::SketchPlane;
    d.set(h, FeatureKind::Hole(from_sketch));
    close(d.volume(), v0 - PI * 25.0 * 5.0 - PI * 25.0 * tip(5.0) / 3.0, 1e-4);
    let a_now = d.part_at([5.0, 5.0, 30.0]);
    let top_area = a_now.solid.faces.iter().filter(|f| f.plane.is_some_and(|p| p.normal()[2] > 0.999 && (p.origin[2] - 30.0).abs() < 1e-9)).filter_map(|f| f.area).sum::<f64>();
    close(top_area, 3600.0 - PI * 25.0, 1e-6);
    // PS15.7: Up to entity, B's top face (z = 20): the full diameter from z = 30 down to it (10)
    // and the drill point beyond: π·25·10 + π·25·h/3.
    let mut up = x.clone();
    up.spec.start = HoleStart::Part;
    up.spec.end = HoleEnd::UpToEntity;
    up.start_plane = None;
    up.up_to = Some(b_top);
    d.set(h, FeatureKind::Hole(up.clone()));
    close(d.volume(), v0 - PI * 25.0 * 10.0 - PI * 25.0 * tip(5.0) / 3.0, 1e-4);
    // With a 2 mm offset (short of the target): 8 deep.
    let mut short = up.clone();
    short.spec.end_offset = Some(Length::mm(2.0));
    d.set(h, FeatureKind::Hole(short));
    close(d.volume(), v0 - PI * 25.0 * 8.0 - PI * 25.0 * tip(5.0) / 3.0, 1e-4);
    // Up to the Top plane (z = 0): through A's bottom, the point outside it: π·25·30.
    let mut to_top = up;
    to_top.up_to = Some(MirrorPlane::Plane(PlaneRef::Top));
    d.set(h, FeatureKind::Hole(to_top));
    close(d.volume(), v0 - PI * 25.0 * 30.0, 1e-4);
    // Missing its target: an error naming it.
    let mut none = x;
    none.spec.end = HoleEnd::UpToEntity;
    none.up_to = None;
    d.set(h, FeatureKind::Hole(none));
    assert!(d.build().error(h).is_some_and(|e| e.contains("go up to")));
}

#[test]
fn warnings_for_what_didnt_take() {
    // PS11.1 (Onshape's yellow): a hole sketch with a point over A and one over nothing (x = 80,
    // between the blocks): the hole at A is made, the other point gives a warning, not an error.
    let (mut d, _, _, _) = hole_blocks();
    let v = Vec2::new;
    let s = d.sketch(PlaneRef::Top, vec![SketchOp::AddPoint { pos: v(30.0, 30.0) }, SketchOp::AddPoint { pos: v(80.0, 30.0) }]);
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(10.0);
    spec.end = HoleEnd::ThroughAll;
    let h = d.add(AddFeature::hole(d.el, FeatureId::new(), HoleFeature { sketches: vec![s], flip: true, spec, ..HoleFeature::default() }));
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.warning(h), Some("1 hole doesn't reach a part in the merge scope"));
    close(d.volume(), 180_000.0 - PI * 25.0 * 30.0, 1e-4);
    // An extrude of two regions whose sketch lost one (its rectangle deleted): the other is
    // extruded, with a warning.
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 30.0, 10.0)]);
    let e = d.extrude(s, &[v(5.0, 5.0), v(25.0, 5.0)], 10.0, |_| {});
    close(d.volume(), 2000.0, 1e-6);
    let g = d.geometry(s);
    let second: Vec<cadrs_sketch::CurveId> = g
        .curves
        .iter()
        .filter(|(_, c)| match c.kind {
            cadrs_sketch::CurveKind::Line { a, b } => g.pos(a).x >= 19.0 && g.pos(b).x >= 19.0,
            _ => false,
        })
        .map(|(id, _)| id)
        .collect();
    d.h.execute(&mut d.d, &EditSketch { element: d.el, feature: s, op: SketchOp::Delete { curves: second, points: vec![], dimensions: vec![], constraints: vec![] } })
        .unwrap();
    let b = d.build();
    assert!(b.errors.is_empty(), "{:?}", b.errors);
    assert_eq!(b.warning(e), Some("1 selected sketch region no longer exists"));
    close(d.volume(), 1000.0, 1e-6);
}

#[test]
fn variable_and_asymmetric_fillets() {
    // PS14.6 on the 10 × 20 × 30 block's vertical edge at (10, 20) (faces at 90°): a section of
    // radius r removes (1 − π/4)r² per mm.
    let c = 1.0 - PI / 4.0;
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let edge = edge_at(&part, [10.0, 20.0, 15.0]);
    let (bottom, top) = (vertex_at(&part, [10.0, 20.0, 0.0]), vertex_at(&part, [10.0, 20.0, 30.0]));
    let vr = |v: VertexRef, r: f64| VertexRadius { vertex: v, radius: r, expr: format!("{r} mm") };
    // Variable, R3 at both vertices: the constant R3, (1 − π/4)·9·30.
    let x = FilletFeature {
        entities: vec![EdgeOrFace::Edge(edge)],
        variable: true,
        vertices: vec![vr(bottom, 3.0), vr(top, 3.0)],
        size: 1.0,
        size_expr: "1 mm".into(),
        ..FilletFeature::default()
    };
    let f = d.add(AddFeature::fillet(d.el, FeatureId::new(), x.clone()));
    close(d.volume(), 6000.0 - c * 9.0 * 30.0, 1e-6);
    // R2 at the bottom to R4 at the top without Smooth transition (linear): between the
    // constant R2's and R4's removal, and within 1 % of (1 − π/4)∫₀³⁰(2 + z/15)² dz =
    // (1 − π/4)·280 (the kernel's evolving section isn't exactly the circle of the local
    // radius; see the conformance case `fillet_variable_radius`).
    let linear = FilletFeature { vertices: vec![vr(bottom, 2.0), vr(top, 4.0)], smooth_transition: false, ..x.clone() };
    d.set(f, FeatureKind::Fillet(linear));
    let removed = 6000.0 - d.volume();
    assert!(removed > c * 4.0 * 30.0 && removed < c * 16.0 * 30.0, "{removed}");
    close(removed, c * 280.0, 0.01 * c * 280.0);
    // A point on the edge at its middle with R3, the vertices unset (R1, the fillet's size):
    // more removed than the constant R1, less than the constant R3.
    let pointed = FilletFeature {
        vertices: vec![],
        edge_points: vec![EdgePoint { edge, location: 0.5, radius: 3.0, expr: "3 mm".into() }],
        ..x.clone()
    };
    d.set(f, FeatureKind::Fillet(pointed));
    let removed = 6000.0 - d.volume();
    assert!(removed > c * 30.0 && removed < c * 9.0 * 30.0, "{removed}");
    // Asymmetric 2 and 4: the quarter ellipse with those semi-axes, (1 − π/4)·2·4 per mm.
    let asym = FilletFeature { variable: false, asymmetric: true, size: 2.0, size_expr: "2 mm".into(), second: 4.0, second_expr: "4 mm".into(), ..x };
    d.set(f, FeatureKind::Fillet(asym));
    close(d.volume(), 6000.0 - c * 8.0 * 30.0, 1e-3);
}

#[test]
fn partial_fillet() {
    // P3.11, PS14.6 Partial fillet on the 10 × 20 × 30 block's vertical edge at (10, 20) (faces
    // at 90°), R3: only the part of the edge between the bounds is filleted, the quarter-round
    // section (1 − π/4)·r² per mm along it, with a flat end face square to the edge at each bound
    // (so nothing more is removed at the ends).
    let c = 1.0 - PI / 4.0;
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let edge = edge_at(&part, [10.0, 20.0, 15.0]);
    let x = FilletFeature {
        entities: vec![EdgeOrFace::Edge(edge)],
        size: 3.0,
        size_expr: "3 mm".into(),
        partial: true,
        partial_first: 0.2,
        partial_first_expr: "0.2".into(),
        partial_second: 0.7,
        partial_second_expr: "0.7".into(),
        ..FilletFeature::default()
    };
    // Parameter 0.2 to 0.7: 15 mm of the 30 mm edge.
    let f = d.add(AddFeature::fillet(d.el, FeatureId::new(), x.clone()));
    close(d.volume(), 6000.0 - c * 9.0 * 15.0, 1e-6);
    // The fillet face, and the end faces: (1 − π/4)·9 each; the block's other faces continue.
    let faces = d.parts()[0].solid.faces.len();
    assert_eq!(faces, 6 + 3, "{faces}");
    // Length 3 mm to 24 mm: 21 mm.
    let length = FilletFeature {
        partial_bound: PartialBound::Length,
        partial_first: 3.0,
        partial_first_expr: "3 mm".into(),
        partial_second: 24.0,
        partial_second_expr: "24 mm".into(),
        ..x.clone()
    };
    d.set(f, FeatureKind::Fillet(length.clone()));
    close(d.volume(), 6000.0 - c * 9.0 * 21.0, 1e-6);
    // Flipped, the bounds are measured from the other end: 3 to 24 mm from the end is 6 to 27
    // mm from the start, the same amount removed, its centroid moved 3 mm along the edge.
    let z = |d: &Doc| d.parts()[0].mass.unwrap().center_of_mass[2];
    let z0 = z(&d);
    d.set(f, FeatureKind::Fillet(FilletFeature { flip_partial: true, ..length.clone() }));
    close(d.volume(), 6000.0 - c * 9.0 * 21.0, 1e-6);
    // The removed material's centroid moves by 3 mm; the block's by 3·removed/volume.
    let removed = c * 9.0 * 21.0;
    close((z(&d) - z0).abs(), 3.0 * removed / (6000.0 - removed), 1e-6);
    // Off the edge, or two edges: an error.
    d.set(f, FeatureKind::Fillet(FilletFeature { partial_second: 31.0, ..length }));
    assert!(!d.build().errors.is_empty());
    let two = FilletFeature { entities: vec![EdgeOrFace::Edge(edge), EdgeOrFace::Edge(edge_at(&part, [0.0, 0.0, 15.0]))], ..x };
    assert!(two.problem().is_some());
}

#[test]
fn smooth_fillet_corners() {
    // Final (PS14.6): R3 on the three edges meeting at the 10 × 20 × 30 block's corner
    // (10, 20, 30). With Smooth fillet corners the corner is set back 4.5 mm and blended: a
    // little more is removed than by the default (sphere) corner, but less than the ball of
    // radius √(4.5² + 3²) about the vertex could hold (an octant of it, πρ³/6); the fillets and
    // the patch are faces of the part (the block's 6 + 3 fillets + the corner).
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    let part = d.parts()[0].clone();
    let edges = vec![
        EdgeOrFace::Edge(edge_at(&part, [10.0, 20.0, 15.0])),
        EdgeOrFace::Edge(edge_at(&part, [5.0, 20.0, 30.0])),
        EdgeOrFace::Edge(edge_at(&part, [10.0, 10.0, 30.0])),
    ];
    let x = FilletFeature { entities: edges, size: 3.0, size_expr: "3 mm".into(), ..FilletFeature::default() };
    let f = d.add(AddFeature::fillet(d.el, FeatureId::new(), x.clone()));
    let default = d.volume();
    let default_faces = d.parts()[0].solid.faces.len();
    d.set(f, FeatureKind::Fillet(FilletFeature { smooth_corners: true, ..x.clone() }));
    assert!(d.build().errors.is_empty(), "{:?}", d.build().errors);
    let smooth = d.volume();
    let rho = (4.5f64 * 4.5 + 9.0).sqrt();
    assert!(smooth < default, "{smooth} vs {default}");
    assert!(default - smooth < PI * rho.powi(3) / 6.0, "{smooth} vs {default}");
    assert_eq!(d.parts()[0].solid.faces.len(), 6 + 3 + 1);
    assert_eq!(default_faces, 6 + 3 + 1);
    // Only for a circular fillet by its radius.
    d.set(f, FeatureKind::Fillet(FilletFeature { smooth_corners: true, measurement: cadrs_core::applied::FilletMeasurement::Width, ..x }));
    assert!(!d.build().errors.is_empty());
}

#[test]
fn one_fillet_on_two_parts() {
    // PS18.3: one Fillet with an edge of each of two blocks (10 × 20 × 30 each), R3: each loses
    // (1 − π/4)·9·30, and both stay separate parts.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 20.0, 30.0);
    d.block(50.0, 0.0, 60.0, 20.0, 30.0);
    let a = d.part_at([5.0, 10.0, 30.0]);
    let b = d.part_at([55.0, 10.0, 30.0]);
    let x = FilletFeature {
        entities: vec![EdgeOrFace::Edge(edge_at(&a, [10.0, 20.0, 15.0])), EdgeOrFace::Edge(edge_at(&b, [60.0, 20.0, 15.0]))],
        size: 3.0,
        size_expr: "3 mm".into(),
        ..FilletFeature::default()
    };
    d.add(AddFeature::fillet(d.el, FeatureId::new(), x));
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    for p in &parts {
        close(p.mass.unwrap().volume, 6000.0 - (1.0 - PI / 4.0) * 9.0 * 30.0, 1e-6);
    }
}

#[test]
fn boolean_offset_and_tool_order() {
    // PS5.5: A (0..60)² × 0..30, and a tool B (20..40)² × 10..40 through its top. Subtract with
    // a 1 mm offset (sharp): the tool grows to (19..41)² × 9..41, so A loses 22·22·(30 − 9):
    // 108 000 − 10 164 = 97 836.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 60.0, 60.0, 30.0);
    let s = d.sketch(PlaneRef::Top, vec![rect(20.0, 20.0, 40.0, 40.0)]);
    d.extrude(s, &[Vec2::new(30.0, 30.0)], 30.0, |e| {
        e.start_offset = Some(cadrs_core::Offset { value: 10.0, expr: "10 mm".into(), flip: false });
    });
    let a = d.part_at([5.0, 5.0, 30.0]);
    let b = d.part_at([30.0, 30.0, 40.0]);
    let sub = BooleanFeature { op: BooleanKind::Subtract, tools: vec![b.id], targets: vec![a.id], keep_tools: false, offset: Some(BooleanOffset::default()) };
    let f = d.add(AddFeature::boolean(d.el, FeatureId::new(), sub.clone()));
    close(d.volume(), 108_000.0 - 22.0 * 22.0 * 21.0, 1e-4);
    assert_eq!(d.parts().len(), 1);
    // Without the offset: 108 000 − 20·20·20.
    d.set(f, FeatureKind::Boolean(BooleanFeature { offset: None, ..sub.clone() }));
    close(d.volume(), 108_000.0 - 8000.0, 1e-6);
    // Only the tool's bottom face offset (1 mm down): (20..40)² × 9..: 108 000 − 20·20·21.
    let bottom = face_at(&b, [0.0, 0.0, -1.0], [30.0, 30.0, 10.0]);
    let faces = BooleanOffset { all: false, faces: vec![bottom], ..BooleanOffset::default() };
    d.set(f, FeatureKind::Boolean(BooleanFeature { offset: Some(faces), ..sub.clone() }));
    close(d.volume(), 108_000.0 - 400.0 * 21.0, 1e-4);
    // Flipped: the tool shrinks to (21..39)² × 11..39: 108 000 − 18·18·19.
    let inward = BooleanOffset { flip: true, ..BooleanOffset::default() };
    d.set(f, FeatureKind::Boolean(BooleanFeature { offset: Some(inward), ..sub }));
    close(d.volume(), 108_000.0 - 18.0 * 18.0 * 19.0, 1e-4);
    // Reordered tools: a Union's result is the first tool (PS5.5).
    let union = BooleanFeature { op: BooleanKind::Union, tools: vec![a.id, b.id], ..BooleanFeature::default() };
    d.set(f, FeatureKind::Boolean(union.clone()));
    assert_eq!(d.parts().iter().map(|p| p.id).collect::<Vec<_>>(), vec![a.id]);
    d.set(f, FeatureKind::Boolean(BooleanFeature { tools: vec![b.id, a.id], ..union }));
    assert_eq!(d.parts().iter().map(|p| p.id).collect::<Vec<_>>(), vec![b.id]);
}

#[test]
fn union_of_parts_that_dont_touch() {
    // As Onshape: a Union of parts apart from each other joins nothing; they stay the parts they
    // were (ids included), with a warning. With a third part touching the first, those two join.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    d.block(20.0, 0.0, 30.0, 10.0, 10.0);
    let a = d.part_at([5.0, 5.0, 10.0]);
    let b = d.part_at([25.0, 5.0, 10.0]);
    let union = BooleanFeature { op: BooleanKind::Union, tools: vec![a.id, b.id], ..BooleanFeature::default() };
    let f = d.add(AddFeature::boolean(d.el, FeatureId::new(), union.clone()));
    let mut ids: Vec<_> = d.parts().iter().map(|p| p.id).collect();
    ids.sort();
    let mut want = vec![a.id, b.id];
    want.sort();
    assert_eq!(ids, want);
    assert!(d.build().warnings.iter().any(|(w, _)| *w == f));
    close(d.volume(), 2000.0, 1e-6);
    // A block touching the first: it joins it, the far one stays.
    d.block(10.0, 0.0, 15.0, 10.0, 10.0);
    let c = d.part_at([12.0, 5.0, 10.0]);
    d.add(AddFeature::boolean(d.el, FeatureId::new(), BooleanFeature { tools: vec![a.id, b.id, c.id], ..union }));
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    assert!(parts.iter().any(|p| p.id == a.id) && parts.iter().any(|p| p.id == b.id));
    close(d.volume(), 2500.0, 1e-6);
}

#[test]
fn loft_match_tangent_from_a_face() {
    // PS20.4: a cylinder r 10 × 20 (an extrude), then a loft from its top face to a circle r 5 on
    // a plane 20 above it, Match tangent at the start. The cylinder's side continues straight up
    // at the start, so the loft (magnitude 1) starts vertical; it joins the cylinder (Add) into
    // one part. Its volume lies between the cone frustum 10 → 5 (π·20·(100 + 50 + 25)/3) and the
    // cylinder r 10 (π·100·20), since the side bulges out beyond the straight cone.
    let mut d = Doc::new();
    let c = d.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 10.0, construction: false }]);
    d.extrude(c, &[Vec2::new(0.0, 0.0)], 20.0, |_| {});
    let cyl = d.parts()[0].clone();
    let top = face_at(&cyl, [0.0, 0.0, 1.0], [0.0, 0.0, 20.0]);
    let plane = d.add(AddFeature {
        element: d.el,
        feature: FeatureId::new(),
        base_name: "Plane".into(),
        kind: FeatureKind::Plane(PlaneFeature {
            kind: PlaneType::Offset,
            entities: vec![PlaneEntity::Plane(PlaneRef::Top)],
            offset: 40.0,
            offset_expr: "40 mm".into(),
            ..PlaneFeature::default()
        }),
    });
    let plane_ref = cadrs_core::parts::plane_feature_ref(&d.features(), plane).expect("the plane builds");
    let s = d.sketch(plane_ref, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 5.0, construction: false }]);
    let g = d.geometry(s);
    let loft = LoftFeature {
        profiles: vec![LoftProfile::Face(top), LoftProfile::Regions { sketch: s, regions: samples::region_refs(s, &g, &[Vec2::new(0.0, 0.0)]) }],
        op: BooleanOp::Add,
        start: LoftCondition::MatchTangent,
        ..LoftFeature::default()
    };
    let l = d.add(AddFeature { element: d.el, feature: FeatureId::new(), base_name: "Loft".into(), kind: FeatureKind::Loft(loft.clone()) });
    let parts = d.parts();
    assert_eq!(parts.len(), 1);
    let v = parts[0].mass.unwrap().volume - PI * 100.0 * 20.0;
    assert!(v > PI * 20.0 * 175.0 / 3.0 && v < PI * 100.0 * 20.0, "{v}");
    // Match curvature builds too (the cylinder's axial curvature is zero) and differs.
    d.set(l, FeatureKind::Loft(LoftFeature { start: LoftCondition::MatchCurvature, ..loft }));
    let w = d.parts()[0].mass.unwrap().volume - PI * 100.0 * 20.0;
    assert!(w > PI * 20.0 * 175.0 / 3.0 && w < PI * 100.0 * 20.0, "{w}");
    assert!((w - v).abs() > 1e-3);
}

#[test]
fn loft_from_a_curved_face() {
    // PS20.1: a revolve-free check through the feature list: a Ø20 × 20 cylinder's curved side
    // can't be a sketch plane, but it is a loft profile... A half cylinder is lofted in the
    // conformance case `loft_from_a_curved_face`; here the feature list only has to accept the
    // curved face as a profile and give the closed form: a cylinder r 10 × 20 along Y from a
    // half disc on Front, its curved face lofted to a 20 × 20 square at z = 30:
    // 20·20·30 − (π·100/2)·20.
    let mut d = Doc::new();
    let v = Vec2::new;
    let half = d.sketch(
        PlaneRef::Front,
        vec![
            SketchOp::AddArc { center: v(0.0, 0.0), start: v(10.0, 0.0), end: v(-10.0, 0.0), construction: false },
            SketchOp::AddPolyline { points: vec![v(-10.0, 0.0), v(10.0, 0.0)], closed: false, construction: false, label: "Add line" },
        ],
    );
    d.extrude(half, &[v(0.0, 5.0)], 20.0, |_| {});
    let body = d.parts()[0].clone();
    let curved = curved_face(&body);
    let plane = d.add(AddFeature {
        element: d.el,
        feature: FeatureId::new(),
        base_name: "Plane".into(),
        kind: FeatureKind::Plane(PlaneFeature { kind: PlaneType::Offset, entities: vec![PlaneEntity::Plane(PlaneRef::Top)], offset: 30.0, offset_expr: "30 mm".into(), ..PlaneFeature::default() }),
    });
    let plane_ref = cadrs_core::parts::plane_feature_ref(&d.features(), plane).expect("the plane builds");
    // The body lies at y 0..20 (Front's normal is −Y, so the extrude runs along +Y... or −Y):
    // the square covers it either way below.
    let ys: Vec<f64> = body.solid.positions.iter().map(|p| p[1]).collect();
    let (y0, y1) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
    close(y1 - y0, 20.0, 1e-6);
    // Top's plane coordinates are world x and y.
    let s = d.sketch(plane_ref, vec![rect(-10.0, y0, 10.0, y1)]);
    let g = d.geometry(s);
    let loft = LoftFeature {
        profiles: vec![LoftProfile::Face(curved), LoftProfile::Regions { sketch: s, regions: samples::region_refs(s, &g, &[v(0.0, (y0 + y1) / 2.0)]) }],
        op: BooleanOp::New,
        ..LoftFeature::default()
    };
    d.add(AddFeature { element: d.el, feature: FeatureId::new(), base_name: "Loft".into(), kind: FeatureKind::Loft(loft) });
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    let loft_v = parts.iter().map(|p| p.mass.unwrap().volume).fold(0.0, f64::max);
    close(loft_v, 12_000.0 - 1000.0 * PI, 1e-3);
}

#[test]
fn sweep_takes_a_whole_sketch_like_its_regions() {
    // PS1.6: a Ø10 circle on Top swept along a 50 mm vertical line (a sketch on Front): the
    // cylinder π·25·50, whether the sketch is picked whole or its region is.
    let mut d = Doc::new();
    let v = Vec2::new;
    let c = d.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: v(0.0, 0.0), radius: 5.0, construction: false }]);
    let path = d.sketch(PlaneRef::Front, vec![SketchOp::AddPolyline { points: vec![v(0.0, 0.0), v(0.0, 50.0)], closed: false, construction: false, label: "Add line" }]);
    let whole = SweepFeature { sketches: vec![c], regions: vec![], path: vec![PathRef::Sketch(path)], ..SweepFeature::default() };
    let f = d.add(AddFeature { element: d.el, feature: FeatureId::new(), base_name: "Sweep".into(), kind: FeatureKind::Sweep(whole.clone()) });
    close(d.volume(), PI * 25.0 * 50.0, 1e-4);
    let g = d.geometry(c);
    let regions = samples::region_refs(c, &g, &[v(0.0, 0.0)]);
    d.set(f, FeatureKind::Sweep(SweepFeature { sketches: vec![], regions, ..whole }));
    close(d.volume(), PI * 25.0 * 50.0, 1e-4);
}

#[test]
fn mass_properties_override_and_reference() {
    // X7: a 100 mm cube (no material). Override mass 10 kg: the uniform cube's inertia about its
    // centre is m(a² + a²)/12 = 10·20 000/12 = 16 666.667 kg·mm² on each axis.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 100.0, 100.0, 100.0);
    let parts = d.parts();
    let refs: Vec<&Part> = parts.iter().collect();
    let r = mass_report_with(&refs, &[], MassOptions { override_mass: Some(10.0), reference: None }).unwrap();
    let m = r.mass.unwrap();
    close(m.mass, 10.0, 1e-12);
    close(m.inertia[(0, 0)], 10.0 * 20_000.0 / 12.0, 1e-6);
    close(m.center_of_mass.x, 50.0, 1e-9);
    // A reference frame at (100, 0, 0) turned 90° about Z (u = +Y, v = −X): the centre
    // (50, 50, 50) is d = (−50, 50, 50) from it, i.e. (d·u, d·v, d·n) = (50, 50, 50); the cube's
    // inertia is the same on every axis, so it doesn't change.
    let frame = cadrs_sketch::PlaneFrame { origin: [100.0, 0.0, 0.0], u: [0.0, 1.0, 0.0], v: [-1.0, 0.0, 0.0] };
    let r = mass_report_with(&refs, &[], MassOptions { override_mass: Some(10.0), reference: Some(frame) }).unwrap();
    let m = r.mass.unwrap();
    close(m.center_of_mass.x, 50.0, 1e-9);
    close(m.center_of_mass.y, 50.0, 1e-9);
    close(m.center_of_mass.z, 50.0, 1e-9);
    close(m.inertia[(1, 1)], 10.0 * 20_000.0 / 12.0, 1e-6);
    // Without an override nor a material there is no mass (Onshape leaves it blank).
    assert!(mass_report_with(&refs, &[], MassOptions::default()).unwrap().mass.is_none());
}

#[test]
fn face_areas_are_exact() {
    // X7's Face tab: a Ø20 × 30 cylinder's side has the exact area 2π·10·30 (from the kernel).
    let mut d = Doc::new();
    let c = d.sketch(PlaneRef::Top, vec![SketchOp::AddCircle { center: Vec2::new(0.0, 0.0), radius: 10.0, construction: false }]);
    d.extrude(c, &[Vec2::new(0.0, 0.0)], 30.0, |_| {});
    let part = d.parts()[0].clone();
    let side = part.solid.faces.iter().find(|f| f.plane.is_none()).unwrap();
    close(side.area.unwrap(), 2.0 * PI * 10.0 * 30.0, 1e-6);
}

#[test]
fn revolve_about_a_mate_connector() {
    // PS7.2: a 10 × 30 rectangle 10 mm off the origin on Front, revolved a full turn about the
    // origin's implicit mate connector (its Z axis): the tube r 10..20, 30 high,
    // π(20² − 10²)·30 = 9000π.
    use cadrs_core::commands::{AddRevolve, SetRevolve};
    use cadrs_core::document::{AxisRef, RevolveFeature, RevolveType};
    use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Front, vec![rect(10.0, 0.0, 20.0, 30.0)]);
    let g = d.geometry(s);
    let r = RevolveFeature {
        regions: samples::region_refs(s, &g, &[Vec2::new(15.0, 15.0)]),
        axis: Some(AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin))),
        kind: RevolveType::Full,
        ..RevolveFeature::default()
    };
    let f = FeatureId::new();
    d.h.execute(&mut d.d, &AddRevolve { element: d.el, feature: f, revolve: RevolveFeature::default() }).unwrap();
    d.h.execute(&mut d.d, &SetRevolve { element: d.el, feature: f, revolve: r, label: "Revolve".into() }).unwrap();
    close(d.volume(), 9000.0 * PI, 1e-6);
}

#[test]
fn loft_normal_and_tangent_direction() {
    // P3.11 (PS20.4): a loft's Normal direction / Tangent direction take a picked vector. Two
    // 10 × 10 squares, on Top and on a plane 20 above it; Normal direction at the start along
    // the Front plane's normal (−Y, square to the loft): every point of the bottom square leaves
    // sideways with the same derivative, so each horizontal slice is the square moved along Y
    // and the volume stays 10·10·20 = 2000 (Cavalieri), while the body leans out past y = −5.
    // Without a direction the feature asks for one.
    let mut d = Doc::new();
    let bottom = d.sketch(PlaneRef::Top, vec![rect(-5.0, -5.0, 5.0, 5.0)]);
    let plane = d.add(AddFeature {
        element: d.el,
        feature: FeatureId::new(),
        base_name: "Plane".into(),
        kind: FeatureKind::Plane(PlaneFeature {
            kind: PlaneType::Offset,
            entities: vec![PlaneEntity::Plane(PlaneRef::Top)],
            offset: 20.0,
            offset_expr: "20 mm".into(),
            ..PlaneFeature::default()
        }),
    });
    let plane_ref = cadrs_core::parts::plane_feature_ref(&d.features(), plane).expect("the plane builds");
    let top = d.sketch(plane_ref, vec![rect(-5.0, -5.0, 5.0, 5.0)]);
    let (gb, gt) = (d.geometry(bottom), d.geometry(top));
    let loft = LoftFeature {
        profiles: vec![
            LoftProfile::Regions { sketch: bottom, regions: samples::region_refs(bottom, &gb, &[Vec2::new(0.0, 0.0)]) },
            LoftProfile::Regions { sketch: top, regions: samples::region_refs(top, &gt, &[Vec2::new(0.0, 0.0)]) },
        ],
        start: LoftCondition::NormalDirection,
        ..LoftFeature::default()
    };
    let l = d.add(AddFeature { element: d.el, feature: FeatureId::new(), base_name: "Loft".into(), kind: FeatureKind::Loft(loft.clone()) });
    assert!(d.build().error(l).is_some_and(|e| e.contains("direction")), "{:?}", d.build().errors);
    let front = cadrs_core::document::DirectionRef::PlaneNormal(PlaneRef::Front);
    d.set(l, FeatureKind::Loft(LoftFeature { start_direction: Some(front), ..loft.clone() }));
    assert!(d.build().errors.is_empty(), "{:?}", d.build().errors);
    close(d.volume(), 2000.0, 1e-3);
    let b = d.parts()[0].solid.bounds().unwrap();
    assert!(b.0[1] < -5.5 && b.1[1] < 5.0 + 1e-6, "{b:?}");
    // Along Top's normal (the profiles' own) Normal direction is Normal to profile: the prism;
    // and Tangent direction is Tangent to profile (the same flared body).
    let up = cadrs_core::document::DirectionRef::PlaneNormal(PlaneRef::Top);
    let normal = LoftFeature { start_direction: Some(up), end: LoftCondition::NormalDirection, end_direction: Some(up), ..loft.clone() };
    d.set(l, FeatureKind::Loft(normal.clone()));
    close(d.volume(), 2000.0, 1e-3);
    d.set(l, FeatureKind::Loft(LoftFeature { end: LoftCondition::TangentDirection, ..normal.clone() }));
    let flared = d.volume();
    d.set(l, FeatureKind::Loft(LoftFeature { end: LoftCondition::TangentToProfile, end_direction: None, ..normal }));
    close(d.volume(), flared, 1e-6);
    assert!(flared > 2000.0 + 1.0, "{flared}");
    // A direction from a part's face makes its feature a parent (Show dependencies): the
    // bottom square's own sketch plane is Top, so its sketches are the parents here.
    let features = d.features();
    let f = features.iter().find(|f| f.id == l).unwrap();
    assert!(f.parents().contains(&bottom) && f.parents().contains(&top));
}

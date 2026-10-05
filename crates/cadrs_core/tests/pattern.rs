//! P3.8: patterns, mirror and mate connectors through the feature list. Every expected value is
//! derived by hand in the test's comment.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::advanced::PathRef;
use cadrs_core::applied::{HoleFeature, HolePoint};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature, SetPartAppearance, SetPartMaterial};
use cadrs_core::document::{AxisRef, BooleanOp, DirectionRef, Document, ExtrudeFeature, FaceRef, FeatureKind, Offset};
use cadrs_core::hole::{HoleEnd, HoleSpec, HoleStart, Length};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef, MateConnectorFeature};
use cadrs_core::pattern::{MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::{Appearance, ElementId, Feature, FeatureId, History, Part, PartId, samples};
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
        let d = Document::new("P3.8");
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

    fn extrude(&mut self, s: FeatureId, seeds: &[Vec2], set: impl FnOnce(&mut ExtrudeFeature)) -> FeatureId {
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, seeds);
        assert_eq!(regions.len(), seeds.len(), "a region is missing");
        let mut e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, 10.0) };
        set(&mut e);
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    /// A box x0..x1 × y0..y1 on Top, `h` high (New).
    fn block(&mut self, x0: f64, y0: f64, x1: f64, y1: f64, h: f64) -> FeatureId {
        let s = self.sketch(PlaneRef::Top, vec![rect(x0, y0, x1, y1)]);
        self.extrude(s, &[Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0)], |e| {
            e.depth = h;
            e.depth_expr = format!("{h} mm");
        })
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind })
            .unwrap();
        feature
    }

    fn set(&mut self, feature: FeatureId, kind: FeatureKind) {
        self.h.execute(&mut self.d, &SetFeature { element: self.el, feature, kind, label: "Edit".into() }).unwrap();
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

/// Every face of the parts made by feature `op`, as references.
fn faces_of(parts: &[Part], op: FeatureId) -> Vec<FaceRef> {
    let mut out = Vec::new();
    for p in parts {
        for (i, f) in p.solid.faces.iter().enumerate() {
            if f.name.op == op.0 {
                out.push(FaceRef { part: p.id, face: f.name, seed: p.solid.face_point(i).unwrap() });
            }
        }
    }
    out
}

/// The top face (z = `z`) of a part, as a reference.
fn top_face(part: &Part, z: f64) -> FaceRef {
    let s = &part.solid;
    let i = (0..s.faces.len())
        .find(|&i| s.faces[i].plane.is_some_and(|pl| pl.normal()[2] > 0.999 && (pl.origin[2] - z).abs() < 1e-6))
        .expect("a top face");
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.face_point(i).unwrap() }
}

/// A blind Ø`d` hole spec, `depth` deep, from the part, with the 118° point.
fn blind(d: f64, depth: f64) -> HoleSpec {
    let mut spec = HoleSpec::default();
    spec.diameter = Length::mm(d);
    spec.depth = Length::mm(depth);
    spec.end = HoleEnd::Blind;
    spec.start = HoleStart::Part;
    spec
}

/// A blind hole's volume: the cylinder and the 118° cone (height r / tan 59°).
fn blind_volume(d: f64, depth: f64) -> f64 {
    let r = d / 2.0;
    PI * r * r * depth + PI * r * r * (r / 59f64.to_radians().tan()) / 3.0
}

#[test]
fn linear_part_pattern_of_a_cube() {
    // A 10 mm cube ×5 at 20 mm along X (the Right plane's normal), New: five parts of 1000 mm³,
    // V × 5 = 5000, the last one's centre at x = 5 + 4·20 = 85. PS9.6: the copies keep the
    // cube's palette colour, and show the appearance and material given to it later.
    let mut d = Doc::new();
    d.block(0.0, 0.0, 10.0, 10.0, 10.0);
    let seed = d.parts()[0].id;
    let mut p = PatternFeature::new(PatternKind::Linear);
    p.parts = vec![seed];
    p.first.direction = Some(DirectionRef::PlaneNormal(PlaneRef::Right));
    p.first.distance = 20.0;
    p.first.count = 5;
    let pat = d.add("Linear pattern", FeatureKind::Pattern(p.clone()));
    let parts = d.parts();
    assert_eq!(parts.len(), 5);
    close(d.volume(), 5000.0, 1e-6);
    let far = parts.iter().map(|p| p.mass.unwrap().center_of_mass.x).fold(f64::MIN, f64::max);
    close(far, 85.0, 1e-9);
    for c in parts.iter().filter(|p| p.id != seed) {
        assert_eq!(c.source, Some(seed));
        assert_eq!(c.palette, parts[0].palette);
        assert_eq!(c.feature, pat);
    }
    let red = Appearance::rgb(200, 30, 30);
    d.h.execute(&mut d.d, &SetPartAppearance { element: d.el, parts: vec![seed], appearance: Some(red) }).unwrap();
    d.h.execute(
        &mut d.d,
        &SetPartMaterial { element: d.el, parts: vec![seed], material: cadrs_core::material::library("Steel") },
    )
    .unwrap();
    let props = d.d.element(d.el).unwrap().part_props().to_vec();
    for c in &d.parts() {
        assert_eq!(cadrs_core::appearance::part_appearance(c, &props), red);
        assert!(cadrs_core::parts::part_material(c, &props).is_some());
    }
    // Skip instances (2, 0) and (3, 0): three cubes; Centered with 5: the seed in the middle,
    // the copies at x −40..40.
    p.skip_on = true;
    p.skipped = vec![[2, 0], [3, 0]];
    d.set(pat, FeatureKind::Pattern(p.clone()));
    close(d.volume(), 3000.0, 1e-6);
    let dots = d.build().dots.get(&pat).cloned().unwrap();
    assert_eq!(dots.len(), 4);
    assert_eq!(dots.iter().filter(|x| x.skipped).count(), 2);
    p.skip_on = false;
    p.first.centered = true;
    d.set(pat, FeatureKind::Pattern(p.clone()));
    let xs: Vec<f64> = d.parts().iter().map(|p| p.mass.unwrap().center_of_mass.x).collect();
    close(xs.iter().copied().fold(f64::MAX, f64::min), 5.0 - 40.0, 1e-9);
    close(xs.iter().copied().fold(f64::MIN, f64::max), 5.0 + 40.0, 1e-9);
    // A second direction along Y (Front's normal is −Y: flipped), 2 × 30: a 5 × 2 grid.
    p.first.centered = false;
    p.second_on = true;
    p.second.direction = Some(DirectionRef::PlaneNormal(PlaneRef::Front));
    p.second.distance = 30.0;
    p.second.count = 2;
    d.set(pat, FeatureKind::Pattern(p));
    assert_eq!(d.parts().len(), 10);
    close(d.volume(), 10_000.0, 1e-6);
}

/// A 100 × 100 × 20 plate centred on the origin (z 0..20) with a sketch point at (30, 0) on
/// Top, and a blind Ø10 × 12 hole drilled up from it (Start from part: the plate's bottom).
fn plate_with_hole() -> (Doc, FeatureId, f64) {
    let mut d = Doc::new();
    d.block(-50.0, -50.0, 50.0, 50.0, 20.0);
    let s = d.sketch(PlaneRef::Top, vec![SketchOp::AddPoint { pos: Vec2::new(30.0, 0.0) }]);
    let g = d.d.element(d.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
    let point = HolePoint { sketch: s, point: g.points.keys().next().unwrap() };
    let hole = d.add(
        "Hole",
        FeatureKind::Hole(HoleFeature { points: vec![point], spec: blind(10.0, 12.0), flip: true, ..HoleFeature::default() }),
    );
    (d, hole, blind_volume(10.0, 12.0))
}

#[test]
fn circular_feature_pattern_of_a_hole() {
    // The hole ×6 round the Z axis (the origin's implicit mate connector), 360° with equal
    // spacing (60° apart): 6 × the hole's volume is gone, with and without Reapply.
    let (mut d, hole, v) = plate_with_hole();
    let plate = 200_000.0;
    close(d.volume(), plate - v, 1e-6);
    let mut p = PatternFeature::new(PatternKind::Circular);
    p.pattern_type = PatternType::Feature;
    p.features = vec![hole];
    p.axis = Some(AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin)));
    p.first.count = 6;
    let pat = d.add("Circular pattern", FeatureKind::Pattern(p.clone()));
    close(d.volume(), plate - 6.0 * v, 1e-6);
    // The plate stays symmetric: its centre of mass is back on the axis.
    let c = d.parts()[0].mass.unwrap().center_of_mass;
    close(c.x, 0.0, 1e-9);
    close(c.y, 0.0, 1e-9);
    // The copies' faces are named under the pattern (instance 1..5).
    let names = d.parts()[0].solid.faces.iter().filter(|f| f.name.op == pat.0).count();
    assert_eq!(names, 10, "two faces (wall and point) per copy");
    p.reapply = true;
    d.set(pat, FeatureKind::Pattern(p.clone()));
    close(d.volume(), plate - 6.0 * v, 1e-6);
    let names = d.parts()[0].solid.faces.iter().filter(|f| f.name.op == pat.0).count();
    assert_eq!(names, 10);
    // Equal spacing off: 60° is the step (the same six).
    p.equal_spacing = false;
    p.angle = 60.0;
    d.set(pat, FeatureKind::Pattern(p));
    close(d.volume(), plate - 6.0 * v, 1e-6);
}

#[test]
fn face_pattern_with_skipped_instances() {
    // The hole's two faces (its wall and point) ×4 at 12 mm along −X (Right's normal, flipped),
    // (2, 0) skipped: two copies, so 3 holes in all.
    let (mut d, hole, v) = plate_with_hole();
    let faces = faces_of(&d.parts(), hole);
    assert_eq!(faces.len(), 2);
    let mut p = PatternFeature::new(PatternKind::Linear);
    p.pattern_type = PatternType::Face;
    p.faces = faces;
    p.first.direction = Some(DirectionRef::PlaneNormal(PlaneRef::Right));
    p.first.flip = true;
    p.first.distance = 12.0;
    p.first.count = 4;
    p.skip_on = true;
    p.skipped = vec![[2, 0]];
    let pat = d.add("Linear pattern", FeatureKind::Pattern(p));
    close(d.volume(), 200_000.0 - 3.0 * v, 1e-6);
    let b = d.build();
    let dots = &b.dots[&pat];
    // The dots sit on the copies of the hole's centre (x 30 − 12k).
    let xs: Vec<f64> = dots.iter().map(|x| x.at[0]).collect();
    assert_eq!(dots.len(), 3);
    for (x, k) in xs.iter().zip([1.0, 2.0, 3.0]) {
        close(*x, 30.0 - 12.0 * k, 1e-6);
    }
}

#[test]
fn mirror_of_a_half_control_arm() {
    // PS26.2: the Control Arm's Extrude 1 and 2 (both New), cut to the half at y ≥ 0 (a Remove
    // of the rectangle y −200..0, 100 up), then Part mirror of both halves across Front (the XZ
    // plane) with Add: one part of the full volume, 368 749.705 mm³ (the course's value).
    let mut d = Doc::new();
    let s = d.sketch(PlaneRef::Top, vec![samples::control_arm_geometry()]);
    d.extrude(s, &samples::EXTRUDE_1_SEEDS, |e| e.depth = samples::EXTRUDE_1_DEPTH);
    d.extrude(s, &samples::EXTRUDE_2_SEEDS, |e| e.depth = samples::EXTRUDE_2_DEPTH);
    close(d.volume(), 368_749.705, 1e-3);
    let cut = d.sketch(PlaneRef::Top, vec![rect(-200.0, -200.0, 200.0, 0.0)]);
    d.extrude(cut, &[Vec2::new(0.0, -100.0)], |e| {
        e.op = BooleanOp::Remove;
        e.merge_all = true;
        e.depth = 100.0;
    });
    let half = d.volume();
    close(half, 368_749.705 / 2.0, 1e-3);
    let parts: Vec<PartId> = d.parts().iter().map(|p| p.id).collect();
    let m = MirrorFeature {
        mirror_type: PatternType::Part,
        parts,
        plane: Some(MirrorPlane::Plane(PlaneRef::Front)),
        op: BooleanOp::Add,
        ..MirrorFeature::default()
    };
    d.add("Mirror", FeatureKind::Mirror(m));
    let parts = d.parts();
    assert_eq!(parts.len(), 1, "the halves and their mirror images are one part");
    close(parts[0].mass.unwrap().volume, 368_749.705, 1e-3);
    close(parts[0].mass.unwrap().center_of_mass.y, 0.0, 1e-9);
}

#[test]
fn face_and_feature_mirror_of_a_pocket() {
    // A 20 × 10 × 5 pocket (x 10..30, y −5..5, from z 15 up through the top of the 100 × 100 ×
    // 20 plate) mirrored across Right (x = 0): Face mirror of its five faces, then (instead)
    // Feature mirror of its extrude: 2 × 1000 gone either way, and the plate's CoM x back to 0.
    let mut d = Doc::new();
    d.block(-50.0, -50.0, 50.0, 50.0, 20.0);
    let s = d.sketch(PlaneRef::Top, vec![rect(10.0, -5.0, 30.0, 5.0)]);
    let pocket = d.extrude(s, &[Vec2::new(20.0, 0.0)], |e| {
        e.op = BooleanOp::Remove;
        e.depth = 10.0;
        e.start_offset = Some(Offset { value: 15.0, expr: "15 mm".into(), flip: false });
    });
    close(d.volume(), 200_000.0 - 1000.0, 1e-6);
    let faces = faces_of(&d.parts(), pocket);
    assert_eq!(faces.len(), 5);
    let mut m = MirrorFeature {
        mirror_type: PatternType::Face,
        faces,
        plane: Some(MirrorPlane::Plane(PlaneRef::Right)),
        ..MirrorFeature::default()
    };
    let mirror = d.add("Mirror", FeatureKind::Mirror(m.clone()));
    close(d.volume(), 200_000.0 - 2000.0, 1e-6);
    close(d.parts()[0].mass.unwrap().center_of_mass.x, 0.0, 1e-9);
    m.mirror_type = PatternType::Feature;
    m.features = vec![pocket];
    d.set(mirror, FeatureKind::Mirror(m.clone()));
    close(d.volume(), 200_000.0 - 2000.0, 1e-6);
    close(d.parts()[0].mass.unwrap().center_of_mass.x, 0.0, 1e-9);
    // Reapply features (P3I.8, E4 step 7): the extrude regenerated from its mirrored sketch,
    // still cutting down from z = 15 (the mirrored frame keeps Top's normal): the same pocket.
    m.reapply = true;
    d.set(mirror, FeatureKind::Mirror(m));
    close(d.volume(), 200_000.0 - 2000.0, 1e-6);
    close(d.parts()[0].mass.unwrap().center_of_mass.x, 0.0, 1e-9);
}

#[test]
fn a_reapplied_mirror_stops_at_its_own_up_to_next() {
    // A plate (z 0..10) under two ceilings: z 30..35 over the right (x 10..40), z 40..45 over
    // the left (x −40..−10). A 10 × 10 post from the plate's top (start offset 10) Up to next
    // on the right reaches z = 30 (20 tall). Mirrored about Right: copying its material gives a
    // 20-tall post on the left too; Reapply features regenerates it there, Up to next: up to the
    // left ceiling, 30 tall (P3I.8, PS27.5).
    let mut d = Doc::new();
    d.block(-50.0, -20.0, 50.0, 20.0, 10.0);
    let ceiling = |d: &mut Doc, x0: f64, x1: f64, z: f64| {
        let s = d.sketch(PlaneRef::Top, vec![rect(x0, -20.0, x1, 20.0)]);
        d.extrude(s, &[Vec2::new((x0 + x1) / 2.0, 0.0)], |e| {
            e.depth = 5.0;
            e.depth_expr = "5 mm".into();
            e.start_offset = Some(Offset { value: z, expr: format!("{z} mm"), flip: false });
        });
    };
    ceiling(&mut d, 10.0, 40.0, 30.0);
    ceiling(&mut d, -40.0, -10.0, 40.0);
    let s = d.sketch(PlaneRef::Top, vec![rect(20.0, -5.0, 30.0, 5.0)]);
    let post = d.extrude(s, &[Vec2::new(25.0, 0.0)], |e| {
        e.end = cadrs_core::document::EndType::UpToNext;
        e.start_offset = Some(Offset { value: 10.0, expr: "10 mm".into(), flip: false });
    });
    let before = d.volume();
    let mut m = MirrorFeature { mirror_type: PatternType::Feature, features: vec![post], plane: Some(MirrorPlane::Plane(PlaneRef::Right)), ..MirrorFeature::default() };
    let mirror = d.add("Mirror", FeatureKind::Mirror(m.clone()));
    let copied = d.volume() - before;
    close(copied, 100.0 * 20.0, 1e-3);
    m.reapply = true;
    d.set(mirror, FeatureKind::Mirror(m));
    let reapplied = d.volume() - before;
    close(reapplied, 100.0 * 30.0, 1e-3);
}

#[test]
fn curve_pattern_along_an_arc() {
    // A 10 mm cube centred on (50, 0) ×3 along the quarter circle R50 from (50, 0) to (0, 50),
    // equal spacing (45° apart), tangent to the curve: the copies' centres at 45° and 90° on
    // the circle, 3000 mm³ in all.
    let mut d = Doc::new();
    d.block(45.0, -5.0, 55.0, 5.0, 10.0);
    let seed = d.parts()[0].id;
    let path = d.sketch(
        PlaneRef::Top,
        vec![SketchOp::AddArc { center: Vec2::new(0.0, 0.0), start: Vec2::new(50.0, 0.0), end: Vec2::new(0.0, 50.0), construction: false }],
    );
    let curve = d.d.element(d.el).unwrap().feature(path).unwrap().sketch().unwrap().geometry.curves.keys().next().unwrap();
    let mut p = PatternFeature::new(PatternKind::Curve);
    p.parts = vec![seed];
    p.path = vec![PathRef::SketchCurve { sketch: path, curve }];
    p.first.count = 3;
    d.add("Curve pattern", FeatureKind::Pattern(p));
    let parts = d.parts();
    assert_eq!(parts.len(), 3);
    close(d.volume(), 3000.0, 1e-6);
    let r = 50.0 / 2f64.sqrt();
    assert!(parts.iter().any(|q| {
        let c = q.mass.unwrap().center_of_mass;
        (c.x - r).abs() < 1e-6 && (c.y - r).abs() < 1e-6
    }));
    assert!(parts.iter().any(|q| {
        let c = q.mass.unwrap().center_of_mass;
        c.x.abs() < 1e-6 && (c.y - 50.0).abs() < 1e-6
    }));
}

#[test]
fn hole_at_a_mate_connector() {
    // PS15.2 / PS27.6: a Mate connector at the centre of a 100 × 100 × 40 plate's top face
    // (x, y = 0, z 40, Z up) and a hole there, Ø46 × 12 with the 118° point (13.8 mm more),
    // drilled along its −Z.
    let mut d = Doc::new();
    d.block(-50.0, -50.0, 50.0, 50.0, 40.0);
    let top = top_face(&d.parts()[0], 40.0);
    let mc = d.add(
        "Mate connector",
        FeatureKind::MateConnector(MateConnectorFeature { origin: Some(ConnectorOrigin::Face(top)), ..MateConnectorFeature::default() }),
    );
    let b = d.build();
    let f = b.connectors[&mc];
    close(f.origin[0], 0.0, 1e-12);
    close(f.origin[1], 0.0, 1e-12);
    close(f.origin[2], 40.0, 1e-12);
    close(f.normal()[2], 1.0, 1e-12);
    d.add(
        "Hole",
        FeatureKind::Hole(HoleFeature { connectors: vec![ConnectorRef::Feature(mc)], spec: blind(46.0, 12.0), ..HoleFeature::default() }),
    );
    close(d.volume(), 400_000.0 - blind_volume(46.0, 12.0), 1e-6);
    let c = d.parts()[0].mass.unwrap().center_of_mass;
    close(c.x, 0.0, 1e-9);
    close(c.y, 0.0, 1e-9);
}

#[test]
fn split_types_and_options() {
    // P3.8 (the Split dialog as Onshape's): a block x −5..15 × y 0..20 × z 0..10 (4000 mm³) split
    // by the Right plane (x = 0) gives 1000 + 3000. Keep both sides off keeps the front (x > 0):
    // 3000 as the same part; flipped, 1000. The Face type splits only the top face: one part of
    // 4000 with 7 faces. A second block's top face (z = 5, x 20..30) splits the first along its
    // whole plane (2000 + 2000), but trimmed to its edges it doesn't reach it (an error).
    use cadrs_core::advanced::{SplitFeature, SplitToolRef, SplitType};
    let mut d = Doc::new();
    d.block(-5.0, 0.0, 15.0, 20.0, 10.0);
    let part = d.parts()[0].clone();
    let mut x = SplitFeature { parts: vec![part.id], tool: Some(SplitToolRef::Plane(PlaneRef::Right)), ..SplitFeature::default() };
    let f = d.add("Split", FeatureKind::Split(x.clone()));
    let mut vols: Vec<f64> = d.parts().iter().map(|p| p.mass.unwrap().volume).collect();
    vols.sort_by(f64::total_cmp);
    assert_eq!(vols.len(), 2);
    close(vols[0], 1000.0, 1e-6);
    close(vols[1], 3000.0, 1e-6);
    x.keep_both = false;
    d.set(f, FeatureKind::Split(x.clone()));
    let parts = d.parts();
    assert_eq!(parts.len(), 1);
    assert_eq!(parts[0].id, part.id);
    close(parts[0].mass.unwrap().volume, 3000.0, 1e-6);
    x.flip = true;
    d.set(f, FeatureKind::Split(x.clone()));
    close(d.volume(), 1000.0, 1e-6);
    assert_eq!(d.parts().len(), 1);
    let x = SplitFeature {
        split_type: SplitType::Face,
        faces: vec![top_face(&part, 10.0)],
        tool: Some(SplitToolRef::Plane(PlaneRef::Right)),
        ..SplitFeature::default()
    };
    d.set(f, FeatureKind::Split(x));
    let parts = d.parts();
    assert_eq!(parts.len(), 1);
    close(parts[0].mass.unwrap().volume, 4000.0, 1e-6);
    assert_eq!(parts[0].solid.faces.len(), 7);
    // A face of another block as the tool.
    let mut e = Doc::new();
    e.block(-5.0, 0.0, 15.0, 20.0, 10.0);
    e.block(20.0, 0.0, 30.0, 20.0, 5.0);
    let parts = e.parts();
    let (a, b) = if parts[0].mass.unwrap().volume > 3000.0 { (&parts[0], &parts[1]) } else { (&parts[1], &parts[0]) };
    let mut x = SplitFeature { parts: vec![a.id], tool: Some(SplitToolRef::Face(top_face(b, 5.0))), ..SplitFeature::default() };
    let g = e.add("Split", FeatureKind::Split(x.clone()));
    assert_eq!(e.parts().len(), 3);
    close(e.volume(), 4000.0 + 1000.0, 1e-6);
    x.trim = true;
    e.set(g, FeatureKind::Split(x));
    assert!(!e.build().errors.is_empty());
}

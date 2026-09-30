//! The Transform feature (Onshape's Transform) through the feature list: each type moves the
//! parts' volume and centroid as derived by hand in its test, the moved parts keep their ids and
//! face names (a fillet or a sketch on a moved face still finds it), and copies are new parts.
#![cfg(feature = "occt")]

use cadrs_core::applied::{EdgeOrFace, FilletFeature};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude, SetFeature};
use cadrs_core::document::{AxisRef, BooleanOp, Document, ExtrudeFeature, FaceRef, FeatureKind};
use cadrs_core::mate::{ConnectorOrigin, ConnectorRef};
use cadrs_core::rebuild::{self, Build};
use cadrs_core::transform::{SecondaryAxis, TransformFeature, TransformType};
use cadrs_core::{ElementId, Feature, FeatureId, History, Part, samples};
use cadrs_sketch::{FaceOrigin, PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

#[track_caller]
fn close3(a: [f64; 3], b: [f64; 3], tol: f64) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() <= tol, "got {a:?}, expected {b:?} ± {tol}");
    }
}

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("Transform");
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

    fn sketch(&mut self, plane: PlaneRef, ops: Vec<SketchOp>) -> FeatureId {
        let f = FeatureId::new();
        self.h.execute(&mut self.d, &AddSketch { element: self.el, feature: f, plane: Some(plane) }).unwrap();
        for op in ops {
            self.h.execute(&mut self.d, &EditSketch { element: self.el, feature: f, op }).unwrap();
        }
        f
    }

    fn extrude(&mut self, s: FeatureId, seed: Vec2, h: f64) -> FeatureId {
        let g = self.d.element(self.el).unwrap().feature(s).unwrap().sketch().unwrap().geometry.clone();
        let regions = samples::region_refs(s, &g, &[seed]);
        assert_eq!(regions.len(), 1, "a region is missing");
        let mut e = ExtrudeFeature { op: BooleanOp::New, ..samples::extrude_of(regions, h) };
        e.depth = h;
        e.depth_expr = format!("{h} mm");
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
        self.extrude(s, Vec2::new((x0 + x1) / 2.0, (y0 + y1) / 2.0), h)
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

    fn transform(&mut self, x: TransformFeature) -> FeatureId {
        let f = self.add("Transform", FeatureKind::Transform(x));
        assert_eq!(self.features().iter().find(|g| g.id == f).unwrap().name, "Transform 1");
        f
    }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> SketchOp {
    let v = Vec2::new;
    SketchOp::AddPolyline { points: vec![v(x0, y0), v(x1, y0), v(x1, y1), v(x0, y1)], closed: true, construction: false, label: "Add rectangle" }
}

/// The face of a part whose plane has the unit normal `n`, as a reference.
fn face_along(part: &Part, n: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let i = (0..s.faces.len())
        .find(|&i| s.faces[i].plane.is_some_and(|p| (0..3).all(|k| (p.normal()[k] - n[k]).abs() < 1e-9)))
        .expect("a face with that normal");
    FaceRef { part: part.id, face: s.faces[i].name, seed: s.face_point(i).unwrap() }
}

fn center(p: &Part) -> [f64; 3] {
    let c = p.mass.unwrap().center_of_mass;
    [c.x, c.y, c.z]
}

/// The 20 × 10 × 5 block at the origin: 1000 mm³, centroid (10, 5, 2.5).
fn one_block() -> (Doc, FeatureId, Part) {
    let mut d = Doc::new();
    let ex = d.block(0.0, 0.0, 20.0, 10.0, 5.0);
    let parts = d.parts();
    assert_eq!(parts.len(), 1);
    close(parts[0].mass.unwrap().volume, 1000.0, 1e-6);
    close3(center(&parts[0]), [10.0, 5.0, 2.5], 1e-9);
    (d, ex, parts[0].clone())
}

/// A fillet of 1 mm round the edges of `face` builds and takes material off `part`.
#[track_caller]
fn fillet_top(d: &mut Doc, face: FaceRef, before: f64) {
    let f = FilletFeature { entities: vec![EdgeOrFace::Face(face)], size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() };
    d.add("Fillet", FeatureKind::Fillet(f));
    let parts = d.parts();
    let v = parts.iter().find(|p| p.id == face.part).expect("the part keeps its id").mass.unwrap().volume;
    // Four edges of a rectangle rounded: (1 − π/4)·r² per mm of edge, less at the corners.
    assert!(v < before - 1.0 && v > before - 20.0, "fillet volume {v}");
}

#[test]
fn translate_xyz_keeps_ids_and_names() {
    let (mut d, ex, part) = one_block();
    let top = face_along(&part, [0.0, 0.0, 1.0]);
    let side = face_along(&part, [1.0, 0.0, 0.0]);
    // (30, −5, 12): the centroid (10, 5, 2.5) → (40, 0, 14.5).
    let t = d.transform(TransformFeature::translate_xyz(vec![part.id], [30.0, -5.0, 12.0]));
    let parts = d.parts();
    assert_eq!(parts.len(), 1);
    let p = &parts[0];
    assert_eq!((p.id, p.name.as_str()), (part.id, "Part 1"), "a moved part keeps its id and name");
    assert!(p.features.contains(&t));
    close(p.mass.unwrap().volume, 1000.0, 1e-6);
    close3(center(p), [40.0, 0.0, 14.5], 1e-9);
    // Every face keeps its name; the top and side are where the move put them, with their
    // extrude's frames (moved with them).
    let names = |s: &Part| {
        let mut v: Vec<_> = s.solid.faces.iter().map(|f| f.name).collect();
        v.sort_by_key(|n| format!("{n:?}"));
        v
    };
    assert_eq!(names(p), names(&part));
    let e = |s: &Part| {
        let mut v: Vec<_> = s.solid.edges.iter().map(|e| e.name).collect();
        v.sort_by_key(|n| format!("{n:?}"));
        v
    };
    assert_eq!(e(p), e(&part), "every edge keeps its name");
    let top_now = p.solid.face(&top.face).unwrap().plane.unwrap();
    close(top_now.origin[2], 17.0, 1e-9);
    let old_top = part.solid.face(&top.face).unwrap().plane.unwrap();
    close3(top_now.u, old_top.u, 1e-12);
    let side_now = p.solid.face(&side.face).unwrap().plane.unwrap();
    close(side_now.distance([50.0, 0.0, 14.0]), 0.0, 1e-9);
    // A sketch on the moved top face sits on it (its old frame refreshed), and an extrude of a
    // 10 × 6 rectangle on it, 3 mm up, is a new part: 180 mm³ at z 17..20.
    let features = d.features();
    let plane = cadrs_core::parts::face_plane(&features, ex, top.face).unwrap();
    let s = d.sketch(plane, vec![]);
    let frame = d.features().iter().find(|f| f.id == s).unwrap().sketch().unwrap().plane.unwrap().frame();
    close(frame.origin[2], 17.0, 1e-9);
    let w = |x: f64, y: f64| {
        let q = frame.to_sketch([x, y, 17.0]);
        Vec2::new(q.x, q.y)
    };
    let pts = vec![w(35.0, -3.0), w(45.0, -3.0), w(45.0, 3.0), w(35.0, 3.0)];
    d.h.execute(&mut d.d, &EditSketch {
        element: d.el,
        feature: s,
        op: SketchOp::AddPolyline { points: pts, closed: true, construction: false, label: "Add rectangle" },
    })
    .unwrap();
    let i = d.features().iter().position(|f| f.id == s).unwrap();
    assert!(!cadrs_core::parts::sketch_face_lost_in(&d.features(), i, &d.parts()));
    d.extrude(s, w(40.0, 0.0), 3.0);
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    close(parts[1].mass.unwrap().volume, 180.0, 1e-6);
    close3(center(&parts[1]), [40.0, 0.0, 18.5], 1e-9);
    // A fillet round the moved top face (referred to by its name, from before the move).
    fillet_top(&mut d, top, 1000.0);
    // A zero move puts it back.
    d.set(t, FeatureKind::Transform(TransformFeature::translate_xyz(vec![part.id], [0.0, 0.0, 0.0])));
    let b = d.build();
    let p = b.parts.iter().find(|q| q.id == part.id).unwrap();
    close(p.mass.unwrap().center_of_mass.z, 2.5, 0.2);
}

#[test]
fn rotate_about_an_axis() {
    let (mut d, _, part) = one_block();
    let top = face_along(&part, [0.0, 0.0, 1.0]);
    let side = face_along(&part, [1.0, 0.0, 0.0]);
    // 90° about Z through the origin (the origin's mate connector): (x, y) → (−y, x), so the
    // centroid (10, 5, 2.5) → (−5, 10, 2.5); the x = 20 side faces +Y at y = 20.
    let z = AxisRef::Connector(ConnectorRef::Implicit(ConnectorOrigin::Origin));
    d.transform(TransformFeature::rotate(vec![part.id], z, 90.0));
    let parts = d.parts();
    let p = &parts[0];
    assert_eq!(p.id, part.id);
    close(p.mass.unwrap().volume, 1000.0, 1e-6);
    close3(center(p), [-5.0, 10.0, 2.5], 1e-9);
    let s = p.solid.face(&side.face).unwrap().plane.unwrap();
    close3(s.normal(), [0.0, 1.0, 0.0], 1e-9);
    close(s.distance([0.0, 20.0, 0.0]), 0.0, 1e-9);
    let t = p.solid.face(&top.face).unwrap().plane.unwrap();
    close(t.origin[2], 5.0, 1e-9);
    fillet_top(&mut d, top, 1000.0);
}

#[test]
fn rotate_flipped_about_an_edge() {
    let (mut d, _, part) = one_block();
    // The bottom edge along X at y = 0 (from the front and bottom faces), −90° with the flip
    // (so +90° the other way): about +X by −90°, (y, z) → (z, −y).
    let front = face_along(&part, [0.0, -1.0, 0.0]).face;
    let bottom = face_along(&part, [0.0, 0.0, -1.0]).face;
    let edge = part.solid.edges.iter().find(|e| e.name.touches(&front) && e.name.touches(&bottom)).unwrap();
    let r = cadrs_core::EdgeRef { part: part.id, edge: edge.name, seed: edge.midpoint() };
    let mut x = TransformFeature::rotate(vec![part.id], AxisRef::Edge(r), 90.0);
    x.flip = true;
    d.transform(x);
    let p = &d.parts()[0];
    let c = center(p);
    // The edge runs either way along X: the centroid lands at y = ±2.5, z = ∓5.
    close(c[0], 10.0, 1e-9);
    close(c[1].abs(), 2.5, 1e-9);
    close(c[2].abs(), 5.0, 1e-9);
    close(p.mass.unwrap().volume, 1000.0, 1e-6);
}

#[test]
fn mate_connector_transform() {
    let (mut d, _, part) = one_block();
    let origin = ConnectorRef::Implicit(ConnectorOrigin::Origin);
    // Flip primary axis: the destination turned half a turn about X, (x, y, z) → (x, −y, −z):
    // the centroid → (10, −5, −2.5).
    let mut x = TransformFeature::new(TransformType::MateConnectors);
    x.parts = vec![part.id];
    x.from = Some(origin);
    x.to = Some(origin);
    x.flip_primary = true;
    let t = d.transform(x.clone());
    let p = &d.parts()[0];
    close3(center(p), [10.0, -5.0, -2.5], 1e-9);
    close(p.mass.unwrap().volume, 1000.0, 1e-6);
    // Then the secondary axis to +Y (a quarter turn about the flipped Z): X → (0, −1, 0),
    // Y → (−1, 0, 0), Z → (0, 0, −1), so (x, y, z) → (−y, −x, −z): the centroid → (−5, −10, −2.5).
    x.secondary = SecondaryAxis::PlusY;
    d.set(t, FeatureKind::Transform(x.clone()));
    close3(center(&d.parts()[0]), [-5.0, -10.0, -2.5], 1e-9);
    // From the block's top face to the top face of a 40 × 20 × 10 block at x 100..140: the two
    // faces' connectors (their centres, (10, 5, 5) and (120, 10, 10), both facing up, their X
    // along the model's X) meet: the block moves by (110, 5, 5), its centroid to (120, 10, 7.5).
    let mut e = Doc::new();
    e.block(0.0, 0.0, 20.0, 10.0, 5.0);
    e.block(100.0, 0.0, 140.0, 20.0, 10.0);
    let parts = e.parts();
    let a = face_along(&parts[0], [0.0, 0.0, 1.0]);
    let b = face_along(&parts[1], [0.0, 0.0, 1.0]);
    let mut x = TransformFeature::new(TransformType::MateConnectors);
    x.parts = vec![parts[0].id];
    x.from = Some(ConnectorRef::Implicit(ConnectorOrigin::Face(a)));
    x.to = Some(ConnectorRef::Implicit(ConnectorOrigin::Face(b)));
    e.transform(x);
    let parts = e.parts();
    close3(center(&parts[0]), [120.0, 10.0, 7.5], 1e-9);
    // The moved face still resolves: a fillet round it.
    fillet_top(&mut e, a, 1000.0);
}

#[test]
fn scale_uniformly() {
    let (mut d, _, part) = one_block();
    let top = face_along(&part, [0.0, 0.0, 1.0]);
    // Twice about the origin: 8 × 1000 mm³, the centroid (20, 10, 5).
    let mut x = TransformFeature::new(TransformType::ScaleUniformly);
    x.parts = vec![part.id];
    x.scale = 2.0;
    x.scale_expr = "2".into();
    let t = d.transform(x.clone());
    let p = &d.parts()[0];
    assert_eq!(p.id, part.id);
    close(p.mass.unwrap().volume, 8000.0, 1e-6);
    close3(center(p), [20.0, 10.0, 5.0], 1e-9);
    close(p.solid.face(&top.face).unwrap().plane.unwrap().origin[2], 10.0, 1e-9);
    // Half about the block's far top corner (20, 10, 5): (x, y, z) → (20, 10, 5) + ((x, y, z)
    // − (20, 10, 5))/2, so the centroid → (15, 7.5, 3.75) and 125 mm³.
    let corner = part.solid.vertices.iter().find(|v| (v.point[0] - 20.0).abs() < 1e-9 && (v.point[1] - 10.0).abs() < 1e-9 && (v.point[2] - 5.0).abs() < 1e-9).unwrap();
    x.scale = 0.5;
    x.scale_expr = "0.5".into();
    x.scale_point = Some(ConnectorRef::Implicit(ConnectorOrigin::Vertex(cadrs_core::VertexRef { part: part.id, vertex: corner.name, point: corner.point })));
    d.set(t, FeatureKind::Transform(x));
    let p = &d.parts()[0];
    close(p.mass.unwrap().volume, 125.0, 1e-6);
    close3(center(p), [15.0, 7.5, 3.75], 1e-9);
    fillet_top(&mut d, top, 125.0);
}

#[test]
fn copy_part_and_copy_in_place() {
    let (mut d, _, part) = one_block();
    let top = face_along(&part, [0.0, 0.0, 1.0]);
    // Copy part, 20 up: the original stays, the copy's centroid is (10, 5, 22.5).
    let mut x = TransformFeature::translate_xyz(vec![part.id], [0.0, 0.0, 20.0]);
    x.copy = true;
    let t = d.transform(x);
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    let (orig, copy) = (&parts[0], &parts[1]);
    assert_eq!(orig.id, part.id);
    close3(center(orig), [10.0, 5.0, 2.5], 1e-9);
    close(copy.mass.unwrap().volume, 1000.0, 1e-6);
    close3(center(copy), [10.0, 5.0, 22.5], 1e-9);
    assert_eq!(copy.id.feature, t, "the copy is the Transform's part");
    assert_eq!(copy.source, Some(part.id), "the copy looks like its original");
    assert_eq!(copy.palette, orig.palette);
    assert_eq!(copy.name, "Part 2");
    // Its faces are instance 1 of the original's, under the Transform.
    for f in &copy.solid.faces {
        assert_eq!(f.name.op, t.0);
        assert!(matches!(f.name.origin, FaceOrigin::Instance { instance: 1, of, .. } if of == part.feature.0), "{:?}", f.name);
    }
    // The original's top face is still its own.
    fillet_top(&mut d, top, 1000.0);
    // Copy in place: two parts where one was.
    let (mut d, _, part) = one_block();
    let mut x = TransformFeature::new(TransformType::CopyInPlace);
    x.parts = vec![part.id];
    d.transform(x);
    let parts = d.parts();
    assert_eq!(parts.len(), 2);
    close3(center(&parts[1]), [10.0, 5.0, 2.5], 1e-9);
}

#[test]
fn translate_by_line_and_distance_and_errors() {
    let (mut d, _, part) = one_block();
    // Along the block's bottom edge on y = 0 (20 mm along X, one way or the other).
    let front = face_along(&part, [0.0, -1.0, 0.0]).face;
    let bottom = face_along(&part, [0.0, 0.0, -1.0]).face;
    let edge = part.solid.edges.iter().find(|e| e.name.touches(&front) && e.name.touches(&bottom)).unwrap();
    let r = cadrs_core::EdgeRef { part: part.id, edge: edge.name, seed: edge.midpoint() };
    let mut x = TransformFeature::new(TransformType::TranslateByLine);
    x.parts = vec![part.id];
    x.line = Some(cadrs_core::document::DirectionRef::Edge(r));
    let t = d.transform(x.clone());
    let cx = center(&d.parts()[0])[0];
    assert!((cx - 30.0).abs() < 1e-9 || (cx + 10.0).abs() < 1e-9, "{cx}");
    x.flip = true;
    d.set(t, FeatureKind::Transform(x.clone()));
    close(center(&d.parts()[0])[0], 20.0 - cx, 1e-9);
    // By distance: 7 mm along the Top plane's normal.
    let mut y = TransformFeature::new(TransformType::TranslateByDistance);
    y.parts = vec![part.id];
    y.direction = Some(cadrs_core::document::DirectionRef::PlaneNormal(PlaneRef::Top));
    y.distance = 7.0;
    d.set(t, FeatureKind::Transform(y.clone()));
    close3(center(&d.parts()[0]), [10.0, 5.0, 9.5], 1e-9);
    y.flip = true;
    d.set(t, FeatureKind::Transform(y.clone()));
    close3(center(&d.parts()[0]), [10.0, 5.0, -4.5], 1e-9);
    // No axis: the feature says what's missing.
    let mut z = TransformFeature::new(TransformType::Rotate);
    z.parts = vec![part.id];
    d.set(t, FeatureKind::Transform(z));
    let b = d.build();
    assert_eq!(b.error(t), Some("Select an axis of rotation"));
    // A part that is gone: an error, and the part stays put.
    let gone = cadrs_core::PartId::new(FeatureId::new(), 0);
    d.set(t, FeatureKind::Transform(TransformFeature::translate_xyz(vec![gone], [1.0, 0.0, 0.0])));
    let b = d.build();
    assert_eq!(b.error(t), Some("The part to transform no longer exists"));
    close3(center(&b.parts[0]), [10.0, 5.0, 2.5], 1e-9);
    // One of two gone: a warning, the other moved.
    d.set(t, FeatureKind::Transform(TransformFeature::translate_xyz(vec![part.id, gone], [1.0, 0.0, 0.0])));
    let b = d.build();
    assert!(b.error(t).is_none());
    assert!(b.warning(t).is_some());
    close3(center(&b.parts[0]), [11.0, 5.0, 2.5], 1e-9);
}

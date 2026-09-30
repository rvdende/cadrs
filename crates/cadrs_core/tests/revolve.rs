//! P3.4: the Revolve feature through the document's commands and the rebuild: its types, a
//! torus, and the course's Reducer Coupling (PS8) in inches, with its mass properties. Every
//! expected value is derived by hand in the test's comment.
#![cfg(feature = "occt")]

use std::f64::consts::PI;

use cadrs_core::commands::{AddExtrude, AddRevolve, AddSketch, EditSketch, SetExtrude, SetRevolve};
use cadrs_core::document::{
    AxisRef, BooleanOp, Document, EndCondition, EndType, ExtrudeFeature, Offset, RevolveFeature, RevolveType,
};
use cadrs_core::rebuild;
use cadrs_core::samples;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part};
use cadrs_sketch::{CurveId, CurveKind, PlaneRef, Sketch, SketchOp, Vec2};

/// Millimetres per inch.
const IN: f64 = 25.4;

struct Doc {
    d: Document,
    h: History,
    el: ElementId,
}

impl Doc {
    fn new() -> Self {
        let d = Document::new("P3.4");
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

    /// The construction line of a sketch (its revolve axis).
    fn centreline(&self, sketch: FeatureId) -> CurveId {
        let g = self.g(sketch);
        g.curves
            .iter()
            .find(|(_, c)| c.construction && matches!(c.kind, CurveKind::Line { .. }))
            .map(|(id, _)| id)
            .expect("a centreline")
    }

    fn revolve(&mut self, r: RevolveFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddRevolve { element: self.el, feature: f, revolve: RevolveFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetRevolve { element: self.el, feature: f, revolve: r, label: "Revolve".into() })
            .unwrap();
        f
    }

    fn extrude(&mut self, e: ExtrudeFeature) -> FeatureId {
        let f = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddExtrude { element: self.el, feature: f, extrude: ExtrudeFeature::default() })
            .unwrap();
        self.h
            .execute(&mut self.d, &SetExtrude { element: self.el, feature: f, extrude: e, label: "Extrude".into() })
            .unwrap();
        f
    }

    fn parts(&self) -> Vec<Part> {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        b.parts.clone()
    }
}

fn v(x: f64, y: f64) -> Vec2 {
    Vec2::new(x, y)
}

fn polygon(points: &[Vec2], construction: bool) -> SketchOp {
    SketchOp::AddPolyline {
        points: points.to_vec(),
        closed: points.len() > 2,
        construction,
        label: "Add line",
    }
}

fn circle(x: f64, y: f64, r: f64, construction: bool) -> SketchOp {
    SketchOp::AddCircle { center: v(x, y), radius: r, construction }
}

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

fn volume(p: &Part) -> f64 {
    p.mass.unwrap().volume
}

/// A revolve of the regions of `sketch` under `seeds` about its centreline.
fn revolve_of(doc: &Doc, sketch: FeatureId, seeds: &[Vec2], kind: RevolveType, angle: f64) -> RevolveFeature {
    let regions = samples::region_refs(sketch, &doc.g(sketch), seeds);
    assert_eq!(regions.len(), seeds.len());
    RevolveFeature {
        regions,
        axis: Some(AxisRef::SketchCurve { sketch, curve: doc.centreline(sketch) }),
        kind,
        angle,
        angle_expr: format!("{angle} deg"),
        ..RevolveFeature::default()
    }
}

/// PS7.3: Full, Blind 90° and Symmetric of a 5 × 20 rectangle 5 mm off a vertical centreline
/// on Front (a tube r 5..10, 20 long about Z): V = π(10² − 5²)·20 = 1500π for the whole turn,
/// a quarter for 90° either way. A torus from a Ø10 circle 20 from the axis: V = 2π²Rr² =
/// 2π²·20·25 = 1000π² (Pappus: the area πr² swept round 2πR).
#[test]
fn revolve_types_and_torus() {
    let mut doc = Doc::new();
    let s = doc.sketch(
        PlaneRef::Front,
        vec![polygon(&[v(0.0, -5.0), v(0.0, 40.0)], true), polygon(&[v(5.0, 0.0), v(10.0, 0.0), v(10.0, 20.0), v(5.0, 20.0)], false)],
    );
    let full = doc.revolve(revolve_of(&doc, s, &[v(7.5, 10.0)], RevolveType::Full, 90.0));
    let tube = 1500.0 * PI;
    let parts = doc.parts();
    assert_eq!(parts.len(), 1);
    close(volume(&parts[0]), tube, 1e-6);
    assert_eq!(parts[0].feature, full);
    // The sketch is used (hidden, greyed).
    assert!(cadrs_core::parts::sketch_consumed(&doc.features(), s));
    // Blind 90° and Symmetric 90°, each its own part.
    let b = revolve_of(&doc, s, &[v(7.5, 10.0)], RevolveType::Blind, 90.0);
    let blind = doc.revolve(RevolveFeature { op: BooleanOp::New, ..b.clone() });
    let sym = doc.revolve(RevolveFeature { kind: RevolveType::Symmetric, ..b.clone() });
    let parts = doc.parts();
    let of = |f: FeatureId| parts.iter().find(|p| p.feature == f).unwrap();
    close(volume(of(blind)), tube / 4.0, 1e-6);
    close(volume(of(sym)), tube / 4.0, 1e-6);
    // Blind turns counter-clockwise about +Z (the centreline drawn upward) from +X, so towards
    // +Y; Symmetric is split across the plane (centroid y = 0); flipped, towards −Y.
    let c = |f: FeatureId| of(f).mass.unwrap().center_of_mass;
    assert!(c(blind).y > 1.0, "{:?}", c(blind));
    close(c(sym).y, 0.0, 1e-9);
    let flipped = doc.revolve(RevolveFeature { flip: true, ..b.clone() });
    let parts = doc.parts();
    let of = |f: FeatureId| parts.iter().find(|p| p.feature == f).unwrap();
    assert!(of(flipped).mass.unwrap().center_of_mass.y < -1.0);
    // A second end: 90° + 45°, three eighths.
    let two = doc.revolve(RevolveFeature {
        second: Some(EndCondition { end: EndType::Blind, depth: 45.0, depth_expr: "45 deg".into(), up_to: None, offset: None }),
        ..b
    });
    let parts = doc.parts();
    close(volume(parts.iter().find(|p| p.feature == two).unwrap()), tube * 3.0 / 8.0, 1e-6);

    // The torus.
    let mut doc = Doc::new();
    let s = doc.sketch(PlaneRef::Front, vec![polygon(&[v(0.0, 0.0), v(0.0, 30.0)], true), circle(20.0, 10.0, 5.0, false)]);
    doc.revolve(revolve_of(&doc, s, &[v(20.0, 10.0)], RevolveType::Full, 90.0));
    let parts = doc.parts();
    close(volume(&parts[0]), 2.0 * PI * PI * 20.0 * 25.0, 1e-6);
    // One toroidal face, sampled as a grid of meridians for its silhouettes: seen from above,
    // two circles (r 15 and 25) round the axis.
    let solid = &parts[0].solid;
    assert_eq!(solid.faces.len(), 1);
    assert_eq!(solid.grids.len(), 1);
    // Seen slightly off the axis, so the silhouettes don't run through the grid's own points.
    let v = {
        let (x, z): (f64, f64) = (0.01, 1.0);
        let l = (x * x + z * z).sqrt();
        [x / l, 0.0, z / l]
    };
    let segs = solid.grids[0].silhouette(v);
    let (mut inner, mut outer) = (0, 0);
    for [(a, _), (b, _)] in &segs {
        for p in [a, b] {
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            if (r - 15.0).abs() < 0.1 {
                inner += 1;
            } else if (r - 25.0).abs() < 0.1 {
                outer += 1;
            } else {
                panic!("r = {r}");
            }
            assert!((p[2] - 10.0).abs() < 0.1, "z = {}", p[2]);
        }
    }
    // Round both circles: a segment between every two of the 72 meridians.
    assert!(inner >= 2 * 72 && outer >= 2 * 72, "{inner} {outer}");
}

/// PS7.4, PS7.5 through the feature list: a groove cut by a revolve (Remove), a Surface
/// revolve (a "Surface N" with no volume), a Thin revolve, and Up to part.
///
/// - A cylinder r 20, 40 tall on Top (Part 1, V = π·400·40 = 16 000π); a 4 × 4 square on
///   Front at x 18..22, z 18..22, revolved Full about Z, Remove: the groove is the ring
///   r 18..20 (inside the part), z 18..22: V = π(400 − 324)·4 = 304π, left 16 000π − 304π.
/// - The square's loop as a Surface, Full: its 4 sides turned: the cylinders 2π·18·4 and
///   2π·22·4, and the annuli top and bottom, 2·π(22² − 18²): 144π + 176π + 640π = 960π.
/// - Thin (1 mm inside the square's loop), Full: the band (the 4 × 4 square less the 2 × 2
///   inside it: 12) turned about its centroid at r 20 (Pappus): 12·2π·20 = 480π.
/// - Up to part: the square turned from its plane about Z until it meets a block standing at
///   x ≤ 0, y 10..60 (a quarter turn about +Z reaches x = 0): V = the quarter ring 16·2π·20/4.
#[test]
fn revolve_remove_surface_thin_up_to() {
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![circle(0.0, 0.0, 20.0, false)]);
    doc.extrude(ExtrudeFeature {
        sketches: vec![s1],
        depth: 40.0,
        depth_expr: "40 mm".into(),
        ..ExtrudeFeature::default()
    });
    let s2 = doc.sketch(
        PlaneRef::Front,
        vec![
            polygon(&[v(0.0, -5.0), v(0.0, 60.0)], true),
            polygon(&[v(18.0, 18.0), v(22.0, 18.0), v(22.0, 22.0), v(18.0, 22.0)], false),
        ],
    );
    let groove = doc.revolve(RevolveFeature {
        op: BooleanOp::Remove,
        ..revolve_of(&doc, s2, &[v(20.0, 20.0)], RevolveType::Full, 90.0)
    });
    let parts = doc.parts();
    assert_eq!(parts.len(), 1);
    close(volume(&parts[0]), 16_000.0 * PI - 304.0 * PI, 1e-6);
    assert!(parts[0].features.contains(&groove));
    // A Surface revolve of the square's loop: a closed ring surface, no volume.
    let surf = doc.revolve(RevolveFeature {
        body: cadrs_core::document::BodyType::Surface,
        sketches: vec![s2],
        regions: vec![],
        ..revolve_of(&doc, s2, &[v(20.0, 20.0)], RevolveType::Full, 90.0)
    });
    let parts = doc.parts();
    let sp = parts.iter().find(|p| p.feature == surf).expect("a surface part");
    assert_eq!(sp.kind, cadrs_core::parts::PartKind::Surface);
    assert!(sp.name.starts_with("Surface"));
    let m = sp.mass.unwrap();
    close(m.volume, 0.0, 1e-9);
    // Sides r 18 and r 22 (2πr·4) and the annular top and bottom (π(22² − 18²) each).
    let area = 2.0 * PI * 18.0 * 4.0 + 2.0 * PI * 22.0 * 4.0 + 2.0 * PI * (22.0 * 22.0 - 18.0 * 18.0);
    close(m.surface_area, area, 1e-6);
    // Thin, 1 mm inside the square's loop: the band's area 16 − 4 = 12 at mean radius 20.
    let thin = doc.revolve(RevolveFeature {
        body: cadrs_core::document::BodyType::Thin,
        thin: cadrs_core::document::ThinWall {
            thickness1: 1.0,
            thickness1_expr: "1 mm".into(),
            ..Default::default()
        },
        op: BooleanOp::New,
        ..revolve_of(&doc, s2, &[v(20.0, 20.0)], RevolveType::Full, 90.0)
    });
    let parts = doc.parts();
    let tp = parts.iter().find(|p| p.feature == thin).expect("a thin part");
    close(volume(tp), 12.0 * 2.0 * PI * 20.0, 1e-6);

    // Up to part.
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![polygon(&[v(-30.0, 10.0), v(0.0, 10.0), v(0.0, 60.0), v(-30.0, 60.0)], false)]);
    let block = doc.extrude(ExtrudeFeature {
        sketches: vec![s1],
        depth: 40.0,
        depth_expr: "40 mm".into(),
        ..ExtrudeFeature::default()
    });
    let s2 = doc.sketch(
        PlaneRef::Front,
        vec![
            polygon(&[v(0.0, -5.0), v(0.0, 60.0)], true),
            polygon(&[v(18.0, 18.0), v(22.0, 18.0), v(22.0, 22.0), v(18.0, 22.0)], false),
        ],
    );
    let block_part = doc.parts().iter().find(|p| p.feature == block).unwrap().id;
    let up = doc.revolve(RevolveFeature {
        kind: RevolveType::UpToPart,
        up_to: Some(cadrs_core::document::UpTo::Part(block_part)),
        op: BooleanOp::New,
        ..revolve_of(&doc, s2, &[v(20.0, 20.0)], RevolveType::Full, 90.0)
    });
    let parts = doc.parts();
    let q = parts.iter().find(|p| p.feature == up).expect("the revolved piece");
    close(volume(q), 16.0 * 2.0 * PI * 20.0 / 4.0, 1e-5);
}

/// PS7.2 and PS7.1 with part geometry: the revolve axis from a cylindrical face, a circular edge
/// and a straight edge, and a planar part face as the input.
///
/// - A cylinder r 10, 30 tall on Top (Part 1). A 5 × 10 rectangle on Front at x 15..20, z 0..10,
///   turned Full about the cylinder's face (its axis, Z): the ring r 15..20, 10 tall:
///   V = π(20² − 15²)·10 = 1750π. About the cylinder's top circular edge (the same axis): the
///   same.
/// - A 4 × 4 × 30 bar at x 0..4, y 0..4: its edge along Z at the origin is the axis; the
///   rectangle turned about it: 1750π again.
/// - The bar's face y = 0 (x 0..4, z 0..30, in a plane through that edge) as the input, turned
///   Full about the edge: a cylinder r 4, 30 tall: 480π.
#[test]
fn revolve_about_part_geometry_and_of_a_face() {
    use cadrs_core::document::{EdgeRef, FaceRef};
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![circle(0.0, 0.0, 10.0, false)]);
    let cyl = doc.extrude(ExtrudeFeature { sketches: vec![s1], depth: 30.0, depth_expr: "30 mm".into(), ..ExtrudeFeature::default() });
    let s2 = doc.sketch(PlaneRef::Front, vec![polygon(&[v(15.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(15.0, 10.0)], false)]);
    let regions = samples::region_refs(s2, &doc.g(s2), &[v(17.5, 5.0)]);
    let parts = doc.parts();
    let part = parts.iter().find(|p| p.feature == cyl).unwrap();
    let (fi, side) = part
        .solid
        .faces
        .iter()
        .enumerate()
        .find(|(_, f)| f.plane.is_none())
        .expect("the cylindrical face");
    let face_ref = FaceRef { part: part.id, face: side.name, seed: part.solid.face_point(fi).unwrap() };
    let top_edge = part
        .solid
        .edges
        .iter()
        .find(|e| e.circle.is_some_and(|c| (c.center[2] - 30.0).abs() < 1e-9))
        .expect("the top circular edge");
    let edge_ref = EdgeRef { part: part.id, edge: top_edge.name, seed: top_edge.midpoint() };
    let ring = 1750.0 * PI;
    let by_face = doc.revolve(RevolveFeature { regions: regions.clone(), axis: Some(AxisRef::Face(face_ref)), ..RevolveFeature::default() });
    let by_edge = doc.revolve(RevolveFeature { regions: regions.clone(), axis: Some(AxisRef::Edge(edge_ref)), ..RevolveFeature::default() });
    let parts = doc.parts();
    let of = |f: FeatureId| parts.iter().find(|p| p.feature == f).expect("a part");
    close(volume(of(by_face)), ring, 1e-6);
    close(volume(of(by_edge)), ring, 1e-6);

    // A straight edge, and a face as the input.
    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Top, vec![polygon(&[v(0.0, 0.0), v(4.0, 0.0), v(4.0, 4.0), v(0.0, 4.0)], false)]);
    let bar = doc.extrude(ExtrudeFeature { sketches: vec![s1], depth: 30.0, depth_expr: "30 mm".into(), ..ExtrudeFeature::default() });
    let s2 = doc.sketch(PlaneRef::Front, vec![polygon(&[v(15.0, 0.0), v(20.0, 0.0), v(20.0, 10.0), v(15.0, 10.0)], false)]);
    let regions = samples::region_refs(s2, &doc.g(s2), &[v(17.5, 5.0)]);
    let parts = doc.parts();
    let part = parts.iter().find(|p| p.feature == bar).unwrap();
    let z_edge = part
        .solid
        .edges
        .iter()
        .find(|e| e.points.iter().all(|p| p[0].abs() < 1e-9 && p[1].abs() < 1e-9))
        .expect("the edge along Z at the origin");
    let edge_ref = EdgeRef { part: part.id, edge: z_edge.name, seed: z_edge.midpoint() };
    let (fi, front) = part
        .solid
        .faces
        .iter()
        .enumerate()
        .find(|(_, f)| f.plane.is_some_and(|p| p.normal()[1] < -0.999))
        .expect("the face y = 0");
    let face_ref = FaceRef { part: part.id, face: front.name, seed: part.solid.face_point(fi).unwrap() };
    let by_line = doc.revolve(RevolveFeature { regions, axis: Some(AxisRef::Edge(edge_ref)), ..RevolveFeature::default() });
    let of_face = doc.revolve(RevolveFeature { faces: vec![face_ref], axis: Some(AxisRef::Edge(edge_ref)), ..RevolveFeature::default() });
    let parts = doc.parts();
    let of = |f: FeatureId| parts.iter().find(|p| p.feature == f).expect("a part");
    close(volume(of(by_line)), ring, 1e-6);
    close(volume(of(of_face)), PI * 16.0 * 30.0, 1e-6);
}

/// A revolve and a diametral dimension survive saving and reloading exactly, and their commands
/// undo and redo exactly.
#[test]
fn revolves_and_diametral_dimensions_persist_and_undo() {
    let mut doc = Doc::new();
    let s = doc.sketch(
        PlaneRef::Front,
        vec![
            polygon(&[v(0.0, 0.0), v(0.0, 40.0)], true),
            polygon(&[v(5.0, 0.0), v(10.0, 0.0), v(10.0, 20.0), v(5.0, 20.0)], false),
        ],
    );
    // Ø20 from the corner (10, 20) to the centreline: the corner moves to x = ±10… it is there.
    let g = doc.g(s);
    let corner = g.point_at(v(10.0, 20.0), 1e-9).unwrap();
    let axis = doc.centreline(s);
    let dia = cadrs_sketch::Dimension::new(
        cadrs_sketch::DimensionKind::Diametral {
            p: cadrs_sketch::constraint::PointRef::Point(corner),
            line: cadrs_sketch::constraint::CurveRef::Curve(axis),
        },
        20.0,
        0.0,
    );
    doc.h
        .execute(&mut doc.d, &EditSketch { element: doc.el, feature: s, op: SketchOp::SetDimension { dimension: dia, moves: vec![], radii: vec![] } })
        .unwrap();
    doc.revolve(RevolveFeature {
        second: Some(EndCondition { end: EndType::Blind, depth: 30.0, depth_expr: "30 deg".into(), up_to: None, offset: None }),
        ..revolve_of(&doc, s, &[v(7.5, 10.0)], RevolveType::Blind, 120.0)
    });
    close(volume(&doc.parts()[0]), 1500.0 * PI * 150.0 / 360.0, 1e-6);
    let dir = std::env::temp_dir().join(format!("cadrs-revolve-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = cadrs_core::Store::new(&dir);
    let meta = cadrs_core::DocumentMeta::new("me", 1_000);
    store.create(&doc.d, &meta).unwrap();
    let back = store.load(doc.d.id).unwrap();
    assert_eq!(back.document, doc.d);
    let _ = std::fs::remove_dir_all(dir);
    // Undo everything, redo everything.
    let end = doc.d.clone();
    let n = doc.h.undo_len();
    while doc.h.undo(&mut doc.d).is_some() {}
    assert!(doc.d.elements[0].features().is_empty());
    for _ in 0..n {
        doc.h.redo(&mut doc.d).unwrap();
    }
    assert_eq!(doc.d, end);
}

/// The Reducer Coupling's two flanges' sketches (PS8.2, PS8.6): an outer circle, a bore and four
/// equal holes on a construction bolt circle at the quadrants (inches).
fn flange(outer: f64, bore: f64, bolt: f64, hole: f64) -> Vec<SketchOp> {
    let mut ops = vec![
        circle(0.0, 0.0, outer / 2.0 * IN, false),
        circle(0.0, 0.0, bore / 2.0 * IN, false),
        circle(0.0, 0.0, bolt / 2.0 * IN, true),
    ];
    let r = bolt / 2.0 * IN;
    for (x, y) in [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)] {
        ops.push(circle(x, y, hole / 2.0 * IN, false));
    }
    ops
}

/// mm³ → in³ and mm² → in².
fn in3(mm3: f64) -> f64 {
    mm3 / IN.powi(3)
}
fn in2(mm2: f64) -> f64 {
    mm2 / IN.powi(2)
}

/// The Reducer Coupling's volume and area, analytically (in³, in²).
///
/// Volume 53.9318 = flange 1 14.8214 + transition 13.0542 + flange 2 26.0562:
/// - flange 1: Ø6 less the Ø2 bore and 4 × Ø0.625, 0.62 thick: 0.62π(9 − 1 − 4·0.3125²)
///   = 0.62π·7.609375 = 14.8214;
/// - the transition (Pappus): the parallelogram from x = 0.62 to 5.245 between the lines r 1.0 →
///   1.5625 and 1.3125 → 1.875 has area 4.625 × 0.3125 = 1.4453125 and its centroid at the mean
///   radius 1.4375: 2π·1.4375·1.4453125 = 13.0542;
/// - flange 2: Ø7.5 less the Ø3.125 bore and 4 × Ø0.75, 0.75 thick: 0.75π(14.0625 − 2.44140625
///   − 4·0.375²) = 0.75π·11.05859375 = 26.0562.
///
/// Area 248.367: flange 1's rim 2π·3·0.62, bore 2π·1·0.62, holes 4·2π·0.3125·0.62, back face
/// π·7.609375 and front face π·7.609375 less the transition's foot π(1.3125² − 1²); the two
/// cones π(r₁ + r₂)·L with L = √(4.625² + 0.5625²): π(1.3125 + 1.875)L outside and π(1 +
/// 1.5625)L inside; flange 2's rim 2π·3.75·0.75, bore 2π·1.5625·0.75, holes 4·2π·0.375·0.75,
/// front face π·11.05859375 and back face that less the transition's end π(1.875² − 1.5625²).
fn reducer_expected() -> (f64, f64) {
    let f1 = 0.62 * PI * (9.0 - 1.0 - 4.0 * 0.3125f64.powi(2));
    let tr = 2.0 * PI * 1.4375 * (4.625 * 0.3125);
    let f2 = 0.75 * PI * (3.75f64.powi(2) - 1.5625f64.powi(2) - 4.0 * 0.375f64.powi(2));
    let l = (4.625f64.powi(2) + 0.5625f64.powi(2)).sqrt();
    let face1 = PI * (9.0 - 1.0 - 4.0 * 0.3125f64.powi(2));
    let face2 = PI * (3.75f64.powi(2) - 1.5625f64.powi(2) - 4.0 * 0.375f64.powi(2));
    let area = 2.0 * PI * 3.0 * 0.62
        + 2.0 * PI * 1.0 * 0.62
        + 4.0 * 2.0 * PI * 0.3125 * 0.62
        + face1
        + face1 - PI * (1.3125f64.powi(2) - 1.0)
        + PI * (1.3125 + 1.875) * l
        + PI * (1.0 + 1.5625) * l
        + 2.0 * PI * 3.75 * 0.75
        + 2.0 * PI * 1.5625 * 0.75
        + 4.0 * 2.0 * PI * 0.375 * 0.75
        + face2
        + face2 - PI * (1.875f64.powi(2) - 1.5625f64.powi(2));
    (f1 + tr + f2, area)
}

/// PS8 (`ex2-*`), in inches, as the course builds it: Sketch 1 on Right (Ø6, Ø2 bore, 4 ×
/// Ø0.625 on a Ø4.75 bolt circle), Extrude 1 New 0.62 in (the whole sketch: the bore and holes
/// left out); Sketch 2 on Front (the parallelogram from the bore's top at x 0.62 to x 5.245, Ø2.625
/// and Ø3.75 diametral to the X centreline), Revolve 1 Add, Full about the centreline; Sketch 3
/// on the revolve's end face (the Ø3.125 inner edge, Ø7.5, 4 × Ø0.75 on a Ø6 bolt circle),
/// Extrude 2 Add 0.75 in. One part: Volume 53.932 in³ (the course's screenshot, `ex2-step10`)
/// and Surface area 248.367 in² (the course's self-check), both to 1e−3.
#[test]
fn reducer_coupling() {
    let (want_v, want_a) = reducer_expected();
    close(want_v, 53.932, 1e-3);
    close(want_a, 248.367, 1e-3);

    let mut doc = Doc::new();
    let s1 = doc.sketch(PlaneRef::Right, flange(6.0, 2.0, 4.75, 0.625));
    doc.extrude(ExtrudeFeature {
        sketches: vec![s1],
        depth: 0.62 * IN,
        depth_expr: "0.62 in".into(),
        ..ExtrudeFeature::default()
    });
    // Sketch 2: the parallelogram (with its diametral dimensions) and the centreline.
    let s2 = doc.sketch(
        PlaneRef::Front,
        vec![
            polygon(&[v(0.0, 0.0), v(6.0 * IN, 0.0)], true),
            polygon(
                &[v(0.62 * IN, 1.0 * IN), v(0.62 * IN, 1.3125 * IN), v(5.245 * IN, 1.875 * IN), v(5.245 * IN, 1.5625 * IN)],
                false,
            ),
        ],
    );
    let rev = doc.revolve(RevolveFeature {
        op: BooleanOp::Add,
        merge_all: true,
        ..revolve_of(&doc, s2, &[v(3.0 * IN, 1.45 * IN)], RevolveType::Full, 90.0)
    });
    let parts = doc.parts();
    assert_eq!(parts.len(), 1, "the revolve joins Part 1");
    // Sketch 3 on the revolve's end face (x = 5.245 in, facing +X).
    let end_face = parts[0]
        .solid
        .faces
        .iter()
        .find(|f| {
            f.name.op == rev.0
                && f.plane.is_some_and(|p| {
                    let n = p.normal();
                    n[0] > 0.999 && (p.origin[0] - 5.245 * IN).abs() < 1e-6
                })
        })
        .expect("the revolve's end face")
        .name;
    let plane = cadrs_core::parts::face_plane(&doc.features(), rev, end_face).expect("a sketch plane");
    // The face's frame is centred on the axis (the annulus's centroid).
    let frame = plane.frame();
    close(frame.origin[1], 0.0, 1e-6);
    close(frame.origin[2], 0.0, 1e-6);
    // Use (PS8.6) of the revolve's inner circular edge on that face: the kernel's exact circle,
    // Ø3.125 about the sketch's origin.
    let solid = &parts[0].solid;
    let inner = solid
        .edges
        .iter()
        .find(|e| e.circle.is_some_and(|c| (c.radius - 1.5625 * IN).abs() < 1e-9 && (c.center[0] - 5.245 * IN).abs() < 1e-9))
        .expect("the Ø3.125 edge");
    let features = doc.features();
    let ctx = cadrs_core::links::LinkContext { solids: vec![(parts[0].feature, solid)], features: &features };
    let link = cadrs_sketch::Link::Edge { feature: rev.0, edge: inner.name };
    match ctx.shape(link, &frame) {
        Some(cadrs_sketch::projection::Projected::Circle(c, r)) => {
            close(r, 1.5625 * IN, 1e-9);
            close(c.x, 0.0, 1e-9);
            close(c.y, 0.0, 1e-9);
        }
        other => panic!("not a circle: {other:?}"),
    }
    // Use of the outer (conical) face's silhouettes on Front: its two generator lines, from
    // r 1.3125 at x 0.62 to r 1.875 at x 5.245, above and below the axis.
    let cone = solid
        .faces
        .iter()
        .find(|f| f.name.op == rev.0 && f.plane.is_none() && solid.face_contains(solid.faces.iter().position(|g| g.name == f.name).unwrap(), [3.0 * IN, 0.0, (1.3125 + (1.875 - 1.3125) * (3.0 - 0.62) / 4.625) * IN]))
        .expect("the outer cone");
    let front = PlaneRef::Front.frame();
    let lines = cadrs_core::links::silhouettes(solid, &cone.name, front.normal());
    assert_eq!(lines.len(), 2, "{lines:?}");
    for (a, b) in lines {
        let (a, b) = (front.to_sketch(a), front.to_sketch(b));
        let (a, b) = if a.x < b.x { (a, b) } else { (b, a) };
        close(a.x, 0.62 * IN, 1e-6);
        close(b.x, 5.245 * IN, 1e-6);
        close(a.y.abs(), 1.3125 * IN, 1e-6);
        close(b.y.abs(), 1.875 * IN, 1e-6);
    }
    let s3 = doc.sketch(plane, flange(7.5, 3.125, 6.0, 0.75));
    doc.extrude(ExtrudeFeature {
        sketches: vec![s3],
        op: BooleanOp::Add,
        merge_all: true,
        depth: 0.75 * IN,
        depth_expr: "0.75 in".into(),
        ..ExtrudeFeature::default()
    });
    let parts = doc.parts();
    assert_eq!(parts.len(), 1, "one part");
    let m = parts[0].mass.unwrap();
    close(in3(m.volume), want_v, 1e-3);
    close(in2(m.surface_area), want_a, 1e-3);
    // As the panel shows them (3 decimals).
    assert_eq!(format!("{:.3}", in3(m.volume)), "53.932");
    assert_eq!(format!("{:.3}", in2(m.surface_area)), "248.367");

    // PS8.9 (optional): one revolve for both flanges and the transition, then a hole extrude
    // (Remove) per flange: the same part.
    let mut doc = Doc::new();
    let pts: Vec<Vec2> = [
        (0.0, 1.0),
        (0.0, 3.0),
        (0.62, 3.0),
        (0.62, 1.3125),
        (5.245, 1.875),
        (5.245, 3.75),
        (5.995, 3.75),
        (5.995, 1.5625),
        (5.245, 1.5625),
        (0.62, 1.0),
    ]
    .iter()
    .map(|(x, y)| v(x * IN, y * IN))
    .collect();
    let s = doc.sketch(PlaneRef::Front, vec![polygon(&[v(0.0, 0.0), v(7.0 * IN, 0.0)], true), polygon(&pts, false)]);
    doc.revolve(revolve_of(&doc, s, &[v(0.3 * IN, 2.0 * IN)], RevolveType::Full, 90.0));
    let holes = |d: &mut Doc, bolt: f64, hole: f64, start: f64, depth: f64| {
        let r = bolt / 2.0 * IN;
        let ops: Vec<SketchOp> = [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)]
            .iter()
            .map(|(x, y)| circle(*x, *y, hole / 2.0 * IN, false))
            .collect();
        let sk = d.sketch(PlaneRef::Right, ops);
        d.extrude(ExtrudeFeature {
            sketches: vec![sk],
            op: BooleanOp::Remove,
            start_offset: (start > 0.0).then(|| Offset { value: start * IN, expr: format!("{start} in"), flip: false }),
            depth: depth * IN,
            depth_expr: format!("{depth} in"),
            ..ExtrudeFeature::default()
        });
    };
    holes(&mut doc, 4.75, 0.625, 0.0, 0.62);
    holes(&mut doc, 6.0, 0.75, 5.245, 0.75);
    let parts = doc.parts();
    assert_eq!(parts.len(), 1);
    let m = parts[0].mass.unwrap();
    close(in3(m.volume), want_v, 1e-3);
    close(in2(m.surface_area), want_a, 1e-3);
}

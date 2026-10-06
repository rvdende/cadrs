//! P3.8 / PS27: the Rocket Guidance Reflector on the stand-in (`cadrs_core::samples::reflector`),
//! every course step through the feature list, with the volume each step takes away checked
//! against a closed form (or, for the fillets on the curved pocket floors, bounds and the mesh),
//! the centre of mass on the Z axis (the 4-fold pattern and the mirror make the plate symmetric)
//! and the mass in kg. `course_ps27_reflector` makes the same steps through the UI.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::applied::{EdgeOrFace, FilletFeature, HoleFeature, HolePoint};
use cadrs_core::commands::{AddExtrude, AddFeature, AddSketch, EditSketch, SetExtrude};
use cadrs_core::document::{AxisRef, BooleanOp, DirectionRef, EdgeRef, EndType, ExtrudeFeature, FaceRef, Offset, UpTo};
use cadrs_core::hole::{Fit, HoleEnd, HoleSpec, HoleStart, HoleType, Length};
use cadrs_core::mate::ConnectorRef;
use cadrs_core::parts::mass_report;
use cadrs_core::pattern::{MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};
use cadrs_core::rebuild;
use cadrs_core::samples::{self, reflector as rf};
use cadrs_core::{Document, ElementId, Feature, FeatureId, FeatureKind, History, Part};
use cadrs_sketch::{FaceOrigin, PlaneRef, SketchOp, Vec2};

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

// ---------------------------------------------------------------------------------------------
// Quadrature: Gauss–Legendre, for the integrals of the curved bottom over the plate's regions

/// `n` Gauss–Legendre nodes and weights on [0, 1].
fn gauss(n: usize) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for i in 1..=n {
        let mut x = (PI * (i as f64 - 0.25) / (n as f64 + 0.5)).cos();
        let mut dp = 0.0;
        for _ in 0..100 {
            let (mut p0, mut p1) = (1.0, x);
            for k in 2..=n {
                let p2 = ((2 * k - 1) as f64 * x * p1 - (k - 1) as f64 * p0) / k as f64;
                p0 = p1;
                p1 = p2;
            }
            dp = n as f64 * (x * p1 - p0) / (x * x - 1.0);
            let dx = p1 / dp;
            x -= dx;
            if dx.abs() < 1e-15 {
                break;
            }
        }
        let w = 2.0 / ((1.0 - x * x) * dp * dp);
        out.push(((x + 1.0) / 2.0, w / 2.0));
    }
    out
}

/// ∫∫ f over the triangle `t` (the square [0, 1]² mapped onto it, Duffy).
fn over_triangle(t: &[[f64; 2]; 3], f: &dyn Fn(f64, f64) -> f64) -> f64 {
    let g = gauss(48);
    let [a, b, c] = *t;
    let area2 = ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1])).abs();
    let mut sum = 0.0;
    for &(u, wu) in &g {
        for &(v, wv) in &g {
            let (s, t2) = (u, v * (1.0 - u));
            let x = a[0] + s * (b[0] - a[0]) + t2 * (c[0] - a[0]);
            let y = a[1] + s * (b[1] - a[1]) + t2 * (c[1] - a[1]);
            sum += wu * wv * (1.0 - u) * f(x, y);
        }
    }
    sum * area2
}

/// ∫∫ f over the rounded square (the square less its four corners outside the R36 arcs).
fn over_plate(f: &dyn Fn(f64, f64) -> f64) -> f64 {
    let g = gauss(64);
    let h = rf::HALF;
    let rect = |x0: f64, y0: f64, x1: f64, y1: f64| -> f64 {
        let mut s = 0.0;
        for &(u, wu) in &g {
            for &(v, wv) in &g {
                s += wu * wv * f(x0 + u * (x1 - x0), y0 + v * (y1 - y0));
            }
        }
        s * (x1 - x0) * (y1 - y0)
    };
    let (r, c) = (rf::CORNER_R, rf::HALF - rf::CORNER_R);
    let mut total = rect(-h, -h, h, h);
    for (sx, sy) in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
        // The corner square less the quarter disc about (±c, ±c).
        let (x0, x1) = if sx > 0.0 { (c, h) } else { (-h, -c) };
        let (y0, y1) = if sy > 0.0 { (c, h) } else { (-h, -c) };
        let square = rect(x0, y0, x1, y1);
        let mut disc = 0.0;
        for &(u, wu) in &g {
            for &(v, wv) in &g {
                let (rho, th) = (u * r, v * PI / 2.0);
                disc += wu * wv * rho * f(sx * (c + rho * th.cos()), sy * (c + rho * th.sin()));
            }
        }
        total -= square - disc * r * PI / 2.0;
    }
    total
}

fn bottom_at(x: f64, y: f64) -> f64 {
    rf::bottom((x * x + y * y).sqrt())
}

/// A blind hole's volume: the cylinder and the 118° point (height r / tan 59°).
fn blind_volume(d: f64, depth: f64) -> f64 {
    let r = d / 2.0;
    PI * r * r * depth + PI * r * r * (r / 59f64.to_radians().tan()) / 3.0
}

// ---------------------------------------------------------------------------------------------
// The studio

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = rf::document().unwrap();
        let el = d.elements[0].id;
        Self { d, h: History::default(), el }
    }

    fn features(&self) -> Vec<Feature> {
        self.d.element(self.el).unwrap().features().to_vec()
    }

    fn part(&self) -> Part {
        let b = rebuild::build(&self.features());
        assert!(b.errors.is_empty(), "rebuild errors: {:?}", b.errors);
        assert_eq!(b.parts.len(), 1);
        b.parts[0].clone()
    }

    fn volume(&self) -> f64 {
        self.part().mass.unwrap().volume
    }

    fn add(&mut self, base: &str, kind: FeatureKind) -> FeatureId {
        let feature = FeatureId::new();
        self.h
            .execute(&mut self.d, &AddFeature { element: self.el, feature, base_name: base.into(), kind })
            .unwrap();
        feature
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

    fn regions(&self, seeds: &[Vec2]) -> Vec<cadrs_core::RegionRef> {
        let g = self.d.element(self.el).unwrap().feature(rf::FEATURE_SKETCH).unwrap().sketch().unwrap().geometry.clone();
        let r = samples::region_refs(rf::FEATURE_SKETCH, &g, seeds);
        assert_eq!(r.len(), seeds.len());
        r
    }
}

/// Every edge of the part between two faces both made by `op` (and, if `sides_only`, both
/// sides of the sweep: the vertical edges).
fn edges_of(part: &Part, op: FeatureId, sides_only: bool) -> Vec<EdgeOrFace> {
    part.solid
        .edges
        .iter()
        .filter(|e| e.name.faces.iter().all(|f| f.op == op.0))
        .filter(|e| !sides_only || e.name.faces.iter().all(|f| matches!(f.origin, FaceOrigin::Side { .. })))
        .map(|e| EdgeOrFace::Edge(EdgeRef { part: part.id, edge: e.name, seed: e.midpoint() }))
        .collect()
}

fn faces_of(part: &Part, ops: &[FeatureId]) -> Vec<FaceRef> {
    part.solid
        .faces
        .iter()
        .enumerate()
        .filter(|(_, f)| ops.iter().any(|o| o.0 == f.name.op))
        .map(|(i, f)| FaceRef { part: part.id, face: f.name, seed: part.solid.face_point(i).unwrap() })
        .collect()
}

fn fillet(edges: Vec<EdgeOrFace>, r: f64) -> FeatureKind {
    FeatureKind::Fillet(FilletFeature { entities: edges, size: r, size_expr: format!("{r} mm"), ..FilletFeature::default() })
}

#[test]
#[ignore = "slow (over 5 s): run with cargo test -r -- --ignored"]
fn reflector_every_step() {
    let mut s = Studio::new();
    // The stand-in: the rounded square (area 182² − (4 − π)·36²) 45 high, less the sphere's cap
    // under it: V₀ = 45·A − ∫∫ bottom dA.
    let area = 182.0 * 182.0 - (4.0 - PI) * 36.0 * 36.0;
    close(over_plate(&|_, _| 1.0), area, 1e-8);
    let v0 = rf::HEIGHT * area - over_plate(&bottom_at);
    close(s.volume(), v0, 1e-6 * v0);
    let top = rf::HEIGHT;
    // Differences of volumes of about 1.2e6 mm³: exact to about 1e-9 of that.
    let tol = 1e-9 * v0;

    // Step 1 (PS27.3): Extrude 2, Remove, the two triangles, Up to the curved bottom (Face of
    // Revolve 1) with a 10 mm offset: each triangle loses 45 − 10 − bottom(r) over its area.
    let part = s.part();
    let bottom_face = part
        .solid
        .faces
        .iter()
        .enumerate()
        .find(|(_, f)| f.name.op == rf::REVOLVE_1.0 && f.plane.is_none())
        .map(|(i, f)| FaceRef { part: part.id, face: f.name, seed: part.solid.face_point(i).unwrap() })
        .expect("the curved bottom");
    let tri_a = Vec2::new(60.0, -16.0);
    let tri_b = Vec2::new(16.0, -60.0);
    let e2 = s.extrude(ExtrudeFeature {
        op: BooleanOp::Remove,
        merge_all: true,
        end: EndType::UpToFace,
        up_to: Some(UpTo::Face(bottom_face)),
        offset: Some(Offset { value: 10.0, expr: "10 mm".into(), flip: false }),
        flip: true,
        ..samples::extrude_of(s.regions(&[tri_a, tri_b]), 25.0)
    });
    let depth = |x: f64, y: f64| top - 10.0 - bottom_at(x, y);
    let dv1 = over_triangle(&rf::TRIANGLE_A, &depth) + over_triangle(&rf::TRIANGLE_B, &depth);
    let v1 = s.volume();
    close(v0 - v1, dv1, 1e-6 * dv1);
    let mesh1 = s.part().solid.volume();

    // Step 2 (PS27.4): Fillet 2, 6 mm on the pockets' 12 edges (3 vertical, 3 round the floor
    // each). A fillet in a pocket adds material: in a corner of angle θ, r²(cot(θ/2) − (π − θ)/2)
    // per mm of height; along a floor edge (the floor is nearly flat), (1 − π/4) r² per mm. The
    // fillets on the curved floor have no closed form: the added volume is bounded by those
    // rates (the corners' height at most the pocket's deepest point, the floor edges' length at
    // most the triangles' perimeters), and OCCT's volume is checked against the mesh.
    let part = s.part();
    let edges = edges_of(&part, e2, false);
    assert_eq!(edges.len(), 12, "3 vertical and 3 floor edges per triangle");
    let f2 = s.add("Fillet", fillet(edges, 6.0));
    let v2 = s.volume();
    let added = v2 - v1;
    let r = 6.0f64;
    let corner = |theta: f64| r * r * (1.0 / (theta / 2.0).tan() - (PI - theta) / 2.0);
    let per_triangle_corners = corner(PI / 2.0) + 2.0 * corner(PI / 4.0);
    let deepest = depth(rf::TRIANGLE_A[0][0], rf::TRIANGLE_A[0][1]).max(depth(rf::TRIANGLE_B[0][0], rf::TRIANGLE_B[0][1]));
    let leg = rf::TRIANGLE_A[1][0] - rf::TRIANGLE_A[0][0];
    let perimeter = 2.0 * (2.0 + 2f64.sqrt()) * leg;
    let high = 2.0 * per_triangle_corners * deepest + (1.0 - PI / 4.0) * r * r * perimeter;
    assert!(added > 0.0 && added < high, "the fillets added {added}, expected 0..{high}");
    // The display mesh (0.05 mm chords) sits up to its deflection off the large curved bottom,
    // so the whole volumes differ by a few 1e-4; the fillets' own addition (the same bottom in
    // both meshes) agrees to 2 %.
    let mesh2 = s.part().solid.volume();
    assert!((v2 - mesh2).abs() < 6e-4 * v2, "OCCT {v2} vs mesh {mesh2}");
    let mesh_added = mesh2 - mesh1;
    assert!((mesh_added - added).abs() < 0.02 * added, "the fillets added {added}, the mesh says {mesh_added}");

    // Step 3 (PS27.5): Circular pattern 1, Feature pattern of Extrude 2 and Fillet 2 about the
    // Pattern Axis mate connector, 360°, 4, equal spacing, Reapply: three more copies, each
    // taking what the seed took (the curved bottom turns into itself).
    let mut p = PatternFeature::new(PatternKind::Circular);
    p.pattern_type = PatternType::Feature;
    p.features = vec![e2, f2];
    p.axis = Some(AxisRef::Connector(ConnectorRef::Feature(rf::PATTERN_AXIS)));
    p.first.count = 4;
    p.reapply = true;
    s.add("Circular pattern", FeatureKind::Pattern(p));
    let v3 = s.volume();
    close(v2 - v3, 3.0 * (v0 - v2), 1e-6 * (v0 - v2));

    // Step 4 (PS27.6): Hole, Simple, at the Pattern Axis connector, Clearance M45 Close (Ø46),
    // Start from part, Blind 12, 118°.
    let mut spec = HoleSpec::default();
    spec.hole_type = HoleType::Clearance;
    spec.size = "M45".into();
    spec.fit = Fit::Close;
    spec.apply_table();
    assert_eq!(spec.diameter.value, 46.0);
    spec.depth = Length::mm(12.0);
    spec.end = HoleEnd::Blind;
    spec.start = HoleStart::Part;
    s.add(
        "Hole",
        FeatureKind::Hole(HoleFeature { connectors: vec![ConnectorRef::Feature(rf::PATTERN_AXIS)], spec, ..HoleFeature::default() }),
    );
    let v4 = s.volume();
    close(v3 - v4, blind_volume(46.0, 12.0), tol);

    // Step 5 (PS27.7, PS27.8): Sketch 3 on the top face, a point on the Y axis 26 from the
    // plate's front edge (y = −91 + 26 = −65); a tapped hole M10 × 1.5 there, tap drill Ø8.5,
    // Blind 20, tapped 10.02.
    let features = s.features();
    let top_name = cadrs_core::parts::cap_name(&features, rf::EXTRUDE_1, 0, true).unwrap();
    let sketch3 = FeatureId::new();
    let plane = cadrs_core::parts::face_plane(&features, rf::EXTRUDE_1, top_name).unwrap();
    s.h.execute(&mut s.d, &AddSketch { element: s.el, feature: sketch3, plane: Some(plane) }).unwrap();
    s.h.execute(&mut s.d, &EditSketch { element: s.el, feature: sketch3, op: SketchOp::AddPoint { pos: Vec2::new(0.0, -65.0) } })
        .unwrap();
    let point = s.d.element(s.el).unwrap().feature(sketch3).unwrap().sketch().unwrap().geometry.points.keys().next().unwrap();
    let mut tap = HoleSpec::default();
    tap.hole_type = HoleType::Tapped;
    tap.size = "M10".into();
    tap.apply_table();
    assert_eq!(tap.diameter.value, 8.5);
    tap.depth = Length::mm(20.0);
    tap.tapped_depth = Length::mm(10.02);
    tap.end = HoleEnd::Blind;
    tap.start = HoleStart::Part;
    let hole2 = s.add(
        "Hole",
        FeatureKind::Hole(HoleFeature { points: vec![HolePoint { sketch: sketch3, point }], spec: tap, ..HoleFeature::default() }),
    );
    let v5 = s.volume();
    let tapped = blind_volume(8.5, 20.0);
    close(v4 - v5, tapped, tol);

    // Step 6 (PS27.9): Linear pattern 1, Face pattern of the tapped hole's 2 faces along the
    // plate's straight side edge (x = 91, top), 26 mm, 6 instances, (2, 0) and (3, 0) skipped
    // (they would fall in the Ø46 hole): 3 more tapped holes, at y −39, 39 and 65.
    let part = s.part();
    let faces = faces_of(&part, &[hole2]);
    assert_eq!(faces.len(), 2);
    let side = part
        .solid
        .edges
        .iter()
        .find(|e| {
            let (a, b) = (e.points[0], *e.points.last().unwrap());
            (a[0] - rf::HALF).abs() < 1e-6 && (b[0] - rf::HALF).abs() < 1e-6 && (a[2] - top).abs() < 1e-6 && (b[2] - top).abs() < 1e-6
        })
        .expect("the top edge of the +x side");
    let (a, b) = (side.points[0], *side.points.last().unwrap());
    let mut lp = PatternFeature::new(PatternKind::Linear);
    lp.pattern_type = PatternType::Face;
    lp.faces = faces;
    lp.first.direction = Some(DirectionRef::Edge(EdgeRef { part: part.id, edge: side.name, seed: side.midpoint() }));
    lp.first.flip = b[1] < a[1];
    lp.first.distance = 26.0;
    lp.first.count = 6;
    lp.skip_on = true;
    lp.skipped = vec![[2, 0], [3, 0]];
    let lin = s.add("Linear pattern", FeatureKind::Pattern(lp));
    let v6 = s.volume();
    close(v5 - v6, 3.0 * tapped, tol);
    let b6 = rebuild::build(&s.features());
    let dots = &b6.dots[&lin];
    assert_eq!(dots.len(), 5);
    let ys: Vec<f64> = dots.iter().map(|d| d.at[1]).collect();
    for (y, k) in ys.iter().zip(1..) {
        close(*y, -65.0 + 26.0 * k as f64, 1e-6);
    }

    // Step 7 (PS27.10): Extrude 3, Remove, the rectangle, Blind 11: 13 × 32 × 11.
    let [x0, y0, x1, y1] = rf::RECT;
    let pocket_volume = (x1 - x0) * (y1 - y0) * 11.0;
    let e3 = s.extrude(ExtrudeFeature {
        op: BooleanOp::Remove,
        merge_all: true,
        flip: true,
        depth_expr: "11 mm".into(),
        ..samples::extrude_of(s.regions(&[Vec2::new((x0 + x1) / 2.0, 0.0)]), 11.0)
    });
    let v7 = s.volume();
    close(v6 - v7, pocket_volume, tol);

    // Step 8 (PS27.11): Fillet 3, 6 mm on the pocket's 4 vertical edges: each adds
    // (1 − π/4)·6²·11.
    let part = s.part();
    let edges = edges_of(&part, e3, true);
    assert_eq!(edges.len(), 4);
    let f3 = s.add("Fillet", fillet(edges, 6.0));
    let v8 = s.volume();
    let rounds = 4.0 * (1.0 - PI / 4.0) * 36.0 * 11.0;
    close(v8 - v7, rounds, tol);

    // Step 9 (PS27.12): Mirror 1, Face mirror of the pocket's 9 faces (Create selection →
    // Pocket) across Right: the same pocket at x −88..−75.
    let part = s.part();
    let pocket = faces_of(&part, &[e3, f3]);
    assert_eq!(pocket.len(), 9);
    // Create selection → Pocket from its floor finds the same 9 faces.
    let floor = part.solid.faces.iter().position(|f| f.name.op == e3.0 && matches!(f.name.origin, FaceOrigin::Cap { .. })).unwrap();
    let mut found: Vec<_> = part.solid.pocket_faces(floor).into_iter().map(|i| part.solid.faces[i].name).collect();
    let mut want: Vec<_> = pocket.iter().map(|f| f.face).collect();
    found.sort();
    want.sort();
    assert_eq!(found, want, "Create selection → Pocket");
    s.add(
        "Mirror",
        FeatureKind::Mirror(MirrorFeature {
            mirror_type: PatternType::Face,
            faces: pocket,
            plane: Some(MirrorPlane::Plane(PlaneRef::Right)),
            ..MirrorFeature::default()
        }),
    );
    let done = s.part();
    let m = done.mass.unwrap();
    close(v8 - m.volume, pocket_volume - rounds, tol);

    // Symmetric about the Z axis: the triangles 4-fold, the tapped holes at x = 0 either side
    // of y = 0, the pockets at ±x.
    close(m.center_of_mass.x, 0.0, 1e-6);
    close(m.center_of_mass.y, 0.0, 1e-6);

    // Step 10 (PS27.13): Mass properties with Aluminum - 1060 (2705 kg/m³), in kg.
    let props = s.d.element(s.el).unwrap().part_props().to_vec();
    let r = mass_report(&[&done], &props).unwrap();
    let mass = r.mass.expect("a material").mass;
    close(mass, m.volume * 2.705e-6, 1e-12);
    let mesh = done.solid.volume();
    assert!((m.volume - mesh).abs() < 6e-4 * m.volume, "OCCT {} vs mesh {mesh}", m.volume);
    println!(
        "reflector stand-in: V0 {v0:.3}, step drops {:.3} (closed {dv1:.3}), fillets +{added:.3}, pattern {:.3}, \
         hole {:.3}, tapped {:.3}, face pattern {:.3}, pocket {:.3}, rounds +{:.3}, mirror {:.3}; V {:.3} mm³ \
         (mesh {mesh:.3}), CoM ({:.3e}, {:.3e}, {:.4}), mass {:.4} kg",
        v0 - v1,
        v2 - v3,
        v3 - v4,
        v4 - v5,
        v5 - v6,
        v6 - v7,
        v8 - v7,
        v8 - m.volume,
        m.volume,
        m.center_of_mass.x,
        m.center_of_mass.y,
        m.center_of_mass.z,
        mass
    );
    close(mass, GOLDEN_MASS_KG, 5e-4);
}

/// The finished stand-in's mass (kg), recorded from OCCT and cross-checked against the mesh.
const GOLDEN_MASS_KG: f64 = 2.665_275;

#[test]
fn the_fixture_is_current() {
    // `fixtures/reflector_standin.cadrs` is the stand-in document as `rf::document()` builds
    // it. Regenerate it with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test reflector`.
    let doc = rf::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/reflector_standin.cadrs");
    let file = rf::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/reflector_standin.cadrs is out of date");
}

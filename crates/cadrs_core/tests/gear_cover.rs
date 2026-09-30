//! P3.6 / PS17: the Jackhammer Gear Cover exercise on the stand-in
//! ([`cadrs_core::samples::gear_cover`]), every course step through the document's commands, with
//! the volume checked against closed forms and the fillets against the mesh.
//!
//! The course's steps (intro-to-part-studios.md PS17, gap doc "What each exercise needs"):
//!
//! 1. Shell the bottom face, 4 mm.
//! 2. Sketch the 6 points on the top (here: on the bosses' top face; two concentric with the
//!    bosses' circular edges, four on the flat top, symmetric about the YZ plane).
//! 3. Hole: M5 Close counterbore Ø9.75 × 5, Through all, Start from part.
//! 4. Drag Shell 1 to the bottom.
//! 5. Chamfer 2 mm × 45° (Distance and angle, Offset) on the two bosses' top edges.
//! 6. Fillet R1 with overflow on the bottom rim (three edges picked, their tangent chain).
//! 7. Fillet Width 3 mm on the slope's edge.
//! 8. Mass properties in kg.
#![cfg(feature = "occt")]
#![allow(clippy::field_reassign_with_default)]

use std::f64::consts::PI;

use cadrs_core::applied::{
    ChamferFeature, ChamferType, EdgeOrFace, FilletFeature, FilletMeasurement, HoleFeature, HolePoint, ShellFeature,
};
use cadrs_core::commands::{AddFeature, AddSketch, EditSketch, MoveFeatures};
use cadrs_core::document::{Document, EdgeRef, FaceRef};
use cadrs_core::hole::{Fit, HoleEnd, HoleSpec, HoleStart, HoleStyle, HoleType};
use cadrs_core::parts::mass_report;
use cadrs_core::rebuild;
use cadrs_core::samples::gear_cover as gc;
use cadrs_core::{ElementId, Feature, FeatureId, History, Part};
use cadrs_sketch::SketchOp;

#[track_caller]
fn close(a: f64, b: f64, tol: f64) {
    assert!((a - b).abs() <= tol, "got {a}, expected {b} ± {tol}");
}

struct Studio {
    d: Document,
    h: History,
    el: ElementId,
}

impl Studio {
    fn new() -> Self {
        let d = gc::document().expect("the stand-in builds");
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

    fn add(&mut self, kind: AddFeature) -> FeatureId {
        let id = kind.feature;
        self.h.execute(&mut self.d, &kind).unwrap();
        id
    }
}

/// A face of the part whose plane faces `n` and contains `p`.
fn face_ref(part: &Part, n: [f64; 3], p: [f64; 3]) -> FaceRef {
    let s = &part.solid;
    let i = s
        .faces
        .iter()
        .enumerate()
        .position(|(i, f)| {
            f.plane.is_some_and(|pl| {
                let m = pl.normal();
                let len = (m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt();
                (m[0] * n[0] + m[1] * n[1] + m[2] * n[2]) / len > 0.999
            }) && s.face_contains(i, p)
        })
        .expect("a face there");
    FaceRef { part: part.id, face: s.faces[i].name, seed: p }
}

/// The edge of the part passing nearest `p`.
fn edge_ref(part: &Part, p: [f64; 3]) -> EdgeRef {
    let e = part
        .solid
        .edges
        .iter()
        .min_by(|a, b| a.distance(p).total_cmp(&b.distance(p)))
        .unwrap();
    assert!(e.distance(p) < 1e-3, "no edge at {p:?}");
    EdgeRef { part: part.id, edge: e.name, seed: p }
}

// ---------------------------------------------------------------------------------------------
// Closed forms (mm). The cover: W = 110 (x −55..55), L = 200 (y −200..0), H = 40, corners R 20
// (centres (±35, −180) and (±35, −20)); the slope z = 24 + 0.2(y + 200) for y < −120; bosses
// R 15, 6 high on the top at (±30, −40); holes M5 Close Ø5.3 (r_h 2.65) with a Ø9.75 (r_c
// 4.875) × 5 counterbore; shell t = 4 with the bottom open.

const T: f64 = 4.0;
const RH: f64 = 2.65;
const RC: f64 = 4.875;

/// ∫₀ˢ √(R² − u²) du.
fn g(r: f64, s: f64) -> f64 {
    (s * (r * r - s * s).max(0.0).sqrt() + r * r * (s / r).clamp(-1.0, 1.0).asin()) / 2.0
}

/// The body before any applied feature: the rounded prism, less the slope's wedge, plus the
/// bosses.
///   prism: H·(W·L − (4 − π)R²);
///   wedge: ∫ w(y)·(H − z_s(y)) dy for y ∈ [−200, −120], with H − z_s = 0.2(−120 − y). On
///     [−180, −120] w = 110: 110·0.2·60²/2 = 39 600. In the front corners (u = y + 180 ∈
///     [−20, 0], w = 2(35 + √(400 − u²)), H − z_s = 0.2(60 − u)):
///     0.4 [35·∫(60 − u) + 60·∫√(400 − u²) − ∫u√(400 − u²)] = 0.4 [35·1400 + 60·100π + 8000/3];
///   bosses: 2·π·15²·6.
fn body_volume() -> f64 {
    let prism = 40.0 * (110.0 * 200.0 - (4.0 - PI) * 400.0);
    let wedge = 39_600.0 + 0.4 * (35.0 * 1400.0 + 6000.0 * PI + 8000.0 / 3.0);
    prism - wedge + 2.0 * PI * 225.0 * 6.0
}

/// What the six through-all counterbored holes remove from the body: on the two bosses (from
/// their top at z 46) πr_c²·5 + πr_h²·41; the four on the flat top (from z 40) πr_c²·5 +
/// πr_h²·35.
fn holes_volume() -> f64 {
    2.0 * (PI * RC * RC * 5.0 + PI * RH * RH * 41.0) + 4.0 * (PI * RC * RC * 5.0 + PI * RH * RH * 35.0)
}

/// The shell's cavity when it comes after the holes (the course's order after step 4): the
/// points of the holed body at least t from its faces (the bottom is open). The cover is convex,
/// so away from the bosses and holes the cavity is the cover offset by t: plan x −51..51, y
/// −196..−4 with corners R 16 on the same centres, up to z 36 and under the slope moved t along
/// its normal (z = a + 0.2 u, u = y + 180, a = 28 − 4√1.04; it meets z 36 at
/// y* = −140 + 20√1.04):
///   front corners (u ∈ [−16, 0]): 2[35(16a − 25.6) + a·64π + 0.2·(−4096/3)];
///   [−180, y*]: 102 [a (y* + 180) + 0.1 (y* + 180)²];  [y*, −20]: 102·36·(−20 − y*);
///   back corners: 36·2(35·16 + 64π).
/// Round each hole and boss the cavity is a solid of revolution about its axis (Pappus): its
/// top z_max(r) from r = r_h + t (the wall round the hole) outwards, where a concave corner of
/// the body (a counterbore's floor meeting its wall, a boss meeting the top) rounds the cavity
/// with radius t, and a convex one leaves it sharp:
///   boss: r ∈ [6.65, 8.875] 41 − √(t² − (r − r_c)²); [8.875, 11] 42 (the boss top less t);
///     [11, 15] 40 − √(t² − (15 − r)²); replacing z 36 over r < 15;
///   flat hole: r ∈ [6.65, 8.875] 35 − √(t² − (r − r_c)²), replacing z 36 over r < 8.875.
fn cavity_volume() -> f64 {
    let a = 28.0 - 4.0 * 1.04f64.sqrt();
    let ys = -140.0 + 20.0 * 1.04f64.sqrt();
    let front = 2.0 * (35.0 * (16.0 * a - 25.6) + a * 64.0 * PI + 0.2 * (-4096.0 / 3.0));
    let slope = 102.0 * (a * (ys + 180.0) + 0.1 * (ys + 180.0).powi(2));
    let flat = 102.0 * 36.0 * (-20.0 - ys);
    let back = 36.0 * 2.0 * (35.0 * 16.0 + 64.0 * PI);
    let main = front + slope + flat + back;
    // ∫ 2π r √(t² − (r − c)²) dr over r ∈ [c + s0, c + s1] = 2π [−(t² − s²)^{3/2}/3 + c·g(t, s)].
    let ring = |c: f64, s0: f64, s1: f64| {
        let f = |s: f64| -(T * T - s * s).max(0.0).powf(1.5) / 3.0 + c * g(T, s);
        2.0 * PI * (f(s1) - f(s0))
    };
    let (r0, r1) = (RH + T, RC + T);
    let annulus = |z: f64, a: f64, b: f64| z * PI * (b * b - a * a);
    let boss = annulus(41.0, r0, r1) - ring(RC, r0 - RC, T) + annulus(42.0, r1, 11.0)
        // [11, 15]: 40·π(15² − 11²) − ∫ 2π r √(t² − (15 − r)²) dr, with r = 15 − s:
        // 2π ∫₀ᵗ (15 − s)√(t² − s²) ds = 2π (15·πt²/4 − t³/3).
        + annulus(40.0, 11.0, 15.0)
        - 2.0 * PI * (15.0 * PI * T * T / 4.0 - T * T * T / 3.0)
        - annulus(36.0, 0.0, 15.0);
    let hole = annulus(35.0, r0, r1) - ring(RC, r0 - RC, T) - annulus(36.0, 0.0, r1);
    main + 2.0 * boss + 4.0 * hole
}

/// A 2 mm × 45° chamfer on a boss's top edge (r 15) removes a 2 × 2 right triangle whose
/// centroid is 2/3 in from the edge: 2π(15 − 2/3)·2 each.
fn chamfer_volume() -> f64 {
    2.0 * 2.0 * PI * (15.0 - 2.0 / 3.0) * 2.0
}

#[test]
fn closed_forms_hold_on_their_own() {
    // The cavity's plain parts: with no holes or bosses the cavity would be the offset cover.
    assert!(cavity_volume() > 0.0 && body_volume() > cavity_volume());
    // A sanity figure for the report: the finished cover is about 2.1e5 mm³.
    let v = body_volume() - holes_volume() - cavity_volume() - chamfer_volume();
    assert!(v > 1.5e5 && v < 3.0e5, "{v}");
}

#[test]
fn gear_cover_course_steps() {
    let mut s = Studio::new();
    // The stand-in as it comes: one part, "Gear Cover", Aluminum - 380, its six features in the
    // closed "Base Features" folder.
    let el = s.d.element(s.el).unwrap();
    assert_eq!(el.folders().len(), 1);
    assert_eq!(el.folders()[0].name, "Base Features");
    assert_eq!(el.folders()[0].features.len(), 6);
    assert!(!el.folders()[0].open);
    close(s.volume(), body_volume(), 1e-4);
    let part = s.part();

    // Step 1 (PS17.3): Shell 1, the bottom face (Face of Extrude 1), 4 mm.
    let bottom = face_ref(&part, [0.0, 0.0, -1.0], [0.0, -100.0, 0.0]);
    assert_eq!(bottom.face.op, gc::EXTRUDE_1.0);
    let shell = s.add(AddFeature::shell(
        s.el,
        FeatureId::new(),
        ShellFeature { faces: vec![bottom], thickness: 4.0, thickness_expr: "4 mm".into(), ..ShellFeature::default() },
    ));
    let shelled = s.volume();
    assert!(shelled < body_volume() / 2.0, "{shelled}");

    // Step 2 (PS17.4): Sketch 4 on the bosses' top face: the two points concentric with the
    // bosses' edges, and four on the flat top, symmetric about the YZ plane.
    let part = s.part();
    let boss_top = face_ref(&part, [0.0, 0.0, 1.0], [gc::BOSS_X, gc::BOSS_Y, 46.0]);
    let plane = cadrs_core::parts::face_plane(&s.features(), gc::EXTRUDE_3, boss_top.face).expect("a planar face");
    let frame = plane.frame();
    let sketch = FeatureId::new();
    s.h.execute(&mut s.d, &AddSketch { element: s.el, feature: sketch, plane: Some(plane) }).unwrap();
    let pts = [
        [-gc::BOSS_X, gc::BOSS_Y],
        [gc::BOSS_X, gc::BOSS_Y],
        [-gc::HOLE_X, gc::HOLE_Y],
        [gc::HOLE_X, gc::HOLE_Y],
        [-gc::TOP_X, gc::TOP_Y],
        [gc::TOP_X, gc::TOP_Y],
    ];
    // The six points: the bosses' centres; (±34, −100) and (±16, −16) on the flat top (each
    // hole's neighbourhood, r ≤ r_c + t, stays clear of the walls' and the slope's offsets, so
    // the closed form's pieces don't meet).
    let ops: Vec<SketchOp> = pts
        .iter()
        .map(|p| SketchOp::AddPoint { pos: frame.to_sketch([p[0], p[1], 46.0]) })
        .collect();
    s.h.execute(&mut s.d, &EditSketch { element: s.el, feature: sketch, op: SketchOp::Batch(ops) }).unwrap();
    let g = s.d.element(s.el).unwrap().feature(sketch).unwrap().sketch().unwrap().geometry.clone();
    let points: Vec<HolePoint> = g.points.keys().map(|point| HolePoint { sketch, point }).collect();
    assert_eq!(points.len(), 6);

    // Step 3 (PS17.5): the hole, Metric, Counterbore, Clearance M5 Close → Ø5.3, Start from
    // part, Through all, counterbore Ø9.75 × 5. Its name is the callout (PS15.10).
    let mut spec = HoleSpec::default();
    spec.style = HoleStyle::Counterbore;
    spec.hole_type = HoleType::Clearance;
    spec.size = "M5".into();
    spec.fit = Fit::Close;
    spec.start = HoleStart::Part;
    spec.end = HoleEnd::ThroughAll;
    spec.apply_table();
    assert_eq!((spec.diameter.value, spec.cbore_diameter.value, spec.cbore_depth.value), (5.3, 9.75, 5.0));
    let hole = s.add(AddFeature::hole(
        s.el,
        FeatureId::new(),
        HoleFeature { points, merge_scope: vec![gc::PART], spec, ..HoleFeature::default() },
    ));
    let hole_name = s.d.element(s.el).unwrap().feature(hole).unwrap().name.clone();
    assert_eq!(hole_name, "Ø 5.3 mm THRU | ⌴Ø 9.75 mm ↧ 5 mm");
    // With the shell first, the counterbores are cut through 4 mm walls ("most holes look like
    // plain holes"): less is removed than from the solid body.
    let shell_first = s.volume();
    assert!(shell_first < shelled);

    // Step 4 (PS17.6): drag Shell 1 to the bottom of the list: now the shell wraps the holes.
    let n = s.features().len();
    s.h.execute(
        &mut s.d,
        &MoveFeatures { element: s.el, features: vec![shell], to: n - 1, folder: None, label: "Reorder Shell 1".into() },
    )
    .unwrap();
    assert_eq!(s.features().last().unwrap().id, shell);
    // The closed form: the holed body less the cavity.
    let v_shell = body_volume() - holes_volume() - cavity_volume();
    close(s.volume(), v_shell, 1e-3);

    // Step 5 (PS17.7): Chamfer 2 mm × 45°, Distance and angle, Offset, on the two bosses' top
    // edges (tangent propagation on: a circle is its own chain).
    let part = s.part();
    let e1 = edge_ref(&part, [-gc::BOSS_X + 15.0, gc::BOSS_Y, 46.0]);
    let e2 = edge_ref(&part, [gc::BOSS_X + 15.0, gc::BOSS_Y, 46.0]);
    s.add(AddFeature::chamfer(
        s.el,
        FeatureId::new(),
        ChamferFeature {
            entities: vec![EdgeOrFace::Edge(e1), EdgeOrFace::Edge(e2)],
            kind: ChamferType::DistanceAngle,
            distance: 2.0,
            distance_expr: "2 mm".into(),
            angle: 45.0,
            angle_expr: "45 deg".into(),
            ..ChamferFeature::default()
        },
    ));
    // Before the fillets: the closed form.
    let v_before_fillets = v_shell - chamfer_volume();
    let occt_before_fillets = s.volume();
    close(occt_before_fillets, v_before_fillets, 1e-3);
    let before = s.part();
    // CoM X = 0 by symmetry.
    close(before.mass.unwrap().center_of_mass.x, 0.0, 1e-6);

    // Step 6 (PS17.8): Fillet R1, overflow on, three bottom rim edges (their tangent chain is
    // the whole outer rim).
    let rim = [[0.0, -200.0, 0.0], [55.0, -100.0, 0.0], [-55.0, -100.0, 0.0]];
    let entities = rim.iter().map(|p| EdgeOrFace::Edge(edge_ref(&before, *p))).collect();
    s.add(AddFeature::fillet(
        s.el,
        FeatureId::new(),
        FilletFeature { entities, size: 1.0, size_expr: "1 mm".into(), ..FilletFeature::default() },
    ));
    let v6 = s.volume();
    // Removed along the rim's length: (1 − π/4)·1² per mm (all the rim's corners are 90°), the
    // rim being the rounded rectangle's perimeter 2(110 + 200) − (8 − 2π)·20.
    let perimeter = 2.0 * (110.0 + 200.0) - (8.0 - 2.0 * PI) * 20.0;
    close(v_before_fillets - v6, (1.0 - PI / 4.0) * perimeter, 0.5);

    // Step 7 (PS17.9): Fillet 3 on the slope's edge (Edge of Extrude 2): Width 3 mm.
    let part = s.part();
    let slope_edge = edge_ref(&part, [55.0, -140.0, 24.0 + 0.2 * 60.0]);
    assert!(slope_edge.edge.faces.iter().any(|f| f.op == gc::EXTRUDE_2.0), "an edge of Extrude 2");
    s.add(AddFeature::fillet(
        s.el,
        FeatureId::new(),
        FilletFeature {
            entities: vec![EdgeOrFace::Edge(slope_edge)],
            measurement: FilletMeasurement::Width,
            size: 3.0,
            size_expr: "3 mm".into(),
            ..FilletFeature::default()
        },
    ));
    let done = s.part();
    let m = done.mass.unwrap();
    // The Width fillet's removal, bounded analytically. Its tangent chain is the slope's whole
    // rim: the two side edges (faces 90° apart), the front edge (78.69°: cos φ = 0.2/√1.04) and
    // the two R20 corners between (φ between those). Between flat faces whose normals are φ
    // apart, width w gives r = w/(2 sin(φ/2)) and removes A(φ) = r²(tan(φ/2) − φ/2) per mm.
    let removed = v6 - m.volume;
    let area = |phi: f64| {
        let r = 3.0 / (2.0 * (phi / 2.0).sin());
        (r * r * ((phi / 2.0).tan() - phi / 2.0), r)
    };
    let (a90, _) = area(PI / 2.0);
    let (a_front, r_front) = area((0.2 / 1.04f64.sqrt()).acos());
    let side = 60.0 * 1.04f64.sqrt();
    let front = 2.0 * (gc::HALF_WIDTH - gc::CORNER_R);
    // A corner: x = 35 + 20 cos a, y = −180 + 20 sin a, z = 24 + 0.2(y + 200), a in [−π/2, 0]:
    // |d/da| = √(400 + 16 cos²a) (Simpson).
    let n = 1000;
    let corner = (0..=n)
        .map(|i| {
            let a = -PI / 2.0 + PI / 2.0 * i as f64 / n as f64;
            let wgt = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
            wgt * (400.0 + 16.0 * a.cos().powi(2)).sqrt()
        })
        .sum::<f64>()
        * (PI / 2.0 / n as f64)
        / 3.0;
    // Round a convex corner of radius R the section's centroid (within r of the edge) runs
    // on a shorter path: at least (1 − r/R) of the edge (Pappus). The two ends, where the
    // fillet stops at the slope's crease, change it by at most A·r each.
    let straight = 2.0 * side * a90 + front * a_front;
    let low = straight + 2.0 * corner * a_front * (1.0 - r_front / gc::CORNER_R) - 2.0 * a90 * r_front;
    let high = straight + 2.0 * corner * a90 + 2.0 * a90 * r_front;
    assert!(low < removed && removed < high, "the Width fillet removed {removed}, expected {low}..{high}");
    // And OCCT's exact volume against the display mesh's (0.05 mm deflection): the mesh's
    // chords cut into the curved faces, so it is a little smaller, by under 1.5e-4.
    let mesh = done.solid.volume();
    assert!((m.volume - mesh).abs() < 1.5e-4 * m.volume, "OCCT {} vs mesh {mesh}", m.volume);
    close(m.center_of_mass.x, 0.0, 1e-6);

    // Step 8 (PS17.10): Mass properties in kg (Aluminum - 380, 2760 kg/m³).
    let props = s.d.element(s.el).unwrap().part_props().to_vec();
    let r = mass_report(&[&done], &props).unwrap();
    let mass = r.mass.expect("a material").mass;
    close(mass, m.volume * 2.76e-6, 1e-12);
    println!(
        "gear cover stand-in: V before fillets {:.3} mm³ (closed form {:.3}), after {:.3} mm³ (mesh {:.3}), \
         width fillet removed {removed:.3} ({low:.3}..{high:.3}), mass {:.4} kg",
        occt_before_fillets, v_before_fillets, m.volume, mesh, mass
    );
    // The golden value, recorded once (OCCT 7.8.1): the finished stand-in.
    close(mass, GOLDEN_MASS_KG, 5e-4);
}

/// The finished stand-in's mass (kg), recorded from OCCT and cross-checked against the mesh.
const GOLDEN_MASS_KG: f64 = 0.521_059;

#[test]
fn the_fixture_is_current() {
    // `fixtures/gear_cover_standin.cadrs` is the stand-in document as `gc::document()` builds
    // it. Regenerate it with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test gear_cover`.
    let doc = gc::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gear_cover_standin.cadrs");
    let file = gc::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/gear_cover_standin.cadrs is out of date");
}

#[test]
fn a_feature_dragged_above_its_parent_fails() {
    // PS11.3: Extrude 1 dragged above Sketch 1 fails (its sketch is below it), and the parts
    // after it keep going without it; dragged back, it builds again.
    let mut s = Studio::new();
    let features = s.features();
    let i = features.iter().position(|f| f.id == gc::EXTRUDE_1).unwrap();
    assert_eq!(i, 1);
    s.h.execute(
        &mut s.d,
        &MoveFeatures { element: s.el, features: vec![gc::EXTRUDE_1], to: 0, folder: Some(gc::FOLDER), label: "Reorder".into() },
    )
    .unwrap();
    let b = rebuild::build(&s.features());
    let why = b.error(gc::EXTRUDE_1).expect("Extrude 1 fails");
    assert!(why.contains("Sketch 1 is below this feature"), "{why}");
    // The folder still holds its six features, in their new order.
    assert_eq!(s.d.element(s.el).unwrap().folders()[0].features[0], gc::EXTRUDE_1);
    s.h.undo(&mut s.d).unwrap();
    assert!(rebuild::build(&s.features()).errors.is_empty());
}

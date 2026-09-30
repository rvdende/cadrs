//! "Introduction to Onshape Assemblies" self-checks (P3B.1+), on the stand-in fixtures of
//! `reference/onshape/training/intro-to-assemblies-gaps.md`. Each compares the assembly's mass
//! properties, in assembly coordinates after the exercise's placements, with closed forms to
//! 1e-4 (relative).

use std::sync::Arc;

use cadrs_core::assembly::commands::{DeleteInstances, InsertInstance, MoveInstances, SetInstancesFixed, SetInstancesHidden};
use cadrs_core::assembly::{self, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::commands::{AddElement, NewElementKind, SetExtrude};
use cadrs_core::rebuild::Build;
use cadrs_core::samples::motor_mount as mm;
use cadrs_core::{Document, ElementId, History};

/// mm per inch, kg per lb, and Aluminum 6061's density (kg/mm³).
const IN: f64 = 25.4;
const LB: f64 = 0.453_592_37;
const RHO: f64 = 2700.0e-9;

fn close(what: &str, got: f64, want: f64, rel: f64) {
    let tol = rel * want.abs().max(1e-9);
    assert!((got - want).abs() <= tol, "{what}: got {got}, want {want} (±{tol})");
}

/// The fixture document with an assembly "Mount Assembly".
fn setup() -> (Document, History, ElementId) {
    let mut doc = mm::document().unwrap();
    let mut h = History::default();
    let asm = ElementId::from_u128(0x3b01_0000_0000_0000_0000_0000_0000_0201);
    h.execute(&mut doc, &AddElement { id: asm, kind: NewElementKind::Assembly, name: Some("Mount Assembly".into()), after: None })
        .unwrap();
    (doc, h, asm)
}

fn studio_build(doc: &Document) -> Arc<Build> {
    cadrs_core::rebuild::build(doc.element(mm::STUDIO).unwrap().features())
}

fn source() -> InstanceSource {
    InstanceSource::Part { element: mm::STUDIO, part: mm::PART }
}

fn insert(doc: &mut Document, h: &mut History, asm: ElementId, id: InstanceId, pose: Pose) {
    h.execute(doc, &InsertInstance { element: asm, instance: Instance::new(id, source(), pose) }).unwrap();
}

fn parts_of(doc: &Document, asm: ElementId) -> (Vec<cadrs_core::Part>, Vec<cadrs_core::PartProps>) {
    let build = studio_build(doc);
    assembly::instance_parts(doc, doc.element(asm).unwrap().assembly_model().unwrap(), |e| {
        (e == mm::STUDIO).then(|| build.clone())
    })
}

/// The bracket's closed form, in inches (unit density): volume, centre of mass and the inertia
/// tensor about the centre (in⁵).
///
/// Three pieces: the base box 3 × 3 × 0.5 (centre (0, 1.5, 0.25)), the upright box 3 × 0.5 × 3
/// (centre (0, 2.75, 2.0)) and, taken away, the hole: a cylinder r = 0.2815, h = 0.5 along Z
/// (centre (0, 1.75, 0.25)).
///
/// - V = 4.5 + 4.5 − π r² h = 9 − 0.124473 = 8.875527 in³.
/// - C = Σ Vᵢ cᵢ / V (the hole counts negative): y = (6.75 + 12.375 − 0.217827) / 8.875527
///   = 2.130259, z = (1.125 + 9 − 0.031118) / 8.875527 = 1.137271, x = 0 by symmetry.
/// - A box a × b × c about its centre: Ixx = V (b² + c²) / 12, …; the cylinder: Ixx = Iyy =
///   V (3r² + h²) / 12, Izz = V r² / 2. Each moved to C by the parallel-axis theorem
///   `I_C = I_c + V (|d|² E − d dᵀ)`, d = cᵢ − C (the hole's with −V). The pieces are symmetric
///   about x = 0, so Ixy = Ixz = 0; Iyz = −Σ Vᵢ dᵢy dᵢz.
fn bracket_closed_form() -> (f64, [f64; 3], [[f64; 3]; 3]) {
    let r = mm::HOLE_D / 2.0;
    let hole = std::f64::consts::PI * r * r * mm::BASE;
    // (volume, centre, own inertia diagonal)
    let boxes = |a: f64, b: f64, c: f64| {
        let v = a * b * c;
        (v, [v * (b * b + c * c) / 12.0, v * (a * a + c * c) / 12.0, v * (a * a + b * b) / 12.0])
    };
    let (v1, i1) = boxes(3.0, 3.0, 0.5);
    let (v2, i2) = boxes(3.0, 0.5, 3.0);
    let ic = [hole * (3.0 * r * r + 0.25) / 12.0, hole * (3.0 * r * r + 0.25) / 12.0, hole * r * r / 2.0];
    let pieces = [
        (v1, [0.0, 1.5, 0.25], i1),
        (v2, [0.0, 2.75, 2.0], i2),
        (-hole, [0.0, 1.75, 0.25], ic.map(|x| -x)),
    ];
    let v: f64 = pieces.iter().map(|p| p.0).sum();
    let mut c = [0.0; 3];
    for p in &pieces {
        for (k, ck) in c.iter_mut().enumerate() {
            *ck += p.0 * p.1[k] / v;
        }
    }
    let mut t = [[0.0; 3]; 3];
    for (vi, ci, own) in &pieces {
        let d = [ci[0] - c[0], ci[1] - c[1], ci[2] - c[2]];
        let dd = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
        for i in 0..3 {
            for j in 0..3 {
                let e = if i == j { own[i] + vi * dd } else { 0.0 };
                t[i][j] += e - vi * d[i] * d[j];
            }
        }
    }
    (v, c, t)
}

#[test]
fn the_studio_matches_the_closed_form() {
    let doc = mm::document().unwrap();
    let build = studio_build(&doc);
    assert!(build.errors.is_empty(), "{:?}", build.errors);
    assert_eq!(build.parts.len(), 1);
    let m = build.parts[0].mass.expect("kernel mass properties");
    let (v, c, _) = bracket_closed_form();
    close("volume (in³)", m.volume / IN.powi(3), v, 1e-4);
    close("recorded volume", v, 8.87553, 1e-6);
    close("CoM y", m.center_of_mass.y / IN, c[1], 1e-4);
    close("CoM z", m.center_of_mass.z / IN, c[2], 1e-4);
    close("recorded CoM y", c[1], 2.13026, 2e-6);
    close("recorded CoM z", c[2], 1.13727, 2e-6);
    assert!(m.center_of_mass.x.abs() < 1e-6);
}

/// Ex1 (A5.3–A5.10): the bracket inserted off the origin, the triad dragged to the hole's
/// underside centre (0, 1.75, 0) in, **Move to origin**, then **Anti-align with Z** (the triad's
/// Z is the hole axis pointing into the part, +Z; the half turn goes about the triad's X,
/// which is the model X), then Fixed. In assembly coordinates:
///
/// - Move to origin: p ↦ p − (0, 1.75, 0) + … the insert's offset cancels: the hole centre goes
///   to the origin, so C = (0, 2.130259 − 1.75, 1.137271) = (0, 0.380259, 1.137271).
/// - Anti-align: a half turn about X, (x, y, z) ↦ (x, −y, −z): **C = (0, −0.380259,
///   −1.137271) in**.
/// - Mass = ρ V = 2.70 g/cm³ × 16.387064 cm³/in³ × 8.875527 in³ = 392.699 g = **0.865757 lb**;
///   V = **8.875527 in³**. The inertia keeps its diagonal (R = diag(1, −1, −1) keeps Iyz and
///   flips the zero Ixy, Ixz).
#[test]
fn ex1_start_an_assembly_mass_properties() {
    let (mut doc, mut h, asm) = setup();
    let id = InstanceId::from_u128(0x0003_b011);
    // Placed by a click away from the origin.
    let placed = Pose::translation([150.0, -120.0, 0.0]);
    insert(&mut doc, &mut h, asm, id, placed);
    // The triad on the hole's underside centre, in assembly coordinates.
    let hole_centre = placed.apply([0.0, mm::HOLE_Y * IN, 0.0]);
    let pose = assembly::moved_to_origin(&placed, hole_centre);
    // The triad's Z (the hole's axis, into the part) and X, as the instance carries them.
    let (z, x) = (pose.rotate([0.0, 0.0, 1.0]), pose.rotate([1.0, 0.0, 0.0]));
    let pose = assembly::aligned_with_z(&pose, [0.0; 3], z, x, true);
    assert_eq!(pose.rotation, [[1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
    h.execute(&mut doc, &MoveInstances { element: asm, poses: vec![(id, pose)], label: "Anti-align with Z".into() }).unwrap();
    h.execute(&mut doc, &SetInstancesFixed { element: asm, instances: vec![id], fixed: true }).unwrap();

    let (parts, props) = parts_of(&doc, asm);
    assert_eq!(parts[0].name, "DC Motor Mount (stand-in) <1>");
    let r = assembly::mass_report(&parts, &props, &[id]).unwrap();
    let m = r.mass.expect("a material");
    let (v, c, t) = bracket_closed_form();
    let want_c = [0.0, -(c[1] - mm::HOLE_Y), -c[2]];
    close("V (in³)", r.volume / IN.powi(3), v, 1e-4);
    close("mass (lb)", m.mass / LB, RHO * IN.powi(3) * v / LB, 1e-4);
    close("recorded mass", RHO * IN.powi(3) * v / LB, 0.86575, 1e-5);
    assert!(m.center_of_mass.x.abs() < 1e-6);
    close("CoM y (in)", m.center_of_mass.y / IN, want_c[1], 1e-4);
    close("CoM z (in)", m.center_of_mass.z / IN, want_c[2], 1e-4);
    close("recorded CoM y", want_c[1], -0.38026, 1e-5);
    close("recorded CoM z", want_c[2], -1.13727, 1e-5);
    // Inertia (kg·mm²): the closed form × ρ × (mm/in)⁵, turned by diag(1, −1, −1).
    let k = RHO * IN.powi(5);
    let sign = [[1.0, -1.0, -1.0], [-1.0, 1.0, 1.0], [-1.0, 1.0, 1.0]];
    for (i, row) in t.iter().enumerate() {
        close(&format!("I{i}{i}"), m.inertia[(i, i)], row[i] * k, 1e-4);
    }
    close("Iyz", m.inertia[(1, 2)], t[1][2] * k * sign[1][2], 1e-4);
    assert!(m.inertia[(0, 1)].abs() < 1e-6 * k && m.inertia[(0, 2)].abs() < 1e-6 * k);
}

#[test]
fn anti_align_about_the_triad_y_gives_the_mirror_y() {
    // If the triad's X ran along the model Y, the half turn is about Y: CoM y is +0.380.
    let (mut doc, mut h, asm) = setup();
    let id = InstanceId::from_u128(0x0003_b012);
    insert(&mut doc, &mut h, asm, id, Pose::IDENTITY);
    let pose = assembly::moved_to_origin(&Pose::IDENTITY, [0.0, mm::HOLE_Y * IN, 0.0]);
    let pose = assembly::aligned_with_z(&pose, [0.0; 3], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0], true);
    assert_eq!(pose.rotation, [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, -1.0]]);
    h.execute(&mut doc, &MoveInstances { element: asm, poses: vec![(id, pose)], label: "Anti-align with Z".into() }).unwrap();
    let (parts, props) = parts_of(&doc, asm);
    let m = assembly::mass_report(&parts, &props, &[id]).unwrap().mass.unwrap();
    let (_, c, _) = bracket_closed_form();
    close("CoM y (in)", m.center_of_mass.y / IN, c[1] - mm::HOLE_Y, 1e-4);
    close("CoM z (in)", m.center_of_mass.z / IN, -c[2], 1e-4);
}

#[test]
fn placement_composition() {
    // Align with Z on an axis that is already +Z does nothing; a tilted axis turns onto Z about
    // the triad origin (which stays put).
    let p = Pose::translation([10.0, 20.0, 30.0]);
    assert_eq!(assembly::aligned_with_z(&p, [1.0, 2.0, 3.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0], false), p);
    let at = [5.0, 5.0, 5.0];
    let q = assembly::aligned_with_z(&Pose::IDENTITY, at, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], false);
    let x = q.rotate([1.0, 0.0, 0.0]);
    assert!((x[2] - 1.0).abs() < 1e-12 && x[0].abs() < 1e-12, "{x:?}");
    let o = q.apply(at);
    assert!((0..3).all(|i| (o[i] - at[i]).abs() < 1e-9), "the triad origin moved: {o:?}");
    // Rotate 90° twice is 180°; four times is the identity.
    let r90 = |p: &Pose| assembly::rotated(p, at, [0.0, 0.0, 1.0], std::f64::consts::FRAC_PI_2);
    let twice = r90(&r90(&Pose::IDENTITY));
    let half = assembly::rotated(&Pose::IDENTITY, at, [0.0, 0.0, 1.0], std::f64::consts::PI);
    assert_eq!(twice.rotation, half.rotation);
    let full = r90(&r90(&twice));
    assert_eq!(full.rotation, Pose::IDENTITY.rotation);
    assert!(full.translation.iter().all(|t| t.abs() < 1e-9));
    // Move to origin puts the triad origin on the origin, whatever the rotation.
    let m = assembly::moved_to_origin(&half, half.apply([1.0, 2.0, 3.0]));
    let o = m.apply([1.0, 2.0, 3.0]);
    assert!(o.iter().all(|t| t.abs() < 1e-9));
}

#[test]
fn undo_removes_inserts_one_by_one() {
    let (mut doc, mut h, asm) = setup();
    let ids = [InstanceId::from_u128(1), InstanceId::from_u128(2), InstanceId::from_u128(3)];
    for (k, id) in ids.iter().enumerate() {
        insert(&mut doc, &mut h, asm, *id, Pose::translation([k as f64 * 100.0, 0.0, 0.0]));
    }
    let names = |doc: &Document| -> Vec<String> { parts_of(doc, asm).0.iter().map(|p| p.name.clone()).collect() };
    assert_eq!(
        names(&doc),
        ["DC Motor Mount (stand-in) <1>", "DC Motor Mount (stand-in) <2>", "DC Motor Mount (stand-in) <3>"]
    );
    for left in [2, 1, 0] {
        h.undo(&mut doc).unwrap();
        assert_eq!(doc.element(asm).unwrap().assembly.instances.len(), left);
    }
    h.redo(&mut doc).unwrap();
    assert_eq!(names(&doc), ["DC Motor Mount (stand-in) <1>"]);
    // Hide, show, delete: each one step.
    h.execute(&mut doc, &SetInstancesHidden { element: asm, instances: vec![ids[0]], hidden: true }).unwrap();
    assert!(parts_of(&doc, asm).1[0].hidden);
    h.execute(&mut doc, &DeleteInstances { element: asm, instances: vec![ids[0]] }).unwrap();
    assert!(doc.element(asm).unwrap().assembly.is_empty());
    h.undo(&mut doc).unwrap();
    h.undo(&mut doc).unwrap();
    assert!(!parts_of(&doc, asm).1[0].hidden);
}

#[test]
fn a_studio_edit_updates_the_instances() {
    // X1: instances stay live. Extrude 2 (the upright) 3 in → 2 in: the instance loses
    // 3 × 0.5 × 1 in³.
    let (mut doc, mut h, asm) = setup();
    let id = InstanceId::from_u128(7);
    insert(&mut doc, &mut h, asm, id, Pose::translation([0.0, 0.0, 50.0]));
    let v0 = parts_of(&doc, asm).0[0].mass.unwrap().volume;
    let mut e = doc.element(mm::STUDIO).unwrap().feature(mm::EXTRUDE_2).unwrap().extrude().unwrap().clone();
    e.depth = 2.0 * IN;
    e.depth_expr = "2 in".into();
    h.execute(&mut doc, &SetExtrude { element: mm::STUDIO, feature: mm::EXTRUDE_2, extrude: e, label: "Extrude".into() })
        .unwrap();
    let (parts, _) = parts_of(&doc, asm);
    let v1 = parts[0].mass.unwrap().volume;
    close("volume change (in³)", (v0 - v1) / IN.powi(3), 1.5, 1e-4);
    // The mesh moved with the instance: its lowest point is at z = 50 mm.
    let zmin = parts[0].solid.positions.iter().map(|p| p[2]).fold(f64::MAX, f64::min);
    close("lowest z", zmin, 50.0, 1e-9);
}

#[test]
fn the_fixture_is_current() {
    // `fixtures/motor_mount_standin.cadrs` is the stand-in document as `mm::document()` builds
    // it. Regenerate it with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test
    // course_assemblies`.
    let doc = mm::document().unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/motor_mount_standin.cadrs");
    let file = mm::file(doc.clone());
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()).unwrap();
    if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
        std::fs::write(&path, &text).unwrap();
    }
    let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
    assert_eq!(stored.document, doc, "fixtures/motor_mount_standin.cadrs is out of date");
}

#[test]
fn assemblies_round_trip_through_the_store_format() {
    let (mut doc, mut h, asm) = setup();
    insert(&mut doc, &mut h, asm, InstanceId::from_u128(9), Pose::rotation_about([1.0, 2.0, 3.0], [0.0, 1.0, 0.0], 0.5));
    h.execute(&mut doc, &SetInstancesFixed { element: asm, instances: vec![InstanceId::from_u128(9)], fixed: true }).unwrap();
    let text = ron::ser::to_string_pretty(&mm::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
    let back: cadrs_core::DocumentFile = ron::from_str(&text).unwrap();
    assert_eq!(back.document, doc);
}

// ---------------------------------------------------------------------------------------------
// Ex4 "Explicit Mate Connectors" (P3B.7, A24) on `fixtures/step_stool_standin.cadrs`.

mod ex4 {
    use std::collections::HashMap;
    use std::sync::Arc;

    use cadrs_core::assembly::mate::Dof;
    use cadrs_core::assembly::solver::{Drive, SolveOptions};
    use cadrs_core::assembly::{self, InstanceId};
    use cadrs_core::samples::step_stool as ss;
    use cadrs_core::{Document, ElementId, History};

    use super::{IN, RHO, close};

    /// g per kg.
    const G: f64 = 1000.0;

    /// The closed form at hinge angle `deg` (a turn about +X through the hinge axis (y −0.5,
    /// z 23), negative towards −Y), in inches: volume (in³) and centre of mass.
    ///
    /// Boxes (volume, centre): Large Frame Bar 12 × 1 × 24 = 288 at (0, 0.5, 12), fixed. The
    /// base frame: plate 10 × 0.5 × 22 = 110 at (0, −1.25, 11); lugs 0.5 × 1.5 × 2 = 1.5 each
    /// at (∓4.75, −0.75, 23), each less a hinge hole π·0.25²·0.5 = 0.0981748 at (∓4.75, −0.5,
    /// 23); Cross Bar 9 × 1 × 1 = 9 at (0, −2, 4.5). V = 288 + 110 + 3 − 0.1963495 + 9 =
    /// 409.8036505 in³. The base frame's pieces turn about the axis: (y, z) ↦ (−0.5 + dy cos θ −
    /// dz sin θ, 23 + dy sin θ + dz cos θ) with (dy, dz) = (y + 0.5, z − 23). Mass = ρ V = 2.70
    /// g/cm³ × 16.387064 cm³/in³ × V = 18 131.792 g. CoM = Σ Vᵢ cᵢ / V: at 0° (0, −0.0333131,
    /// 11.6421217); at −42.5° (0, −2.4218873, 12.7546071).
    fn closed_form(deg: f64) -> (f64, [f64; 3]) {
        let hole = std::f64::consts::PI * 0.25 * 0.25 * 0.5;
        let fixed = [(288.0, [0.0, 0.5, 12.0])];
        let moving = [
            (110.0, [0.0, -1.25, 11.0]),
            (1.5, [-4.75, -0.75, 23.0]),
            (1.5, [4.75, -0.75, 23.0]),
            (-hole, [-4.75, -0.5, 23.0]),
            (-hole, [4.75, -0.5, 23.0]),
            (9.0, [0.0, -2.0, 4.5]),
        ];
        let (s, c) = deg.to_radians().sin_cos();
        let turn = |p: [f64; 3]| {
            let (dy, dz) = (p[1] + 0.5, p[2] - 23.0);
            [p[0], -0.5 + dy * c - dz * s, 23.0 + dy * s + dz * c]
        };
        let all: Vec<(f64, [f64; 3])> = fixed.into_iter().chain(moving.into_iter().map(|(v, p)| (v, turn(p)))).collect();
        let v: f64 = all.iter().map(|x| x.0).sum();
        let com = [0, 1, 2].map(|k| all.iter().map(|x| x.0 * x.1[k]).sum::<f64>() / v);
        (v, com)
    }

    fn builds(doc: &Document) -> HashMap<ElementId, Arc<cadrs_core::rebuild::Build>> {
        [ss::LARGE_STUDIO, ss::BASE_STUDIO].into_iter().map(|e| (e, cadrs_core::rebuild::build(doc.element(e).unwrap().features()))).collect()
    }

    /// The whole assembly's mass (g), volume (in³) and CoM (in).
    fn report(doc: &Document) -> (f64, f64, [f64; 3]) {
        let b = builds(doc);
        let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
        let (parts, props) = assembly::instance_parts(doc, asm, |e| b.get(&e).cloned());
        let ids: Vec<InstanceId> = asm.instances.iter().map(|i| i.id).collect();
        let r = assembly::mass_report(&parts, &props, &ids).unwrap();
        let m = r.mass.unwrap();
        let c = m.center_of_mass;
        (m.mass * G, r.volume / IN.powi(3), [c.x / IN, c.y / IN, c.z / IN])
    }

    #[test]
    fn the_closed_form_is_the_gap_lists() {
        let (v, c) = closed_form(0.0);
        close("V", v, 409.80365, 1e-7);
        close("mass g", v * IN.powi(3) * RHO * G, 18_131.792, 1e-7);
        close("CoM y", c[1], -0.0333131, 1e-5);
        close("CoM z", c[2], 11.6421217, 1e-7);
        let (_, c) = closed_form(ss::OPEN_ANGLE);
        close("CoM y open", c[1], -2.4218873, 1e-7);
        close("CoM z open", c[2], 12.7546071, 1e-7);
    }

    #[test]
    fn ex4_step_stool_mass_properties_at_both_limits() {
        // A24.4–A24.12: the two explicit connectors, Revolute 1 with limits −42.5°…0°, Reset
        // (0°), then the opening limit.
        let mut doc = ss::document().unwrap();
        let mut h = History::default();
        ss::add_connectors_and_hinge(&mut doc, &mut h).unwrap();
        let (mass, v, c) = report(&doc);
        let (v0, c0) = closed_form(0.0);
        close("V", v, v0, 1e-4);
        close("mass", mass, v0 * IN.powi(3) * RHO * G, 1e-4);
        assert!(c[0].abs() < 1e-6, "CoM x {}", c[0]);
        close("CoM y", c[1], c0[1], 1e-4);
        close("CoM z", c[2], c0[2], 1e-4);
        // The panel's rounding (3 decimals): 18 131.792 g, (0, −0.033, 11.642) in.
        assert_eq!(format!("{mass:.3} {:.3} {:.3}", c[1], c[2]), "18131.792 -0.033 11.642");
        // Apply limit position: the hinge at −42.5°.
        let solids = ss::solids(&doc);
        let model = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap().clone();
        let drive = Drive { mate: ss::HINGE, dof: Dof::Angle, value: ss::OPEN_ANGLE.to_radians() };
        let sol = assembly::solve(&model, &solids, &SolveOptions { movers: vec![ss::BASE], snap: Some(ss::HINGE), drives: vec![drive], ..Default::default() });
        assert!(sol.converged, "{}", sol.residual);
        let poses = sol.changed(&model);
        h.execute(&mut doc, &cadrs_core::assembly::commands::MoveInstances { element: ss::ASSEMBLY, poses, label: "Apply limit".into() }).unwrap();
        let (mass1, _, c) = report(&doc);
        let (_, c1) = closed_form(ss::OPEN_ANGLE);
        close("mass open", mass1, mass, 1e-9);
        assert!(c[0].abs() < 1e-6, "CoM x {}", c[0]);
        close("CoM y open", c[1], c1[1], 1e-4);
        close("CoM z open", c[2], c1[2], 1e-4);
        assert_eq!(format!("{:.3} {:.3}", c[1], c[2]), "-2.422 12.755");
    }

    #[test]
    fn the_step_stool_fixture_is_current() {
        // Regenerate with `CADRS_WRITE_FIXTURES=1 cargo test -p cadrs_core --test course_assemblies`.
        let doc = ss::document().unwrap();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/step_stool_standin.cadrs");
        let text = ron::ser::to_string_pretty(&ss::file(doc.clone()), ron::ser::PrettyConfig::default()).unwrap();
        if std::env::var("CADRS_WRITE_FIXTURES").is_ok() {
            std::fs::write(&path, &text).unwrap();
        }
        let b = builds(&doc);
        assert_eq!(b[&ss::BASE_STUDIO].parts.len(), 3, "Base Frame Bar, Cross Bar, Back Foot");
        assert_eq!(b[&ss::LARGE_STUDIO].parts.len(), 1);
        let stored = cadrs_core::Store::load_path(&path).expect("the fixture loads");
        assert_eq!(stored.document, doc, "fixtures/step_stool_standin.cadrs is out of date");
    }
}

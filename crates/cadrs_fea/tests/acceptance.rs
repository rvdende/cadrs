//! P3F.5 acceptance (`intro-to-parametric-cad-gaps.md`, "P3.5 simulation"): a steel cantilever
//! against Euler–Bernoulli beam theory, the same beam in two bonded halves, and the solve time.
//!
//! **The beam** (the gap list's): 100 × 10 × 10 mm along x, steel E = 200 GPa = 200 000 MPa,
//! ν = 0.3, the end x = 0 fixed, 100 N down (−z) spread over the end x = 100.
//! - Second moment of area of the 10 × 10 section: I = b·h³/12 = 10·10³/12 = 833.33 mm⁴.
//! - Tip deflection: δ = P·L³/(3·E·I) = 100·100³/(3·200 000·833.33) = 10⁸/(5·10⁸) = **0.200 mm**.
//!   (Timoshenko's shear term adds P·L/(κ·G·A) = 100·100/((5/6)·76 923·100) = 0.0016 mm, 0.8 %;
//!   the clamped end's Poisson restraint takes a little off: both well inside 3 %.)
//! - Mid-span bending stress at the top fibre: M = P·(L − x) = 100·50 = 5 000 N·mm,
//!   σ = M·c/I = 5 000·5/833.33 = **30.0 MPa** (tension on top: the beam hangs down).

use cadrs_fea::{Body, Bond, Load, LoadKind, Material, Model, Options, Solution, Surface, solve};

const STEEL: Material = Material { youngs: 200_000.0, poisson: 0.3 };
// Surface::cuboid numbers the faces 0 −x, 1 +x, 2 −y, 3 +y, 4 −z, 5 +z.
const MINUS_X: u32 = 0;
const PLUS_X: u32 = 1;

fn body(name: &str, lo: [f64; 3], hi: [f64; 3]) -> Body {
    Body { name: name.into(), surface: Surface::cuboid(lo, hi), material: STEEL }
}

fn cantilever() -> Model {
    Model {
        bodies: vec![body("Beam", [0.0; 3], [100.0, 10.0, 10.0])],
        loads: vec![
            Load { name: "Fixed 1".into(), targets: vec![(0, vec![MINUS_X])], kind: LoadKind::Fixed },
            Load { name: "Force 1".into(), targets: vec![(0, vec![PLUS_X])], kind: LoadKind::Force([0.0, 0.0, -100.0]) },
        ],
        bonds: vec![],
    }
}

/// The mean vertical displacement of the nodes on the plane x = 100 (the loaded end).
fn tip_deflection(s: &Solution) -> f64 {
    let (mut sum, mut n) = (0.0, 0);
    for b in &s.bodies {
        for (p, u) in b.mesh.nodes.iter().zip(&b.displacement) {
            if (p[0] - 100.0).abs() < 1e-9 {
                sum += u[2];
                n += 1;
            }
        }
    }
    -sum / n as f64
}

fn no_progress(_: cadrs_fea::Stage, _: f32) {}

/// The tests take turns, so the timing test has the machine to itself (the solver is parallel).
static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());
fn turn() -> std::sync::MutexGuard<'static, ()> {
    ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner())
}

#[test]
fn cantilever_tip_deflection_and_mid_span_stress() {
    let _turn = turn();
    let s = solve(&cantilever(), &Options::default(), &no_progress).unwrap();
    let delta = tip_deflection(&s);
    let err = (delta - 0.200) / 0.200;
    eprintln!("cantilever: {} elements, h {:.2} mm, tip {delta:.5} mm ({:+.2} %)", s.stats.elements, s.stats.element_size, err * 100.0);
    assert!(err.abs() < 0.03, "tip deflection {delta} mm vs 0.200 mm");
    // σxx at mid-span on the top face's centre line, from the element there.
    let sigma = s.stress_at(0, [50.0, 5.0, 10.0]).unwrap()[0];
    let err = (sigma - 30.0) / 30.0;
    eprintln!("cantilever: mid-span top σxx {sigma:.3} MPa ({:+.2} %)", err * 100.0);
    assert!(err.abs() < 0.05, "mid-span σ {sigma} MPa vs 30.0 MPa");
    // Nodal (averaged) von Mises near there agrees too: the top fibre is in uniaxial tension.
    let b = &s.bodies[0];
    let (k, _) = b.mesh.nodes.iter().enumerate().filter(|(_, p)| (p[2] - 10.0).abs() < 1e-9).min_by(|a, c| {
        let d = |p: &[f64; 3]| (p[0] - 50.0).powi(2) + (p[1] - 5.0).powi(2);
        d(a.1).total_cmp(&d(c.1))
    }).unwrap();
    let near = b.mesh.nodes[k];
    let expect = 100.0 * (100.0 - near[0]) * 5.0 / (10.0 * 1000.0 / 12.0);
    eprintln!("cantilever: von Mises at node {near:?}: {:.3} MPa (beam theory there {expect:.3})", b.von_mises[k]);
    assert!((b.von_mises[k] - expect).abs() / expect < 0.05);
}

#[test]
fn two_halves_bonded_bend_like_the_solid_beam() {
    let _turn = turn();
    let solid = solve(&cantilever(), &Options::default(), &no_progress).unwrap();
    // The same beam as two 50 mm halves joined at x = 50 by a bond (a Fastened mate with
    // Simulation connection): the same deflection, within 5 %.
    let halves = Model {
        bodies: vec![body("Half 1", [0.0; 3], [50.0, 10.0, 10.0]), body("Half 2", [50.0, 0.0, 0.0], [100.0, 10.0, 10.0])],
        loads: vec![
            Load { name: "Fixed 1".into(), targets: vec![(0, vec![MINUS_X])], kind: LoadKind::Fixed },
            Load { name: "Force 1".into(), targets: vec![(1, vec![PLUS_X])], kind: LoadKind::Force([0.0, 0.0, -100.0]) },
        ],
        bonds: vec![Bond { name: "Fastened 1".into(), a: 1, b: 0, required: true }],
    };
    let s = solve(&halves, &Options::default(), &no_progress).unwrap();
    let (d0, d1) = (tip_deflection(&solid), tip_deflection(&s));
    eprintln!("halves: {} tied nodes; solid {d0:.5} mm, bonded {d1:.5} mm ({:+.2} %)", s.stats.bonded_nodes[0], (d1 - d0) / d0 * 100.0);
    assert!(((d1 - d0) / d0).abs() < 0.05);
    assert!(((d1 - 0.2) / 0.2).abs() < 0.05);
    // The joint carries the bending smoothly: the top fibre's von Mises on either side of it is
    // the beam's 30 MPa (M·c/I at x = 50), within 10 % at the nodes nearest the seam.
    for (body, x) in [(0usize, 50.0), (1usize, 50.0)] {
        let b = &s.bodies[body];
        let (k, p) = b.mesh.nodes.iter().enumerate().filter(|(_, p)| (p[2] - 10.0).abs() < 1e-9).min_by(|a, c| {
            let d = |p: &[f64; 3]| (p[0] - x).powi(2) + (p[1] - 5.0).powi(2);
            d(a.1).total_cmp(&d(c.1))
        }).unwrap();
        let expect = 100.0 * (100.0 - p[0]) * 5.0 / (10.0 * 1000.0 / 12.0);
        eprintln!("halves: {} at {p:?}: von Mises {:.2} MPa (beam {expect:.2})", b.name, b.von_mises[k]);
        assert!((b.von_mises[k] - expect).abs() / expect < 0.10);
    }
    // Without the bond the loaded half is free.
    let mut free = halves.clone();
    free.bonds.clear();
    assert_eq!(solve(&free, &Options::default(), &no_progress).unwrap_err(), cadrs_fea::FeaError::Free("Half 2".into()));
    // Apart, the halves can't be bonded.
    let mut apart = halves;
    apart.bodies[1] = body("Half 2", [51.0, 0.0, 0.0], [100.0, 10.0, 10.0]);
    assert!(matches!(solve(&apart, &Options::default(), &no_progress).unwrap_err(), cadrs_fea::FeaError::Unbonded(_)));
}

#[test]
fn a_pressure_and_a_normal_force_push_in() {
    let _turn = turn();
    // A 10 mm cube fixed at its bottom (z = 0), 1 MPa on its top: uniaxial compression
    // σzz ≈ −1 MPa in the middle, and a normal force of 100 N on the top (100 mm²) is the same.
    for kind in [LoadKind::Pressure(1.0), LoadKind::NormalForce(100.0)] {
        let m = Model {
            bodies: vec![body("Cube", [0.0; 3], [10.0; 3])],
            loads: vec![
                Load { name: "Fixed".into(), targets: vec![(0, vec![4])], kind: LoadKind::Fixed },
                Load { name: "Load".into(), targets: vec![(0, vec![5])], kind },
            ],
            bonds: vec![],
        };
        let s = solve(&m, &Options { target_elements: 3000, ..Default::default() }, &no_progress).unwrap();
        let szz = s.stress_at(0, [5.0, 5.0, 6.0]).unwrap()[2];
        assert!((szz + 1.0).abs() < 0.08, "{kind:?}: σzz {szz}");
        assert!(s.displacement_at(0, [5.0, 5.0, 10.0]).unwrap()[2] < 0.0);
    }
}

#[test]
fn twenty_thousand_elements_solve_in_under_ten_seconds_off_the_main_thread() {
    let _turn = turn();
    // The best of up to three runs: the machine may be busy with other work (wall-clock time is
    // what the user waits for, so it is what's measured).
    let budget = 10.0;
    let mut best = f64::MAX;
    for _ in 0..3 {
        let model = cantilever();
        let opts = Options { element_size: Some(1.3), ..Default::default() };
        let start = std::time::Instant::now();
        let s = std::thread::spawn(move || solve(&model, &opts, &no_progress)).join().unwrap().unwrap();
        let secs = start.elapsed().as_secs_f64();
        eprintln!(
            "timing: {} elements, {} nodes, {} dofs: mesh {:.2} s, assemble {:.2} s, factor {:.2} s, total {secs:.2} s; tip {:.5} mm",
            s.stats.elements, s.stats.nodes, s.stats.dofs, s.stats.mesh_seconds, s.stats.assemble_seconds, s.stats.factor_seconds, tip_deflection(&s)
        );
        assert!(s.stats.elements >= 20_000, "{} elements", s.stats.elements);
        assert!((tip_deflection(&s) - 0.2).abs() / 0.2 < 0.03);
        best = best.min(secs);
        if best < 10.0 {
            break;
        }
    }
    assert!(best < budget, "{best} s");
}

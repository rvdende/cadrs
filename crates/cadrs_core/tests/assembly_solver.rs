//! The assembly solver (P3B.2, `intro-to-assemblies.md` A6.7, A6.10, A7.1–A7.4, A13): the DOF
//! each mate type leaves, offsets, limits, groups, and speed.

use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::mate::{
    Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, mate_position,
};
use cadrs_core::assembly::solver::{self, Drive, SolveOptions};
use cadrs_core::assembly::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::{ElementId, FeatureId, PartId};

fn src() -> InstanceSource {
    InstanceSource::Part {
        element: ElementId::from_u128(1),
        part: PartId::new(FeatureId::from_u128(1), 0),
    }
}

fn inst(n: u128, pose: Pose, fixed: bool) -> Instance {
    let mut i = Instance::new(InstanceId::from_u128(n), src(), pose);
    i.index = n as u32;
    i.fixed = fixed;
    i
}

fn feature(n: u128, kind: MateKind) -> MateFeature {
    MateFeature::new(MateId::from_u128(n), format!("m{n}"), kind)
}

/// A tilted frame, so no axis lines up with the model's.
fn tilted(origin: [f64; 3]) -> ConnectorFrame {
    ConnectorFrame::new(origin, [0.3, -0.4, 0.866], [0.9, 0.1, 0.2])
}

fn frame_of(c: &MateConnector) -> ConnectorFrame {
    c.local_frame(None)
}

fn world(asm: &Assembly, c: &MateConnector) -> ConnectorFrame {
    c.local_frame(None)
        .moved(&asm.instance(c.instance).unwrap().pose)
}

fn apply(asm: &mut Assembly, s: &solver::Solution) {
    for (id, p) in &s.poses {
        asm.instance_mut(*id).unwrap().pose = *p;
    }
}

fn two(t: MateType, a_pose: Pose) -> Assembly {
    let fa = tilted([10.0, 5.0, -3.0]);
    let fb = tilted([-4.0, 2.0, 7.0]);
    let m = Mate::new(
        t,
        MateConnector::at(InstanceId::from_u128(1), fa),
        MateConnector::at(InstanceId::from_u128(2), fb),
    );
    Assembly {
        instances: vec![inst(1, a_pose, false), inst(2, Pose::IDENTITY, true)],
        mates: vec![feature(1, MateKind::Mate(m))],
        ..Default::default()
    }
}

fn dist(a: [f64; 3], b: [f64; 3]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

/// The eight connector mate types (Tangent and Width have their own tests).
const CONNECTOR_TYPES: [MateType; 8] = [
    MateType::Fastened,
    MateType::Revolute,
    MateType::Slider,
    MateType::Cylindrical,
    MateType::PinSlot,
    MateType::Planar,
    MateType::Ball,
    MateType::Parallel,
];

#[test]
fn each_type_leaves_its_dof() {
    // A7: Fastened 0, Revolute 1 (rotate Z), Slider 1 (translate Z), Cylindrical 2, Pin slot 2,
    // Planar 3, Ball 3, Parallel 4 — the rank of the constraint Jacobian is 6 − DOF, at a solved
    // and at an unsolved (perturbed) placement; after solving the constraint holds.
    for t in CONNECTOR_TYPES {
        let dof = t.dof_count().unwrap();
        let asm = two(t, Pose::IDENTITY);
        let m = asm.mates[0].mate().unwrap();
        assert_eq!(
            solver::mate_rank(m, m.connectors[0].frame, m.connectors[1].frame),
            6 - dof as usize,
            "{t:?}"
        );
        let mut asm = two(
            t,
            Pose::rotation_about([1.0, 2.0, 3.0], [0.2, 0.7, 0.1], 0.4),
        );
        let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
        assert!(s.converged, "{t:?}: {}", s.residual);
        apply(&mut asm, &s);
        let dofs = solver::dof_counts(&asm, &frame_of);
        assert_eq!(dofs[&InstanceId::from_u128(1)], dof, "{t:?}");
        assert_eq!(dofs[&InstanceId::from_u128(2)], 0, "fixed");
    }
}

#[test]
fn each_type_allows_its_motion_and_no_other() {
    // Solve, then move the free instance along / about the mate's Z (in the mate frame) and
    // re-solve: an allowed motion stays, a forbidden one is undone.
    for t in CONNECTOR_TYPES {
        let mut asm = two(t, Pose::translation([3.0, -2.0, 1.0]));
        let s = solver::solve(
            &asm,
            &frame_of,
            &SolveOptions {
                snap: Some(MateId::from_u128(1)),
                ..Default::default()
            },
        );
        assert!(s.converged);
        apply(&mut asm, &s);
        let c0 = asm.mates[0].mate().unwrap().connectors[0];
        let f = world(&asm, &c0);
        let slide = |a: [f64; 3]| Pose::translation([a[0] * 5.0, a[1] * 5.0, a[2] * 5.0]);
        let motions = [
            (slide(f.x), t.dof().contains(&Dof::X)),
            (slide(f.y()), t.dof().contains(&Dof::Y)),
            (slide(f.z), t.dof().contains(&Dof::Z)),
            (
                Pose::rotation_about(f.origin, f.z, 0.7),
                t.dof().contains(&Dof::Angle),
            ),
            (
                Pose::rotation_about(f.origin, f.x, 0.4),
                t == MateType::Ball,
            ),
            (
                Pose::rotation_about(f.origin, f.y(), -0.3),
                t == MateType::Ball,
            ),
        ];
        for (motion, allowed) in motions {
            let mut moved = asm.clone();
            let p = moved
                .instance(InstanceId::from_u128(1))
                .unwrap()
                .pose
                .then(&motion);
            moved.instance_mut(InstanceId::from_u128(1)).unwrap().pose = p;
            let s = solver::solve(&moved, &frame_of, &SolveOptions::default());
            assert!(s.converged);
            let kept = s.poses[0].1;
            let same = dist(kept.translation, p.translation) < 1e-6
                && (0..3)
                    .all(|i| (0..3).all(|j| (kept.rotation[i][j] - p.rotation[i][j]).abs() < 1e-9));
            assert_eq!(same, allowed, "{t:?}");
        }
    }
}

#[test]
fn offset_z_moves_the_rod_by_the_offset() {
    // A8.1 / A15.12: Fastened with Offset Z 0.5 in. The first connector (the rod's top edge)
    // ends 0.5 in along the second's Z (the flange hole's), same axes.
    let rod = MateConnector::at(
        InstanceId::from_u128(1),
        ConnectorFrame::new([0.0, 0.0, 190.5], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    );
    let hole = MateConnector::at(
        InstanceId::from_u128(2),
        ConnectorFrame::new([24.13, 24.13, 165.1], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    );
    let mut m = Mate::new(MateType::Fastened, rod, hole);
    let mut asm = Assembly {
        instances: vec![
            inst(1, Pose::translation([100.0, 50.0, 0.0]), false),
            inst(2, Pose::IDENTITY, true),
        ],
        mates: vec![feature(1, MateKind::Mate(m.clone()))],
        ..Default::default()
    };
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            snap: Some(MateId::from_u128(1)),
            ..Default::default()
        },
    );
    apply(&mut asm, &s);
    let at_zero = world(&asm, &rod);
    assert!(dist(at_zero.origin, [24.13, 24.13, 165.1]) < 1e-9);
    m.offset = Some(MateOffset {
        translation: [0.0, 0.0, 0.5 * 25.4],
        ..Default::default()
    });
    asm.mates[0].kind = MateKind::Mate(m);
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    assert!(s.converged);
    apply(&mut asm, &s);
    let f = world(&asm, &rod);
    assert!(
        dist(f.origin, [24.13, 24.13, 165.1 + 12.7]) < 1e-7,
        "{:?}",
        f.origin
    );
    // Moved by exactly 0.5 in, not turned.
    assert!(dist(f.origin, at_zero.origin) - 12.7 < 1e-7);
    assert!(dist(f.z, [0.0, 0.0, 1.0]) < 1e-9 && dist(f.x, [1.0, 0.0, 0.0]) < 1e-9);
}

#[test]
fn a_limit_clamps_a_drag() {
    // A8.4 / A15.16: a Slider limited to Z −3.25…0 in. Dragging the piston 5 in up (past the
    // limit) and solving leaves it at the limit; dragging it 1 in stays.
    let piston = MateConnector::at(
        InstanceId::from_u128(1),
        ConnectorFrame::new([0.0, 0.0, 31.75], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    );
    let cap = MateConnector::at(
        InstanceId::from_u128(2),
        ConnectorFrame::new([0.0, 0.0, 31.75], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    );
    let mut m = Mate::new(MateType::Slider, piston, cap);
    m.limits = Some(MateLimits {
        z: Some((-3.25 * 25.4, 0.0)),
        ..Default::default()
    });
    let asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, false),
            inst(2, Pose::IDENTITY, true),
        ],
        mates: vec![feature(1, MateKind::Mate(m))],
        ..Default::default()
    };
    for (drag, want) in [(5.0, 3.25), (1.0, 1.0), (-2.0, 0.0)] {
        let mut a = asm.clone();
        a.instance_mut(InstanceId::from_u128(1)).unwrap().pose =
            Pose::translation([0.0, 0.0, drag * 25.4]);
        let s = solver::solve(
            &a,
            &frame_of,
            &SolveOptions {
                movers: vec![InstanceId::from_u128(1)],
                ..Default::default()
            },
        );
        assert!(s.converged);
        apply(&mut a, &s);
        let z = a
            .instance(InstanceId::from_u128(1))
            .unwrap()
            .pose
            .translation[2]
            / 25.4;
        assert!((z - want).abs() < 1e-7, "drag {drag}: z {z}");
        let (pos, _) = mate_position(&world(&a, &piston), &world(&a, &cap));
        assert!((-3.25 * 25.4 - 1e-7..=1e-7).contains(&pos));
    }
    // Apply limit position: a drive to the limit.
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            snap: Some(MateId::from_u128(1)),
            drives: vec![Drive {
                mate: MateId::from_u128(1),
                dof: Dof::Z,
                value: -3.25 * 25.4,
            }],
            ..Default::default()
        },
    );
    assert!((s.poses[0].1.translation[2] / 25.4 - 3.25).abs() < 1e-9);
}

#[test]
fn a_group_keeps_relative_transforms() {
    // A13.3: two grouped instances move as one when the group is mated to a fixed instance.
    let p1 = Pose::rotation_about([5.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.3)
        .then(&Pose::translation([40.0, 10.0, 0.0]));
    let p2 = Pose::translation([55.0, -20.0, 8.0]);
    let fa = tilted([1.0, 2.0, 3.0]);
    let fb = tilted([0.0, 0.0, 0.0]);
    let m = Mate::new(
        MateType::Fastened,
        MateConnector::at(InstanceId::from_u128(1), fa),
        MateConnector::at(InstanceId::from_u128(3), fb),
    );
    let mut asm = Assembly {
        instances: vec![
            inst(1, p1, false),
            inst(2, p2, false),
            inst(3, Pose::IDENTITY, true),
        ],
        mates: vec![
            feature(
                1,
                MateKind::Group {
                    instances: vec![InstanceId::from_u128(1), InstanceId::from_u128(2)],
                },
            ),
            feature(2, MateKind::Mate(m)),
        ],
        ..Default::default()
    };
    let rel0 = p2.then(&p1.inverse());
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            snap: Some(MateId::from_u128(2)),
            ..Default::default()
        },
    );
    assert!(s.converged);
    apply(&mut asm, &s);
    let (q1, q2) = (asm.instances[0].pose, asm.instances[1].pose);
    assert!(
        dist(q1.translation, p1.translation) > 1.0,
        "the group moved"
    );
    let rel1 = q2.then(&q1.inverse());
    assert!(dist(rel1.translation, rel0.translation) < 1e-9);
    for i in 0..3 {
        for j in 0..3 {
            assert!((rel1.rotation[i][j] - rel0.rotation[i][j]).abs() < 1e-12);
        }
    }
    let dofs = solver::dof_counts(&asm, &frame_of);
    assert_eq!(
        dofs[&InstanceId::from_u128(2)],
        0,
        "grouped and fastened: no DOF"
    );
    // A13.4: without the mate the group still has 6 DOF.
    asm.mates.truncate(1);
    assert_eq!(
        solver::dof_counts(&asm, &frame_of)[&InstanceId::from_u128(1)],
        6
    );
}

#[test]
fn a_fifty_instance_chain_solves_fast() {
    // 50 instances, each Revolute to the one before (the first fixed), every one knocked off by
    // a small, different amount. < 50 ms in the debug build `cargo test` makes (the workspace's
    // dev profile: our crates at opt-level 1, dependencies at 3); about 5 ms there in practice.
    let n = 50u128;
    let mut instances = Vec::new();
    let mut mates = Vec::new();
    for k in 0..n {
        let jiggle = Pose::rotation_about([0.0; 3], [0.3, 0.5, 0.8], 0.02 * ((k % 7) as f64 - 3.0))
            .then(&Pose::translation([
                0.3 * (k % 3) as f64,
                -0.2 * (k % 5) as f64,
                0.1 * (k % 4) as f64,
            ]));
        instances.push(inst(
            k + 1,
            if k == 0 {
                Pose::IDENTITY
            } else {
                jiggle.then(&Pose::translation([20.0 * k as f64, 0.0, 0.0]))
            },
            k == 0,
        ));
        if k > 0 {
            // The joint half way between them, axis Z.
            let joint =
                |x: f64| ConnectorFrame::new([x, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
            let m = Mate::new(
                MateType::Revolute,
                MateConnector::at(InstanceId::from_u128(k + 1), joint(-10.0)),
                MateConnector::at(InstanceId::from_u128(k), joint(10.0)),
            );
            mates.push(feature(k, MateKind::Mate(m)));
        }
    }
    let asm = Assembly {
        instances,
        mates,
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!(
        "50-instance chain: {ms:.1} ms, {} iterations, residual {:e}",
        s.iterations, s.residual
    );
    assert!(s.converged, "{}", s.residual);
    assert!(ms < 50.0, "{ms} ms");
    // Every joint holds.
    let mut solved = asm.clone();
    apply(&mut solved, &s);
    for f in &solved.mates {
        let m = f.mate().unwrap();
        assert!(
            dist(
                world(&solved, &m.connectors[0]).origin,
                world(&solved, &m.connectors[1]).origin
            ) < 1e-6
        );
    }
}

/// Issue #135: one Solve places a chain of Fastened instances that start far from their
/// places (half turns, metres away): no second Solve needed.
#[test]
fn one_solve_converges_a_scattered_fastened_chain() {
    let n = 6u128;
    let start = |k: u128| {
        let a = 0.9 + k as f64 * 0.7;
        Pose::rotation_about([0.0; 3], [a.sin(), a.cos(), 0.4], 2.0 + k as f64).then(
            &Pose::translation([k as f64 * 400.0, -(k as f64) * 250.0, 90.0]),
        )
    };
    let instances =
        (1..=n).map(|k| inst(k, if k == 1 { Pose::IDENTITY } else { start(k) }, k == 1));
    let mut mates = Vec::new();
    let mut ends = Vec::new();
    for k in 2..=n {
        let a = MateConnector::at(InstanceId::from_u128(k), tilted([k as f64, 3.0, -2.0]));
        let b = MateConnector::at(
            InstanceId::from_u128(k - 1),
            ConnectorFrame::new([20.0, k as f64, 5.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
        );
        ends.push((a, b));
        mates.push(feature(
            k,
            MateKind::Mate(Mate::new(MateType::Fastened, a, b)),
        ));
    }
    let mut asm = Assembly {
        instances: instances.collect(),
        mates,
        ..Default::default()
    };
    // The mate dialog's Solve: the mate's instance is the mover, the others weigh 10⁻³.
    let opts = SolveOptions {
        movers: vec![InstanceId::from_u128(n)],
        ..Default::default()
    };
    let s = solver::solve(&asm, &frame_of, &opts);
    apply(&mut asm, &s);
    assert!(
        s.converged,
        "residual {} after {} iterations",
        s.residual, s.iterations
    );
    for (a, b) in &ends {
        let (fa, fb) = (world(&asm, a), world(&asm, b));
        assert!(
            dist(fa.origin, fb.origin) < 1e-6,
            "{:?} vs {:?}",
            fa.origin,
            fb.origin
        );
        assert!(dist(fa.z, fb.z) < 1e-6 && dist(fa.x, fb.x) < 1e-6);
    }
}

/// Issue #135: Fastened mates that close loops (a part fastened to two parts fastened
/// together) are redundant but consistent; one Solve still places every part.
#[test]
fn one_solve_converges_redundant_fastened_loops() {
    let n = 6u128;
    // Where every part belongs, and where it starts: turned and moved far from there.
    let truth = |k: u128| {
        let a = k as f64 * 1.3;
        Pose::rotation_about([0.0; 3], [a.cos(), a.sin(), 0.5], 0.3 * k as f64).then(
            &Pose::translation([k as f64 * 60.0, 20.0, -(k as f64) * 15.0]),
        )
    };
    let start = |k: u128| {
        let a = 0.9 + k as f64 * 0.7;
        Pose::rotation_about([0.0; 3], [a.sin(), a.cos(), 0.4], 2.0 + k as f64).then(
            &Pose::translation([k as f64 * 400.0, -(k as f64) * 250.0, 90.0]),
        )
    };
    let pairs = [
        (2, 1),
        (3, 2),
        (4, 3),
        (5, 4),
        (6, 5),
        (3, 1),
        (5, 2),
        (4, 1),
        (6, 3),
    ];
    let mut mates = Vec::new();
    let mut ends = Vec::new();
    for (j, (a, b)) in pairs.iter().enumerate() {
        // A frame in the world where the two parts meet, in each part's own coordinates.
        let t = j as f64;
        let w = ConnectorFrame::new(
            [10.0 * t, 5.0 - t, 3.0 * t],
            [0.3, -0.4 + 0.05 * t, 0.866],
            [0.9, 0.1, 0.2],
        );
        let local = |k: u128| w.moved(&truth(k).inverse());
        let ca = MateConnector::at(InstanceId::from_u128(*a), local(*a));
        let cb = MateConnector::at(InstanceId::from_u128(*b), local(*b));
        ends.push((ca, cb));
        mates.push(feature(
            j as u128 + 1,
            MateKind::Mate(Mate::new(MateType::Fastened, ca, cb)),
        ));
    }
    let mut asm = Assembly {
        instances: (1..=n)
            .map(|k| inst(k, if k == 1 { truth(1) } else { start(k) }, k == 1))
            .collect(),
        mates,
        ..Default::default()
    };
    let opts = SolveOptions {
        movers: vec![InstanceId::from_u128(n)],
        ..Default::default()
    };
    let s = solver::solve(&asm, &frame_of, &opts);
    apply(&mut asm, &s);
    assert!(
        s.converged,
        "residual {} after {} iterations",
        s.residual, s.iterations
    );
    for (a, b) in &ends {
        let (fa, fb) = (world(&asm, a), world(&asm, b));
        assert!(
            dist(fa.origin, fb.origin) < 1e-6,
            "{:?} vs {:?}",
            fa.origin,
            fb.origin
        );
    }
}

/// Issue #135: one Solve places random assemblies (2–7 parts, a tree of mates and sometimes
/// loops, starting far off or half a turn away) whose mates all hold at known placements: how
/// many needed a second Solve (or more, up to four), and how many never converged.
fn one_solve_converges_random_assemblies(types: &[MateType]) -> (usize, usize) {
    let mut x: u64 = 0x2545F4914F6CDD1D;
    let mut rnd = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % 1_000_000) as f64 / 1_000_000.0
    };
    let (mut more, mut never) = (0, 0);
    for trial in 0..4000 {
        let n = 2 + (rnd() * 6.0) as u128;
        let truth: Vec<Pose> = (0..=n)
            .map(|_| {
                Pose::rotation_about(
                    [0.0; 3],
                    [rnd() - 0.5, rnd() - 0.5, rnd() - 0.5],
                    rnd() * 6.3,
                )
                .then(&Pose::translation([
                    rnd() * 500.0,
                    rnd() * 500.0,
                    rnd() * 500.0,
                ]))
            })
            .collect();
        let mut pairs: Vec<(u128, u128)> = (2..=n)
            .map(|k| (k, 1 + (rnd() * (k - 1) as f64) as u128))
            .collect();
        if rnd() < 0.5 {
            for _ in 0..(rnd() * 3.0) as usize {
                let a = 2 + (rnd() * (n - 1) as f64) as u128;
                let b = 1 + (rnd() * (a - 1) as f64) as u128;
                pairs.push((a.min(n), b));
            }
        }
        let mut mates = Vec::new();
        for (j, (a, b)) in pairs.iter().enumerate() {
            let w = ConnectorFrame::new(
                [rnd() * 100.0, rnd() * 100.0, rnd() * 100.0],
                [rnd() - 0.5, rnd() - 0.5, rnd() - 0.5 + 0.01],
                [rnd() - 0.5 + 0.01, rnd() - 0.5, rnd() - 0.5],
            );
            let local = |k: u128| w.moved(&truth[k as usize].inverse());
            let ca = MateConnector::at(InstanceId::from_u128(*a), local(*a));
            let cb = MateConnector::at(InstanceId::from_u128(*b), local(*b));
            let t = types[(rnd() * types.len() as f64) as usize % types.len()];
            mates.push(feature(j as u128 + 1, MateKind::Mate(Mate::new(t, ca, cb))));
        }
        let mode = rnd();
        let asm = Assembly {
            instances: (1..=n)
                .map(|k| {
                    let p = if k == 1 {
                        truth[1]
                    } else if mode < 0.3 {
                        // A half turn away.
                        Pose::rotation_about(
                            [0.0; 3],
                            [rnd() - 0.5, rnd() - 0.5, rnd() - 0.5],
                            std::f64::consts::PI,
                        )
                        .then(&truth[k as usize])
                    } else {
                        Pose::rotation_about(
                            [0.0; 3],
                            [rnd() - 0.5, rnd() - 0.5, rnd() - 0.5],
                            rnd() * 6.3,
                        )
                        .then(&Pose::translation([
                            rnd() * 2000.0,
                            rnd() * 2000.0,
                            rnd() * 2000.0,
                        ]))
                    };
                    inst(k, p, k == 1)
                })
                .collect(),
            mates,
            ..Default::default()
        };
        let opts = SolveOptions {
            movers: vec![InstanceId::from_u128(n)],
            ..Default::default()
        };
        let s = solver::solve(&asm, &frame_of, &opts);
        if !s.converged {
            let mut a2 = asm.clone();
            apply(&mut a2, &s);
            let mut presses = 1;
            let first = s.clone();
            let mut last = s;
            while !last.converged && presses < 4 {
                last = solver::solve(&a2, &frame_of, &opts);
                apply(&mut a2, &last);
                presses += 1;
            }
            eprintln!(
                "trial {trial}: {n} parts, {} mates: residual {} after one solve ({} iterations); \
                 converged after {presses} solves: {}",
                pairs.len(),
                first.residual,
                first.iterations,
                last.converged
            );
            if last.converged {
                more += 1;
            } else {
                never += 1;
            }
        }
    }
    (more, never)
}

/// Issue #135's case, Fastened mates only: 16 of 4000 needed a second Solve before the fix.
#[test]
fn one_solve_converges_random_fastened_assemblies() {
    assert_eq!(
        one_solve_converges_random_assemblies(&[MateType::Fastened]),
        (0, 0)
    );
}

/// The same with every connector mate type but Pin slot. Some of these never converge (a
/// mix of Planar, Parallel and Ball in a loop can sit in a local minimum); this counts the ones a
/// second Solve would still finish: 202 of 4000 before issue #135's fix, 25 after.
#[test]
#[ignore = "slow: about 8 s"]
fn one_solve_converges_random_mixed_assemblies() {
    let types = [
        MateType::Fastened,
        MateType::Revolute,
        MateType::Slider,
        MateType::Cylindrical,
        MateType::Planar,
        MateType::Ball,
        MateType::Parallel,
    ];
    let (more, never) = one_solve_converges_random_assemblies(&types);
    eprintln!("{more} needed more solves, {never} never converged");
    assert!(more <= 25, "{more} needed more than one solve");
}

//! The assembly solver (P3B.2, `intro-to-assemblies.md` A6.7, A6.10, A7.1–A7.4, A13): the DOF
//! each mate type leaves, offsets, limits, groups, and speed.

use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::mate::{Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, mate_position};
use cadrs_core::assembly::solver::{self, Drive, SolveOptions};
use cadrs_core::assembly::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use cadrs_core::{ElementId, FeatureId, PartId};

fn src() -> InstanceSource {
    InstanceSource::Part { element: ElementId::from_u128(1), part: PartId::new(FeatureId::from_u128(1), 0) }
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
    c.local_frame(None).moved(&asm.instance(c.instance).unwrap().pose)
}

fn apply(asm: &mut Assembly, s: &solver::Solution) {
    for (id, p) in &s.poses {
        asm.instance_mut(*id).unwrap().pose = *p;
    }
}

fn two(t: MateType, a_pose: Pose) -> Assembly {
    let fa = tilted([10.0, 5.0, -3.0]);
    let fb = tilted([-4.0, 2.0, 7.0]);
    let m = Mate::new(t, MateConnector::at(InstanceId::from_u128(1), fa), MateConnector::at(InstanceId::from_u128(2), fb));
    Assembly { instances: vec![inst(1, a_pose, false), inst(2, Pose::IDENTITY, true)], mates: vec![feature(1, MateKind::Mate(m))], ..Default::default() }
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
        assert_eq!(solver::mate_rank(m, m.connectors[0].frame, m.connectors[1].frame), 6 - dof as usize, "{t:?}");
        let mut asm = two(t, Pose::rotation_about([1.0, 2.0, 3.0], [0.2, 0.7, 0.1], 0.4));
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
        let s = solver::solve(&asm, &frame_of, &SolveOptions { snap: Some(MateId::from_u128(1)), ..Default::default() });
        assert!(s.converged);
        apply(&mut asm, &s);
        let c0 = asm.mates[0].mate().unwrap().connectors[0];
        let f = world(&asm, &c0);
        let slide = |a: [f64; 3]| Pose::translation([a[0] * 5.0, a[1] * 5.0, a[2] * 5.0]);
        let motions = [
            (slide(f.x), t.dof().contains(&Dof::X)),
            (slide(f.y()), t.dof().contains(&Dof::Y)),
            (slide(f.z), t.dof().contains(&Dof::Z)),
            (Pose::rotation_about(f.origin, f.z, 0.7), t.dof().contains(&Dof::Angle)),
            (Pose::rotation_about(f.origin, f.x, 0.4), t == MateType::Ball),
            (Pose::rotation_about(f.origin, f.y(), -0.3), t == MateType::Ball),
        ];
        for (motion, allowed) in motions {
            let mut moved = asm.clone();
            let p = moved.instance(InstanceId::from_u128(1)).unwrap().pose.then(&motion);
            moved.instance_mut(InstanceId::from_u128(1)).unwrap().pose = p;
            let s = solver::solve(&moved, &frame_of, &SolveOptions::default());
            assert!(s.converged);
            let kept = s.poses[0].1;
            let same = dist(kept.translation, p.translation) < 1e-6
                && (0..3).all(|i| (0..3).all(|j| (kept.rotation[i][j] - p.rotation[i][j]).abs() < 1e-9));
            assert_eq!(same, allowed, "{t:?}");
        }
    }
}

#[test]
fn offset_z_moves_the_rod_by_the_offset() {
    // A8.1 / A15.12: Fastened with Offset Z 0.5 in. The first connector (the rod's top edge)
    // ends 0.5 in along the second's Z (the flange hole's), same axes.
    let rod = MateConnector::at(InstanceId::from_u128(1), ConnectorFrame::new([0.0, 0.0, 190.5], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let hole = MateConnector::at(InstanceId::from_u128(2), ConnectorFrame::new([24.13, 24.13, 165.1], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let mut m = Mate::new(MateType::Fastened, rod, hole);
    let mut asm = Assembly {
        instances: vec![inst(1, Pose::translation([100.0, 50.0, 0.0]), false), inst(2, Pose::IDENTITY, true)],
        mates: vec![feature(1, MateKind::Mate(m.clone()))],
        ..Default::default()
    };
    let s = solver::solve(&asm, &frame_of, &SolveOptions { snap: Some(MateId::from_u128(1)), ..Default::default() });
    apply(&mut asm, &s);
    let at_zero = world(&asm, &rod);
    assert!(dist(at_zero.origin, [24.13, 24.13, 165.1]) < 1e-9);
    m.offset = Some(MateOffset { translation: [0.0, 0.0, 0.5 * 25.4], ..Default::default() });
    asm.mates[0].kind = MateKind::Mate(m);
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    assert!(s.converged);
    apply(&mut asm, &s);
    let f = world(&asm, &rod);
    assert!(dist(f.origin, [24.13, 24.13, 165.1 + 12.7]) < 1e-7, "{:?}", f.origin);
    // Moved by exactly 0.5 in, not turned.
    assert!(dist(f.origin, at_zero.origin) - 12.7 < 1e-7);
    assert!(dist(f.z, [0.0, 0.0, 1.0]) < 1e-9 && dist(f.x, [1.0, 0.0, 0.0]) < 1e-9);
}

#[test]
fn a_limit_clamps_a_drag() {
    // A8.4 / A15.16: a Slider limited to Z −3.25…0 in. Dragging the piston 5 in up (past the
    // limit) and solving leaves it at the limit; dragging it 1 in stays.
    let piston = MateConnector::at(InstanceId::from_u128(1), ConnectorFrame::new([0.0, 0.0, 31.75], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let cap = MateConnector::at(InstanceId::from_u128(2), ConnectorFrame::new([0.0, 0.0, 31.75], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]));
    let mut m = Mate::new(MateType::Slider, piston, cap);
    m.limits = Some(MateLimits { z: Some((-3.25 * 25.4, 0.0)), ..Default::default() });
    let asm = Assembly {
        instances: vec![inst(1, Pose::IDENTITY, false), inst(2, Pose::IDENTITY, true)],
        mates: vec![feature(1, MateKind::Mate(m))],
        ..Default::default()
    };
    for (drag, want) in [(5.0, 3.25), (1.0, 1.0), (-2.0, 0.0)] {
        let mut a = asm.clone();
        a.instance_mut(InstanceId::from_u128(1)).unwrap().pose = Pose::translation([0.0, 0.0, drag * 25.4]);
        let s = solver::solve(&a, &frame_of, &SolveOptions { movers: vec![InstanceId::from_u128(1)], ..Default::default() });
        assert!(s.converged);
        apply(&mut a, &s);
        let z = a.instance(InstanceId::from_u128(1)).unwrap().pose.translation[2] / 25.4;
        assert!((z - want).abs() < 1e-7, "drag {drag}: z {z}");
        let (pos, _) = mate_position(&world(&a, &piston), &world(&a, &cap));
        assert!((-3.25 * 25.4 - 1e-7..=1e-7).contains(&pos));
    }
    // Apply limit position: a drive to the limit.
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions { snap: Some(MateId::from_u128(1)), drives: vec![Drive { mate: MateId::from_u128(1), dof: Dof::Z, value: -3.25 * 25.4 }], ..Default::default() },
    );
    assert!((s.poses[0].1.translation[2] / 25.4 - 3.25).abs() < 1e-9);
}

#[test]
fn a_group_keeps_relative_transforms() {
    // A13.3: two grouped instances move as one when the group is mated to a fixed instance.
    let p1 = Pose::rotation_about([5.0, 0.0, 0.0], [0.0, 1.0, 0.0], 0.3).then(&Pose::translation([40.0, 10.0, 0.0]));
    let p2 = Pose::translation([55.0, -20.0, 8.0]);
    let fa = tilted([1.0, 2.0, 3.0]);
    let fb = tilted([0.0, 0.0, 0.0]);
    let m = Mate::new(MateType::Fastened, MateConnector::at(InstanceId::from_u128(1), fa), MateConnector::at(InstanceId::from_u128(3), fb));
    let mut asm = Assembly {
        instances: vec![inst(1, p1, false), inst(2, p2, false), inst(3, Pose::IDENTITY, true)],
        mates: vec![
            feature(1, MateKind::Group { instances: vec![InstanceId::from_u128(1), InstanceId::from_u128(2)] }),
            feature(2, MateKind::Mate(m)),
        ],
        ..Default::default()
    };
    let rel0 = p2.then(&p1.inverse());
    let s = solver::solve(&asm, &frame_of, &SolveOptions { snap: Some(MateId::from_u128(2)), ..Default::default() });
    assert!(s.converged);
    apply(&mut asm, &s);
    let (q1, q2) = (asm.instances[0].pose, asm.instances[1].pose);
    assert!(dist(q1.translation, p1.translation) > 1.0, "the group moved");
    let rel1 = q2.then(&q1.inverse());
    assert!(dist(rel1.translation, rel0.translation) < 1e-9);
    for i in 0..3 {
        for j in 0..3 {
            assert!((rel1.rotation[i][j] - rel0.rotation[i][j]).abs() < 1e-12);
        }
    }
    let dofs = solver::dof_counts(&asm, &frame_of);
    assert_eq!(dofs[&InstanceId::from_u128(2)], 0, "grouped and fastened: no DOF");
    // A13.4: without the mate the group still has 6 DOF.
    asm.mates.truncate(1);
    assert_eq!(solver::dof_counts(&asm, &frame_of)[&InstanceId::from_u128(1)], 6);
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
            .then(&Pose::translation([0.3 * (k % 3) as f64, -0.2 * (k % 5) as f64, 0.1 * (k % 4) as f64]));
        instances.push(inst(k + 1, if k == 0 { Pose::IDENTITY } else { jiggle.then(&Pose::translation([20.0 * k as f64, 0.0, 0.0])) }, k == 0));
        if k > 0 {
            // The joint half way between them, axis Z.
            let joint = |x: f64| ConnectorFrame::new([x, 0.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
            let m = Mate::new(
                MateType::Revolute,
                MateConnector::at(InstanceId::from_u128(k + 1), joint(-10.0)),
                MateConnector::at(InstanceId::from_u128(k), joint(10.0)),
            );
            mates.push(feature(k, MateKind::Mate(m)));
        }
    }
    let asm = Assembly { instances, mates, ..Default::default() };
    let t = std::time::Instant::now();
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    let ms = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("50-instance chain: {ms:.1} ms, {} iterations, residual {:e}", s.iterations, s.residual);
    assert!(s.converged, "{}", s.residual);
    assert!(ms < 50.0, "{ms} ms");
    // Every joint holds.
    let mut solved = asm.clone();
    apply(&mut solved, &s);
    for f in &solved.mates {
        let m = f.mate().unwrap();
        assert!(dist(world(&solved, &m.connectors[0]).origin, world(&solved, &m.connectors[1]).origin) < 1e-6);
    }
}

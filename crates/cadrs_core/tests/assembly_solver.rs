//! The assembly solver (P3B.2, `intro-to-assemblies.md` A6.7, A6.10, A7.1–A7.4, A13): the DOF
//! each mate type leaves, offsets, limits, groups, and speed.

use cadrs_core::assembly::connector::{ConnectorFrame, MateConnector};
use cadrs_core::assembly::mate::{
    Dof, Mate, MateFeature, MateId, MateKind, MateLimits, MateOffset, MateType, mate_position,
};
use cadrs_core::assembly::positions;
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

/// A frame at `origin` with Z up.
fn flat(origin: [f64; 3]) -> ConnectorFrame {
    ConnectorFrame::new(origin, [0.0, 0.0, 1.0], [1.0, 0.0, 0.0])
}

fn revolute(n: u128, a: (u128, [f64; 3]), b: (u128, [f64; 3])) -> MateFeature {
    feature(
        n,
        MateKind::Mate(Mate::new(
            MateType::Revolute,
            MateConnector::at(InstanceId::from_u128(a.0), flat(a.1)),
            MateConnector::at(InstanceId::from_u128(b.0), flat(b.1)),
        )),
    )
}

#[test]
fn a_new_mate_keeps_the_movers_other_mates() {
    // #312: links A and B (10 mm, ends at 0 and 10 along X) hang from the fixed ground G: A turns
    // about G's origin (Revolute 1), B about A's far end (Revolute 2), both lying along X. A new
    // Revolute joins B's far end to G at (10, 10, 0), closing the triangle. B has another mate,
    // so the pick solves every mate from where the parts are: B turns a quarter about A's end and
    // A stays put. Snapping B onto the new mate alone would tear it off Revolute 2.
    let new = MateId::from_u128(3);
    let mut asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, Pose::IDENTITY, false),
            inst(3, Pose::translation([10.0, 0.0, 0.0]), false),
            inst(4, Pose::translation([50.0, 0.0, 0.0]), false),
        ],
        mates: vec![
            revolute(1, (2, [0.0; 3]), (1, [0.0; 3])),
            revolute(2, (3, [0.0; 3]), (2, [10.0, 0.0, 0.0])),
            revolute(3, (3, [10.0, 0.0, 0.0]), (1, [10.0, 10.0, 0.0])),
        ],
        ..Default::default()
    };
    let b = InstanceId::from_u128(3);
    assert!(solver::mated_elsewhere(&asm, b, new));
    assert!(
        !solver::mated_elsewhere(&asm, InstanceId::from_u128(4), new),
        "no mates"
    );

    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            movers: vec![b],
            ..Default::default()
        },
    );
    assert!(s.converged, "{}", s.residual);
    apply(&mut asm, &s);
    for m in &asm.mates {
        let [c0, c1] = m.mate().unwrap().connectors;
        let d = dist(world(&asm, &c0).origin, world(&asm, &c1).origin);
        assert!(d < 1e-6, "{} off by {d}", m.name);
    }
    let a = asm.instance(InstanceId::from_u128(2)).unwrap().pose;
    assert!(
        dist(a.apply([10.0, 0.0, 0.0]), [10.0, 0.0, 0.0]) < 1e-3,
        "A stays put"
    );

    // The old pick, the new mate's snap alone, puts B's near end at (0, 10, 0): off Revolute 2.
    let mut asm2 = asm.clone();
    asm2.instance_mut(b).unwrap().pose = Pose::translation([10.0, 0.0, 0.0]);
    asm2.instance_mut(InstanceId::from_u128(2)).unwrap().pose = Pose::IDENTITY;
    let s = solver::solve(
        &asm2,
        &frame_of,
        &SolveOptions {
            movers: vec![b],
            snap: Some(new),
            snap_only: true,
            ..Default::default()
        },
    );
    apply(&mut asm2, &s);
    let near = asm2.instance(b).unwrap().pose.apply([0.0; 3]);
    assert!(
        dist(near, [10.0, 0.0, 0.0]) > 5.0,
        "the snap alone tears B off: {near:?}"
    );
}

#[test]
fn a_picks_snap_keeps_the_free_motion_where_it_is() {
    // A Revolute's pick on a part with no other mates snaps it onto the joint with the least
    // movement: the axes meet, but B keeps its 30° turn about them (hold_free) instead of being
    // turned back to line up with A, which on two identical parts puts one exactly on the other.
    let b = InstanceId::from_u128(2);
    let turned = Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], 30f64.to_radians());
    let start = Pose::translation([40.0, 25.0, 7.0]).then(&turned);
    let mut asm = Assembly {
        instances: vec![inst(1, Pose::IDENTITY, true), inst(2, start, false)],
        mates: vec![revolute(1, (1, [10.0, 0.0, 0.0]), (2, [0.0; 3]))],
        ..Default::default()
    };
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            movers: vec![b],
            snap: Some(MateId::from_u128(1)),
            snap_only: true,
            hold_free: true,
            ..Default::default()
        },
    );
    apply(&mut asm, &s);
    let [c0, c1] = asm.mates[0].mate().unwrap().connectors;
    let (f0, f1) = (world(&asm, &c0), world(&asm, &c1));
    assert!(dist(f0.origin, f1.origin) < 1e-6, "on the joint");
    let angle = f1.x[1].atan2(f1.x[0]).to_degrees();
    assert!((angle - 30.0).abs() < 1e-6, "B keeps its 30° turn: {angle}");
}

/// One of three identical links (100 mm between the centres of its round ends, 5 mm thick, lying
/// along X): a connector at a round end's axis (Z up, as a cylinder's axis points), on its top
/// face (Z up) or on its bottom face (Z down: a face's connector points out of it).
fn link_end(i: u128, x: f64) -> MateConnector {
    MateConnector::at(InstanceId::from_u128(i), flat([x, 0.0, 2.5]))
}
fn link_top(i: u128) -> MateConnector {
    MateConnector::at(InstanceId::from_u128(i), flat([50.0, 0.0, 5.0]))
}
fn link_bottom(i: u128) -> MateConnector {
    MateConnector::at(
        InstanceId::from_u128(i),
        ConnectorFrame::new([50.0, 0.0, 0.0], [0.0, 0.0, -1.0], [1.0, 0.0, 0.0]),
    )
}

/// What a mate dialog's pick does (`mate_dialog::run_solve`): a mover with other mates solves
/// every mate from where the parts are; one without is snapped with its free motion held.
fn pick(asm: &mut Assembly, n: u128, t: MateType, mover: MateConnector, other: MateConnector) {
    let id = MateId::from_u128(n);
    let mut f = feature(n, MateKind::Mate(Mate::new(t, mover, other)));
    f.id = id;
    asm.mates.push(f);
    let held = solver::mated_elsewhere(asm, mover.instance, id);
    let opts = if held {
        SolveOptions {
            movers: vec![mover.instance],
            ..Default::default()
        }
    } else {
        SolveOptions {
            movers: vec![mover.instance],
            snap: Some(id),
            snap_only: true,
            hold_free: true,
            ..Default::default()
        }
    };
    let s = solver::solve(asm, &frame_of, &opts);
    assert!(s.converged, "mate {n}: {}", s.residual);
    apply(asm, &s);
}

#[test]
fn a_chain_of_cylindrical_and_planar_mates_doesnt_flip() {
    // Three links: part 1 fixed; part 2 joined to it by a Cylindrical on their left ends and a
    // Planar from part 2's bottom face to part 1's top face (so part 2 lies on part 1, its
    // bottom face against part 1's top: the two faces' connectors point opposite ways); then
    // part 3 the same way on part 2's right end. Opposite axes are a real placement for those
    // mates (the same axis; faces against each other), not a false solution: no solve may flip a
    // part to make them point the same way, which put part 2 into part 1 and moved everything.
    let mut asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, Pose::translation([30.0, 60.0, 0.0]), false),
            inst(3, Pose::translation([60.0, 140.0, 0.0]), false),
        ],
        ..Default::default()
    };
    let b = InstanceId::from_u128(2);
    pick(
        &mut asm,
        1,
        MateType::Cylindrical,
        link_end(2, 0.0),
        link_end(1, 0.0),
    );
    pick(&mut asm, 2, MateType::Planar, link_bottom(2), link_top(1));
    let z = |asm: &Assembly, c: MateConnector| world(asm, &c).origin[2];
    // On top of part 1, the right way up: not turned over into it.
    assert!(
        (z(&asm, link_bottom(2)) - 5.0).abs() < 1e-6,
        "part 2 lies on part 1"
    );
    assert!((z(&asm, link_top(2)) - 10.0).abs() < 1e-6, "part 2 upright");
    let part2 = asm.instance(b).unwrap().pose;
    pick(
        &mut asm,
        3,
        MateType::Cylindrical,
        link_end(3, 0.0),
        link_end(2, 100.0),
    );
    pick(&mut asm, 4, MateType::Planar, link_bottom(3), link_top(2));
    let moved = asm.instance(b).unwrap().pose;
    assert!(
        dist(
            moved.apply([100.0, 0.0, 0.0]),
            part2.apply([100.0, 0.0, 0.0])
        ) < 1e-6
            && dist(moved.apply([0.0, 0.0, 5.0]), part2.apply([0.0, 0.0, 5.0])) < 1e-6,
        "part 2 stays put: {part2:?} → {moved:?}"
    );
    assert!(
        (z(&asm, link_bottom(3)) - 10.0).abs() < 1e-6,
        "part 3 lies on part 2"
    );
    assert!((z(&asm, link_top(3)) - 15.0).abs() < 1e-6, "part 3 upright");
}

#[test]
fn a_chain_of_cylindrical_and_planar_mates_goes_to_its_named_position() {
    // The links of the chain above, each lying on the one before (a Cylindrical about the shared
    // end, a Planar from its bottom face to the top of the one under it: connectors opposite).
    // Applying a named position of the chain bent, from straight, snaps each mate to its values:
    // the snap put a Planar's connectors' axes alike, turning the link over into the one under
    // it, and the solve then couldn't reach the position (it rose off the plane instead).
    let mates = vec![
        feature(
            1,
            MateKind::Mate(Mate::new(
                MateType::Cylindrical,
                link_end(2, 0.0),
                link_end(1, 0.0),
            )),
        ),
        feature(
            2,
            MateKind::Mate(Mate::new(MateType::Planar, link_bottom(2), link_top(1))),
        ),
        feature(
            3,
            MateKind::Mate(Mate::new(
                MateType::Cylindrical,
                link_end(3, 0.0),
                link_end(2, 100.0),
            )),
        ),
        feature(
            4,
            MateKind::Mate(Mate::new(MateType::Planar, link_bottom(3), link_top(2))),
        ),
    ];
    let straight = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, Pose::translation([0.0, 0.0, 5.0]), false),
            inst(3, Pose::translation([100.0, 0.0, 10.0]), false),
        ],
        mates,
        ..Default::default()
    };
    // Bent: link 2 turned 40° about its pin, link 3 another 70° about its.
    let turn = |deg: f64| Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], deg.to_radians());
    let p2 = turn(40.0).then(&Pose::translation([0.0, 0.0, 5.0]));
    let end = p2.apply([100.0, 0.0, 0.0]);
    let p3 = turn(110.0).then(&Pose::translation([end[0], end[1], 10.0]));
    let mut bent = straight.clone();
    bent.instance_mut(InstanceId::from_u128(2)).unwrap().pose = p2;
    bent.instance_mut(InstanceId::from_u128(3)).unwrap().pose = p3;
    assert!(solver::solve(&bent, &frame_of, &SolveOptions::default()).converged);
    let solids = std::collections::HashMap::new();
    let own: Vec<MateId> = straight.mates.iter().map(|f| f.id).collect();
    let position = positions::NamedPosition {
        id: positions::NamedPositionId::new(),
        name: "Bent".into(),
        values: positions::mate_values(&bent, &solids, &own),
        poses: Vec::new(),
    };
    let s = positions::solve_to(&straight, &solids, &position);
    assert!(s.converged, "residual {}", s.residual);
    let mut asm = straight.clone();
    apply(&mut asm, &s);
    for (i, want) in [(2, p2), (3, p3)] {
        let got = asm.instance(InstanceId::from_u128(i)).unwrap().pose;
        for p in [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 5.0]] {
            assert!(
                dist(got.apply(p), want.apply(p)) < 1e-6,
                "link {i}: {got:?}, want {want:?}"
            );
        }
    }
}

#[test]
fn a_mate_that_doesnt_hold_is_found_with_how_far_it_misses() {
    // #193: a Revolute between two fixed links whose ends are 10 mm apart can't hold (nothing
    // can move to meet it); a Planar that holds isn't listed.
    let revolute = feature(
        1,
        MateKind::Mate(Mate::new(
            MateType::Revolute,
            link_end(1, 0.0),
            link_end(2, 0.0),
        )),
    );
    let planar = feature(
        2,
        MateKind::Mate(Mate::new(MateType::Planar, link_bottom(3), link_top(1))),
    );
    let asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, Pose::translation([0.0, 0.0, 10.0]), true),
            inst(3, Pose::translation([100.0, 0.0, 5.0]), false),
        ],
        mates: vec![revolute, planar],
        ..Default::default()
    };
    let misses = solver::unsatisfied(&asm, &frame_of);
    assert_eq!(misses.len(), 1, "{misses:?}");
    assert_eq!(misses[0].0, MateId::from_u128(1));
    assert!((misses[0].1 - 10.0).abs() < 1e-9, "{misses:?}");
}

/// Three links folded into a triangle, each lying on the one before (Cylindrical + Planar), as
/// in #193's report: the loop is closed by a Revolute from link 3's free end to link 1's, which
/// lies a link's thickness below it.
fn closed_triangle() -> (Assembly, Pose, Pose) {
    let turn = |deg: f64| Pose::rotation_about([0.0; 3], [0.0, 0.0, 1.0], deg.to_radians());
    let p2 = turn(60.0).then(&Pose::translation([0.0, 0.0, 5.0]));
    let end = p2.apply([100.0, 0.0, 0.0]);
    let p3 = turn(-60.0).then(&Pose::translation([end[0], end[1], 10.0]));
    let mate = |n: u128, t: MateType, a: MateConnector, b: MateConnector| {
        feature(n, MateKind::Mate(Mate::new(t, a, b)))
    };
    let asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, p2, false),
            inst(3, p3, false),
        ],
        mates: vec![
            mate(1, MateType::Cylindrical, link_end(2, 0.0), link_end(1, 0.0)),
            mate(2, MateType::Planar, link_bottom(2), link_top(1)),
            mate(
                3,
                MateType::Cylindrical,
                link_end(3, 0.0),
                link_end(2, 100.0),
            ),
            mate(4, MateType::Planar, link_bottom(3), link_top(2)),
            mate(
                5,
                MateType::Revolute,
                link_end(3, 100.0),
                link_end(1, 100.0),
            ),
        ],
        ..Default::default()
    };
    (asm, p2, p3)
}

#[test]
fn a_mate_that_cant_hold_breaks_alone_and_the_others_hold() {
    // #193: the solve used to settle on a compromise, the links tilted, that held none of the
    // five mates, and stored it. Now the Revolute breaks (leaving it out lets the rest hold),
    // the others hold and the links stay flat where they were.
    let (asm, p2, p3) = closed_triangle();
    let s = solver::solve(
        &asm,
        &frame_of,
        &SolveOptions {
            movers: vec![InstanceId::from_u128(3)],
            ..Default::default()
        },
    );
    assert!(!s.converged);
    assert_eq!(s.broken, vec![MateId::from_u128(5)]);
    let mut placed = asm.clone();
    apply(&mut placed, &s);
    let misses = solver::unsatisfied(&placed, &frame_of);
    assert_eq!(misses.len(), 1, "{misses:?}");
    assert_eq!(misses[0].0, MateId::from_u128(5));
    assert!((misses[0].1 - 10.0).abs() < 1e-6, "{misses:?}");
    for (i, want) in [(2, p2), (3, p3)] {
        let got = placed.instance(InstanceId::from_u128(i)).unwrap().pose;
        for p in [[0.0, 0.0, 0.0], [100.0, 0.0, 0.0], [0.0, 0.0, 5.0]] {
            assert!(dist(got.apply(p), want.apply(p)) < 1e-6, "link {i} moved");
        }
    }
}

#[test]
fn one_mate_breaks_rather_than_the_two_it_conflicts_with() {
    // Link 2 fastened onto link 1 first, then a Cylindrical and a Planar that put it on top of
    // link 3, 200 mm away. Breaking the Fastened lets both hold; keeping it (the list's order)
    // would break two.
    let mate = |n: u128, t: MateType, a: MateConnector, b: MateConnector| {
        feature(n, MateKind::Mate(Mate::new(t, a, b)))
    };
    let asm = Assembly {
        instances: vec![
            inst(1, Pose::IDENTITY, true),
            inst(2, Pose::translation([50.0, 80.0, 0.0]), false),
            inst(3, Pose::translation([200.0, 0.0, 0.0]), true),
        ],
        mates: vec![
            mate(1, MateType::Fastened, link_end(2, 0.0), link_end(1, 0.0)),
            mate(2, MateType::Cylindrical, link_end(2, 0.0), link_end(3, 0.0)),
            mate(3, MateType::Planar, link_bottom(2), link_top(3)),
        ],
        ..Default::default()
    };
    let s = solver::solve(&asm, &frame_of, &SolveOptions::default());
    assert_eq!(s.broken, vec![MateId::from_u128(1)]);
    let mut placed = asm.clone();
    apply(&mut placed, &s);
    let misses = solver::unsatisfied(&placed, &frame_of);
    assert_eq!(misses.len(), 1, "{misses:?}");
    assert_eq!(misses[0].0, MateId::from_u128(1));
}

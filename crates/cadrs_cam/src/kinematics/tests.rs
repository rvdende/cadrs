use super::samples::*;
use super::*;

const X: P3 = [1.0, 0.0, 0.0];
const Z: P3 = [0.0, 0.0, 1.0];

/// The unit vector at a tilt from Z (degrees) toward the longitude `lon`.
fn leaning(tilt: f64, lon: f64) -> P3 {
    let (t, l) = (tilt.to_radians(), lon.to_radians());
    [t.sin() * l.cos(), t.sin() * l.sin(), t.cos()]
}

fn close(a: &Cl, b: &Cl) -> bool {
    dist(a.tip, b.tip) < 1e-7 && angle_between(a.axis, b.axis) < 1e-6
}

/// Every solution of every location over a block and a cap of tilts puts the tool there.
fn round_trips(chain: &Chain, max_tilt: f64) {
    assert!(chain.is_mill());
    let mut near = chain.home();
    let mut count = 0;
    for tilt in [0.0, 5.0, 30.0, 60.0, 85.0, 100.0] {
        if tilt > max_tilt {
            continue;
        }
        for lon in [0.0, 37.0, 90.0, 145.0, 180.0, 233.0, 270.0, 359.0] {
            for tip in [[0.0, 0.0, 0.0], [40.0, -25.0, 30.0], [-70.0, 55.0, 5.0]] {
                let cl = Cl::new(tip, leaning(tilt, lon));
                let all = chain.solutions(&cl, 80.0, &near).unwrap();
                assert!(!all.is_empty());
                for q in &all {
                    let at = chain.locate(q, 80.0);
                    assert!(close(&at, &cl), "{tilt} {lon} {tip:?}: {at:?} for {q:?}");
                }
                let q = chain.solve(&cl, 80.0, &near).unwrap();
                assert!(chain.beyond(&q).is_none());
                assert!(close(&chain.locate(&q, 80.0), &cl));
                near = q;
                count += 1;
            }
        }
    }
    assert!(count > 50);
}

#[test]
fn a_three_axis_machine_goes_where_its_slides_put_it() {
    let c = Chain::three_axis([300.0, 200.0, 100.0]);
    let cl = Cl::new([12.0, -7.0, 3.0], Z);
    let q = c.solve(&cl, 50.0, &c.home()).unwrap();
    // The tool mount is the origin: the tip is a tool's length below it.
    assert!(dist([q[0], q[1], q[2]], [12.0, -7.0, 53.0]) < 1e-9);
    assert!(close(&c.locate(&q, 50.0), &cl));
    // It cannot lean.
    let err = c
        .solve(&Cl::new([0.0; 3], leaning(10.0, 0.0)), 50.0, &c.home())
        .unwrap_err();
    assert!(matches!(err, IkError::Orientation { tilt } if (tilt - 10.0).abs() < 1e-6));
    // Nor go past its travel.
    let err = c
        .solve(&Cl::new([301.0, 0.0, 0.0], Z), 50.0, &c.home())
        .unwrap_err();
    assert!(
        matches!(err, IkError::Limit { ref joint, .. } if joint == "X"),
        "{err:?}"
    );
}

#[test]
fn a_table_that_moves_carries_the_work_the_other_way() {
    let c = trunnion_ac();
    let q = c
        .solve(&Cl::new([25.0, 0.0, 0.0], Z), 100.0, &c.home())
        .unwrap();
    // The table's X slide moves the table toward −X as it rises, so the tool is over +25.
    assert!((q[c.index("X").unwrap()] - 25.0).abs() < 1e-9, "{q:?}");
    assert!(q[c.index("A").unwrap()].abs() < 1e-9 && q[c.index("C").unwrap()].abs() < 1e-9);
}

#[test]
fn every_arrangement_of_two_rotaries_round_trips() {
    round_trips(&trunnion_ac(), 100.0);
    round_trips(&trunnion_bc(), 100.0);
    round_trips(&head_table(), 100.0);
    round_trips(&head_head(), 100.0);
    round_trips(&nutating(), 90.0);
}

#[test]
fn a_tilted_tool_has_two_solutions_and_an_upright_one_keeps_its_table_angle() {
    let c = trunnion_ac();
    let (a, cc) = (c.index("A").unwrap(), c.index("C").unwrap());
    let all = c
        .solutions(&Cl::new([0.0; 3], leaning(40.0, 20.0)), 80.0, &c.home())
        .unwrap();
    assert_eq!(all.len(), 2);
    // The trunnion tilted one way with the platter turned so, or the other way with the
    // platter half a turn on.
    assert!((all[0][a] + all[1][a]).abs() < 1e-9 && (all[0][a].abs() - 40.0).abs() < 1e-9);
    assert!((((all[0][cc] - all[1][cc]).abs()) - 180.0).abs() < 1e-9);
    // Upright, the platter's angle is free: it stays where it was.
    let mut near = c.home();
    near[cc] = 77.0;
    let q = c.solve(&Cl::new([10.0, 0.0, 0.0], Z), 80.0, &near).unwrap();
    assert!((q[cc] - 77.0).abs() < 1e-9 && q[a].abs() < 1e-9);
}

#[test]
fn a_nutating_table_stops_short_of_what_its_lean_allows() {
    // A 30° lean reaches 60° of tilt and no more.
    let mut c = nutating();
    c.table[2].dir = normalize([0.0, (30f64).to_radians().sin(), (30f64).to_radians().cos()]);
    assert!(
        c.solve(&Cl::new([0.0; 3], leaning(59.0, 10.0)), 80.0, &c.home())
            .is_ok()
    );
    let err = c
        .solve(&Cl::new([0.0; 3], leaning(70.0, 10.0)), 80.0, &c.home())
        .unwrap_err();
    assert!(matches!(err, IkError::Orientation { .. }), "{err:?}");
}

#[test]
fn a_limited_rotary_takes_the_other_solution_or_says_which_axis_ran_out() {
    let mut c = trunnion_ac();
    c.table[2].min = -5.0;
    // Only the tilt toward +A is within travel, whichever side the last position was on.
    let cl = Cl::new([0.0; 3], leaning(50.0, 90.0));
    let (a, mut near) = (c.index("A").unwrap(), c.home());
    near[a] = -4.0;
    let q = c.solve(&cl, 80.0, &near).unwrap();
    assert!((q[a] - 50.0).abs() < 1e-9, "{q:?}");
    c.table[2].max = 30.0;
    let err = c.solve(&cl, 80.0, &near).unwrap_err();
    assert!(
        matches!(err, IkError::Limit { ref joint, .. } if joint == "A"),
        "{err:?}"
    );
}

#[test]
fn a_continuous_rotary_winds_on_and_a_limited_one_comes_back() {
    let c = fourth_axis();
    let a = c.index("A").unwrap();
    // Round and round the bar: the angle keeps rising past 360.
    let mut near = c.home();
    let mut last = 0.0;
    for step in 0..100 {
        let turn = step as f64 * 15.0;
        let cl = Cl::new([step as f64, 0.0, 0.0], turned(Z, X, turn));
        near = c.solve(&cl, 60.0, &near).unwrap();
        assert!(close(&c.locate(&near, 60.0), &cl));
        assert!(
            step == 0 || (near[a] - last).abs() < 15.0 + 1e-9,
            "{step}: {} after {last}",
            near[a]
        );
        last = near[a];
    }
    assert!((last.abs() - 99.0 * 15.0).abs() < 1e-6, "{last}");
    // With stops at ±200 the same angle is found within them.
    let mut c = fourth_axis();
    c.table[0].continuous = false;
    (c.table[0].min, c.table[0].max) = (-200.0, 200.0);
    let mut near = c.home();
    near[a] = 190.0;
    let q = c
        .solve(&Cl::new([0.0; 3], turned(Z, X, -150.0)), 60.0, &near)
        .unwrap();
    assert!(
        q[a].abs() <= 200.0
            && close(
                &c.locate(&q, 60.0),
                &Cl::new([0.0; 3], turned(Z, X, -150.0))
            )
    );
}

#[test]
fn one_rotary_cannot_lean_the_tool_along_its_own_axis() {
    let c = fourth_axis();
    assert!(
        c.solve(&Cl::new([0.0; 3], leaning(30.0, 90.0)), 60.0, &c.home())
            .is_ok()
    );
    let err = c
        .solve(&Cl::new([0.0; 3], leaning(30.0, 0.0)), 60.0, &c.home())
        .unwrap_err();
    assert!(
        matches!(err, IkError::Orientation { tilt } if (tilt - 30.0).abs() < 1e-6),
        "{err:?}"
    );
}

/// The dome of brep-bench's `cnc/five-axis-dome`: a ball mill held square to a ball of radius
/// 40 about the origin. On a trunnion whose two axes meet at the ball's centre, a pass up a
/// meridian is the trunnion turning alone and a pass round the pole the platter turning alone:
/// no slide moves at all.
#[test]
fn a_dome_finished_square_to_it_moves_only_the_rotary_axes() {
    let mut c = trunnion_ac();
    c.table[2].origin = [0.0, 0.0, 0.0];
    c.table[3].origin = [0.0, 0.0, 0.0];
    c.table_mount = Frame::WORLD;
    let n = |lat: f64, lon: f64| leaning(90.0 - lat, lon);
    let at = |lat: f64, lon: f64| Cl::new(scale(n(lat, lon), 40.0), n(lat, lon));
    let (a, cc) = (c.index("A").unwrap(), c.index("C").unwrap());
    let lin = c.linears();
    // Up the meridian at longitude 30°, latitude 3° to 75°.
    let start = c.solve(&at(3.0, 30.0), 80.0, &c.home()).unwrap();
    let mut near = start.clone();
    for k in 1..=72 {
        let lat = 3.0 + k as f64;
        let q = c.solve(&at(lat, 30.0), 80.0, &near).unwrap();
        for i in &lin {
            assert!((q[*i] - start[*i]).abs() < 1e-9, "a slide moved at {lat}");
        }
        assert!(
            (q[cc] - start[cc]).abs() < 1e-9,
            "the platter turned at {lat}"
        );
        assert!(
            ((q[a] - near[a]).abs() - 1.0).abs() < 1e-9,
            "the trunnion at {lat}"
        );
        near = q;
    }
    assert!((near[a].abs() - 15.0).abs() < 1e-9);
    // Round the pole at latitude 80°.
    let start = c.solve(&at(80.0, 0.0), 80.0, &near).unwrap();
    let mut near = start.clone();
    for k in 1..=360 {
        let q = c.solve(&at(80.0, k as f64), 80.0, &near).unwrap();
        for i in &lin {
            assert!((q[*i] - start[*i]).abs() < 1e-9);
        }
        assert!((q[a] - start[a]).abs() < 1e-9);
        assert!(((q[cc] - near[cc]).abs() - 1.0).abs() < 1e-9);
        near = q;
    }
    assert!(((near[cc] - start[cc]).abs() - 360.0).abs() < 1e-6);
}

/// A worm's thread is a screw: a cutter following it along a bar on a rotary axis moves the
/// lead for every turn, and nothing else moves.
#[test]
fn a_thread_on_a_rotary_axis_is_a_slide_and_a_turn_in_step() {
    let c = fourth_axis();
    let (x, y, z, a) = (0, 1, 2, c.index("A").unwrap());
    let lead = 2.0 * std::f64::consts::PI;
    let radius = 10.0;
    let mut near = c.home();
    let mut first = None;
    for k in 0..=720 {
        let turn = k as f64;
        let axis = turned(Z, X, turn);
        let cl = Cl::new(
            add([lead * turn / 360.0, 0.0, 0.0], scale(axis, radius)),
            axis,
        );
        let q = c.solve(&cl, 60.0, &near).unwrap();
        let f = first.get_or_insert(q.clone()).clone();
        assert!((q[y] - f[y]).abs() < 1e-9 && (q[z] - f[z]).abs() < 1e-9);
        assert!(((q[x] - f[x]) - lead * (q[a] - f[a]).abs() / 360.0).abs() < 1e-9);
        near = q;
    }
    assert!(((near[a] - first.unwrap()[a]).abs() - 720.0).abs() < 1e-6);
}

#[test]
fn a_move_is_cut_up_until_the_tip_keeps_to_its_line() {
    for (c, tilt) in [
        (trunnion_ac(), 60.0),
        (head_head(), 60.0),
        (nutating(), 50.0),
    ] {
        let from = Cl::new([-30.0, 10.0, 5.0], leaning(5.0, 0.0));
        let to = Cl::new([30.0, -10.0, 15.0], leaning(tilt, 100.0));
        let q0 = c.solve(&from, 80.0, &c.home()).unwrap();
        let tol = Tolerance {
            tip: 0.01,
            axis: 0.1,
            tcp: false,
        };
        let cut = c.linearize(&from, &q0, &to, 80.0, &tol).unwrap();
        assert!(cut.len() > 4, "{}", cut.len());
        assert!(close(&c.locate(&cut.last().unwrap().q, 80.0), &to));
        assert!(cut.iter().all(|w| !w.leap));
        // Between each pair of positions the tip is within tolerance of the line (checked
        // finer than the cutting-up looked).
        let mut prev = (0.0, q0.clone());
        let mut worst: f64 = 0.0;
        for Waypoint { t, q, .. } in &cut {
            assert!(*t > prev.0);
            for s in 1..8 {
                let u = s as f64 / 8.0;
                let mid: Vec<f64> = prev.1.iter().zip(q).map(|(a, b)| a + (b - a) * u).collect();
                let want = from.toward(&to, prev.0 + (t - prev.0) * u);
                worst = worst.max(dist(c.locate(&mid, 80.0).tip, want.tip));
            }
            prev = (*t, q.clone());
        }
        assert!(worst < 0.012, "{worst}");
        // With the control keeping the tip on the line, far fewer positions do.
        let tcp = c
            .linearize(&from, &q0, &to, 80.0, &Tolerance { tcp: true, ..tol })
            .unwrap();
        assert!(tcp.len() < cut.len());
    }
    // A move that turns nothing is one position.
    let c = trunnion_ac();
    let (from, to) = (Cl::new([0.0; 3], Z), Cl::new([50.0, 20.0, 0.0], Z));
    let q0 = c.solve(&from, 80.0, &c.home()).unwrap();
    assert_eq!(
        c.linearize(&from, &q0, &to, 80.0, &Tolerance::default())
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn an_arm_of_six_turns_is_searched() {
    let c = arm();
    assert!(!c.is_mill());
    // Along a path the search follows from each position to the next.
    let from = Cl::new([-40.0, -60.0, 20.0], Z);
    let to = Cl::new([60.0, 80.0, 60.0], leaning(35.0, 200.0));
    let mut near = c.search(&from, 120.0, &c.home()).unwrap();
    assert!(close(&c.locate(&near, 120.0), &from));
    for k in 1..=40 {
        let cl = from.toward(&to, k as f64 / 40.0);
        let q = c.search(&cl, 120.0, &near).unwrap();
        assert!(c.beyond(&q).is_none());
        assert!(close(&c.locate(&q, 120.0), &cl), "{k}");
        // It follows on: no joint leaps.
        assert!(c.distance(&q, &near) < 40.0, "{k}: {q:?} after {near:?}");
        near = q;
    }
    // Out of reach is said, not stretched for.
    assert_eq!(
        c.search(&Cl::new([2000.0, 0.0, 0.0], Z), 120.0, &near),
        Err(IkError::Unreachable)
    );
}

#[test]
fn poses_move_each_body_with_the_joints_before_it() {
    let c = head_table();
    let mut q = c.home();
    let (x, b, cc) = (
        c.index("X").unwrap(),
        c.index("B").unwrap(),
        c.index("C").unwrap(),
    );
    q[x] = 100.0;
    q[b] = 90.0;
    q[cc] = 90.0;
    let poses = c.poses(&q);
    assert_eq!(poses.len(), c.len());
    // The head's body: slid 100 along X, then turned a quarter about the B line through
    // (100, 0, 450), which swings the gauge point (150 below it) out to +X... or −X by the
    // right hand: about +Y, −Z goes to −X.
    let gauge = poses[b].point([0.0, 0.0, 300.0]);
    assert!(dist(gauge, [-50.0, 0.0, 450.0]) < 1e-9, "{gauge:?}");
    // The platter's: a point on its +X goes to +Y.
    assert!(dist(poses[cc].point([10.0, 0.0, 0.0]), [0.0, 10.0, 0.0]) < 1e-9);
    // And the tool, seen from the platter: its axis lay along −X... which the platter sees
    // a quarter turn back.
    let at = c.locate(&q, 0.0);
    assert!(
        dist(at.axis, [0.0, 1.0, 0.0]) < 1e-9 || dist(at.axis, [0.0, -1.0, 0.0]) < 1e-9,
        "{at:?}"
    );
}

#[test]
fn time_is_set_by_the_slowest_axis() {
    let c = trunnion_ac();
    let mut q = c.home();
    q[c.index("A").unwrap()] = 90.0;
    q[c.index("X").unwrap()] = 100.0;
    // 90° at 7200°/min against 100 mm at 10 000 mm/min.
    assert!((c.least_minutes(&c.home(), &q, true) - 90.0 / 7200.0).abs() < 1e-12);
}

#[test]
fn a_run_is_tracked_move_by_move_and_says_where_the_machine_cannot_follow() {
    let mut c = trunnion_ac();
    (c.table[2].min, c.table[2].max) = (-30.0, 30.0);
    let moves: Vec<(Cl, f64)> = (0..=8)
        .map(|k| {
            (
                Cl::new([k as f64 * 5.0, 0.0, 0.0], leaning(k as f64 * 6.0, 90.0)),
                80.0,
            )
        })
        .collect();
    let track = c.track(&moves, &c.rest());
    assert_eq!(track.rows.len(), moves.len() + 1);
    let a = c.index("A").unwrap();
    // Up to 30° the trunnion follows; past it the moves are listed, the machine shown where
    // it would have to be.
    for (k, (cl, length)) in moves.iter().enumerate() {
        assert!(close(&c.locate(&track.rows[k + 1], *length), cl), "{k}");
    }
    assert_eq!(
        track.issues.iter().map(|i| i.0).collect::<Vec<_>>(),
        vec![6, 7, 8]
    );
    assert!(matches!(&track.issues[0].1, IkError::Limit { joint, .. } if joint == "A"));
    assert!((track.rows[9][a].abs() - 48.0).abs() < 1e-9);
    // Part of the way through a move, the machine is where that part of the move is.
    let mid = moves[2].0.toward(&moves[3].0, 0.5);
    assert!(close(&c.locate(&track.at(&c, 3, &mid, 80.0), 80.0), &mid));
    // A machine with no rotary axis is left where it was by a lean.
    let three = Chain::generic(None);
    let track = three.track(&moves, &three.rest());
    assert_eq!(track.issues.len(), 8);
    assert!(matches!(track.issues[0].1, IkError::Orientation { .. }));
    assert_eq!(track.rows[9], track.rows[1]);
}

/// LinuxCNC's table-rotary/tilting kinematics (`xyzac-trt-kins`, `xyzbc-trt-kins`), written
/// out from its published equations: the joints X Y Z that put the controlled point at `pos`
/// with the table's first rotary at `tilt` and its platter at `c` (degrees). `d` is the
/// offset of the tilting axis's line (dy, dz for an A axis; dx, dz for a B axis) and `con`
/// its sign convention (+1: "conventional directions", the table turning left-handed as an
/// angle rises; −1, its default: right-handed).
fn linuxcnc_trt_inverse(about_y: bool, pos: P3, tilt: f64, c: f64, d: [f64; 2], con: f64) -> P3 {
    let (sa, ca) = tilt.to_radians().sin_cos();
    let (sc, cc) = c.to_radians().sin_cos();
    let [x, y, z] = pos;
    if about_y {
        let (dx, dz) = (d[0], d[1]);
        let dpx = -ca * dx + con * sa * dz + dx;
        let dpz = -con * sa * dx - ca * dz + dz;
        [
            cc * ca * x + con * sc * ca * y - con * sa * z + dpx,
            -con * sc * x + cc * y,
            con * cc * sa * x + sc * sa * y + ca * z + dpz,
        ]
    } else {
        let (dy, dz) = (d[0], d[1]);
        [
            cc * x + con * sc * y,
            -con * sc * ca * x + cc * ca * y + con * sa * z - ca * dy - con * sa * dz + dy,
            sc * sa * x - con * cc * sa * y + ca * z + con * sa * dy - ca * dz + dz,
        ]
    }
}

/// A third opinion on the kinematics: a table-table machine set up as LinuxCNC's sample
/// 5-axis mills are gives the same slides as LinuxCNC's own equations, for either rotary
/// convention, any offset of the tilting axis and any tool length.
#[test]
fn a_trunnion_agrees_with_linuxcncs_published_kinematics() {
    let mut rng = 0x9e37_79b9_7f4a_7c15u64;
    let mut next = |lo: f64, hi: f64| {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        lo + (hi - lo) * ((rng >> 11) as f64 / (1u64 << 53) as f64)
    };
    for about_y in [false, true] {
        for con in [-1.0, 1.0] {
            for _ in 0..50 {
                let (d0, d1, tool) = (next(-40.0, 40.0), next(-80.0, 20.0), next(20.0, 150.0));
                // The tilting axis's line (through its offset, along X or Y) and the platter's
                // (through the origin, along Z); LinuxCNC's default turns the table
                // right-handed as an angle rises, its "conventional directions" left-handed.
                let (line, point) = if about_y {
                    ([0.0, -con, 0.0], [d0, 0.0, d1])
                } else {
                    ([-con, 0.0, 0.0], [0.0, d0, d1])
                };
                let chain = Chain {
                    tool: vec![lin("X", X), lin("Y", [0.0, 1.0, 0.0]), lin("Z", Z)],
                    table: vec![
                        Joint::rotary(if about_y { "B" } else { "A" }, point, line, -120.0, 120.0),
                        Joint::rotary("C", [0.0; 3], [0.0, 0.0, -con], 0.0, 0.0).endless(),
                    ],
                    tool_mount: Frame::WORLD,
                    table_mount: Frame::WORLD,
                };
                let (tilt, c) = (next(-110.0, 110.0), next(-720.0, 720.0));
                let p = [next(-60.0, 60.0), next(-60.0, 60.0), next(-30.0, 60.0)];
                let mut q = chain.rest();
                q[3] = tilt;
                q[4] = c;
                let ours = chain.slide_to(p, tool, &q).unwrap();
                // LinuxCNC's controlled point is the tip with the tool's length added along
                // Z, and its pivot offset carries the tool's length too.
                let theirs = linuxcnc_trt_inverse(
                    about_y,
                    [p[0], p[1], p[2] + tool],
                    tilt,
                    c,
                    [d0, d1 + tool],
                    con,
                );
                assert!(
                    dist([ours[0], ours[1], ours[2]], theirs) < 1e-9,
                    "about_y {about_y} con {con}: {:?} against {theirs:?}",
                    &ours[..3]
                );
                // And forward: at those slides the tip is on the point, the tool upright in
                // the machine.
                let at = chain.locate(&ours, tool);
                assert!(dist(at.tip, p) < 1e-9);
            }
        }
    }
}

//! Tests of the rotary (4-axis) and leaning (5-axis) patterns, on shapes whose answers are
//! known: a cylinder, a worm's thread, a stepped shaft, a dome, a cone's wall, holes round a
//! block.

use super::multiaxis::{Pose, holes_any_axis};
use super::*;
use crate::geom::{Aabb, P3, add, cross, dist, dot, len, normalize, scale, sub};
use crate::kinematics::{angle_between, samples};
use crate::model::{Face, Surface, box_triangles, cylinder_triangles};
use crate::oriented::TriGrid;
use crate::post::reader::read;
use crate::post::{NcOperation, NcProgram, post};
use crate::toolpath::{Coolant, FeedKind, UPRIGHT};
use crate::{Dialect, Machine, PostSettings};

const X: P3 = [1.0, 0.0, 0.0];

/// A cylinder of `radius` along X from `x0` to `x1` (its ends closed).
fn bar(radius: f64, x0: f64, x1: f64) -> Vec<[P3; 3]> {
    // (Made along Z, then its axes turned round: z → x, x → y, y → z.)
    cylinder_triangles([0.0, 0.0], radius, x0, x1, 256)
        .into_iter()
        .map(|t| t.map(|p| [p[2], p[0], p[1]]))
        .collect()
}

fn setup<'a>(model: &'a Model, stock: &'a Stock, tool: &'a Tool, params: &'a Params) -> Input<'a> {
    Input {
        model,
        stock,
        tool,
        machine: MachineKind::Mill,
        params,
        rest: None,
        fixtures: &[],
    }
}

/// The poses a toolpath's cutting moves end at (not its plunges, links or rapids).
fn cuts(tp: &Toolpath) -> Vec<Pose> {
    let mut axis = UPRIGHT;
    let mut out = Vec::new();
    for s in &tp.steps {
        match s {
            Step::Axis(a) => axis = *a,
            Step::Feed {
                to,
                kind: FeedKind::Cut,
                ..
            } => out.push(Pose { tip: *to, axis }),
            _ => {}
        }
    }
    out
}

fn rotary_params(pattern: Pattern, tool: &Tool) -> Params {
    let mut p = Params::new(pattern, tool, MachineKind::Mill);
    p.rotary.axis = Some(([0.0; 3], X));
    p
}

#[test]
fn a_rotary_spiral_keeps_a_ball_on_a_cylinder_all_the_way_along() {
    let model = Model::from_triangles("bar", bar(20.0, 0.0, 60.0));
    let stock = Stock::block(Aabb::new([0.0, -21.0, -21.0], [60.0, 21.0, 21.0]));
    let tool = Tool::new(ToolKind::BallMill, 6.0);
    let mut p = rotary_params(Pattern::Rotary, &tool);
    p.step_over = 2.0;
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    assert!(tp.is_multi_axis());
    let poses = cuts(&tp);
    assert!(poses.len() > 1000, "{}", poses.len());
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for q in &poses {
        // The tool points along the radius through its tip, square to the axis.
        assert!(dot(q.axis, X).abs() < 1e-9);
        let radial = [0.0, q.tip[1], q.tip[2]];
        assert!(angle_between(radial, q.axis) < 1e-6, "{q:?}");
        // Its tip is on the cylinder (a 256-sided one: 0.0015 inside at the flats' middles).
        assert!((len(radial) - 20.0).abs() < 0.01, "{}", len(radial));
        lo = lo.min(q.tip[0]);
        hi = hi.max(q.tip[0]);
    }
    assert!(lo.abs() < 1e-9 && (hi - 60.0).abs() < 1e-9, "{lo} {hi}");
    // Thirty turns for the spiral and a ring at each end: 32 turns of 2π·20.
    let cut: f64 = poses.windows(2).map(|w| dist(w[0].tip, w[1].tip)).sum();
    assert!(
        (cut / (32.0 * std::f64::consts::TAU * 20.0) - 1.0).abs() < 0.01,
        "{cut}"
    );
    // Rapids keep clear: beyond the stock's corner by the clearance.
    let clear = (21f64 * 21.0 * 2.0).sqrt() + p.links.clearance;
    let first = tp.steps.iter().find_map(|s| match s {
        Step::Rapid(q) => Some(*q),
        _ => None,
    });
    assert!((len([0.0, first.unwrap()[1], first.unwrap()[2]]) - clear).abs() < 1e-6);
}

/// The radius of a single-start worm of module 2 at pitch radius 10 (lead 2π): a rack tooth π
/// wide at the pitch line with flanks at 20°, from radius 7.6 to 12, wound round the axis.
fn worm_radius(x: f64, theta_deg: f64) -> f64 {
    let lead = std::f64::consts::TAU;
    let mut xi = (x - lead * theta_deg / 360.0) % lead;
    if xi > lead / 2.0 {
        xi -= lead;
    }
    if xi < -lead / 2.0 {
        xi += lead;
    }
    let slope = 20f64.to_radians().tan();
    (10.0 + (std::f64::consts::FRAC_PI_2 - xi.abs()) / slope).clamp(7.6, 12.0)
}

fn worm(length: f64) -> Vec<[P3; 3]> {
    let (nx, nt) = ((length / 0.04).round() as usize, 120);
    let at = |i: usize, j: usize| {
        let (x, theta) = (length * i as f64 / nx as f64, 360.0 * j as f64 / nt as f64);
        let (s, c) = theta.to_radians().sin_cos();
        let r = worm_radius(x, theta);
        // (Angle 0 is +Z, 90° is −Y... as `RotaryAxis` has it for an axis along X: the
        // radius at 0 is Z and at 90° it is X × Z = −Y.)
        [x, -r * s, r * c]
    };
    let mut t = Vec::new();
    for i in 0..nx {
        for j in 0..nt {
            let (a, b, c, d) = (at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1));
            // Outward: for a cylinder the normal of (a, b, c) must point along the radius.
            t.push([a, c, b]);
            t.push([a, d, c]);
        }
    }
    t
}

#[test]
fn a_worm_is_finished_along_its_thread_without_cutting_into_it() {
    let tris = worm(12.0);
    // (The mesh faces outward.)
    let n = normalize(cross(
        sub(tris[0][1], tris[0][0]),
        sub(tris[0][2], tris[0][0]),
    ));
    assert!(dot(n, [0.0, tris[0][0][1], tris[0][0][2]]) > 0.0);
    let model = Model::from_triangles("worm", tris);
    let stock = Stock::block(Aabb::new([0.0, -12.5, -12.5], [12.0, 12.5, 12.5]));
    let tool = Tool::new(ToolKind::BallMill, 1.0);
    let mut p = rotary_params(Pattern::Rotary, &tool);
    p.rotary.style = RotaryStyle::Thread;
    p.rotary.lead = std::f64::consts::TAU;
    p.rotary.range = Some([1.0, 11.0]);
    p.step_over = 0.4;
    p.tolerance = 0.02;
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    let poses = cuts(&tp);
    assert!(poses.len() > 1500, "{}", poses.len());
    // Nowhere does the ball cut into the worm: points all over it stay outside the thread
    // (to the mesh's own coarseness at the thread's corners).
    let mut deepest: f64 = 0.0;
    let mut lowest = f64::INFINITY;
    for q in &poses {
        let centre = add(q.tip, scale(q.axis, 0.5));
        let side = cross(X, q.axis);
        for (u, v) in [
            (0.0, 0.0),
            (0.35, 0.0),
            (-0.35, 0.0),
            (0.0, 0.35),
            (0.0, -0.35),
            (0.25, 0.25),
            (-0.25, -0.25),
        ] {
            // A point of the ball's lower half.
            let down = (1.0f64 - (u * u + v * v) / 0.25).max(0.0).sqrt() * 0.5;
            let at = add(
                add(centre, scale(X, u)),
                add(scale(side, v), scale(q.axis, -down)),
            );
            let r = (at[1] * at[1] + at[2] * at[2]).sqrt();
            let theta = (-at[1]).atan2(at[2]).to_degrees();
            deepest = deepest.max(worm_radius(at[0], theta) - r);
        }
        lowest = lowest.min((q.tip[1] * q.tip[1] + q.tip[2] * q.tip[2]).sqrt());
    }
    assert!(deepest < 0.06, "{deepest}");
    // It reaches the thread's root (7.6) and its crest (12).
    assert!(lowest < 7.65, "{lowest}");
    // Posted for a rotary axis along X, every feed block is X and A in step at the thread's
    // lead: a turn of the axis for every 2π along it.
    let m = four_axis_machine();
    let nc = post(&program(&tp, &tool), &m, &m.post).unwrap();
    let moves = read(&nc);
    let mut blocks = 0;
    for w in moves.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        if b.rapid || a.xyz.iter().any(|v| v.is_nan()) {
            continue;
        }
        let (dx, da) = (b.xyz[0] - a.xyz[0], b.angle('A') - a.angle('A'));
        // (A plunge moves neither.)
        if dx.abs() > 1e-6 || da.abs() > 1e-6 {
            assert!(
                (dx - std::f64::consts::TAU * da.abs() / 360.0).abs() < 2e-3,
                "line {}: {dx} {da}",
                b.line
            );
            blocks += 1;
        }
    }
    // A helix at one depth is one block: far fewer blocks than places dropped.
    assert!(
        blocks > 10 && blocks < poses.len() / 2,
        "{blocks} blocks for {} places",
        poses.len()
    );
}

fn four_axis_machine() -> Machine {
    let mut m = crate::machine::catalog()
        .into_iter()
        .find(|m| m.kind == MachineKind::Mill)
        .unwrap();
    m.dialect = Dialect::LinuxCnc;
    m.post = PostSettings::for_dialect(Dialect::LinuxCnc, MachineKind::Mill);
    // (Its rotary axis is the line the patterns here turn about: X through the work origin.)
    m.chain = Some(samples::fourth_axis());
    m
}

fn program(tp: &Toolpath, tool: &Tool) -> NcProgram {
    NcProgram {
        name: "Part".into(),
        number: 1,
        offsets: Vec::new(),
        operations: vec![NcOperation {
            name: "Op".into(),
            tool: tool.clone(),
            tool_number: 1,
            toolpath: tp.clone(),
            coolant: Coolant::Off,
            datum: 1,
            stop_before: false,
            force_tool_change: false,
            tilt: None,
        }],
    }
}

#[test]
fn rotary_roughing_steps_down_in_radius_to_the_part_and_its_stock() {
    // A shaft of radius 10 with a collar of radius 16 in its middle, from round stock of 20.
    let mut tris = bar(10.0, 0.0, 60.0);
    tris.extend(bar(16.0, 20.0, 40.0));
    let model = Model::from_triangles("shaft", tris);
    let stock = Stock {
        bounds: Aabb::new([0.0, -20.0, -20.0], [60.0, 20.0, 20.0]),
        triangles: Some(bar(20.0, 0.0, 60.0)),
    };
    let tool = Tool::new(ToolKind::EndMill, 8.0);
    let mut p = rotary_params(Pattern::RotaryRough, &tool);
    p.step_down = 3.0;
    p.wall_stock = 0.5;
    p.floor_stock = 0.5;
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    let poses = cuts(&tp);
    assert!(!poses.is_empty());
    let radius = |q: &Pose| (q.tip[1] * q.tip[1] + q.tip[2] * q.tip[2]).sqrt();
    // Never into the part or the stock it leaves: an end mill over the collar's step rests
    // on the collar while any of it is under the cutter.
    for q in &poses {
        // (Within the cutter's rounded corner of the collar's ends it is somewhere between.)
        let part = if q.tip[0] > 20.0 - 3.9 && q.tip[0] < 40.0 + 3.9 {
            16.0
        } else {
            10.0
        };
        assert!(radius(q) >= part + 0.5 - 0.02, "{q:?}: {}", radius(q));
        assert!(radius(q) <= 20.0 - 3.0 + 1e-6);
    }
    // Levels 17, 14, 11 and the part itself: over the shaft's ends the lowest is 10.5.
    let lowest = poses.iter().map(radius).fold(f64::INFINITY, f64::min);
    assert!((lowest - 10.5).abs() < 0.02, "{lowest}");
    let mut levels: Vec<i64> = poses
        .iter()
        .map(|q| (radius(q) * 100.0).round() as i64)
        .collect();
    levels.sort_unstable();
    levels.dedup();
    for want in [1700, 1650, 1400, 1100, 1050] {
        assert!(
            levels.iter().any(|l| (l - want).abs() <= 2),
            "{want} in {levels:?}"
        );
    }
    // Over the collar nothing is cut below 16.5: no pass at 14 or 11 there.
    assert!(
        poses
            .iter()
            .filter(|q| q.tip[0] > 24.0 && q.tip[0] < 36.0)
            .all(|q| radius(q) > 16.4)
    );
}

/// A dome of radius 30 on a plate, as triangles (the top half of a ball, a box under it).
fn dome() -> Model {
    let n = 40;
    let at = |i: usize, j: usize| {
        let (lat, lon) = (
            std::f64::consts::FRAC_PI_2 * i as f64 / n as f64,
            std::f64::consts::TAU * j as f64 / (4 * n) as f64,
        );
        [
            30.0 * lat.cos() * lon.cos(),
            30.0 * lat.cos() * lon.sin(),
            30.0 * lat.sin(),
        ]
    };
    let mut t = Vec::new();
    for i in 0..n {
        for j in 0..4 * n {
            let (a, b, c, d) = (at(i, j), at(i, j + 1), at(i + 1, j + 1), at(i + 1, j));
            t.push([a, b, c]);
            if i + 1 < n {
                t.push([a, c, d]);
            }
        }
    }
    t.extend(box_triangles(&Aabb::new(
        [-40.0, -40.0, -10.0],
        [40.0, 40.0, 0.0],
    )));
    Model::from_triangles("dome", t)
}

#[test]
fn a_parallel_finish_square_to_a_dome_holds_the_ball_on_it_with_its_axis_along_the_radius() {
    let model = dome();
    let stock = Stock::block(Aabb::new([-40.0, -40.0, -10.0], [40.0, 40.0, 31.0]));
    let tool = Tool::new(ToolKind::BallMill, 8.0);
    let mut p = Params::new(Pattern::Parallel, &tool, MachineKind::Mill);
    p.step_over = 6.0;
    p.tolerance = 0.02;
    p.angle = 0.0;
    p.tilt.mode = AxisMode::Normal;
    p.tilt.max_tilt = 60.0;
    // Only the dome: the passes stop at the plate.
    p.heights.bottom = Height::new(HeightRef::Origin, 0.5);
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    assert!(tp.is_multi_axis());
    let poses = cuts(&tp);
    assert!(poses.len() > 200, "{}", poses.len());
    let mut on_dome = 0;
    let mut most: f64 = 0.0;
    for q in &poses {
        let lean = angle_between(UPRIGHT, q.axis);
        most = most.max(lean);
        assert!(lean <= 60.0 + 1e-6, "{lean}");
        // The ball never cuts into the dome: its centre is a radius off the sphere or more
        // (the dome is faceted: a hair less at the facets' middles).
        let centre = add(q.tip, scale(q.axis, 4.0));
        if centre[2] > 4.5 {
            assert!(len(centre) >= 34.0 - 0.05, "{q:?}: {}", len(centre));
        }
        // Where the tool leans less than its limit it is square to the dome: its axis along
        // the radius through its tip, and its tip on the dome. (Not at the dome's foot: there
        // the plate's normal turns 90° to the dome's at once, and the tool turns at most
        // `MOST_TURN_PER_MM`, so it comes square to the dome a few millimetres up.)
        if lean < 59.0 && q.tip[2] > 20.0 {
            assert!((len(q.tip) - 30.0).abs() < 0.06, "{q:?}: {}", len(q.tip));
            assert!(angle_between(q.axis, normalize(q.tip)) < 4.0, "{q:?}");
            on_dome += 1;
        }
    }
    assert!(on_dome > 40 && most > 55.0, "{on_dome} {most}");
    // Upright, the same pattern is the 3-axis one: no lean at all.
    p.tilt.mode = AxisMode::Fixed;
    assert!(
        !generate(&setup(&model, &stock, &tool, &p))
            .unwrap()
            .is_multi_axis()
    );
}

#[test]
fn a_lead_angle_tips_the_tool_forward_along_the_cut() {
    let model = Model::from_triangles(
        "plate",
        box_triangles(&Aabb::new([0.0, 0.0, -5.0], [60.0, 40.0, 0.0])),
    );
    let stock = Stock::block(Aabb::new([0.0, 0.0, -5.0], [60.0, 40.0, 0.5]));
    let tool = Tool::new(ToolKind::BallMill, 8.0);
    let mut p = Params::new(Pattern::Parallel, &tool, MachineKind::Mill);
    p.angle = 0.0;
    p.cutting_method = CuttingMethod::OneWay;
    p.tilt.mode = AxisMode::Normal;
    p.tilt.lead = 15.0;
    p.tilt.smoothing = 0.0;
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    let poses = cuts(&tp);
    // Along +X on a flat plate: the tool's top leans 15° toward +X, and the ball still
    // touches the plate (its centre a radius above it). (A straight pass is its two ends on
    // the plate; off the plate's edge the tool is upright again.)
    let inner: Vec<&Pose> = poses
        .iter()
        .filter(|q| q.tip[0] > 0.01 && q.tip[0] < 59.0 && q.tip[1] > 8.0 && q.tip[1] < 32.0)
        .collect();
    assert!(inner.len() > 5, "{poses:?}");
    for q in inner {
        assert!(
            (angle_between(UPRIGHT, q.axis) - 15.0).abs() < 1e-6,
            "{q:?}"
        );
        assert!(q.axis[0].abs() > 0.25 && q.axis[1].abs() < 1e-9, "{q:?}");
        let centre = add(q.tip, scale(q.axis, 4.0));
        assert!((centre[2] - 4.0).abs() < 1e-6, "{centre:?}");
    }
}

#[test]
fn holes_at_any_angle_are_each_drilled_down_their_own_axis() {
    // A block with a hole down from its top, one in from its side and one at 45° through
    // its top edge (the holes are faces here; the block itself is left solid).
    let b = Aabb::new([0.0, 0.0, 0.0], [60.0, 40.0, 30.0]);
    let mut model = Model::from_triangles("block", box_triangles(&b));
    let slant = normalize([1.0, 0.0, 1.0]);
    let holes = [
        ([20.0, 20.0, 18.0], [0.0, 0.0, 1.0], 12.0),
        ([52.0, 20.0, 15.0], [1.0, 0.0, 0.0], 8.0),
        (sub([60.0, 10.0, 30.0], scale(slant, 10.0)), slant, 10.0),
    ];
    for (k, (from, axis, depth)) in holes.iter().enumerate() {
        let side = normalize(cross(*axis, [0.3, 0.9, 0.1]));
        let ring = |along: f64| -> Vec<P3> {
            (0..24)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 24.0;
                    let out = add(
                        scale(side, 3.0 * a.cos()),
                        scale(cross(*axis, side), 3.0 * a.sin()),
                    );
                    add(add(*from, scale(*axis, along)), out)
                })
                .collect()
        };
        model.faces.push(Face {
            name: format!("hole {k}"),
            body: 0,
            surface: Surface::Cylinder {
                point: *from,
                axis: *axis,
                radius: 3.0,
                inner: true,
            },
            loops: vec![ring(0.0), ring(*depth)],
            triangles: vec![],
        });
    }
    let stock = Stock::block(b);
    let tool = Tool::new(ToolKind::Drill, 6.0);
    let mut p = Params::new(Pattern::Drill, &tool, MachineKind::Mill);
    p.cycle.any_axis = true;
    let input = setup(&model, &stock, &tool, &p);
    let found = holes_any_axis(&input, &TriGrid::new(&model.triangles));
    assert_eq!(found.len(), 3);
    for h in &found {
        // Entered from outside the block, along the hole.
        assert!(
            holes
                .iter()
                .any(|(_, axis, depth)| angle_between(*axis, h.axis) < 1e-6
                    && (h.depth - depth).abs() < 1e-6),
            "{h:?}"
        );
        assert!((h.diameter - 6.0).abs() < 1e-9);
    }
    let tp = generate(&input).unwrap();
    // Each hole: a plunge along its axis to its depth (and the drill's point beyond, the
    // holes being open at both ends as far as the solid block shows).
    let mut axis = UPRIGHT;
    let mut plunges: Vec<(P3, P3)> = Vec::new();
    let mut at = [0.0; 3];
    for s in &tp.steps {
        match s {
            Step::Axis(a) => axis = *a,
            Step::Rapid(q) => at = *q,
            Step::Feed {
                to,
                kind: FeedKind::Plunge,
                ..
            } => {
                assert!(
                    angle_between(normalize(sub(at, *to)), axis) < 1e-6,
                    "a plunge off its axis"
                );
                plunges.push((*to, axis));
                at = *to;
            }
            Step::Feed { to, .. } => at = *to,
            _ => {}
        }
    }
    assert_eq!(plunges.len(), 3);
    for (from, hole_axis, _) in &holes {
        let (bottom, _) = plunges
            .iter()
            .find(|(_, a)| angle_between(*a, *hole_axis) < 1e-6)
            .unwrap();
        // The tip ends on the hole's line, at or beyond its far end.
        let w = sub(*bottom, *from);
        assert!(len(sub(w, scale(*hole_axis, dot(w, *hole_axis)))) < 1e-6);
        assert!(dot(w, *hole_axis) <= 1e-6, "{bottom:?}");
    }
    // The upright hole first (the tool starts upright), and with the limit at 30° only it.
    assert!(angle_between(plunges[0].1, UPRIGHT) < 1e-6);
    p.tilt.max_tilt = 30.0;
    let only = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    assert!(!only.is_multi_axis());
}

#[test]
fn swarf_runs_the_tools_side_along_a_cones_wall() {
    // A frustum: radius 30 at z 0, 20 at z 25 (its wall leans in 21.8°).
    let n = 96;
    let at = |z: f64, j: usize| {
        let a = std::f64::consts::TAU * j as f64 / n as f64;
        let r = 30.0 - 10.0 * z / 25.0;
        [r * a.cos(), r * a.sin(), z]
    };
    let mut tris = Vec::new();
    for j in 0..n {
        let (a, b, c, d) = (at(0.0, j), at(0.0, j + 1), at(25.0, j + 1), at(25.0, j));
        tris.push([a, b, c]);
        tris.push([a, c, d]);
    }
    let wall: Vec<u32> = (0..tris.len() as u32).collect();
    tris.extend(box_triangles(&Aabb::new(
        [-45.0, -45.0, -5.0],
        [45.0, 45.0, 0.0],
    )));
    let mut model = Model::from_triangles("cone", tris);
    model.faces.push(Face {
        name: "wall".into(),
        body: 0,
        surface: Surface::Other,
        loops: vec![],
        triangles: wall,
    });
    let stock = Stock::block(Aabb::new([-45.0, -45.0, -5.0], [45.0, 45.0, 25.0]));
    let tool = Tool::new(ToolKind::EndMill, 10.0);
    let mut p = Params::new(Pattern::Swarf, &tool, MachineKind::Mill);
    p.drive.faces = vec!["wall".into()];
    p.step_down = 10.0;
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    assert!(tp.warnings.is_empty(), "{:?}", tp.warnings);
    let poses = cuts(&tp);
    assert!(poses.len() > 200);
    let lean = (10f64 / 25.0).atan().to_degrees();
    for q in &poses {
        // The tool leans with the wall, in toward the cone's axis...
        assert!(
            (angle_between(UPRIGHT, q.axis) - lean).abs() < 0.05,
            "{q:?}"
        );
        let out = normalize([q.tip[0], q.tip[1], 0.0]);
        assert!(dot(q.axis, out) < -0.3);
        // ...and its axis is its radius off the wall all the way up: the line through the
        // tip along the axis stays 5 from the cone's surface.
        for up in [0.0, 5.0, 10.0] {
            let c = add(q.tip, scale(q.axis, up));
            let r = (c[0] * c[0] + c[1] * c[1]).sqrt();
            let off = (r - (30.0 - 10.0 * c[2] / 25.0)) * lean.to_radians().cos();
            // (The cone is 96 flats: 0.016 nearer at their middles.)
            assert!((off - 5.0).abs() < 0.03, "{q:?} at {up}: {off}");
        }
    }
    // Three depths: 15 and 5 up the wall, then its bottom edge.
    let mut depths: Vec<i64> = poses
        .iter()
        .map(|q| (q.tip[2] * 10.0).round() as i64)
        .collect();
    depths.sort_unstable();
    depths.dedup();
    assert_eq!(depths.len(), 3, "{depths:?}");
    // Climb milling: round the outside of the cone clockwise seen from above (the wall on
    // the tool's right).
    let turn: f64 = poses
        .windows(2)
        .take(50)
        .map(|w| w[0].tip[0] * w[1].tip[1] - w[0].tip[1] * w[1].tip[0])
        .sum();
    assert!(turn < 0.0);
}

#[test]
fn a_threads_lead_is_read_off_the_parts_helical_edges() {
    use super::rotary::{RotaryAxis, fit_lead};
    use crate::model::Edge;
    let axis = RotaryAxis::new([0.0; 3], X);
    let helix = |lead: f64, radius: f64, turns: f64| -> Vec<P3> {
        (0..=(turns * 72.0) as usize)
            .map(|k| {
                let a = (k as f64 * 5.0).to_radians();
                // (Right-handed about +X: from +Z toward −Y.)
                [
                    lead * k as f64 * 5.0 / 360.0,
                    -radius * a.sin(),
                    radius * a.cos(),
                ]
            })
            .collect()
    };
    let edge = |points: Vec<P3>| Edge {
        name: "e".into(),
        body: 0,
        points,
        circle: None,
    };
    let mut m = Model::from_triangles("worm", worm(12.0));
    // No edges: nothing to read it from, and the pattern says so.
    assert_eq!(fit_lead(&m, &axis), None);
    let stock = Stock::block(Aabb::new([0.0, -12.5, -12.5], [12.0, 12.5, 12.5]));
    let tool = Tool::new(ToolKind::BallMill, 1.0);
    let mut p = rotary_params(Pattern::Rotary, &tool);
    p.rotary.style = RotaryStyle::Thread;
    p.rotary.range = Some([1.0, 11.0]);
    p.step_over = 1.0;
    let err = generate(&setup(&m, &stock, &tool, &p)).unwrap_err();
    assert!(err.contains("type the lead"), "{err}");
    // A ring and a straight edge are no thread; the crest's helix is, and the longest wins.
    m.edges.push(edge(helix(0.0, 12.0, 1.0)));
    m.edges
        .push(edge((0..20).map(|k| [k as f64, 0.0, 12.0]).collect()));
    assert_eq!(fit_lead(&m, &axis), None);
    m.edges.push(edge(helix(5.0, 9.0, 0.5)));
    m.edges.push(edge(helix(std::f64::consts::TAU, 12.0, 1.9)));
    let lead = fit_lead(&m, &axis).unwrap();
    assert!((lead - std::f64::consts::TAU).abs() < 1e-9, "{lead}");
    // With it the thread is followed without the lead typed: X and the angle in step.
    let poses = cuts(&generate(&setup(&m, &stock, &tool, &p)).unwrap());
    let w: Vec<&Pose> = poses.iter().take(40).collect();
    let angle = |q: &Pose| (-q.axis[1]).atan2(q.axis[2]).to_degrees();
    let (dx, da) = (w[20].tip[0] - w[5].tip[0], angle(w[20]) - angle(w[5]));
    assert!((dx - lead * da / 360.0).abs() < 1e-6, "{dx} {da}");
    // A left-hand thread reads negative.
    let left: Vec<P3> = helix(4.0, 10.0, 2.0)
        .into_iter()
        .map(|q| [-q[0], q[1], q[2]])
        .collect();
    let mut l = Model::from_triangles("left", bar(10.0, -8.0, 0.0));
    l.edges.push(edge(left));
    assert!((fit_lead(&l, &axis).unwrap() + 4.0).abs() < 1e-9);
}

#[test]
fn a_leaning_tools_holder_is_kept_off_the_part_and_the_whole_tool_off_the_workholding() {
    use super::multiaxis::Guard;
    use crate::tool::Profile;
    // A plate with a tall block standing on it; a short tool in a fat holder.
    let mut tris = box_triangles(&Aabb::new([0.0, 0.0, -5.0], [80.0, 40.0, 0.0]));
    tris.extend(box_triangles(&Aabb::new(
        [50.0, 0.0, 0.0],
        [80.0, 40.0, 40.0],
    )));
    let model = Model::from_triangles("step", tris);
    let stock = Stock::block(Aabb::new([0.0, 0.0, -5.0], [80.0, 40.0, 40.0]));
    let mut tool = Tool::new(ToolKind::BallMill, 6.0);
    tool.cutting_length = 10.0;
    tool.overall_length = 20.0;
    tool.holder = Some(Profile::cylinder("Holder", 30.0, 40.0));
    let p = Params::new(Pattern::Parallel, &tool, MachineKind::Mill);
    let input = setup(&model, &stock, &tool, &p);
    let mut guard = Guard::new(&input);
    let mut scratch = Vec::new();
    // Upright on the plate, well away from the block: clear.
    let clear = Pose {
        tip: [20.0, 20.0, 0.0],
        axis: UPRIGHT,
    };
    assert_eq!(guard.lift(&clear, &mut scratch), 0.0);
    // Close to the block the holder (radius 15 and a millimetre's clearance, from 20 up)
    // would be in it: the tool backs out until the holder is over the block's top.
    let close = Pose {
        tip: [40.0, 20.0, 0.0],
        axis: UPRIGHT,
    };
    let up = guard.lift(&close, &mut scratch);
    assert!((up - 20.0).abs() < 1e-6, "{up}");
    // Leaning away from the block by 45° the holder swings clear of it.
    let away = Pose {
        tip: [40.0, 20.0, 0.0],
        axis: normalize([-1.0, 0.0, 1.0]),
    };
    assert_eq!(guard.lift(&away, &mut scratch), 0.0);
    // Applied to a pass: the pose that hits is backed out and counted, and clear after.
    let mut poses = vec![clear, close, away];
    guard.apply(&mut poses);
    assert_eq!((guard.hits, poses[0], poses[2]), (1, clear, away));
    assert!((poses[1].tip[2] - 20.0).abs() < 1e-6);
    assert_eq!(guard.lift(&poses[1], &mut scratch), 0.0);
    let mut warnings = Vec::new();
    guard.report(&mut warnings);
    assert!(
        warnings[0].contains("1 places") && warnings[0].contains("20 mm"),
        "{warnings:?}"
    );
    // Workholding: a clamp beside the cut is cleared by the cutter itself too.
    let clamp = box_triangles(&Aabb::new([18.0, 18.0, 0.0], [22.0, 22.0, 8.0]));
    let held = Input {
        fixtures: &clamp,
        ..setup(&model, &stock, &tool, &p)
    };
    let guard = Guard::new(&held);
    let over = guard.lift(&clear, &mut scratch);
    // (The ball rests on the clamp's top edge: a little under its full 8 mm.)
    assert!(over > 7.0 && over <= 8.0 + 1e-9, "{over}");
}

#[test]
fn a_leaning_tool_turns_no_faster_than_its_rate_along_a_pass() {
    use super::multiaxis::{MOST_TURN_PER_MM, limit_turn_rate};
    // A pass whose surface normal flips 90° between two places a millimetre apart.
    let tips: Vec<P3> = (0..21).map(|k| [k as f64 * 0.5, 0.0, 0.0]).collect();
    let mut axes: Vec<P3> = (0..21)
        .map(|k| {
            if k < 10 {
                normalize([-1.0, 0.0, 1.0])
            } else {
                normalize([1.0, 0.0, 1.0])
            }
        })
        .collect();
    limit_turn_rate(&tips, &mut axes, MOST_TURN_PER_MM);
    for w in axes.windows(2) {
        assert!(angle_between(w[0], w[1]) <= MOST_TURN_PER_MM * 0.5 + 1e-9);
    }
    // The pass starts as it leant, and turns over toward the other side after the flip.
    assert!((angle_between(axes[0], normalize([-1.0, 0.0, 1.0]))).abs() < 1e-9);
    assert!(axes[20][0] > 0.0 && axes[9][0] < 0.0);
}

/// What the dome's leaning finish hands on keeps to the turning rate: from one cut to the
/// next the tool turns no faster than [`MOST_TURN_PER_MM`] for the way its ball's centre goes
/// (with a little over for poses backed off over a crest), even over the pole and across the
/// holes' mouths.
#[test]
fn a_leaning_finish_turns_the_tool_no_faster_than_its_rate() {
    use super::multiaxis::MOST_TURN_PER_MM;
    let model = dome();
    let stock = Stock::block(Aabb::new([-40.0, -40.0, -10.0], [40.0, 40.0, 31.0]));
    let tool = Tool::new(ToolKind::BallMill, 8.0);
    let mut p = Params::new(Pattern::Parallel, &tool, MachineKind::Mill);
    p.step_over = 6.0;
    p.tolerance = 0.02;
    p.tilt.mode = AxisMode::Normal;
    p.tilt.max_tilt = 60.0;
    p.heights.bottom = Height::new(HeightRef::Origin, 0.5);
    let tp = generate(&setup(&model, &stock, &tool, &p)).unwrap();
    let poses = cuts(&tp);
    let mut worst: f64 = 0.0;
    for w in poses.windows(2) {
        let (c0, c1) = (
            add(w[0].tip, scale(w[0].axis, 4.0)),
            add(w[1].tip, scale(w[1].axis, 4.0)),
        );
        let turn = angle_between(w[0].axis, w[1].axis);
        // (A pass's end and the next's start are no cut between them.)
        if turn < 0.5 || dist(c0, c1) > 8.0 {
            continue;
        }
        worst = worst.max(turn / dist(c0, c1).max(1e-6));
    }
    assert!(worst <= MOST_TURN_PER_MM * 1.1, "{worst}°/mm");
}

//! Hole making: Drill (drilling, peck drilling, chip breaking and tapping cycles), Bore
//! (helical milling of holes larger than the end mill) and Thread mill (a thread cut by a
//! single-form thread mill on a helix, a pitch a turn).
//!
//! Holes are the model's vertical cylindrical walls the material surrounds
//! ([`crate::model::Model::holes`]), with their exact diameters and depths from the kernel.

use crate::geom::{P2, dist2};
use crate::model::Hole;
use crate::tool::ToolKind;
use crate::toolpath::{CycleKind, DrillCycle, FeedKind, Step, Toolpath};

use super::link::Builder;
use super::{HoleSelect, Input};

/// The holes a toolpath takes, nearest first from the model's corner.
fn pick_holes(input: &Input, bore: bool) -> (Vec<Hole>, Vec<String>) {
    let p = input.params;
    let t = input.tool;
    let all = input.model.holes();
    let mut warnings = Vec::new();
    let near_picked = |h: &Hole| {
        p.drive
            .holes
            .iter()
            .any(|c| dist2(*c, h.center) < h.diameter / 2.0 + 1e-3)
    };
    // The diameter a drill or tap is for.
    let fits = |h: &Hole| -> bool {
        if bore {
            h.diameter > t.diameter + 2.0 * p.wall_stock + 1e-3
        } else if t.kind == ToolKind::Tap {
            // A tapped hole is modelled at the tap's size or its tap-drill size.
            (h.diameter - t.diameter).abs() < 0.05
                || (h.diameter - (t.diameter - t.pitch)).abs() < 0.15
        } else {
            (h.diameter - t.diameter).abs() < 0.05
        }
    };
    let picked_d: Vec<f64> = all
        .iter()
        .filter(|h| near_picked(h))
        .map(|h| h.diameter)
        .collect();
    let mut holes: Vec<Hole> = all
        .iter()
        .filter(|h| match p.cycle.select {
            HoleSelect::MatchingTool => fits(h),
            HoleSelect::All => true,
            HoleSelect::Blind => !h.through,
            HoleSelect::SameDiameter => picked_d.iter().any(|d| (d - h.diameter).abs() < 1e-3),
        })
        .filter(|h| {
            p.drive.holes.is_empty() || p.cycle.select == HoleSelect::SameDiameter || near_picked(h)
        })
        .copied()
        .collect();
    if !bore {
        let small: Vec<&Hole> = holes
            .iter()
            .filter(|h| h.diameter < t.diameter - 1e-3)
            .collect();
        if !small.is_empty() {
            warnings.push(format!(
                "{} hole(s) are smaller than the tool and were left out",
                small.len()
            ));
        }
        holes.retain(|h| h.diameter >= t.diameter - 1e-3);
    }
    // Nearest neighbour from the model's lower-left corner.
    let b = input.model.bounds();
    let mut from: P2 = [b.min[0], b.min[1]];
    let mut ordered = Vec::new();
    while !holes.is_empty() {
        let i = holes
            .iter()
            .enumerate()
            .min_by(|a, b| dist2(a.1.center, from).total_cmp(&dist2(b.1.center, from)))
            .map(|(i, _)| i)
            .unwrap();
        let h = holes.swap_remove(i);
        from = h.center;
        ordered.push(h);
    }
    (ordered, warnings)
}

/// Whether the straight move between two holes at height `z` clears the model and the
/// material still on it (stock not yet faced, what earlier toolpaths left).
fn clear_between(input: &Input, a: P2, b: P2, z: f64) -> bool {
    let n = ((dist2(a, b) / 1.0).ceil() as usize).clamp(2, 400);
    let r = input.tool.radius();
    (0..=n).all(|i| {
        let t = i as f64 / n as f64;
        let p = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        if input.material_top(p, r) >= z - 0.5 {
            return false;
        }
        [[0.0, 0.0], [r, 0.0], [-r, 0.0], [0.0, r], [0.0, -r]]
            .iter()
            .all(|o| {
                input
                    .model
                    .top_at([p[0] + o[0], p[1] + o[1]])
                    .is_none_or(|top| top < z - 0.5)
            })
    })
}

/// Drill: a cycle at each hole.
pub fn drill(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let t = input.tool;
    let (holes, warnings) = pick_holes(input, false);
    if holes.is_empty() {
        return Err(if input.model.holes().is_empty() {
            "The model has no holes".into()
        } else {
            format!(
                "No hole fits the {} (Ø{})",
                t.name,
                crate::tool::fmt_mm(t.diameter)
            )
        });
    }
    let mut b = Builder::new(input);
    b.warnings.extend(warnings);
    let clearance = input.clearance();
    let kind = if t.kind == ToolKind::Tap {
        CycleKind::Tap
    } else if p.cycle.kind == CycleKind::Tap {
        CycleKind::Drill
    } else {
        p.cycle.kind
    };
    let tip = if t.kind == ToolKind::Drill {
        t.radius() / (t.angle.clamp(60.0, 179.0) / 2.0).to_radians().tan()
    } else {
        0.0
    };
    let mut prev: Option<(P2, f64)> = None;
    for h in holes {
        // Over whatever material is left above the hole (stock not yet faced).
        let top = h.top.max(input.material_top(h.center, t.radius()));
        let r_plane = top + p.links.rapid_distance;
        let bottom = if h.through {
            h.bottom - p.cycle.break_through - tip
        } else {
            h.bottom + p.floor_stock
        };
        let retract = if p.cycle.retract_full {
            clearance
        } else {
            r_plane
        };
        // Up to clearance first when the way at the R plane isn't clear.
        match prev {
            None => b.rapid([h.center[0], h.center[1], clearance]),
            Some((c, z)) => {
                let travel = if clear_between(input, c, h.center, z) {
                    z
                } else {
                    clearance
                };
                b.up_to(travel);
                b.rapid([h.center[0], h.center[1], travel.max(r_plane)]);
            }
        }
        let feed = if kind == CycleKind::Tap {
            t.pitch * p.speeds.spindle_rpm
        } else {
            p.speeds.feed
        };
        b.steps.push(Step::Drill(DrillCycle {
            at: h.center,
            r_plane,
            bottom,
            retract,
            feed,
            kind,
            peck: p.cycle.peck,
            dwell: p.cycle.dwell,
            pitch: t.pitch,
        }));
        b.at = Some([h.center[0], h.center[1], retract]);
        prev = Some((h.center, retract));
    }
    Ok(b.finish())
}

/// Bore: helixes down each hole bigger than the end mill, a full circle at the bottom of each:
/// first round the middle (within the tool's radius, so no core is left), then out a step over
/// at a time to the wall; out through the cleared middle.
pub fn bore(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let (holes, warnings) = pick_holes(input, true);
    if holes.is_empty() {
        return Err("No hole is bigger than the tool".into());
    }
    let mut b = Builder::new(input);
    b.warnings.extend(warnings);
    let clearance = input.clearance();
    // Climb: the wall (material outside the circle) on the right, so counter-clockwise.
    let cw = p.direction != super::Direction::Climb;
    let tr = input.tool.radius();
    for h in holes {
        let r = h.diameter / 2.0 - tr - p.wall_stock;
        if r <= 1e-3 {
            continue;
        }
        let c = h.center;
        let top = h.top.max(input.material_top(c, h.diameter / 2.0)) + p.links.feed_distance;
        let bottom = if h.through {
            h.bottom - p.cycle.break_through
        } else {
            h.bottom + p.floor_stock
        };
        // The rings it helixes down: the first within the tool's own radius of the middle, so
        // it clears the middle (no core left standing), then out a step over at a time.
        let mut rings = vec![r.min(tr * 0.8)];
        let step = p.step_over.clamp(0.05, tr * 1.6);
        while *rings.last().unwrap() < r - 1e-9 {
            let next = (rings.last().unwrap() + step).min(r);
            rings.push(next);
        }
        b.up_to(clearance);
        b.rapid([c[0], c[1], clearance]);
        for ring in &rings {
            // (After the first ring, up the cleared middle.)
            b.rapid([c[0], c[1], top]);
            let start = [c[0] + ring, c[1]];
            b.feed([start[0], start[1], top], FeedKind::LeadIn);
            // The helix, a turn per step down.
            let pitch = p.step_down.max(0.05);
            let mut z = top;
            while z > bottom + 1e-9 {
                let next = (z - pitch).max(bottom);
                b.arc([start[0], start[1], next], c, cw, FeedKind::Ramp);
                z = next;
            }
            // A level circle at the bottom, then off the wall into the cleared middle.
            b.arc([start[0], start[1], bottom], c, cw, FeedKind::Cut);
            b.feed([c[0], c[1], bottom], FeedKind::LeadOut);
        }
        b.rapid([c[0], c[1], clearance]);
    }
    if !b.moved() {
        return Err("No hole is bigger than the tool".into());
    }
    Ok(b.finish())
}

/// The major diameter of the thread to mill in a hole: the parameter, else the hole's own
/// diameter when it's modelled at the thread's size, or its diameter plus the pitch when it's
/// the tap drill of a standard metric thread.
fn thread_major(h: &Hole, pitch: f64, set: f64) -> f64 {
    if set > 0.0 {
        return set;
    }
    const SIZES: [f64; 14] = [
        2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0, 12.0, 14.0, 16.0, 20.0, 24.0, 30.0,
    ];
    let up = h.diameter + pitch;
    if SIZES.iter().any(|d| (d - up).abs() < 0.15)
        && !SIZES.iter().any(|d| (d - h.diameter).abs() < 0.05)
    {
        up
    } else {
        h.diameter
    }
}

/// Thread mill: each hole threaded by a helix, a pitch a turn. Climb milling (with the spindle
/// clockwise) runs counter-clockwise: a right-hand thread from the bottom up, a left-hand one
/// from the top down; conventional, or a left-hand tool, the other way round. In from the
/// middle on a half circle, out the same.
pub fn thread_mill(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let t = input.tool;
    if t.pitch <= 0.0 {
        return Err("The thread mill needs a pitch".into());
    }
    let (holes, warnings) = pick_holes(input, true);
    if holes.is_empty() {
        return Err("No hole is bigger than the thread mill".into());
    }
    let mut b = Builder::new(input);
    b.warnings.extend(warnings);
    let clearance = input.clearance();
    let pitch = t.pitch;
    let climb = p.direction == super::Direction::Climb;
    // Climb is counter-clockwise round a hole's wall with the spindle turning clockwise (M3), the
    // other way with a left-hand tool (M4).
    let ccw = climb != (t.hand == crate::tool::Hand::Left);
    // A right-hand thread rises turning counter-clockwise.
    let up = ccw != p.left_hand_thread;
    let mut too_big = 0;
    for h in holes {
        let major = thread_major(&h, pitch, p.thread_diameter);
        let full = major / 2.0 - t.radius();
        let first = h.diameter / 2.0 - t.radius();
        if full <= 0.05 || first <= 0.0 {
            too_big += 1;
            continue;
        }
        let c = h.center;
        // The thread's length: out the bottom of a through hole, short of a blind one's.
        let z_top = h.top;
        let z_bot = if h.through {
            h.bottom - pitch
        } else {
            h.bottom + 1.5 * pitch + p.floor_stock
        };
        if z_bot >= z_top {
            continue;
        }
        let (z0, z1) = if up { (z_bot, z_top) } else { (z_top, z_bot) };
        let top = h.top.max(input.material_top(c, h.diameter / 2.0)) + p.links.rapid_distance;
        b.up_to(clearance);
        b.rapid([c[0], c[1], clearance]);
        b.rapid([c[0], c[1], top]);
        let passes = p.passes.max(1);
        for k in 1..=passes {
            let r = first + (full - first) * k as f64 / passes as f64;
            b.feed([c[0], c[1], z0], FeedKind::Plunge);
            // In on a half circle to the wall's side at +X.
            let s = [c[0] + r, c[1]];
            let mid = [c[0] + r / 2.0, c[1]];
            b.arc([s[0], s[1], z0], mid, !ccw, FeedKind::LeadIn);
            // The helix, half a turn at a time.
            let turns = (z1 - z0).abs() / pitch;
            let halves = (turns * 2.0).floor() as usize;
            let dz = (z1 - z0).signum() * pitch / 2.0;
            let mut z = z0;
            let mut ang: f64 = 0.0;
            for _ in 0..halves {
                z += dz;
                ang += std::f64::consts::PI;
                let q = [c[0] + r * ang.cos(), c[1] + r * ang.sin()];
                b.arc([q[0], q[1], z], c, !ccw, FeedKind::Cut);
            }
            // The rest of the last turn.
            let rest = turns * 2.0 - halves as f64;
            if rest > 1e-6 {
                ang += std::f64::consts::PI * rest;
                let q = [c[0] + r * ang.cos(), c[1] + r * ang.sin()];
                b.arc([q[0], q[1], z1], c, !ccw, FeedKind::Cut);
            }
            // Out to the middle on a half circle.
            let at = b.at.map_or([c[0], c[1]], |a| [a[0], a[1]]);
            let m = [(at[0] + c[0]) / 2.0, (at[1] + c[1]) / 2.0];
            b.arc([c[0], c[1], z1], m, !ccw, FeedKind::LeadOut);
            if k < passes {
                b.rapid([c[0], c[1], top]);
            }
        }
        b.rapid([c[0], c[1], clearance]);
    }
    if too_big > 0 {
        b.warnings.push(format!(
            "{too_big} hole(s) too small for the thread mill were left out"
        ));
    }
    if !b.moved() {
        return Err("No hole could be threaded with this tool".into());
    }
    Ok(b.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::{Aabb, P3};
    use crate::machine::MachineKind;
    use crate::model::{Face, Model, Surface, box_triangles};
    use crate::strategy::{Params, Pattern, Stock, generate};
    use crate::tool::Tool;

    fn plate() -> Model {
        let mut m = Model::from_triangles(
            "plate",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [80.0, 40.0, 10.0])),
        );
        let ring = |cx: f64, cy: f64, r: f64, z: f64| -> Vec<P3> {
            (0..24)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 24.0;
                    [cx + r * a.cos(), cy + r * a.sin(), z]
                })
                .collect()
        };
        for (cx, d, bottom) in [(10.0, 6.0, 0.0), (40.0, 6.0, 4.0), (70.0, 20.0, 0.0)] {
            m.faces.push(Face {
                name: format!("hole{cx}"),
                body: 0,
                surface: Surface::Cylinder {
                    point: [cx, 20.0, 0.0],
                    axis: [0.0, 0.0, 1.0],
                    radius: d / 2.0,
                    inner: true,
                },
                loops: vec![
                    ring(cx, 20.0, d / 2.0, 10.0),
                    ring(cx, 20.0, d / 2.0, bottom),
                ],
                triangles: vec![],
            });
        }
        m
    }

    fn run(pattern: Pattern, tool: Tool) -> Result<Toolpath, String> {
        run_over(pattern, tool, 10.0)
    }

    /// With the stock's top at `stock_top` (the plate's is at 10).
    fn run_over(pattern: Pattern, tool: Tool, stock_top: f64) -> Result<Toolpath, String> {
        let m = plate();
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [80.0, 40.0, stock_top]));
        let p = Params::new(pattern, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: None,
            fixtures: &[],
        };
        generate(&input)
    }

    #[test]
    fn drills_matching_holes_through_and_blind() {
        let tp = run(Pattern::Drill, Tool::new(ToolKind::Drill, 6.0)).unwrap();
        let cycles: Vec<DrillCycle> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Drill(c) => Some(*c),
                _ => None,
            })
            .collect();
        assert_eq!(cycles.len(), 2);
        // Through: past the bottom by the break-through and the tip.
        assert!(cycles[0].bottom < -1.0);
        // Blind: to its bottom.
        assert!((cycles[1].bottom - 4.0).abs() < 1e-9);
        assert!((cycles[0].r_plane - 13.0).abs() < 1e-9);
    }

    #[test]
    fn holes_start_over_the_unfaced_stock() {
        // 4 mm of stock over the plate: nothing rapids below it.
        let tp = run_over(Pattern::Drill, Tool::new(ToolKind::Drill, 6.0), 14.0).unwrap();
        for s in &tp.steps {
            if let Step::Drill(c) = s {
                assert!((c.r_plane - 17.0).abs() < 1e-9, "{c:?}");
            }
        }
        let tp = run_over(Pattern::Bore, Tool::new(ToolKind::EndMill, 10.0), 14.0).unwrap();
        let mut at = [0.0; 3];
        for s in &tp.steps {
            match s {
                Step::Rapid(p) => {
                    assert!(p[2] >= 14.0 + 1e-9, "a rapid into the stock at {p:?}");
                    at = *p;
                }
                Step::Feed { to, .. } | Step::Arc { to, .. } => at = *to,
                _ => {}
            }
        }
        assert!(at[2] > 14.0);
    }

    /// Material an earlier toolpath left standing between two holes (a ridge at 18, over the
    /// holes' R plane at 13): the drill goes up to the clearance height to cross it.
    #[test]
    fn holes_are_travelled_between_over_what_is_left() {
        let m = plate();
        let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [80.0, 40.0, 10.0]));
        let mut rest = crate::heightmap::HeightMap::new([0.0, 0.0], [80.0, 40.0], 0.5);
        for j in 0..rest.ny {
            for i in 0..rest.nx {
                let x = rest.point(i, j)[0];
                let n = rest.index(i, j);
                rest.z[n] = if (22.0..28.0).contains(&x) {
                    18.0
                } else {
                    10.0
                };
            }
        }
        let tool = Tool::new(ToolKind::Drill, 6.0);
        let p = Params::new(Pattern::Drill, &tool, MachineKind::Mill);
        let input = Input {
            model: &m,
            stock: &stock,
            tool: &tool,
            machine: MachineKind::Mill,
            params: &p,
            rest: Some(&rest),
            fixtures: &[],
        };
        let clearance = input.clearance();
        assert!(clearance > 18.0);
        let tp = generate(&input).unwrap();
        let over_second = tp
            .steps
            .iter()
            .find_map(|s| match s {
                Step::Rapid(q) if (q[0] - 40.0).abs() < 1e-9 => Some(*q),
                _ => None,
            })
            .unwrap();
        assert!(over_second[2] >= clearance - 1e-9, "{over_second:?}");
    }

    #[test]
    fn thread_mill_helixes_up_the_big_hole() {
        let mut tool = Tool::new(ToolKind::ThreadMill, 12.0);
        tool.pitch = 2.0;
        let tp = run(Pattern::ThreadMill, tool).unwrap();
        // The Ø20 through hole (modelled at the thread's size): a right-hand thread, climb,
        // from below its bottom up, counter-clockwise, at 10 − 6 = 4 from its axis.
        let arcs: Vec<(P3, [f64; 2], bool, FeedKind)> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Arc {
                    to,
                    center,
                    cw,
                    kind,
                    ..
                } => Some((*to, *center, *cw, *kind)),
                _ => None,
            })
            .collect();
        let cuts: Vec<_> = arcs.iter().filter(|a| a.3 == FeedKind::Cut).collect();
        assert!(cuts.iter().all(|a| !a.2), "counter-clockwise");
        for a in &cuts {
            assert!((dist2([a.0[0], a.0[1]], [70.0, 20.0]) - 4.0).abs() < 1e-9);
        }
        // Rising a pitch a turn (two half turns), from 2 below the bottom to the top.
        assert!((cuts[1].0[2] - cuts[0].0[2] - 1.0).abs() < 1e-9);
        assert!((cuts.first().unwrap().0[2] + 1.0).abs() < 1e-9);
        assert!((cuts.last().unwrap().0[2] - 10.0).abs() < 1e-9);
        // (The two Ø6 holes are too small for it: every turn is round the Ø20.)
    }

    /// The thread's hand and the cutting direction, with either spindle direction: a
    /// right-hand thread always rises turning counter-clockwise (or falls turning clockwise).
    #[test]
    fn thread_hand_and_direction() {
        for left_thread in [false, true] {
            for climb in [true, false] {
                for left_tool in [false, true] {
                    let mut tool = Tool::new(ToolKind::ThreadMill, 12.0);
                    tool.pitch = 2.0;
                    if left_tool {
                        tool.hand = crate::tool::Hand::Left;
                    }
                    let m = plate();
                    let stock = Stock::block(Aabb::new([0.0, 0.0, 0.0], [80.0, 40.0, 10.0]));
                    let mut p = Params::new(Pattern::ThreadMill, &tool, MachineKind::Mill);
                    p.left_hand_thread = left_thread;
                    p.direction = if climb {
                        crate::strategy::Direction::Climb
                    } else {
                        crate::strategy::Direction::Conventional
                    };
                    let input = Input {
                        model: &m,
                        stock: &stock,
                        tool: &tool,
                        machine: MachineKind::Mill,
                        params: &p,
                        rest: None,
                        fixtures: &[],
                    };
                    let tp = generate(&input).unwrap();
                    let cuts: Vec<(P3, bool)> = tp
                        .steps
                        .iter()
                        .filter_map(|s| match s {
                            Step::Arc {
                                to,
                                cw,
                                kind: FeedKind::Cut,
                                ..
                            } => Some((*to, *cw)),
                            _ => None,
                        })
                        .collect();
                    let cw = cuts[0].1;
                    let rising = cuts[1].0[2] > cuts[0].0[2];
                    // Right-hand: rising and counter-clockwise go together.
                    assert_eq!(
                        rising == !cw,
                        !left_thread,
                        "{left_thread} {climb} {left_tool}"
                    );
                    // Climb: counter-clockwise with M3, clockwise with M4.
                    assert_eq!(!cw, climb != left_tool, "{left_thread} {climb} {left_tool}");
                }
            }
        }
    }

    #[test]
    fn thread_size_from_a_tap_drill_hole() {
        let h = |d: f64| Hole {
            center: [0.0, 0.0],
            diameter: d,
            top: 10.0,
            bottom: 0.0,
            through: true,
        };
        // An M10 tap drill (8.5) threads to 10; a hole modelled at M10 stays 10.
        assert!((thread_major(&h(8.5), 1.5, 0.0) - 10.0).abs() < 1e-9);
        assert!((thread_major(&h(10.0), 1.5, 0.0) - 10.0).abs() < 1e-9);
        assert!((thread_major(&h(8.5), 1.5, 9.0) - 9.0).abs() < 1e-9);
    }

    #[test]
    fn no_fitting_hole_is_an_error() {
        assert!(run(Pattern::Drill, Tool::new(ToolKind::Drill, 3.0)).is_err());
    }

    #[test]
    fn bore_helixes_the_big_hole() {
        let tp = run(Pattern::Bore, Tool::new(ToolKind::EndMill, 10.0)).unwrap();
        let arcs: Vec<_> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Arc { to, center, cw, .. } => Some((*to, *center, *cw)),
                _ => None,
            })
            .collect();
        assert!(arcs.len() > 3);
        for (to, c, cw) in &arcs {
            assert_eq!(*c, [70.0, 20.0]);
            assert!(!cw);
            assert!(to[0] > 70.0 && to[0] <= 75.0 + 1e-9);
        }
        // Down through the bottom, the last ring at the wall.
        let last = arcs.last().unwrap().0;
        assert!(last[2] < 0.0 && (last[0] - 75.0).abs() < 1e-9);
    }

    /// A Ø20 hole with a D6: the rings start within the tool's radius of the middle (so the
    /// middle is cut, no core left) and step out by at most the step over to the wall; every
    /// rapid is in the middle (cleared) or above the material.
    #[test]
    fn bore_wide_hole_leaves_no_core() {
        let tool = Tool::new(ToolKind::EndMill, 6.0);
        let tp = run(Pattern::Bore, tool.clone()).unwrap();
        let mut radii: Vec<f64> = tp
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Arc { to, center, .. } if *center == [70.0, 20.0] => {
                    Some(dist2([to[0], to[1]], *center))
                }
                _ => None,
            })
            .collect();
        radii.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
        assert!(radii[0] <= tool.radius(), "{radii:?}");
        assert!((radii.last().unwrap() - 7.0).abs() < 1e-9, "{radii:?}");
        let step = Params::new(Pattern::Bore, &tool, MachineKind::Mill).step_over;
        assert!(
            radii.windows(2).all(|w| w[1] - w[0] <= step + 1e-9),
            "{radii:?}"
        );
        let mut at = [0.0, 0.0, 50.0];
        for s in &tp.steps {
            match s {
                Step::Rapid(p) => {
                    let middle = dist2([p[0], p[1]], [70.0, 20.0]) < 1e-9;
                    assert!(middle || p[2] > 10.0, "a rapid at {p:?} from {at:?}");
                    at = *p;
                }
                Step::Feed { to, .. } | Step::Arc { to, .. } => at = *to,
                _ => {}
            }
        }
    }
}

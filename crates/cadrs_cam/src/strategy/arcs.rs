//! A toolpath's runs along exact circles, as arcs: a profile or an engraving round picked edges
//! that are circles (the model keeps each circular edge's exact circle, [`crate::model::Edge`])
//! runs along those circles offset by the tool (the same centres), but the strategies build
//! their contours as polylines. [`onto_circles`] puts the runs of feed moves that follow such
//! a circle back on it, as [`Step::Arc`]s: about the circle's centre, at its radius, from one
//! circle to the next where they touch (a lobe into a valley: their tangent point). The tool
//! then follows the circles themselves, as the machine does for a G2/G3, and what it cuts is
//! the exact sweep along them.

use crate::geom::{P2, P3};
use crate::toolpath::{FeedKind, Step, Toolpath};

/// A circle a toolpath may run along: its centre and radius (in the toolpath's XY).
pub type Circle = (P2, f64);

/// How far off a circle a contour's points may be and still follow it (mm): the polylines'
/// offsets (`region::ARC_TOLERANCE`) and the edges' own tessellation.
pub const ON_CIRCLE: f64 = 0.02;

/// The toolpath with its runs of feed moves along any of `circles` (each move's ends and
/// middle within [`ON_CIRCLE`] of it) made arcs on them.
pub fn onto_circles(tp: Toolpath, circles: &[Circle]) -> Toolpath {
    if circles.is_empty() {
        return tp;
    }
    let steps = tp.steps;
    // Each step's start position.
    let mut starts: Vec<Option<P3>> = Vec::with_capacity(steps.len());
    let mut at: Option<P3> = None;
    for s in &steps {
        starts.push(at);
        if let Some(p) = end_of(s) {
            at = Some(p);
        }
    }
    // The runs: (first step, last step, circle), steps all feed moves along it.
    let mut runs: Vec<(usize, usize, usize)> = Vec::new();
    let mut i = 0;
    while i < steps.len() {
        let fits = |k: usize, c: usize| -> bool {
            let (Some(a), Step::Feed { to, .. }) = (starts[k], &steps[k]) else {
                return false;
            };
            on_move(a, *to, circles[c])
        };
        let Some(c) = (0..circles.len()).find(|&c| fits(i, c)) else {
            i += 1;
            continue;
        };
        let mut j = i;
        // On while the moves follow it at one feed, turning one way.
        let (feed0, kind0) = feed_of(&steps[i]);
        let mut sense = 0.0;
        while j < steps.len() && fits(j, c) && feed_of(&steps[j]) == (feed0, kind0) {
            let (a, b) = (starts[j].unwrap(), end_of(&steps[j]).unwrap());
            let turn = turn_about(circles[c].0, a, b);
            if sense != 0.0 && turn * sense < 0.0 {
                break;
            }
            if turn != 0.0 {
                sense = turn.signum();
            }
            j += 1;
        }
        if sense == 0.0 {
            i += 1;
            continue;
        }
        runs.push((i, j - 1, c));
        i = j;
    }
    if runs.is_empty() {
        return Toolpath { steps, ..tp };
    }
    // Where each run starts and ends exactly: where it meets the run next to it (one right
    // after the other, on circles that touch or cross there), else its ends put on its circle.
    let mut ends: Vec<(P3, P3)> = runs
        .iter()
        .map(|&(a, b, c)| {
            let (p, q) = (starts[a].unwrap(), end_of(&steps[b]).unwrap());
            (onto(circles[c], p), onto(circles[c], q))
        })
        .collect();
    for k in 1..runs.len() {
        let (prev, next) = (runs[k - 1], runs[k]);
        if prev.1 + 1 != next.0 {
            continue;
        }
        let p = starts[next.0].unwrap();
        if let Some(m) = meeting(circles[prev.2], circles[next.2], [p[0], p[1]]) {
            let q = [m[0], m[1], p[2]];
            ends[k - 1].1 = q;
            ends[k].0 = q;
        }
    }
    // The moves straight up and down over a run's ends (a plunge onto it, a rise off it, the
    // rapids above) moved across with them, so they stay straight up and down.
    let mut steps = steps;
    for (r, &(a, b, _)) in runs.iter().enumerate() {
        let (was, now) = (starts[a].unwrap(), ends[r].0);
        for k in (0..a).rev() {
            if end_of(&steps[k]).is_some() && !over(&mut steps[k], was, now) {
                break;
            }
        }
        let (was, now) = (end_of(&steps[b]).unwrap(), ends[r].1);
        for step in steps.iter_mut().skip(b + 1) {
            if end_of(step).is_some() && !over(step, was, now) {
                break;
            }
        }
    }
    // The steps again: each run as arcs (none over half a turn), the move into it ending
    // where it starts.
    let mut out: Vec<Step> = Vec::with_capacity(steps.len());
    let mut r = 0;
    let mut i = 0;
    while i < steps.len() {
        if r < runs.len() && runs[r].0 == i {
            let (a, b, c) = runs[r];
            let (s, e) = ends[r];
            // The move before ends where the run starts.
            if let Some(last) = out.last_mut() {
                set_end(last, s);
            }
            let (feed, kind) = feed_of(&steps[a]);
            let (centre, radius) = circles[c];
            let sense = turn_about(centre, starts[a].unwrap(), end_of(&steps[b]).unwrap()).signum();
            // The turn from s to e the run's way (the moves' turns summed, s and e exact).
            let total: f64 = (a..=b)
                .map(|k| turn_about(centre, starts[k].unwrap(), end_of(&steps[k]).unwrap()))
                .sum();
            let exact = {
                let t = turn_about(centre, s, e);
                // (The same way round as the moves, whole turns added as they went.)
                let mut t = t;
                while t * sense < 0.0 || (total - t).abs() > std::f64::consts::PI {
                    t += sense * std::f64::consts::TAU;
                    if t.abs() > 4.0 * std::f64::consts::TAU {
                        break;
                    }
                }
                t
            };
            let pieces = (exact.abs() / std::f64::consts::PI - 1e-9).ceil().max(1.0) as usize;
            let a0 = (s[1] - centre[1]).atan2(s[0] - centre[0]);
            for n in 1..=pieces {
                let f = n as f64 / pieces as f64;
                let to = if n == pieces {
                    e
                } else {
                    let ang = a0 + exact * f;
                    [
                        centre[0] + radius * ang.cos(),
                        centre[1] + radius * ang.sin(),
                        s[2] + (e[2] - s[2]) * f,
                    ]
                };
                out.push(Step::Arc {
                    to,
                    center: centre,
                    cw: exact < 0.0,
                    feed,
                    kind,
                });
            }
            i = b + 1;
            r += 1;
            continue;
        }
        out.push(steps[i].clone());
        i += 1;
    }
    Toolpath { steps: out, ..tp }
}

/// The circles that offsetting `circles` (each an edge's: centre, unit normal, radius) by
/// `dist` either way gives, for those square to the toolpath's Z.
pub fn offsets(circles: &[(P3, P3, f64)], dists: &[f64]) -> Vec<Circle> {
    let mut out = Vec::new();
    for &(c, n, r) in circles {
        if n[2].abs() < 1.0 - 1e-9 {
            continue;
        }
        for &d in dists {
            for rr in [r - d, r + d] {
                if rr > ON_CIRCLE * 10.0 {
                    out.push(([c[0], c[1]], rr));
                }
            }
        }
    }
    out
}

fn end_of(s: &Step) -> Option<P3> {
    match s {
        Step::Rapid(p) | Step::Feed { to: p, .. } | Step::Arc { to: p, .. } => Some(*p),
        Step::Drill(c) => Some([c.at[0], c.at[1], c.retract]),
        _ => None,
    }
}

fn feed_of(s: &Step) -> (f64, FeedKind) {
    match s {
        Step::Feed { feed, kind, .. } => (*feed, *kind),
        _ => (0.0, FeedKind::Cut),
    }
}

fn set_end(s: &mut Step, p: P3) {
    match s {
        Step::Rapid(q) | Step::Feed { to: q, .. } => *q = p,
        _ => {}
    }
}

/// A move to over `was` (straight above or below it) moved over `now`: whether it was one.
fn over(s: &mut Step, was: P3, now: P3) -> bool {
    match s {
        Step::Rapid(q) | Step::Feed { to: q, .. }
            if (q[0] - was[0]).abs() <= 1e-9 && (q[1] - was[1]).abs() <= 1e-9 =>
        {
            (q[0], q[1]) = (now[0], now[1]);
            true
        }
        _ => false,
    }
}

fn dist_from(c: P2, p: P3) -> f64 {
    (p[0] - c[0]).hypot(p[1] - c[1])
}

/// Whether the move from `a` to `b` follows the circle: both ends and its middle near it, and
/// it goes somewhere across XY.
fn on_move(a: P3, b: P3, (c, r): Circle) -> bool {
    let m = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0, 0.0];
    let moved = (b[0] - a[0]).hypot(b[1] - a[1]) > 1e-9;
    moved
        && [a, b, m]
            .iter()
            .all(|p| (dist_from(c, *p) - r).abs() <= ON_CIRCLE)
}

/// The turn (radians, anticlockwise positive) from `a` to `b` about `c`.
fn turn_about(c: P2, a: P3, b: P3) -> f64 {
    let (u, v) = ([a[0] - c[0], a[1] - c[1]], [b[0] - c[0], b[1] - c[1]]);
    (u[0] * v[1] - u[1] * v[0]).atan2(u[0] * v[0] + u[1] * v[1])
}

/// `p` put on the circle (straight out from its centre), its height kept.
fn onto((c, r): Circle, p: P3) -> P3 {
    let d = dist_from(c, p);
    if d < 1e-12 {
        return p;
    }
    [
        c[0] + (p[0] - c[0]) * r / d,
        c[1] + (p[1] - c[1]) * r / d,
        p[2],
    ]
}

/// Where two circles meet near `p`: their tangent point where they touch (within
/// [`ON_CIRCLE`]), else the crossing nearer `p`; `None` when they don't meet near it.
fn meeting((c0, r0): Circle, (c1, r1): Circle, p: P2) -> Option<P2> {
    let d = (c1[0] - c0[0]).hypot(c1[1] - c0[1]);
    if d < 1e-12 {
        return None;
    }
    let u = [(c1[0] - c0[0]) / d, (c1[1] - c0[1]) / d];
    let near = |q: P2| ((q[0] - p[0]).hypot(q[1] - p[1]) <= 1.0).then_some(q);
    // Touching outside each other, or one inside the other.
    if (d - (r0 + r1)).abs() <= ON_CIRCLE {
        return near([c0[0] + u[0] * r0, c0[1] + u[1] * r0]);
    }
    if (d - (r0 - r1).abs()).abs() <= ON_CIRCLE {
        let s = if r0 >= r1 { r0 } else { -r0 };
        return near([c0[0] + u[0] * s, c0[1] + u[1] * s]);
    }
    // Crossing: along the line of centres by `x`, across it by ±`h`.
    let x = (d * d + r0 * r0 - r1 * r1) / (2.0 * d);
    let h2 = r0 * r0 - x * x;
    if h2 < 0.0 {
        return None;
    }
    let h = h2.sqrt();
    let base = [c0[0] + u[0] * x, c0[1] + u[1] * x];
    let (q0, q1) = (
        [base[0] - u[1] * h, base[1] + u[0] * h],
        [base[0] + u[1] * h, base[1] - u[0] * h],
    );
    let dq = |q: P2| (q[0] - p[0]).hypot(q[1] - p[1]);
    near(if dq(q0) <= dq(q1) { q0 } else { q1 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(to: P3) -> Step {
        Step::Feed {
            to,
            feed: 1000.0,
            kind: FeedKind::Cut,
        }
    }

    /// Two tangent circles walked as polylines a hair off them become two arcs on them,
    /// meeting at their exact tangent point.
    #[test]
    fn polylines_along_tangent_circles_become_their_arcs() {
        let (a, b): (Circle, Circle) = (([0.0, 0.0], 10.0), ([15.0, 0.0], 5.0));
        let mut steps = vec![Step::Rapid([0.0, -10.003, 0.0])];
        // A quarter round the first anticlockwise to its tangent point with the second, then
        // half round the second clockwise.
        for i in 1..=40 {
            let t = -std::f64::consts::FRAC_PI_2 + std::f64::consts::FRAC_PI_2 * i as f64 / 40.0;
            let r = 10.0 + 0.004 * (i as f64 * 0.7).sin();
            steps.push(feed([r * t.cos(), r * t.sin(), 0.0]));
        }
        for i in 1..=40 {
            let t = std::f64::consts::PI - std::f64::consts::PI * i as f64 / 40.0;
            let r = 5.0 - 0.003 * (i as f64 * 0.3).cos();
            steps.push(feed([15.0 + r * t.cos(), r * t.sin(), 0.0]));
        }
        let out = onto_circles(
            Toolpath {
                steps,
                warnings: vec![],
            },
            &[a, b],
        );
        let arcs: Vec<(P3, P2, bool)> = out
            .steps
            .iter()
            .filter_map(|s| match s {
                Step::Arc { to, center, cw, .. } => Some((*to, *center, *cw)),
                _ => None,
            })
            .collect();
        // A quarter about the first, then half round the second (one arc: half a turn).
        assert_eq!(arcs.len(), 2, "{:?}", out.steps);
        assert!(matches!(out.steps[0], Step::Rapid(p) if (p[1] + 10.0).abs() < 1e-12));
        assert_eq!(arcs[0], ([10.0, 0.0, 0.0], [0.0, 0.0], false));
        assert_eq!(arcs[1].1, [15.0, 0.0]);
        assert!(arcs[1].2);
        assert!((arcs[1].0[0] - 20.0).abs() < 1e-12 && arcs[1].0[1].abs() < 1e-12);
    }
}

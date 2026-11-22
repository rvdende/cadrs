//! Cutting round a rotary axis (4-axis): the part turns under a tool that points at the axis.
//!
//! The part is seen in cylinder coordinates about the axis: how far along it (`x`), the angle
//! round it (`theta`) and the distance from it. At a place (x, theta) the tool comes in along
//! the radius there until it touches the part: the drop-cutter along that radius
//! ([`TriGrid::drop_along`]), which gives the radius its tip rests at. A pass is a line in
//! (x, theta), dropped place by place:
//!
//! - **Spiral**: one line round and round, the step over along the axis each turn.
//! - **Line**: along the axis at one angle after another.
//! - **Circular**: a ring at one place along the axis after another.
//! - **Thread**: helices at a thread's lead, stepped across it, so each runs down the thread
//!   at one depth: a worm or a screw is finished along its own lay.
//!
//! The places of a pass are close enough round the axis that straight moves between them
//! keep to the turning surface within the tolerance; on the machine those moves are a slide
//! and a turn in step, which the post writes as one block where the radius holds.
//!
//! [`rough`] clears the stock down to the part in levels of radius: the same spiral at each
//! level (or, in the Thread style, the same helices at the thread's lead), cut only where the
//! level before left material.

use super::multiaxis::{Clearance, Emitter, Guard, Pose, perpendicular};
use super::{Input, RotaryStyle, finish::offset_cutter};
use crate::geom::{P3, add, cross, dot, len, normalize, scale, sub};
use crate::oriented::TriGrid;
use crate::toolpath::Toolpath;

/// A rotary axis as the patterns use it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RotaryAxis {
    /// A point of the line (where `x` is 0).
    pub point: P3,
    /// Its direction (unit).
    pub dir: P3,
    /// The radius at angle 0 (unit, square to the line).
    pub zero: P3,
    /// The radius at 90°: `dir × zero`.
    pub side: P3,
}

impl RotaryAxis {
    /// The axis through `point` along `dir`, its angle 0 as near the work plane's +Z as a
    /// radius can be.
    pub fn new(point: P3, dir: P3) -> RotaryAxis {
        let dir = normalize(dir);
        let zero = perpendicular(dir);
        RotaryAxis {
            point,
            dir,
            zero,
            side: cross(dir, zero),
        }
    }

    /// The axis an operation turns about: the one set, else the machine's (which the studio
    /// fills in), else the part's length along X through the middle of its section.
    pub fn of(input: &Input) -> RotaryAxis {
        match input.params.rotary.axis {
            Some((point, dir)) if len(dir) > 1e-9 => {
                // Places along it are counted from where it passes the frame's origin, the
                // way the frame's own axis nearest it runs: for an axis along X they are the
                // frame's X.
                let mut d = normalize(dir);
                let k = (0..3)
                    .max_by(|a, b| d[*a].abs().total_cmp(&d[*b].abs()))
                    .unwrap_or(0);
                if d[k] < 0.0 {
                    d = scale(d, -1.0);
                }
                RotaryAxis::new(sub(point, scale(d, dot(point, d))), d)
            }
            _ => {
                let c = input.model.bounds().union(&input.stock.bounds).center();
                RotaryAxis::new([0.0, c[1], c[2]], [1.0, 0.0, 0.0])
            }
        }
    }

    /// The unit radius at the angle `theta` (degrees).
    pub fn radial(&self, theta: f64) -> P3 {
        let (s, c) = theta.to_radians().sin_cos();
        add(scale(self.zero, c), scale(self.side, s))
    }

    /// The point `x` along the axis.
    pub fn at(&self, x: f64) -> P3 {
        add(self.point, scale(self.dir, x))
    }

    /// A point's place along the axis and its distance from it.
    pub fn along_and_radius(&self, q: P3) -> (f64, f64) {
        let w = sub(q, self.point);
        let x = dot(w, self.dir);
        (x, len(sub(w, scale(self.dir, x))))
    }

    /// The pose of a tool at the angle `theta`, its tip `r` out along the radius there from
    /// the place `x`, `offset` across it.
    pub fn pose(&self, x: f64, theta: f64, r: f64, offset: f64) -> Pose {
        let out = self.radial(theta);
        let across = cross(self.dir, out);
        Pose {
            tip: add(add(self.at(x), scale(across, offset)), scale(out, r)),
            axis: out,
        }
    }
}

/// A straight line in (x, theta): where it starts and where it ends.
type Guide = ([f64; 2], [f64; 2]);

/// What the rotary patterns work in: the axis, the part's extent about it and the drops.
struct Turning<'a> {
    input: &'a Input<'a>,
    axis: RotaryAxis,
    grid: TriGrid,
    /// The cutter grown by the stock to leave, and how far to lift what it is dropped to.
    dropped: crate::tool::Tool,
    lift: f64,
    /// The part's extent along the axis and its greatest radius.
    part: ([f64; 2], f64),
    /// The stock's greatest radius.
    stock_radius: f64,
    /// The range along the axis that is cut.
    range: [f64; 2],
}

impl<'a> Turning<'a> {
    fn new(input: &'a Input<'a>) -> Result<Turning<'a>, String> {
        let p = input.params;
        let axis = RotaryAxis::of(input);
        if input.model.triangles.is_empty() {
            return Err("The job has no bodies to machine".into());
        }
        let mut span = [f64::INFINITY, f64::NEG_INFINITY];
        let mut radius: f64 = 0.0;
        for q in input.model.triangles.iter().flatten() {
            let (x, r) = axis.along_and_radius(*q);
            span = [span[0].min(x), span[1].max(x)];
            radius = radius.max(r);
        }
        let stock_points: Vec<P3> = match &input.stock.triangles {
            Some(t) => t.iter().flatten().copied().collect(),
            None => input.stock.bounds.corners().to_vec(),
        };
        let stock_radius = stock_points
            .iter()
            .map(|q| axis.along_and_radius(*q).1)
            .fold(radius, f64::max);
        // Along the axis: what is asked for, else the part's length, kept clear of
        // workholding that reaches over either end (a chuck's jaws, a tailstock).
        let reach = input.tool.radius() + p.collision.holder_clearance.max(0.0);
        let mut range = p.rotary.range.unwrap_or(span);
        if p.rotary.range.is_none() && !input.fixtures.is_empty() {
            let mut held = [f64::INFINITY, f64::NEG_INFINITY];
            for q in input.fixtures.iter().flatten() {
                let x = axis.along_and_radius(*q).0;
                held = [held[0].min(x), held[1].max(x)];
            }
            let middle = (span[0] + span[1]) / 2.0;
            if held[1] < middle {
                range[0] = range[0].max(held[1] + reach);
            } else if held[0] > middle {
                range[1] = range[1].min(held[0] - reach);
            }
        }
        if range[1] <= range[0] {
            return Err(
                "Nothing of the part along the rotary axis is clear of the workholding".into(),
            );
        }
        let (dropped, lift) = offset_cutter(input.tool, p.wall_stock.max(0.0));
        Ok(Turning {
            input,
            axis,
            grid: TriGrid::new(&input.model.triangles),
            dropped,
            lift,
            part: (span, radius),
            stock_radius,
            range,
        })
    }

    /// The radius the tip rests at over the place (x, theta): on the part with its stock to
    /// leave, or at the core where nothing of the part is under the cutter.
    fn drop(&self, x: f64, theta: f64, scratch: &mut Vec<u32>) -> f64 {
        let p = self.input.params;
        let pose = self.axis.pose(x, theta, 0.0, p.rotary.offset);
        let core = p.rotary.core.max(0.0);
        let h = self.grid.drop_along(
            &self.dropped,
            pose.tip,
            pose.axis,
            (-self.part.1 - 1.0, self.part.1 + 1.0),
            f64::NEG_INFINITY,
            scratch,
        );
        let extra = (p.floor_stock - p.wall_stock).max(0.0);
        if h.is_finite() {
            (h + self.lift + extra).max(core)
        } else {
            core
        }
    }

    /// The step in angle (degrees) that keeps a straight move between two places within the
    /// tolerance of the arc at the radius `r`.
    fn angle_step(&self, r: f64) -> f64 {
        let tol = (self.input.params.tolerance * 0.5).max(1e-4);
        let r = r.max(tol * 2.0);
        (2.0 * (1.0 - tol / r).clamp(-1.0, 1.0).acos().to_degrees()).clamp(0.05, 10.0)
    }

    /// The places of a straight line in (x, theta) from `a` to `b`, close enough along it for
    /// the surface and the tolerance, each with the radius the tip rests at.
    fn follow(&self, a: [f64; 2], b: [f64; 2], scratch: &mut Vec<u32>) -> Vec<[f64; 3]> {
        let r = self.part.1.max(1.0);
        let turn = (b[1] - a[1]).abs();
        let along = (b[0] - a[0]).abs();
        let sample = (self.input.tool.radius() / 2.0).clamp(0.05, 1.0);
        // By the arc at the part's greatest radius and by the length along the axis.
        let n = (turn / self.angle_step(r))
            .max((turn.to_radians() * r).hypot(along) / sample)
            .ceil()
            .max(1.0) as usize;
        let mut places: Vec<[f64; 3]> = (0..=n)
            .map(|k| {
                let t = k as f64 / n as f64;
                let (x, theta) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
                [x, theta, self.drop(x, theta, scratch)]
            })
            .collect();
        settle(&mut places, (self.input.params.tolerance * 0.5).max(1e-4));
        places
    }

    fn poses(&self, places: &[[f64; 3]]) -> Vec<Pose> {
        let offset = self.input.params.rotary.offset;
        places
            .iter()
            .map(|q| self.axis.pose(q[0], q[1], q[2], offset))
            .collect()
    }

    /// The material that may stand over a pass's first place: the stock from there out to
    /// its surface. (What was cut before is not counted on: beside a pass the stock may stand
    /// whole, within the tool's width.) The approach feeds in through it.
    fn cover(&self, places: &[[f64; 3]]) -> f64 {
        places
            .first()
            .map_or(0.0, |q| (self.stock_radius - q[2]).max(0.0))
    }

    fn clearance(&self) -> Clearance {
        let mut radius = self.stock_radius;
        for q in self.input.fixtures.iter().flatten() {
            radius = radius.max(self.axis.along_and_radius(*q).1);
        }
        Clearance::Cylinder {
            point: self.axis.point,
            dir: self.axis.dir,
            radius: radius + self.input.params.links.clearance,
        }
    }

    /// The lines in (x, theta) the style covers the part with, in cutting order.
    fn guides(&self) -> Result<Vec<Guide>, String> {
        let p = self.input.params;
        let ro = &p.rotary;
        let [x0, x1] = self.range;
        let (a0, sweep) = (
            ro.start_angle,
            if ro.sweep.abs() < 1e-9 {
                360.0
            } else {
                ro.sweep
            },
        );
        let step = p.step_over.max(1e-3);
        let whole = (sweep.abs() - 360.0).abs() < 1e-9;
        Ok(match ro.style {
            RotaryStyle::Spiral => {
                // A ring at the start, the spiral, a ring at the end: both ends are finished
                // square.
                let turns = ((x1 - x0) / step).max(0.0);
                let end = a0 + 360.0 * turns * sweep.signum();
                vec![
                    ([x0, a0 - 360.0 * sweep.signum()], [x0, a0]),
                    ([x0, a0], [x1, end]),
                    ([x1, end], [x1, end + 360.0 * sweep.signum()]),
                ]
            }
            RotaryStyle::Line => {
                let around = (step / self.part.1.max(step)).to_degrees();
                let n = (sweep.abs() / around).ceil().max(1.0) as usize;
                // (A whole turn's last line would be its first again.)
                let count = if whole { n } else { n + 1 };
                (0..count)
                    .map(|k| {
                        let a = a0 + sweep * k as f64 / n as f64;
                        // Out along the axis and back.
                        if k % 2 == 0 {
                            ([x0, a], [x1, a])
                        } else {
                            ([x1, a], [x0, a])
                        }
                    })
                    .collect()
            }
            RotaryStyle::Circular => {
                let n = ((x1 - x0) / step).ceil().max(1.0) as usize;
                (0..=n)
                    .map(|k| {
                        let x = x0 + (x1 - x0) * k as f64 / n as f64;
                        // (Each ring on from where the last ended: no turning back.)
                        let from = a0 + if whole { sweep * k as f64 } else { 0.0 };
                        if whole || k % 2 == 0 {
                            ([x, from], [x, from + sweep])
                        } else {
                            ([x, a0 + sweep], [x, a0])
                        }
                    })
                    .collect()
            }
            RotaryStyle::Thread => {
                // The lead typed, else the part's own: its helical edges'.
                let signed = if ro.lead.abs() > 1e-6 {
                    if ro.left_hand {
                        -ro.lead.abs()
                    } else {
                        ro.lead.abs()
                    }
                } else {
                    fit_lead(self.input.model, &self.axis).ok_or(
                        "The part has no helical edge to take a thread's lead from: type the \
                         lead (mm a turn)",
                    )?
                };
                let lead = signed.abs();
                // A right-hand thread advances along the axis as the angle rises.
                let hand = signed.signum();
                let n = (lead / step).ceil().max(1.0) as usize;
                let turn = 360.0 * (x1 - x0) / lead;
                (0..n)
                    .map(|k| {
                        // Each helix starts a step further round: a step further across the
                        // thread.
                        let a = a0 + 360.0 * k as f64 / n as f64;
                        ([x0, a], [x1, a + hand * turn])
                    })
                    .collect()
            }
        })
    }
}

/// The lead of the thread on a part (mm a turn; negative: left-hand), from its helical edges:
/// those that keep their distance from the axis while they go round it and along it in step.
/// The edge that goes furthest round decides. `None` when no edge is a helix about the axis.
pub fn fit_lead(model: &crate::model::Model, axis: &RotaryAxis) -> Option<f64> {
    let mut best: Option<(f64, f64)> = None;
    for e in &model.edges {
        if e.points.len() < 8 {
            continue;
        }
        // Each point's place along the axis, distance from it and angle round it, the angle
        // carried on past ±180°.
        let mut turn = 0.0;
        let mut last: Option<f64> = None;
        let mut pts: Vec<(f64, f64, f64)> = Vec::with_capacity(e.points.len());
        for q in &e.points {
            let w = sub(*q, axis.point);
            let x = dot(w, axis.dir);
            let (c, s) = (dot(w, axis.zero), dot(w, axis.side));
            let a = s.atan2(c).to_degrees();
            if let Some(l) = last {
                let mut d = a - l;
                while d > 180.0 {
                    d -= 360.0;
                }
                while d < -180.0 {
                    d += 360.0;
                }
                turn += d;
            }
            last = Some(a);
            pts.push((x, c.hypot(s), turn));
        }
        let n = pts.len() as f64;
        let radius = pts.iter().map(|p| p.1).sum::<f64>() / n;
        let round = turn.abs();
        if radius < 1e-6
            || round < 60.0
            || pts
                .iter()
                .any(|p| (p.1 - radius).abs() > 0.02 * radius.max(1.0))
        {
            continue;
        }
        // The straight line of x against the angle, by least squares.
        let (mt, mx) = (
            pts.iter().map(|p| p.2).sum::<f64>() / n,
            pts.iter().map(|p| p.0).sum::<f64>() / n,
        );
        let (mut stt, mut stx) = (0.0, 0.0);
        for p in &pts {
            stt += (p.2 - mt) * (p.2 - mt);
            stx += (p.2 - mt) * (p.0 - mx);
        }
        if stt < 1e-9 {
            continue;
        }
        let slope = stx / stt;
        let off = pts
            .iter()
            .map(|p| (p.0 - mx - slope * (p.2 - mt)).abs())
            .fold(0.0, f64::max);
        let lead = slope * 360.0;
        // (A ring round the axis is no thread; nor is an edge that wanders off the line.)
        if lead.abs() < 1e-3 || off > 0.02 {
            continue;
        }
        if best.is_none_or(|(r, _)| round > r) {
            best = Some((round, lead));
        }
    }
    best.map(|b| b.1)
}

/// The radii of a line's places (evenly spaced along it) made straight stretch by stretch,
/// within `tol` of what was dropped and never under it: where the radius holds or changes
/// steadily, the places then lie on one line in (x, theta, radius), which on the machine is
/// every axis running straight, so the post writes the stretch as one block. A faceted model
/// would otherwise put a hair's difference in every place.
fn settle(places: &mut [[f64; 3]], tol: f64) {
    let n = places.len();
    if n < 3 {
        return;
    }
    // The places kept: the furthest from the straight line between two kept ones, while any
    // is further than the tolerance (Douglas–Peucker on the radius).
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut spans = vec![(0, n - 1)];
    while let Some((a, b)) = spans.pop() {
        let (ra, rb) = (places[a][2], places[b][2]);
        let mut worst = (0.0, a);
        for (k, p) in places.iter().enumerate().take(b).skip(a + 1) {
            let line = ra + (rb - ra) * (k - a) as f64 / (b - a) as f64;
            let off = (p[2] - line).abs();
            if off > worst.0 {
                worst = (off, k);
            }
        }
        if worst.0 > tol {
            keep[worst.1] = true;
            spans.push((a, worst.1));
            spans.push((worst.1, b));
        }
    }
    // Each stretch lifted so nothing dropped is above it, then the places between put on it.
    let kept: Vec<usize> = (0..n).filter(|k| keep[*k]).collect();
    let over: Vec<f64> = kept
        .windows(2)
        .map(|w| {
            let (a, b) = (w[0], w[1]);
            (a + 1..b)
                .map(|k| {
                    let line = places[a][2]
                        + (places[b][2] - places[a][2]) * (k - a) as f64 / (b - a) as f64;
                    places[k][2] - line
                })
                .fold(0.0, f64::max)
        })
        .collect();
    let raised: Vec<f64> = kept
        .iter()
        .enumerate()
        .map(|(i, k)| {
            let before = if i > 0 { over[i - 1] } else { 0.0 };
            let after = over.get(i).copied().unwrap_or(0.0);
            places[*k][2] + before.max(after)
        })
        .collect();
    for (i, w) in kept.windows(2).enumerate() {
        let (a, b) = (w[0], w[1]);
        for (k, place) in places.iter_mut().enumerate().take(b + 1).skip(a) {
            place[2] = raised[i] + (raised[i + 1] - raised[i]) * (k - a) as f64 / (b - a) as f64;
        }
    }
}

/// Rotary finishing.
pub fn finish(input: &Input) -> Result<Toolpath, String> {
    let t = Turning::new(input)?;
    let guides = t.guides()?;
    let mut e = Emitter::new(input, t.clearance());
    let mut scratch = Vec::new();
    let mut guard = Guard::new(input);
    for (a, b) in guides {
        let places = t.follow(a, b, &mut scratch);
        let mut poses = t.poses(&places);
        guard.apply(&mut poses);
        // (A pass joins the one before where it starts on its end: the spiral's rings.)
        e.pass(&poses, true, t.cover(&places));
    }
    if !e.moved() {
        return Err("Nothing to finish round the axis".into());
    }
    guard.report(&mut e.warnings);
    Ok(e.finish())
}

/// Rotary roughing: the stock cleared down to the part in levels of radius.
pub fn rough(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let t = Turning::new(input)?;
    let mut scratch = Vec::new();
    // The lines every level follows at its own radius, dropped once: one spiral over the
    // range, or, along a thread, the helices at its lead (a tool that fits the thread's space
    // then opens it level by level, instead of riding over it).
    let [x0, x1] = t.range;
    let lines: Vec<Vec<[f64; 3]>> = if p.rotary.style == RotaryStyle::Thread {
        t.guides()?
            .into_iter()
            .map(|(a, b)| t.follow(a, b, &mut scratch))
            .collect()
    } else {
        let turns = ((x1 - x0) / p.step_over.max(1e-3)).max(1.0);
        vec![t.follow([x0, 0.0], [x1, 360.0 * turns], &mut scratch)]
    };
    let lowest = lines
        .iter()
        .flatten()
        .map(|q| q[2])
        .fold(f64::INFINITY, f64::min);
    let mut e = Emitter::new(input, t.clearance());
    let mut guard = Guard::new(input);
    let mut level = t.stock_radius;
    let mut above = f64::INFINITY;
    let mut levels = 0;
    while level > lowest + 1e-6 && levels < 10_000 {
        level = (level - p.step_down.max(1e-3)).max(lowest);
        levels += 1;
        // Cut where this level is below what the level before left: the part (with its
        // stock) where it stands higher than that level, else that level itself.
        let mut run: Vec<[f64; 3]> = Vec::new();
        let mut flush = |run: &mut Vec<[f64; 3]>, e: &mut Emitter| {
            if run.len() > 1 {
                let mut poses = t.poses(run);
                guard.apply(&mut poses);
                e.pass(&poses, false, t.cover(run));
            }
            run.clear();
        };
        // (The part where it stands above the level by no more than the tolerance is cut at the
        // level: the part is seen through its facets, whose sag would otherwise make a pass at
        // a level the part's own radius (with its stock) meets creep by microns, a spiral
        // that isn't one helix.)
        let sag = p.tolerance.max(0.0);
        for line in &lines {
            for q in line {
                let r = if q[2] > level + sag { q[2] } else { level };
                if r < above.min(t.stock_radius) - 1e-6 {
                    run.push([q[0], q[1], r]);
                } else {
                    flush(&mut run, &mut e);
                }
            }
            flush(&mut run, &mut e);
        }
        above = level;
    }
    if !e.moved() {
        return Err("The stock is no bigger round the axis than the part: nothing to rough".into());
    }
    guard.report(&mut e.warnings);
    Ok(e.finish())
}

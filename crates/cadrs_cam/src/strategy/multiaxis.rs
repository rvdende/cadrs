//! Cutting with the tool leaning: what the 4- and 5-axis patterns share, and the 5-axis ones.
//!
//! A multi-axis toolpath is a run of [`Pose`]s: where the tool's tip is and which way the
//! tool points. Between two poses the tip goes straight and the axis turns steadily, so a
//! pattern gives poses close enough for that to follow its surface.
//!
//! - **Links** ([`Emitter`]): between passes the tool backs out along its own axis to a
//!   [`Clearance`] shape (a plane over the part, a cylinder round a rotary axis, a ball round
//!   everything), turns to the next pass's lean out there, comes down that pass's axis and
//!   feeds the last of the way in. Passes that end close to where the next starts are joined
//!   by a feed along the surface.
//! - **The tool's axis** ([`AxisRule`]): the Axis tab's rule (square to the surface, away from
//!   or toward a point, through a line) at the point of contact, then the lead and side tilts,
//!   held within the most the tool may lean and evened out along the pass.
//! - **Multi-axis finishing** ([`tilt`]): any 3-axis finishing pattern with such a rule. The
//!   3-axis passes say where on the surface the tool cuts; at each point the tool is leant by
//!   the rule and dropped down its own axis onto the part ([`TriGrid::drop_along`]), so the
//!   leant path is as free of gouges as the 3-axis one.
//! - **Swarf** ([`swarf`]): the tool's side along the picked walls, its axis along each wall's
//!   own slope, its end on the wall's bottom edge.
//! - **Drilling at any angle** ([`drill`]): every hole down its own axis in one operation.
//! - **The tool's shank and holder** ([`Guard`]): at every pose they are kept off the part,
//!   and the whole tool off the workholding, by backing the tool out along its own axis.

use std::collections::HashMap;

use super::collision::{CollisionAction, assembly_discs};
use super::{AxisControl, AxisMode, Direction, HoleSelect, Input, finish::offset_cutter};
use crate::geom::{Aabb, P3, add, cross, dist, dot, len, lerp, normalize, scale, sub};
use crate::kinematics::{angle_between, slerp, turned};
use crate::model::Surface;
use crate::oriented::TriGrid;
use crate::toolpath::{CycleKind, FeedKind, Step, Toolpath, UPRIGHT};

/// Where the tool's tip is and the unit vector up its axis, from the tip toward the spindle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub tip: P3,
    pub axis: P3,
}

/// Where the tool travels between passes: clear of the part, the stock and the workholding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Clearance {
    /// The plane at this height.
    Plane(f64),
    /// The cylinder of this radius round a line (a rotary axis).
    Cylinder { point: P3, dir: P3, radius: f64 },
    /// The ball of this radius.
    Sphere { center: P3, radius: f64 },
}

impl Clearance {
    /// The shape for a pattern whose tool leans at most `lean` degrees from the work plane's
    /// Z: the usual clearance plane while the tool stays well above the horizontal, else a
    /// ball round the part, the stock and the workholding.
    pub fn over(input: &Input, lean: f64) -> Clearance {
        if lean <= 60.0 {
            return Clearance::Plane(input.clearance());
        }
        let b = everything(input);
        Clearance::Sphere {
            center: b.center(),
            radius: len(b.size()) / 2.0 + input.params.links.clearance,
        }
    }

    /// Where the tip is once the tool has backed out along its axis to the shape.
    pub fn lift(&self, p: &Pose) -> P3 {
        // The far root of |w + s·a|² = r² (0 when already outside).
        let out = |w: P3, a: P3, r: f64| -> Option<f64> {
            let (qa, qb, qc) = (dot(a, a), 2.0 * dot(w, a), dot(w, w) - r * r);
            if qa < 1e-12 {
                return None;
            }
            let disc = (qb * qb - 4.0 * qa * qc).max(0.0);
            Some(((-qb + disc.sqrt()) / (2.0 * qa)).max(0.0))
        };
        match *self {
            Clearance::Plane(z) => {
                if p.axis[2] > 0.2 {
                    add(p.tip, scale(p.axis, ((z - p.tip[2]) / p.axis[2]).max(0.0)))
                } else {
                    // (Too flat to reach the plane along its axis: straight up.)
                    [p.tip[0], p.tip[1], z.max(p.tip[2])]
                }
            }
            Clearance::Cylinder { point, dir, radius } => {
                let w = sub(p.tip, point);
                let across = |v: P3| sub(v, scale(dir, dot(v, dir)));
                match out(across(w), across(p.axis), radius) {
                    Some(s) => add(p.tip, scale(p.axis, s)),
                    // (The tool along the axis: straight out from it.)
                    None => {
                        let r = across(w);
                        add(p.tip, scale(normalize(r), (radius - len(r)).max(0.0)))
                    }
                }
            }
            Clearance::Sphere { center, radius } => {
                let s = out(sub(p.tip, center), p.axis, radius).unwrap_or(0.0);
                add(p.tip, scale(p.axis, s))
            }
        }
    }

    /// The poses on the way from one lifted pose to another along the shape (neither end):
    /// the tool turning no more than 30° between two.
    pub fn across(&self, from: &Pose, to: &Pose) -> Vec<Pose> {
        let lean = angle_between(from.axis, to.axis);
        match *self {
            Clearance::Plane(_) => {
                let n = (lean / 30.0).ceil().max(1.0) as usize;
                (1..n)
                    .map(|k| {
                        let t = k as f64 / n as f64;
                        Pose {
                            tip: lerp(from.tip, to.tip, t),
                            axis: slerp(from.axis, to.axis, t),
                        }
                    })
                    .collect()
            }
            Clearance::Cylinder { point, dir, radius } => {
                let zero = normalize(perpendicular(dir));
                let side = cross(dir, zero);
                let cyl = |q: P3| {
                    let w = sub(q, point);
                    (dot(w, dir), dot(w, side).atan2(dot(w, zero)))
                };
                let ((x0, a0), (x1, a1)) = (cyl(from.tip), cyl(to.tip));
                let mut turn = a1 - a0;
                while turn > std::f64::consts::PI {
                    turn -= std::f64::consts::TAU;
                }
                while turn < -std::f64::consts::PI {
                    turn += std::f64::consts::TAU;
                }
                let n = (turn.abs().to_degrees() / 15.0)
                    .max(lean / 30.0)
                    .ceil()
                    .max(1.0) as usize;
                (1..n)
                    .map(|k| {
                        let t = k as f64 / n as f64;
                        let a = a0 + turn * t;
                        let out = add(scale(zero, a.cos()), scale(side, a.sin()));
                        Pose {
                            tip: add(
                                add(point, scale(dir, x0 + (x1 - x0) * t)),
                                scale(out, radius),
                            ),
                            axis: slerp(from.axis, to.axis, t),
                        }
                    })
                    .collect()
            }
            Clearance::Sphere { center, radius } => {
                let (u0, u1) = (
                    normalize(sub(from.tip, center)),
                    normalize(sub(to.tip, center)),
                );
                let n = (angle_between(u0, u1) / 15.0)
                    .max(lean / 30.0)
                    .ceil()
                    .max(1.0) as usize;
                (1..n)
                    .map(|k| {
                        let t = k as f64 / n as f64;
                        Pose {
                            tip: add(center, scale(slerp(u0, u1, t), radius)),
                            axis: slerp(from.axis, to.axis, t),
                        }
                    })
                    .collect()
            }
        }
    }
}

/// A unit vector square to `d`.
pub fn perpendicular(d: P3) -> P3 {
    let helper = if d[2].abs() < 0.9 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, 0.0, 0.0]
    };
    normalize(sub(helper, scale(d, dot(helper, d))))
}

/// The box round the part, the stock and the workholding.
pub fn everything(input: &Input) -> Aabb {
    let mut b = input.stock.bounds.union(&input.model.bounds());
    for p in input.fixtures.iter().flatten() {
        b.include(*p);
    }
    b
}

/// How far along its axis from the tip a pose leaves the stock's box (0 outside it): the
/// material that may stand over a cut the stock has not been cleared from.
pub fn stock_cover(input: &Input, p: &Pose) -> f64 {
    let b = &input.stock.bounds;
    if !b.contains(p.tip) {
        return 0.0;
    }
    (0..3)
        .filter_map(|k| {
            if p.axis[k] > 1e-9 {
                Some((b.max[k] - p.tip[k]) / p.axis[k])
            } else if p.axis[k] < -1e-9 {
                Some((b.min[k] - p.tip[k]) / p.axis[k])
            } else {
                None
            }
        })
        .fold(f64::INFINITY, f64::min)
        .max(0.0)
}

/// Writes passes of poses as steps, with the links between them.
pub struct Emitter<'a> {
    pub input: &'a Input<'a>,
    pub clear: Clearance,
    pub steps: Vec<Step>,
    pub warnings: Vec<String>,
    at: Option<Pose>,
    /// The lean last written ([`Step::Axis`]).
    lean: P3,
    /// The material that may stand over the cut the tool is in (what its approach fed in
    /// through): it leaves through as much at feed before it rapids.
    cover: f64,
}

impl<'a> Emitter<'a> {
    /// An emitter that starts the spindle.
    pub fn new(input: &'a Input<'a>, clear: Clearance) -> Emitter<'a> {
        let p = input.params;
        let mut steps = Vec::new();
        if !p.posting.comment.is_empty() {
            steps.push(Step::Comment(p.posting.comment.clone()));
        }
        steps.push(Step::Spindle {
            rpm: p.speeds.spindle_rpm,
            cw: true,
        });
        Emitter {
            input,
            clear,
            steps,
            warnings: Vec::new(),
            at: None,
            lean: UPRIGHT,
            cover: 0.0,
        }
    }

    /// Out of the cut along the tool's axis to the clearance shape: through whatever may
    /// stand over the cut at feed, then at rapid. Where the tool ends, lifted.
    fn leave(&mut self) -> Option<Pose> {
        let cur = self.at?;
        let out = Pose {
            tip: self.clear.lift(&cur),
            axis: cur.axis,
        };
        let through = self.cover.min(dist(out.tip, cur.tip));
        if through > 1e-6 {
            self.feed(
                Pose {
                    tip: add(cur.tip, scale(cur.axis, through)),
                    axis: cur.axis,
                },
                FeedKind::Retract,
            );
        }
        self.cover = 0.0;
        self.rapid(out);
        Some(out)
    }

    fn turn_to(&mut self, axis: P3) {
        if dist(axis, self.lean) > 1e-12 {
            self.steps.push(Step::Axis(axis));
            self.lean = axis;
        }
    }

    pub fn rapid(&mut self, p: Pose) {
        if self
            .at
            .is_some_and(|a| dist(a.tip, p.tip) < 1e-9 && dist(a.axis, p.axis) < 1e-12)
        {
            return;
        }
        self.turn_to(p.axis);
        self.steps.push(Step::Rapid(p.tip));
        self.at = Some(p);
    }

    pub fn feed(&mut self, p: Pose, kind: FeedKind) {
        if self
            .at
            .is_some_and(|a| dist(a.tip, p.tip) < 1e-9 && dist(a.axis, p.axis) < 1e-12)
        {
            return;
        }
        let s = &self.input.params.speeds;
        let feed = match kind {
            FeedKind::Cut | FeedKind::Link => s.feed,
            FeedKind::Plunge => s.plunge,
            FeedKind::Ramp => s.ramp,
            FeedKind::LeadIn | FeedKind::LeadOut => s.lead,
            FeedKind::Retract => s.plunge.max(s.feed),
        };
        self.turn_to(p.axis);
        self.steps.push(Step::Feed {
            to: p.tip,
            feed,
            kind,
        });
        self.at = Some(p);
    }

    /// The tool brought to `first`, ready to cut: from where it is, by a feed when `join` and it
    /// is already there (a pass that starts where the last ended), else out along its
    /// axis, across the clearance shape and down `first`'s axis, the last `cover` (the
    /// material that may stand over the cut) and the feed distance at the plunge feed.
    pub fn approach(&mut self, first: Pose, join: bool, cover: f64) {
        let links = self.input.params.links;
        if let Some(cur) = self.at {
            // (Only where the next pass starts where this one ended: a straight feed across
            // any gap would cut a chord through whatever curves between.)
            if join
                && dist(cur.tip, first.tip) <= 0.05
                && angle_between(cur.axis, first.axis) <= 1.0
            {
                self.feed(first, FeedKind::Link);
                return;
            }
            let out = self.leave().expect("in a cut");
            let over = Pose {
                tip: self.clear.lift(&first),
                axis: first.axis,
            };
            for p in self.clear.across(&out, &over) {
                self.rapid(p);
            }
            self.rapid(over);
        } else {
            self.rapid(Pose {
                tip: self.clear.lift(&first),
                axis: first.axis,
            });
        }
        let up = cover.max(0.0) + links.feed_distance.max(0.1);
        let above = add(first.tip, scale(first.axis, up));
        // (Never a rapid further out than the clearance it came from.)
        let lifted = self.clear.lift(&first);
        if dist(lifted, first.tip) > up {
            self.rapid(Pose {
                tip: above,
                axis: first.axis,
            });
        }
        self.feed(first, FeedKind::Plunge);
        self.cover = cover.max(0.0);
    }

    /// A pass: the approach to its first pose, then a cut through the rest.
    pub fn pass(&mut self, poses: &[Pose], join: bool, cover: f64) {
        let Some(first) = poses.first() else {
            return;
        };
        self.approach(*first, join, cover);
        for p in &poses[1..] {
            self.feed(*p, FeedKind::Cut);
        }
    }

    /// Out of the cut along the tool's axis to the clearance shape.
    pub fn retract(&mut self) {
        self.leave();
    }

    pub fn moved(&self) -> bool {
        self.at.is_some()
    }

    /// Ends clear of the part.
    pub fn finish(mut self) -> Toolpath {
        self.retract();
        Toolpath {
            steps: self.steps,
            warnings: self.warnings,
        }
    }
}

/// Keeps a leaning tool's shank and holder off the part and the whole tool off the
/// workholding (the Collision tab, for toolpaths whose tool leans): each pose is tested by
/// dropping the tool assembly's sections down the tool's own axis, and backed out along it
/// as far as they need, or only counted when the tab says to warn.
pub struct Guard {
    part: Option<TriGrid>,
    fixtures: Option<TriGrid>,
    /// The shank, holder and arbor as flat cutters of their radii (with the clearances), each
    /// with the height above the tip it starts at.
    sections: Vec<(crate::tool::Tool, f64)>,
    tool: crate::tool::Tool,
    retract: bool,
    reach: f64,
    /// The poses that would have hit, and the furthest one had to back out.
    pub hits: usize,
    pub most: f64,
}

impl Guard {
    pub fn new(input: &Input) -> Guard {
        let c = input.params.collision;
        let sections = assembly_discs(input.tool, &c)
            .into_iter()
            .map(|(radius, height)| {
                (
                    crate::tool::Tool::new(crate::tool::ToolKind::EndMill, 2.0 * radius.max(1e-3)),
                    height,
                )
            })
            .collect();
        Guard {
            part: (c.check && !input.model.triangles.is_empty())
                .then(|| TriGrid::new(&input.model.triangles)),
            fixtures: (!input.fixtures.is_empty()).then(|| TriGrid::new(input.fixtures)),
            sections,
            tool: input.tool.clone(),
            retract: c.action == CollisionAction::Retract,
            reach: len(everything(input).size()) + input.tool.gauge_length(),
            hits: 0,
            most: 0.0,
        }
    }

    /// How far up its axis the tool at `pose` must back out for its shank and holder to clear
    /// the part and all of it to clear the workholding (0: clear as it is).
    pub fn lift(&self, pose: &Pose, scratch: &mut Vec<u32>) -> f64 {
        let mut up: f64 = 0.0;
        let sections = |grid: &TriGrid, up: &mut f64, scratch: &mut Vec<u32>| {
            for (section, height) in &self.sections {
                let from = add(pose.tip, scale(pose.axis, *height));
                *up = up.max(grid.drop_along(
                    section,
                    from,
                    pose.axis,
                    (0.0, self.reach),
                    0.0,
                    scratch,
                ));
            }
        };
        if let Some(part) = &self.part {
            sections(part, &mut up, scratch);
        }
        if let Some(fixtures) = &self.fixtures {
            sections(fixtures, &mut up, scratch);
            // The cutter too: nothing of the tool touches the workholding.
            up = up.max(fixtures.drop_along(
                &self.tool,
                pose.tip,
                pose.axis,
                (0.0, self.reach),
                0.0,
                scratch,
            ));
        }
        up
    }

    /// Tests a pass's poses, backing out those that would hit (when the tab says to).
    pub fn apply(&mut self, poses: &mut [Pose]) {
        if self.part.is_none() && self.fixtures.is_none() {
            return;
        }
        let mut scratch = Vec::new();
        for p in poses.iter_mut() {
            let up = self.lift(p, &mut scratch);
            if up > 1e-6 {
                self.hits += 1;
                self.most = self.most.max(up);
                if self.retract {
                    p.tip = add(p.tip, scale(p.axis, up));
                }
            }
        }
    }

    /// What it found, for the toolpath's warnings.
    pub fn report(&self, warnings: &mut Vec<String>) {
        if self.hits == 0 {
            return;
        }
        let most = crate::tool::fmt_mm(self.most);
        warnings.push(if self.retract {
            format!(
                "The tool's shank or holder would hit at {} places: the tool backs out along \
                 its axis there, by up to {most} mm, and leaves that much uncut",
                self.hits
            )
        } else {
            format!(
                "The tool's shank or holder hits at {} places, by up to {most} mm",
                self.hits
            )
        });
    }
}

/// The lean (degrees from the work plane's Z) under which a leaning tool stands upright
/// ([`AxisRule::within`]).
pub const POLE: f64 = 3.0;

/// The tool's axis by the Axis tab's rule.
pub struct AxisRule<'a> {
    pub control: &'a AxisControl,
}

impl AxisRule<'_> {
    /// The axis where the tool touches the surface at `contact` (its unit normal `normal`),
    /// cutting along the unit vector `feed`.
    pub fn at(&self, contact: P3, normal: P3, feed: P3) -> P3 {
        let c = self.control;
        let mut a = match c.mode {
            AxisMode::Fixed => UPRIGHT,
            AxisMode::Normal => normal,
            AxisMode::FromPoint(o) => normalize(sub(contact, o)),
            AxisMode::TowardPoint(o) => normalize(sub(o, contact)),
            AxisMode::ThroughLine { point, dir } => {
                let d = normalize(dir);
                let w = sub(contact, point);
                normalize(sub(w, scale(d, dot(w, d))))
            }
        };
        if len(a) < 0.5 {
            a = UPRIGHT;
        }
        // The lead and side tilts, about the cut's direction square to the axis.
        let along = sub(feed, scale(a, dot(feed, a)));
        if len(along) > 1e-9 && (c.lead != 0.0 || c.side != 0.0) {
            let along = normalize(along);
            // (About a × f the axis turns toward f; about f it turns to the cut's right.)
            a = turned(a, normalize(cross(a, along)), c.lead);
            a = normalize(turned(a, along, -c.side));
        }
        self.within(a)
    }

    /// The axis held within the most the tool may lean from the work plane's Z.
    ///
    /// Near upright the lean is eased off to none: within [`POLE`] of the work plane's Z the
    /// tool stands upright, and up to twice that it leans twice as far past the cone's edge.
    /// That is where a machine's rotary axis about Z (a table's C, a head's C) has to turn half
    /// a turn in a few millimetres to follow a lean that passes beside it; upright, it can
    /// turn while nothing changes, and the lean comes back without a jump.
    pub fn within(&self, a: P3) -> P3 {
        let lean = angle_between(UPRIGHT, a);
        let most = self.control.max_tilt.clamp(0.0, 180.0);
        let eased = if lean < POLE {
            0.0
        } else if lean < 2.0 * POLE {
            2.0 * (lean - POLE)
        } else {
            lean
        };
        let target = eased.min(most);
        if lean < 1e-12 || (target - lean).abs() < 1e-12 {
            a
        } else {
            slerp(UPRIGHT, a, target / lean)
        }
    }
}

/// The axes of a pass evened out: each the mean of those within `span` mm of cut either side
/// of it (a faceted surface's normals jump from facet to facet).
pub fn smooth_axes(tips: &[P3], axes: &mut [P3], span: f64) {
    if span <= 0.0 || axes.len() < 3 {
        return;
    }
    let mut s = vec![0.0; tips.len()];
    for i in 1..tips.len() {
        s[i] = s[i - 1] + dist(tips[i - 1], tips[i]);
    }
    let raw = axes.to_vec();
    let (mut lo, mut hi) = (0, 0);
    for i in 0..raw.len() {
        while s[i] - s[lo] > span {
            lo += 1;
        }
        while hi + 1 < raw.len() && s[hi + 1] - s[i] <= span {
            hi += 1;
        }
        let sum = raw[lo..=hi].iter().fold([0.0; 3], |m, a| add(m, *a));
        if len(sum) > 1e-9 {
            axes[i] = normalize(sum);
        }
    }
}

/// The most the tool's axis turns over a stretch of a pass made one move ([`simplify`]): a
/// pass over the top of a dome leans one way, stands upright and leans the other, and the
/// upright poses (where a machine turns its free rotary axis) must not be merged away into a
/// move whose ends lean opposite ways.
const MOST_TURN_MERGED: f64 = 2.0 * POLE;

/// The most the tool's lean changes for a millimetre of a pass (degrees): where the surface's
/// normal turns faster (a hole's mouth, a sharp edge) the tool turns no faster than a
/// machine's head or table can follow at feed, and is backed off where that leaves it short
/// of the surface.
pub const MOST_TURN_PER_MM: f64 = 10.0;

/// The axes of a pass held to turning at most `rate` degrees a millimetre between neighbours:
/// forward, then back, so a sudden turn is spread both ways.
pub fn limit_turn_rate(tips: &[P3], axes: &mut [P3], rate: f64) {
    let step = |i: usize, j: usize, axes: &mut [P3]| {
        let most = rate * dist(tips[i], tips[j]);
        let turn = angle_between(axes[i], axes[j]);
        if turn > most + 1e-9 {
            axes[j] = slerp(axes[i], axes[j], most / turn);
        }
    };
    for k in 1..axes.len() {
        step(k - 1, k, axes);
    }
    for k in (1..axes.len()).rev() {
        step(k, k - 1, axes);
    }
}

/// A pass with the poses dropped that lie on the way between their neighbours: the tip
/// within `tol` of the straight line, the axis within a fifth of a degree of its turn.
pub fn simplify(poses: &[Pose], tol: f64) -> Vec<Pose> {
    if poses.len() < 3 {
        return poses.to_vec();
    }
    let mut out = vec![poses[0]];
    let mut from = 0;
    let mut to = 2;
    while to < poses.len() {
        let (a, b) = (poses[from], poses[to]);
        let l = dist(a.tip, b.tip);
        let fits = poses[from + 1..to].iter().all(|m| {
            let t = if l > 1e-12 {
                (dot(sub(m.tip, a.tip), sub(b.tip, a.tip)) / (l * l)).clamp(0.0, 1.0)
            } else {
                0.0
            };
            dist(m.tip, lerp(a.tip, b.tip, t)) <= tol
                && angle_between(m.axis, slerp(a.axis, b.axis, t)) <= 0.2
        }) && angle_between(a.axis, b.axis) <= MOST_TURN_MERGED;
        if fits && to - from < 64 {
            to += 1;
        } else {
            out.push(poses[to - 1]);
            from = to - 1;
            to = from + 2;
        }
    }
    out.push(poses[poses.len() - 1]);
    out
}

/// A 3-axis finishing toolpath with the tool leant by the Axis tab's rule: its cutting passes
/// say where on the surface the tool is; at each place the tool is leant and dropped down its
/// own axis onto the part.
pub fn tilt(input: &Input, tp: Toolpath) -> Result<Toolpath, String> {
    let p = input.params;
    let rule = AxisRule { control: &p.tilt };
    let grid = TriGrid::new(&input.model.triangles);
    let top = everything(input).max[2] + 10.0;
    // The cutting passes: runs of cuts and of the feeds that link them along the surface.
    let mut runs: Vec<Vec<P3>> = Vec::new();
    let mut run: Vec<P3> = Vec::new();
    let mut at: Option<P3> = None;
    for s in &tp.steps {
        match s {
            Step::Feed {
                to,
                kind: FeedKind::Cut | FeedKind::Link,
                ..
            } => {
                if run.is_empty()
                    && let Some(a) = at
                {
                    run.push(a);
                }
                run.push(*to);
                at = Some(*to);
            }
            Step::Rapid(q) | Step::Feed { to: q, .. } | Step::Arc { to: q, .. } => {
                if run.len() > 1 {
                    runs.push(std::mem::take(&mut run));
                }
                run.clear();
                at = Some(*q);
            }
            _ => {}
        }
    }
    if run.len() > 1 {
        runs.push(run);
    }
    if runs.is_empty() {
        return Err("Nothing to finish between the heights".into());
    }
    // The stock to leave (the 3-axis pass has left it already): the cutter grown by it is the
    // one kept clear of the part.
    let (dropped, lift) = offset_cutter(input.tool, p.wall_stock.max(0.0));
    let step = (input.tool.radius() / 2.0).clamp(0.2, 1.0);
    let reach = input.tool.overall_length.max(input.tool.diameter);
    let mut scratch = Vec::new();
    let mut passes: Vec<Vec<Pose>> = Vec::new();
    let mut most_lean: f64 = 0.0;
    let mut guard = Guard::new(input);
    // A ball turns about its centre and a bull nose about its corner's; a flat end about the
    // middle of its end.
    let pivot = match input.tool.kind {
        crate::tool::ToolKind::BallMill | crate::tool::ToolKind::Lollipop => input.tool.radius(),
        crate::tool::ToolKind::BullMill => input.tool.corner_radius.clamp(0.0, input.tool.radius()),
        crate::tool::ToolKind::TaperBall => input.tool.tip_diameter / 2.0,
        _ => 0.0,
    };
    for run in &runs {
        // Places along the pass close enough for the surface's normal to be followed.
        let mut places: Vec<P3> = vec![run[0]];
        for w in run.windows(2) {
            let n = (dist(w[0], w[1]) / step).ceil().max(1.0) as usize;
            places.extend((1..=n).map(|k| lerp(w[0], w[1], k as f64 / n as f64)));
        }
        // The point each lean turns about: the middle of the cutter's ball or corner as the
        // 3-axis pass has it. Leant about it, a ball stays the very ball the 3-axis pass
        // dropped onto the part, so the leant pass is that pass with only its shank moved, and
        // runs on as smoothly however the surface's normal jumps (a hole's mouth, a sharp
        // edge).
        let pivots: Vec<P3> = places
            .iter()
            .map(|q| add(*q, scale(UPRIGHT, pivot)))
            .collect();
        // The surface's normal where the cutter touches: under the place, then, from there,
        // where the pivot looks back along that normal (on a curved face the cutter touches
        // beside the point under its tip). Off the part: the tool upright.
        let hits: Vec<Option<(P3, P3)>> = places
            .iter()
            .zip(&pivots)
            .map(|(q, c)| {
                let under = grid.ray(
                    [q[0], q[1], top],
                    [0.0, 0.0, -1.0],
                    top - input.bottom() + 20.0,
                    &mut scratch,
                )?;
                let back = grid
                    .ray(
                        add(*c, scale(under.normal, 1e-3)),
                        scale(under.normal, -1.0),
                        2.0 * input.tool.diameter + 1.0,
                        &mut scratch,
                    )
                    .filter(|_| pivot > 0.0);
                Some(back.map_or((under.at, under.normal), |h| (h.at, h.normal)))
            })
            .collect();
        let mut axes: Vec<P3> = (0..places.len())
            .map(|i| {
                let ahead = if i + 1 < places.len() {
                    sub(places[i + 1], places[i])
                } else {
                    sub(places[i], places[i.saturating_sub(1)])
                };
                match hits[i] {
                    Some((c, n)) => rule.at(c, n, normalize(ahead)),
                    None => UPRIGHT,
                }
            })
            .collect();
        // Off the part the tool leans as it does at the nearest place on it: the lean neither
        // ramps up from upright as a pass comes onto the part nor drops as it runs off.
        let on: Vec<usize> = (0..places.len()).filter(|i| hits[*i].is_some()).collect();
        if !on.is_empty() {
            for i in 0..places.len() {
                if hits[i].is_none() {
                    let k = on.partition_point(|j| *j < i);
                    let near = match (k.checked_sub(1).map(|a| on[a]), on.get(k)) {
                        (Some(a), Some(b)) => {
                            if i - a <= b - i {
                                a
                            } else {
                                *b
                            }
                        }
                        (Some(a), None) => a,
                        (None, Some(b)) => *b,
                        (None, None) => continue,
                    };
                    axes[i] = axes[near];
                }
            }
        }
        smooth_axes(&places, &mut axes, p.tilt.smoothing);
        // Within the rule's limits (the most lean, upright near the pole), and then turning no
        // faster than the rate along the way the cutter's centre goes: last, so what is posted
        // keeps to it.
        for a in axes.iter_mut() {
            *a = rule.within(*a);
        }
        limit_turn_rate(&pivots, &mut axes, MOST_TURN_PER_MM);
        // How far up its axis a pose must back off for the leant cutter (grown by the stock to
        // leave) to clear the part: its flank, or the rim of a flat end leant over.
        let mut clear = |tip: P3, axis: P3| -> f64 {
            grid.drop_along(
                &dropped,
                sub(tip, scale(axis, lift)),
                axis,
                (-dropped.diameter, reach),
                0.0,
                &mut scratch,
            )
            .max(0.0)
        };
        let mut poses: Vec<Pose> = Vec::with_capacity(places.len());
        for i in 0..places.len() {
            let axis = axes[i];
            most_lean = most_lean.max(angle_between(UPRIGHT, axis));
            let tip = sub(pivots[i], scale(axis, pivot));
            let pose = Pose {
                tip: add(tip, scale(axis, clear(tip, axis))),
                axis,
            };
            // A lean that swings from one side to the other past upright (across a hole's
            // mouth, over a crest) stands upright on the way, where a machine's rotary axis
            // about Z can turn while nothing moves ([`AxisRule::within`]).
            if let Some(last) = poses.last().copied() {
                let (closest, t) = (1..16)
                    .map(|k| {
                        let t = k as f64 / 16.0;
                        (angle_between(UPRIGHT, slerp(last.axis, axis, t)), t)
                    })
                    .fold((f64::INFINITY, 0.0), |m, x| if x.0 < m.0 { x } else { m });
                let ends = angle_between(UPRIGHT, last.axis).min(angle_between(UPRIGHT, axis));
                if closest < 2.0 * POLE && ends > closest + 1.0 {
                    let centre = lerp(add(last.tip, scale(last.axis, pivot)), pivots[i], t);
                    let tip = sub(centre, scale(UPRIGHT, pivot));
                    poses.push(Pose {
                        tip: add(tip, scale(UPRIGHT, clear(tip, UPRIGHT))),
                        axis: UPRIGHT,
                    });
                }
            }
            // Between two poses the tip goes straight while the tool turns: where that takes
            // the cutter into the part (a quick turn over a crest), a pose between them,
            // backed off, keeps it out.
            if let Some(last) = poses.last().copied() {
                let mid = Pose {
                    tip: lerp(last.tip, pose.tip, 0.5),
                    axis: slerp(last.axis, pose.axis, 0.5),
                };
                let up = clear(mid.tip, mid.axis);
                if up > p.tolerance {
                    poses.push(Pose {
                        tip: add(mid.tip, scale(mid.axis, up)),
                        axis: mid.axis,
                    });
                }
            }
            poses.push(pose);
        }
        guard.apply(&mut poses);
        passes.push(simplify(&poses, p.tolerance * 0.5));
    }
    let mut e = Emitter::new(input, Clearance::over(input, most_lean));
    e.warnings = tp.warnings;
    guard.report(&mut e.warnings);
    for poses in &passes {
        let cover = stock_cover(input, &poses[0]);
        e.pass(poses, true, cover);
    }
    Ok(e.finish())
}

/// A hole at any angle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hole3 {
    /// The middle of its mouth.
    pub top: P3,
    /// Out of the hole, along its axis (unit).
    pub axis: P3,
    pub diameter: f64,
    /// From its mouth to the end of its wall.
    pub depth: f64,
    /// Open at the far end too.
    pub through: bool,
}

/// The model's holes at any angle: its inner cylindrical faces, those on one axis taken
/// together, each entered from its open end (from the higher one when both are open).
pub fn holes_any_axis(input: &Input, grid: &TriGrid) -> Vec<Hole3> {
    struct Bore {
        point: P3,
        axis: P3,
        radius: f64,
        span: (f64, f64),
    }
    let m = input.model;
    let mut bores: Vec<Bore> = Vec::new();
    for f in &m.faces {
        let Surface::Cylinder {
            point,
            axis,
            radius,
            inner: true,
        } = f.surface
        else {
            continue;
        };
        let axis = normalize(axis);
        let along = |q: P3| dot(sub(q, point), axis);
        let pts = f
            .loops
            .iter()
            .flatten()
            .copied()
            .chain(f.triangles.iter().flat_map(|k| m.triangles[*k as usize]));
        let span = pts.fold((f64::INFINITY, f64::NEG_INFINITY), |s, q| {
            (s.0.min(along(q)), s.1.max(along(q)))
        });
        if !span.0.is_finite() {
            continue;
        }
        // The same hole as one found already (a wall split in two faces)?
        let same = bores.iter_mut().find(|b| {
            (b.radius - radius).abs() < 1e-6
                && len(cross(b.axis, axis)) < 1e-6
                && len(cross(sub(point, b.point), b.axis)) < 1e-6
        });
        match same {
            Some(b) => {
                let shift = dot(sub(point, b.point), b.axis);
                let flip = dot(axis, b.axis).signum();
                let (lo, hi) = (shift + flip * span.0, shift + flip * span.1);
                b.span = (b.span.0.min(lo.min(hi)), b.span.1.max(lo.max(hi)));
            }
            None => bores.push(Bore {
                point,
                axis,
                radius,
                span,
            }),
        }
    }
    let far = len(everything(input).size()) + 1.0;
    let mut scratch = Vec::new();
    bores
        .iter()
        .filter_map(|b| {
            let end = |s: f64| add(b.point, scale(b.axis, s));
            // An end is open when a ray from just inside it, out along the axis, meets
            // nothing of the part (a blind hole's floor faces it).
            let inside = (b.span.1 - b.span.0).min(0.02) / 2.0;
            let open_hi = grid
                .ray(end(b.span.1 - inside), b.axis, far, &mut scratch)
                .is_none();
            let open_lo = grid
                .ray(
                    end(b.span.0 + inside),
                    scale(b.axis, -1.0),
                    far,
                    &mut scratch,
                )
                .is_none();
            let from_hi = match (open_hi, open_lo) {
                (true, true) => b.axis[2] >= 0.0,
                (true, false) => true,
                (false, true) => false,
                (false, false) => return None,
            };
            Some(Hole3 {
                top: end(if from_hi { b.span.1 } else { b.span.0 }),
                axis: if from_hi { b.axis } else { scale(b.axis, -1.0) },
                diameter: 2.0 * b.radius,
                depth: b.span.1 - b.span.0,
                through: open_hi && open_lo,
            })
        })
        .collect()
}

/// Drilling with every hole down its own axis: the holes the tool fits (or those picked),
/// each in turn the nearest in angle and place to the last.
pub fn drill(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let grid = TriGrid::new(&input.model.triangles);
    let d = input.tool.diameter;
    let mut holes: Vec<Hole3> = holes_any_axis(input, &grid)
        .into_iter()
        .filter(|h| match p.cycle.select {
            HoleSelect::MatchingTool => (h.diameter - d).abs() <= 0.1 + d * 0.02,
            HoleSelect::Blind => !h.through && h.diameter >= d - 1e-6,
            HoleSelect::All | HoleSelect::SameDiameter => h.diameter >= d - 1e-6,
        })
        .filter(|h| angle_between(UPRIGHT, h.axis) <= p.tilt.max_tilt + 1e-6)
        .collect();
    if holes.is_empty() {
        return Err("No hole the tool fits can be reached".into());
    }
    // The nearest in angle first, then in place.
    let mut order = Vec::with_capacity(holes.len());
    let mut last = Pose {
        tip: [0.0, 0.0, f64::INFINITY],
        axis: UPRIGHT,
    };
    while !holes.is_empty() {
        let cost =
            |h: &Hole3| angle_between(last.axis, h.axis) * 10.0 + dist(last.tip, h.top).min(1e9);
        let k = (0..holes.len())
            .min_by(|a, b| cost(&holes[*a]).total_cmp(&cost(&holes[*b])))
            .unwrap();
        let h = holes.swap_remove(k);
        last = Pose {
            tip: h.top,
            axis: h.axis,
        };
        order.push(h);
    }
    let lean = order
        .iter()
        .map(|h| angle_between(UPRIGHT, h.axis))
        .fold(0.0, f64::max);
    let mut e = Emitter::new(input, Clearance::over(input, lean));
    // A drill's point goes past a through hole's far end until its full diameter is through.
    let point = input.tool.height_at(input.tool.radius()).unwrap_or(0.0);
    let feed_distance = p.links.feed_distance.max(0.5);
    for h in &order {
        let depth = if h.through {
            h.depth + point + p.cycle.break_through
        } else {
            h.depth
        };
        let at = |down: f64| Pose {
            tip: add(h.top, scale(h.axis, -down)),
            axis: h.axis,
        };
        let above = at(-feed_distance);
        // Over the hole on the clearance shape, down its axis to just above its mouth.
        match e.at {
            Some(cur) => {
                let out = Pose {
                    tip: e.clear.lift(&cur),
                    axis: cur.axis,
                };
                let over = Pose {
                    tip: e.clear.lift(&above),
                    axis: h.axis,
                };
                e.rapid(out);
                for q in e.clear.across(&out, &over) {
                    e.rapid(q);
                }
                e.rapid(over);
            }
            None => e.rapid(Pose {
                tip: e.clear.lift(&above),
                axis: h.axis,
            }),
        }
        e.rapid(above);
        let peck = if matches!(p.cycle.kind, CycleKind::Peck | CycleKind::ChipBreak)
            && p.cycle.peck > 0.0
        {
            p.cycle.peck
        } else {
            f64::INFINITY
        };
        let mut down = 0.0;
        while down < depth - 1e-9 {
            let next = (down + peck).min(depth);
            e.feed(at(next), FeedKind::Plunge);
            down = next;
            if down < depth - 1e-9 {
                // Clear the chips: right out for a peck, a little back for a chip break.
                if p.cycle.kind == CycleKind::Peck {
                    e.rapid(above);
                    e.rapid(at(down - 0.5));
                } else {
                    e.rapid(at(down - 0.5));
                }
            }
        }
        if p.cycle.dwell > 0.0 {
            e.steps.push(Step::Dwell(p.cycle.dwell));
        }
        if p.cycle.kind == CycleKind::Tap {
            e.feed(above, FeedKind::Retract);
        } else {
            e.rapid(above);
        }
    }
    Ok(e.finish())
}

/// Swarf cutting: the tool's side along the picked walls. Each wall's bottom edge is followed
/// with the tool's axis along the wall's own slope there (its steepest line, which on a plane,
/// a cylinder, a cone or any wall ruled that way is a straight line of the wall), the tool's
/// end on the edge and its side on the wall, in depth passes down to the edge.
pub fn swarf(input: &Input) -> Result<Toolpath, String> {
    let p = input.params;
    let m = input.model;
    let picked: Vec<u32> = p
        .drive
        .faces
        .iter()
        .filter_map(|n| m.face(n))
        .flat_map(|f| f.triangles.iter().copied())
        .collect();
    if picked.is_empty() {
        return Err("Pick the walls to cut with the tool's side".into());
    }
    // Each picked triangle's normal and its slope's direction (up the wall).
    let frame_of = |k: u32| -> Option<(P3, P3)> {
        let t = &m.triangles[k as usize];
        let n = normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])));
        let up = sub(UPRIGHT, scale(n, n[2]));
        (len(up) > 0.17).then(|| (n, normalize(up)))
    };
    // The picked faces' boundary: the edges only one picked triangle has.
    let key = |q: P3| [0, 1, 2].map(|k| (q[k] * 1e4).round() as i64);
    let mut count: HashMap<([i64; 3], [i64; 3]), usize> = HashMap::new();
    for &k in &picked {
        let t = &m.triangles[k as usize];
        for i in 0..3 {
            let (a, b) = (key(t[i]), key(t[(i + 1) % 3]));
            *count
                .entry(if a <= b { (a, b) } else { (b, a) })
                .or_default() += 1;
        }
    }
    // Its bottom edges: those along the wall with the wall rising from them.
    struct Rail {
        a: P3,
        b: P3,
        n: P3,
        up: P3,
    }
    let mut rails: Vec<Rail> = Vec::new();
    let mut flat = 0;
    for &k in &picked {
        let Some((n, up)) = frame_of(k) else {
            flat += 1;
            continue;
        };
        let t = &m.triangles[k as usize];
        for i in 0..3 {
            let (a, b, c) = (t[i], t[(i + 1) % 3], t[(i + 2) % 3]);
            let (ka, kb) = (key(a), key(b));
            if count[&if ka <= kb { (ka, kb) } else { (kb, ka) }] != 1 || dist(a, b) < 1e-9 {
                continue;
            }
            let along = normalize(sub(b, a));
            if dot(along, up).abs() > 0.7 || dot(sub(c, lerp(a, b, 0.5)), up) <= 0.0 {
                continue;
            }
            // Climb milling has the wall on the tool's right: the cut runs along n × up.
            let forward = dot(along, cross(n, up)) > 0.0;
            let climb = p.direction == Direction::Climb;
            let (a, b) = if forward == climb { (a, b) } else { (b, a) };
            rails.push(Rail { a, b, n, up });
        }
    }
    if rails.is_empty() {
        return Err(if flat > 0 {
            "The picked faces are too flat to cut with the tool's side".into()
        } else {
            "The picked walls have no bottom edge to follow".into()
        });
    }
    // The rails chained end to start.
    let mut starts: HashMap<[i64; 3], Vec<usize>> = HashMap::new();
    for (i, r) in rails.iter().enumerate() {
        starts.entry(key(r.a)).or_default().push(i);
    }
    let ends: std::collections::HashSet<[i64; 3]> = rails.iter().map(|r| key(r.b)).collect();
    let mut used = vec![false; rails.len()];
    let mut chains: Vec<Vec<usize>> = Vec::new();
    // (Open chains from their free starts first, then what is left: closed loops.)
    let order: Vec<usize> = (0..rails.len())
        .filter(|i| !ends.contains(&key(rails[*i].a)))
        .chain(0..rails.len())
        .collect();
    for i in order {
        if used[i] {
            continue;
        }
        let mut chain = vec![i];
        used[i] = true;
        loop {
            let last = *chain.last().unwrap();
            let next = starts
                .get(&key(rails[last].b))
                .and_then(|v| v.iter().copied().find(|j| !used[*j]));
            match next {
                Some(j) => {
                    used[j] = true;
                    chain.push(j);
                }
                None => break,
            }
        }
        chains.push(chain);
    }
    // How high the wall stands over a point of its bottom edge, along its slope.
    let wall: Vec<P3> = picked
        .iter()
        .flat_map(|k| m.triangles[*k as usize])
        .collect();
    let reach = input.tool.diameter.max(2.0);
    let height = |v: P3, up: P3| -> f64 {
        wall.iter()
            .filter(|q| {
                let w = sub(**q, v);
                len(sub(w, scale(up, dot(w, up)))) <= reach
            })
            .map(|q| dot(sub(*q, v), up))
            .fold(0.0, f64::max)
    };
    let r = input.tool.radius() + p.wall_stock.max(0.0);
    let grid = TriGrid::new(&m.triangles);
    let mut scratch = Vec::new();
    let mut e = Emitter::new(input, Clearance::Plane(0.0));
    let mut guard = Guard::new(input);
    let mut lean: f64 = 0.0;
    let mut left: f64 = 0.0;
    let mut passes: Vec<Vec<Pose>> = Vec::new();
    for chain in &chains {
        // The chain's corners with the wall's frame at each (the mean of the edges meeting).
        let mut pts: Vec<(P3, P3, P3)> = Vec::new();
        for (k, &i) in chain.iter().enumerate() {
            let rail = &rails[i];
            let (mut n, mut up) = (rail.n, rail.up);
            if k > 0 {
                let before = &rails[chain[k - 1]];
                n = normalize(add(n, before.n));
                up = normalize(add(up, before.up));
            }
            pts.push((rail.a, n, up));
            if k + 1 == chain.len() {
                pts.push((rail.b, rail.n, rail.up));
            }
        }
        let tallest = pts
            .iter()
            .map(|(v, _, up)| height(*v, *up))
            .fold(0.0, f64::max);
        // Depth passes from the top of the wall down to its bottom edge.
        let mut levels = Vec::new();
        let mut level = tallest - p.step_down.max(1e-3);
        while level > 1e-6 {
            levels.push(level);
            level -= p.step_down.max(1e-3);
        }
        levels.push(0.0);
        for level in levels {
            let poses: Vec<Pose> = pts
                .iter()
                .map(|(v, n, up)| {
                    let axis = AxisRule { control: &p.tilt }.within(*up);
                    lean = lean.max(angle_between(UPRIGHT, axis));
                    let rise = level.min(height(*v, *up)) + p.floor_stock.max(0.0);
                    let tip = add(add(*v, scale(*n, r)), scale(axis, rise));
                    // A wall that isn't straight along the tool pushes it off: up its own
                    // axis until the cutter clears.
                    let h = grid.drop_along(
                        input.tool,
                        tip,
                        axis,
                        (0.0, input.tool.cutting_length),
                        0.0,
                        &mut scratch,
                    );
                    left = left.max(h);
                    Pose {
                        tip: add(tip, scale(axis, h)),
                        axis,
                    }
                })
                .collect();
            let mut poses = poses;
            guard.apply(&mut poses);
            passes.push(poses);
        }
    }
    guard.report(&mut e.warnings);
    e.clear = Clearance::over(input, lean);
    for poses in &passes {
        // In from the side: a lead the tool's radius off the wall.
        let first = poses[0];
        let off = normalize(cross(
            first.axis,
            normalize(sub(poses[1.min(poses.len() - 1)].tip, first.tip)),
        ));
        let lead = Pose {
            tip: add(first.tip, scale(off, input.tool.radius().max(1.0))),
            axis: first.axis,
        };
        let start = if len(off) > 0.5 { lead } else { first };
        e.approach(start, false, stock_cover(input, &start));
        e.feed(first, FeedKind::LeadIn);
        for q in &poses[1..] {
            e.feed(*q, FeedKind::Cut);
        }
    }
    if left > p.tolerance.max(0.01) {
        e.warnings.push(format!(
            "The walls aren't straight along the tool everywhere: up to {} mm is left",
            crate::tool::fmt_mm(left)
        ));
    }
    Ok(e.finish())
}

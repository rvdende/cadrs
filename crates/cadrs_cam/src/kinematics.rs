//! Machine kinematics: how a machine's axes carry the tool and the work.
//!
//! A machine is two open chains of joints from its base: one out to the **tool mount** (the
//! spindle nose) and one out to the **table mount** (where a setup's work sits). A [`Joint`]
//! slides its side of the machine along a line or turns it about one; both are given in the
//! machine's coordinates with every axis at its [`Joint::home`] value. A vertical mill whose
//! table moves in X and Y has X and Y on the table chain and Z on the tool chain; a trunnion
//! adds A and C to the table chain; a robot arm has six turns on the tool chain and none on
//! the table's. A [`Chain`] is all a post, a simulation or a collision check needs to know of
//! the machine's motion; `cadrs_core` derives one from an assembly's mates, and the catalogue
//! machines carry one outright.
//!
//! - **Forward** ([`Chain::poses`], [`Chain::locate`]): axis values → where every moving body
//!   is, and where the tool's tip and axis are as the work sees them.
//! - **Inverse** ([`Chain::solve`]): a cutter location (tip and axis in the table mount's
//!   coordinates) → axis values. Three slides with up to two turns, in any arrangement (table
//!   and table, head and table, head and head; square, nutating or offset axes), are solved in
//!   closed form with every solution found; any other chain (an arm) by damped least squares
//!   from the last position, the turn about the tool's own axis left free.
//! - **Along a move** ([`Chain::linearize`]): the axes run straight from one position to the
//!   next, so the tip strays from the straight cut wherever a turn is involved; a move is cut
//!   up until it stays within tolerance.
//!
//! Slides are in mm, turns in degrees, right-handed about the joint's direction.

use serde::{Deserialize, Serialize};

use crate::geom::{Frame, P3, add, cross, dist, dot, len, normalize, scale, sub};

/// A slide or a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum JointKind {
    Linear,
    Rotary,
}

/// One axis of a machine: what it carries slides along `dir` or turns about the line through
/// `origin` along `dir`, as its value rises from `home`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    /// Its letter or name: "X", "A", "J3".
    pub name: String,
    pub kind: JointKind,
    /// A point of a turn's line (machine coordinates, every axis at home).
    pub origin: P3,
    /// The way the carried side moves as the value rises, or the line it turns about
    /// right-handed (a unit vector, machine coordinates, every axis at home).
    pub dir: P3,
    /// The least and greatest value (mm or degrees); a continuous turn has none.
    pub min: f64,
    pub max: f64,
    /// A turn without stops: it winds on past ±360°.
    #[serde(default)]
    pub continuous: bool,
    /// The value it reads where the machine was described.
    #[serde(default)]
    pub home: f64,
    /// Its rapid rate (mm/min or degrees/min).
    pub rapid: f64,
    /// Its fastest feed (mm/min or degrees/min).
    pub max_feed: f64,
}

impl Joint {
    /// A slide along `dir` between `min` and `max` mm.
    pub fn linear(name: &str, dir: P3, min: f64, max: f64) -> Joint {
        Joint {
            name: name.into(),
            kind: JointKind::Linear,
            origin: [0.0; 3],
            dir: normalize(dir),
            min,
            max,
            continuous: false,
            home: 0.0,
            rapid: 10_000.0,
            max_feed: 5_000.0,
        }
    }

    /// A turn about the line through `origin` along `dir` between `min` and `max` degrees.
    pub fn rotary(name: &str, origin: P3, dir: P3, min: f64, max: f64) -> Joint {
        Joint {
            name: name.into(),
            kind: JointKind::Rotary,
            origin,
            dir: normalize(dir),
            min,
            max,
            continuous: false,
            home: 0.0,
            rapid: 7_200.0,
            max_feed: 3_600.0,
        }
    }

    /// The same turn without stops.
    pub fn endless(mut self) -> Joint {
        self.continuous = true;
        self
    }

    /// The same axis at these rates (mm/min or degrees/min).
    pub fn rates(mut self, rapid: f64, max_feed: f64) -> Joint {
        self.rapid = rapid;
        self.max_feed = max_feed;
        self
    }

    pub fn is_rotary(&self) -> bool {
        self.kind == JointKind::Rotary
    }

    /// Where it puts what it carries at the value `q`.
    pub fn motion(&self, q: f64) -> Xf {
        match self.kind {
            JointKind::Linear => Xf::translation(scale(self.dir, q - self.home)),
            JointKind::Rotary => Xf::rotation(self.origin, self.dir, q - self.home),
        }
    }

    /// Whether `q` is within its travel (to a hair).
    pub fn within(&self, q: f64) -> bool {
        self.continuous && self.is_rotary()
            || (q >= self.min - LIMIT_SLACK && q <= self.max + LIMIT_SLACK)
    }
}

/// How far past a stop a value may be and still count as on it (rounding).
const LIMIT_SLACK: f64 = 1e-6;

/// A rigid motion: `p ↦ r·p + t`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Xf {
    /// Its rotation, by rows.
    pub r: [[f64; 3]; 3],
    pub t: P3,
}

impl Xf {
    pub const IDENTITY: Xf = Xf {
        r: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        t: [0.0; 3],
    };

    pub fn translation(d: P3) -> Xf {
        Xf {
            t: d,
            ..Xf::IDENTITY
        }
    }

    /// The turn by `degrees`, right-handed, about the line through `origin` along the unit
    /// vector `dir`.
    pub fn rotation(origin: P3, dir: P3, degrees: f64) -> Xf {
        let r = rodrigues(dir, degrees);
        let turned = apply(&r, origin);
        Xf {
            r,
            t: sub(origin, turned),
        }
    }

    /// The motion that takes a frame's own coordinates to the coordinates it is given in.
    pub fn of_frame(f: &Frame) -> Xf {
        Xf {
            r: [
                [f.x[0], f.y[0], f.z[0]],
                [f.x[1], f.y[1], f.z[1]],
                [f.x[2], f.y[2], f.z[2]],
            ],
            t: f.origin,
        }
    }

    /// `inner` first, then this.
    pub fn then(&self, inner: &Xf) -> Xf {
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = (0..3).map(|k| self.r[i][k] * inner.r[k][j]).sum();
            }
        }
        Xf {
            r,
            t: add(apply(&self.r, inner.t), self.t),
        }
    }

    pub fn inverse(&self) -> Xf {
        let mut r = [[0.0; 3]; 3];
        for (i, row) in r.iter_mut().enumerate() {
            for (j, v) in row.iter_mut().enumerate() {
                *v = self.r[j][i];
            }
        }
        let t = scale(apply(&r, self.t), -1.0);
        Xf { r, t }
    }

    pub fn point(&self, p: P3) -> P3 {
        add(apply(&self.r, p), self.t)
    }

    pub fn dir(&self, d: P3) -> P3 {
        apply(&self.r, d)
    }

    /// The frame `f` moved by it.
    pub fn frame(&self, f: &Frame) -> Frame {
        Frame {
            origin: self.point(f.origin),
            x: self.dir(f.x),
            y: self.dir(f.y),
            z: self.dir(f.z),
        }
    }
}

fn apply(r: &[[f64; 3]; 3], v: P3) -> P3 {
    [dot(r[0], v), dot(r[1], v), dot(r[2], v)]
}

/// The rotation by `degrees` about the unit vector `u`.
fn rodrigues(u: P3, degrees: f64) -> [[f64; 3]; 3] {
    let (s, c) = degrees.to_radians().sin_cos();
    let t = 1.0 - c;
    [
        [
            c + u[0] * u[0] * t,
            u[0] * u[1] * t - u[2] * s,
            u[0] * u[2] * t + u[1] * s,
        ],
        [
            u[1] * u[0] * t + u[2] * s,
            c + u[1] * u[1] * t,
            u[1] * u[2] * t - u[0] * s,
        ],
        [
            u[2] * u[0] * t - u[1] * s,
            u[2] * u[1] * t + u[0] * s,
            c + u[2] * u[2] * t,
        ],
    ]
}

/// `v` turned by `degrees` about the unit vector `u`.
pub fn turned(v: P3, u: P3, degrees: f64) -> P3 {
    apply(&rodrigues(u, degrees), v)
}

/// The angle (degrees) between two unit vectors.
pub fn angle_between(a: P3, b: P3) -> f64 {
    len(cross(a, b)).atan2(dot(a, b)).to_degrees()
}

/// The unit vector `t` of the way round from `a` to `b` (both unit; the great arc between
/// them, or `a` when they are opposite and no arc is the shortest).
pub fn slerp(a: P3, b: P3, t: f64) -> P3 {
    let angle = len(cross(a, b)).atan2(dot(a, b));
    if angle < 1e-12 || (std::f64::consts::PI - angle) < 1e-9 {
        return if t < 1.0 { a } else { b };
    }
    let s = angle.sin();
    normalize(add(
        scale(a, ((1.0 - t) * angle).sin() / s),
        scale(b, (t * angle).sin() / s),
    ))
}

/// Which chain a joint is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Side {
    /// Between the base and the tool.
    Tool,
    /// Between the base and the work.
    Table,
}

/// A cutter location: the tool's tip and the unit vector up its axis, from the tip toward the
/// spindle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Cl {
    pub tip: P3,
    pub axis: P3,
}

impl Cl {
    pub fn new(tip: P3, axis: P3) -> Cl {
        Cl {
            tip,
            axis: normalize(axis),
        }
    }

    /// The location `t` of the way to `to`: the tip along the straight line, the axis round
    /// the great arc.
    pub fn toward(&self, to: &Cl, t: f64) -> Cl {
        Cl {
            tip: crate::geom::lerp(self.tip, to.tip, t),
            axis: slerp(self.axis, to.axis, t),
        }
    }

    /// The same location in the coordinates `frame` is given in, this one being in `frame`'s.
    pub fn out_of(&self, frame: &Frame) -> Cl {
        Cl {
            tip: frame.to_world(self.tip),
            axis: frame.dir_to_world(self.axis),
        }
    }

    /// The same location in `frame`'s coordinates.
    pub fn seen_from(&self, frame: &Frame) -> Cl {
        Cl {
            tip: frame.to_local(self.tip),
            axis: frame.dir_to_local(self.axis),
        }
    }
}

/// A machine's axes: the joints from the base out to the tool mount and out to the table
/// mount, each list nearest the base first, and the two mounts with every axis at home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chain {
    pub tool: Vec<Joint>,
    pub table: Vec<Joint>,
    /// The spindle's gauge point; its Z runs up the tool's axis, from the tip toward the
    /// spindle (the tip of a tool `length` long is at `origin − length·z`).
    pub tool_mount: Frame,
    /// Where work is put: cutter locations are given in its coordinates.
    pub table_mount: Frame,
}

/// Why no axis values put the tool where it was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum IkError {
    /// The machine cannot point the tool that way (too few turning axes, or a nutating axis
    /// short of the tilt).
    Orientation { tilt: f64 },
    /// It could, with this axis beyond its travel.
    Limit {
        joint: String,
        value: f64,
        min: f64,
        max: f64,
    },
    /// The slides cannot bring the tip there (they don't span every direction), or the search
    /// of a general chain found no position.
    Unreachable,
}

impl std::fmt::Display for IkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IkError::Orientation { tilt } => write!(
                f,
                "The machine's rotary axes cannot point the tool there (it is {tilt:.2}° off)"
            ),
            IkError::Limit {
                joint,
                value,
                min,
                max,
            } => write!(
                f,
                "{joint} would be at {value:.3}, beyond its travel ({min:.3} to {max:.3})"
            ),
            IkError::Unreachable => write!(f, "The machine cannot reach there"),
        }
    }
}

/// Tolerances a move is cut up to ([`Chain::linearize`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tolerance {
    /// How far the tip may stray from the straight line (mm).
    pub tip: f64,
    /// How far the axis may stray from the arc between the two ends (degrees).
    pub axis: f64,
    /// The control keeps the tip on the line itself (tool centre point control): only the
    /// axis strays.
    pub tcp: bool,
}

impl Default for Tolerance {
    fn default() -> Self {
        Tolerance {
            tip: 0.005,
            axis: 0.05,
            tcp: false,
        }
    }
}

/// A position along a move ([`Chain::linearize`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Waypoint {
    /// How far along the move it is (0 to 1).
    pub t: f64,
    /// The axis values there.
    pub q: Vec<f64>,
    /// The axes cannot run straight here from the position before with the tool in the cut
    /// (a rotary at its stop comes round the other way): the tool has to be lifted clear
    /// first.
    pub leap: bool,
}

/// A machine's positions through a run of moves ([`Chain::track`]).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AxisTrack {
    /// The axis values before the first move, then at the end of each.
    pub rows: Vec<Vec<f64>>,
    /// The moves the machine cannot make as asked (by their place in the run), and why: an
    /// axis beyond its travel (the machine is tracked there all the same), a lean it has no
    /// axis for, a place out of reach.
    pub issues: Vec<(usize, IkError)>,
}

impl AxisTrack {
    /// The axis values part of the way through move `i`, the tool then at `cl`: solved on
    /// from where the move started, so playing a run forward or stepping about in it shows
    /// the same machine.
    pub fn at(&self, chain: &Chain, i: usize, cl: &Cl, length: f64) -> Vec<f64> {
        let Some(from) = self.rows.get(i) else {
            return self.rows.last().cloned().unwrap_or_else(|| chain.rest());
        };
        match chain.solve_past(cl, length, from) {
            Ok((q, _)) => q,
            Err(_) => from.clone(),
        }
    }
}

/// A rotary far enough off to say the tool cannot be pointed (degrees).
const ORIENT_TOL: f64 = 1e-6;

impl Chain {
    /// Three slides along the machine's X, Y and Z, all carrying the tool, with these travels
    /// (mm, each from 0): a machine known only by its travel.
    pub fn three_axis(travel: [f64; 3]) -> Chain {
        let axis = |n: &str, d: P3, t: f64| Joint::linear(n, d, -t, t);
        Chain {
            tool: vec![
                axis("X", [1.0, 0.0, 0.0], travel[0]),
                axis("Y", [0.0, 1.0, 0.0], travel[1]),
                axis("Z", [0.0, 0.0, 1.0], travel[2]),
            ],
            table: vec![],
            tool_mount: Frame::WORLD,
            table_mount: Frame::WORLD,
        }
    }

    /// The chain of a machine known only by its travel and, for one that indexes, its two
    /// rotary letters and tilt: X, Y and Z carrying the tool without stops (where its travel
    /// starts isn't known), and for an indexing machine a trunnion under the work, its tilt
    /// axis (A about X or B about Y) and its platter (C about Z) both through the table mount.
    /// The rotaries turn the table left-handed as their values rise, which is the tool turning
    /// right-handed about the work (the usual sign).
    pub fn generic(tilt: Option<(char, char, f64)>) -> Chain {
        let slide = |n: &str, d: P3| Joint::linear(n, d, -1e9, 1e9);
        let mut table = Vec::new();
        if let Some((first, second, max_tilt)) = tilt {
            let line = |c: char| match c.to_ascii_uppercase() {
                'A' => [-1.0, 0.0, 0.0],
                'B' => [0.0, -1.0, 0.0],
                _ => [0.0, 0.0, -1.0],
            };
            table.push(Joint::rotary(
                &first.to_string(),
                [0.0; 3],
                line(first),
                -max_tilt,
                max_tilt,
            ));
            table.push(
                Joint::rotary(&second.to_string(), [0.0; 3], line(second), 0.0, 0.0).endless(),
            );
        }
        Chain {
            tool: vec![
                slide("X", [1.0, 0.0, 0.0]),
                slide("Y", [0.0, 1.0, 0.0]),
                slide("Z", [0.0, 0.0, 1.0]),
            ],
            table,
            tool_mount: Frame::WORLD,
            table_mount: Frame::WORLD,
        }
    }

    /// The number of axes.
    pub fn len(&self) -> usize {
        self.tool.len() + self.table.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every joint, the tool chain's first: the order of a list of axis values.
    pub fn joints(&self) -> impl Iterator<Item = &Joint> {
        self.tool.iter().chain(self.table.iter())
    }

    /// The joint at a place in that order.
    pub fn joint(&self, i: usize) -> &Joint {
        if i < self.tool.len() {
            &self.tool[i]
        } else {
            &self.table[i - self.tool.len()]
        }
    }

    pub fn side(&self, i: usize) -> Side {
        if i < self.tool.len() {
            Side::Tool
        } else {
            Side::Table
        }
    }

    /// The place of the joint named `name`.
    pub fn index(&self, name: &str) -> Option<usize> {
        self.joints().position(|j| j.name == name)
    }

    /// The places of the turning axes, in order.
    pub fn rotaries(&self) -> Vec<usize> {
        (0..self.len())
            .filter(|i| self.joint(*i).is_rotary())
            .collect()
    }

    /// The places of the sliding axes, in order.
    pub fn linears(&self) -> Vec<usize> {
        (0..self.len())
            .filter(|i| !self.joint(*i).is_rotary())
            .collect()
    }

    /// Every axis at home.
    pub fn home(&self) -> Vec<f64> {
        self.joints().map(|j| j.home).collect()
    }

    /// Every slide at home and every turn at zero (or as near as its travel allows): where a
    /// program starts and where plain 3-axis work is done.
    pub fn rest(&self) -> Vec<f64> {
        self.joints()
            .map(|j| {
                if !j.is_rotary() {
                    j.home
                } else if j.continuous {
                    0.0
                } else {
                    0.0f64.clamp(j.min, j.max)
                }
            })
            .collect()
    }

    /// Where each joint puts the body it carries (machine coordinates), in the order of the
    /// axis values: a body's points as modelled at home, moved.
    pub fn poses(&self, q: &[f64]) -> Vec<Xf> {
        let mut out = Vec::with_capacity(self.len());
        let mut at = Xf::IDENTITY;
        for (j, v) in self.tool.iter().zip(q) {
            at = at.then(&j.motion(*v));
            out.push(at);
        }
        at = Xf::IDENTITY;
        for (j, v) in self.table.iter().zip(&q[self.tool.len().min(q.len())..]) {
            at = at.then(&j.motion(*v));
            out.push(at);
        }
        out
    }

    /// Where the tool chain's last body is.
    pub fn tool_pose(&self, q: &[f64]) -> Xf {
        let mut at = Xf::IDENTITY;
        for (j, v) in self.tool.iter().zip(q) {
            at = at.then(&j.motion(*v));
        }
        at
    }

    /// Where the table chain's last body is.
    pub fn table_pose(&self, q: &[f64]) -> Xf {
        let mut at = Xf::IDENTITY;
        for (j, v) in self.table.iter().zip(&q[self.tool.len().min(q.len())..]) {
            at = at.then(&j.motion(*v));
        }
        at
    }

    /// The tool mount and the table mount (machine coordinates) at the axis values `q`.
    pub fn mounts(&self, q: &[f64]) -> (Frame, Frame) {
        (
            self.tool_pose(q).frame(&self.tool_mount),
            self.table_pose(q).frame(&self.table_mount),
        )
    }

    /// Forward kinematics: where a tool `length` long has its tip and axis, as the table mount
    /// sees them, at the axis values `q`.
    pub fn locate(&self, q: &[f64], length: f64) -> Cl {
        let (tool, table) = self.mounts(q);
        let tip = sub(tool.origin, scale(tool.z, length));
        Cl {
            tip: table.to_local(tip),
            axis: table.dir_to_local(tool.z),
        }
    }

    /// The first axis beyond its travel at `q`.
    pub fn beyond(&self, q: &[f64]) -> Option<IkError> {
        self.joints()
            .zip(q)
            .find(|(j, v)| !j.within(**v))
            .map(|(j, v)| IkError::Limit {
                joint: j.name.clone(),
                value: *v,
                min: j.min,
                max: j.max,
            })
    }

    /// Whether it is three slides spanning every direction and at most two turns: a machine
    /// whose axis values are found in closed form.
    pub fn is_mill(&self) -> bool {
        let lin = self.linears();
        if lin.len() != 3 || self.rotaries().len() > 2 {
            return false;
        }
        let d: Vec<P3> = lin.iter().map(|i| self.joint(*i).dir).collect();
        dot(d[0], cross(d[1], d[2])).abs() > 1e-6
    }

    /// Inverse kinematics: the axis values that put a tool `length` long at `cl` (the table
    /// mount's coordinates), the solution nearest `near` (the last position; [`Chain::home`]
    /// to start with) among those within every axis's travel.
    pub fn solve(&self, cl: &Cl, length: f64, near: &[f64]) -> Result<Vec<f64>, IkError> {
        match self.solve_past(cl, length, near)? {
            (q, None) => Ok(q),
            (_, Some(beyond)) => Err(beyond),
        }
    }

    /// [`Chain::solve`], but where every solution has an axis beyond its travel, the nearest
    /// of them all the same, with the axis that is beyond: where the machine would have to
    /// be, for showing it there and saying which axis ran out.
    pub fn solve_past(
        &self,
        cl: &Cl,
        length: f64,
        near: &[f64],
    ) -> Result<(Vec<f64>, Option<IkError>), IkError> {
        if !self.is_mill() {
            return self.search(cl, length, near).map(|q| (q, None));
        }
        let all = self.solutions(cl, length, near)?;
        let mut within: Option<(f64, Vec<f64>)> = None;
        let mut past: Option<(f64, Vec<f64>, IkError)> = None;
        for q in all {
            let cost = self.distance(&q, near);
            match self.beyond(&q) {
                Some(e) => {
                    if past.as_ref().is_none_or(|(c, _, _)| cost < *c) {
                        past = Some((cost, q, e));
                    }
                }
                None => {
                    if within.as_ref().is_none_or(|(c, _)| cost < *c) {
                        within = Some((cost, q));
                    }
                }
            }
        }
        match (within, past) {
            (Some((_, q)), _) => Ok((q, None)),
            (None, Some((_, q, e))) => Ok((q, Some(e))),
            (None, None) => Err(IkError::Unreachable),
        }
    }

    /// The machine's position at the end of each of a run of moves (`moves`: where each ends
    /// and the length of the tool it is made with), from `start`: each solved on from the one
    /// before, so the machine keeps to the side it is on. A move the machine cannot make
    /// leaves it where it was, and is listed with why.
    pub fn track(&self, moves: &[(Cl, f64)], start: &[f64]) -> AxisTrack {
        let mut rows = Vec::with_capacity(moves.len() + 1);
        let mut issues = Vec::new();
        let mut q = start.to_vec();
        rows.push(q.clone());
        for (k, (cl, length)) in moves.iter().enumerate() {
            // (A rotary the lean leaves free is turned to what the next lean needs.)
            let ahead = moves[k + 1..]
                .iter()
                .map(|m| m.0.axis)
                .find(|a| dist(*a, cl.axis) > 1e-9);
            let seeded = self.solve_toward(cl, *length, &q, ahead).ok();
            match self.solve_past(cl, *length, seeded.as_deref().unwrap_or(&q)) {
                Ok((next, beyond)) => {
                    q = next;
                    if let Some(e) = beyond {
                        issues.push((k, e));
                    }
                }
                Err(e) => issues.push((k, e)),
            }
            rows.push(q.clone());
        }
        AxisTrack { rows, issues }
    }

    /// How far apart two positions are for choosing between solutions: the turns in degrees,
    /// the slides hardly counting (a turn the other way round is what is to be avoided).
    pub fn distance(&self, a: &[f64], b: &[f64]) -> f64 {
        self.joints()
            .zip(a.iter().zip(b))
            .map(|(j, (x, y))| {
                if j.is_rotary() {
                    (x - y).abs()
                } else {
                    (x - y).abs() * 1e-4
                }
            })
            .sum()
    }

    /// Every closed-form solution for a mill ([`Chain::is_mill`]), each turn wound to the
    /// value nearest `near` that its travel allows (or left beyond it when none does: the
    /// caller tells by [`Chain::beyond`]). A turn the location leaves free (the tool along a
    /// rotary axis) stays at `near`'s.
    pub fn solutions(&self, cl: &Cl, length: f64, near: &[f64]) -> Result<Vec<Vec<f64>>, IkError> {
        let n = self.len();
        let nt = self.tool.len();
        let rot = self.rotaries();
        let (t, b): (Vec<usize>, Vec<usize>) = rot.iter().partition(|i| **i < nt);
        let dir = |i: usize| self.joint(i).dir;
        // The turn of joint `i` from home at `near`.
        let held =
            |i: usize| near.get(i).copied().unwrap_or(self.joint(i).home) - self.joint(i).home;
        // The work's axis and the spindle's, both at home in machine coordinates: the turns
        // must bring the one onto the other.
        let a0 = normalize(self.table_mount.dir_to_world(cl.axis));
        let z0 = self.tool_mount.z;
        // Each solution: the turn from home of every rotary, in `rot`'s order.
        let turns: Vec<Vec<f64>> = match (t.len(), b.len()) {
            (0, 0) => {
                let off = angle_between(a0, z0);
                if off > ORIENT_TOL {
                    return Err(IkError::Orientation { tilt: off });
                }
                vec![vec![]]
            }
            (0, 1) => vec![vec![one_angle(dir(b[0]), a0, z0, held(b[0]))?]],
            (1, 0) => vec![vec![one_angle(dir(t[0]), z0, a0, held(t[0]))?]],
            (0, 2) => two_angles(dir(b[0]), dir(b[1]), a0, z0, (held(b[0]), held(b[1])))?
                .into_iter()
                .map(|(p, q)| vec![p, q])
                .collect(),
            (2, 0) => two_angles(dir(t[0]), dir(t[1]), z0, a0, (held(t[0]), held(t[1])))?
                .into_iter()
                .map(|(p, q)| vec![p, q])
                .collect(),
            // The head turned by θ and the table by φ agree where
            // Rot(head, −θ)·Rot(table, φ)·a0 = z0.
            _ => two_angles(dir(t[0]), dir(b[0]), a0, z0, (-held(t[0]), held(b[0])))?
                .into_iter()
                .map(|(p, q)| vec![-p, q])
                .collect(),
        };
        let mut out = Vec::new();
        for turn in turns {
            let mut q = self.home();
            for (k, i) in rot.iter().enumerate() {
                let j = self.joint(*i);
                q[*i] = wind(j, j.home + turn[k], near.get(*i).copied().unwrap_or(j.home));
            }
            let q = self
                .slide_to(cl.tip, length, &q)
                .ok_or(IkError::Unreachable)?;
            debug_assert_eq!(q.len(), n);
            out.push(q);
        }
        Ok(out)
    }

    /// The axis values with `q`'s turns and the slides moved so that the tip of a tool
    /// `length` long is at `tip` (the table mount's coordinates): for a mill
    /// ([`Chain::is_mill`]; `None` for any other chain).
    pub fn slide_to(&self, tip: P3, length: f64, q: &[f64]) -> Option<Vec<f64>> {
        let lin = self.linears();
        if lin.len() != 3 {
            return None;
        }
        let mut q = q.to_vec();
        for i in &lin {
            q[*i] = self.joint(*i).home;
        }
        let tip0 = sub(self.tool_mount.origin, scale(self.tool_mount.z, length));
        let work = self.table_mount.to_world(tip);
        // The slides' part is linear: how far the tip is from the work's point with the slides
        // at home, and what a millimetre of each changes.
        let miss = |q: &[f64]| {
            sub(
                self.tool_pose(q).point(tip0),
                self.table_pose(q).point(work),
            )
        };
        let e0 = miss(&q);
        let mut m = [[0.0; 3]; 3];
        for (k, i) in lin.iter().enumerate() {
            let mut p = q.clone();
            p[*i] += 1.0;
            let c = sub(miss(&p), e0);
            for r in 0..3 {
                m[r][k] = c[r];
            }
        }
        let x = solve3(&m, scale(e0, -1.0))?;
        for (k, i) in lin.iter().enumerate() {
            q[*i] += x[k];
        }
        Some(q)
    }

    /// Inverse kinematics of any chain by damped least squares from `near`: the tip's
    /// position and the axis's direction are met, the turn about the tool's own axis is
    /// whatever lies nearest. Stops count: a step that would pass one is held on it.
    pub fn search(&self, cl: &Cl, length: f64, near: &[f64]) -> Result<Vec<f64>, IkError> {
        let n = self.len();
        if n == 0 {
            return Err(IkError::Unreachable);
        }
        let mut seed: Vec<f64> = (0..n)
            .map(|i| near.get(i).copied().unwrap_or(self.joint(i).home))
            .collect();
        // A few starts: the last position, then spread through the travel (a start at a
        // singular pose, an arm stretched straight, can stall).
        let mut rng = 0x2545_f491_4f6c_dd1du64;
        let mut best: Option<(f64, Vec<f64>)> = None;
        for attempt in 0..24 {
            if let Some(q) = self.descend(cl, length, &seed) {
                let cost = self.distance(&q, near);
                if attempt == 0 {
                    return Ok(q);
                }
                if best.as_ref().is_none_or(|(c, _)| cost < *c) {
                    best = Some((cost, q));
                }
                if attempt >= 8 {
                    break;
                }
            }
            for (i, s) in seed.iter_mut().enumerate() {
                let j = self.joint(i);
                rng ^= rng << 13;
                rng ^= rng >> 7;
                rng ^= rng << 17;
                let u = (rng >> 11) as f64 / (1u64 << 53) as f64;
                let (lo, hi) = if !j.continuous && j.min.is_finite() && j.max.is_finite() {
                    (j.min, j.max)
                } else {
                    (j.home - 180.0, j.home + 180.0)
                };
                *s = lo + (hi - lo) * u;
            }
        }
        best.map(|(_, q)| q).ok_or(IkError::Unreachable)
    }

    /// One run of the damped least squares from `seed`: the axis values when it arrives.
    fn descend(&self, cl: &Cl, length: f64, seed: &[f64]) -> Option<Vec<f64>> {
        // An axis a unit off counts as this far off at the tip (mm): the two are weighed
        // together.
        const REACH: f64 = 100.0;
        /// The most an axis moves in one step (mm or degrees).
        const MOST_STEP: f64 = 15.0;
        let n = self.len();
        let clamp = |q: &mut [f64]| {
            for (i, v) in q.iter_mut().enumerate() {
                let j = self.joint(i);
                if !(j.continuous && j.is_rotary()) {
                    *v = v.clamp(j.min, j.max);
                }
            }
        };
        let miss = |q: &[f64]| -> [f64; 6] {
            let at = self.locate(q, length);
            let p = sub(at.tip, cl.tip);
            let a = scale(sub(at.axis, cl.axis), REACH);
            [p[0], p[1], p[2], a[0], a[1], a[2]]
        };
        let norm = |r: &[f64; 6]| r.iter().map(|v| v * v).sum::<f64>().sqrt();
        let mut q = seed.to_vec();
        clamp(&mut q);
        let mut r = miss(&q);
        let mut lambda = 1e-3;
        for _ in 0..400 {
            let e = norm(&r);
            if e < 1e-9 {
                return Some(q);
            }
            // The Jacobian by central differences.
            let mut jac = vec![[0.0; 6]; n];
            for (i, col) in jac.iter_mut().enumerate() {
                let h = 1e-4;
                let (mut a, mut b) = (q.clone(), q.clone());
                a[i] += h;
                b[i] -= h;
                let (ra, rb) = (miss(&a), miss(&b));
                for k in 0..6 {
                    col[k] = (ra[k] - rb[k]) / (2.0 * h);
                }
            }
            // (JᵀJ + λ·diag) dq = −Jᵀr, retried with more damping while the step fails.
            let mut stepped = false;
            for _ in 0..12 {
                let mut m = vec![vec![0.0; n + 1]; n];
                for i in 0..n {
                    for j in 0..n {
                        m[i][j] = (0..6).map(|k| jac[i][k] * jac[j][k]).sum();
                    }
                    m[i][i] += lambda * (1.0 + m[i][i]);
                    m[i][n] = -(0..6).map(|k| jac[i][k] * r[k]).sum::<f64>();
                }
                let Some(dq) = gauss(m) else {
                    lambda *= 10.0;
                    continue;
                };
                // A step is kept short, so the search stays with the pose it started in.
                let longest = dq.iter().fold(0.0f64, |m, v| m.max(v.abs()));
                let k = if longest > MOST_STEP {
                    MOST_STEP / longest
                } else {
                    1.0
                };
                let mut p: Vec<f64> = q.iter().zip(&dq).map(|(a, b)| a + b * k).collect();
                clamp(&mut p);
                let rp = miss(&p);
                if norm(&rp) < e {
                    q = p;
                    r = rp;
                    lambda = (lambda * 0.3).max(1e-12);
                    stepped = true;
                    break;
                }
                lambda *= 10.0;
            }
            if !stepped {
                break;
            }
        }
        (norm(&r) < 1e-7).then_some(q)
    }

    /// The positions a straight cut from `from` (the machine at `q0`) to `to` needs so that,
    /// the axes running straight from each to the next, the tip stays within `tol.tip` of the
    /// line and the axis within `tol.axis` of the arc: `to`'s position last, any the move had
    /// to be cut at before it. The positions follow on from one another along the cut, so the
    /// machine stays on the side it started on where it can; where it cannot (an axis at its
    /// stop must come round the other way) the position is marked a [`Waypoint::leap`].
    pub fn linearize(
        &self,
        from: &Cl,
        q0: &[f64],
        to: &Cl,
        length: f64,
        tol: &Tolerance,
    ) -> Result<Vec<Waypoint>, IkError> {
        let q1 = self.follow(from, q0, to, length)?;
        self.linearize_to(from, q0, to, &q1, length, tol)
    }

    /// The position at the end of a move, reached by following it from `q0` a few degrees of
    /// lean at a time: the machine stays on the side it is on, where a solve of the far end
    /// alone might take the other.
    pub fn follow(&self, from: &Cl, q0: &[f64], to: &Cl, length: f64) -> Result<Vec<f64>, IkError> {
        let n = (angle_between(from.axis, to.axis) / 5.0).ceil().max(1.0) as usize;
        let mut q = q0.to_vec();
        for k in 1..=n {
            q = self.solve(&from.toward(to, k as f64 / n as f64), length, &q)?;
        }
        Ok(q)
    }

    /// [`Chain::linearize`] to the end position `q1` (the caller's choice among those that
    /// put the tool at `to`: a rotary the tool's lean leaves free may be turned to where the
    /// moves after want it).
    pub fn linearize_to(
        &self,
        from: &Cl,
        q0: &[f64],
        to: &Cl,
        q1: &[f64],
        length: f64,
        tol: &Tolerance,
    ) -> Result<Vec<Waypoint>, IkError> {
        let mut out = Vec::new();
        self.cut_up(from, to, (0.0, q0), 1.0, q1, length, tol, 0, &mut out)?;
        Ok(out)
    }

    /// The rotary axes the tool's lean at `cl` leaves free (the tool along such an axis:
    /// turning it changes nothing), for a mill.
    pub fn free_rotaries(&self, cl: &Cl, length: f64, near: &[f64]) -> Vec<usize> {
        if !self.is_mill() {
            return Vec::new();
        }
        let Ok((q, _)) = self.solve_past(cl, length, near) else {
            return Vec::new();
        };
        self.rotaries()
            .into_iter()
            .filter(|i| {
                let mut seed = q.clone();
                seed[*i] += 53.0;
                self.solve_past(cl, length, &seed)
                    .is_ok_and(|(p, _)| (p[*i] - seed[*i]).abs() < 1e-6)
            })
            .collect()
    }

    /// [`Chain::solve`] looking ahead: where the tool's lean at `cl` leaves a rotary axis
    /// free (upright over a platter), that axis is turned to what the lean `ahead` (the next
    /// lean the toolpath makes, the table mount's coordinates) will need, instead of being
    /// left where it was only to swing round the moment the tool tilts.
    pub fn solve_toward(
        &self,
        cl: &Cl,
        length: f64,
        near: &[f64],
        ahead: Option<P3>,
    ) -> Result<Vec<f64>, IkError> {
        let Some(axis) = ahead else {
            return self.solve(cl, length, near);
        };
        let free = self.free_rotaries(cl, length, near);
        if free.is_empty() {
            return self.solve(cl, length, near);
        }
        let mut seed = near.to_vec();
        let then = Cl {
            tip: cl.tip,
            axis: normalize(axis),
        };
        if let Ok((q, _)) = self.solve_past(&then, length, near) {
            for i in free {
                seed[i] = q[i];
            }
        }
        self.solve(cl, length, &seed)
    }

    /// Follows the cut from the position `a` to the place `tb` along it, adding the positions
    /// needed; the last added is `tb`'s.
    #[allow(clippy::too_many_arguments)]
    fn cut_up(
        &self,
        from: &Cl,
        to: &Cl,
        a: (f64, &[f64]),
        tb: f64,
        last: &[f64],
        length: f64,
        tol: &Tolerance,
        depth: usize,
        out: &mut Vec<Waypoint>,
    ) -> Result<(), IkError> {
        /// The most a rotary turns in one piece (degrees): a piece is judged by its middle
        /// alone, which a longer turn could pass by chance.
        const MOST_TURN: f64 = 20.0;
        let end = from.toward(to, tb);
        // The move's own end is where the caller put it; a place before it is solved on from
        // the way there (so a rotary left free by the lean turns steadily to the end's).
        let q = if tb >= 1.0 {
            last.to_vec()
        } else {
            let f = (tb - a.0) / (1.0 - a.0);
            let toward: Vec<f64> = a.1.iter().zip(last).map(|(x, y)| x + (y - x) * f).collect();
            self.solve(&end, length, &toward)?
        };
        // Where the machine is halfway between the two positions, against where the cut is.
        let tm = 0.5 * (a.0 + tb);
        let want = from.toward(to, tm);
        let mid: Vec<f64> = a.1.iter().zip(&q).map(|(x, y)| 0.5 * (x + y)).collect();
        let is = self.locate(&mid, length);
        let tip_off = if tol.tcp { 0.0 } else { dist(is.tip, want.tip) };
        let turn = self
            .joints()
            .zip(a.1.iter().zip(&q))
            .filter(|(j, _)| j.is_rotary())
            .map(|(_, (x, y))| (x - y).abs())
            .fold(0.0, f64::max);
        let fits = tip_off <= tol.tip
            && angle_between(is.axis, want.axis) <= tol.axis
            && turn <= MOST_TURN;
        if fits || depth >= 16 {
            out.push(Waypoint {
                t: tb,
                q,
                leap: !fits,
            });
            return Ok(());
        }
        self.cut_up(from, to, a, tm, last, length, tol, depth + 1, out)?;
        let at = out
            .last()
            .map(|w| w.q.clone())
            .unwrap_or_else(|| a.1.to_vec());
        self.cut_up(from, to, (tm, &at), tb, last, length, tol, depth + 1, out)
    }

    /// The least time (minutes) the axes need between two positions at their fastest rates
    /// (`rapid`: the rapid rates, else the feed limits).
    pub fn least_minutes(&self, a: &[f64], b: &[f64], rapid: bool) -> f64 {
        self.joints()
            .zip(a.iter().zip(b))
            .map(|(j, (x, y))| {
                let rate = if rapid { j.rapid } else { j.max_feed };
                if rate > 0.0 {
                    (x - y).abs() / rate
                } else {
                    0.0
                }
            })
            .fold(0.0, f64::max)
    }
}

/// The value equal to `angle` (degrees, whole turns apart) nearest `near` that `j`'s travel
/// allows; with none allowed, the nearest all the same.
fn wind(j: &Joint, angle: f64, near: f64) -> f64 {
    let nearest = angle + 360.0 * ((near - angle) / 360.0).round();
    if j.continuous || j.within(nearest) {
        return nearest;
    }
    // The turns apart within the travel, nearest first.
    let lo = ((j.min - LIMIT_SLACK - angle) / 360.0).ceil();
    let hi = ((j.max + LIMIT_SLACK - angle) / 360.0).floor();
    if lo > hi {
        return nearest;
    }
    let k = ((near - angle) / 360.0).round().clamp(lo, hi);
    angle + 360.0 * k
}

/// The turn θ (degrees) about `u` with `Rot(u, θ)·w = v`, `hold` where any turn does (both
/// along `u`).
fn one_angle(u: P3, w: P3, v: P3, hold: f64) -> Result<f64, IkError> {
    let off = (dot(u, w).clamp(-1.0, 1.0).acos() - dot(u, v).clamp(-1.0, 1.0).acos()).to_degrees();
    if off.abs() > ORIENT_TOL {
        return Err(IkError::Orientation { tilt: off.abs() });
    }
    Ok(about(u, w, v).unwrap_or(hold))
}

/// The angle (degrees) about `u` from `w`'s part square to it round to `v`'s (`None` when
/// either has none).
fn about(u: P3, w: P3, v: P3) -> Option<f64> {
    let a = sub(w, scale(u, dot(u, w)));
    let b = sub(v, scale(u, dot(u, v)));
    if len(a) < 1e-9 || len(b) < 1e-9 {
        return None;
    }
    Some(dot(u, cross(a, b)).atan2(dot(a, b)).to_degrees())
}

/// The turns (θ₁, θ₂), in degrees, with `Rot(u1, θ₁)·Rot(u2, θ₂)·w = v`: none when the two
/// axes cannot bring `w` onto `v`, else both solutions (one where they meet). A turn that any
/// value serves takes `hold`'s.
fn two_angles(u1: P3, u2: P3, w: P3, v: P3, hold: (f64, f64)) -> Result<Vec<(f64, f64)>, IkError> {
    // Along u1 the first turn changes nothing: u1·Rot(u2, θ₂)·w = u1·v fixes θ₂. With w's
    // part along u2 (kept) and square to it (turned), that is A·cos θ₂ + B·sin θ₂ = D.
    let along = dot(u2, w);
    let square = sub(w, scale(u2, along));
    let a = dot(u1, square);
    let b = dot(u1, cross(u2, w));
    let d = dot(u1, v) - dot(u1, u2) * along;
    let h = a.hypot(b);
    let mut second = Vec::new();
    if h < 1e-10 {
        // θ₂ moves nothing along u1 (w along u2, or u1 along u2): any value, if the rest fits.
        if d.abs() > 1e-8 {
            return Err(IkError::Orientation {
                tilt: d.abs().min(1.0).asin().to_degrees(),
            });
        }
        second.push(hold.1);
    } else {
        let c = d / h;
        if c.abs() > 1.0 + 1e-9 {
            // How far short the tilt falls.
            let reach = h.min(1.0).asin().to_degrees();
            let want = d.abs().min(1.0).asin().to_degrees();
            return Err(IkError::Orientation { tilt: want - reach });
        }
        let phi = b.atan2(a).to_degrees();
        let delta = c.clamp(-1.0, 1.0).acos().to_degrees();
        second.push(phi + delta);
        // (At 0° and at 180° the two are one turn.)
        if delta > 1e-9 && 180.0 - delta > 1e-9 {
            second.push(phi - delta);
        }
    }
    Ok(second
        .into_iter()
        .map(|t2| {
            let w2 = turned(w, u2, t2);
            (about(u1, w2, v).unwrap_or(hold.0), t2)
        })
        .collect())
}

/// `m·x = b` for a 3×3 `m` (`None` when it is singular).
fn solve3(m: &[[f64; 3]; 3], b: P3) -> Option<P3> {
    let rows = (0..3)
        .map(|i| vec![m[i][0], m[i][1], m[i][2], b[i]])
        .collect();
    let x = gauss(rows)?;
    Some([x[0], x[1], x[2]])
}

/// Gaussian elimination with partial pivoting on an n×(n+1) system (`None` when singular).
fn gauss(mut m: Vec<Vec<f64>>) -> Option<Vec<f64>> {
    let n = m.len();
    for c in 0..n {
        let p = (c..n).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs()))?;
        if m[p][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, p);
        for r in 0..n {
            if r != c {
                let f = m[r][c] / m[c][c];
                if f != 0.0 {
                    for k in c..=n {
                        m[r][k] -= f * m[c][k];
                    }
                }
            }
        }
    }
    Some((0..n).map(|i| m[i][n] / m[i][i]).collect())
}

pub mod samples;
#[cfg(test)]
mod tests;

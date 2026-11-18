//! The stock as a solid a cutter can cut from any side: a **tri-dexel** model.
//!
//! A height map ([`crate::heightmap`]) holds one height per column, so it can only be cut from
//! above. Here the stock is held on a lattice of points in its own frame by three grids of rays,
//! one grid along each lattice axis, a ray through every lattice line; each ray keeps the
//! stretches of itself that are inside material ([`Span`]s, their ends exact, not snapped to
//! the lattice). A surface is seen sharply by the rays that cross it, whichever way it faces,
//! which is what three grids are for: a wall is invisible to the rays that run along it.
//!
//! # Cutting
//!
//! [`TriDexel::cut`] takes away what a cutter sweeps through in one move: the tip going
//! straight, the axis turning steadily (by slerp) from one [`Pose`] to the next. The cutter is
//! a stack of **convex** pieces ([`SimCutter`]: frusta and zones of balls from
//! [`Cutter::convex`], tall ones cut into chunks). A move is split into sub-moves short enough
//! that each piece can be taken to slide, without turning, from where it is at the sub-move's
//! start to where it is at its end, held at the sub-move's middle axis. What a sliding convex
//! piece sweeps is convex, so the part of a ray inside it is one interval, found **exactly**:
//! with the ray's parameter `t` and the slide's `s` (0 to 1), "the ray's point, slid back by
//! `s`, is in the piece" is a conic region of the `(t, s)` plane (a quadratic inequality for
//! the cone or the ball) cut by straight lines (`0 ≤ s ≤ 1`, and the two planes that end the
//! piece). The interval's ends are the least and greatest `t` of that region, and those are
//! among a handful of points with closed forms: the conic's own extremes in `t`, where it
//! meets each line, and the lines' corners (`extent`). No sampling along the move.
//!
//! The error is all in the sub-moves, and bounded. With `eps` the cutter's tolerance
//! ([`SimCutter::eps`]), `l` the distance of a piece's farthest point from its middle, `hm`
//! its middle's height above the tip and `dphi` the angle the axis turns in the move, the
//! move is split into `n ≥ max(l·dphi / (2·eps), dphi·√(hm / (8·eps)))` sub-moves: the first
//! keeps a piece held at the middle axis within `eps` of where it truly is at either end of a
//! sub-move, the second keeps its middle's true path (an arc) within `eps` of the chord it is
//! slid along. Arcs off the axis (a bull mill's corner, a barrel's side) are frusta within
//! `eps` of the arc. So the cut surface is within `3·eps` of the true swept surface, and a
//! move that doesn't turn the tool (`dphi` 0, every 3-axis move) is cut exactly but for the
//! arcs' `eps`. The simulation uses a quarter of a cell for `eps` ([`sim_eps`]).
//!
//! Rays are rejected by the box of each sweep; a long diagonal move is cut in lengths so the
//! boxes stay tight. The three grids are cut side by side (rayon), a grid's rows too when a
//! cutter covers many; sums are taken in a fixed order, so one thread gives the same stock and
//! the same volumes as many.
//!
//! # The rest
//!
//! - [`TriDexel::hits`]: whether a part of the tool (the cutter, the shank above the flutes,
//!   the holder) would run into material during a move: the same intervals, tested instead of
//!   taken away.
//! - [`TriDexel::of_solid`]: a closed mesh's inside, by winding number along each ray, with a
//!   rasteriser's fill rule so a ray through an edge or a vertex counts each crossing once.
//! - [`TriDexel::compare`]: the stock against the part on the same lattice: where the stock's
//!   surface is inside the part (a gouge) and how deep, and the most stock left over it.
//! - [`TriDexel::mesh`]: a closed triangle mesh of the stock (marching cubes over the lattice,
//!   its vertices at the rays' span ends), coloured by which operation cut each surface.
//! - [`TriDexel::height_map`]: the stock as seen down any frame's Z, for the 3-axis patterns'
//!   rest machining.
//!
//! Volumes are the mean of the three grids' sums (each ray standing for a column one lattice
//! cell across, half a cell on the lattice's faces): exact for a block and for flat cuts
//! midway between lattice planes, within about a cell times the cut surface otherwise.

use std::sync::OnceLock;

use rayon::prelude::*;

use crate::cutter::{Convex, Cutter};
use crate::geom::{Aabb, Frame, P3, add, cross, dot, len, lerp, normalize, scale, sub};
use crate::heightmap::{EMPTY, HeightMap};
use crate::kinematics::slerp;
pub use crate::strategy::multiaxis::Pose;
use crate::tool::Tool;

/// The tag of a surface no operation has cut: the stock as it came.
pub const UNCUT: u8 = 0;
/// The tag [`TriDexel::mark_gouges`] gives a surface cut into the part (drawn red).
pub const GOUGE: u8 = 255;

/// Spans shorter than this (mm) are dropped: slivers left by rounding.
const MIN_SPAN: f32 = 1e-4;
/// A piece must be this far (mm) into material along a ray to count as hitting it.
const MIN_HIT: f64 = 1e-3;
/// How many rays a cut must cover before the grids (or a grid's rows) are cut on separate
/// threads.
const PARALLEL_RAYS: f64 = 20_000.0;

/// A stretch of a ray inside material, from `lo` to `hi` along the ray (mm from the lattice's
/// corner), with the tag of the operation that cut the surface at each end ([`UNCUT`] for the
/// stock's own).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub lo: f32,
    pub hi: f32,
    pub lo_tag: u8,
    pub hi_tag: u8,
}

/// A ray's material. Most rays hold one span, kept in place; only the others allocate.
#[derive(Debug, Clone, PartialEq, Default)]
enum Ray {
    #[default]
    Empty,
    One(Span),
    Many(Vec<Span>),
}

impl Ray {
    fn spans(&self) -> &[Span] {
        match self {
            Ray::Empty => &[],
            Ray::One(s) => std::slice::from_ref(s),
            Ray::Many(v) => v,
        }
    }

    fn spans_mut(&mut self) -> &mut [Span] {
        match self {
            Ray::Empty => &mut [],
            Ray::One(s) => std::slice::from_mut(s),
            Ray::Many(v) => v,
        }
    }

    fn of(mut spans: Vec<Span>) -> Ray {
        match spans.len() {
            0 => Ray::Empty,
            1 => Ray::One(spans[0]),
            _ => {
                spans.shrink_to_fit();
                Ray::Many(spans)
            }
        }
    }

    fn length(&self) -> f64 {
        self.spans().iter().map(|s| (s.hi - s.lo) as f64).sum()
    }

    /// Takes `a`…`b` out of the ray, the new ends tagged `tag`; returns the length removed.
    fn subtract(&mut self, a: f32, b: f32, tag: u8) -> f32 {
        // (A ray that only touches the cutter is not cut in two.)
        if b - a < MIN_SPAN {
            return 0.0;
        }
        // What is left of one span: the part under `a`, the part over `b`.
        let split = |s: Span| -> (Option<Span>, Option<Span>) {
            let left = (a > s.lo + MIN_SPAN).then_some(Span {
                hi: a,
                hi_tag: tag,
                ..s
            });
            let right = (b < s.hi - MIN_SPAN).then_some(Span {
                lo: b,
                lo_tag: tag,
                ..s
            });
            (left, right)
        };
        let width = |s: Option<Span>| s.map_or(0.0, |s| s.hi - s.lo);
        match self {
            Ray::Empty => 0.0,
            Ray::One(s) => {
                let s = *s;
                if b <= s.lo || a >= s.hi {
                    return 0.0;
                }
                let (left, right) = split(s);
                let removed = s.hi - s.lo - width(left) - width(right);
                *self = match (left, right) {
                    (Some(l), Some(r)) => Ray::Many(vec![l, r]),
                    (Some(x), None) | (None, Some(x)) => Ray::One(x),
                    (None, None) => Ray::Empty,
                };
                removed
            }
            Ray::Many(v) => {
                if !v.iter().any(|s| b > s.lo && a < s.hi) {
                    return 0.0;
                }
                let mut out = Vec::with_capacity(v.len() + 1);
                let mut removed = 0.0;
                for &s in v.iter() {
                    if b <= s.lo || a >= s.hi {
                        out.push(s);
                        continue;
                    }
                    let (left, right) = split(s);
                    removed += s.hi - s.lo - width(left) - width(right);
                    out.extend(left);
                    out.extend(right);
                }
                *self = Ray::of(out);
                removed
            }
        }
    }
}

/// The two axes across axis `a`, in turn: a grid of rays along `a` is indexed by them.
const fn others(a: usize) -> (usize, usize) {
    ((a + 1) % 3, (a + 2) % 3)
}

/// The rays along one lattice axis: `nu` by `nv` of them, row by row (`v` the row).
#[derive(Debug, Clone, PartialEq)]
struct Grid {
    nu: usize,
    nv: usize,
    rays: Vec<Ray>,
}

/// The lattice's spacing and size, for the code that works on one grid at a time.
#[derive(Debug, Clone, Copy)]
struct Lattice {
    step: P3,
    n: [usize; 3],
}

impl Lattice {
    fn size(&self) -> P3 {
        [0, 1, 2].map(|k| (self.n[k] - 1) as f64 * self.step[k])
    }

    /// The rays of the grid along `axis` that pass through the box `bb` (`u0, u1, v0, v1`),
    /// if the box reaches the lattice at all.
    fn range(&self, axis: usize, bb: &Aabb) -> Option<[usize; 4]> {
        let size = self.size();
        if bb.max[axis] < 0.0 || bb.min[axis] > size[axis] {
            return None;
        }
        let (b, c) = others(axis);
        let along = |k: usize| -> Option<(usize, usize)> {
            let lo = ((bb.min[k] - 1e-9) / self.step[k]).ceil().max(0.0);
            let hi = ((bb.max[k] + 1e-9) / self.step[k])
                .floor()
                .min((self.n[k] - 1) as f64);
            (hi >= lo).then_some((lo as usize, hi as usize))
        };
        let ((u0, u1), (v0, v1)) = (along(b)?, along(c)?);
        Some([u0, u1, v0, v1])
    }
}

/// How much of a column a ray at index `i` of `n` stands for: half on the lattice's faces.
fn weight(i: usize, n: usize) -> f64 {
    if i == 0 || i + 1 == n { 0.5 } else { 1.0 }
}

/// The stock (or a part, or the workholding) as three grids of rays on a lattice.
#[derive(Debug, Clone, PartialEq)]
pub struct TriDexel {
    /// The lattice's frame, in the coordinates everything is handed over in (poses, triangles,
    /// reported places): the model's.
    pub frame: Frame,
    /// The lattice's first point, in the frame's coordinates.
    pub min: P3,
    /// The spacing asked for (mm).
    pub cell: f64,
    /// The spacing along each axis: `cell`, adjusted so the lattice spans its box exactly.
    pub step: P3,
    /// Lattice points along each axis.
    pub n: [usize; 3],
    /// The rays along X (indexed by y, z), along Y (by z, x) and along Z (by x, y).
    grids: [Grid; 3],
}

/// A cell size for a stock of `size` at a `quality` from 0 (fast) to 1 (fine): 192 to 640
/// rays along its longest side (at most about 1.2 million rays, some tens of MB).
pub fn solid_cell(size: P3, quality: f64) -> f64 {
    let longest = size[0].max(size[1]).max(size[2]).max(1e-3);
    (longest / (192.0 + 448.0 * quality.clamp(0.0, 1.0))).max(0.01)
}

/// The tolerance the simulation cuts with on a lattice of `cell` (see the module docs).
pub fn sim_eps(cell: f64) -> f64 {
    (cell / 4.0).clamp(0.002, 0.25)
}

/// Which part of a tool to test ([`TriDexel::hits`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Which {
    /// The cutting part: its flutes.
    Cutter,
    /// The shank between the flutes and the holder.
    Shank,
    /// The holder and the arbor.
    Holder,
    /// The shank, the holder and the arbor: all that must not touch material.
    Assembly,
    /// The whole tool.
    All,
}

/// A tool as the convex pieces the simulation sweeps, each a solid of revolution about the
/// tool's axis at heights above the tip.
#[derive(Debug, Clone, PartialEq)]
pub struct SimCutter {
    /// The cutting part ([`Cutter::convex`]).
    pub cutter: Vec<Convex>,
    /// The shank above the flutes, up to the holder.
    pub shank: Vec<Convex>,
    /// The holder and the arbor on it.
    pub holder: Vec<Convex>,
    /// The tolerance its arcs were cut into frusta with, and moves are split by.
    pub eps: f64,
}

impl SimCutter {
    /// `tool` to a tolerance of `eps` (mm).
    pub fn new(tool: &Tool, eps: f64) -> SimCutter {
        let eps = eps.max(1e-4);
        let shape = Cutter::of(tool);
        let mut cutter = Vec::new();
        for k in shape.convex(eps) {
            chunks(k, &mut cutter);
        }
        let (mut shank, mut holder) = (Vec::new(), Vec::new());
        if !tool.is_beam() {
            let (r, from) = tool.shank();
            let from = from.max(shape.length);
            if tool.overall_length > from + 1e-9 && r > 0.0 {
                chunks(
                    Convex::Frustum {
                        r0: r,
                        h0: from,
                        r1: r,
                        h1: tool.overall_length,
                    },
                    &mut shank,
                );
            }
            // (The holder starts where the stick-out ends, even with flutes that reach into it.)
            let mut base = tool.overall_length;
            for profile in [&tool.holder, &tool.arbor].into_iter().flatten() {
                for w in profile.points.windows(2) {
                    let (lo, hi) = if w[0][1] <= w[1][1] {
                        (w[0], w[1])
                    } else {
                        (w[1], w[0])
                    };
                    if hi[1] - lo[1] > 1e-9 && lo[0].max(hi[0]) > 0.0 {
                        chunks(
                            Convex::Frustum {
                                r0: lo[0].max(0.0),
                                h0: base + lo[1],
                                r1: hi[0].max(0.0),
                                h1: base + hi[1],
                            },
                            &mut holder,
                        );
                    }
                }
                base += profile.length();
            }
        }
        SimCutter {
            cutter,
            shank,
            holder,
            eps,
        }
    }

    /// The pieces of a part of the tool, each pulled in by `slack` all round (and the tip
    /// lifted by it), so that running along a surface the tool itself cut isn't a hit.
    pub fn pieces(&self, which: Which, slack: f64) -> Vec<Convex> {
        let groups: Vec<&Vec<Convex>> = match which {
            Which::Cutter => vec![&self.cutter],
            Which::Shank => vec![&self.shank],
            Which::Holder => vec![&self.holder],
            Which::Assembly => vec![&self.shank, &self.holder],
            Which::All => vec![&self.cutter, &self.shank, &self.holder],
        };
        let all = groups.into_iter().flat_map(|g| g.iter().copied());
        if slack <= 0.0 {
            return all.collect();
        }
        all.filter_map(|k| match k {
            Convex::Frustum { r0, h0, r1, h1 } => {
                let h0 = if h0 <= 1e-9 { h0 + slack } else { h0 };
                (r0.max(r1) > slack && h1 > h0 + 1e-9).then_some(Convex::Frustum {
                    r0: (r0 - slack).max(0.0),
                    h0,
                    r1: (r1 - slack).max(0.0),
                    h1,
                })
            }
            Convex::Sphere { hc, rho, h0, h1 } => (rho > slack).then_some(Convex::Sphere {
                hc,
                rho: rho - slack,
                h0,
                h1,
            }),
        })
        .collect()
    }
}

/// `k` in chunks no taller than they are wide (a tall cylinder turning with the axis sweeps
/// far at its ends; a short piece doesn't, so needs fewer sub-moves).
fn chunks(k: Convex, out: &mut Vec<Convex>) {
    match k {
        Convex::Frustum { r0, h0, r1, h1 } => {
            let n = ((h1 - h0) / (2.0 * r0.max(r1).max(0.5)))
                .ceil()
                .clamp(1.0, 32.0) as usize;
            for i in 0..n {
                let (f0, f1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
                out.push(Convex::Frustum {
                    r0: r0 + (r1 - r0) * f0,
                    h0: h0 + (h1 - h0) * f0,
                    r1: r0 + (r1 - r0) * f1,
                    h1: h0 + (h1 - h0) * f1,
                });
            }
        }
        Convex::Sphere { .. } => out.push(k),
    }
}

/// A convex piece sliding without turning, in lattice coordinates: the tool's tip at `b` and
/// its axis along `a` (unit) at the start, moved by `d` at the end.
#[derive(Debug, Clone, Copy)]
struct Sweep {
    k: Convex,
    b: P3,
    a: P3,
    d: P3,
}

impl Sweep {
    /// The box of the piece at the start of the slide.
    fn rest_bounds(&self) -> Aabb {
        let mut bb = Aabb::EMPTY;
        match self.k {
            Convex::Frustum { r0, h0, r1, h1 } => {
                // Its two end discs: a disc of radius r across the axis reaches r·√(1 − aₖ²)
                // along coordinate k.
                for (r, h) in [(r0, h0), (r1, h1)] {
                    let c = add(self.b, scale(self.a, h));
                    let e = [0, 1, 2].map(|k| r * (1.0 - self.a[k] * self.a[k]).max(0.0).sqrt());
                    bb.include(sub(c, e));
                    bb.include(add(c, e));
                }
            }
            Convex::Sphere { hc, rho, .. } => {
                let c = add(self.b, scale(self.a, hc));
                bb.include(sub(c, [rho; 3]));
                bb.include(add(c, [rho; 3]));
            }
        }
        bb
    }

    /// The box of everything it sweeps.
    fn bounds(&self) -> Aabb {
        let bb = self.rest_bounds();
        let mut out = bb;
        out.include(add(bb.min, self.d));
        out.include(add(bb.max, self.d));
        out
    }

    /// The slide cut into lengths for the grid along `axis`: a long slide across both of the
    /// grid's directions has a box mostly empty of it, so it is taken in pieces whose boxes
    /// are about as wide as the piece. (The pieces sweep exactly what the whole does.)
    fn lengths(&self, axis: usize, lat: &Lattice) -> impl Iterator<Item = Sweep> + '_ {
        let (b, c) = others(axis);
        let bb = self.rest_bounds();
        let size = bb.size();
        let wide = size[b].max(size[c]) + lat.step[b].max(lat.step[c]);
        let m = (self.d[b].abs().min(self.d[c].abs()) / wide)
            .ceil()
            .clamp(1.0, 256.0) as usize;
        (0..m).map(move |i| Sweep {
            b: add(self.b, scale(self.d, i as f64 / m as f64)),
            d: scale(self.d, 1.0 / m as f64),
            ..*self
        })
    }

    /// The sweep as the rays along `axis` see it.
    fn form(&self, axis: usize) -> RayForm {
        let (beta, gamma) = (self.a[axis], dot(self.d, self.a));
        let dd = dot(self.d, self.d);
        let smax = if dd > 1e-18 { 1.0 } else { 0.0 };
        match self.k {
            Convex::Frustum { r0, h0, r1, h1 } => {
                // The cone's radius at height h is q0 + k·h.
                let k = (r1 - r0) / (h1 - h0);
                let q0 = r0 - k * h0;
                let m = 1.0 + k * k;
                RayForm {
                    axis,
                    centre: self.b,
                    a: self.a,
                    d: self.d,
                    qa: 1.0 - m * beta * beta,
                    qb: -2.0 * self.d[axis] + 2.0 * m * beta * gamma,
                    qc: dd - m * gamma * gamma,
                    beta,
                    gamma,
                    m,
                    lin: q0 * k,
                    con: q0 * q0,
                    lift: 0.0,
                    slab: Some((h0, h1)),
                    smax,
                    ball: false,
                }
            }
            Convex::Sphere { hc, rho, h0, h1 } => RayForm {
                axis,
                centre: add(self.b, scale(self.a, hc)),
                a: self.a,
                d: self.d,
                qa: 1.0,
                qb: -2.0 * self.d[axis],
                qc: dd,
                beta,
                gamma,
                m: 1.0,
                lin: 0.0,
                con: rho * rho,
                lift: hc,
                // A whole ball needs no end planes.
                slab: (h0 > hc - rho + 1e-9 || h1 < hc + rho - 1e-9).then_some((h0, h1)),
                smax,
                ball: true,
            },
        }
    }
}

/// A [`Sweep`] prepared for the rays along one axis: the part of the conic that is the same
/// for every ray, and what each ray's own part is worked out from.
struct RayForm {
    axis: usize,
    /// What a ray's offset is measured from: the tip for a frustum, the centre for a ball.
    centre: P3,
    a: P3,
    d: P3,
    /// The conic's `t²`, `t·s` and `s²` coefficients.
    qa: f64,
    qb: f64,
    qc: f64,
    /// How fast the height along the piece's axis changes with `t` and (negated) with `s`.
    beta: f64,
    gamma: f64,
    /// A cone's `1 + k²`, `q0·k` and `q0²` (its radius at height h is `q0 + k·h`); a ball's
    /// 1, 0 and radius².
    m: f64,
    lin: f64,
    con: f64,
    /// The height of `centre` above the tip (a ball's centre).
    lift: f64,
    /// The heights the piece lies between, if planes end it.
    slab: Option<(f64, f64)>,
    /// 1, or 0 for a piece that doesn't move.
    smax: f64,
    ball: bool,
}

impl RayForm {
    /// The stretch of the ray at `(pu, pv)` (lattice mm along the grid's two directions)
    /// inside the sweep, as coordinates along the ray.
    fn span(&self, pu: f64, pv: f64) -> Option<(f64, f64)> {
        let (b, c) = others(self.axis);
        // The ray's point level with the centre, from the centre: `t` is measured from there,
        // which keeps the numbers small.
        let mut w = [0.0; 3];
        w[b] = pu - self.centre[b];
        w[c] = pv - self.centre[c];
        let (wa, wd) = (dot(w, self.a), dot(w, self.d));
        let ww = w[b] * w[b] + w[c] * w[c];
        let (qd, qe, qf) = if self.ball {
            (0.0, -2.0 * wd, ww - self.con)
        } else {
            let g = self.m * wa + self.lin;
            (
                -2.0 * self.beta * g,
                -2.0 * wd + 2.0 * self.gamma * g,
                ww - self.m * wa * wa - 2.0 * self.lin * wa - self.con,
            )
        };
        let slab = self
            .slab
            .map(|(h0, h1)| [wa + self.lift, self.beta, self.gamma, h0, h1]);
        let (lo, hi) = extent([self.qa, self.qb, self.qc, qd, qe, qf], self.smax, slab)?;
        Some((lo + self.centre[self.axis], hi + self.centre[self.axis]))
    }
}

/// The real roots of `a·x² + b·x + c` (a line's one root when `a` is nothing beside the rest;
/// a double root where the discriminant is within rounding of zero).
fn roots(a: f64, b: f64, c: f64) -> ([f64; 2], usize) {
    let big = b.abs().max(c.abs());
    if a.abs() <= 1e-13 * big || a == 0.0 {
        return if b.abs() > 1e-300 {
            ([-c / b, 0.0], 1)
        } else {
            ([0.0; 2], 0)
        };
    }
    let mut disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        if disc < -1e-10 * (b * b + 4.0 * (a * c).abs()) {
            return ([0.0; 2], 0);
        }
        disc = 0.0;
    }
    let q = -0.5 * (b + b.signum() * disc.sqrt());
    if q == 0.0 {
        return ([0.0, 0.0], 1);
    }
    ([q / a, c / q], 2)
}

/// The least and greatest `t` of the region of the `(t, s)` plane where
/// `qa·t² + qb·t·s + qc·s² + qd·t + qe·s + qf ≤ 0`, `0 ≤ s ≤ smax` and, with a `slab`
/// (`[ho, beta, gamma, h0, h1]`), `h0 ≤ ho + beta·t − gamma·s ≤ h1`. The region is convex (it
/// is a convex piece's sweep seen along a ray: see the module docs), so its extremes in `t`
/// are among: the conic's own extremes, where it crosses each bounding line, and the corners
/// where two lines meet inside it. `None` when the region is empty.
fn extent(q: [f64; 6], smax: f64, slab: Option<[f64; 5]>) -> Option<(f64, f64)> {
    let [a, b, c, d, e, f] = q;
    let value = |t: f64, s: f64| (a * t + b * s + d) * t + (c * s + e) * s + f;
    // How far off zero rounding can leave the conic at a point found to be on it.
    let near = |t: f64, s: f64| {
        1e-7 * (1.0 + f.abs() + (d * t).abs() + (a * t * t).abs() + (e * s).abs() + c.abs())
    };
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    let mut take = |t: f64, s: f64| {
        if !t.is_finite() || s < -1e-9 || s > smax + 1e-9 {
            return;
        }
        if let Some([ho, beta, gamma, h0, h1]) = slab {
            let h = ho + beta * t - gamma * s;
            if h < h0 - 1e-9 || h > h1 + 1e-9 {
                return;
            }
        }
        if value(t, s) > near(t, s) {
            return;
        }
        lo = lo.min(t);
        hi = hi.max(t);
    };
    let ends: &[f64] = if smax > 0.0 { &[0.0, 1.0] } else { &[0.0] };
    // On a line of constant s: the start and the end of the slide.
    let on_s = |s: f64, take: &mut dyn FnMut(f64, f64)| {
        let (r, n) = roots(a, b * s + d, (c * s + e) * s + f);
        for &t in &r[..n] {
            take(t, s);
        }
    };
    for &s in ends {
        on_s(s, &mut take);
    }
    // The conic's own extremes in t: where its tangent runs along s.
    if c.abs() > 1e-20 {
        let (r, n) = roots(
            4.0 * a * c - b * b,
            4.0 * c * d - 2.0 * b * e,
            4.0 * c * f - e * e,
        );
        for &t in &r[..n] {
            take(t, -(b * t + e) / (2.0 * c));
        }
    }
    if let Some([ho, beta, gamma, h0, h1]) = slab {
        for hv in [h0, h1] {
            if beta.abs() >= gamma.abs() && beta.abs() > 1e-12 {
                // The end plane's line as t = t0 + g·s.
                let (t0, g) = ((hv - ho) / beta, gamma / beta);
                let (r, n) = roots(
                    (a * g + b) * g + c,
                    2.0 * a * t0 * g + b * t0 + d * g + e,
                    (a * t0 + d) * t0 + f,
                );
                for &s in &r[..n] {
                    take(t0 + g * s, s);
                }
            } else if gamma.abs() > 1e-12 {
                // As s = s0 + g·t (level, s = s0, when the ray runs across the piece's axis).
                let (s0, g) = ((ho - hv) / gamma, beta / gamma);
                let (r, n) = roots(
                    a + (b + c * g) * g,
                    b * s0 + 2.0 * c * s0 * g + d + e * g,
                    (c * s0 + e) * s0 + f,
                );
                for &t in &r[..n] {
                    take(t, s0 + g * t);
                }
            }
            // The corners with the slide's ends.
            if beta.abs() > 1e-12 {
                for &s in ends {
                    take((hv - ho + gamma * s) / beta, s);
                }
            }
        }
    }
    (lo <= hi).then_some((lo, hi))
}

/// Takes the sweeps out of one grid; returns the volume removed as this grid measures it.
fn cut_grid(g: &mut Grid, axis: usize, lat: &Lattice, sweeps: &[Sweep], tag: u8) -> f64 {
    let (b, c) = others(axis);
    let (nu, nv) = (g.nu, g.nv);
    let mut total = 0.0;
    for sweep in sweeps {
        for part in sweep.lengths(axis, lat) {
            let Some([u0, u1, v0, v1]) = lat.range(axis, &part.bounds()) else {
                continue;
            };
            let form = part.form(axis);
            let row = |v: usize, rays: &mut [Ray]| -> f64 {
                let pv = v as f64 * lat.step[c];
                let mut sum = 0.0;
                for (u, ray) in rays.iter_mut().enumerate().take(u1 + 1).skip(u0) {
                    if matches!(ray, Ray::Empty) {
                        continue;
                    }
                    if let Some((lo, hi)) = form.span(u as f64 * lat.step[b], pv) {
                        let removed = ray.subtract(lo as f32, hi as f32, tag);
                        if removed > 0.0 {
                            sum += removed as f64 * weight(u, nu);
                        }
                    }
                }
                sum * weight(v, nv)
            };
            let rows = &mut g.rays[v0 * nu..(v1 + 1) * nu];
            // Row sums are added in row order either way, so the total is the same on one
            // thread as on many.
            if ((u1 - u0 + 1) * (v1 - v0 + 1)) as f64 > PARALLEL_RAYS {
                let sums: Vec<f64> = rows
                    .par_chunks_mut(nu)
                    .enumerate()
                    .map(|(i, r)| row(v0 + i, r))
                    .collect();
                total += sums.iter().sum::<f64>();
            } else {
                let mut sum = 0.0;
                for (i, r) in rows.chunks_mut(nu).enumerate() {
                    sum += row(v0 + i, r);
                }
                total += sum;
            }
        }
    }
    total * lat.step[b] * lat.step[c]
}

/// What comparing a stock with its part found ([`TriDexel::compare`]).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Deviation {
    /// The deepest the stock's surface is inside the part (mm; 0 without a gouge).
    pub gouge_depth: f64,
    /// Places on the stock's surface more than the tolerance inside the part (up to
    /// [`crate::sim::MAX_REPORTED`]), and how many surface points there are in all.
    pub gouges: Vec<P3>,
    pub gouge_count: usize,
    /// The most stock left over the part (mm), measured from the stock's surface to the part.
    pub excess: f64,
}

/// A triangle mesh of a stock, for drawing it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SimMesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
    /// Each vertex's colour, by its tag ([`tag_color`]).
    pub colors: Vec<[f32; 4]>,
    /// Each vertex's tag: the operation that cut the surface there ([`UNCUT`], [`GOUGE`]).
    pub tags: Vec<u8>,
}

/// The colour a surface with `tag` is drawn in: the stock's own grey, red for a gouge, and a
/// colour of eight in turn for each operation.
pub fn tag_color(tag: u8) -> [f32; 4] {
    const OPERATIONS: [[f32; 3]; 8] = [
        [0.36, 0.62, 0.90],
        [0.42, 0.78, 0.55],
        [0.93, 0.73, 0.33],
        [0.70, 0.55, 0.88],
        [0.35, 0.78, 0.80],
        [0.90, 0.56, 0.42],
        [0.62, 0.74, 0.36],
        [0.85, 0.52, 0.72],
    ];
    let [r, g, b] = match tag {
        UNCUT => [0.62, 0.65, 0.70],
        GOUGE => [0.92, 0.12, 0.12],
        t => OPERATIONS[(t as usize - 1) % OPERATIONS.len()],
    };
    [r, g, b, 1.0]
}

/// The spans' distances from `x` to the ends of the span it is in (below, above), if it is in
/// one (ends included, within rounding).
fn inside(spans: &[Span], x: f32) -> Option<(f32, f32)> {
    spans
        .iter()
        .find(|s| x >= s.lo - 1e-4 && x <= s.hi + 1e-4)
        .map(|s| ((x - s.lo).max(0.0), (s.hi - x).max(0.0)))
}

/// How far `x` is from the nearest span end (`None` for a ray without material).
fn nearest(spans: &[Span], x: f32) -> Option<f32> {
    spans
        .iter()
        .flat_map(|s| [(x - s.lo).abs(), (x - s.hi).abs()])
        .reduce(f32::min)
}

impl TriDexel {
    /// An empty lattice over the box `b` (in `frame`'s coordinates) at about `cell`.
    fn empty(frame: Frame, b: &Aabb, cell: f64) -> TriDexel {
        let cell = cell.max(1e-3);
        let size = b.size().map(|s| s.max(1e-6));
        let n = [0, 1, 2].map(|k| ((size[k] / cell).round() as usize).max(1) + 1);
        let step = [0, 1, 2].map(|k| size[k] / (n[k] - 1) as f64);
        let grid = |axis: usize| {
            let (b, c) = others(axis);
            Grid {
                nu: n[b],
                nv: n[c],
                rays: vec![Ray::Empty; n[b] * n[c]],
            }
        };
        TriDexel {
            frame,
            min: if b.is_empty() { [0.0; 3] } else { b.min },
            cell,
            step,
            n,
            grids: [grid(0), grid(1), grid(2)],
        }
    }

    /// A solid block: the box `b` in `frame`'s coordinates, on a lattice of about `cell`
    /// (adjusted along each axis so the lattice ends on the box's faces).
    pub fn block(frame: Frame, b: &Aabb, cell: f64) -> TriDexel {
        let mut out = TriDexel::empty(frame, b, cell);
        let size = out.size();
        for (axis, g) in out.grids.iter_mut().enumerate() {
            g.rays.fill(Ray::One(Span {
                lo: 0.0,
                hi: size[axis] as f32,
                lo_tag: UNCUT,
                hi_tag: UNCUT,
            }));
        }
        out
    }

    /// The inside of a closed mesh (`tris`, in the coordinates `frame` is given in) on the
    /// lattice over the box `b` (in `frame`'s coordinates): give a part its stock's frame, box
    /// and cell and the two can be compared. Each ray is inside where the surfaces it has
    /// crossed leave it a winding number other than zero, so solids that overlap or touch
    /// make one, and a mesh wound inside out still fills.
    pub fn of_solid(frame: Frame, tris: &[[P3; 3]], b: &Aabb, cell: f64) -> TriDexel {
        let mut out = TriDexel::empty(frame, b, cell);
        let local: Vec<[P3; 3]> = tris.iter().map(|t| t.map(|p| out.to_lattice(p))).collect();
        let lat = out.lattice();
        let grids: Vec<Grid> = (0..3usize)
            .into_par_iter()
            .map(|axis| solid_grid(&local, axis, &lat))
            .collect();
        out.grids = grids.try_into().expect("three grids");
        out
    }

    /// A stock height map (in `map_frame`'s coordinates: each column material from its floor,
    /// or `bottom`, up to its height) as a solid on a lattice of about `cell` in that frame,
    /// over the map's points. Each lattice line takes the map's nearest column; a wall of the
    /// map (a step between two columns) stands midway between the two lattice lines it falls
    /// between.
    pub fn of_height_map(map: &HeightMap, map_frame: &Frame, bottom: f64, cell: f64) -> TriDexel {
        let top = map
            .z
            .iter()
            .copied()
            .filter(|z| *z != EMPTY)
            .fold(bottom as f32, f32::max) as f64;
        // The lattice spans the map's points (a block's map has them on the block's faces), so
        // with the map's own cell each lattice line is a column of the map.
        let b = Aabb::new(
            [map.origin[0], map.origin[1], bottom],
            [
                map.origin[0] + (map.nx - 1) as f64 * map.cell,
                map.origin[1] + (map.ny - 1) as f64 * map.cell,
                top.max(bottom + cell.max(1e-3)),
            ],
        );
        let mut out = TriDexel::empty(*map_frame, &b, cell);
        let (n, step, size) = (out.n, out.step, out.size());
        // The column over a lattice point: its floor and top above the lattice's bottom.
        let column = |i: usize, j: usize| -> Option<(f64, f64)> {
            let at = |k: usize, idx: usize, count: usize| {
                let x = b.min[k] + idx as f64 * step[k];
                (((x - map.origin[k]) / map.cell).round().max(0.0) as usize).min(count - 1)
            };
            let m = map.index(at(0, i, map.nx), at(1, j, map.ny));
            let z = map.z[m];
            if z == EMPTY {
                return None;
            }
            let floor = map.floor.as_ref().map_or(bottom, |f| f[m] as f64);
            let (lo, hi) = (
                (floor - bottom).clamp(0.0, size[2]),
                (z as f64 - bottom).clamp(0.0, size[2]),
            );
            (hi - lo > MIN_SPAN as f64).then_some((lo, hi))
        };
        let columns: Vec<Option<(f64, f64)>> = (0..n[0] * n[1])
            .map(|c| column(c % n[0], c / n[0]))
            .collect();
        let span = |lo: f64, hi: f64| Span {
            lo: lo as f32,
            hi: hi as f32,
            lo_tag: UNCUT,
            hi_tag: UNCUT,
        };
        // Along Z: the columns themselves.
        for (ray, col) in out.grids[2].rays.iter_mut().zip(&columns) {
            if let Some((lo, hi)) = *col {
                *ray = Ray::One(span(lo, hi));
            }
        }
        // Along X and Y: runs of lattice points whose columns hold the ray's height, each run
        // reaching half a lattice step past its end points (the columns' own width).
        for axis in 0..2 {
            let (bu, cv) = others(axis);
            let nu = out.grids[axis].nu;
            for (r, ray) in out.grids[axis].rays.iter_mut().enumerate() {
                let (u, v) = (r % nu, r / nu);
                let mut idx = [0usize; 3];
                idx[bu] = u;
                idx[cv] = v;
                let z = idx[2] as f64 * step[2];
                let mut spans = Vec::new();
                let mut run: Option<usize> = None;
                for i in 0..=n[axis] {
                    idx[axis] = i;
                    let filled = i < n[axis]
                        && columns[idx[1] * n[0] + idx[0]]
                            .is_some_and(|(lo, hi)| z >= lo - 1e-9 && z <= hi + 1e-9);
                    match (run, filled) {
                        (None, true) => run = Some(i),
                        (Some(first), false) => {
                            let lo = ((first as f64 - 0.5) * step[axis]).max(0.0);
                            let hi = ((i as f64 - 0.5) * step[axis]).min(size[axis]);
                            spans.push(span(lo, hi));
                            run = None;
                        }
                        _ => {}
                    }
                }
                *ray = Ray::of(spans);
            }
        }
        out
    }

    fn lattice(&self) -> Lattice {
        Lattice {
            step: self.step,
            n: self.n,
        }
    }

    /// The lattice's extent along each axis (mm).
    pub fn size(&self) -> P3 {
        self.lattice().size()
    }

    /// The lattice's box in its frame's coordinates.
    pub fn bounds(&self) -> Aabb {
        Aabb::new(self.min, add(self.min, self.size()))
    }

    /// A point of the model in lattice coordinates (mm from the lattice's first point, along
    /// its axes).
    pub fn to_lattice(&self, p: P3) -> P3 {
        sub(self.frame.to_local(p), self.min)
    }

    /// A lattice point back in the model's coordinates.
    pub fn from_lattice(&self, q: P3) -> P3 {
        self.frame.to_world(add(q, self.min))
    }

    /// The spans of the ray along `axis` at indices `u`, `v` of the next two axes in turn
    /// (X: y, z; Y: z, x; Z: x, y), as distances along the ray from the lattice's first point.
    pub fn spans(&self, axis: usize, u: usize, v: usize) -> &[Span] {
        let g = &self.grids[axis];
        g.rays[v * g.nu + u].spans()
    }

    /// Whether the lattice point nearest `q` (lattice coordinates) is in material, by the ray
    /// along Z through it (`false` off the lattice).
    fn filled(&self, q: P3) -> bool {
        let i = [0, 1, 2].map(|k| (q[k] / self.step[k]).round());
        if (0..3).any(|k| i[k] < 0.0 || i[k] >= self.n[k] as f64) {
            return false;
        }
        let z = (i[2] * self.step[2]) as f32;
        inside(self.spans(2, i[0] as usize, i[1] as usize), z).is_some()
    }

    /// The ray along `axis` through the lattice point `idx`.
    fn ray_at(&self, axis: usize, idx: [usize; 3]) -> &[Span] {
        let (b, c) = others(axis);
        self.spans(axis, idx[b], idx[c])
    }

    /// The material's volume (mm³): see the module docs for how exact.
    pub fn volume(&self) -> f64 {
        let mut total = 0.0;
        for (axis, g) in self.grids.iter().enumerate() {
            let (b, c) = others(axis);
            let mut sum = 0.0;
            for (v, row) in g.rays.chunks(g.nu).enumerate() {
                let mut r = 0.0;
                for (u, ray) in row.iter().enumerate() {
                    r += ray.length() * weight(u, g.nu);
                }
                sum += r * weight(v, g.nv);
            }
            total += sum * self.step[b] * self.step[c];
        }
        total / 3.0
    }

    /// How many spans the three grids hold (a measure of the stock's memory).
    pub fn span_count(&self) -> usize {
        self.grids
            .iter()
            .flat_map(|g| g.rays.iter())
            .map(|r| r.spans().len())
            .sum()
    }

    /// The pieces' slides for a move from `from` to `to` (see the module docs), in lattice
    /// coordinates.
    fn sweeps(&self, pieces: &[Convex], from: Pose, to: Pose, eps: f64) -> Vec<Sweep> {
        let (t0, t1) = (self.to_lattice(from.tip), self.to_lattice(to.tip));
        let a0 = normalize(self.frame.dir_to_local(from.axis));
        let a1 = normalize(self.frame.dir_to_local(to.axis));
        let dphi = len(cross(a0, a1)).atan2(dot(a0, a1));
        let eps = eps.max(1e-4);
        let mut n = 1.0f64;
        if dphi > 1e-9 {
            for k in pieces {
                let (h0, h1) = k.heights();
                let hm = (h0 + h1) / 2.0;
                let l = ((h1 - h0) / 2.0).hypot(k.max_radius());
                n = n
                    .max((l * dphi / (2.0 * eps)).ceil())
                    .max((dphi * (hm.abs() / (8.0 * eps)).sqrt()).ceil());
            }
        }
        let n = n.min(4096.0) as usize;
        let mut out = Vec::with_capacity(n * pieces.len());
        for i in 0..n {
            let (f0, f1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let (p0, p1) = (lerp(t0, t1, f0), lerp(t0, t1, f1));
            let (x0, x1, mid) = if dphi > 1e-9 {
                (
                    slerp(a0, a1, f0),
                    slerp(a0, a1, f1),
                    slerp(a0, a1, (f0 + f1) / 2.0),
                )
            } else {
                (a0, a0, a0)
            };
            for &k in pieces {
                let (h0, h1) = k.heights();
                let hm = (h0 + h1) / 2.0;
                // The piece's middle goes from where it truly starts to where it truly ends.
                let c0 = add(p0, scale(x0, hm));
                let c1 = add(p1, scale(x1, hm));
                out.push(Sweep {
                    k,
                    b: sub(c0, scale(mid, hm)),
                    a: mid,
                    d: sub(c1, c0),
                });
            }
        }
        out
    }

    /// Takes away what the cutting part of `c` sweeps moving from `from` to `to` (poses in the
    /// model's coordinates), tagging the surfaces it leaves `tag`. Returns the volume removed
    /// (mm³).
    pub fn cut(&mut self, c: &SimCutter, from: Pose, to: Pose, tag: u8) -> f64 {
        let sweeps = self.sweeps(&c.cutter, from, to, c.eps);
        let lat = self.lattice();
        // Roughly how many rays the cut covers, to decide whether threads are worth it.
        let (b0, c0) = others(0);
        let work: f64 = sweeps
            .iter()
            .map(|s| {
                let size = s.bounds().size();
                (size[b0] / lat.step[b0] + 1.0) * (size[c0] / lat.step[c0] + 1.0)
            })
            .sum();
        let [gx, gy, gz] = &mut self.grids;
        let (x, y, z) = if work > PARALLEL_RAYS {
            let (x, (y, z)) = rayon::join(
                || cut_grid(gx, 0, &lat, &sweeps, tag),
                || {
                    rayon::join(
                        || cut_grid(gy, 1, &lat, &sweeps, tag),
                        || cut_grid(gz, 2, &lat, &sweeps, tag),
                    )
                },
            );
            (x, y, z)
        } else {
            (
                cut_grid(gx, 0, &lat, &sweeps, tag),
                cut_grid(gy, 1, &lat, &sweeps, tag),
                cut_grid(gz, 2, &lat, &sweeps, tag),
            )
        };
        (x + y + z) / 3.0
    }

    /// Whether a part of the tool (`which`, pulled in by `slack` all round) would be in
    /// material anywhere on the move from `from` to `to`: a place where it is (in the model's
    /// coordinates), or `None`. Material thinner than the lattice between rays can be missed.
    pub fn hits(
        &self,
        c: &SimCutter,
        which: Which,
        from: Pose,
        to: Pose,
        slack: f64,
    ) -> Option<P3> {
        let pieces = c.pieces(which, slack);
        if pieces.is_empty() {
            return None;
        }
        let sweeps = self.sweeps(&pieces, from, to, c.eps);
        let lat = self.lattice();
        for (axis, g) in self.grids.iter().enumerate() {
            let (b, cv) = others(axis);
            for sweep in &sweeps {
                for part in sweep.lengths(axis, &lat) {
                    let Some([u0, u1, v0, v1]) = lat.range(axis, &part.bounds()) else {
                        continue;
                    };
                    let form = part.form(axis);
                    for v in v0..=v1 {
                        for u in u0..=u1 {
                            let spans = g.rays[v * g.nu + u].spans();
                            if spans.is_empty() {
                                continue;
                            }
                            let (pu, pv) = (u as f64 * lat.step[b], v as f64 * lat.step[cv]);
                            let Some((lo, hi)) = form.span(pu, pv) else {
                                continue;
                            };
                            for s in spans {
                                let (x0, x1) = (lo.max(s.lo as f64), hi.min(s.hi as f64));
                                if x1 - x0 > MIN_HIT {
                                    let mut q = [0.0; 3];
                                    q[axis] = (x0 + x1) / 2.0;
                                    q[b] = pu;
                                    q[cv] = pv;
                                    return Some(self.from_lattice(q));
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// The stock as seen down `frame`'s Z (a frame in the model's coordinates) over `area` (in
    /// that frame's): each column's top and bottom, on a grid of `cell`. Down the lattice's
    /// own Z it is exact (the nearest ray's first and last ends). Down any other direction it
    /// errs on the side of material, by up to about a lattice cell: every surface point the
    /// rays found covers the map cells within a lattice cell's reach of it, up to half a
    /// cell's rise above it.
    pub fn height_map(&self, frame: &Frame, area: &Aabb, cell: f64) -> HeightMap {
        let mut m = HeightMap::new([area.min[0], area.min[1]], [area.max[0], area.max[1]], cell);
        let mut floor = vec![EMPTY; m.nx * m.ny];
        let rel = self.frame.relative(frame);
        let aligned = rel.x[0] > 1.0 - 1e-9 && rel.y[1] > 1.0 - 1e-9 && rel.z[2] > 1.0 - 1e-9;
        if aligned {
            for j in 0..m.ny {
                for i in 0..m.nx {
                    let p = m.point(i, j);
                    let q = self.to_lattice(frame.to_world([p[0], p[1], 0.0]));
                    let (u, v) = ((q[0] / self.step[0]).round(), (q[1] / self.step[1]).round());
                    if u < 0.0 || v < 0.0 || u as usize >= self.n[0] || v as usize >= self.n[1] {
                        continue;
                    }
                    let spans = self.spans(2, u as usize, v as usize);
                    if let (Some(first), Some(last)) = (spans.first(), spans.last()) {
                        let k = m.index(i, j);
                        m.z[k] = (last.hi as f64 - q[2]) as f32;
                        floor[k] = (first.lo as f64 - q[2]) as f32;
                    }
                }
            }
            m.floor = Some(floor);
            return m;
        }
        // How far half a lattice cell reaches along each of the frame's axes.
        let axes = [frame.x, frame.y, frame.z].map(|a| self.frame.dir_to_local(a));
        let e = axes.map(|a| {
            0.5 * (a[0].abs() * self.step[0]
                + a[1].abs() * self.step[1]
                + a[2].abs() * self.step[2])
        });
        let reach = [e[0] + 0.25 * self.cell, e[1] + 0.25 * self.cell];
        let mut low = vec![f32::INFINITY; m.nx * m.ny];
        for (axis, g) in self.grids.iter().enumerate() {
            let (b, c) = others(axis);
            for (r, ray) in g.rays.iter().enumerate() {
                let (u, v) = (r % g.nu, r / g.nu);
                for s in ray.spans() {
                    for x in [s.lo, s.hi] {
                        let mut q = [0.0; 3];
                        q[axis] = x as f64;
                        q[b] = u as f64 * self.step[b];
                        q[c] = v as f64 * self.step[c];
                        let p = frame.to_local(self.from_lattice(q));
                        let i0 = ((p[0] - reach[0] - m.origin[0]) / m.cell).ceil().max(0.0);
                        let j0 = ((p[1] - reach[1] - m.origin[1]) / m.cell).ceil().max(0.0);
                        let i1 = ((p[0] + reach[0] - m.origin[0]) / m.cell)
                            .floor()
                            .min(m.nx as f64 - 1.0);
                        let j1 = ((p[1] + reach[1] - m.origin[1]) / m.cell)
                            .floor()
                            .min(m.ny as f64 - 1.0);
                        if i1 < i0 || j1 < j0 {
                            continue;
                        }
                        let (top, bottom) = ((p[2] + e[2]) as f32, (p[2] - e[2]) as f32);
                        for j in j0 as usize..=j1 as usize {
                            for i in i0 as usize..=i1 as usize {
                                let k = m.index(i, j);
                                if m.z[k] == EMPTY || top > m.z[k] {
                                    m.z[k] = top;
                                }
                                low[k] = low[k].min(bottom);
                            }
                        }
                    }
                }
            }
        }
        for (k, z) in m.z.iter().enumerate() {
            if *z != EMPTY {
                floor[k] = low[k];
            }
        }
        m.floor = Some(floor);
        m
    }

    /// The stock against its `part` (on the same lattice: [`TriDexel::of_solid`] with this
    /// stock's frame, box and cell): gouges deeper than `tol`, and the stock left over.
    ///
    /// Both are measured at the points where the rays leave the stock (its surface). How far
    /// such a point is from the part's surface is worked out from how far it is along each of
    /// the three axes. Where those three distances are to one face, the distance to it is the
    /// plane's (`1/d² = 1/dx² + 1/dy² + 1/dz²`): exact against a flat face whichever way it
    /// faces, close against a curved one. Whether they are is checked by looking just past
    /// where that plane would be: if the part is still there, the three are to different
    /// faces (a point deep in a block), and the depth is the least of them: exact for faces
    /// square to the axes, up to √3 over for a slanted one. Next to the part's edges a gouge
    /// under a cell deep can go unseen by the rays along it; the rays across it see it.
    pub fn compare(&self, part: &TriDexel, tol: f64) -> Deviation {
        self.deviation(part, tol, None)
    }

    /// [`TriDexel::compare`], and the gouged surfaces tagged [`GOUGE`] so the mesh shows them
    /// red.
    pub fn mark_gouges(&mut self, part: &TriDexel, tol: f64) -> Deviation {
        let mut marks = Vec::new();
        let d = self.deviation(part, tol, Some(&mut marks));
        for (axis, ray, span, upper) in marks {
            let s = &mut self.grids[axis].rays[ray].spans_mut()[span];
            if upper {
                s.hi_tag = GOUGE;
            } else {
                s.lo_tag = GOUGE;
            }
        }
        d
    }

    fn deviation(
        &self,
        part: &TriDexel,
        tol: f64,
        mut marks: Option<&mut Vec<(usize, usize, usize, bool)>>,
    ) -> Deviation {
        assert!(
            self.n == part.n,
            "the part must be on the stock's lattice ({:?} and {:?})",
            self.n,
            part.n
        );
        let mut out = Deviation::default();
        let tol32 = tol as f32;
        for (axis, g) in self.grids.iter().enumerate() {
            let (b, c) = others(axis);
            let step = self.step[axis] as f32;
            for (r, ray) in g.rays.iter().enumerate() {
                let (u, v) = (r % g.nu, r / g.nu);
                let own = part.grids[axis].rays[r].spans();
                for (k, s) in ray.spans().iter().enumerate() {
                    for (x, upper) in [(s.lo, false), (s.hi, true)] {
                        let mut idx = [0usize; 3];
                        idx[b] = u;
                        idx[c] = v;
                        let along = inside(own, x).map(|(l, h)| l.min(h));
                        if let Some(d_ray) = along.filter(|d| *d > tol32) {
                            // In the part: how deep, from the lattice point nearest it.
                            let i = ((x / step).round().max(0.0) as usize).min(self.n[axis] - 1);
                            idx[axis] = i;
                            let xp = i as f32 * step;
                            let mut depth = d_ray as f64;
                            if let Some((pl, ph)) = inside(own, xp)
                                && pl.min(ph) > 1e-5
                            {
                                // How far the lattice point is from the part's surface along
                                // each axis, and which way the nearer surface is.
                                let mut d = [0.0f64; 3];
                                let mut toward = [0.0f64; 3];
                                d[axis] = pl.min(ph) as f64;
                                toward[axis] = if ph <= pl { 1.0 } else { -1.0 };
                                let mut on_surface = false;
                                for other in [b, c] {
                                    let y = idx[other] as f32 * self.step[other] as f32;
                                    match inside(part.ray_at(other, idx), y) {
                                        Some((l, h)) => {
                                            d[other] = (l.min(h) as f64).max(1e-6);
                                            toward[other] = if h <= l { 1.0 } else { -1.0 };
                                        }
                                        // On the part's surface there, as near as the
                                        // lattice can tell.
                                        None => on_surface = true,
                                    }
                                }
                                depth = if on_surface {
                                    0.0
                                } else {
                                    let dp = 1.0
                                        / (1.0 / (d[0] * d[0])
                                            + 1.0 / (d[1] * d[1])
                                            + 1.0 / (d[2] * d[2]))
                                            .sqrt();
                                    // The way to that plane, and a point a cell and a half
                                    // past it.
                                    let n = [0, 1, 2].map(|k| toward[k] * dp / d[k]);
                                    let past =
                                        dp + 1.5 * self.step[0].max(self.step[1]).max(self.step[2]);
                                    let p = [0, 1, 2].map(|k| idx[k] as f64 * self.step[k]);
                                    if part.filled(add(p, scale(n, past))) {
                                        (d_ray as f64).min(d[b]).min(d[c])
                                    } else {
                                        (dp - n[axis] * (x - xp) as f64).clamp(0.0, d_ray as f64)
                                    }
                                };
                            }
                            if depth > tol {
                                out.gouge_count += 1;
                                out.gouge_depth = out.gouge_depth.max(depth);
                                if out.gouges.len() < crate::sim::MAX_REPORTED {
                                    let mut q = [0.0; 3];
                                    q[axis] = x as f64;
                                    q[b] = u as f64 * self.step[b];
                                    q[c] = v as f64 * self.step[c];
                                    out.gouges.push(self.from_lattice(q));
                                }
                                if let Some(m) = marks.as_mut() {
                                    m.push((axis, r, k, upper));
                                }
                            }
                        } else if along.is_none() {
                            // Outside the part: how far, from the lattice point just inside
                            // the stock.
                            let i = if upper {
                                ((x - 1e-4) / step).floor()
                            } else {
                                ((x + 1e-4) / step).ceil()
                            };
                            idx[axis] = (i.max(0.0) as usize).min(self.n[axis] - 1);
                            let mut inv = 0.0f64;
                            let mut on_part = false;
                            for ax in 0..3 {
                                let y = idx[ax] as f32 * self.step[ax] as f32;
                                let spans = part.ray_at(ax, idx);
                                if inside(spans, y).is_some() {
                                    on_part = true;
                                } else if let Some(d) = nearest(spans, y) {
                                    let d = (d as f64).max(1e-6);
                                    inv += 1.0 / (d * d);
                                }
                            }
                            if !on_part && inv > 0.0 {
                                out.excess = out.excess.max(1.0 / inv.sqrt());
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// A closed triangle mesh of the stock in the model's coordinates, wound outward: marching
    /// cubes over every `step`-th lattice point (1: all of them), each vertex where the ray
    /// along its lattice edge leaves or enters material, coloured by the tag of that surface.
    /// Each cube's triangles come from tracing the surface's loops round its faces, so both
    /// cubes at a face draw the same lines across it and every edge is shared by two
    /// triangles.
    pub fn mesh(&self, step: usize) -> SimMesh {
        let step = step.max(1);
        // The lattice indices kept along each axis (the last always).
        let fine: [Vec<usize>; 3] = [0, 1, 2].map(|k| {
            let mut v: Vec<usize> = (0..self.n[k]).step_by(step).collect();
            if *v.last().expect("a lattice point") != self.n[k] - 1 {
                v.push(self.n[k] - 1);
            }
            v
        });
        let nn = [fine[0].len(), fine[1].len(), fine[2].len()];
        let size = self.size();
        // A kept point's coordinate; a step beyond the lattice for the empty layer round it.
        let coord = |k: usize, i: isize| -> f64 {
            if i < 0 {
                -self.step[k]
            } else if i as usize >= nn[k] {
                size[k] + self.step[k]
            } else {
                fine[k][i as usize] as f64 * self.step[k]
            }
        };
        // Whether the kept points of a layer are in material, by the rays along Z.
        let layer = |kz: isize| -> Vec<bool> {
            let mut occ = vec![false; nn[0] * nn[1]];
            if kz < 0 || kz as usize >= nn[2] {
                return occ;
            }
            let z = coord(2, kz) as f32;
            for j in 0..nn[1] {
                for i in 0..nn[0] {
                    occ[j * nn[0] + i] = inside(self.spans(2, fine[0][i], fine[1][j]), z).is_some();
                }
            }
            occ
        };
        let mut mesh = SimMesh::default();
        // Where the surface crosses the lattice edge from the kept point `at` to the next
        // along `axis` (`full`: `at` is in material), and its tag.
        let crossing = |axis: usize, at: [isize; 3], full: bool| -> (P3, u8) {
            let (b, c) = others(axis);
            let (xa, xb) = (coord(axis, at[axis]), coord(axis, at[axis] + 1));
            let spans = self.spans(axis, fine[b][at[b] as usize], fine[c][at[c] as usize]);
            let (lo, hi) = (xa as f32 - 1e-3, xb as f32 + 1e-3);
            let found = if full {
                spans
                    .iter()
                    .find(|s| s.hi >= lo && s.hi <= hi)
                    .map(|s| (s.hi, s.hi_tag))
            } else {
                spans
                    .iter()
                    .find(|s| s.lo >= lo && s.lo <= hi)
                    .map(|s| (s.lo, s.lo_tag))
            };
            // (Where the grids disagree by a hair: the middle of the edge.)
            let (x, tag) = found.map_or(((xa + xb) / 2.0, UNCUT), |(x, t)| (x as f64, t));
            let mut q = [0.0; 3];
            q[axis] = x.clamp(0.0, size[axis]);
            q[b] = coord(b, at[b]);
            q[c] = coord(c, at[c]);
            (q, tag)
        };
        // The vertices made so far on the edges along X and Y in two layers (the cubes' floor
        // and ceiling) and along Z between them. X edges start one before the lattice, so
        // theirs are shifted by one along X; Y edges along Y.
        const NONE: u32 = u32::MAX;
        let (wx, wy) = (nn[0] + 1, nn[0]);
        let mut ex = [vec![NONE; wx * nn[1]], vec![NONE; wx * nn[1]]];
        let mut ey = [vec![NONE; wy * (nn[1] + 1)], vec![NONE; wy * (nn[1] + 1)]];
        let mut ez = vec![NONE; nn[0] * nn[1]];
        let mut occ = [layer(-1), layer(0)];
        let table = cube_table();
        for kz in -1..nn[2] as isize {
            let filled = |l: usize, i: isize, j: isize| -> bool {
                i >= 0
                    && j >= 0
                    && (i as usize) < nn[0]
                    && (j as usize) < nn[1]
                    && occ[l][j as usize * nn[0] + i as usize]
            };
            for j in -1..nn[1] as isize {
                for i in -1..nn[0] as isize {
                    let mut mask = 0usize;
                    for corner in 0..8usize {
                        let (dx, dy, dz) = (corner & 1, (corner >> 1) & 1, corner >> 2);
                        if filled(dz, i + dx as isize, j + dy as isize) {
                            mask |= 1 << corner;
                        }
                    }
                    if mask == 0 || mask == 255 {
                        continue;
                    }
                    let mut vertex = |edge: u8, mesh: &mut SimMesh| -> u32 {
                        let (axis, p, q) = (
                            (edge / 4) as usize,
                            (edge & 1) as isize,
                            ((edge >> 1) & 1) as isize,
                        );
                        // The edge's first point, and where its vertex index is kept.
                        let (at, slot): ([isize; 3], &mut u32) = match axis {
                            0 => (
                                [i, j + p, kz + q],
                                &mut ex[q as usize][(j + p) as usize * wx + (i + 1) as usize],
                            ),
                            1 => (
                                [i + p, j, kz + q],
                                &mut ey[q as usize][(j + 1) as usize * wy + (i + p) as usize],
                            ),
                            _ => (
                                [i + p, j + q, kz],
                                &mut ez[(j + q) as usize * nn[0] + (i + p) as usize],
                            ),
                        };
                        if *slot == NONE {
                            let l = (at[2] - kz) as usize;
                            let (pos, tag) = crossing(axis, at, filled(l, at[0], at[1]));
                            *slot = mesh.positions.len() as u32;
                            mesh.positions
                                .push(self.from_lattice(pos).map(|x| x as f32));
                            mesh.tags.push(tag);
                        }
                        *slot
                    };
                    for t in &table[mask] {
                        let tri = t.map(|e| vertex(e, &mut mesh));
                        mesh.indices.extend(tri);
                    }
                }
            }
            // Up a layer.
            ex.swap(0, 1);
            ex[1].fill(NONE);
            ey.swap(0, 1);
            ey[1].fill(NONE);
            ez.fill(NONE);
            occ.swap(0, 1);
            occ[1] = layer(kz + 2);
        }
        // Normals: each vertex's triangles' normals, weighted by area.
        let mut normals = vec![[0.0f64; 3]; mesh.positions.len()];
        for t in mesh.indices.chunks(3) {
            let p = [0, 1, 2].map(|k| mesh.positions[t[k] as usize].map(|x| x as f64));
            let n = cross(sub(p[1], p[0]), sub(p[2], p[0]));
            for &k in t {
                normals[k as usize] = add(normals[k as usize], n);
            }
        }
        mesh.normals = normals
            .into_iter()
            .map(|n| {
                let n = if len(n) > 1e-12 {
                    normalize(n)
                } else {
                    self.frame.z
                };
                n.map(|x| x as f32)
            })
            .collect();
        mesh.colors = mesh.tags.iter().map(|t| tag_color(*t)).collect();
        mesh
    }
}

/// The rays along `axis` through a closed mesh (`tris`, in lattice coordinates): see
/// [`TriDexel::of_solid`].
/// How far inside the lattice's outermost planes their rays are taken, in steps.
const HAIR: f64 = 1e-7;

fn solid_grid(tris: &[[P3; 3]], axis: usize, lat: &Lattice) -> Grid {
    let (b, c) = others(axis);
    let (nu, nv) = (lat.n[b], lat.n[c]);
    let size = lat.size();
    // Every crossing: the ray, how far along it, and +1 going into the solid, −1 out of it.
    let mut hits: Vec<(u32, f32, i8)> = Vec::new();
    // The edge function of `p` against the edge a→b, worked out from the edge's ends in a
    // fixed order so the two triangles sharing an edge get exactly opposite values.
    let edge = |a: [f64; 2], b: [f64; 2], p: [f64; 2]| -> f64 {
        if (a[0], a[1]) <= (b[0], b[1]) {
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
        } else {
            -((a[0] - b[0]) * (p[1] - b[1]) - (a[1] - b[1]) * (p[0] - b[0]))
        }
    };
    for t in tris {
        let p = t.map(|q| [q[b], q[c]]);
        let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
        let normal = cross(e1, e2);
        // The normal's part along the ray is twice the triangle's area as the rays see it.
        let area = normal[axis];
        if area == 0.0 || !area.is_finite() {
            continue;
        }
        let sign = area.signum();
        let range = |k: usize, step: f64, count: usize| -> Option<(usize, usize)> {
            let lo = p.iter().map(|q| q[k]).fold(f64::INFINITY, f64::min);
            let hi = p.iter().map(|q| q[k]).fold(f64::NEG_INFINITY, f64::max);
            // (A hair wider than the triangle: the rays in the lattice's outermost planes are
            // taken a hair inside them, below.)
            let (i0, i1) = (
                ((lo - 2.0 * HAIR * step) / step).ceil().max(0.0),
                ((hi + 2.0 * HAIR * step) / step)
                    .floor()
                    .min(count as f64 - 1.0),
            );
            (i1 >= i0).then_some((i0 as usize, i1 as usize))
        };
        let (Some((u0, u1)), Some((v0, v1))) =
            (range(0, lat.step[b], nu), range(1, lat.step[c], nv))
        else {
            continue;
        };
        for v in v0..=v1 {
            for u in u0..=u1 {
                // A ray in the lattice's outermost plane is taken a hair inside it: a solid that
                // fills the lattice's box has faces lying in those planes, and a ray lying in a
                // face would count as inside on one side of the box only.
                let inside = |k: usize, i: usize, count: usize| {
                    let (hair, at) = (HAIR * lat.step[k], i as f64 * lat.step[k]);
                    at.clamp(hair, (count - 1) as f64 * lat.step[k] - hair)
                };
                let q = [inside(b, u, nu), inside(c, v, nv)];
                // Inside, or on an edge the triangle owns: of the two triangles at an edge
                // exactly one owns it (by which way the edge runs), so a ray through an edge
                // or a vertex crosses the surface once, as a ray a hair to one side would.
                let covered = (0..3).all(|i| {
                    let (a, bb) = (p[i], p[(i + 1) % 3]);
                    let w = sign * edge(a, bb, q);
                    let dir = [sign * (bb[0] - a[0]), sign * (bb[1] - a[1])];
                    w > 0.0 || (w == 0.0 && (dir[1] > 0.0 || (dir[1] == 0.0 && dir[0] > 0.0)))
                });
                if !covered {
                    continue;
                }
                let depth = t[0][axis]
                    - (normal[b] * (q[0] - t[0][b]) + normal[c] * (q[1] - t[0][c])) / area;
                hits.push((
                    (v * nu + u) as u32,
                    depth.clamp(0.0, size[axis]) as f32,
                    if area > 0.0 { -1 } else { 1 },
                ));
            }
        }
    }
    // Along each ray, in order; where one solid ends as another begins, into the next first.
    hits.sort_unstable_by(|x, y| x.0.cmp(&y.0).then(x.1.total_cmp(&y.1)).then(y.2.cmp(&x.2)));
    let mut rays = vec![Ray::Empty; nu * nv];
    let mut i = 0;
    while i < hits.len() {
        let ray = hits[i].0;
        let mut spans: Vec<Span> = Vec::new();
        let (mut winding, mut start) = (0i32, 0.0f32);
        while i < hits.len() && hits[i].0 == ray {
            let (_, x, s) = hits[i];
            let before = winding;
            winding += s as i32;
            if before == 0 && winding != 0 {
                start = x;
            } else if before != 0 && winding == 0 && x - start > MIN_SPAN {
                // (Two solids that touch run on as one span.)
                match spans.last_mut() {
                    Some(last) if start - last.hi <= MIN_SPAN => last.hi = x,
                    _ => spans.push(Span {
                        lo: start,
                        hi: x,
                        lo_tag: UNCUT,
                        hi_tag: UNCUT,
                    }),
                }
            }
            i += 1;
        }
        rays[ray as usize] = Ray::of(spans);
    }
    Grid { nu, nv, rays }
}

/// The triangles of each of the 256 ways a cube's corners can be in or out of material, as
/// triples of cube edges (edge `4·axis + p + 2·q`: along `axis`, at offsets `p` and `q` along
/// the other two axes in X, Y, Z order; corner `x + 2·y + 4·z`).
///
/// Worked out rather than tabled: on each face the surface runs from each edge where a walk
/// round the face (counter-clockwise seen from outside) leaves material to the edge where the
/// walk came into it; the lines join up into loops round the cube, and each loop is a fan of
/// triangles. Where a face has two corners in material diagonally across, each is cut off on
/// its own: the rule only looks at the face, so the cubes on both sides of it agree.
fn cube_table() -> &'static [Vec<[u8; 3]>] {
    static TABLE: OnceLock<Vec<Vec<[u8; 3]>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        // Each face's corners, counter-clockwise seen from outside.
        const FACES: [[usize; 4]; 6] = [
            [1, 3, 7, 5], // +X
            [0, 4, 6, 2], // −X
            [2, 6, 7, 3], // +Y
            [0, 1, 5, 4], // −Y
            [4, 5, 7, 6], // +Z
            [0, 2, 3, 1], // −Z
        ];
        // The cube edge between two corners next to each other.
        let edge_of = |a: usize, b: usize| -> u8 {
            let axis = (a ^ b).trailing_zeros() as usize;
            let low = a & b;
            let bit = |k: usize| (low >> k) & 1;
            let (p, q) = match axis {
                0 => (bit(1), bit(2)),
                1 => (bit(0), bit(2)),
                _ => (bit(0), bit(1)),
            };
            (axis * 4 + p + 2 * q) as u8
        };
        (0..256usize)
            .map(|mask| {
                let full = |corner: usize| (mask >> corner) & 1 == 1;
                let mut next = [u8::MAX; 12];
                for face in FACES {
                    for i in 0..4 {
                        if !(full(face[i]) && !full(face[(i + 1) % 4])) {
                            continue;
                        }
                        // Leaving material after corner i: back along the corners in
                        // material to where the walk came in.
                        let mut j = i;
                        while full(face[(j + 3) % 4]) && (j + 3) % 4 != i {
                            j = (j + 3) % 4;
                        }
                        let leave = edge_of(face[i], face[(i + 1) % 4]);
                        let enter = edge_of(face[(j + 3) % 4], face[j]);
                        next[leave as usize] = enter;
                    }
                }
                let mut tris = Vec::new();
                let mut seen = [false; 12];
                for first in 0..12u8 {
                    if next[first as usize] == u8::MAX || seen[first as usize] {
                        continue;
                    }
                    let mut ring = Vec::new();
                    let mut e = first;
                    while !seen[e as usize] {
                        seen[e as usize] = true;
                        ring.push(e);
                        e = next[e as usize];
                    }
                    for k in 1..ring.len().saturating_sub(1) {
                        tris.push([ring[0], ring[k + 1], ring[k]]);
                    }
                }
                tris
            })
            .collect()
    })
}

#[cfg(test)]
mod tests;

//! What a pipe fills as it slides, exactly (a coil spring pushed through a plate).
//!
//! A pipe here is everything within r of a spine curve C: a round section swept along it with
//! a ball at each end. Slid along d it fills everything within r of the strip C(s) + λd the
//! spine sweeps, so its boundary is made of that strip's two offsets (each the extrusion along
//! d of a silhouette curve C ± r n, n square to the spine and to d), the back half of the pipe
//! where it starts and the front half where it stops, half a cylinder along d by each end of
//! the spine, and a lune of a ball at each end of those. Where the spine's shadow along d laps
//! over itself (one coil behind another) these run into one another, and what is boundary is
//! told as [`super::sweep_tube`] tells it: a point of one of them is inside when some other
//! part of the strip is nearer it than r, each face is traced in its own parameters where that
//! changes, and [`super::sweep_moving::boundary_of_some`] cuts and joins them.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::sweep_tube::{
    Chain, CurvePt, Field, chain_runs, cover_curves, drop_twins, onto_boundary, tensor,
};
use super::topo::Body;
use crate::*;
use std::f64::consts::{PI, TAU};

/// A pipe as it was made: its spine's curves in order (each from its `.1` to its `.2`), its
/// radius, and whether a ball closes each end.
#[derive(Clone)]
pub(crate) struct Pipe {
    pub segs: Vec<(Curve, f64, f64)>,
    pub r: f64,
    pub balls: [bool; 2],
}

/// A face of what the slid pipe may bound, over a box of its own surface's parameters.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// An offset of the strip, on the side `.0` of it: (s, λ).
    Wall(f64),
    /// Half the pipe where it starts (0.0, its back) or stops (1.0, its front): (s, angle from
    /// the wall of side +1 round to the other).
    Pipe(f64),
    /// Half a cylinder along the slide by the spine's first end (0) or last (1): (angle, how
    /// far along).
    Cyl(usize),
    /// A lune of the ball at an end of the spine, where the pipe starts (0.0) or stops (1.0):
    /// (longitude, latitude) about the wall's normal there.
    Lune(usize, f64),
}

struct Cand {
    kind: Kind,
    bx: ((f64, f64), (f64, f64)),
    grid: (usize, usize),
}

/// The pipe and its slide, with what its inside test samples.
struct Slid<'a> {
    pipe: &'a Pipe,
    d: V3,
    len: f64,
    e: V3,
    size: f64,
    /// The spine's points at SAMPLES to a curve.
    pts: Vec<P3>,
    /// The longest run of the spine between two of them.
    step: f64,
}

const SAMPLES: usize = 8;

impl Slid<'_> {
    /// How many curves the spine is of: its parameter s runs from nothing to that, one to a
    /// curve.
    fn n(&self) -> f64 {
        self.pipe.segs.len() as f64
    }

    /// The spine's point and its derivative at s.
    fn at(&self, s: f64) -> (P3, V3) {
        let n = self.pipe.segs.len();
        let i = (s.floor().max(0.0) as usize).min(n - 1);
        let (c, t0, t1) = &self.pipe.segs[i];
        let (p, d1, _) = c.d2(t0 + (t1 - t0) * (s - i as f64));
        (p, d1 * (t1 - t0))
    }

    /// At s: the spine's point, its unit tangent, the wall's normal (square to the spine and
    /// to the slide) and the normal square to both that looks back along the slide.
    fn frame(&self, s: f64) -> (P3, V3, V3, V3) {
        let (p, d1) = self.at(s);
        let tau = d1.normalize();
        let n = tau.cross(&self.e).normalize();
        (p, tau, n, tau.cross(&n))
    }

    fn delta(&self) -> f64 {
        1e-11 * self.size
    }

    /// How far `x` is from the stretch of the strip at s (the spine's point there, slid).
    fn dist(&self, s: f64, x: &P3) -> f64 {
        let w = x - self.at(s.clamp(0.0, self.n())).0;
        let l = w.dot(&self.e).clamp(0.0, self.len);
        (w - self.e * l).norm()
    }

    /// How fast the square of [`Self::dist`] falls with s (half of it): what is left of `x`
    /// from the strip's nearest point at s, along the spine's own direction there.
    fn slope(&self, s: f64, x: &P3) -> f64 {
        let (p, d1) = self.at(s);
        let w = x - p;
        let l = w.dot(&self.e).clamp(0.0, self.len);
        (w - self.e * l).dot(&d1)
    }

    /// The least of [`Self::dist`] between a and b: where its slope turns, found between
    /// them by false position (an end where it only rises or falls from it).
    fn golden(&self, x: &P3, mut a: f64, mut b: f64) -> (f64, f64) {
        let (mut ga, mut gb) = (self.slope(a, x), self.slope(b, x));
        // (Falling still at b, or rising already at a: the end.)
        if ga <= 0.0 {
            return (a, self.dist(a, x));
        }
        if gb >= 0.0 {
            return (b, self.dist(b, x));
        }
        let mut m = 0.5 * (a + b);
        for _ in 0..60 {
            m = (a * gb - b * ga) / (gb - ga);
            if !(m > a && m < b) {
                m = 0.5 * (a + b);
            }
            let gm = self.slope(m, x);
            if gm.abs() <= 1e-13 * self.size * self.size || b - a <= 1e-11 {
                break;
            }
            // (The Illinois rule: the end that stays is halved, so that both close in.)
            if gm > 0.0 {
                (a, ga) = (m, gm);
                gb *= 0.5;
            } else {
                (b, gb) = (m, gm);
                ga *= 0.5;
            }
        }
        (m, self.dist(m, x))
    }

    /// Every local least of [`Self::dist`] along the spine for `x`, the nearest first: (s,
    /// the distance).
    fn leasts(&self, x: &P3) -> Vec<(f64, f64)> {
        let h = 1.0 / SAMPLES as f64;
        let vals: Vec<f64> = self
            .pts
            .iter()
            .map(|p| {
                let w = x - p;
                let l = w.dot(&self.e).clamp(0.0, self.len);
                (w - self.e * l).norm()
            })
            .collect();
        let m = vals.len();
        // (Only those a sample puts within r and the spine's run between two samples: the
        // least by one is no further below the sample than that.)
        let near = self.pipe.r + self.step;
        let mut out: Vec<(f64, f64)> = (0..m)
            .filter(|&i| {
                (i == 0 || vals[i] <= vals[i - 1])
                    && (i + 1 == m || vals[i] <= vals[i + 1])
                    && vals[i] < near
            })
            .map(|i| {
                self.golden(
                    x,
                    (i as f64 - 1.0).max(0.0) * h,
                    ((i + 1) as f64 * h).min(self.n()),
                )
            })
            .collect();
        out.sort_by(|a, b| a.1.total_cmp(&b.1));
        out
    }

    /// Whether `x` is inside what the pipe fills (some part of the strip nearer than r), with
    /// where along the spine the strip is nearest it, as a part of the spine.
    fn cover(&self, x: &P3) -> (bool, f64) {
        let l = self
            .leasts(x)
            .first()
            .copied()
            .unwrap_or((0.0, f64::INFINITY));
        (l.1 < self.pipe.r - self.delta(), l.0 / self.n())
    }

    /// The local least of [`Self::dist`] that `tau` (a part of the spine) runs down to.
    fn least_from(&self, x: &P3, tau: f64) -> (f64, f64) {
        let h = 1.0 / SAMPLES as f64;
        let n = self.n();
        let mut m = (tau * n).clamp(0.0, n);
        let (mut a, mut b) = ((m - h).max(0.0), (m + h).min(n));
        let (mut fa, mut fm, mut fb) = (self.dist(a, x), self.dist(m, x), self.dist(b, x));
        for _ in 0..64 {
            if fa < fm && a > 0.0 {
                (b, fb, m, fm) = (m, fm, a, fa);
                a = (a - h).max(0.0);
                fa = self.dist(a, x);
            } else if fb < fm && b < n {
                (a, fa, m, fm) = (m, fm, b, fb);
                b = (b + h).min(n);
                fb = self.dist(b, x);
            } else {
                break;
            }
        }
        let _ = (fa, fb);
        self.golden(x, a, b)
    }

    /// [`Self::cover`] read only about `tau`.
    fn cover_near(&self, x: &P3, tau: f64) -> (bool, f64) {
        let l = self.least_from(x, tau);
        (l.1 < self.pipe.r - self.delta(), l.0 / self.n())
    }

    /// The frame of the half cylinder by an end of the spine (its axis along the slide, its
    /// angle from the wall of one side round the outside to the other's), and of the lunes
    /// there (their poles on the walls' normal, longitude from the pipe's own end).
    fn end_frames(&self, end: usize) -> (Frame, [(Frame, f64); 2]) {
        let s = if end == 0 { 0.0 } else { self.n() };
        let (p, tau, n, back) = self.frame(s);
        let out = if end == 0 { -tau } else { tau };
        // (Square to the slide and to the wall's normal, away from the spine.)
        let mut m = self.e.cross(&n);
        let mut x = n;
        if m.dot(&out) < 0.0 {
            (m, x) = (-m, -n);
        }
        let cyl = Frame {
            o: p,
            x,
            y: m,
            z: self.e,
        };
        let lune = |pose: f64| {
            let b = if pose < 0.5 { back } else { -back };
            let y = n.cross(&b);
            (
                Frame {
                    o: p + self.d * pose,
                    x: b,
                    y,
                    z: n,
                },
                m.dot(&y).atan2(m.dot(&b)),
            )
        };
        (cyl, [lune(0.0), lune(1.0)])
    }

    /// A candidate's point at a place of it, exactly.
    fn point(&self, c: &Cand, w: P2) -> P3 {
        match c.kind {
            Kind::Wall(side) => {
                let (p, _, n, _) = self.frame(w.x);
                p + n * (side * self.pipe.r) + self.d * w.y
            }
            Kind::Pipe(pose) => {
                let (p, _, n, back) = self.frame(w.x);
                let b = if pose < 0.5 { back } else { -back };
                p + self.d * pose + (n * w.y.cos() + b * w.y.sin()) * self.pipe.r
            }
            Kind::Cyl(end) => {
                let f = self.end_frames(end).0;
                f.o + (f.x * w.x.cos() + f.y * w.x.sin()) * self.pipe.r + f.z * w.y
            }
            Kind::Lune(end, pose) => {
                let f = self.end_frames(end).1[usize::from(pose > 0.5)].0;
                f.o + ((f.x * w.x.cos() + f.y * w.x.sin()) * w.y.cos() + f.z * w.y.sin())
                    * self.pipe.r
            }
        }
    }

    /// The candidate that holds `x`, a point at r from the strip's stretch at `tau`, and its
    /// place there: `prefer` where several meet.
    fn face_at(
        &self,
        cands: &[Cand],
        x: &P3,
        tau: f64,
        prefer: Option<usize>,
    ) -> Option<(usize, P2)> {
        let n = self.n();
        let s = (tau * n).clamp(0.0, n);
        let (p, t, nw, back) = self.frame(s);
        let w = x - p;
        let l = w.dot(&self.e);
        let r = self.pipe.r;
        let tiny = 1e-9 * r;
        // Past an end of the spine (from the strip's nearest point there, the spine's end
        // slid): on the cylinder there, or a lune of its ball.
        let past = (w - self.e * l.clamp(0.0, self.len)).dot(&t);
        let end = if s <= 1e-9 && past < -tiny {
            Some(0)
        } else if s >= n - 1e-9 && past > tiny {
            Some(1)
        } else {
            None
        };
        let pose = if l <= tiny {
            Some(0.0)
        } else if l >= self.len - tiny {
            Some(1.0)
        } else {
            None
        };
        let (kind, uv) = match (end, pose) {
            (Some(end), None) => {
                let f = self.end_frames(end).0;
                (Kind::Cyl(end), P2::new(w.dot(&f.y).atan2(w.dot(&f.x)), l))
            }
            (Some(end), Some(pose)) => {
                let f = self.end_frames(end).1[usize::from(pose > 0.5)].0;
                let v = x - f.o;
                (
                    Kind::Lune(end, pose),
                    P2::new(
                        v.dot(&f.y).atan2(v.dot(&f.x)),
                        (v.dot(&f.z) / r).clamp(-1.0, 1.0).asin(),
                    ),
                )
            }
            (None, None) => {
                let v = w - self.e * l;
                (
                    Kind::Wall(if v.dot(&nw) >= 0.0 { 1.0 } else { -1.0 }),
                    P2::new(s, l / self.len),
                )
            }
            (None, Some(pose)) => {
                let v = x - (p + self.d * pose);
                let b = if pose < 0.5 { back } else { -back };
                (Kind::Pipe(pose), P2::new(s, v.dot(&b).atan2(v.dot(&nw))))
            }
        };
        let mut first = None;
        for (i, c) in cands.iter().enumerate() {
            if c.kind != kind {
                continue;
            }
            let ((u0, u1), (v0, v1)) = c.bx;
            let slack = 1e-9;
            if uv.x >= u0 - slack && uv.x <= u1 + slack && uv.y >= v0 - slack && uv.y <= v1 + slack
            {
                let at = P2::new(uv.x.clamp(u0, u1), uv.y.clamp(v0, v1));
                if prefer == Some(i) {
                    return Some((i, at));
                }
                first.get_or_insert((i, at));
            }
        }
        first
    }
}

/// The surface through `at` over a box, within `tol`: a quintic B-spline through its points at
/// sites halved wherever it misses midway.
pub(super) fn fit_patch(
    at: &(dyn Fn(f64, f64) -> P3 + Sync),
    ((u0, u1), (v0, v1)): ((f64, f64), (f64, f64)),
    (nu, nv): (usize, usize),
    tol: f64,
) -> Option<super::nurbs::NurbsSurface> {
    let sites = |lo: f64, hi: f64, n: usize| -> Vec<f64> {
        (0..=n)
            .map(|i| lo + (hi - lo) * i as f64 / n as f64)
            .collect()
    };
    let (mut us, mut vs) = (sites(u0, u1, nu), sites(v0, v1, nv));
    for _ in 0..12 {
        let surf = tensor(at, &us, &vs);
        let bad = |u: f64, v: f64| (surf.point(u, v) - at(u, v)).norm() > tol;
        let mid = |x: &[f64], i: usize| 0.5 * (x[i] + x[i + 1]);
        let mut split_u: Vec<bool> = (0..us.len() - 1)
            .map(|i| vs.iter().any(|&v| bad(mid(&us, i), v)))
            .collect();
        let mut split_v: Vec<bool> = (0..vs.len() - 1)
            .map(|j| us.iter().any(|&u| bad(u, mid(&vs, j))))
            .collect();
        let cells: Vec<(usize, usize)> = (0..us.len() - 1)
            .flat_map(|i| (0..vs.len() - 1).map(move |j| (i, j)))
            .collect();
        for (i, j) in cells {
            if !(split_u[i] || split_v[j]) && bad(mid(&us, i), mid(&vs, j)) {
                split_u[i] = true;
                split_v[j] = true;
            }
        }
        if !split_u.iter().chain(&split_v).any(|s| *s) {
            return Some(surf);
        }
        if us.len() > 1200 || vs.len() > 400 {
            return None;
        }
        let halve = |x: &[f64], split: &[bool]| -> Vec<f64> {
            let mut out = Vec::with_capacity(2 * x.len());
            for i in 0..x.len() - 1 {
                out.push(x[i]);
                if split[i] {
                    out.push(0.5 * (x[i] + x[i + 1]));
                }
            }
            out.push(x[x.len() - 1]);
            out
        };
        us = halve(&us, &split_u);
        vs = halve(&vs, &split_v);
    }
    None
}

/// The curve through `at` from a to b by its own parameter, within `tol`: a quintic B-spline
/// through its points at sites halved wherever it misses midway.
fn fit_curve(
    at: &dyn Fn(f64) -> P3,
    (a, b): (f64, f64),
    n: usize,
    tol: f64,
) -> Option<super::nurbs::NurbsCurve> {
    let mut ts: Vec<f64> = (0..=n).map(|i| a + (b - a) * i as f64 / n as f64).collect();
    for _ in 0..16 {
        let pts: Vec<P3> = ts.iter().map(|&t| at(t)).collect();
        let c = super::nurbs::NurbsCurve::interpolate(&pts, &ts, 5.min(ts.len() - 1), None, None);
        let mut next = Vec::with_capacity(2 * ts.len());
        let mut split = false;
        for w in ts.windows(2) {
            next.push(w[0]);
            let m = 0.5 * (w[0] + w[1]);
            if (c.point(m) - at(m)).norm() > tol {
                next.push(m);
                split = true;
            }
        }
        next.push(b);
        if !split {
            return Some(c);
        }
        if next.len() > 4000 {
            return None;
        }
        ts = next;
    }
    None
}

/// [`crate::Kernel::sweep_solid`] of a pipe by one slide `d`: None where this doesn't take it
/// (an end without its ball, a spine that runs along the slide or bends within r of itself),
/// an error when its boundary isn't made out.
pub(crate) fn slide(k: &mut NativeKernel, pipe: &Pipe, d: V3) -> Option<Result<OpResult>> {
    use rayon::prelude::*;
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let t_start = web_time::Instant::now();
    let len = d.norm();
    if pipe.segs.is_empty() || !(pipe.balls[0] && pipe.balls[1]) || !(len > 1e-9 * pipe.r) {
        return None;
    }
    let refused = |why: String| -> Option<Result<OpResult>> {
        if debug {
            eprintln!("pipe slide: refused: {why}");
        }
        Some(Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: format!("the slid pipe: {why}"),
        }))
    };
    let e = d / len;
    let nseg = pipe.segs.len();
    let mut sl = Slid {
        pipe,
        d,
        len,
        e,
        size: 1.0,
        pts: Vec::new(),
        step: 0.0,
    };
    sl.pts = (0..=nseg * SAMPLES)
        .map(|i| sl.at(i as f64 / SAMPLES as f64).0)
        .collect();
    // (A little over the chord: the spine's own bow between two samples.)
    sl.step = 1.1
        * sl.pts
            .windows(2)
            .map(|w| (w[1] - w[0]).norm())
            .fold(0.0, f64::max);
    let mut bb = (sl.pts[0], sl.pts[0]);
    for p in &sl.pts {
        bb = (bb.0.inf(p), bb.1.sup(p));
    }
    sl.size = (bb.1 - bb.0).norm() + 2.0 * pipe.r;
    let sl = sl;
    let r = pipe.r;
    // The spine clear of the slide's own direction (its walls stand), turning no tighter than
    // r in its shadow (the walls fold nowhere), and joined smoothly.
    let mut turn: Vec<f64> = vec![0.0; nseg + 1];
    for i in 0..nseg * SAMPLES {
        let (s0, s1) = (i as f64 / SAMPLES as f64, (i + 1) as f64 / SAMPLES as f64);
        let (f0, f1) = (sl.frame(s0), sl.frame(s1 - 1e-9));
        if !(f0.2.norm() > 0.5) || sl.at(s0).1.normalize().cross(&e).norm() < 1e-3 {
            return refused("its spine runs along the slide".into());
        }
        let bend = f0.2.cross(&f1.2).norm().asin().abs();
        let run = {
            let w = f1.0 - f0.0;
            (w - e * w.dot(&e)).norm()
        };
        if bend * r > 0.9 * run {
            return refused("its spine bends within its radius, seen along the slide".into());
        }
        turn[i / SAMPLES + 1] += bend;
    }
    for i in 1..nseg {
        let (a, b) = (sl.frame(i as f64 - 1e-9), sl.frame(i as f64));
        if (a.1 - b.1).norm() > 1e-6 {
            return refused("its spine has a corner".into());
        }
    }
    // The spine in chunks its shadow turns three quarters of a turn in at most: none laps
    // over itself.
    let mut chunks: Vec<(f64, f64)> = Vec::new();
    let (mut from, mut sum) = (0usize, 0.0);
    for i in 0..nseg {
        if sum + turn[i + 1] > 0.75 * TAU && i > from {
            chunks.push((from as f64, i as f64));
            (from, sum) = (i, 0.0);
        }
        sum += turn[i + 1];
    }
    chunks.push((from as f64, nseg as f64));
    let ends: Vec<(Frame, [(Frame, f64); 2])> = (0..2).map(|end| sl.end_frames(end)).collect();
    if ends.iter().any(|f| {
        f.1.iter()
            .any(|l| l.1.abs() < 1e-3 || l.1.abs() > PI - 1e-3)
    }) {
        return refused("its spine ends along the slide".into());
    }
    let mut cands: Vec<Cand> = Vec::new();
    for &(s0, s1) in &chunks {
        let cells = ((s1 - s0) as usize * SAMPLES).max(8);
        for side in [1.0, -1.0] {
            cands.push(Cand {
                kind: Kind::Wall(side),
                bx: ((s0, s1), (0.0, 1.0)),
                grid: (cells, 128),
            });
        }
        for pose in [0.0, 1.0] {
            cands.push(Cand {
                kind: Kind::Pipe(pose),
                bx: ((s0, s1), (0.0, PI)),
                grid: (cells, 24),
            });
        }
    }
    for (end, frames) in ends.iter().enumerate() {
        cands.push(Cand {
            kind: Kind::Cyl(end),
            bx: ((0.0, PI), (0.0, len)),
            grid: (24, 128),
        });
        for pose in [0.0, 1.0] {
            let to = frames.1[usize::from(pose > 0.5)].1;
            cands.push(Cand {
                kind: Kind::Lune(end, pose),
                bx: ((to.min(0.0), to.max(0.0)), (-0.5 * PI, 0.5 * PI)),
                grid: (16, 24),
            });
        }
    }
    let cands = cands;
    let delta = sl.delta();
    let fit_tol = 1e-7 * sl.size;
    let jump = 0.5 / nseg as f64;
    super::par::start();
    // Each candidate traced in its own parameters: where its points pass from boundary to
    // inside, by the stretch of the strip that comes nearer than r beyond.
    let traced: Vec<Vec<super::sweep_moving::Given>> = cands
        .par_iter()
        .enumerate()
        .map(|(i, c)| {
            let point = |w: P2| sl.point(c, w);
            let cover = |w: P2| sl.cover(&point(w));
            let near = |w: P2, tc: f64| sl.cover_near(&point(w), tc);
            let fd = Field {
                cover: &cover,
                near: &near,
                margin: None,
            };
            let ((u0, u1), (v0, v1)) = c.bx;
            // (Its nodes read all at once: the walls' are most of the work.)
            let (mu, mv) = c.grid;
            let nodes: Vec<(bool, f64)> = (0..(mu + 1) * (mv + 1))
                .into_par_iter()
                .map(|k| {
                    let (a, b) = (k / (mv + 1), k % (mv + 1));
                    cover(P2::new(
                        u0 + (u1 - u0) * a as f64 / mu as f64,
                        v0 + (v1 - v0) * b as f64 / mv as f64,
                    ))
                })
                .collect();
            let t_nodes = web_time::Instant::now();
            let chains: Vec<Chain> = cover_curves(
                &fd,
                c.bx,
                c.grid,
                Some(&nodes),
                1e-4,
                jump,
                &point,
                fit_tol,
                &[],
            )
            .unwrap_or_default();
            let t_chains = web_time::Instant::now();
            let held =
                |f: usize, uv: P2, x: &P3| (sl.point(&cands[f], uv) - x).norm() <= 1e-6 * sl.size;
            let pair = |w: P2, tau: f64, prefer: Option<(usize, usize)>| {
                let x = point(w);
                let l = sl.least_from(&x, tau);
                if (l.1 - r).abs() > 1e-6 * sl.size {
                    return None;
                }
                let (f, uv) = sl.face_at(&cands, &x, l.0 / sl.n(), prefer.map(|p| p.1))?;
                held(f, uv, &x).then_some(((i, f), (x, w, uv)))
            };
            let place = |w: P2, tau: f64, (_, f): (usize, usize)| -> CurvePt {
                let x = point(w);
                // (The stretch of the strip, of those that touch the point, that puts it on
                // the face asked for.)
                let mut ls = sl.leasts(&x);
                ls.insert(0, sl.least_from(&x, tau));
                let uv = ls
                    .iter()
                    .filter(|l| (l.1 - r).abs() <= 1e-6 * sl.size)
                    .find_map(|l| {
                        sl.face_at(&cands, &x, l.0 / sl.n(), Some(f))
                            .filter(|h| h.0 == f)
                    })
                    .map_or_else(
                        || {
                            let ((a0, a1), (b0, b1)) = cands[f].bx;
                            P2::new(0.5 * (a0 + a1), 0.5 * (b0 + b1))
                        },
                        |h| h.1,
                    );
                (x, w, uv)
            };
            let within = |w: P2| {
                (u0 - 1e-9..=u1 + 1e-9).contains(&w.x) && (v0 - 1e-9..=v1 + 1e-9).contains(&w.y)
            };
            let margin = |w: P2, tau: f64| {
                if !within(w) {
                    return (f64::NAN, tau);
                }
                let l = sl.least_from(&point(w), tau);
                ((l.1 - r + delta) / delta, l.0 / sl.n())
            };
            let onto = |w: P2, dir: V2, tau: f64, full: bool| {
                if full {
                    onto_boundary(&|w, _| cover(w), w, dir, tau, c.bx)
                } else {
                    onto_boundary(&near, w, dir, tau, c.bx)
                }
            };
            let mut out = Vec::new();
            for chain in chains {
                chain_runs(&chain, &pair, &place, &onto, &margin, jump, &[], &mut out);
            }
            if debug {
                eprintln!(
                    "  {:?}: nodes by {:.0} ms, chains {:.0} ms, stretches {:.0} ms",
                    c.kind,
                    (t_nodes - t_start).as_secs_f64() * 1e3,
                    (t_chains - t_nodes).as_secs_f64() * 1e3,
                    t_chains.elapsed().as_secs_f64() * 1e3
                );
            }
            out
        })
        .collect();
    let mut given: Vec<super::sweep_moving::Given> = traced.into_iter().flatten().collect();
    drop_twins(&mut given, &|_| true, sl.size, fit_tol);
    // Two walls meet along a line (both run along the slide): its two ends are all of it.
    for g in &mut given {
        let (n, straight) = (g.pts.len(), 1e-10 * sl.size);
        if n < 3
            || !matches!(
                (cands[g.a].kind, cands[g.b].kind),
                (Kind::Wall(_), Kind::Wall(_))
            )
        {
            continue;
        }
        let (a, b) = (g.pts[0], g.pts[n - 1]);
        let along = (b.0 - a.0).normalize();
        let on_line = g.pts.iter().all(|p| {
            let w = p.0 - a.0;
            (w - along * w.dot(&along)).norm() <= straight
                && (p.1.x - a.1.x).abs() <= 1e-9
                && (p.2.x - a.2.x).abs() <= 1e-9
        });
        if on_line && (b.1.x - a.1.x).abs() <= 1e-9 && (b.2.x - a.2.x).abs() <= 1e-9 {
            g.pts = vec![a, b];
        }
    }
    if debug {
        eprintln!(
            "pipe slide: {} candidates, {} curves by {:.0} ms",
            cands.len(),
            given.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
        if std::env::var_os("CADRS_TUBE_CURVES").is_some() {
            for g in &given {
                let (p, q) = (g.pts[0], g.pts[g.pts.len() - 1]);
                eprintln!(
                    "  curve of faces ({}, {}) [{:?}, {:?}]: {} points, {:?} -> {:?}; on the first ({:.5}, {:.5}) -> ({:.5}, {:.5}), on the second ({:.5}, {:.5}) -> ({:.5}, {:.5})",
                    g.a,
                    g.b,
                    cands[g.a].kind,
                    cands[g.b].kind,
                    g.pts.len(),
                    p.0,
                    q.0,
                    p.1.x,
                    p.1.y,
                    q.1.x,
                    q.1.y,
                    p.2.x,
                    p.2.y,
                    q.2.x,
                    q.2.y
                );
                if std::env::var("CADRS_TUBE_CURVES").is_ok_and(|v| v == format!("{},{}", g.a, g.b))
                {
                    for p in &g.pts {
                        eprintln!(
                            "    ({:.9}, {:.9}) | ({:.9}, {:.9}) at {:?}",
                            p.1.x, p.1.y, p.2.x, p.2.y, p.0
                        );
                    }
                }
            }
        }
    }
    // The faces: the walls each the extrusion of its silhouette's curve, the pipe's halves
    // fitted, the cylinders and the balls as they are; each turned to face away from the strip.
    let built: Vec<std::result::Result<Body, String>> = cands
        .par_iter()
        .map(|c| {
            let surf = match c.kind {
                Kind::Wall(_) => {
                    let at = |s: f64| sl.point(c, P2::new(s, 0.0));
                    let n = (c.bx.0.1 - c.bx.0.0) as usize * 4;
                    let curve = fit_curve(&at, c.bx.0, n.max(8), 0.5 * fit_tol)
                        .ok_or("a silhouette won't fit")?;
                    Surface::Extrusion {
                        c: Curve::nurbs(curve),
                        d,
                    }
                }
                Kind::Pipe(_) => {
                    let at = |u: f64, v: f64| sl.point(c, P2::new(u, v));
                    let n = (c.bx.0.1 - c.bx.0.0) as usize * 4;
                    Surface::Nurbs(std::sync::Arc::new(
                        fit_patch(&at, c.bx, (n.max(8), 8), fit_tol)
                            .ok_or("a pipe's half won't fit")?,
                    ))
                }
                Kind::Cyl(end) => Surface::Cylinder { f: ends[end].0, r },
                Kind::Lune(end, pose) => Surface::Sphere {
                    f: ends[end].1[usize::from(pose > 0.5)].0,
                    r,
                },
            };
            let mut b = super::face_build::face_from_surface(&surf, Some(c.bx))
                .map_err(|e| e.to_string())?;
            let ((u0, u1), (v0, v1)) = c.bx;
            let w = P2::new(0.5 * (u0 + u1), 0.5 * (v0 + v1));
            let x = sl.point(c, w);
            let foot = sl.least_from(&x, {
                // (Its own stretch of the strip: the spine's point under a wall or a pipe, the
                // spine's end under the rest.)
                match c.kind {
                    Kind::Wall(_) | Kind::Pipe(_) => w.x / sl.n(),
                    Kind::Cyl(end) | Kind::Lune(end, _) => end as f64,
                }
            });
            let under = {
                let p = sl.at(foot.0).0;
                let v = x - p;
                p + sl.e * v.dot(&sl.e).clamp(0.0, len)
            };
            let face = &mut b.faces[0];
            let (_, su, sv) = face.surf.d1(w.x, w.y);
            let natural = su.cross(&sv) * if face.sense { 1.0 } else { -1.0 };
            if natural.dot(&(x - under)) < 0.0 {
                Body::reverse_face(face);
            }
            Ok(b)
        })
        .collect();
    let mut faces = Vec::with_capacity(cands.len());
    for b in built {
        match b {
            Ok(b) => faces.push(b),
            Err(why) => return refused(why),
        }
    }
    if debug {
        eprintln!(
            "pipe slide: faces made by {:.0} ms",
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let member = |i: usize, uv: P2| sl.cover(&sl.point(&cands[i], uv)).0;
    let still = |_: f64| (nalgebra::Rotation3::identity(), V3::zeros());
    let oracle = super::sweep_moving::Oracle::with_measure(Box::new(|_: &V3| 1.0), &still, 2);
    let solid = match super::sweep_moving::boundary_of_some(
        faces,
        &oracle,
        sl.size,
        0.0,
        f64::INFINITY,
        given,
        Some(&member),
        fit_tol,
        true,
        None,
    ) {
        Ok(s) => s,
        Err(e) => {
            if debug {
                eprintln!("pipe slide: refused: {e}");
            }
            return Some(Err(e));
        }
    };
    if debug {
        eprintln!(
            "pipe slide: {} faces by {:.0} ms",
            solid.faces.len(),
            t_start.elapsed().as_secs_f64() * 1e3
        );
    }
    let id = k.insert(solid);
    Some(Ok(OpResult {
        bodies: vec![id],
        history: History::default(),
    }))
}

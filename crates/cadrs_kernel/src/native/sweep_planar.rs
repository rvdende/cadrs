//! The solid a prism fills moving in its planes ([`crate::SolidMotion::Planar`]): its polygon's
//! swept region, extruded.
//!
//! A polygon moving rigidly in its plane sweeps a region bounded by pieces of three kinds of
//! curve: the path of each corner; the envelope of each side, at each instant the foot of the
//! perpendicular from the motion's instantaneous centre of rotation onto the side's line (where
//! the side moves along itself), while it falls within the side; and, for a motion that doesn't
//! come back to where it started, the polygon's sides where it starts and ends. Each is sampled
//! densely by its own parameter, and its samples lying inside none of the polygon's places
//! (deeper than rounding, over all instants) are kept: their runs, cut where they enter one
//! (found by bisection), are the region's boundary pieces. The pieces chained end to end make
//! its loops, each piece a cubic B-spline through its exact points (a side a line), and the
//! loops (outer ones with the holes inside them) are the region extruded through the prism.

use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use crate::*;
use nalgebra::{Point2, Vector2};
use std::f64::consts::PI;

type Q = Point2<f64>;
type W = Vector2<f64>;

/// A planar motion: at each instant t in [0, 1] the rotation angle and the translation that
/// take the polygon's points from where they start (x ↦ R(θ) x + d).
struct Motion2 {
    steps: Vec<Step2>,
}

enum Step2 {
    Turn { c: W, a0: f64, a1: f64 },
    Slide { v0: W, v1: W },
}

impl Motion2 {
    fn at(&self, t: f64) -> (f64, W) {
        let (mut th, mut d) = (0.0, W::zeros());
        for s in &self.steps {
            match s {
                Step2::Turn { c, a0, a1 } => {
                    let a = a0 + (a1 - a0) * t;
                    let r = nalgebra::Rotation2::new(a);
                    // x ↦ R(a)(x − c) + c after the steps before.
                    d = r * (d - c) + c;
                    th += a;
                }
                Step2::Slide { v0, v1 } => d += v0 + (v1 - v0) * t,
            }
        }
        (th, d)
    }

    fn apply(&self, t: f64, x: &Q) -> Q {
        let (th, d) = self.at(t);
        Q::from(nalgebra::Rotation2::new(th) * x.coords + d)
    }

    fn unapply(&self, t: f64, x: &Q) -> Q {
        let (th, d) = self.at(t);
        Q::from(nalgebra::Rotation2::new(-th) * (x.coords - d))
    }

    /// The instantaneous centre of rotation at t (None for a pure slide).
    fn centre(&self, t: f64) -> Option<Q> {
        let h = 1e-6;
        let ((ta, da), (tb, db)) = (self.at(t - h), self.at(t + h));
        let (_, d) = self.at(t);
        let dth = (tb - ta) / (2.0 * h);
        let dd = (db - da) / (2.0 * h);
        if dth.abs() < 1e-12 {
            return None;
        }
        // v(X) = θ' J (X − d) + d' = θ' J (X − I): I = d + J d' / θ'.
        let jd = W::new(-dd.y, dd.x);
        Some(Q::from(d + jd / dth))
    }
}

/// The signed distance of q from the polygon's boundary (positive inside).
fn signed_distance(poly: &[Q], q: &Q) -> f64 {
    let n = poly.len();
    let mut d = f64::INFINITY;
    let mut wind = 0i32;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let ab = b - a;
        let s = ((q - a).dot(&ab) / ab.norm_squared()).clamp(0.0, 1.0);
        d = d.min((a + ab * s - q).norm());
        let side = ab.perp(&(q - a));
        if a.y <= q.y {
            if b.y > q.y && side > 0.0 {
                wind += 1;
            }
        } else if b.y <= q.y && side < 0.0 {
            wind -= 1;
        }
    }
    if wind != 0 { d } else { -d }
}

/// A candidate curve of the region's boundary: its point at s in [0, 1] (None where it isn't
/// one: an envelope's foot off its side).
type Candidate<'a> = Box<dyn Fn(f64) -> Option<Q> + Sync + 'a>;

/// A piece of the region's boundary: its points, the candidate it lies on and that candidate's
/// parameter at its first and last points, and whether it is a straight side.
struct Piece {
    pts: Vec<Q>,
    cand: usize,
    s: (f64, f64),
    straight: bool,
}

/// A run of a candidate's samples outside the region, with its parameters at its ends.
type Run = (Vec<Q>, (f64, f64));

/// The boundary pieces along one candidate: runs of its samples inside no place of the polygon,
/// their ends cut by bisection where it enters one or stops being a candidate.
fn pieces_of(
    c: &Candidate,
    inside: &(dyn Fn(&Q) -> bool + Sync),
    n: usize,
    closed: bool,
    shortest: f64,
) -> Vec<Run> {
    let keep = |s: f64| c(s).filter(|q| !inside(q));
    let mut flags: Vec<Option<Q>> = (0..=n).map(|i| keep(i as f64 / n as f64)).collect();
    if closed {
        flags[n] = flags[0];
    }
    // The boundary between a kept parameter and one not, by bisection.
    let cut = |a: f64, b: f64, kept_at_a: bool| -> Option<(Q, f64)> {
        let (mut lo, mut hi) = (a, b);
        for _ in 0..50 {
            let m = 0.5 * (lo + hi);
            if keep(m).is_some() == kept_at_a {
                lo = m;
            } else {
                hi = m;
            }
        }
        let s = if kept_at_a { lo } else { hi };
        keep(s).map(|q| (q, s))
    };
    let mut runs: Vec<(Vec<Q>, (f64, f64))> = Vec::new();
    let mut cur: Vec<Q> = Vec::new();
    let mut s0 = 0.0;
    for (i, flag) in flags.iter().enumerate() {
        let si = i as f64 / n as f64;
        match *flag {
            Some(q) => {
                if cur.is_empty() {
                    s0 = si;
                    if i > 0
                        && let Some((e, se)) = cut((i - 1) as f64 / n as f64, si, false)
                    {
                        cur.push(e);
                        s0 = se;
                    }
                }
                cur.push(q);
            }
            None => {
                if !cur.is_empty() {
                    let mut s1 = (i - 1) as f64 / n as f64;
                    if let Some((e, se)) = cut(s1, si, true) {
                        cur.push(e);
                        s1 = se;
                    }
                    runs.push((std::mem::take(&mut cur), (s0, s1)));
                }
            }
        }
    }
    if !cur.is_empty() {
        runs.push((cur, (s0, 1.0)));
    }
    // A closed candidate's run across its start and end is one.
    if closed && runs.len() > 1 && flags[0].is_some() && flags[n].is_some() {
        let (first, (_, f1)) = runs.remove(0);
        let last = runs.last_mut().unwrap();
        last.0.pop();
        last.0.extend(first);
        last.1.1 = f1;
    } else if closed && runs.len() == 1 && flags.iter().all(Option::is_some) {
        runs[0].0.pop();
        let first = runs[0].0[0];
        runs[0].0.push(first);
    }
    // (Runs of no length: a candidate touching the boundary at a point.)
    runs.into_iter()
        .filter(|(r, _)| {
            r.len() >= 2 && r.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<f64>() > shortest
        })
        .collect()
}

/// Where two candidates cross near the parameters `a` and `b`: Newton's method on both.
fn crossing(ca: &Candidate, a: f64, cb: &Candidate, b: f64) -> Option<(Q, f64, f64)> {
    let (mut a, mut b) = (a, b);
    let h = 1e-7;
    for _ in 0..40 {
        let (pa, pb) = (ca(a)?, cb(b)?);
        let f = pa - pb;
        if f.norm() < 1e-13 * (1.0 + pa.coords.norm()) {
            return Some((pa, a, b));
        }
        let da = (ca(a + h)? - ca(a - h)?) / (2.0 * h);
        let db = (cb(b + h)? - cb(b - h)?) / (2.0 * h);
        // f + da Δa − db Δb = 0.
        let det = da.x * (-db.y) - da.y * (-db.x);
        if det.abs() < 1e-300 {
            return None;
        }
        let x = (-f.x * (-db.y) + f.y * (-db.x)) / det;
        let y = (da.x * -f.y - da.y * -f.x) / det;
        a += x;
        b += y;
    }
    let (pa, pb) = (ca(a)?, cb(b)?);
    ((pa - pb).norm() < 1e-9 * (1.0 + pa.coords.norm())).then_some((pa, a, b))
}

/// [`crate::Kernel::sweep_solid`] for a planar motion.
pub fn sweep_planar(
    k: &mut NativeKernel,
    b: &Body,
    n: V3,
    steps: &[PlanarStep],
) -> Result<OpResult> {
    let unsupported =
        || KernelError::Unsupported("sweeping solids other than prisms in their planes");
    let n = n.normalize();
    let ex = any_perpendicular(&n);
    let ey = n.cross(&ex);
    // The prism: two caps square to n, the other faces planes along it.
    let mut caps: Vec<(usize, f64)> = Vec::new();
    for (fi, f) in b.faces.iter().enumerate() {
        let Surface::Plane { f: fr } = &f.surf else {
            return Err(unsupported());
        };
        let m = fr.x.cross(&fr.y).normalize();
        if m.cross(&n).norm() < tol::ANGULAR {
            caps.push((fi, fr.o.coords.dot(&n)));
        } else if m.dot(&n).abs() > tol::ANGULAR {
            return Err(unsupported());
        }
    }
    let [(c0, h0), (_, h1)] = caps[..] else {
        return Err(unsupported());
    };
    let (h0, h1) = (h0.min(h1), h0.max(h1));
    // The polygon: the cap's loop's corners, counter-clockwise about n.
    let f = &b.faces[c0];
    let [lp] = f.loops.as_slice() else {
        return Err(unsupported());
    };
    let to2 = |p: &P3| Q::new(p.coords.dot(&ex), p.coords.dot(&ey));
    let mut poly: Vec<Q> = Vec::new();
    for c in lp {
        let e = &b.edges[c.edge];
        if !matches!(e.curve, super::curve::Curve::Line { .. }) {
            return Err(unsupported());
        }
        let v = if c.fwd { e.v[0] } else { e.v[1] };
        poly.push(to2(&b.verts[v].p));
    }
    let area2: f64 = (0..poly.len())
        .map(|i| poly[i].coords.perp(&poly[(i + 1) % poly.len()].coords))
        .sum();
    if area2 < 0.0 {
        poly.reverse();
    }
    // The motion in the plane's coordinates.
    let m = Motion2 {
        steps: steps
            .iter()
            .map(|s| match *s {
                PlanarStep::Turn { about, from, to } => Step2::Turn {
                    c: to2(&about).coords,
                    a0: from,
                    a1: to,
                },
                PlanarStep::Slide { from, to } => Step2::Slide {
                    v0: W::new(from.dot(&ex), from.dot(&ey)),
                    v1: W::new(to.dot(&ex), to.dot(&ey)),
                },
            })
            .collect(),
    };
    let (t0, t1) = (m.at(0.0), m.at(1.0));
    let size = poly
        .iter()
        .map(|q| q.coords.norm())
        .fold(0.0, f64::max)
        .max(1.0);
    // Back where it started: turned whole turns, not moved.
    let turns = (t1.0 - t0.0) / (2.0 * PI);
    let closed = (turns - turns.round()).abs() < 1e-9 && (t1.1 - t0.1).norm() < 1e-9 * size;
    let deep = 1e-9 * (size + t0.1.norm() + t1.1.norm());
    // Inside some place of the polygon: the first instant found deeper than rounding settles it;
    // instants where the point is off the polygon's bounding circle are passed over at once; the
    // instants nearest to holding it (within a step's travel) refined by golden-section search.
    let centre = Q::from(poly.iter().map(|q| q.coords).sum::<W>() / poly.len() as f64);
    let radius = poly.iter().map(|q| (q - centre).norm()).fold(0.0, f64::max);
    // The instants sampled (the turn's cosine and sine, the shift), and how far a point's place
    // on the polygon can move between two: the turn between them times its distance from the
    // shifts, plus a shift's change.
    let nt = 4096;
    let inst: Vec<(f64, f64, f64, W)> = (0..=nt)
        .map(|i| {
            let (th, d) = m.at(i as f64 / nt as f64);
            let (s, c) = th.sin_cos();
            (th, c, s, d)
        })
        .collect();
    let (mut turn, mut shift, mut reach) = (0.0f64, 0.0f64, 0.0f64);
    for w in inst.windows(2) {
        turn = turn.max((w[1].0 - w[0].0).abs());
        shift = shift.max((w[1].3 - w[0].3).norm());
        reach = reach.max(w[0].3.norm()).max(w[1].3.norm());
    }
    let inside = |q: &Q| -> bool {
        // (A step's travel at most: an instant farther out than three of them is followed by
        // as many more as can't reach two of them, passed over.)
        let speed = 1.01 * (turn * (q.coords.norm() + reach) + shift) + f64::MIN_POSITIVE;
        let mut peaks: Vec<(f64, usize)> = Vec::new();
        let mut prev: Option<(f64, usize)> = None;
        let mut prev2: Option<(f64, usize)> = None;
        let mut i = 0;
        loop {
            let (_, c, s, d) = inst[i];
            let w = q.coords - d;
            let y = Q::new(c * w.x + s * w.y, -s * w.x + c * w.y);
            let off = (y - centre).norm() - radius;
            let v = if off > 0.0 {
                -off
            } else {
                signed_distance(&poly, &y)
            };
            if v > deep {
                return true;
            }
            // A peak among three instants running.
            if let Some((pv, pi)) = prev
                && pi + 1 == i
                && prev2.is_none_or(|(p2, i2)| i2 + 1 != pi || pv >= p2)
                && pv >= v
            {
                peaks.push((pv, pi));
            }
            prev2 = prev;
            prev = Some((v, i));
            if i == nt {
                break;
            }
            let skip = if v < -3.0 * speed {
                ((-v - 2.0 * speed) / speed).floor() as usize
            } else {
                1
            };
            i = (i + skip.max(1)).min(nt);
        }
        if let Some((pv, pi)) = prev
            && !peaks.iter().any(|p| p.1 == pi)
        {
            peaks.push((pv, pi));
        }
        peaks.retain(|p| p.0 > -2.0 * speed);
        if peaks.is_empty() {
            return false;
        }
        peaks.sort_by(|a, b| b.0.total_cmp(&a.0));
        let f = |t: f64| signed_distance(&poly, &m.unapply(t, q));
        let h = 1.0 / nt as f64;
        let g = (5f64.sqrt() - 1.0) / 2.0;
        for &(_, i) in peaks.iter().take(3) {
            let c = i as f64 * h;
            let (mut lo, mut hi) = ((c - h).max(0.0), (c + h).min(1.0));
            let (mut x1, mut x2) = (hi - g * (hi - lo), lo + g * (hi - lo));
            let (mut f1, mut f2) = (f(x1), f(x2));
            for _ in 0..50 {
                if f1 < f2 {
                    lo = x1;
                    x1 = x2;
                    f1 = f2;
                    x2 = lo + g * (hi - lo);
                    f2 = f(x2);
                } else {
                    hi = x2;
                    x2 = x1;
                    f2 = f1;
                    x1 = hi - g * (hi - lo);
                    f1 = f(x1);
                }
            }
            if f1.max(f2) > deep {
                return true;
            }
        }
        false
    };
    // The candidates.
    let np = poly.len();
    let mut cands: Vec<(Candidate, bool)> = Vec::new();
    for &v in poly.iter() {
        let mm = &m;
        cands.push((Box::new(move |t: f64| Some(mm.apply(t, &v))), closed));
    }
    for j in 0..np {
        let (a, bb) = (poly[j], poly[(j + 1) % np]);
        let mm = &m;
        cands.push((
            Box::new(move |t: f64| {
                let (pa, pb) = (mm.apply(t, &a), mm.apply(t, &bb));
                let ic = mm.centre(t)?;
                let ab = pb - pa;
                let s = (ic - pa).dot(&ab) / ab.norm_squared();
                (s > 0.0 && s < 1.0).then(|| pa + ab * s)
            }),
            closed,
        ));
    }
    if !closed {
        for t in [0.0, 1.0] {
            for j in 0..np {
                let (a, bb) = (m.apply(t, &poly[j]), m.apply(t, &poly[(j + 1) % np]));
                cands.push((Box::new(move |s: f64| Some(a + (bb - a) * s)), false));
            }
        }
    }
    let samples = 2048;
    let tol_join = 1e-6 * (size + t0.1.norm() + t1.1.norm());
    let len = |q: &[Q]| q.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<f64>();
    let mut pieces: Vec<Piece> = Vec::new();
    // Each candidate's runs outside the region (the inside tests, most of the work), the
    // candidates side by side, in their order.
    let runs: Vec<Vec<Run>> = {
        use rayon::prelude::*;
        super::par::start();
        cands
            .par_iter()
            .enumerate()
            .map(|(i, (c, cl))| {
                let straight = i >= 2 * np;
                pieces_of(
                    c,
                    &inside,
                    if straight { 64 } else { samples },
                    *cl,
                    tol_join,
                )
            })
            .collect()
    };
    for (i, runs) in runs.into_iter().enumerate() {
        let straight = i >= 2 * np;
        for (pts, s) in runs {
            // Split where it turns back (an envelope's cusp, its tail past which lies inside the
            // region too thinly to tell): the stretches either side, the short ones dropped.
            let n = pts.len();
            let mut cuts = vec![0];
            for j in 1..n.saturating_sub(1) {
                let (u, v) = (pts[j] - pts[j - 1], pts[j + 1] - pts[j]);
                if u.dot(&v) < 0.0 {
                    cuts.push(j);
                }
            }
            cuts.push(n - 1);
            if cuts.len() == 2 {
                pieces.push(Piece {
                    pts,
                    cand: i,
                    s,
                    straight,
                });
                continue;
            }
            let ds = (s.1 - s.0) / (n - 1).max(1) as f64;
            for w in cuts.windows(2) {
                let part: Vec<Q> = pts[w[0]..=w[1]].to_vec();
                if len(&part) > 1e-3 * size {
                    let s = (s.0 + ds * w[0] as f64, s.0 + ds * w[1] as f64);
                    pieces.push(Piece {
                        pts: part,
                        cand: i,
                        s,
                        straight,
                    });
                }
            }
        }
    }
    // Pieces two candidates trace alike (two corners on one circle about a turn's centre): once.
    let mut distinct: Vec<Piece> = Vec::new();
    for p in pieces {
        let l = len(&p.pts);
        // (Its first, middle and last points on the other's polyline, as long as it.)
        let on = |o: &[Q], q: &Q| {
            o.windows(2).any(|w| {
                let ab = w[1] - w[0];
                let t = ((q - w[0]).dot(&ab) / ab.norm_squared().max(1e-300)).clamp(0.0, 1.0);
                (w[0] + ab * t - q).norm() < tol_join * 10.0
            })
        };
        let probes = [p.pts[0], p.pts[p.pts.len() / 2], *p.pts.last().unwrap()];
        let same = distinct.iter().any(|o| {
            (len(&o.pts) - l).abs() < tol_join * 1e3 && probes.iter().all(|q| on(&o.pts, q))
        });
        if !same {
            distinct.push(p);
        }
    }
    let mut pieces = distinct;
    if std::env::var_os("CADRS_SWEEP_SOLID_DEBUG").is_some() {
        for (i, p) in pieces.iter().enumerate() {
            eprintln!(
                "piece {i} cand {} {} pts len {:.4}: ({:.4}, {:.4}) -> ({:.4}, {:.4})",
                p.cand,
                p.pts.len(),
                len(&p.pts),
                p.pts[0].x,
                p.pts[0].y,
                p.pts.last().unwrap().x,
                p.pts.last().unwrap().y
            );
        }
    }
    // Chained end to end, each piece either way round; where a chain's end stops short of the
    // next piece's start (a crossing of two corners' paths, where a point lies inside the other's
    // place only for a moment), both are carried on to where their curves cross.
    let mut used = vec![false; pieces.len()];
    let mut loops: Vec<Vec<(usize, bool)>> = Vec::new();
    let end_of = |p: &Piece, rev: bool| -> (Q, f64) {
        if rev {
            (p.pts[0], p.s.0)
        } else {
            (*p.pts.last().unwrap(), p.s.1)
        }
    };
    let start_of = |p: &Piece, rev: bool| -> (Q, f64) {
        if rev {
            (*p.pts.last().unwrap(), p.s.1)
        } else {
            (p.pts[0], p.s.0)
        }
    };
    let gap_most = 0.05 * size;
    for s0 in 0..pieces.len() {
        if used[s0] {
            continue;
        }
        used[s0] = true;
        let mut lp = vec![(s0, false)];
        let start = start_of(&pieces[s0], false).0;
        let mut guard = 0;
        loop {
            let (cur, crev) = *lp.last().unwrap();
            let (end, se) = end_of(&pieces[cur], crev);
            if lp.len() > 1 && (end - start).norm() <= tol_join * 100.0 {
                break;
            }
            guard += 1;
            if guard > pieces.len() + 1 {
                break;
            }
            // The nearest start (either way round) among the unused pieces, and the loop's own.
            let mut best: Option<(usize, bool, f64)> = None;
            for j in 0..pieces.len() {
                if used[j] {
                    continue;
                }
                for rev in [false, true] {
                    let d = (start_of(&pieces[j], rev).0 - end).norm();
                    if best.is_none_or(|b| d < b.2) {
                        best = Some((j, rev, d));
                    }
                }
            }
            let back = (start - end).norm();
            if best.is_none_or(|b| back < b.2) && back <= gap_most {
                // Closing: carried on to the first piece's start if they stop short.
                if back > tol_join * 100.0 {
                    let (s_first, _) = (start_of(&pieces[s0], false).1, ());
                    let (cj, c0) = (pieces[cur].cand, pieces[s0].cand);
                    if let Some((q, a, b)) = crossing(&cands[cj].0, se, &cands[c0].0, s_first) {
                        let _ = (a, b);
                        if crev {
                            pieces[cur].pts.insert(0, q)
                        } else {
                            pieces[cur].pts.push(q)
                        }
                        pieces[s0].pts.insert(0, q);
                    }
                }
                break;
            }
            let Some((j, rev, d)) = best else { break };
            if d > gap_most {
                break;
            }
            if d > tol_join * 100.0 {
                let (sj, _) = (start_of(&pieces[j], rev).1, ());
                let (ci, cjj) = (pieces[cur].cand, pieces[j].cand);
                match crossing(&cands[ci].0, se, &cands[cjj].0, sj) {
                    Some((q, _, _)) => {
                        if crev {
                            pieces[cur].pts.insert(0, q)
                        } else {
                            pieces[cur].pts.push(q)
                        }
                        if rev {
                            pieces[j].pts.push(q)
                        } else {
                            pieces[j].pts.insert(0, q)
                        }
                    }
                    None => break,
                }
            }
            used[j] = true;
            lp.push((j, rev));
        }
        let (cur, crev) = *lp.last().unwrap();
        let end = end_of(&pieces[cur], crev).0;
        // (A loop as short as the gaps chained over: a piece closing on itself, no region.)
        let perimeter: f64 = lp.iter().map(|&(j, _)| len(&pieces[j].pts)).sum();
        if (end - start_of(&pieces[s0], false).0).norm() <= tol_join * 100.0
            && perimeter > tol_join * 1e4
        {
            loops.push(lp);
        } else if std::env::var_os("CADRS_SWEEP_SOLID_DEBUG").is_some() {
            eprintln!(
                "planar sweep: an open chain of {} pieces, gap {:e}",
                lp.len(),
                (end - start).norm()
            );
        }
    }
    let ends = |j: usize, rev: bool| {
        let p = &pieces[j].pts;
        if rev {
            (*p.last().unwrap(), p[0])
        } else {
            (p[0], *p.last().unwrap())
        }
    };
    if std::env::var_os("CADRS_SWEEP_SOLID_DEBUG").is_some() {
        for lp in &loops {
            let desc: Vec<String> = lp
                .iter()
                .map(|&(j, _)| {
                    let p = &pieces[j].pts;
                    format!(
                        "{}{}:{:.3e}",
                        if pieces[j].straight { "L" } else { "C" },
                        p.len(),
                        len(p)
                    )
                })
                .collect();
            eprintln!("planar sweep loop: {desc:?}");
        }
    }
    if loops.is_empty() {
        return Err(KernelError::InvalidResult {
            op: "solid sweep",
            why: "the swept region doesn't close".into(),
        });
    }
    // Each loop's curves (its pieces' ends snapped to the next's starts), its signed area.
    let fit = |pts: &[Q], straight: bool| -> Curve2 {
        if straight || pts.len() == 2 {
            return Curve2::Line {
                a: pts[0],
                b: *pts.last().unwrap(),
                source: None,
            };
        }
        // (Points closer than a millionth of the size to the one before dropped, the last kept:
        // a step of almost nothing slows the spline to a near stop, where a point's parameter
        // on it is ill-defined.)
        let close = 1e-6 * size;
        let mut ps: Vec<P3> = Vec::with_capacity(pts.len());
        for (i, q) in pts.iter().enumerate() {
            let p = P3::new(q.x, q.y, 0.0);
            match ps.last() {
                Some(l) if (p - l).norm() < close => {
                    if i + 1 == pts.len() && ps.len() > 1 {
                        *ps.last_mut().unwrap() = p;
                    }
                }
                _ => ps.push(p),
            }
        }
        if ps.len() == 2 {
            return Curve2::Line {
                a: pts[0],
                b: *pts.last().unwrap(),
                source: None,
            };
        }
        let mut params = vec![0.0];
        for w in ps.windows(2) {
            params.push(params.last().unwrap() + (w[1] - w[0]).norm());
        }
        let c =
            super::nurbs::NurbsCurve::interpolate(&ps, &params, 3.min(ps.len() - 1), None, None);
        Curve2::Spline {
            degree: c.degree,
            knots: c.knots.clone(),
            poles: c.cps.iter().map(|h| Q::new(h.x / h.w, h.y / h.w)).collect(),
            weights: vec![],
            source: None,
        }
    };
    let mut built: Vec<(Loop, f64, Vec<Q>)> = Vec::new();
    for lp in &loops {
        let mut curves = Vec::new();
        let mut poly_pts: Vec<Q> = Vec::new();
        for (k2, &(pi, rev)) in lp.iter().enumerate() {
            let mut pts = pieces[pi].pts.clone();
            if rev {
                pts.reverse();
            }
            let (nj, nrev) = lp[(k2 + 1) % lp.len()];
            *pts.last_mut().unwrap() = ends(nj, nrev).0;
            poly_pts.extend(&pts[..pts.len() - 1]);
            curves.push(fit(&pts, pieces[pi].straight));
        }
        let a: f64 = (0..poly_pts.len())
            .map(|i| {
                poly_pts[i]
                    .coords
                    .perp(&poly_pts[(i + 1) % poly_pts.len()].coords)
            })
            .sum::<f64>()
            * 0.5;
        built.push((Loop { curves }, a, poly_pts));
    }
    // Outer loops (the larger by area first) and the holes inside them.
    built.sort_by(|a, b| b.1.abs().total_cmp(&a.1.abs()));
    let inside_poly = |pts: &[Q], q: &Q| signed_distance(pts, q) > 0.0;
    let mut regions: Vec<(Region, Vec<Q>)> = Vec::new();
    for (lp, _, pts) in built {
        let probe = pts[0];
        match regions
            .iter_mut()
            .find(|(_, outer)| inside_poly(outer, &probe))
        {
            Some((r, _)) => r.holes.push(lp),
            None => regions.push((
                Region {
                    outer: lp,
                    holes: vec![],
                    source: Some(0),
                },
                pts,
            )),
        }
    }
    let plane = Plane {
        origin: P3::from(n * h0),
        x_dir: nalgebra::Unit::new_normalize(ex),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let prof = Profile::new(plane, regions.into_iter().map(|(r, _)| r).collect());
    let out = k.extrude_with(&prof, &ExtrudeSpec::blind(plane.normal, h1 - h0))?;
    if std::env::var_os("CADRS_SWEEP_SOLID_DEBUG").is_some() {
        for &id in &out.bodies {
            let body = k.get(id)?;
            eprintln!(
                "planar sweep: {} faces, volume {:e}, check {:?}",
                body.faces.len(),
                super::mass::body_volume(body),
                body.check_with(super::topo::CheckLevel::Full).first()
            );
        }
    }
    Ok(out)
}

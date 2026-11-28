//! Intersections that are graphs over one parameter of one of the surfaces: every point of the
//! curve is a closed form of that parameter, so nothing is searched for or marched along.
//!
//! - A torus and a plane or a sphere. On the torus `o + ρ(v)(cos u·x + sin u·y) + r sin v·z`,
//!   ρ = R + r cos v, both a plane's and a sphere's equations are linear in cos(u − u₀):
//!   `cos(u − u₀) = (n₀ + n₁ cos v + n₂ sin v) / (A·ρ(v))`. The curve is `u = u₀ ± acos g(v)`
//!   over the stretches of v where |g| ≤ 1, whose ends (g = ±1: `α + β cos v + γ sin v = 0`)
//!   are closed forms too; the two branches meet there.
//! - Two equal tori about one centre: they meet in two planes through it, so as a torus and
//!   a plane twice.
//! - A cylinder, a cone or a circle carried along a line (a slanted cylinder) and a sphere or
//!   another of those. Along a ruling `B(θ) + h·D(θ)` a
//!   quadric's equation is a quadratic in h: `h = (−β ± √(β² − αγ))/α`, over the stretches of θ
//!   where the discriminant is positive.
//!
//! What comes back are polylines of exact points, a few a stretch and only where the curve runs
//! through the two faces' boxes; [`super::ssi`] fits them like a traced curve's. Where the
//! branches meet in a corner (a node, where the curve crosses itself: a cylinder across one
//! whose side it just reaches) a loop closes there, so that no fit runs through it. A pair that
//! touches otherwise (a double root inside a stretch, or a curve shrunk to a point) is left to
//! the numeric engine.

use super::curve::Curve;
use super::math::*;
use super::ssi_numeric::Pt;
use super::surface::Surface;

type UvBox = ((f64, f64), (f64, f64));
type M3 = nalgebra::Matrix3<f64>;

/// A polyline of exact points of an intersection, and whether it closes.
pub(super) type Poly = (Vec<Pt>, bool);

/// The intersection of `a` and `b` as polylines of exact points where it is a graph over a
/// parameter of `a` (see the module), within `a`'s box `ua` and the box `bb` in space; `None`
/// where it isn't one, or the surfaces touch.
pub(super) fn graph(a: &Surface, ua: UvBox, b: &Surface, bb: &Bbox) -> Option<Vec<Poly>> {
    match (a, b) {
        (Surface::Torus { .. }, Surface::Plane { .. } | Surface::Sphere { .. }) => {
            torus(a, ua, b, b, bb)
        }
        (
            Surface::Torus { f, rmaj, rmin },
            Surface::Torus {
                f: g,
                rmaj: rb,
                rmin: tb,
            },
        ) => {
            // Two equal tori about one centre: their equations differ by
            // 4R²((x·z₁)² − (x·z₂)²), so they meet in the planes through the centre square to
            // z₁ − z₂ and to z₁ + z₂, each of which cuts the first in a graph (the two cross
            // where the tori touch, on the line square to both axes).
            let size = rmaj + rmin;
            let same = (f.o - g.o).norm() <= 1e-9 * size
                && (rmaj - rb).abs() <= 1e-9 * size
                && (rmin - tb).abs() <= 1e-9 * size;
            if !same || f.z.cross(&g.z).norm() < 1e-6 {
                return None;
            }
            let mut out = Vec::new();
            for n in [f.z - g.z, f.z + g.z] {
                let plane = Surface::Plane {
                    f: Frame::from_z(f.o, n.normalize()),
                };
                out.extend(torus(a, ua, &plane, b, bb)?);
            }
            Some(out)
        }
        (
            Surface::Cylinder { .. } | Surface::Cone { .. } | Surface::Extrusion { .. },
            Surface::Sphere { .. }
            | Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Extrusion { .. },
        ) => ruled(a, ua, b, bb),
        _ => None,
    }
}

/// Loops smaller than this (mm) are the numeric engine's: the surfaces all but touch there.
const SMALL: f64 = 1e-4;

/// The roots in [0, 2π) of `α + β cos v + γ sin v`, rising; `None` where it has a double root
/// or all but one (the curve it bounds would be smaller than [`SMALL`] at radius `r`).
fn trig_roots(alpha: f64, beta: f64, gamma: f64, r: f64) -> Option<Vec<f64>> {
    let m = beta.hypot(gamma);
    let scale = alpha.abs().max(m);
    if m <= 1e-14 * scale || scale == 0.0 {
        return Some(Vec::new());
    }
    let x = -alpha / m;
    if r * (2.0 * (1.0 - x.abs()).abs()).sqrt() < SMALL {
        return None;
    }
    if x.abs() > 1.0 {
        return Some(Vec::new());
    }
    let (phi, w) = (gamma.atan2(beta), x.acos());
    let mut out = vec![(phi - w).rem_euclid(TAU), (phi + w).rem_euclid(TAU)];
    out.sort_by(f64::total_cmp);
    Some(out)
}

/// The stretches of `[lo, hi]` (an angle's, `hi − lo` ≤ 2π) inside the box side `(b0, b1)` of
/// that angle taken at every turn, a little beyond it either way.
fn within_turns(lo: f64, hi: f64, (b0, b1): (f64, f64)) -> Vec<(f64, f64)> {
    let pad = 1e-6 + 0.02 * (b1 - b0).abs().min(TAU);
    let (b0, b1) = (b0 - pad, b1 + pad);
    if !(b1 - b0).is_finite() || b1 - b0 >= TAU {
        return vec![(lo, hi)];
    }
    let mut out = Vec::new();
    let k0 = ((lo - b1) / TAU).ceil() as i64;
    let k1 = ((hi - b0) / TAU).floor() as i64;
    for k in k0..=k1 {
        let (x0, x1) = (b0 + k as f64 * TAU, b1 + k as f64 * TAU);
        let (s, e) = (lo.max(x0), hi.min(x1));
        if e > s {
            out.push((s, e));
        }
    }
    out
}

/// Whether the values from `x` to `y` come within the box side `(b0, b1)`; for an angle, the
/// short way from `x` to `y`, at any turn.
fn span_in(x: f64, y: f64, (b0, b1): (f64, f64), angle: bool) -> bool {
    if !(b1 - b0).is_finite() || (angle && b1 - b0 >= TAU) {
        return true;
    }
    let y = if angle { unwrap_near(y, x, TAU) } else { y };
    let (lo, hi) = (x.min(y), x.max(y));
    let pad = 0.5 * (hi - lo) + 1e-6 + 0.02 * (b1 - b0);
    let (lo, hi) = (lo - pad, hi + pad);
    if !angle {
        return lo <= b1 && hi >= b0;
    }
    // Some turn of [lo, hi] meets [b0, b1].
    let k = ((b0 - hi) / TAU).ceil();
    lo + k * TAU <= b1
}

/// The runs of `pts` (exact points; `closed` when the last joins the first) whose spans come
/// near the box `bb` and pass `near` (the parametrized surface's box).
fn runs_in(pts: Vec<Pt>, closed: bool, bb: &Bbox, near: &dyn Fn(&Pt, &Pt) -> bool) -> Vec<Poly> {
    let n = pts.len();
    if n < 2 {
        return Vec::new();
    }
    let spans = if closed { n } else { n - 1 };
    let kept = |k: usize| {
        let (p, q) = (&pts[k], &pts[(k + 1) % n]);
        let mut h = Bbox::empty();
        h.add(&p.p);
        h.add(&q.p);
        let h = h.grow(0.5 * (q.p - p.p).norm() + 1e-9);
        h.overlaps(bb) && near(p, q)
    };
    let keep: Vec<bool> = (0..spans).map(kept).collect();
    if keep.iter().all(|&k| k) {
        return vec![(pts, closed)];
    }
    // From a span left out on, round once (a closed polyline's runs may pass its join).
    let start = if closed {
        keep.iter().position(|&k| !k).unwrap_or(0)
    } else {
        0
    };
    let mut out: Vec<Poly> = Vec::new();
    let mut run: Vec<Pt> = Vec::new();
    for i in 0..spans {
        let k = if closed { (start + i) % spans } else { i };
        if keep[k] {
            if run.is_empty() {
                run.push(pts[k]);
            }
            run.push(pts[(k + 1) % n]);
        } else if !run.is_empty() {
            out.push((std::mem::take(&mut run), false));
        }
    }
    if !run.is_empty() {
        out.push((run, false));
    }
    out
}

/// A curve that is a graph over a parameter t of a surface, in two branches that meet where
/// the stretches of t it runs over end.
struct Graph<'a> {
    /// The ends of the stretches in [0, 2π), rising; none when the curve runs all the way
    /// round (or nowhere).
    turns: Vec<f64>,
    /// Whether the curve runs at `t` (strictly inside a stretch).
    runs: &'a dyn Fn(f64) -> bool,
    /// The point at `t` of the branch `sign` (±1), or the one both share where `t` is a turn.
    point: &'a dyn Fn(f64, f64, bool) -> Pt,
    /// The side of the parametrized surface's box along t.
    tbox: (f64, f64),
    /// Whether the span between two points comes within that box's other side.
    near: &'a dyn Fn(&Pt, &Pt) -> bool,
}

/// The polylines of a graph within the boxes, with their parameters on `b`.
fn polylines(g: Graph, b: &Surface, bb: &Bbox) -> Vec<Poly> {
    use std::f64::consts::{FRAC_PI_2, PI};
    // The samples of one branch over [w0, w1], crowded toward an end where the branches meet
    // (the other parameter runs like a square root there).
    let branch = |w0: f64, w1: f64, turn0: bool, turn1: bool, sign: f64| -> Vec<Pt> {
        let n = (((w1 - w0) / (TAU / 48.0)).ceil() as usize + 4).clamp(6, 49);
        (0..=n)
            .map(|i| {
                let s = i as f64 / n as f64;
                let w = match (turn0, turn1) {
                    (true, true) => 0.5 * (1.0 - (PI * s).cos()),
                    (true, false) => 1.0 - (FRAC_PI_2 * s).cos(),
                    (false, true) => (FRAC_PI_2 * s).sin(),
                    (false, false) => s,
                };
                let end = (i == 0 && turn0) || (i == n && turn1);
                (g.point)(w0 + (w1 - w0) * w, sign, end)
            })
            .collect()
    };
    let mut polys: Vec<Poly> = Vec::new();
    let mut push = |pts: Vec<Pt>, closed: bool| polys.extend(runs_in(pts, closed, bb, g.near));
    let whole = g.turns.is_empty();
    let stretches: Vec<(f64, f64)> = if whole {
        if !(g.runs)(0.0) {
            return Vec::new();
        }
        vec![(0.0, TAU)]
    } else {
        (0..g.turns.len())
            .map(|k| {
                let lo = g.turns[k];
                let hi = if k + 1 < g.turns.len() {
                    g.turns[k + 1]
                } else {
                    g.turns[0] + TAU
                };
                (lo, hi)
            })
            .filter(|&(lo, hi)| (g.runs)(0.5 * (lo + hi)))
            .collect()
    };
    // Whether the branches meet at the turn `t` in a corner (a node: the curve crosses itself
    // there, the discriminant's root double), from their chords a little way into the stretch
    // (`into` its direction): at a simple root they leave the shared point opposite ways, as
    // ±√δ along one direction; at a node at an angle. A curve is fitted only between corners:
    // a cubic through one rings ever wider as points crowd in.
    let corner = |t: f64, into: f64, len: f64| {
        let p = (g.point)(t, 1.0, true).p;
        let dt = into * 1e-4 * len;
        let u = (g.point)(t + dt, 1.0, false).p - p;
        let d = (g.point)(t + dt, -1.0, false).p - p;
        let (nu, nd) = (u.norm(), d.norm());
        nu > 0.0 && nd > 0.0 && u.dot(&d) > -0.9 * nu * nd
    };
    for (lo, hi) in stretches {
        for (w0, w1) in within_turns(lo, hi, g.tbox) {
            let (t0, t1) = (!whole && w0 == lo, !whole && w1 == hi);
            let up = branch(w0, w1, t0, t1, 1.0);
            let mut down = branch(w0, w1, t0, t1, -1.0);
            let (c0, c1) = (
                t0 && corner(w0, 1.0, w1 - w0),
                t1 && corner(w1, -1.0, w1 - w0),
            );
            // A loop whose join at its end `hi` is a corner (a node): begun there, so that the
            // corner is where it closes, which its fit leaves free ([`super::ssi`]'s fit
            // repeats a closed curve's first point last). A branch whose two ends are one node
            // closes there by itself.
            if t0 && t1 && (c0 || c1) {
                let node = (up[0].p - up[up.len() - 1].p).norm() <= super::tol::POINT;
                if c0 && c1 && node {
                    for mut l in [up, down] {
                        l.pop();
                        push(l, true);
                    }
                    continue;
                }
                if c1 && !c0 {
                    let mut l = up;
                    l.reverse();
                    l.extend(down.into_iter().skip(1));
                    l.pop();
                    push(l, true);
                    continue;
                }
            }
            match (t0, t1) {
                // Both ends shared: one loop, up one branch and back down the other.
                (true, true) => {
                    down.reverse();
                    let mut l = up;
                    l.extend(down.into_iter().skip(1));
                    l.pop();
                    push(l, true);
                }
                (true, false) => {
                    down.reverse();
                    down.pop();
                    down.extend(up);
                    push(down, false);
                }
                (false, true) => {
                    down.reverse();
                    let mut l = up;
                    l.extend(down.into_iter().skip(1));
                    push(l, false);
                }
                (false, false) => {
                    let all = whole && w0 == lo && w1 == hi;
                    for mut l in [up, down] {
                        if all {
                            l.pop();
                        }
                        push(l, all);
                    }
                }
            }
        }
    }
    // The points' parameters on the other surface, each from its neighbour's.
    for (pts, _) in polys.iter_mut() {
        let mut hint = None;
        for q in pts.iter_mut() {
            q.b = b.project(&q.p, hint);
            hint = Some(q.b);
        }
    }
    polys
}

/// A torus and a plane or a sphere `b`: `cos(u − u₀) = g(v)` (see the module). The points'
/// second parameters are `on`'s (`b`, or the surface `b` stands for).
fn torus(a: &Surface, ua: UvBox, b: &Surface, on: &Surface, bb: &Bbox) -> Option<Vec<Poly>> {
    let Surface::Torus { f, rmaj, rmin } = a else {
        return None;
    };
    let (rr, r) = (*rmaj, *rmin);
    // (A torus whose tube reaches its axis has ρ = 0 on it.)
    if rr <= r * (1.0 + 1e-6) || r <= 0.0 {
        return None;
    }
    // cos(u − u0) = (n0 + n1 cos v + n2 sin v) / (amp·(R + r cos v)).
    let (ax, ay, n0, n1, n2) = match b {
        Surface::Plane { f: fp } => {
            let n = fp.x.cross(&fp.y);
            let d = f.o - fp.o;
            (n.dot(&f.x), n.dot(&f.y), -n.dot(&d), 0.0, -r * n.dot(&f.z))
        }
        Surface::Sphere { f: fs, r: s } => {
            let d = f.o - fs.o;
            (
                2.0 * d.dot(&f.x),
                2.0 * d.dot(&f.y),
                s * s - d.norm_squared() - rr * rr - r * r,
                -2.0 * rr * r,
                -2.0 * r * d.dot(&f.z),
            )
        }
        _ => return None,
    };
    let amp = ax.hypot(ay);
    let scale = match b {
        Surface::Plane { .. } => 1.0,
        _ => 2.0 * (rr + r),
    };
    // (The plane square to the axis, the sphere centred on it: circles, and not a graph over v.)
    if amp <= 1e-6 * scale {
        return None;
    }
    let u0 = ay.atan2(ax);
    let g = |v: f64| (n0 + n1 * v.cos() + n2 * v.sin()) / (amp * (rr + r * v.cos()));
    // Where g = 1 (the branches meet at u0) and where g = −1 (at u0 + π).
    let mut turns = trig_roots(n0 - amp * rr, n1 - amp * r, n2, r)?;
    turns.extend(trig_roots(n0 + amp * rr, n1 + amp * r, n2, r)?);
    turns.sort_by(f64::total_cmp);
    if turns.windows(2).any(|w| r * (w[1] - w[0]) < SMALL) {
        return None;
    }
    let ubox = ua.0;
    Some(polylines(
        Graph {
            turns,
            runs: &|v| g(v).abs() < 1.0,
            point: &|v, sign, turn| {
                let c = if turn {
                    g(v).signum()
                } else {
                    g(v).clamp(-1.0, 1.0)
                };
                let u = u0 + sign * c.acos();
                Pt {
                    p: a.eval(u, v),
                    a: (u, v),
                    b: (0.0, 0.0),
                }
            },
            tbox: ua.1,
            near: &|p, q| span_in(p.a.0, q.a.0, ubox, true),
        },
        on,
        bb,
    ))
}

/// The roots in [0, 2π) of `d` (a smooth function of an angle), rising, from its signs at 128
/// angles; `None` where it comes within `floor` of zero without crossing it (a double root, or
/// two too near each other to tell).
fn sign_roots(d: &dyn Fn(f64) -> f64, floor: f64) -> Option<Vec<f64>> {
    const N: usize = 128;
    let at = |k: usize| TAU * k as f64 / N as f64;
    let ds: Vec<f64> = (0..N).map(|k| d(at(k))).collect();
    let mut out = Vec::new();
    for k in 0..N {
        let (prev, here, next) = (ds[(k + N - 1) % N], ds[k], ds[(k + 1) % N]);
        if (here > 0.0) != (next > 0.0) {
            // Bisection, a secant step first each round (the Illinois rule's safeguard).
            let (mut x0, mut x1, mut f0, mut f1) = (at(k), at(k) + TAU / N as f64, here, next);
            for _ in 0..80 {
                if x1 - x0 <= 4.0 * f64::EPSILON * (1.0 + x1.abs()) {
                    break;
                }
                let secant = x0 - f0 * (x1 - x0) / (f1 - f0);
                let mid = 0.5 * (x0 + x1);
                for x in [secant, mid] {
                    if !(x > x0 && x < x1) {
                        continue;
                    }
                    let f = d(x);
                    if (f > 0.0) == (f0 > 0.0) {
                        (x0, f0) = (x, f);
                    } else {
                        (x1, f1) = (x, f);
                    }
                }
            }
            out.push(0.5 * (x0 + x1));
            continue;
        }
        // Nearest zero of its neighbours without crossing: how near it comes between them
        // (a golden section search for the least of ±d).
        let s = if here > 0.0 { 1.0 } else { -1.0 };
        if (prev > 0.0) == (here > 0.0) && here.abs() <= prev.abs() && here.abs() <= next.abs() {
            let least =
                super::math::minimize(|x| s * d(x), at(k) - TAU / N as f64, at(k) + TAU / N as f64);
            let (f1, f2) = (s * d(least), f64::INFINITY);
            if f1.min(f2) < floor {
                return None;
            }
        }
    }
    out.sort_by(f64::total_cmp);
    Some(out)
}

/// [`ruled`] where `a`'s rulings all run along one of `b`'s: along the ruling at θ the equation
/// is `2β h + γ = 0` (`coefs` gives β and γ), β a constant and a cosine and a sine of θ. The
/// surfaces meet in the rulings where β and γ are both zero (a ruling the two share: where β's
/// zero is double they touch along it, `a` to one side of `b`) and in the curve `h = −γ/2β`
/// elsewhere. Only the shared rulings are given here: `None` where that curve comes within the
/// boxes (the numeric engine's), so what this settles in closed form is the pair that only
/// touches, a search of both faces for a crossing that isn't there.
fn along_a_ruling(
    a: &Surface,
    ua: UvBox,
    b: &Surface,
    bb: &Bbox,
    coefs: &dyn Fn(f64) -> (f64, f64),
) -> Option<Vec<Poly>> {
    let (tbox, vbox) = ua;
    if !(vbox.0.is_finite() && vbox.1.is_finite()) || tbox.1 - tbox.0 > TAU + 1e-9 {
        return None;
    }
    // β = A + B cos θ + C sin θ, from three of its values.
    let (b0, b1, b2) = (
        coefs(0.0).0,
        coefs(std::f64::consts::FRAC_PI_2).0,
        coefs(std::f64::consts::PI).0,
    );
    let ca = 0.5 * (b0 + b2);
    let (cb, cc) = (b0 - ca, b1 - ca);
    let m = cb.hypot(cc);
    let scale = ca.abs().max(m);
    if scale == 0.0 {
        return None;
    }
    // β's zeros in a turn: two, one (double: the cosine's crest or trough at zero), or none.
    let mut turn: Vec<f64> = Vec::new();
    if m > 1e-12 * scale {
        let (phi, x) = (cc.atan2(cb), -ca / m);
        if (1.0 - x.abs()).abs() < 1e-9 {
            turn.push(if x > 0.0 {
                phi
            } else {
                phi + std::f64::consts::PI
            });
        } else if x.abs() < 1.0 {
            turn.extend([phi - x.acos(), phi + x.acos()]);
        }
    }
    // (Each at every turn within the box.)
    let mut zeros: Vec<f64> = Vec::new();
    for z in turn {
        let mut t = z + TAU * ((tbox.0 - z) / TAU).floor();
        while t <= tbox.1 + 1e-9 {
            if t >= tbox.0 - 1e-9 {
                zeros.push(t);
            }
            t += TAU;
        }
    }
    let size = bb.diagonal().max(crate::native::tol::POINT);
    let span = vbox.1 - vbox.0;
    // The curve h = −γ/2β: nowhere within the boxes, or the pair is the numeric engine's.
    // Sampled along θ (not at a zero of β, where it runs off along the ruling or through the
    // shared one), each span between samples widened by its own length.
    const N: usize = 512;
    let dt = (tbox.1 - tbox.0) / N as f64;
    let grown = bb.grow(1e-6 * size);
    let mut last: Option<(f64, P3)> = None;
    for i in 0..=N {
        let t = tbox.0 + dt * i as f64;
        if zeros.iter().any(|z| (t - z).abs() <= 0.5 * dt) {
            last = None;
            continue;
        }
        let (be, ga) = coefs(t);
        let h = -ga / (2.0 * be);
        if !h.is_finite() {
            return None;
        }
        let q = a.eval(t, h.clamp(vbox.0 - span, vbox.1 + span));
        let reach = last.map_or(0.0, |(h0, q0): (f64, P3)| {
            (h - h0).abs().min(4.0 * span).max((q - q0).norm())
        });
        if h >= vbox.0 - reach && h <= vbox.1 + reach && grown.grow(reach).contains(&q) {
            return None;
        }
        last = Some((h, q));
    }
    // The shared rulings: where γ is zero with β, the ruling itself within the boxes.
    let mut polys: Vec<Poly> = Vec::new();
    for &z in &zeros {
        if coefs(z).1.abs() > 1e-9 * scale * size.max(span) {
            continue;
        }
        const PIECES: usize = 8;
        let pts: Vec<Pt> = (0..=PIECES)
            .map(|i| {
                let h = vbox.0 + span * i as f64 / PIECES as f64;
                Pt {
                    p: a.eval(z, h),
                    a: (z, h),
                    b: (0.0, 0.0),
                }
            })
            .collect();
        polys.extend(runs_in(pts, false, bb, &|_, _| true));
    }
    for (pts, _) in polys.iter_mut() {
        let mut hint = None;
        for q in pts.iter_mut() {
            q.b = b.project(&q.p, hint);
            hint = Some(q.b);
        }
    }
    Some(polys)
}

/// A cylinder, a cone or a circle carried along a line `a` and a sphere, a cylinder, a cone or
/// such a carried circle `b`: along `a`'s ruling at θ,
/// `B(θ) + h·D(θ)`, `b`'s equation `xᵀMx + 2l·x + k = 0` (x from `b`'s origin) is
/// `α h² + 2β h + γ = 0`.
fn ruled(a: &Surface, ua: UvBox, b: &Surface, bb: &Bbox) -> Option<Vec<Poly>> {
    // Whether a surface's rulings all run one way.
    let prism = |s: &Surface| matches!(s, Surface::Cylinder { .. } | Surface::Extrusion { .. });
    // (The other way round the rulings all meet the cone alike.)
    if matches!(a, Surface::Cone { .. }) && prism(b) {
        return None;
    }
    // The ruling at θ: its point at h = 0 and its direction.
    type Ruling<'a> = Box<dyn Fn(f64) -> (P3, V3) + 'a>;
    let (ruling, ra): (Ruling, f64) = match a {
        Surface::Cylinder { f, r } => (
            Box::new(move |t: f64| (f.o + (f.x * t.cos() + f.y * t.sin()) * *r, f.z)),
            *r,
        ),
        Surface::Cone { f, r, a } => {
            let (sa, ca) = a.sin_cos();
            (
                Box::new(move |t: f64| {
                    let e = f.x * t.cos() + f.y * t.sin();
                    (f.o + e * *r, e * sa + f.z * ca)
                }),
                *r,
            )
        }
        Surface::Extrusion {
            c: c @ Curve::Circle { r, .. },
            d,
        } => (Box::new(move |t: f64| (c.point(t), *d)), *r),
        _ => return None,
    };
    // b as a quadric about a point: xᵀMx + 2l·x + k = 0.
    let axial = |z: &V3, m: f64| M3::identity() * m - z * z.transpose();
    let (ob, m, l, k) = match b {
        Surface::Sphere { f, r } => (f.o, M3::identity(), V3::zeros(), -r * r),
        Surface::Cylinder { f, r } => (f.o, axial(&f.z, 1.0), V3::zeros(), -r * r),
        Surface::Cone { f, r, a } => {
            let (s, c) = a.sin_cos();
            if c.abs() < 1e-6 {
                return None;
            }
            (f.o, axial(&f.z, c * c), f.z * (-r * s * c), -r * r * c * c)
        }
        // A circle carried along d: a point x (from the circle's centre) is on it where its
        // foot along d in the circle's plane, x − d (n·x)/(n·d), is r from the centre.
        Surface::Extrusion {
            c: Curve::Circle { c, x, y, r },
            d,
        } => {
            let n = x.cross(y);
            let nd = n.dot(d);
            if nd.abs() < 1e-6 * d.norm() {
                return None;
            }
            let foot = M3::identity() - d * n.transpose() / nd;
            (*c, foot.transpose() * foot, V3::zeros(), -r * r)
        }
        _ => return None,
    };
    // (α, β, γ) at θ.
    let coefs = |t: f64| {
        let (base, d) = ruling(t);
        let base = base - ob;
        let md = m * d;
        (
            d.dot(&md),
            base.dot(&md) + l.dot(&d),
            base.dot(&(m * base)) + 2.0 * l.dot(&base) + k,
        )
    };
    // The rulings must all cross the quadric's directions: α of one sign, away from zero (a
    // ruling along one of b's has a root at infinity, and the branches change over there).
    let dn = ruling(0.0).1.norm();
    // Rulings that all run along one of b's own (a cylinder along a cone's ruling, as a
    // cutter's shank lies on the cone its side sweeps): α is zero at every θ and the equation
    // along a ruling linear.
    if dn > 1e-12
        && prism(a)
        && (0..32).all(|i| coefs(TAU * i as f64 / 32.0).0.abs() <= 1e-12 * dn * dn)
    {
        return along_a_ruling(a, ua, b, bb, &|t| {
            let (_, be, ga) = coefs(t);
            (be, ga)
        });
    }
    let mut alpha_min = f64::INFINITY;
    let mut sign = 0.0;
    for i in 0..32 {
        let al = coefs(TAU * i as f64 / 32.0).0;
        if i > 0 && (al > 0.0) != (sign > 0.0) {
            return None;
        }
        sign = al;
        alpha_min = alpha_min.min(al.abs());
    }
    if alpha_min < 1e-6 * dn * dn || dn < 1e-12 {
        return None;
    }
    // (In millimetres along a ruling.)
    let alpha_min = alpha_min / dn;
    let o = ruling(0.0).0 - ob;
    let disc = |t: f64| {
        let (al, be, ga) = coefs(t);
        be * be - al * ga
    };
    // (The chord along a ruling is 2√Δ/|α|: a loop narrower than SMALL is a touch.)
    let turns = sign_roots(&disc, (0.5 * SMALL * alpha_min).powi(2))?;
    let size = ra.abs() + o.norm() + (-k).sqrt();
    if turns.windows(2).any(|w| {
        size * (w[1] - w[0]) < SMALL
            || disc(0.5 * (w[0] + w[1])).abs() < (SMALL * alpha_min).powi(2)
    }) {
        return None;
    }
    let vbox = ua.1;
    Some(polylines(
        Graph {
            turns,
            runs: &|t| disc(t) > 0.0,
            point: &|t, sign, turn| {
                let (al, be, ga) = coefs(t);
                let s = if turn {
                    0.0
                } else {
                    (be * be - al * ga).max(0.0).sqrt()
                };
                // (The root without cancellation: by the other one where −β and ±√Δ differ in
                // sign.)
                let h = if -be * sign >= 0.0 || s == 0.0 {
                    (-be + sign * s) / al
                } else {
                    ga / (-be - sign * s)
                };
                Pt {
                    p: a.eval(t, h),
                    a: (t, h),
                    b: (0.0, 0.0),
                }
            },
            tbox: ua.0,
            near: &|p, q| span_in(p.a.1, q.a.1, vbox, false),
        },
        b,
        bb,
    ))
}

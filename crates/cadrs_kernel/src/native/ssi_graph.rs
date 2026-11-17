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
//! - A cylinder or a cone and a sphere, a cylinder or a cone. Along a ruling `B(θ) + h·D(θ)` a
//!   quadric's equation is a quadratic in h: `h = (−β ± √(β² − αγ))/α`, over the stretches of θ
//!   where the discriminant is positive.
//!
//! What comes back are polylines of exact points, a few a stretch and only where the curve runs
//! through the two faces' boxes; [`super::ssi`] fits them like a traced curve's. A pair that
//! touches (a double root: the curve crosses itself or shrinks to a point there) is left to the
//! numeric engine.

use super::math::*;
use super::ssi_numeric::Pt;
use super::surface::Surface;

type UvBox = ((f64, f64), (f64, f64));

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
            Surface::Cylinder { .. } | Surface::Cone { .. },
            Surface::Sphere { .. } | Surface::Cylinder { .. } | Surface::Cone { .. },
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
    for (lo, hi) in stretches {
        for (w0, w1) in within_turns(lo, hi, g.tbox) {
            let (t0, t1) = (!whole && w0 == lo, !whole && w1 == hi);
            let up = branch(w0, w1, t0, t1, 1.0);
            let mut down = branch(w0, w1, t0, t1, -1.0);
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
            let (mut lo, mut hi) = (at(k) - TAU / N as f64, at(k) + TAU / N as f64);
            const PHI: f64 = 0.618_033_988_749_894_9;
            let (mut x1, mut x2) = (hi - PHI * (hi - lo), lo + PHI * (hi - lo));
            let (mut f1, mut f2) = (s * d(x1), s * d(x2));
            for _ in 0..40 {
                if f1 < f2 {
                    (hi, x2, f2) = (x2, x1, f1);
                    x1 = hi - PHI * (hi - lo);
                    f1 = s * d(x1);
                } else {
                    (lo, x1, f1) = (x1, x2, f2);
                    x2 = lo + PHI * (hi - lo);
                    f2 = s * d(x2);
                }
            }
            if f1.min(f2) < floor {
                return None;
            }
        }
    }
    out.sort_by(f64::total_cmp);
    Some(out)
}

/// A cylinder or a cone `a` and a sphere, a cylinder or a cone `b`: along `a`'s ruling at θ,
/// `B(θ) + h·D(θ)`, `b`'s equation `xᵀMx + 2l·x + k = 0` (x from `b`'s origin) is
/// `α h² + 2β h + γ = 0`.
fn ruled(a: &Surface, ua: UvBox, b: &Surface, bb: &Bbox) -> Option<Vec<Poly>> {
    // The ruling at θ: its point at h = 0 and its direction.
    let (fa, ra, (sa, ca)) = match a {
        Surface::Cylinder { f, r } => (f, *r, (0.0, 1.0)),
        // (The other way round the cylinder's rulings all meet the cone alike.)
        Surface::Cone { .. } if matches!(b, Surface::Cylinder { .. }) => return None,
        Surface::Cone { f, r, a } => (f, *r, a.sin_cos()),
        _ => return None,
    };
    // b as a quadric about its origin: M = m·I − zzᵀ.
    let (fb, m, l, k) = match b {
        Surface::Sphere { f, r } => (f, 1.0, V3::zeros(), -r * r),
        Surface::Cylinder { f, r } => (f, 1.0, V3::zeros(), -r * r),
        Surface::Cone { f, r, a } => {
            let (s, c) = a.sin_cos();
            if c.abs() < 1e-6 {
                return None;
            }
            (f, c * c, f.z * (-r * s * c), -r * r * c * c)
        }
        _ => return None,
    };
    let axial = !matches!(b, Surface::Sphere { .. });
    let quad = |x: &V3, y: &V3| {
        m * x.dot(y)
            - if axial {
                x.dot(&fb.z) * y.dot(&fb.z)
            } else {
                0.0
            }
    };
    let o = fa.o - fb.o;
    // (α, β, γ) at θ.
    let coefs = |t: f64| {
        let e = fa.x * t.cos() + fa.y * t.sin();
        let (base, d) = (o + e * ra, e * sa + fa.z * ca);
        (
            quad(&d, &d),
            quad(&d, &base) + l.dot(&d),
            quad(&base, &base) + 2.0 * l.dot(&base) + k,
        )
    };
    // The rulings must all cross the quadric's directions: α of one sign, away from zero (a
    // ruling along one of b's has a root at infinity, and the branches change over there).
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
    if alpha_min < 1e-6 {
        return None;
    }
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

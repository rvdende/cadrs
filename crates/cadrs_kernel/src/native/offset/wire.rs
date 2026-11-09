//! Offsets of planar wires, in the wire's plane (z = 0 here): exact, the curve at the distance
//! from the wire (with arc joins at its convex corners), trimmed of what lies nearer.
//!
//! 0. The edges are cut where their curvature has an extremum or changes sign (`MAT2d_CutCurve`'s
//!    points, found as `math_FunctionRoots` finds them), and where their radius of curvature on
//!    the side offset to equals the distance: there the offset has a cusp, which so falls at the
//!    ends of pieces whose offsets are each smooth. A nearly planar wire is first flattened.
//! 1. Every edge is offset to one side: lines and arcs exactly (an arc whose radius would fall
//!    to nothing is dropped), other curves as cubics through the offset points at the edge's own
//!    parameters, refined until they follow the exact offset within 1e-10 of the wire's size.
//! 2. Consecutive offsets are joined by arcs about the vertex between them, turning as the
//!    wire turns (at convex corners however slight). An open wire is run out and back, so its
//!    ends are half turns (round caps).
//! 3. The raw loop is split where it crosses itself, and only the pieces that stay the full
//!    distance from the wire are kept (the rest lie in overlaps: concave corners, arcs inside
//!    the distance, the swallowtails of an edge curving more tightly than the distance).
//! 4. Neighbouring pieces of one curve are joined again and the pieces chained into loops.

use crate::native::curve::Curve;
use crate::native::math::*;
use crate::native::nurbs::NurbsCurve;
use crate::native::tol;
use crate::{KernelError, Result};
use std::f64::consts::PI;

/// A curve between two parameters, run from `t0` to `t1` (`t0 < t1`).
#[derive(Debug, Clone)]
pub struct Piece2 {
    pub c: Curve,
    pub t0: f64,
    pub t1: f64,
}

impl Piece2 {
    fn start(&self) -> P3 {
        self.c.point(self.t0)
    }
    fn end(&self) -> P3 {
        self.c.point(self.t1)
    }
    fn dir(&self, t: f64) -> V2 {
        let d = self.c.tangent(t);
        V2::new(d.x, d.y)
            .try_normalize(tol::TINY)
            .unwrap_or(V2::new(1.0, 0.0))
    }
}

/// One element of the raw offset loop and where it came from.
#[derive(Debug, Clone)]
struct Raw {
    p: Piece2,
    /// How far it may lie from the exact offset (an approximated curve's).
    err: f64,
}

fn left(v: V2) -> V2 {
    V2::new(-v.y, v.x)
}

fn p2(p: &P3) -> V2 {
    V2::new(p.x, p.y)
}

/// The edge run forwards (`t0 < t1`).
fn forward(c: &Curve, t0: f64, t1: f64) -> Piece2 {
    if t1 >= t0 {
        Piece2 {
            c: c.clone(),
            t0,
            t1,
        }
    } else {
        // `reversed` maps an increasing range: the edge's [t1, t0] run the other way.
        let (c, a, b) = c.reversed(t1, t0);
        Piece2 { c, t0: a, t1: b }
    }
}

/// The edge offset `l` to its left and how far that lies from the exact offset at most (lines and
/// arcs exactly; other curves as cubics through offset points at the edge's own parameters,
/// within 1e-10 of the wire's size), or None when it shrinks to nothing.
fn offset_edge(e: &Piece2, l: f64, size: f64) -> Option<(Piece2, f64)> {
    match &e.c {
        Curve::Line { .. } => {
            let n = left(e.dir(e.t0));
            let sh = V3::new(n.x, n.y, 0.0) * l;
            let (c, len) = Curve::line(e.start() + sh, e.end() + sh);
            Some((
                Piece2 {
                    c,
                    t0: 0.0,
                    t1: len,
                },
                0.0,
            ))
        }
        Curve::Circle { c, x, y, r } => {
            // Run counter-clockwise the left is the centre's side.
            let ccw = x.cross(y).z > 0.0;
            let r2 = if ccw { r - l } else { r + l };
            if r2 <= tol::REL * size {
                return None;
            }
            Some((
                Piece2 {
                    c: Curve::Circle {
                        c: *c,
                        x: *x,
                        y: *y,
                        r: r2,
                    },
                    t0: e.t0,
                    t1: e.t1,
                },
                0.0,
            ))
        }
        _ => {
            // The offset point and its derivative, o' = |c'|(1 − κl)·T: smooth in the edge's
            // parameter even where the offset has a cusp (o' = 0), so a cubic through points at
            // those parameters converges fast.
            let at = |t: f64| -> (P3, V3) {
                let (p, d1, d2) = e.c.d2(t);
                let v = V2::new(d1.x, d1.y);
                let sp = v.norm().max(tol::TINY);
                let tn = v / sp;
                let k = cross2(v, V2::new(d2.x, d2.y)) / (sp * sp * sp);
                let n = left(tn);
                let dv = tn * (sp * (1.0 - k * l));
                (p + V3::new(n.x, n.y, 0.0) * l, V3::new(dv.x, dv.y, 0.0))
            };
            let fit_tol = size * 1e-10;
            // Evenly along each knot span (the piece is smooth: `cut_at_cusps` cut it where its
            // curvature may jump).
            let mut base: Vec<f64> = vec![e.t0];
            if let Curve::Nurbs(n) = &e.c {
                let eps = 1e-9 * (e.t1 - e.t0);
                for &k in &n.knots {
                    if k > base[base.len() - 1] + eps && k < e.t1 - eps {
                        base.push(k);
                    }
                }
            }
            base.push(e.t1);
            let per = (32 / (base.len() - 1)).max(4);
            let base: Vec<f64> = base
                .windows(2)
                .flat_map(|w| (0..per).map(move |i| w[0] + (w[1] - w[0]) * i as f64 / per as f64))
                .chain([e.t1])
                .collect();
            let mut split = 1;
            loop {
                let ts: Vec<f64> = base
                    .windows(2)
                    .flat_map(|w| {
                        (0..split).map(move |i| w[0] + (w[1] - w[0]) * i as f64 / split as f64)
                    })
                    .chain([e.t1])
                    .collect();
                let pts: Vec<P3> = ts.iter().map(|&t| at(t).0).collect();
                // End derivatives where the offset has a tangent there.
                let end_d = |t: f64| {
                    let d = at(t).1;
                    (d.norm() > 1e-3 * (e.c.d2(t).1.norm())).then_some(d)
                };
                let c = NurbsCurve::interpolate(&pts, &ts, 3, end_d(e.t0), end_d(e.t1));
                // The error: halfway between the points it passes through.
                let err = ts
                    .windows(2)
                    .map(|w| {
                        let m = 0.5 * (w[0] + w[1]);
                        (c.point(m) - at(m).0).norm()
                    })
                    .fold(0.0, f64::max);
                if err <= fit_tol || ts.len() > 4096 {
                    let (a, b) = c.range();
                    return Some((
                        Piece2 {
                            c: Curve::Nurbs(std::sync::Arc::new(c)),
                            t0: a,
                            t1: b,
                        },
                        err,
                    ));
                }
                split *= 2;
            }
        }
    }
}

/// The arc about `v` from `v + l·n1` turning by `phi`.
fn join_arc(v: P3, n1: V2, phi: f64, l: f64) -> Piece2 {
    let u0 = n1 * l.signum();
    let y = left(u0) * phi.signum();
    Piece2 {
        c: Curve::Circle {
            c: v,
            x: V3::new(u0.x, u0.y, 0.0),
            y: V3::new(y.x, y.y, 0.0),
            r: l.abs(),
        },
        t0: 0.0,
        t1: phi.abs(),
    }
}

/// The plane through `pts` fitting them best (least squares): its centroid and normal, when
/// every point lies within `rel` of the points' extent from it.
pub fn best_plane(pts: &[P3], rel: f64) -> Option<(P3, V3)> {
    if pts.len() < 3 {
        return None;
    }
    let o = P3::from(pts.iter().fold(V3::zeros(), |s, p| s + p.coords) / pts.len() as f64);
    let cov = pts.iter().fold(nalgebra::Matrix3::zeros(), |m, p| {
        let d = p - o;
        m + d * d.transpose()
    });
    let eig = nalgebra::SymmetricEigen::new(cov);
    let k = eig.eigenvalues.imin();
    let n: V3 = eig.eigenvectors.column(k).into();
    let n = n.try_normalize(tol::TINY)?;
    let size = pts.iter().map(|p| (p - o).norm()).fold(0.0, f64::max);
    pts.iter()
        .all(|p| (p - o).dot(&n).abs() <= rel * size)
        .then_some((o, n))
}

/// The curve projected onto the plane z = 0 (a wire only nearly planar, fitted to that plane),
/// with its range: unchanged when it lies in the plane already.
fn flattened(c: &Curve, t0: f64, t1: f64) -> (Curve, f64, f64) {
    let flat = |n: &NurbsCurve| {
        let mut n = n.clone();
        for h in &mut n.cps {
            h.z = 0.0;
        }
        n
    };
    match c {
        Curve::Line { o, d } if d.z != 0.0 || o.z != 0.0 => {
            let (a, b) = (c.point(t0), c.point(t1));
            let (l, len) = Curve::line(P3::new(a.x, a.y, 0.0), P3::new(b.x, b.y, 0.0));
            if t1 >= t0 {
                (l, 0.0, len)
            } else {
                (l, len, 0.0)
            }
        }
        Curve::Circle { c: o, x, y, r }
            if x.z.abs() < tol::ROUNDOFF_FINE && y.z.abs() < tol::ROUNDOFF_FINE && o.z != 0.0 =>
        {
            (
                Curve::Circle {
                    c: P3::new(o.x, o.y, 0.0),
                    x: *x,
                    y: *y,
                    r: *r,
                },
                t0,
                t1,
            )
        }
        Curve::Ellipse { c: o, x, y, a, b }
            if x.z.abs() < tol::ROUNDOFF_FINE && y.z.abs() < tol::ROUNDOFF_FINE && o.z != 0.0 =>
        {
            (
                Curve::Ellipse {
                    c: P3::new(o.x, o.y, 0.0),
                    x: *x,
                    y: *y,
                    a: *a,
                    b: *b,
                },
                t0,
                t1,
            )
        }
        Curve::Circle { x, y, .. } | Curve::Ellipse { x, y, .. }
            if x.z.abs() >= tol::ROUNDOFF_FINE || y.z.abs() >= tol::ROUNDOFF_FINE =>
        {
            let (lo, hi) = (t0.min(t1), t0.max(t1));
            let n = flat(&c.to_nurbs(lo, hi));
            if t1 >= t0 {
                (Curve::Nurbs(std::sync::Arc::new(n)), lo, hi)
            } else {
                (Curve::Nurbs(std::sync::Arc::new(n)), hi, lo)
            }
        }
        Curve::Nurbs(n) if n.cps.iter().any(|h| h.z != 0.0) => {
            (Curve::Nurbs(std::sync::Arc::new(flat(n))), t0, t1)
        }
        _ => (c.clone(), t0, t1),
    }
}

/// The parameters of `c` on [t0, t1] where its signed curvature has an extremum or changes sign
/// (OCCT's `Geom2dLProp_CurAndInf2d`), in order: none on lines and circles, the vertices of an
/// ellipse, and on a B-spline the roots of κ' and κ found as OCCT finds them
/// ([`bspline_curvature_points`]).
fn curvature_points(c: &Curve, t0: f64, t1: f64) -> Vec<f64> {
    if let Curve::Nurbs(n) = c {
        return bspline_curvature_points(n, t0, t1);
    }
    let mut out = Vec::new();
    match c {
        Curve::Line { .. } | Curve::Circle { .. } => {}
        Curve::Ellipse { .. } => {
            let k0 = (t0 / (PI / 2.0)).floor() as i64;
            for k in k0..=k0 + 5 {
                let t = k as f64 * PI / 2.0;
                if t >= t0 && t <= t1 {
                    out.push(t);
                }
            }
        }
        Curve::Nurbs(_) => {}
    }
    out
}

// OCCT's constants for finding the spine's cuts, by the names they have there.
/// `Precision::PConfusion`: parameters closer than this are one.
const P_CONFUSION: f64 = 1e-9;
/// `gp::Resolution`: the smallest magnitude a division may take.
const RESOLUTION: f64 = f64::MIN_POSITIVE;
/// `Geom2dLProp_NumericCurInf2d`: the tolerances of κ' roots, as a fraction of the piece's span.
const CUR_EXT_EPS: f64 = 1e-4;
/// `Geom2dLProp_NumericCurInf2d`: the tolerances of κ roots (absolute).
const CUR_NUL_EPS: f64 = 1e-6;
/// `Geom2dLProp_FuncCurNul`: a second derivative this small makes the sine of κ zero.
const CUR_NUL_D2: f64 = 1e-4;
/// `math_FunctionRoots`: roots closer than this fraction of the range's magnitude are one.
const ROOTS_SAME: f64 = 1e-10;
/// `math_FunctionRoots`: a derivative this small is taken as zero.
const ROOTS_FLAT: f64 = 1e-10;

/// The roots of κ' (curvature extrema) and of κ (inflections) of the B-spline `n` on [t0, t1],
/// in order, as OCCT's `Geom2dLProp_CurAndInf2d` finds them for `MAT2d_CutCurve`, so an
/// offset's spine is cut where OCCT cuts it:
/// - κ' on each C3 piece (interior knots of multiplicity above degree − 3): OCCT's
///   `Geom2dLProp_FuncCurExt` (κ'·|D1|, its derivative by a difference of 1e-6 of the piece's
///   span) through [`function_roots`] with 100 samples and tolerances of 1e-4 of the span, each
///   root polished by [`bracketed_root`] within that tolerance;
/// - κ the same way through `Geom2dLProp_FuncCurNul` (the sine between D1 and D2, taken as
///   zero where |D2| < 1e-4) with 30 samples and tolerances of 1e-6.
///
/// A piece's last sample is the knot that ends it, where the curve is evaluated on the next
/// piece (as OCCT's `BSplCLib::LocateParameter` picks the span): a jump of κ' there is a root.
fn bspline_curvature_points(n: &NurbsCurve, t0: f64, t1: f64) -> Vec<f64> {
    // The C3 pieces.
    let p = n.degree;
    let mut breaks = vec![t0];
    let mut i = 0;
    while i < n.knots.len() {
        let k = n.knots[i];
        let m = n.knots[i..].iter().take_while(|&&x| x == k).count();
        if k > t0 && k < t1 && p < m + 3 {
            breaks.push(k);
        }
        i += m;
    }
    breaks.push(t1);
    let d123 = |u: f64| {
        let d = n.ders(u, 3);
        (
            V2::new(d[1].x, d[1].y),
            V2::new(d[2].x, d[2].y),
            V2::new(d[3].x, d[3].y),
        )
    };
    // κ'·|D1| (Geom2dLProp_FuncCurExt::Value).
    let cur_ext = |u: f64| -> Option<f64> {
        let (v1, v2, v3) = d123(u);
        let v1v1 = v1.norm_squared();
        let v13 = v1v1 * v1v1.sqrt();
        let v15 = v13 * v1v1;
        if v15 < RESOLUTION {
            return None;
        }
        Some(cross2(v1, v3) / v13 - 3.0 * cross2(v1, v2) * v1.dot(&v2) / v15)
    };
    let mut out: Vec<f64> = Vec::new();
    // LProp_CurAndInf: kept in order, a parameter already there dropped.
    let add = |out: &mut Vec<f64>, t: f64| match out.iter().position(|&x| x > t) {
        Some(i) => out.insert(i, t),
        None if out.last().is_none_or(|&l| t > l) => out.push(t),
        None => {}
    };
    for w in breaks.windows(2) {
        let (a, b) = (w[0], w[1]);
        let eps_h = CUR_EXT_EPS * (b - a);
        let func = CurExt {
            f: &cur_ext,
            eps: eps_h,
            last: t1,
        };
        for r in function_roots(&func, a, b, 100, eps_h, eps_h, eps_h).unwrap_or_default() {
            let polished =
                bracketed_root(&cur_ext, r - eps_h, r + eps_h, P_CONFUSION).map_or(r, |(x, _)| x);
            add(&mut out, polished);
        }
    }
    let nul = CurNul(d123);
    for w in breaks.windows(2) {
        for r in function_roots(&nul, w[0], w[1], 30, CUR_NUL_EPS, CUR_NUL_EPS, CUR_NUL_EPS)
            .unwrap_or_default()
        {
            add(&mut out, r);
        }
    }
    out
}

fn cross2(a: V2, b: V2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// A function and its derivative, for [`function_roots`] (OCCT's
/// `math_FunctionWithDerivative`).
trait RootFn {
    /// F(x), or None where it isn't defined.
    fn value(&self, x: f64) -> Option<f64>;
    /// F(x) and F'(x), or None where they aren't defined.
    fn values(&self, x: f64) -> Option<(f64, f64)>;
}

/// κ'·|D1| with its derivative by a difference of `eps / 100` (backwards past `last`): OCCT's
/// `Geom2dLProp_FuncCurExt`.
struct CurExt<'a, F: Fn(f64) -> Option<f64>> {
    f: &'a F,
    eps: f64,
    last: f64,
}

impl<F: Fn(f64) -> Option<f64>> RootFn for CurExt<'_, F> {
    fn value(&self, x: f64) -> Option<f64> {
        (self.f)(x)
    }
    fn values(&self, x: f64) -> Option<(f64, f64)> {
        let mut dx = self.eps / 100.0;
        if x + dx > self.last {
            dx = -dx;
        }
        let f = (self.f)(x).unwrap_or(0.0);
        let f2 = (self.f)(x + dx).unwrap_or(0.0);
        Some((f, (f2 - f) / dx))
    }
}

/// The sine between D1 and D2 and its derivative, from D1, D2, D3 (OCCT's
/// `Geom2dLProp_FuncCurNul`: zero where |D2| < 1e-4).
struct CurNul<F: Fn(f64) -> (V2, V2, V2)>(F);

impl<F: Fn(f64) -> (V2, V2, V2)> RootFn for CurNul<F> {
    fn value(&self, x: f64) -> Option<f64> {
        self.values(x).map(|v| v.0)
    }
    fn values(&self, x: f64) -> Option<(f64, f64)> {
        let (v1, v2, v3) = (self.0)(x);
        let (cp1, cp2) = (cross2(v1, v2), cross2(v1, v3));
        let (nv1, nv2) = (v1.norm(), v2.norm());
        if nv2 < CUR_NUL_D2 {
            Some((0.0, 0.0))
        } else if nv1 * nv2 < RESOLUTION {
            None
        } else {
            let f = cp1 / (nv1 * nv2);
            let d = (cp2 - cp1 * v1.dot(&v2) / (nv1 * nv1) - cp1 * v2.dot(&v3) / (nv2 * nv2))
                / (nv1 * nv2);
            Some((f, d))
        }
    }
}

/// The roots of `f` on [a, b] as OCCT's `math_FunctionRoots` (its "new code") finds them, so
/// spine cuts land where OCCT's do: `2·nsample` (at least 20) samples; Brent's method on every
/// strict sign change; samples exactly zero and ends within `eps_f` taken as roots; and where
/// the samples have a minimum of |f| that a step from a neighbour says may reach zero, the
/// minimum searched ([`refine_minimum`]) and taken when within `eps_f`. Roots closer than 1e-10
/// of the range's magnitude are one. None when `f` isn't defined at a sample; empty when every
/// sample lies within `eps_null` of zero.
fn function_roots<F: RootFn>(
    f: &F,
    a: f64,
    b: f64,
    nsample: usize,
    eps_x_in: f64,
    eps_f: f64,
    eps_null: f64,
) -> Option<Vec<f64>> {
    let (x0, xn) = if b < a { (b, a) } else { (a, b) };
    let n = (nsample * 2).max(20);
    let neps_x = ROOTS_SAME * (x0.abs() + xn.abs());
    let tol = eps_x_in.max(neps_x);
    let dx = (xn - x0) / n as f64;
    // (The samples step by dx, as OCCT's, rather than x0 + i·dx.)
    let mut vals = Vec::with_capacity(n + 1);
    let mut x = x0;
    for _ in 0..=n {
        if x > xn {
            x = xn;
        }
        vals.push(f.value(x)?);
        x += dx;
    }
    let mut sol = Vec::new();
    if vals.iter().all(|v| v.abs() <= eps_null) {
        return Some(sol);
    }
    // Strict sign changes between samples.
    let mut x = x0;
    for i in 0..n {
        let x2 = (x + dx).min(xn);
        if (vals[i] < 0.0 && vals[i + 1] > 0.0) || (vals[i] >= 0.0 && vals[i + 1] < 0.0) {
            brent(f, x, vals[i], x2, vals[i + 1], tol, neps_x, &mut sol);
        }
        x += dx;
    }
    // Samples where f is exactly zero but not half a step on. (OCCT looks at the point half a
    // step on twice, so never solves between two points there.)
    for (i, v) in vals.iter().enumerate() {
        if *v == 0.0 {
            let x = (x0 + i as f64 * dx).min(xn);
            let y = f.value((x + dx * 0.5).clamp(x0, xn)).unwrap_or(0.0);
            if y != 0.0 {
                append_root(&mut sol, x, neps_x);
            }
        }
    }
    if vals[0].abs() <= eps_f {
        append_root(&mut sol, x0, neps_x);
    }
    if vals[n].abs() <= eps_f {
        append_root(&mut sol, xn, neps_x);
    }
    // Minima of |f| over the samples that may reach zero between them.
    let majdx = 5.0 * dx;
    let near_zero = |x: f64| {
        let (y, d) = f.values(x).unwrap_or((0.0, 0.0));
        d.abs() > ROOTS_FLAT && (y / d).abs() < majdx
    };
    let mut xm = x0 + dx;
    for i in 1..n {
        if xm > xn {
            xm = xn;
        }
        let (fm, f0, f3) = (vals[i], vals[i - 1], vals[i + 1]);
        // (For a maximum below zero OCCT steps from the sample before only.)
        let rediscr = if fm > 0.0 && f0 > fm && f3 > fm {
            near_zero((xm - dx).max(x0)) || near_zero((xm + dx).min(xn))
        } else if fm < 0.0 && f0 < fm && f3 < fm {
            near_zero((xm - dx).max(x0))
        } else {
            false
        };
        if rediscr {
            refine_minimum(
                f,
                MinSearch {
                    xm,
                    dx,
                    lo: x0,
                    hi: xn,
                    f0,
                    f3,
                    eps_x: eps_x_in,
                    eps_f,
                    tol,
                    neps_x,
                },
                &mut sol,
            );
        }
        xm += dx;
    }
    Some(sol)
}

/// Where [`refine_minimum`] searches: about the sample `xm` (neighbours `dx` away, values `f0`
/// and `f3`, inside [lo, hi]), with [`function_roots`]' tolerances.
struct MinSearch {
    xm: f64,
    dx: f64,
    lo: f64,
    hi: f64,
    f0: f64,
    f3: f64,
    eps_x: f64,
    eps_f: f64,
    tol: f64,
    neps_x: f64,
}

/// The part of [`function_roots`] that looks for a root at a minimum of |f| near a sample: the
/// derivative's root by [`bracketed_root`] and a golden-section search (solving on any sign
/// change it meets), the one nearer zero kept when within `eps_f`.
fn refine_minimum<F: RootFn>(f: &F, s: MinSearch, sol: &mut Vec<f64>) {
    let MinSearch {
        xm,
        dx,
        lo,
        hi,
        mut f0,
        mut f3,
        eps_x,
        eps_f,
        tol,
        neps_x,
    } = s;
    let mut x0 = (xm - dx).max(lo);
    let mut x3 = (xm + dx).min(hi);
    let val = |x: f64| f.value(x).unwrap_or(0.0);
    let deriv = |x: f64| f.values(x).map(|v| v.1);
    // The derivative's root.
    let by_deriv = bracketed_root(&deriv, x0, x3, eps_x).and_then(|(x, d)| {
        let v = val(x).abs();
        (v < eps_f).then_some((x, v, d))
    });
    // The golden-section search.
    const R: f64 = 0.61803399;
    const C: f64 = 1.0 - R;
    let tol_cr = neps_x * 10.0;
    let minimum = f0 > 0.0;
    let (mut x1, mut x2) = if (x3 - xm).abs() > (x0 - xm).abs() {
        (xm, xm + C * (x3 - xm))
    } else {
        (xm - C * (xm - x0), xm)
    };
    let (mut f1, mut f2) = (val(x1), val(x2));
    let tol_x = 0.001 * neps_x;
    // (A cap OCCT doesn't have: the interval shrinks by 0.618 a step, so 2000 steps are far
    // more than a double needs.)
    let mut steps = 0;
    while (x3 - x0).abs() > tol_cr * (x1.abs() + x2.abs())
        && (x1 - x2).abs() > tol_x
        && steps < 2000
    {
        steps += 1;
        let right = if minimum { f2 < f1 } else { f2 > f1 };
        if right {
            x0 = x1;
            x1 = x2;
            x2 = R * x1 + C * x3;
            f0 = f1;
            f1 = f2;
            f2 = val(x2);
        } else {
            x3 = x2;
            x2 = x1;
            x1 = R * x2 + C * x0;
            f3 = f2;
            f2 = f1;
            f1 = val(x1);
        }
        if f1 * f0 < 0.0 {
            brent(f, x0, f0, x1, f1, tol, neps_x, sol);
        }
        if f2 * f3 < 0.0 {
            brent(f, x2, f2, x3, f3, tol, neps_x, sol);
        }
    }
    let by_search = if (minimum && f1 < f2) || (!minimum && f1 > f2) {
        (f1.abs() < eps_f).then_some((x1, f1.abs()))
    } else {
        (f2.abs() < eps_f).then_some((x2, f2.abs()))
    };
    let pick = match (by_deriv, by_search) {
        (Some((xa, va, da)), Some((xb, vb))) => {
            if vb - va > eps_f {
                xa
            } else if va - vb > eps_f || da.abs() >= deriv(xb).unwrap_or(0.0).abs() {
                xb
            } else {
                xa
            }
        }
        (Some((xa, ..)), None) => xa,
        (None, Some((xb, _))) => xb,
        (None, None) => return,
    };
    append_root(sol, pick, neps_x);
}

/// `x` added to the ordered roots unless one lies within `dx` of it (OCCT's `AppendRoot`: the
/// roots up to the first at or past `x` are compared).
fn append_root(sol: &mut Vec<f64>, x: f64, dx: f64) {
    let mut at = Some(sol.len());
    for (i, &t) in sol.iter().enumerate() {
        let near = (x - t).abs() <= dx;
        if near {
            at = None;
        } else if t >= x {
            at = Some(i);
        }
        if near || t >= x {
            break;
        }
    }
    if let Some(i) = at {
        sol.insert(i, x);
    }
}

/// Brent's method between `x1` and `x2` (values `y1`, `y2` of opposite signs) to `tol`, the root
/// polished by up to six Newton steps that stay inside and get nearer zero, then added by
/// [`append_root`] within `dx` (OCCT's `Solve` in `math_FunctionRoots`).
#[allow(clippy::too_many_arguments)]
fn brent<F: RootFn>(
    f: &F,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    tol: f64,
    dx: f64,
    sol: &mut Vec<f64>,
) {
    const ITMAX: usize = 100;
    const EPSEPS: f64 = 2e-14;
    let tols2 = 0.5 * tol;
    let (mut a, mut b, mut c) = (x1, x2, x2);
    let (mut fa, mut fb, mut fc) = (y1, y2, y2);
    let (mut d, mut e) = (0.0, 0.0);
    for _ in 0..ITMAX {
        if (fb > 0.0 && fc > 0.0) || (fb < 0.0 && fc < 0.0) {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol1 = EPSEPS * b.abs() + tols2;
        let xm = 0.5 * (c - b);
        if xm.abs() < tol1 || fb == 0.0 {
            let mut xp = b;
            for _ in 0..6 {
                let Some((yp, dp)) = f.values(xp) else { break };
                if dp.abs() > ROOTS_FLAT {
                    xp -= yp / dp;
                }
                if !(xp <= x2 && xp >= x1) {
                    break;
                }
                let yp = f.value(xp).unwrap_or(yp);
                if yp.abs() >= fb.abs() {
                    break;
                }
                b = xp;
                fb = yp;
            }
            append_root(sol, b, dx);
            return;
        }
        (d, e) = brent_step(a, b, c, fa, fb, fc, d, e, xm, tol1);
        a = b;
        fa = fb;
        if d.abs() > tol1 {
            b += d;
        } else {
            b += if xm >= 0.0 { tol1.abs() } else { -tol1.abs() };
        }
        fb = f.value(b).unwrap_or(fb);
    }
}

/// One step of Brent's method from the root `b` (other end `c`, last point `a`): the inverse
/// quadratic (or secant) step when it falls well inside, else bisection; the new (d, e).
#[allow(clippy::too_many_arguments)]
fn brent_step(
    a: f64,
    b: f64,
    c: f64,
    fa: f64,
    fb: f64,
    fc: f64,
    d: f64,
    e: f64,
    xm: f64,
    tol1: f64,
) -> (f64, f64) {
    if e.abs() < tol1 || fa.abs() <= fb.abs() {
        return (xm, xm);
    }
    let s = fb / fa;
    let (mut p, mut q);
    if a == c {
        p = 2.0 * xm * s;
        q = 1.0 - s;
    } else {
        q = fa / fc;
        let r = fb / fc;
        p = s * (2.0 * xm * q * (q - r) - (b - a) * (r - 1.0));
        q = (q - 1.0) * (r - 1.0) * (s - 1.0);
    }
    if p > 0.0 {
        q = -q;
    }
    p = p.abs();
    let min1 = 3.0 * xm * q - (tol1 * q).abs();
    let min2 = (e * q).abs();
    if 2.0 * p < min1.min(min2) {
        (p / q, d)
    } else {
        (xm, xm)
    }
}

/// The root of `g` between `b1` and `b2` (values of opposite signs) to `tolerance`, with `g`
/// there, by OCCT's `math_BracketedRoot` (Brent's method in at most 100 steps); None when the
/// ends' values have the same sign or it doesn't converge.
fn bracketed_root(
    g: &dyn Fn(f64) -> Option<f64>,
    b1: f64,
    b2: f64,
    tolerance: f64,
) -> Option<(f64, f64)> {
    const ZEPS: f64 = 1e-12;
    let mut a = b1;
    let mut root = b2;
    let mut fa = g(a).unwrap_or(0.0);
    let mut err = g(root).unwrap_or(0.0);
    if fa * err > 0.0 {
        return None;
    }
    let mut fc = err;
    let (mut c, mut d, mut e) = (0.0, 0.0, 0.0);
    for _ in 0..100 {
        if err * fc > 0.0 {
            c = a;
            fc = fa;
            d = root - a;
            e = d;
        }
        // (OCCT compares the far end with the last point here, not with the root.)
        if fc.abs() < fa.abs() {
            a = root;
            root = c;
            c = a;
            fa = err;
            err = fc;
            fc = fa;
        }
        let tol1 = 2.0 * ZEPS * root.abs() + 0.5 * tolerance;
        let xm = 0.5 * (c - root);
        if xm.abs() <= tol1 || err == 0.0 {
            return Some((root, err));
        }
        // (The same step as Brent's in `brent`, with the root's value `err`.)
        (d, e) = brent_step(a, root, c, fa, err, fc, d, e, xm, tol1);
        a = root;
        fa = err;
        if d.abs() > tol1 {
            root += d;
        } else {
            root += if xm > 0.0 { tol1.abs() } else { -tol1.abs() };
        }
        err = g(root).unwrap_or(err);
    }
    None
}

/// `e` cut at `ts` as OCCT's `MAT2d_CutCurve` does: in order, skipping cuts within 1e-8 in
/// parameter or 1e-6 in space of the last cut or of the end. Empty when nothing is cut.
fn cut_at(e: &Piece2, ts: &[f64]) -> Vec<Piece2> {
    let (ptol, tol) = (1e-8, tol::POINT);
    let (mut uf, ul) = (e.t0, e.t1);
    let (mut pf, pl) = (e.start(), e.end());
    let mut out = Vec::new();
    for &uc in ts {
        let pc = e.c.point(uc);
        if uc - uf > ptol && (pc - pf).norm() > tol {
            if ul - uc < ptol || (pl - pc).norm() < tol {
                continue;
            }
            out.push(Piece2 {
                c: e.c.clone(),
                t0: uf,
                t1: uc,
            });
            uf = uc;
            pf = pc;
        }
    }
    if !out.is_empty() {
        out.push(Piece2 {
            c: e.c.clone(),
            t0: uf,
            t1: ul,
        });
    }
    out
}

/// `e` cut into `n` pieces of equal parameter span (OCCT's `CutCurve`).
fn cut_even(e: &Piece2, n: usize) -> Vec<Piece2> {
    let ts: Vec<f64> = (1..n)
        .map(|i| e.t0 + (e.t1 - e.t0) * i as f64 / n as f64)
        .collect();
    cut_at(e, &ts)
}

/// A piece of the spine: `p`, the source curve run from one parameter to the other (either way).
#[derive(Debug, Clone)]
struct SpineEdge {
    p: Piece2,
}

impl SpineEdge {
    /// The edge (c run from `a` to `b`, either way round) on [lo, hi].
    fn new(c: &Curve, a: f64, b: f64, lo: f64, hi: f64) -> Self {
        let p = if a <= b {
            forward(c, lo, hi)
        } else {
            forward(c, hi, lo)
        };
        SpineEdge { p }
    }

    fn reversed(&self) -> Self {
        let (c, a, b) = self.p.c.reversed(self.p.t0, self.p.t1);
        SpineEdge {
            p: Piece2 { c, t0: a, t1: b },
        }
    }
}

/// The wire's edges (in the wire's order, each run from its first to its second parameter) cut
/// where their curvature has extrema or changes sign, as OCCT prepares the spine of a planar
/// offset (`BRepFill_OffsetWire::PrepareSpine`, `CutEdge`): every piece offsets to an edge of
/// its own. Each edge is cut in its own parameter direction. A wire of one edge has it cut in
/// three at least (a closed edge otherwise has no vertex to start from), and of two edges the
/// second in two when the first stays whole; an arc of a circle of half a turn or more isn't
/// cut when the wire has other edges.
fn prepare_spine(wire: &[(Curve, f64, f64)]) -> Vec<SpineEdge> {
    let n = wire.len();
    let mut out = Vec::new();
    let mut first_cut = 0;
    for (i, (c, a, b)) in wire.iter().enumerate() {
        let (lo, hi) = (a.min(*b), a.max(*b));
        let e = Piece2 {
            c: c.clone(),
            t0: lo,
            t1: hi,
        };
        let half_circle = n > 1 && matches!(c, Curve::Circle { .. }) && hi - lo >= PI;
        let cuts = if half_circle {
            Vec::new()
        } else {
            cut_at(&e, &curvature_points(c, lo, hi))
        };
        let force = if half_circle {
            0
        } else if n == 1 {
            2
        } else if n == 2 && i == 1 && first_cut == 0 {
            1
        } else {
            0
        };
        let pieces = match force {
            1 if cuts.is_empty() => cut_even(&e, 2),
            2 if cuts.is_empty() => cut_even(&e, 3),
            2 if cuts.len() == 2 => {
                let mid = 0.5 * (e.t0 + e.t1);
                let mut v = cuts.clone();
                let k = if v[0].t1 > mid { 0 } else { 1 };
                let half = cut_even(&v[k], 2);
                if !half.is_empty() {
                    v.splice(k..=k, half);
                }
                v
            }
            _ => cuts,
        };
        if i == 0 {
            first_cut = pieces.len();
        }
        let pieces = if pieces.is_empty() { vec![e] } else { pieces };
        let mut edges: Vec<SpineEdge> = pieces
            .iter()
            .map(|p| SpineEdge::new(c, *a, *b, p.t0, p.t1))
            .collect();
        if a > b {
            edges.reverse();
        }
        out.extend(edges);
    }
    out
}

/// The spine's curved pieces cut where their radius of curvature on `side` (+1 their left, −1
/// their right) equals `dist`, and where a B-spline's curvature may jump (a knot of multiplicity
/// degree − 1 or more): so each piece's offset by `dist` is smooth, its cusps at its ends (the
/// stretch curving more tightly than the distance offsets backwards, into the swallowtail the
/// trimming removes). Lines and circles curve evenly and stay whole.
fn cut_at_cusps(spine: Vec<Piece2>, side: f64, dist: f64) -> Vec<Piece2> {
    let mut out = Vec::with_capacity(spine.len());
    for e in spine {
        if matches!(e.c, Curve::Line { .. } | Curve::Circle { .. }) {
            out.push(e);
            continue;
        }
        let mut roots = Vec::new();
        if let Curve::Nurbs(c) = &e.c {
            let eps = 1e-9 * (e.t1 - e.t0);
            let mut i = c.degree + 1;
            while i < c.cps.len() {
                let k = c.knots[i];
                let mult = c.knots[i..].iter().take_while(|&&x| x == k).count();
                if mult + 1 >= c.degree && k > e.t0 + eps && k < e.t1 - eps {
                    roots.push(k);
                }
                i += mult;
            }
        }
        // Curvature towards the side less 1/dist: positive where the offset folds back.
        let f = |t: f64| {
            let (_, d1, d2) = e.c.d2(t);
            let v = V2::new(d1.x, d1.y);
            let sp = v.norm().max(tol::TINY);
            cross2(v, V2::new(d2.x, d2.y)) / (sp * sp * sp) * side - 1.0 / dist
        };
        let n = match &e.c {
            Curve::Nurbs(c) => (16 * c.cps.len()).max(128),
            _ => 128,
        };
        let ts: Vec<f64> = (0..=n)
            .map(|i| e.t0 + (e.t1 - e.t0) * i as f64 / n as f64)
            .collect();
        let vals: Vec<f64> = ts.iter().map(|&t| f(t)).collect();
        for i in 0..n {
            if (vals[i] > 0.0) == (vals[i + 1] > 0.0) {
                continue;
            }
            let (mut lo, mut hi, flo) = (ts[i], ts[i + 1], vals[i]);
            for _ in 0..200 {
                let mid = 0.5 * (lo + hi);
                if mid <= lo || mid >= hi {
                    break;
                }
                if (f(mid) > 0.0) == (flo > 0.0) {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            roots.push(0.5 * (lo + hi));
        }
        roots.sort_by(f64::total_cmp);
        let cut = cut_at(&e, &roots);
        if cut.is_empty() {
            out.push(e);
        } else {
            out.extend(cut);
        }
    }
    out
}

/// The loops of the wire `edges` (in the plane z = 0, chained in order) offset by `d`: a closed
/// wire outwards for d > 0, an open one on both sides with round ends.
pub fn offset_wire(edges: &[(Curve, f64, f64)], d: f64) -> Result<Vec<Vec<Piece2>>> {
    if edges.is_empty() || d == 0.0 {
        return Err(KernelError::InvalidParameter(
            "an offset needs edges and a distance".into(),
        ));
    }
    let clock = web_time::Instant::now();
    let flat: Vec<(Curve, f64, f64)> = edges.iter().map(|(c, a, b)| flattened(c, *a, *b)).collect();
    let mut bb = Bbox::empty();
    // Each edge's length in the plane (its chords): an edge square to the plane (a wire not in
    // z = 0) flattens to a point, which has no direction to offset along.
    let mut lens = Vec::with_capacity(flat.len());
    for (c, a, b) in &flat {
        let e = forward(c, *a, *b);
        let pts: Vec<P3> = (0..=8)
            .map(|k| e.c.point(e.t0 + (e.t1 - e.t0) * k as f64 / 8.0))
            .collect();
        if !(e.t0.is_finite() && e.t1.is_finite())
            || pts.iter().any(|p| !(p.x.is_finite() && p.y.is_finite()))
        {
            return Err(KernelError::InvalidParameter(
                "an offset of a wire with an edge of no length in its plane".into(),
            ));
        }
        lens.push(pts.windows(2).map(|w| (w[1] - w[0]).norm()).sum::<f64>());
        pts.iter().for_each(|p| bb.add(p));
    }
    if lens.iter().any(|l| *l <= bb.diagonal() * tol::REL_FINE) {
        return Err(KernelError::InvalidParameter(
            "an offset of a wire with an edge of no length in its plane".into(),
        ));
    }
    let size = bb.diagonal().max(d.abs());
    let tol = size * 1e-7;
    // (Edges however short are kept: OCCT offsets them to edges of their own.)
    let spine = chained(prepare_spine(&flat));
    // The wire's own gaps between edges (vertex tolerances) set the closure and chaining.
    let gap = spine
        .windows(2)
        .map(|w| (w[0].p.end() - w[1].p.start()).norm())
        .fold(0.0, f64::max);
    let (first, last) = (spine[0].p.start(), spine[spine.len() - 1].p.end());
    let closing = (first - last).norm();
    let closed = closing < (tol * 10.0).max(size * 1e-4).max(gap * 3.0);
    // The side to offset to (to the left by l): outwards, the right of a counter-clockwise
    // closed wire; an open one is run out and back.
    let l = if closed {
        let mut area = 0.0;
        for e in spine.iter().map(|s| &s.p) {
            for k in 0..32 {
                let (a, b) = (
                    e.c.point(e.t0 + (e.t1 - e.t0) * k as f64 / 32.0),
                    e.c.point(e.t0 + (e.t1 - e.t0) * (k + 1) as f64 / 32.0),
                );
                area += a.x * b.y - b.x * a.y;
            }
        }
        if area >= 0.0 { -d } else { d }
    } else {
        d.abs()
    };
    // The side offset to (an open wire is offset to both, out and back).
    let sides: &[f64] = if closed { &[l.signum()] } else { &[1.0, -1.0] };
    let mut wire: Vec<Piece2> = spine.into_iter().map(|s| s.p).collect();
    for &side in sides {
        wire = cut_at_cusps(wire, side, d.abs());
    }
    if std::env::var_os("CADRS_OFFSET_DEBUG").is_some() {
        eprintln!("spine: {} pieces", wire.len());
    }
    let seq: Vec<Piece2> = if closed {
        wire.clone()
    } else {
        let mut s = wire.clone();
        s.extend(wire.iter().rev().map(|e| {
            let (c, a, b) = e.c.reversed(e.t0, e.t1);
            Piece2 { c, t0: a, t1: b }
        }));
        s
    };
    let chain_tol = (size * tol::REL_POINT)
        .max(l.abs() * tol::REL_POINT)
        .max(gap * 3.0)
        .max(if closed { closing * 3.0 } else { 0.0 });
    let n = seq.len();
    let tick = |what: &str| {
        if std::env::var_os("CADRS_OFFSET_DEBUG").is_some() {
            eprintln!("  {what}: {:?}", clock.elapsed());
        }
    };
    let mut raw: Vec<Raw> = Vec::new();
    // Each edge's offset in `raw` (none for an arc that shrinks away), and whether the corner
    // after it is concave.
    let mut edge_raw: Vec<Option<usize>> = Vec::with_capacity(n);
    let mut concave_after = vec![false; n];
    // The arcs at concave corners too slight to tell from the wire's gaps: the corner's index,
    // the arc's in `raw`, and the corner.
    let mut slight_concave: Vec<(usize, usize, P3)> = Vec::new();
    for i in 0..n {
        let e = &seq[i];
        edge_raw.push(offset_edge(e, l, size).map(|(o, err)| {
            raw.push(Raw { p: o, err });
            raw.len() - 1
        }));
        // The join to the next edge.
        let next = &seq[(i + 1) % n];
        let (t1, t2) = (e.dir(e.t1), next.dir(next.t0));
        let mut phi = (t1.x * t2.y - t1.y * t2.x).atan2(t1.dot(&t2));
        // Tangent: the offsets meet without an arc. A convex corner gets one however small its
        // offset (as OCCT's, from an angle of 1e-7 up); a concave one within the wire's gaps
        // closes up in the chaining.
        let convex = phi * l < 0.0;
        // (A corner the wire's gaps could account for closes up in the chaining.)
        concave_after[i] = !convex && (l * phi).abs() > 10.0 * chain_tol;
        if phi.abs() < 1e-7 || (!convex && (l * phi).abs() <= chain_tol) {
            continue;
        }
        if phi.abs() > PI - tol::ANGULAR {
            phi = -PI * l.signum();
        }
        if !convex && !concave_after[i] {
            slight_concave.push((i, raw.len(), e.end()));
        }
        raw.push(Raw {
            p: join_arc(e.end(), left(t1), phi, l),
            err: 0.0,
        });
    }
    tick("raw");
    // Where the raw loop crosses itself.
    let polys: Vec<Vec<f64>> = raw
        .iter()
        .map(|r| {
            let k = match &r.p.c {
                Curve::Line { .. } => 1,
                Curve::Circle { .. } | Curve::Ellipse { .. } => {
                    ((r.p.t1 - r.p.t0).abs() / 0.05).ceil() as usize
                }
                Curve::Nurbs(n) => (8 * n.cps.len()).max(64),
            }
            .clamp(1, 4096);
            let mut ts: Vec<f64> = (0..=k)
                .map(|i| r.p.t0 + (r.p.t1 - r.p.t0) * i as f64 / k as f64)
                .collect();
            if ts.len() < 2 {
                ts = vec![r.p.t0, r.p.t1];
            }
            ts
        })
        .collect();
    struct Seg {
        el: usize,
        k: usize,
        a: V2,
        b: V2,
        lo: f64,
        hi: f64,
    }
    let mut segs: Vec<Seg> = Vec::new();
    for (el, ts) in polys.iter().enumerate() {
        for k in 0..ts.len() - 1 {
            let (a, b) = (
                p2(&raw[el].p.c.point(ts[k])),
                p2(&raw[el].p.c.point(ts[k + 1])),
            );
            segs.push(Seg {
                el,
                k,
                a,
                b,
                lo: a.x.min(b.x),
                hi: a.x.max(b.x),
            });
        }
    }
    tick(&format!("{} segments", segs.len()));
    segs.sort_by(|a, b| a.lo.total_cmp(&b.lo));
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); raw.len()];
    // The pairs of elements found crossing, at what parameters, and where.
    let mut crossed: Vec<(usize, f64, usize, f64, P3)> = Vec::new();
    let near_end = |el: usize, t: f64| {
        let p = &raw[el].p;
        let span = (p.t1 - p.t0).abs().max(tol::TINY);
        (t - p.t0).abs() < tol::REL * span || (t - p.t1).abs() < tol::REL * span
    };
    let slack = tol * 10.0;
    for i in 0..segs.len() {
        let s = &segs[i];
        for o in &segs[i + 1..] {
            if o.lo > s.hi + slack {
                break;
            }
            if s.a.y.max(s.b.y) + slack < o.a.y.min(o.b.y)
                || o.a.y.max(o.b.y) + slack < s.a.y.min(s.b.y)
            {
                continue;
            }
            // Neighbouring segments of one curve meet at their shared point.
            if s.el == o.el && (s.k as i64 - o.k as i64).abs() <= 1 {
                continue;
            }
            let Some((u, v)) = seg_cross(&s.a, &s.b, &o.a, &o.b, slack) else {
                continue;
            };
            let (ts, to) = (&polys[s.el], &polys[o.el]);
            let ta = ts[s.k] + (ts[s.k + 1] - ts[s.k]) * u;
            let tb = to[o.k] + (to[o.k + 1] - to[o.k]) * v;
            let Some((ta, tb)) = refine(&raw[s.el].p, &raw[o.el].p, ta, tb, tol) else {
                continue;
            };
            // A curve meets itself only at two different parameters.
            if s.el == o.el {
                let p = &raw[s.el].p;
                let (a, b) = (p.c.point(ta.min(tb)), p.c.point(0.5 * (ta + tb)));
                if (ta - tb).abs() < tol::REL * (p.t1 - p.t0).abs() || (a - b).norm() < tol * 10.0 {
                    continue;
                }
            }
            let (ea, eb) = (near_end(s.el, ta), near_end(o.el, tb));
            if ea && eb {
                continue;
            }
            crossed.push((s.el, ta, o.el, tb, raw[s.el].p.c.point(ta)));
            if !ea {
                cuts[s.el].push(ta);
            }
            if !eb {
                cuts[o.el].push(tb);
            }
        }
    }
    // The offsets either side of a concave corner cross near it, often at so slight an angle
    // (a polyline's chord after an arc, say) that the polygons above miss it: Newton from the
    // corner.
    for i in 0..n {
        let (Some(a), Some(b)) = (edge_raw[i], edge_raw[(i + 1) % n]) else {
            continue;
        };
        if !concave_after[i] || a == b {
            continue;
        }
        let (pa, pb) = (&raw[a].p, &raw[b].p);
        if let Some((ta, tb)) = refine(pa, pb, pa.t1, pb.t0, tol) {
            let (ea, eb) = (near_end(a, ta), near_end(b, tb));
            if !(ea && eb) {
                crossed.push((a, ta, b, tb, pa.c.point(ta)));
            }
            if !ea {
                cuts[a].push(ta);
            }
            if !eb {
                cuts[b].push(tb);
            }
        }
    }
    // At a concave corner so slight that the keep test below can't tell its pieces apart
    // (within the wire's gaps of the distance), where the offsets either side cross near the
    // corner, the arc is left out and the offsets end at the crossing: the arc only bridges
    // offsets that don't cross.
    let mut dropped: Vec<usize> = Vec::new();
    // Per element, the parameter it ends at and the one it starts from.
    let mut trim: Vec<(f64, f64)> = raw.iter().map(|r| (r.p.t1, r.p.t0)).collect();
    for (i, k, v) in &slight_concave {
        let (Some(a), Some(b)) = (edge_raw[*i], edge_raw[(i + 1) % n]) else {
            continue;
        };
        let near = crossed
            .iter()
            .filter(|c| (c.4 - v).norm() < 2.0 * l.abs())
            .find_map(|&(x, tx, y, ty, _)| match (x, y) {
                _ if x == a && y == b => Some((tx, ty)),
                _ if x == b && y == a => Some((ty, tx)),
                _ => None,
            });
        if let Some((ta, tb)) = near {
            dropped.push(*k);
            trim[a].0 = trim[a].0.min(ta);
            trim[b].1 = trim[b].1.max(tb);
        }
    }
    tick("crossings");
    // The pieces between cuts, kept where they stay the distance from the wire.
    // (Less the wire's gaps: an arc about one edge's end lies that much nearer the next.)
    // (Within 1e-9 of the distance: a swallowtail of an offset curve lies very little nearer.)
    let keep_at = l.abs() * (1.0 - tol::REL)
        - (tol * 0.01)
            .max(gap * 2.0)
            .max(if closed { closing * 2.0 } else { 0.0 });
    // Boxes round the wire's edges (a B-spline's control points, else samples with a margin)
    // rule most edges out without a closest-point search.
    let boxes: Vec<Bbox> = wire.iter().map(edge_box).collect();
    let far = |p: &P3, err: f64| -> bool {
        let keep_at = keep_at - 2.0 * err;
        wire.iter().zip(&boxes).all(|(e, b)| {
            let dx = (b.min.x - p.x).max(p.x - b.max.x).max(0.0);
            let dy = (b.min.y - p.y).max(p.y - b.max.y).max(0.0);
            if dx * dx + dy * dy >= keep_at * keep_at {
                return true;
            }
            let q = e.c.point(e.c.closest(p, e.t0, e.t1));
            ((q.x - p.x).powi(2) + (q.y - p.y).powi(2)).sqrt() >= keep_at
        })
    };
    let mut kept: Vec<(usize, Piece2)> = Vec::new();
    for (el, r) in raw.iter().enumerate() {
        if dropped.contains(&el) {
            continue;
        }
        let (t_end, t_start) = trim[el];
        let mut ts = cuts[el].clone();
        ts.push(r.p.t0);
        ts.push(r.p.t1);
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() < tol::REL_FINE * (1.0 + b.abs()));
        // Cuts closer together than the tolerance are one (the ends stay). Not the two cuts of
        // a self-crossing, which meet at one point with a loop between (a swallowtail).
        let mut merged: Vec<f64> = Vec::new();
        let near = |a: f64, b: f64| (r.p.c.point(a) - r.p.c.point(b)).norm() < tol * 10.0;
        for (i, &t) in ts.iter().enumerate() {
            let last = i + 1 == ts.len();
            match merged.last() {
                Some(&m)
                    if near(t, m) && [0.25, 0.5, 0.75].iter().all(|f| near(m + (t - m) * f, m)) =>
                {
                    if last {
                        *merged.last_mut().unwrap() = t;
                    }
                }
                _ => merged.push(t),
            }
        }
        if merged.len() < 2 {
            merged = vec![r.p.t0, r.p.t1];
        }
        // A trim falls exactly on a boundary (it ends the offset where the next begins): the
        // cut merged nearest it moves there.
        for t in [t_end, t_start] {
            if t > r.p.t0 && t < r.p.t1 {
                let k = (1..merged.len() - 1)
                    .min_by(|&i, &j| (merged[i] - t).abs().total_cmp(&(merged[j] - t).abs()));
                match k {
                    Some(k) if near(merged[k], t) => merged[k] = t,
                    _ => {
                        let at = merged.partition_point(|&x| x < t);
                        merged.insert(at, t);
                    }
                }
            }
        }
        let ts = merged;
        let mut run: Option<Piece2> = None;
        for w in ts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if b - a < 1e-14 * (1.0 + a.abs()) {
                continue;
            }
            // (By the piece's middle: a trim may have merged with a cut close by.)
            let mid = 0.5 * (a + b);
            let inside = mid >= t_start && mid <= t_end;
            let good = inside
                && [0.25, 0.5, 0.75]
                    .iter()
                    .all(|f| far(&r.p.c.point(a + (b - a) * f), r.err));
            match (&mut run, good) {
                (Some(p), true) => p.t1 = b,
                (None, true) => {
                    run = Some(Piece2 {
                        c: r.p.c.clone(),
                        t0: a,
                        t1: b,
                    })
                }
                (Some(_), false) => kept.push((el, run.take().unwrap())),
                (None, false) => {}
            }
        }
        if let Some(p) = run {
            kept.push((el, p));
        }
    }
    // Slivers (bits of an element between near cuts) close up in the chaining; a whole element
    // stays however short (the offset of a short edge, or of a corner).
    let sliver = chain_tol.min(tol::POINT);
    kept.retain(|(el, p)| {
        (p.t0 == raw[*el].p.t0 && p.t1 == raw[*el].p.t1)
            || (p.end() - p.start()).norm() > sliver
            || polyline_len(p, 16) > sliver * 2.0
    });
    tick("kept");
    if std::env::var_os("CADRS_OFFSET_DEBUG").is_some() {
        eprintln!(
            "offset l {l} closed {closed} size {size}: {} raw, {} cuts, {} kept",
            raw.len(),
            cuts.iter().map(Vec::len).sum::<usize>(),
            kept.len()
        );
        for (i, r) in raw.iter().enumerate() {
            eprintln!(
                "  raw {i}: {:?} -> {:?} cuts {:?}",
                p2(&r.p.start()),
                p2(&r.p.end()),
                cuts[i]
            );
        }
        for (el, p) in &kept {
            eprintln!("  kept {el}: {:?} -> {:?}", p2(&p.start()), p2(&p.end()));
        }
    }
    // Chained into loops (pieces that don't close up are dropped).
    let mut used = vec![false; kept.len()];
    let mut loops: Vec<Vec<Piece2>> = Vec::new();
    for s0 in 0..kept.len() {
        if used[s0] {
            continue;
        }
        let mut lp = vec![s0];
        used[s0] = true;
        let first = kept[s0].1.start();
        let ok;
        loop {
            let end = kept[*lp.last().unwrap()].1.end();
            if lp.len() > 1 && (end - first).norm() < chain_tol {
                ok = true;
                break;
            }
            let next = (0..kept.len())
                .filter(|&j| !used[j])
                .map(|j| (j, (kept[j].1.start() - end).norm()))
                .filter(|(_, dd)| *dd < chain_tol)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            match next {
                Some((j, _)) => {
                    used[j] = true;
                    lp.push(j);
                }
                None => {
                    ok = lp.len() == 1 && (end - first).norm() < chain_tol;
                    break;
                }
            }
        }
        // (A sliver closing on itself is a leftover of a corner, not a loop.)
        if ok
            && lp
                .iter()
                .map(|&j| polyline_len(&kept[j].1, 16))
                .sum::<f64>()
                > chain_tol * 10.0
        {
            // A piece split where its own curve's loop closes is one edge again.
            let mut pieces: Vec<Piece2> = lp.iter().map(|&j| kept[j].1.clone()).collect();
            let els: Vec<usize> = lp.iter().map(|&j| kept[j].0).collect();
            if pieces.len() > 1
                && els[0] == *els.last().unwrap()
                && (pieces.last().unwrap().t1 - pieces[0].t0).abs() > 0.0
            {
                let (a, b) = (&pieces[pieces.len() - 1], &pieces[0]);
                if (a.t1 - b.t0).abs() < tol::REL_FINE * (1.0 + a.t1.abs()) {
                    let merged = Piece2 {
                        c: a.c.clone(),
                        t0: a.t0,
                        t1: b.t1,
                    };
                    pieces.pop();
                    pieces[0] = merged;
                }
            }
            loops.push(pieces);
        }
    }
    tick("loops");
    Ok(loops)
}

/// The length of the polyline through `n + 1` points of the piece (a lower bound).
fn polyline_len(p: &Piece2, n: usize) -> f64 {
    (0..n)
        .map(|i| {
            (p.c.point(p.t0 + (p.t1 - p.t0) * (i + 1) as f64 / n as f64)
                - p.c.point(p.t0 + (p.t1 - p.t0) * i as f64 / n as f64))
            .norm()
        })
        .sum()
}

/// A box holding the piece: a B-spline's control points (a valid hull with positive weights),
/// else samples grown by the sag between them.
fn edge_box(e: &Piece2) -> Bbox {
    let mut b = Bbox::empty();
    if let Curve::Nurbs(n) = &e.c
        && n.cps.iter().all(|h| h.w > 0.0)
    {
        for h in &n.cps {
            b.add(&crate::native::nurbs::dehom(h));
        }
        return b;
    }
    let k = 32;
    for i in 0..=k {
        b.add(&e.c.point(e.t0 + (e.t1 - e.t0) * i as f64 / k as f64));
    }
    b.grow(b.diagonal() * 0.01 + tol::ROUNDOFF)
}

/// The edges in the order and direction they chain in (each next the one whose end is nearest
/// the last end), from the first edge; an open wire from one of its free ends.
fn chained(mut rest: Vec<SpineEdge>) -> Vec<SpineEdge> {
    // Start at an end no other edge reaches (an open wire's), else at the first edge.
    let free = |p: P3, skip: usize, all: &[SpineEdge]| {
        all.iter()
            .enumerate()
            .filter(|(j, _)| *j != skip)
            .map(|(_, e)| (e.p.start() - p).norm().min((e.p.end() - p).norm()))
            .fold(f64::INFINITY, f64::min)
    };
    let mut start = (0, false, 0.0);
    for (i, e) in rest.iter().enumerate() {
        for (flip, p) in [(false, e.p.start()), (true, e.p.end())] {
            let d = free(p, i, &rest);
            if d > start.2 * 10.0 + tol::ROUNDOFF_FINE && d > tol::ROUNDOFF {
                start = (i, flip, d);
            }
        }
    }
    let lens: f64 = rest.iter().map(|e| (e.p.end() - e.p.start()).norm()).sum();
    let mut out = Vec::new();
    let first = rest.remove(start.0);
    out.push(if start.1 && start.2 > lens * 1e-4 {
        first.reversed()
    } else {
        first
    });
    while let Some(end) = out.last().map(|e: &SpineEdge| e.p.end()) {
        let Some((j, flip, _)) = rest
            .iter()
            .enumerate()
            .flat_map(|(j, e)| {
                [
                    (j, false, (e.p.start() - end).norm()),
                    (j, true, (e.p.end() - end).norm()),
                ]
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
        else {
            break;
        };
        let e = rest.remove(j);
        out.push(if flip { e.reversed() } else { e });
    }
    out
}

/// Where segments ab and cd cross: their parameters, with `slack` at the ends.
fn seg_cross(a: &V2, b: &V2, c: &V2, d: &V2, slack: f64) -> Option<(f64, f64)> {
    let (r, s) = (b - a, d - c);
    let den = r.x * s.y - r.y * s.x;
    let (lr, ls) = (r.norm(), s.norm());
    if den.abs() <= 1e-14 * lr * ls {
        return None;
    }
    let q = c - a;
    let u = (q.x * s.y - q.y * s.x) / den;
    let v = (q.x * r.y - q.y * r.x) / den;
    let (eu, ev) = (slack / lr.max(tol::TINY), slack / ls.max(tol::TINY));
    if u < -eu || u > 1.0 + eu || v < -ev || v > 1.0 + ev {
        return None;
    }
    Some((u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)))
}

/// Newton on a(t) = b(u) from a guess, kept inside both ranges.
fn refine(a: &Piece2, b: &Piece2, mut t: f64, mut u: f64, tol: f64) -> Option<(f64, f64)> {
    for _ in 0..30 {
        let (pa, da, _) = a.c.d2(t);
        let (pb, db, _) = b.c.d2(u);
        let f = V2::new(pa.x - pb.x, pa.y - pb.y);
        if f.norm() < tol * 1e-3 {
            break;
        }
        let (j00, j01, j10, j11) = (da.x, -db.x, da.y, -db.y);
        let Some((dt, du)) = solve2(j00, j01, j10, j11, -f.x, -f.y) else {
            break;
        };
        t = (t + dt).clamp(a.t0, a.t1);
        u = (u + du).clamp(b.t0, b.t1);
    }
    let (pa, pb) = (a.c.point(t), b.c.point(u));
    if ((pa.x - pb.x).powi(2) + (pa.y - pb.y).powi(2)).sqrt() < tol * 10.0 {
        Some((t, u))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total_len(loops: &[Vec<Piece2>]) -> f64 {
        loops.iter().flatten().map(|p| p.c.length(p.t0, p.t1)).sum()
    }

    #[test]
    fn ellipse_cut_at_its_vertices() {
        let c = Curve::Ellipse {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            a: 3.0,
            b: 1.0,
        };
        let ts = curvature_points(&c, 0.0, 2.0 * PI);
        assert_eq!(ts.len(), 5);
        let spine = prepare_spine(&[(c, 0.0, 2.0 * PI)]);
        assert_eq!(spine.len(), 4);
    }

    #[test]
    fn single_closed_circle_cut_in_three() {
        // OCCT's ForcedCut: a closed wire of one uncut edge is cut in three.
        let c = Curve::Circle {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            r: 2.0,
        };
        let loops = offset_wire(&[(c, 0.0, 2.0 * PI)], 0.5).unwrap();
        assert_eq!(loops.len(), 1);
        assert_eq!(loops[0].len(), 3);
        assert!((total_len(&loops) - 2.0 * PI * 2.5).abs() < 1e-9);
    }

    #[test]
    fn b_spline_cut_at_curvature_extremum() {
        // A symmetric cubic arch: curvature peaks at its middle.
        let n = NurbsCurve::bezier(
            &[
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 2.0, 0.0),
                P3::new(3.0, 2.0, 0.0),
                P3::new(4.0, 0.0, 0.0),
            ],
            &[1.0; 4],
        );
        let ts = curvature_points(&Curve::nurbs(n), 0.0, 1.0);
        assert!(ts.iter().any(|t| (t - 0.5).abs() < 1e-6), "{ts:?}");
    }

    /// The quadratic B-spline through (0,0) … (6,1) with simple knots at 1 and 2.
    fn quadratic() -> NurbsCurve {
        let poles = [(0.0, 0.0), (1.0, 2.0), (3.0, 2.0), (4.0, 0.0), (6.0, 1.0)];
        let cps = poles
            .iter()
            .map(|&(x, y)| crate::native::nurbs::hom(&P3::new(x, y, 0.0), 1.0))
            .collect();
        NurbsCurve::new(2, vec![0.0, 0.0, 0.0, 1.0, 2.0, 3.0, 3.0, 3.0], cps).unwrap()
    }

    #[test]
    fn b_spline_cut_where_occt_cuts() {
        // OCCT 7.8's Geom2dLProp_CurAndInf2d on this curve: extrema of curvature in each span,
        // and a root of κ' where it jumps at the knot 1 (sampled on the next span there).
        let ts = curvature_points(&Curve::nurbs(quadratic()), 0.0, 3.0);
        let occt = [0.999999999365578, 1.4000000005028, 2.1999999994956];
        assert_eq!(ts.len(), occt.len(), "{ts:?}");
        for (t, o) in ts.iter().zip(occt) {
            assert!((t - o).abs() < 1e-8, "{ts:?}");
        }
    }

    #[test]
    fn edge_cut_where_it_curves_as_tightly_as_the_distance() {
        // The ellipse x²/25 + y²/9 = 1 (counter-clockwise, its centre to the left) curves more
        // tightly than 2.5 about the ends of its long axis: its radius of curvature
        // (25 sin²t + 9 cos²t)^1.5 / 15 equals 2.5 at sin²t = (37.5^(2/3) − 9) / 16, four
        // times round. Offset to its right, or by less than 1.8, it isn't cut.
        let c = Curve::Ellipse {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            a: 5.0,
            b: 3.0,
        };
        let e = forward(&c, 0.0, 2.0 * PI);
        let cut = cut_at_cusps(vec![e.clone()], 1.0, 2.5);
        assert_eq!(cut.len(), 5);
        let t = ((37.5f64.powf(2.0 / 3.0) - 9.0) / 16.0).sqrt().asin();
        for (p, want) in cut.iter().zip([t, PI - t, PI + t, 2.0 * PI - t]) {
            assert!((p.t1 - want).abs() < 1e-12, "{} {want}", p.t1);
        }
        assert_eq!(cut_at_cusps(vec![e.clone()], -1.0, 2.5).len(), 1);
        assert_eq!(cut_at_cusps(vec![e], 1.0, 1.0).len(), 1);
    }

    #[test]
    fn ellipse_shrunk_past_its_curvature_meets_itself_on_its_long_axis() {
        // Inwards by 2.5 the offset of the ellipse x²/25 + y²/9 = 1 folds in swallowtails about
        // the ends of its long axis; trimmed, it meets itself there in two corners, and every
        // point of it lies 2.5 from the ellipse.
        let c = Curve::Ellipse {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            a: 5.0,
            b: 3.0,
        };
        let loops = offset_wire(&[(c.clone(), 0.0, 2.0 * PI)], -2.5).unwrap();
        assert_eq!(loops.len(), 1);
        let mut corners = 0;
        let n = loops[0].len();
        for (i, p) in loops[0].iter().enumerate() {
            for k in 0..=16 {
                let q = p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 16.0);
                let near = c.point(c.closest(&q, 0.0, 2.0 * PI));
                assert!(((q - near).norm() - 2.5).abs() < 1e-8, "{q:?}");
            }
            let next = &loops[0][(i + 1) % n];
            if p.dir(p.t1).dot(&next.dir(next.t0)) < 0.999 {
                corners += 1;
                let q = p.end();
                assert!(q.y.abs() < 1e-9 && q.x.abs() > 1.0, "{q:?}");
            }
        }
        assert_eq!(corners, 2);
    }

    #[test]
    fn slight_convex_corner_gets_an_arc() {
        // A square whose top edge is tilted by 1e-6 rad: outwards the corner is convex and gets
        // a (tiny) arc as OCCT's, inwards none.
        let pts = [
            P3::new(0.0, 0.0, 0.0),
            P3::new(10.0, 0.0, 0.0),
            P3::new(10.0, 10.0, 0.0),
            P3::new(5.0, 10.0, 0.0),
            P3::new(0.0, 10.0 - 5e-6, 0.0),
        ];
        let edges: Vec<(Curve, f64, f64)> = (0..5)
            .map(|i| {
                let (c, l) = Curve::line(pts[i], pts[(i + 1) % 5]);
                (c, 0.0, l)
            })
            .collect();
        let out = offset_wire(&edges, 1.0).unwrap();
        let inn = offset_wire(&edges, -1.0).unwrap();
        // Outwards: 5 edges, 4 corner arcs and the slight one; inwards the 5 edges.
        assert_eq!(out[0].len(), 10);
        assert_eq!(inn[0].len(), 5);
    }

    #[test]
    fn nearly_planar_points_fit_a_plane() {
        let pts: Vec<P3> = (0..20)
            .map(|i| P3::new(i as f64, (i * i) as f64 * 0.1, 1e-4 * (i % 3) as f64))
            .collect();
        let (_, n) = best_plane(&pts, 1e-2).unwrap();
        assert!(n.z.abs() > 0.999);
        assert!(best_plane(&pts, 1e-9).is_none());
    }
}

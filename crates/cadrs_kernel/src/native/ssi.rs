//! Intersections: surface–surface (analytic cases exactly, the rest by the numeric engine of
//! [`super::ssi_numeric`]: subdivision with a loop test for every branch, then marching on both
//! surfaces), curve–surface and curve–curve.

use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::surface::Surface;
use super::tol;
use rayon::prelude::*;

/// A piece of an intersection curve.
#[derive(Debug, Clone)]
pub struct IntCurve {
    pub c: Curve,
    pub t0: f64,
    pub t1: f64,
    pub closed: bool,
    /// For a traced curve, its p-curves on the two surfaces, fitted with the 3D curve from the
    /// tracer's (u, v) (z = 0; for either surface, None where they don't follow the curve). A
    /// consumer checks one against its surface and the curve before taking it
    /// ([`pcurve_from`]): a curve changed since (trimmed, rebuilt) leaves them behind.
    pub pcs: Option<Arc<[Option<Curve>; 2]>>,
}

/// The p-curve on `s` among `ic`'s fitted ones that puts `s` on the curve (within 1e-7 of its
/// size at 9 points over [t0, t1]), if any.
pub fn pcurve_from(
    pcs: &[Option<Curve>; 2],
    s: &Surface,
    c: &Curve,
    t0: f64,
    t1: f64,
) -> Option<Curve> {
    let size = 1.0 + c.point(t0).coords.amax().max(c.point(t1).coords.amax());
    pcs.iter()
        .flatten()
        .find(|pc| {
            (0..=8).all(|k| {
                let t = t0 + (t1 - t0) * k as f64 / 8.0;
                let q = pc.point(t);
                (s.eval(q.x, q.y) - c.point(t)).norm() <= 1e-7 * size
            })
        })
        .cloned()
}

/// The intersection of two surfaces near the box `bb` (the overlap of the two faces' boxes),
/// given the uv boxes of the faces for the numeric fallback: the crossing curves, the curves
/// along which the surfaces touch, the boundary of a region where they coincide, and isolated
/// points of contact as curves of no length (t0 = t1).
pub fn surface_surface(
    a: &Surface,
    ua: ((f64, f64), (f64, f64)),
    b: &Surface,
    ub: ((f64, f64), (f64, f64)),
    bb: &Bbox,
) -> Vec<IntCurve> {
    let r = surface_surface_full(a, ua, b, ub, bb);
    if std::env::var_os("CADRS_SSI_DEBUG").is_some() {
        eprintln!(
            "surface_surface: {} crossing (length {:.4}), {} tangent, {} coincident, points {:?}",
            r.curves.len(),
            r.curves.iter().map(|c| c.c.length(c.t0, c.t1)).sum::<f64>(),
            r.tangent.len(),
            r.coincident.len(),
            r.points
        );
        for c in &r.tangent {
            eprintln!(
                "  tangent {:?} .. {:?} length {}",
                c.c.point(c.t0),
                c.c.point(c.t1),
                c.c.length(c.t0, c.t1)
            );
        }
    }
    // (A point of contact as a curve of no length.)
    let points = r.points.into_iter().map(|p| IntCurve {
        c: Curve::Line { o: p, d: V3::x() },
        t0: 0.0,
        t1: 0.0,
        closed: false,
        pcs: None,
    });
    r.curves
        .into_iter()
        .chain(r.tangent)
        .chain(r.coincident)
        .chain(points)
        .collect()
}

/// A surface–surface intersection in parts.
#[derive(Debug, Clone, Default)]
pub struct Ssi {
    /// Curves where the surfaces cross.
    pub curves: Vec<IntCurve>,
    /// Curves along which they touch without crossing (numeric cases; the closed forms give
    /// theirs in `curves`).
    pub tangent: Vec<IntCurve>,
    /// Points on the crossing curves where the surfaces are tangent (branch points and touching
    /// points, where curves meet): each curve passing one runs through it.
    pub singular: Vec<P3>,
    /// The boundary of the region where the surfaces coincide (over their boxes): the sides of
    /// either box lying on the other surface inside its box. (Booleans treat coincident faces
    /// apart.)
    pub coincident: Vec<IntCurve>,
    /// Isolated points where the surfaces touch.
    pub points: Vec<P3>,
    /// The work it took, deterministic for the inputs: the numeric engine's cell pairs searched
    /// and points traced (1 for a closed form). What a boolean's work budget counts.
    pub work: u64,
}

/// How near two surfaces must be to coincide (within a part in 10⁹ of the problem's size,
/// and at least [`super::tol::CONFUSION`]).
fn coincidence_tol(bb: &Bbox) -> f64 {
    super::tol::CONFUSION.max(tol::REL * bb.diagonal())
}

/// [`surface_surface`] in parts.
pub fn surface_surface_full(
    a: &Surface,
    ua: ((f64, f64), (f64, f64)),
    b: &Surface,
    ub: ((f64, f64), (f64, f64)),
    bb: &Bbox,
) -> Ssi {
    surface_surface_tol(a, ua, b, ub, bb, super::boolean::VTOL)
}

/// [`surface_surface`] where it has a closed form (coincident surfaces' region, a plane and an
/// extrusion, the analytic pairs), else `None`: the curves the numeric engine would trace (fitted
/// B-splines) not traced, for a caller that only takes exact ones.
pub(crate) fn surface_surface_closed(
    a: &Surface,
    ua: ((f64, f64), (f64, f64)),
    b: &Surface,
    ub: ((f64, f64), (f64, f64)),
    bb: &Bbox,
) -> Option<Vec<IntCurve>> {
    let tc = coincidence_tol(bb);
    let closed = a.same_domain(b, tc).is_some()
        || plane_extrusion(a, b, ub, bb).is_some()
        || plane_extrusion(b, a, ua, bb).is_some()
        || analytic(a, b, bb, super::boolean::VTOL).is_some()
        || analytic(b, a, bb, super::boolean::VTOL).is_some();
    closed.then(|| surface_surface(a, ua, b, ub, bb))
}

/// [`surface_surface_full`] for a boolean merging points within `tol`: surfaces touching
/// within it touch (two spheres meet in a point, not a circle that small).
pub fn surface_surface_tol(
    a: &Surface,
    ua: ((f64, f64), (f64, f64)),
    b: &Surface,
    ub: ((f64, f64), (f64, f64)),
    bb: &Bbox,
    tol: f64,
) -> Ssi {
    let closed = |curves| Ssi {
        curves,
        work: 1,
        ..Ssi::default()
    };
    let tc = coincidence_tol(bb);
    if a.same_domain(b, tc).is_some() {
        return Ssi {
            coincident: super::ssi_coincident::region_boundary(a, ua, b, ub, tc),
            ..Ssi::default()
        };
    }
    if let Some(r) = plane_extrusion(a, b, ub, bb) {
        return closed(r);
    }
    if let Some(r) = plane_extrusion(b, a, ua, bb) {
        return closed(r);
    }
    if let Some(r) = analytic(a, b, bb, tol) {
        return closed(r);
    }
    if let Some(r) = analytic(b, a, bb, tol) {
        return closed(r);
    }
    // Numeric, over the surface without an implicit form first (its points' parameters are
    // the ones the fit refines on).
    let implicit = |s: &Surface| s.signed_distance(&P3::origin()).is_some();
    if implicit(a) && !implicit(b) {
        return numeric(b, ub, a, ua, bb);
    }
    numeric(a, ua, b, ub, bb)
}

/// A plane and an extrusion: rulings where the plane contains the direction, otherwise the
/// base curve projected along the direction onto the plane (an affine image: exact).
fn plane_extrusion(
    p: &Surface,
    e: &Surface,
    ue: ((f64, f64), (f64, f64)),
    bb: &Bbox,
) -> Option<Vec<IntCurve>> {
    let (Surface::Plane { f }, Surface::Extrusion { c, d }) = (p, e) else {
        return None;
    };
    let n = f.x.cross(&f.y);
    let (u0, u1) = ue.0;
    let pad = (u1 - u0).abs() * tol::REL_POINT;
    let (u0, u1) = (u0 - pad, u1 + pad);
    let dn = n.dot(d);
    if dn.abs() < tol::REL_FINE * d.norm() {
        // Rulings through the points where the base curve meets the plane.
        let pl = Surface::Plane { f: *f };
        let bbig = bb.grow(bb.diagonal() * 0.01 + tol::POINT);
        let mut out = Vec::new();
        let (lo, hi) = match c.nurbs_range() {
            Some((a, b)) => (u0.max(a), u1.min(b)),
            None => (u0, u1),
        };
        for t in curve_surface(c, lo, hi, &pl) {
            out.extend(bound_line(c.point(t), d.normalize(), &bbig));
        }
        return Some(out);
    }
    // x ↦ x − d (n·(x − o)) / (n·d): the projection along d onto the plane.
    let m = nalgebra::Matrix3::identity() - d * n.transpose() / dn;
    let aff = Affine {
        m,
        t: d * (n.dot(&f.o.coords) / dn),
    };
    let (lo, hi) = match c.nurbs_range() {
        Some((a, b)) => (u0.max(a), u1.min(b)),
        None => (u0, u1),
    };
    if hi <= lo {
        return Some(vec![]);
    }
    let curve = match c {
        Curve::Line { o, d: dl } => {
            let o2 = aff.point(o);
            let d2 = m * dl;
            Curve::Line { o: o2, d: d2 }
        }
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            Curve::Nurbs(Arc::new(c.to_nurbs(lo, hi).transformed(&aff)))
        }
        Curve::Nurbs(nc) => Curve::Nurbs(Arc::new(nc.transformed(&aff))),
    };
    let closed = c.is_periodic() && (hi - lo) >= TAU - tol::ANGULAR;
    Some(vec![IntCurve {
        c: curve,
        t0: lo,
        t1: hi,
        closed,
        pcs: None,
    }])
}

/// A plane and a cone meet in a conic: in the cone's frame, with g(u) = n_x cos u + n_y sin u,
/// the plane n·X = c gives v = (c − r g)/(s g + n_z k) (s, k the sine and cosine of the half
/// angle), so X·D = (A cos u, A sin u, k(c − r g)) with D = s g + n_z k and A = r n_z k + s c:
/// with the half-angle substitution a rational quadratic in t, exact as rational Bézier pieces.
/// The pieces cover the parts of the curve inside `bb`. None when the plane passes through the
/// apex (A = 0: lines, handled elsewhere or numerically).
fn plane_cone(fp: &Frame, fc: &Frame, r: f64, ang: f64, bb: &Bbox) -> Option<Vec<IntCurve>> {
    use super::nurbs::{H, NurbsCurve};
    let n = plane_n(fp);
    let nl = V3::new(n.dot(&fc.x), n.dot(&fc.y), n.dot(&fc.z));
    let c = n.dot(&(fp.o - fc.o));
    let (s, k) = ang.sin_cos();
    let q = nl.z * k;
    let a = r * q + s * c;
    if a.abs() < tol::REL * (1.0 + r.abs() + c.abs()) {
        return None;
    }
    let local_point = |u: f64| -> Option<P3> {
        let g = nl.x * u.cos() + nl.y * u.sin();
        let d = s * g + q;
        if d.abs() < 1e-14 {
            return None;
        }
        let v = (c - r * g) / d;
        let rho = r + s * v;
        // The cone's own sheet only (radius r + v sin a ≥ 0): the other nappe of the double
        // cone the conic lies on is no part of the surface (a cone past 90°, nearly flat,
        // has it a hair away).
        if rho < -tol::ROUNDOFF * (1.0 + r.abs()) {
            return None;
        }
        Some(fc.o + (fc.x * u.cos() + fc.y * u.sin()) * rho + fc.z * (k * v))
    };
    let inside = |u: f64| local_point(u).is_some_and(|p| bb.contains(&p));
    // Runs of u inside the box: an even sampling, and the u where the conic passes each box
    // corner's height along the axis (a conic of a plane nearly along the axis is huge, its
    // part in the box a sliver of u between even samples).
    let m = 360;
    let mut us: Vec<f64> = (0..m).map(|i| TAU * i as f64 / m as f64).collect();
    let (gr, phi) = (nl.x.hypot(nl.y), nl.y.atan2(nl.x));
    if k.abs() > tol::ANGULAR_FINE && gr > tol::ANGULAR_FINE {
        for kx in 0..8 {
            let corner = P3::new(
                if kx & 1 == 0 { bb.min.x } else { bb.max.x },
                if kx & 2 == 0 { bb.min.y } else { bb.max.y },
                if kx & 4 == 0 { bb.min.z } else { bb.max.z },
            );
            let v = (corner - fc.o).dot(&fc.z) / k;
            // v(u) = (c − r g) / (s g + q) at that v: g = (c − v q) / (v s + r), g = gr cos(u − φ).
            let den = v * s + r;
            if den.abs() < tol::TINY {
                continue;
            }
            let g = (c - v * q) / den;
            if g.abs() <= gr {
                let w = (g / gr).acos();
                for u in [phi + w, phi - w] {
                    let u = u.rem_euclid(TAU);
                    // Either side of it too (the conic's part in the box may be thinner than
                    // the even step).
                    us.extend([
                        u,
                        (u - tol::REL_POINT).rem_euclid(TAU),
                        (u + tol::REL_POINT).rem_euclid(TAU),
                    ]);
                }
            }
        }
    }
    us.sort_by(f64::total_cmp);
    us.dedup_by(|a, b| (*a - *b).abs() < tol::REL_FINE);
    let m = us.len();
    let ins: Vec<bool> = us.iter().map(|&u| inside(u)).collect();
    if !ins.iter().any(|x| *x) {
        return Some(vec![]);
    }
    let edge_of = |a: f64, b: f64| -> f64 {
        // The boundary between an inside `a` and an outside `b`.
        let (mut lo, mut hi) = (a, b);
        for _ in 0..60 {
            let mid = 0.5 * (lo + hi);
            if inside(mid) { lo = mid } else { hi = mid }
        }
        lo
    };
    // The sample `i` steps on from `start`, unwrapped past 2π.
    let at = |start: usize, i: usize| us[(start + i) % m] + if start + i >= m { TAU } else { 0.0 };
    let mut runs: Vec<(f64, f64, bool)> = Vec::new();
    match (0..m).find(|&i| !ins[i]) {
        None => runs.push((0.0, TAU, true)),
        Some(start) => {
            let mut i = 0;
            while i < m {
                if ins[(start + i) % m] {
                    let mut e = i;
                    while e + 1 < m && ins[(start + e + 1) % m] {
                        e += 1;
                    }
                    // Samples before and after the run are outside (the run starts after the
                    // outside sample `start`, and ends before one).
                    let ua = edge_of(at(start, i), at(start, i - 1));
                    let ub = edge_of(at(start, e), at(start, e + 1));
                    runs.push((ua, ub, false));
                    i = e + 1;
                } else {
                    i += 1;
                }
            }
        }
    }
    let to_world = |h: &nalgebra::Vector4<f64>| -> H {
        let p = fc.o.coords * h.w + fc.x * h.x + fc.y * h.y + fc.z * h.z;
        H::new(p.x, p.y, p.z, h.w)
    };
    // One rational quadratic Bézier piece on [ua, ub] (|ub − ua| < π), homogeneous, local.
    let piece = |ua: f64, ub: f64| -> [nalgebra::Vector4<f64>; 3] {
        let um = 0.5 * (ua + ub);
        let tt = (0.25 * (ub - ua)).tan();
        let (cm, sm) = (um.cos(), um.sin());
        let at = |tau: f64| -> nalgebra::Vector4<f64> {
            let t = -tt + 2.0 * tt * tau;
            let (c1, s1, one) = (1.0 - t * t, 2.0 * t, 1.0 + t * t);
            let cu = cm * c1 - sm * s1;
            let su = sm * c1 + cm * s1;
            let g = nl.x * cu + nl.y * su;
            nalgebra::Vector4::new(a * cu, a * su, k * (c * one - r * g), s * g + q * one)
        };
        let (p0, pm, p1) = (at(0.0), at(0.5), at(1.0));
        [p0, pm * 2.0 - (p0 + p1) * 0.5, p1]
    };
    let mut out = Vec::new();
    for (ua, ub, closed) in runs {
        if ub - ua < tol::ROUNDOFF {
            continue;
        }
        // Pieces of at most π/4 whose weights share one sign.
        let mut spans: Vec<(f64, f64)> = Vec::new();
        let n0 = ((ub - ua) / (PI / 4.0)).ceil().max(1.0) as usize;
        let mut stack: Vec<(f64, f64)> = (0..n0)
            .rev()
            .map(|i| {
                (
                    ua + (ub - ua) * i as f64 / n0 as f64,
                    ua + (ub - ua) * (i + 1) as f64 / n0 as f64,
                )
            })
            .collect();
        while let Some((x, y)) = stack.pop() {
            let b = piece(x, y);
            let pos = b.iter().all(|h| h.w > tol::TINY);
            let neg = b.iter().all(|h| h.w < -tol::TINY);
            if pos || neg || y - x < tol::POINT {
                spans.push((x, y));
            } else {
                let mid = 0.5 * (x + y);
                stack.push((mid, y));
                stack.push((x, mid));
            }
        }
        let mut cps: Vec<H> = Vec::new();
        let mut knots = vec![0.0; 3];
        for (i, &(x, y)) in spans.iter().enumerate() {
            let mut b = piece(x, y);
            if b[0].w < 0.0 {
                for h in b.iter_mut() {
                    *h = -*h;
                }
            }
            if let Some(last) = cps.last() {
                let f = last.w / b[0].w;
                for h in b.iter_mut() {
                    *h *= f;
                }
                cps.push(to_world(&b[1]));
                cps.push(to_world(&b[2]));
            } else {
                cps.extend(b.iter().map(to_world));
            }
            let kv = (i + 1) as f64;
            if i + 1 < spans.len() {
                knots.extend([kv, kv]);
            } else {
                knots.extend([kv; 3]);
            }
        }
        let nc = NurbsCurve {
            degree: 2,
            knots,
            cps,
        };
        let (t0, t1) = nc.range();
        out.push(IntCurve {
            c: Curve::Nurbs(std::sync::Arc::new(nc)),
            t0,
            t1,
            closed,
            pcs: None,
        });
    }
    Some(out)
}

fn bound_line(o: P3, d: V3, bb: &Bbox) -> Option<IntCurve> {
    // Clip the line to the box (slabs).
    let (mut t0, mut t1) = (f64::NEG_INFINITY, f64::INFINITY);
    for k in 0..3 {
        let (lo, hi) = (bb.min[k], bb.max[k]);
        if d[k].abs() < 1e-14 {
            if o[k] < lo - tol::ROUNDOFF || o[k] > hi + tol::ROUNDOFF {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o[k]) / d[k], (hi - o[k]) / d[k]);
        let (a, b) = if a < b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
    }
    if t1 - t0 < tol::ROUNDOFF {
        return None;
    }
    Some(IntCurve {
        c: Curve::Line { o, d },
        t0,
        t1,
        closed: false,
        pcs: None,
    })
}

fn circle(c: P3, n: V3, r: f64) -> IntCurve {
    let f = Frame::from_z(c, n);
    IntCurve {
        c: Curve::Circle {
            c,
            x: f.x,
            y: f.y,
            r,
        },
        t0: 0.0,
        t1: TAU,
        closed: true,
        pcs: None,
    }
}

fn plane_n(f: &Frame) -> V3 {
    f.x.cross(&f.y)
}

/// Closed forms. `None` when the pair isn't handled here (try the other order, then numeric).
fn analytic(a: &Surface, b: &Surface, bb: &Bbox, run_tol: f64) -> Option<Vec<IntCurve>> {
    let bbig = bb.grow(bb.diagonal() * 0.01 + tol::POINT);
    match (a, b) {
        (Surface::Plane { f: fa }, Surface::Plane { f: fb }) => {
            let (na, nb) = (plane_n(fa), plane_n(fb));
            let d = na.cross(&nb);
            if d.norm() < tol::DEGENERATE {
                return Some(vec![]);
            }
            let d = d.normalize();
            // A point on both planes.
            let (ha, hb) = (na.dot(&fa.o.coords), nb.dot(&fb.o.coords));
            let n12 = na.dot(&nb);
            let det = 1.0 - n12 * n12;
            let c1 = (ha - hb * n12) / det;
            let c2 = (hb - ha * n12) / det;
            let p = P3::from(na * c1 + nb * c2);
            // Start the line near the box centre for good conditioning.
            let p = p + d * (bb.center() - p).dot(&d);
            Some(bound_line(p, d, &bbig).into_iter().collect())
        }
        (Surface::Plane { f: fp }, Surface::Cylinder { f: fc, r }) => {
            let n = plane_n(fp);
            let z = fc.z;
            let cz = n.dot(&z);
            if (1.0 - cz.abs()) < tol::ANGULAR_FINE {
                // Square to the axis: a circle.
                let t = (fp.o - fc.o).dot(&n) / cz;
                let c = fc.o + z * t;
                return Some(vec![IntCurve {
                    c: Curve::Circle {
                        c,
                        x: fc.x,
                        y: fc.y,
                        r: *r,
                    },
                    t0: 0.0,
                    t1: TAU,
                    closed: true,
                    pcs: None,
                }]);
            }
            // Nearly parallel (single-precision data tilt a plane by 1e-8): the ellipse is huge
            // and its parts in the region are straight to second order in the tilt; each is the
            // chord between where the plane cuts the cross-section circles at the region's
            // lowest and highest points along the axis.
            if cz.abs() >= tol::ANGULAR_FINE && cz.abs() * bbig.diagonal() < 1e-3 {
                let corners: Vec<P3> = (0..8)
                    .map(|k| {
                        P3::new(
                            if k & 1 == 0 { bbig.min.x } else { bbig.max.x },
                            if k & 2 == 0 { bbig.min.y } else { bbig.max.y },
                            if k & 4 == 0 { bbig.min.z } else { bbig.max.z },
                        )
                    })
                    .collect();
                let hs: Vec<f64> = corners.iter().map(|c| (c - fc.o).dot(&z)).collect();
                let (h0, h1) = (
                    hs.iter().copied().fold(f64::INFINITY, f64::min),
                    hs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                );
                // At height h: the cross-section's centre, the plane's line in it (direction
                // w = z × n, offset d from the centre), and its two crossings.
                let cuts = |h: f64| -> Option<[P3; 2]> {
                    let c = fc.o + z * h;
                    let d = (c - fp.o).dot(&n);
                    // The plane's trace in the section plane: n projected square to z.
                    let np = (n - z * n.dot(&z)).try_normalize(tol::DEGENERATE)?;
                    let dd = d / n.dot(&np);
                    if dd.abs() > *r + 1e-7 * r.max(1.0) {
                        return None;
                    }
                    let w = z.cross(&np).normalize();
                    let k = (r * r - dd * dd).max(0.0).sqrt();
                    let foot = c - np * dd;
                    Some([foot + w * k, foot - w * k])
                };
                if let (Some(a), Some(b)) = (cuts(h0), cuts(h1)) {
                    let mut out = Vec::new();
                    let tangent = (a[0] - a[1]).norm() < 1e-7 * r.max(1.0);
                    for i in 0..if tangent { 1 } else { 2 } {
                        if let Some(d) = (b[i] - a[i]).try_normalize(tol::DEGENERATE) {
                            out.extend(bound_line(a[i], d, &bbig));
                        }
                    }
                    return Some(out);
                }
            }
            // Parallel to the axis: lines where the plane cuts the circle.
            if cz.abs() < tol::ANGULAR_FINE {
                let d = (fc.o - fp.o).dot(&n); // signed distance of the axis from the plane
                // (A plane touching within the tangency tolerance, outside as well as inside,
                // meets the cylinder along one line.)
                if d.abs() > r + 1e-7 * r.max(1.0) {
                    return Some(vec![]);
                }
                let foot = fc.o - n * d;
                let w = z.cross(&n).normalize();
                let h = (r * r - d * d).max(0.0).sqrt();
                let mut out = Vec::new();
                // Tangent (to within a tenth of the boolean's tolerance in distance; the two lines
                // of a plane 1e-16 inside are still 1.5e-8 apart): one line.
                if h < tol::ROUNDOFF || r - d.abs() < 1e-7 * r.max(1.0) {
                    out.extend(bound_line(foot, z, &bbig));
                } else {
                    out.extend(bound_line(foot + w * h, z, &bbig));
                    out.extend(bound_line(foot - w * h, z, &bbig));
                }
                return Some(out);
            }
            // Oblique: an ellipse centred where the axis meets the plane.
            let t = (fp.o - fc.o).dot(&n) / cz;
            let c = fc.o + z * t;
            let minor = z.cross(&n).normalize(); // in the plane, square to the axis
            let major = n.cross(&minor).normalize(); // in the plane, along the tilt
            let a_len = r / cz.abs();
            // A plane almost along the axis: the ellipse is far bigger than the region the faces
            // span (parameters on it are ill-conditioned there); its part in the box as exact
            // rational pieces instead (a cylinder is a cone of half angle 0).
            if a_len > 4.0 * bbig.diagonal() {
                return plane_cone(fp, fc, *r, 0.0, &bbig);
            }
            Some(vec![IntCurve {
                c: Curve::Ellipse {
                    c,
                    x: major,
                    y: minor,
                    a: a_len,
                    b: *r,
                },
                t0: 0.0,
                t1: TAU,
                closed: true,
                pcs: None,
            }])
        }
        (Surface::Plane { f: fp }, Surface::Sphere { f: fs, r }) => {
            let n = plane_n(fp);
            let d = (fs.o - fp.o).dot(&n);
            if d.abs() >= *r - tol::ROUNDOFF_FINE {
                return Some(vec![]);
            }
            Some(vec![circle(fs.o - n * d, n, (r * r - d * d).sqrt())])
        }
        (Surface::Plane { f: fp }, Surface::Cone { f: fc, r, a: ang }) => {
            let n = plane_n(fp);
            let cz = n.dot(&fc.z);
            if (1.0 - cz.abs()) < tol::ANGULAR_FINE {
                // Square to the axis: v = height / cos a; radius r + v sin a.
                let h = (fp.o - fc.o).dot(&fc.z);
                let v = h / ang.cos();
                let rr = r + v * ang.sin();
                if rr <= tol::ROUNDOFF_FINE {
                    return Some(vec![]);
                }
                let c = fc.o + fc.z * h;
                return Some(vec![IntCurve {
                    c: Curve::Circle {
                        c,
                        x: fc.x,
                        y: fc.y,
                        r: rr,
                    },
                    t0: 0.0,
                    t1: TAU,
                    closed: true,
                    pcs: None,
                }]);
            }
            // Through the apex: the rulings in the plane, none (the apex alone), one (the plane
            // tangent along it) or two. A ruling at angle u runs from the apex along
            // (x cos u + y sin u) sin a + z cos a (the way the radius r + v sin a grows), in the
            // plane where sin a (n·x cos u + n·y sin u) + cos a (n·z) = 0. (The general conic
            // below has no curve here, and the marcher takes long over a tangent ruling.)
            let (s, k) = ang.sin_cos();
            let nl = V3::new(n.dot(&fc.x), n.dot(&fc.y), cz);
            let c = n.dot(&(fp.o - fc.o));
            if (r * nl.z * k + s * c).abs() < tol::REL * (1.0 + r.abs() + c.abs())
                && s.abs() > tol::ANGULAR_FINE
            {
                let apex = fc.o - fc.z * (r * k / s);
                let m = nl.x.hypot(nl.y);
                let g = -k * nl.z / s;
                let rulings: Vec<f64> = if m < tol::ANGULAR_FINE {
                    vec![]
                } else {
                    let phi = nl.y.atan2(nl.x);
                    let x = g / m;
                    if (x.abs() - 1.0).abs() < 1e-9 {
                        vec![if x > 0.0 { phi } else { phi + PI }]
                    } else if x.abs() < 1.0 {
                        let w = x.acos();
                        vec![phi + w, phi - w]
                    } else {
                        vec![]
                    }
                };
                // (Along a ruling the radius grows as v does when sin a > 0, else as it falls.)
                let sense = if s > 0.0 { 1.0 } else { -1.0 };
                let mut out = Vec::new();
                for u in rulings {
                    let d = ((fc.x * u.cos() + fc.y * u.sin()) * s + fc.z * k) * sense;
                    if let Some(mut l) = bound_line(apex, d.normalize(), &bbig) {
                        l.t0 = l.t0.max(0.0);
                        if l.t1 > l.t0 + tol::ROUNDOFF {
                            out.push(l);
                        }
                    }
                }
                return Some(out);
            }
            plane_cone(fp, fc, *r, *ang, &bbig)
        }
        (Surface::Plane { f: fp }, Surface::Torus { f: ft, rmaj, rmin }) => {
            let n = plane_n(fp);
            let cz = n.dot(&ft.z);
            if (1.0 - cz.abs()) < tol::ANGULAR_FINE {
                let h = (fp.o - ft.o).dot(&ft.z);
                let tol = tol::REL * (1.0 + rmaj + rmin);
                if h.abs() > *rmin + tol {
                    return Some(vec![]);
                }
                let c = ft.o + ft.z * h;
                if h.abs() >= *rmin - tol {
                    // Tangent (a blend's plane): the circle the tube touches it along.
                    return Some(vec![IntCurve {
                        c: Curve::Circle {
                            c,
                            x: ft.x,
                            y: ft.y,
                            r: *rmaj,
                        },
                        t0: 0.0,
                        t1: TAU,
                        closed: true,
                        pcs: None,
                    }]);
                }
                let w = (rmin * rmin - h * h).sqrt();
                return Some(vec![
                    IntCurve {
                        c: Curve::Circle {
                            c,
                            x: ft.x,
                            y: ft.y,
                            r: rmaj + w,
                        },
                        t0: 0.0,
                        t1: TAU,
                        closed: true,
                        pcs: None,
                    },
                    IntCurve {
                        c: Curve::Circle {
                            c,
                            x: ft.x,
                            y: ft.y,
                            r: rmaj - w,
                        },
                        t0: 0.0,
                        t1: TAU,
                        closed: true,
                        pcs: None,
                    },
                ]);
            }
            if cz.abs() < tol::ANGULAR_FINE && (ft.o - fp.o).dot(&n).abs() < tol::ROUNDOFF {
                // A plane through the axis: two meridian circles.
                let w = ft.z.cross(&n).normalize();
                return Some(vec![
                    IntCurve {
                        c: Curve::Circle {
                            c: ft.o + w * *rmaj,
                            x: w,
                            y: ft.z,
                            r: *rmin,
                        },
                        t0: 0.0,
                        t1: TAU,
                        closed: true,
                        pcs: None,
                    },
                    IntCurve {
                        c: Curve::Circle {
                            c: ft.o - w * *rmaj,
                            x: -w,
                            y: ft.z,
                            r: *rmin,
                        },
                        t0: 0.0,
                        t1: TAU,
                        closed: true,
                        pcs: None,
                    },
                ]);
            }
            None
        }
        (Surface::Cylinder { f: f1, r: r1 }, Surface::Cylinder { f: f2, r: r2 }) => {
            if f1.z.cross(&f2.z).norm() > tol::ANGULAR_FINE {
                return None;
            }
            // Parallel axes: lines.
            let d = f2.o - f1.o;
            let d = d - f1.z * d.dot(&f1.z);
            let l = d.norm();
            if l < tol::DEGENERATE {
                return Some(vec![]); // coaxial: no curve (or the same surface)
            }
            let tol = tol::REL * (1.0 + r1 + r2);
            if l > r1 + r2 + tol || l < (r1 - r2).abs() - tol {
                return Some(vec![]);
            }
            let x = (l * l + r1 * r1 - r2 * r2) / (2.0 * l);
            let h = (r1 * r1 - x * x).max(0.0).sqrt();
            let ex = d / l;
            let ey = f1.z.cross(&ex);
            let base = f1.o + ex * x;
            let mut out = Vec::new();
            let tangent = (l - (r1 + r2)).abs() <= tol || (l - (r1 - r2).abs()).abs() <= tol;
            if tangent || h < tol::ROUNDOFF {
                out.extend(bound_line(base, f1.z, &bbig));
            } else {
                out.extend(bound_line(base + ey * h, f1.z, &bbig));
                out.extend(bound_line(base - ey * h, f1.z, &bbig));
            }
            Some(out)
        }
        (Surface::Sphere { f: f1, r: r1 }, Surface::Sphere { f: f2, r: r2 }) => {
            let d = f2.o - f1.o;
            let l = d.norm();
            // Touching within the run's tolerance or a relative 1e-7 (a gap or overlap that
            // small): a point of tangency, not a circle (one of radius √(2rδ) would cut a sliver cap).
            let tol = run_tol.max(1e-7 * (r1 + r2));
            if l < tol::DEGENERATE || l > r1 + r2 - tol || l < (r1 - r2).abs() + tol {
                return Some(vec![]);
            }
            let x = (l * l + r1 * r1 - r2 * r2) / (2.0 * l);
            Some(vec![circle(
                f1.o + d / l * x,
                d / l,
                (r1 * r1 - x * x).max(0.0).sqrt(),
            )])
        }
        _ => {
            // Coaxial surfaces of revolution: circles where the meridians cross. A sphere is
            // symmetric about any axis through its centre.
            let (Some((mut oa, mut za)), Some((ob, zb))) = (a.axis(), b.axis()) else {
                return None;
            };
            if let Surface::Sphere { f, .. } = a {
                let w = f.o - ob;
                if (w - zb * w.dot(&zb)).norm() < tol::ROUNDOFF {
                    oa = ob;
                    za = zb;
                }
            }
            let (ob, zb) = if let Surface::Sphere { f, .. } = b {
                let w = f.o - oa;
                if (w - za * w.dot(&za)).norm() < tol::ROUNDOFF {
                    (oa, za)
                } else {
                    (ob, zb)
                }
            } else {
                (ob, zb)
            };
            if !matches!(
                a,
                Surface::Cylinder { .. }
                    | Surface::Cone { .. }
                    | Surface::Sphere { .. }
                    | Surface::Torus { .. }
            ) {
                return None;
            }
            if !matches!(
                b,
                Surface::Cylinder { .. }
                    | Surface::Cone { .. }
                    | Surface::Sphere { .. }
                    | Surface::Torus { .. }
            ) {
                return None;
            }
            if za.cross(&zb).norm() > tol::ANGULAR_FINE {
                return None;
            }
            let w = ob - oa;
            if (w - za * w.dot(&za)).norm() > tol::ROUNDOFF {
                return None;
            }
            Some(coaxial(a, b, &oa, &za, bb))
        }
    }
}

/// The meridian of an analytic surface of revolution about (o, z) in the (ρ, h) half plane:
/// a line (cylinder, cone, plane square to the axis) or a circle (sphere, torus).
#[derive(Debug, Clone, Copy)]
enum Meridian {
    /// Points p + s·d.
    Line {
        p: (f64, f64),
        d: (f64, f64),
    },
    Circle {
        c: (f64, f64),
        r: f64,
    },
}

fn meridian(s: &Surface, o: &P3, z: &V3) -> Option<Meridian> {
    let h_of = |p: &P3| (p - o).dot(z);
    Some(match s {
        Surface::Cylinder { r, .. } => Meridian::Line {
            p: (*r, 0.0),
            d: (0.0, 1.0),
        },
        Surface::Cone { f, r, a } => {
            let s = f.z.dot(z).signum();
            Meridian::Line {
                p: (*r, h_of(&f.o)),
                d: (a.sin(), a.cos() * s),
            }
        }
        Surface::Sphere { f, r } => Meridian::Circle {
            c: (0.0, h_of(&f.o)),
            r: *r,
        },
        Surface::Torus { f, rmaj, rmin } => Meridian::Circle {
            c: (*rmaj, h_of(&f.o)),
            r: *rmin,
        },
        Surface::Plane { f } => {
            let n = f.x.cross(&f.y);
            if n.cross(z).norm() > tol::ANGULAR_FINE {
                return None;
            }
            Meridian::Line {
                p: (0.0, h_of(&f.o)),
                d: (1.0, 0.0),
            }
        }
        _ => return None,
    })
}

/// Intersections of two meridians (ρ, h), tangencies included (within `tol`).
fn meridian_points(a: Meridian, b: Meridian, tol: f64) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    match (a, b) {
        (Meridian::Line { p: p1, d: d1 }, Meridian::Line { p: p2, d: d2 }) => {
            let den = d1.0 * d2.1 - d1.1 * d2.0;
            if den.abs() < 1e-14 {
                return out;
            }
            let (wx, wy) = (p2.0 - p1.0, p2.1 - p1.1);
            let s = (wx * d2.1 - wy * d2.0) / den;
            out.push((p1.0 + d1.0 * s, p1.1 + d1.1 * s));
        }
        (Meridian::Line { p, d }, Meridian::Circle { c, r })
        | (Meridian::Circle { c, r }, Meridian::Line { p, d }) => {
            let l = (d.0 * d.0 + d.1 * d.1).sqrt();
            let (dx, dy) = (d.0 / l, d.1 / l);
            let (wx, wy) = (p.0 - c.0, p.1 - c.1);
            let b = wx * dx + wy * dy;
            let cc = wx * wx + wy * wy - r * r;
            let disc = b * b - cc;
            if disc < -2.0 * r * tol {
                return out;
            }
            if disc.abs() <= 2.0 * r * tol {
                let s = -b;
                out.push((p.0 + dx * s, p.1 + dy * s));
            } else {
                let q = disc.sqrt();
                for s in [-b - q, -b + q] {
                    out.push((p.0 + dx * s, p.1 + dy * s));
                }
            }
        }
        (Meridian::Circle { c: c1, r: r1 }, Meridian::Circle { c: c2, r: r2 }) => {
            let (dx, dy) = (c2.0 - c1.0, c2.1 - c1.1);
            let d = (dx * dx + dy * dy).sqrt();
            if d < 1e-14 {
                return out;
            }
            if d > r1 + r2 + tol || d < (r1 - r2).abs() - tol {
                return out;
            }
            let x = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
            let h2 = r1 * r1 - x * x;
            let (ex, ey) = (dx / d, dy / d);
            let base = (c1.0 + ex * x, c1.1 + ey * x);
            if h2 <= (r1 * tol * 2.0).max(1e-18) {
                out.push(base);
            } else {
                let h = h2.sqrt();
                out.push((base.0 - ey * h, base.1 + ex * h));
                out.push((base.0 + ey * h, base.1 - ex * h));
            }
        }
    }
    out
}

fn coaxial(a: &Surface, b: &Surface, o: &P3, z: &V3, bb: &Bbox) -> Vec<IntCurve> {
    let (Some(ma), Some(mb)) = (meridian(a, o, z), meridian(b, o, z)) else {
        return vec![];
    };
    let f = Frame::from_z(*o, *z);
    let _ = bb;
    meridian_points(ma, mb, 1e-7)
        .into_iter()
        .filter(|(r, _)| *r > tol::ROUNDOFF)
        .map(|(r, h)| IntCurve {
            c: Curve::Circle {
                c: o + z * h,
                x: f.x,
                y: f.y,
                r,
            },
            t0: 0.0,
            t1: TAU,
            closed: true,
            pcs: None,
        })
        .collect()
}

/// The signed distance to `s` by projection from a hint (`hint` is updated with the foot's uv).
pub fn sdist_hint(s: &Surface, p: &P3, hint: &mut Option<(f64, f64)>) -> f64 {
    if let Some(d) = s.signed_distance(p) {
        return d;
    }
    let (mut u, mut v) = s.project(p, *hint);
    let (mut q, mut su, mut sv) = s.d1(u, v);
    // A hinted Newton may stall on the wrong side of a fold or on the domain's edge: when the
    // foot isn't square to the surface, check against a projection from a coarse grid.
    let r = p - q;
    let tang = (r.dot(&su) / su.norm().max(tol::TINY)).abs()
        + (r.dot(&sv) / sv.norm().max(tol::TINY)).abs();
    if hint.is_some() && tang > 1e-7 * (1.0 + r.norm()) {
        let (u2, v2) = s.project_coarse(p, 8);
        let (q2, su2, sv2) = s.d1(u2, v2);
        if (p - q2).norm() < r.norm() - tol::ROUNDOFF_FINE {
            (u, v, q, su, sv) = (u2, v2, q2, su2, sv2);
        }
    }
    let n = su.cross(&sv);
    let l = n.norm();
    *hint = Some((u, v));
    if l < tol::TINY {
        return (p - q).norm();
    }
    (p - q).dot(&(n / l))
}

/// The signed distance to `s` (by projection when it has no implicit form).
pub fn sdist(s: &Surface, p: &P3) -> f64 {
    if let Some(d) = s.signed_distance(p) {
        return d;
    }
    let (u, v) = s.project(p, None);
    let (q, su, sv) = s.d1(u, v);
    let n = su.cross(&sv);
    let l = n.norm();
    if l < tol::TINY {
        return (p - q).norm();
    }
    (p - q).dot(&(n / l))
}

/// The intersection by the numeric engine ([`super::ssi_numeric`]): its traced curves fitted
/// with B-splines through exact points (tangent curves through the touching points found).
fn numeric(
    a: &Surface,
    ua: ((f64, f64), (f64, f64)),
    b: &Surface,
    ub: ((f64, f64), (f64, f64)),
    bb: &Bbox,
) -> Ssi {
    use super::ssi_numeric::{UvBox, intersect_stats};
    // A box that isn't finite (a face without p-curves) falls back on the surface's own.
    let uvbox = |s: &Surface, x: ((f64, f64), (f64, f64))| {
        let ok = [x.0.0, x.0.1, x.1.0, x.1.1].iter().all(|v| v.is_finite())
            && x.0.1 > x.0.0
            && x.1.1 > x.1.0;
        let ((u0, u1), (v0, v1)) = if ok { x } else { s.param_box() };
        // Within the surface's own range where it has one (a B-spline evaluated past its ends
        // is a continuation the face isn't on).
        let ((pu0, pu1), (pv0, pv1)) = s.param_box();
        let clamp = |(a, b): (f64, f64), (lo, hi): (f64, f64), periodic: bool| {
            if periodic {
                (a.max(-1e4), b.min(1e4))
            } else {
                (a.max(lo).max(-1e4), b.min(hi).min(1e4))
            }
        };
        let (u0, u1) = clamp((u0, u1), (pu0, pu1), s.u_periodic());
        let (v0, v1) = clamp((v0, v1), (pv0, pv1), s.v_periodic());
        UvBox::new((u0, u1), (v0, v1))
    };
    // One surface lying all on the other (a copy with another domain, or another form of it):
    // no crossing to search for, only the shared region's boundary.
    let tc = coincidence_tol(bb);
    if super::ssi_coincident::lies_on(a, ua, b, tc) || super::ssi_coincident::lies_on(b, ub, a, tc)
    {
        return Ssi {
            coincident: super::ssi_coincident::region_boundary(a, ua, b, ub, tc),
            ..Ssi::default()
        };
    }
    let (box_a, box_b) = (uvbox(a, ua), uvbox(b, ub));
    let (res, stats) = intersect_stats(a, box_a, b, box_b, bb);
    let mut work = (stats.pairs + res.curves.iter().map(|c| c.pts.len()).sum::<usize>()) as u64;
    let (ta, tb) = ((box_a.u, box_a.v), (box_b.u, box_b.v));
    let res_singular: Vec<P3> = res.singular.iter().map(|q| q.p).collect();
    let h = bb.diagonal().max(tol::ROUNDOFF) / 96.0;
    let mut polys: Vec<(Vec<super::ssi_numeric::Pt>, bool)> = Vec::new();
    let mut tangents: Vec<(Vec<P3>, bool)> = Vec::new();
    for c in res.curves {
        if c.tangent {
            tangents.push((c.pts.iter().map(|p| p.p).collect(), c.closed));
        } else {
            polys.push((c.pts, c.closed));
        }
    }
    // A side of either box lying on the other surface, the two crossing there: the intersection
    // runs along it exactly, where a march stays inside the box only by rounding (and so comes
    // in pieces). The side is the curve; traced points along it go.
    let mut side_curves: Vec<IntCurve> = Vec::new();
    let mut side_ends: Vec<P3> = Vec::new();
    if !res.coincident {
        let (sides, w) = super::ssi_coincident::crossing_sides(a, ta, b, tb, tc);
        work += w;
        if !sides.is_empty() {
            let near = (10.0 * tc).max(1e-7 * bb.diagonal());
            polys = off_sides(polys, &sides, a, b, near, &mut side_ends);
            side_curves = sides.into_iter().map(|s| s.curve).collect();
        }
    }
    // A curve traced twice (from seeds the first march didn't pass): the copy goes.
    let on = |q: &super::ssi_numeric::Pt, line: &[super::ssi_numeric::Pt], closed: bool| {
        let n = line.len();
        (0..if closed { n } else { n - 1 })
            .any(|k| seg_dist3(&q.p, &line[k].p, &line[(k + 1) % n].p) < h * 0.25)
    };
    let mut keep = vec![true; polys.len()];
    for j in 0..polys.len() {
        for i in 0..j {
            if keep[i]
                && polys[i].0.len() >= 2
                && polys[j].0.iter().all(|q| on(q, &polys[i].0, polys[i].1))
            {
                keep[j] = false;
                break;
            }
        }
    }
    let mut k = 0;
    polys.retain(|_| {
        k += 1;
        keep[k - 1]
    });
    let dbg = std::env::var("CADRS_SSI_DEBUG").is_ok();
    if dbg {
        eprintln!(
            "numeric: {} polylines before joining: {:?}",
            polys.len(),
            polys
                .iter()
                .map(|(p, c)| (p.len(), *c, p[0].p, p[p.len() - 1].p))
                .collect::<Vec<_>>()
        );
    }
    join_polylines(&mut polys, h * 1e-3, |q| q.p);
    // Crossings on both sides of a periodic seam are one point in space: a polyline made only of
    // such (all its points at one place) is no curve.
    polys.retain(|(p, _)| p.iter().any(|q| (q.p - p[0].p).norm() > h * 1e-3));
    if dbg {
        eprintln!(
            "numeric: {} after: {:?}",
            polys.len(),
            polys.iter().map(|(p, c)| (p.len(), *c)).collect::<Vec<_>>()
        );
    }
    let mut curves: Vec<IntCurve> = polys
        .into_iter()
        .filter(|(p, _)| p.len() >= 2)
        .map(|(p, closed)| fit_curve(a, b, p, closed, &side_ends))
        .collect();
    curves.extend(side_curves);
    let tangent: Vec<IntCurve> = tangents
        .into_iter()
        .filter_map(|(p, closed)| fit_points(p, closed))
        .collect();
    // Where the surfaces coincide over an area: its boundary, the sides of either box on the
    // other surface and where the two part inside a's box.
    let coincident = if res.coincident {
        // (Within ten times the coincidence tolerance: surfaces made to coincide often differ by
        // about that much along their sides, and a region's edge traced at their noise is
        // noise.)
        let tr = 10.0 * tc;
        let mut c = super::ssi_coincident::region_boundary(a, ta, b, tb, tr);
        let (inner, w) = super::ssi_coincident::interior_boundary(a, ta, b, tb, tr, &c);
        work += w;
        c.extend(inner);
        c
    } else {
        Vec::new()
    };
    // (Where the surfaces coincide, touching points chain along the region's edge: its
    // boundary stands for them.)
    let tangent = if res.coincident { Vec::new() } else { tangent };
    let points = res.points.iter().map(|q| q.p).collect();
    Ssi {
        curves,
        tangent,
        singular: res_singular,
        coincident,
        points,
        work,
    }
}

/// The traced polylines without their points along the sides (each within `near` of its side):
/// the runs between, each ended on the side where it leaves it (those ends added to `ends`). A
/// run that stays within twenty times `near` of a side all along is the side's rounding, not a
/// branch: it goes too.
fn off_sides(
    polys: Vec<(Vec<super::ssi_numeric::Pt>, bool)>,
    sides: &[super::ssi_coincident::Side],
    a: &Surface,
    b: &Surface,
    near: f64,
    ends: &mut Vec<P3>,
) -> Vec<(Vec<super::ssi_numeric::Pt>, bool)> {
    use super::ssi_numeric::Pt;
    // The nearest side's point at the same parameter along it, and how far.
    let to_side = |q: &Pt| -> Option<(f64, Pt)> {
        sides
            .iter()
            .filter_map(|sd| {
                let (s, o, uv, ouv) = if sd.on_a {
                    (a, b, q.a, q.b)
                } else {
                    (b, a, q.b, q.a)
                };
                let (along, periodic) = if sd.fix_u {
                    (uv.1, s.v_periodic())
                } else {
                    (uv.0, s.u_periodic())
                };
                let (lo, hi) = (sd.range.0.min(sd.range.1), sd.range.0.max(sd.range.1));
                let e = tol::DIFF_STEP * (hi - lo);
                let t = if periodic {
                    wrap(along, lo - e, TAU)
                } else {
                    along
                };
                if t < lo - e || t > hi + e {
                    return None;
                }
                let t = t.clamp(lo, hi);
                let own = if sd.fix_u { (sd.val, t) } else { (t, sd.val) };
                let p = s.eval(own.0, own.1);
                let other = o.project(&p, Some(ouv));
                let pt = if sd.on_a {
                    Pt {
                        p,
                        a: own,
                        b: other,
                    }
                } else {
                    Pt {
                        p,
                        a: other,
                        b: own,
                    }
                };
                Some(((p - q.p).norm(), pt))
            })
            .min_by(|x, y| x.0.total_cmp(&y.0))
    };
    let mut out = Vec::new();
    for (mut pts, closed) in polys {
        let near_side: Vec<Option<(f64, Pt)>> = pts.iter().map(to_side).collect();
        let mut flags: Vec<bool> = near_side
            .iter()
            .map(|x| x.as_ref().is_some_and(|x| x.0 <= near))
            .collect();
        if !flags.iter().any(|&f| f) {
            out.push((pts, closed));
            continue;
        }
        let mut feet: Vec<Option<Pt>> = near_side.iter().map(|x| x.as_ref().map(|x| x.1)).collect();
        let mut dist: Vec<f64> = near_side
            .iter()
            .map(|x| x.as_ref().map_or(f64::INFINITY, |x| x.0))
            .collect();
        if closed {
            // Opened at a point on a side (and back to it).
            let k = flags.iter().position(|&f| f).unwrap_or(0);
            pts.rotate_left(k);
            flags.rotate_left(k);
            feet.rotate_left(k);
            dist.rotate_left(k);
            pts.push(pts[0]);
            flags.push(true);
            feet.push(feet[0]);
            dist.push(dist[0]);
        }
        let n = pts.len();
        let mut k = 0;
        while k < n {
            if flags[k] {
                k += 1;
                continue;
            }
            let mut e = k;
            while e + 1 < n && !flags[e + 1] {
                e += 1;
            }
            if dist[k..=e].iter().any(|&d| d > 20.0 * near) {
                // Where the run meets the side the two branches come together (the surfaces
                // touch there) and its last points, within twenty times `near`, may follow
                // either: the run ends at the side's point by its last point beyond them.
                let (mut s0, mut s1) = (k, e);
                if k > 0 {
                    while s0 < s1 && dist[s0] <= 20.0 * near {
                        s0 += 1;
                    }
                }
                if e + 1 < n {
                    while s1 > s0 && dist[s1] <= 20.0 * near {
                        s1 -= 1;
                    }
                }
                let mut run: Vec<Pt> = Vec::with_capacity(s1 - s0 + 3);
                if k > 0 {
                    run.extend(feet[s0].or(feet[k - 1]));
                }
                run.extend_from_slice(&pts[s0..=s1]);
                if e + 1 < n {
                    run.extend(feet[s1].or(feet[e + 1]));
                }
                for (i, j) in [(k > 0, 0), (e + 1 < n, run.len() - 1)] {
                    if i {
                        ends.push(run[j].p);
                    }
                }
                if run.len() >= 2 {
                    out.push((run, false));
                }
            }
            k = e + 1;
        }
    }
    out
}

/// A cubic B-spline through points as they are (a tangent curve's touching points, which
/// refinement can't sharpen: the surfaces' distance has no sign change there).
fn fit_points(mut pts: Vec<P3>, closed: bool) -> Option<IntCurve> {
    if closed {
        pts.push(pts[0]);
    }
    pts.dedup_by(|x, y| (*x - *y).norm() < tol::ROUNDOFF);
    if pts.len() < 2 {
        return None;
    }
    let t = chord(&pts);
    let c = NurbsCurve::interpolate(&pts, &t, 3.min(pts.len() - 1), None, None);
    let n = c.range();
    Some(IntCurve {
        c: Curve::Nurbs(Arc::new(c)),
        t0: n.0,
        t1: n.1,
        closed,
        pcs: None,
    })
}

/// A point on both surfaces near a's (u, v): Newton on a's parameters for sd_b = 0, with the
/// point's (u, v) on `a`.
pub fn refine_point_uv(a: &Surface, b: &Surface, uv: P2) -> (P3, P2) {
    let (mut u, mut v) = (uv.x, uv.y);
    let e = 1e-7;
    let mut hint: Option<(f64, f64)> = if b.signed_distance(&P3::origin()).is_some() {
        None
    } else {
        Some(b.project(&a.eval(u, v), None))
    };
    for _ in 0..30 {
        let p = a.eval(u, v);
        let g0 = sdist_hint(b, &p, &mut hint);
        if g0.abs() < 1e-13 {
            break;
        }
        let (_, su, sv) = a.d1(u, v);
        // ∇g ≈ n_b·(Su, Sv) with n_b the distance gradient.
        let mut h1 = hint;
        let mut h2 = hint;
        let mut h3 = hint;
        let mut h4 = hint;
        let gu = (sdist_hint(b, &(p + su * e), &mut h1) - sdist_hint(b, &(p - su * e), &mut h2))
            / (2.0 * e);
        let gv = (sdist_hint(b, &(p + sv * e), &mut h3) - sdist_hint(b, &(p - sv * e), &mut h4))
            / (2.0 * e);
        let l = gu * gu + gv * gv;
        if l < tol::TINY {
            break;
        }
        u -= g0 * gu / l;
        v -= g0 * gv / l;
    }
    (a.eval(u, v), P2::new(u, v))
}

/// Distance from p to the segment ab.
fn seg_dist3(p: &P3, a: &P3, b: &P3) -> f64 {
    let d = b - a;
    let l = d.norm_squared();
    let t = if l < tol::TINY {
        0.0
    } else {
        ((p - a).dot(&d) / l).clamp(0.0, 1.0)
    };
    (a + d * t - p).norm()
}

/// Joins polylines whose ends meet (within `tol`; `at` gives a point's position), and closes
/// those whose own ends meet.
fn join_polylines<T: Clone>(polys: &mut Vec<(Vec<T>, bool)>, tol: f64, at: impl Fn(&T) -> P3) {
    let mut changed = true;
    while changed {
        changed = false;
        'outer: for i in 0..polys.len() {
            if polys[i].1 {
                continue;
            }
            for j in 0..polys.len() {
                if i == j || polys[j].1 {
                    continue;
                }
                let (Some(ai), Some(bi), Some(aj), Some(bj)) = (
                    polys[i].0.first().map(&at),
                    polys[i].0.last().map(&at),
                    polys[j].0.first().map(&at),
                    polys[j].0.last().map(&at),
                ) else {
                    continue;
                };
                // The direction a polyline leaves by at its end (or start): two pieces join only
                // where they run on smoothly (not at a kink: two generators of a cone through
                // its apex are two curves).
                let out_dir = |p: &[T], end: bool| -> V3 {
                    let n = p.len();
                    if n < 2 {
                        return V3::zeros();
                    }
                    let (x, y) = if end {
                        (at(&p[n - 2]), at(&p[n - 1]))
                    } else {
                        (at(&p[1]), at(&p[0]))
                    };
                    (y - x).try_normalize(tol::TINY).unwrap_or_else(V3::zeros)
                };
                let smooth = |ei: bool, ej: bool| {
                    out_dir(&polys[i].0, ei).dot(&out_dir(&polys[j].0, ej)) < -0.85
                };
                let mut pj = polys[j].0.clone();
                let mut pi = polys[i].0.clone();
                let (bi_aj, bi_bj, ai_bj, ai_aj) = (
                    (bi - aj).norm() < tol && smooth(true, false),
                    (bi - bj).norm() < tol && smooth(true, true),
                    (ai - bj).norm() < tol && smooth(false, true),
                    (ai - aj).norm() < tol && smooth(false, false),
                );
                let joined = if bi_aj {
                    pi.extend(pj.drain(1..));
                    true
                } else if bi_bj {
                    pj.reverse();
                    pi.extend(pj.drain(1..));
                    true
                } else if ai_bj {
                    pj.extend(pi.drain(1..));
                    pi = pj.clone();
                    true
                } else if ai_aj {
                    pj.reverse();
                    pj.extend(pi.drain(1..));
                    pi = pj.clone();
                    true
                } else {
                    false
                };
                if joined {
                    let closed = pi.len() > 2 && (at(&pi[0]) - at(&pi[pi.len() - 1])).norm() < tol;
                    if closed {
                        pi.pop();
                    }
                    polys[i] = (pi, closed);
                    polys.remove(j);
                    changed = true;
                    break 'outer;
                }
            }
        }
        // Close single polylines whose ends meet.
        for p in polys.iter_mut() {
            if !p.1 && p.0.len() > 2 && (at(&p.0[0]) - at(&p.0[p.0.len() - 1])).norm() < tol {
                p.0.pop();
                p.1 = true;
            }
        }
    }
}

/// Fits a cubic B-spline through exact points of a traced curve, adding points of both
/// surfaces (refined on the plane square to the chord) where the fit strays from the
/// intersection by more than 1e-9 (relative on a big curve). An end among `fixed` (where the
/// curve meets a side the intersection runs along: two branches there, a refined point may
/// land on either) keeps its first span as it is.
fn fit_curve(
    a: &Surface,
    b: &Surface,
    mut pts: Vec<super::ssi_numeric::Pt>,
    closed: bool,
    fixed: &[P3],
) -> IntCurve {
    if closed {
        pts.push(pts[0]);
    }
    pts.dedup_by(|x, y| (x.p - y.p).norm() < tol::ROUNDOFF);
    // Angles continuous along the curve (joined pieces may differ by turns), so that the
    // parameters of neighbours average to a start between them.
    for k in 1..pts.len() {
        let prev = pts[k - 1];
        let q = &mut pts[k];
        if a.u_periodic() {
            q.a.0 = unwrap_near(q.a.0, prev.a.0, TAU);
        }
        if a.v_periodic() {
            q.a.1 = unwrap_near(q.a.1, prev.a.1, TAU);
        }
        if b.u_periodic() {
            q.b.0 = unwrap_near(q.b.0, prev.b.0, TAU);
        }
        if b.v_periodic() {
            q.b.1 = unwrap_near(q.b.1, prev.b.1, TAU);
        }
    }
    let fit = |pts: &[super::ssi_numeric::Pt]| {
        let xs: Vec<P3> = pts.iter().map(|q| q.p).collect();
        let t = chord(&xs);
        (
            NurbsCurve::interpolate(&xs, &t, 3.min(xs.len() - 1), None, None),
            t,
        )
    };
    // How near the fit keeps to the intersection: 1e-9, or 2e-10 of the curve's size on a big
    // model (a fit to 1e-9 over a metre takes thousands of points, all carried downstream).
    let tol = {
        let mut bb = Bbox::empty();
        pts.iter().for_each(|q| bb.add(&q.p));
        1e-9f64.max(2e-10 * bb.diagonal())
    };
    let is_fixed = |q: &super::ssi_numeric::Pt| fixed.contains(&q.p);
    let fixed_ends = [
        !closed && pts.first().is_some_and(is_fixed),
        !closed && pts.last().is_some_and(is_fixed),
    ];
    for _round in 0..6 {
        let (c, t) = fit(&pts);
        let last = pts.len() - 2;
        let found: Vec<Option<(usize, super::ssi_numeric::Pt)>> = (0..pts.len() - 1)
            .into_par_iter()
            .map(|k| {
                if (k == 0 && fixed_ends[0]) || (k == last && fixed_ends[1]) {
                    return None;
                }
                let q = c.point((t[k] + t[k + 1]) / 2.0);
                let chord = (pts[k + 1].p - pts[k].p).norm();
                let exact = super::ssi_numeric::refine_between(a, b, &pts[k], &pts[k + 1], &q)?;
                let off = (exact.p - q).norm();
                (off > tol && chord > tol::POINT && off < 0.5 * chord).then_some((k + 1, exact))
            })
            .collect();
        let ins: Vec<(usize, super::ssi_numeric::Pt)> = found.into_iter().flatten().collect();
        if std::env::var_os("CADRS_SSI_DEBUG").is_some() {
            eprintln!(
                "fit round {_round}: {} points, {} to insert",
                pts.len(),
                ins.len()
            );
        }
        if ins.is_empty() || pts.len() > 20000 {
            let n = c.range();
            let pcs = uv_fits(a, b, &pts, &t, &c, 4.0 * tol);
            return IntCurve {
                c: Curve::Nurbs(Arc::new(c)),
                t0: n.0,
                t1: n.1,
                closed,
                pcs,
            };
        }
        for (k, p) in ins.into_iter().rev() {
            pts.insert(k, p);
        }
    }
    let (c, t) = fit(&pts);
    let n = c.range();
    let pcs = uv_fits(a, b, &pts, &t, &c, 4.0 * tol);
    IntCurve {
        c: Curve::Nurbs(Arc::new(c)),
        t0: n.0,
        t1: n.1,
        closed,
        pcs,
    }
}

/// The p-curves of a fitted intersection curve `c` on `a` and `b`: the tracer's (u, v) on each
/// interpolated with the 3D points' parameters `t` (one knot vector for all three, so they share
/// the curve's parameter; research/algorithms/intersection_solvers.md card 1, OCCT's
/// `GeomInt_WLApprox`). Each is kept when its surface point stays within `tol` of the curve
/// midway along every span, and none runs through a pole of its surface (where the tracer's
/// free coordinate is arbitrary); else None for it (its consumers project).
fn uv_fits(
    a: &Surface,
    b: &Surface,
    pts: &[super::ssi_numeric::Pt],
    t: &[f64],
    c: &NurbsCurve,
    tol: f64,
) -> Option<Arc<[Option<Curve>; 2]>> {
    if pts.len() < 2 {
        return None;
    }
    let deg = 3.min(pts.len() - 1);
    let one = |s: &Surface, uv: &dyn Fn(&super::ssi_numeric::Pt) -> (f64, f64)| -> Option<Curve> {
        let qs: Vec<P3> = pts
            .iter()
            .map(|q| {
                let (u, v) = uv(q);
                P3::new(u, v, 0.0)
            })
            .collect();
        // (At a pole or an apex the surface's derivatives along one direction vanish.)
        let pole = qs.iter().any(|q| {
            let (_, su, sv) = s.d1(q.x, q.y);
            su.norm() < tol::DEGENERATE || sv.norm() < tol::DEGENERATE
        });
        if pole {
            return None;
        }
        let pc = NurbsCurve::interpolate(&qs, t, deg, None, None);
        let ok = t.windows(2).all(|w| {
            let tm = 0.5 * (w[0] + w[1]);
            let q = pc.point(tm);
            (s.eval(q.x, q.y) - c.point(tm)).norm() <= tol
        });
        ok.then(|| Curve::Nurbs(Arc::new(pc)))
    };
    let pcs = [one(a, &|q| q.a), one(b, &|q| q.b)];
    pcs.iter().any(Option::is_some).then(|| Arc::new(pcs))
}

fn chord(pts: &[P3]) -> Vec<f64> {
    let mut t = vec![0.0];
    for w in pts.windows(2) {
        t.push(t.last().unwrap() + (w[1] - w[0]).norm());
    }
    t
}

/// Parameters in [t0, t1] where the curve meets the surface (sign changes of the distance,
/// and touching points), refined.
pub fn curve_surface(c: &Curve, t0: f64, t1: f64, s: &Surface) -> Vec<f64> {
    // A circle or an ellipse against a plane: A cos t + B sin t + C = 0, tangency included.
    if let (Curve::Circle { .. } | Curve::Ellipse { .. }, Surface::Plane { f }) = (c, s) {
        let n = f.x.cross(&f.y);
        let (cc, ax, ay) = match c {
            Curve::Circle { c, x, y, r } => (*c, x * *r, y * *r),
            Curve::Ellipse { c, x, y, a, b } => (*c, x * *a, y * *b),
            _ => unreachable!(),
        };
        let (a, b, k) = (n.dot(&ax), n.dot(&ay), n.dot(&(cc - f.o)));
        let r = (a * a + b * b).sqrt();
        if r < 1e-14 {
            return vec![]; // in a plane parallel to it
        }
        let phi = b.atan2(a);
        let q = -k / r;
        let mut ts = Vec::new();
        if q.abs() > 1.0 + tol::ANGULAR_FINE {
            return ts;
        }
        if (q.abs() - 1.0).abs() <= tol::ANGULAR_FINE * 4.0 || (1.0 - q * q) * r * r < 1e-20 {
            ts.push(if q > 0.0 { phi } else { phi + PI });
        } else {
            let d = q.clamp(-1.0, 1.0).acos();
            ts.push(phi + d);
            ts.push(phi - d);
        }
        let mut out: Vec<f64> = Vec::new();
        for t in ts {
            for k in -2..=2 {
                let tt = t + TAU * k as f64;
                if tt >= t0 - tol::ROUNDOFF_FINE
                    && tt <= t1 + tol::ROUNDOFF_FINE
                    && !out.iter().any(|x| (x - tt).abs() < tol::ROUNDOFF_FINE)
                {
                    out.push(tt.clamp(t0, t1));
                }
            }
        }
        out.sort_by(f64::total_cmp);
        drop_seam_twin(c, t0, t1, &mut out);
        return out;
    }
    // Exact cases.
    let mut searched: Vec<f64> = Vec::new();
    if let Curve::Line { o, d } = c {
        let ts: Vec<f64> = super::classify::ray_surface(s, o, d, t0 - 1.0, t1 + 1.0)
            .into_iter()
            .map(|(t, _, _)| t)
            .collect();
        if s.signed_distance(&P3::origin()).is_some()
            && !matches!(
                s,
                Surface::Torus { .. }
                    | Surface::Revolution { .. }
                    | Surface::Extrusion { .. }
                    | Surface::Nurbs(_)
            )
        {
            return ts
                .into_iter()
                .map(|t| touching_root(c, s, t))
                .filter(|t| *t >= t0 - tol::ROUNDOFF && *t <= t1 + tol::ROUNDOFF)
                .map(|t| t.clamp(t0, t1))
                .collect();
        }
        // On these the line's hits come from a search over the surface (refined by Newton):
        // they join the sampled sign changes below, which a few samples along a line can step
        // over (two crossings of a revolved arc's patch between two samples).
        searched = ts
            .into_iter()
            .filter(|t| {
                *t >= t0 - tol::ROUNDOFF
                    && *t <= t1 + tol::ROUNDOFF
                    && s.distance(&c.point(*t)) <= tol::CONFUSION
            })
            .map(|t| t.clamp(t0, t1))
            .collect();
    }
    let n = match c {
        Curve::Line { .. } => 8,
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            (((t1 - t0) / (PI / 32.0)).ceil() as usize).max(8)
        }
        Curve::Nurbs(nc) => (nc.cps.len() * 8).clamp(16, 2000),
    };
    // The signed distance, its projection hinted (on B-spline and other non-implicit surfaces
    // a fresh projection is a global search): along the samples from the previous one, inside
    // a bracket from the bracket's own sample.
    let implicit = s.signed_distance(&P3::origin()).is_some();
    let fh = |t: f64, hint: &mut Option<(f64, f64)>| -> f64 {
        if implicit {
            sdist(s, &c.point(t))
        } else {
            sdist_hint(s, &c.point(t), hint)
        }
    };
    let mut out: Vec<f64> = Vec::new();
    let mut hints: Vec<Option<(f64, f64)>> = Vec::with_capacity(n + 1);
    let mut h: Option<(f64, f64)> = None;
    let mut vals = Vec::with_capacity(n + 1);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let v = fh(t, &mut h);
        vals.push((t, v));
        hints.push(h);
    }
    // On an exact distance (1-Lipschitz in space), two samples of one sign can still have a
    // crossing pair between them where the curve runs no farther between them than their
    // distances add up to: the samples refined there, halving, until that is ruled out (a
    // sphere's meridian 50 long through a cylinder of radius 1.25 it barely clips: two
    // crossings 1.4 apart, between samples 4.9 apart).
    if implicit {
        let mut refined: Vec<(f64, f64)> = Vec::with_capacity(vals.len());
        // (Intervals to look into: their end samples (t, distance) and depth.)
        type Sample = (f64, f64);
        let mut stack: Vec<(Sample, Sample, u32)> = Vec::new();
        // (A budget: a curve along the surface is near it everywhere, its stretches found below.)
        let mut budget = 4096usize;
        for w in vals.windows(2) {
            refined.push(w[0]);
            stack.push((w[0], w[1], 0));
            let mut extra: Vec<(f64, f64)> = Vec::new();
            while let Some((a, b, depth)) = stack.pop() {
                let reach = (c.point(b.0) - c.point(a.0)).norm() * 1.1;
                // (Not near a touch: both samples well off the surface; a midpoint that comes
                // near it is the touching search's below.)
                let near = 1e-6 * (1.0 + c.point(a.0).coords.norm());
                if budget == 0
                    || a.1.signum() != b.1.signum()
                    || a.1.abs() + b.1.abs() > reach
                    || a.1.abs().min(b.1.abs()) < near
                    || depth >= 24
                    || reach < tol::ROUNDOFF
                {
                    continue;
                }
                budget -= 1;
                let m = 0.5 * (a.0 + b.0);
                let mid = (m, sdist(s, &c.point(m)));
                extra.push(mid);
                stack.push((a, mid, depth + 1));
                stack.push((mid, b, depth + 1));
            }
            extra.sort_by(|x, y| x.0.total_cmp(&y.0));
            refined.extend(extra);
        }
        if let Some(&last) = vals.last() {
            refined.push(last);
        }
        vals = refined;
    }
    let mut prev: (f64, f64);
    let f = |t: f64| -> f64 {
        // Hinted from the nearest sample.
        let i = (((t - t0) / (t1 - t0)) * n as f64)
            .round()
            .clamp(0.0, n as f64) as usize;
        let mut hh = hints[i];
        fh(t, &mut hh)
    };
    for i in 0..vals.len() {
        let (t, ft) = vals[i];
        // A sample on the surface, unless it is inside the zone round a point of touch (the
        // samples either side off it on one side): the touch's minimum is found below, and the
        // sample would be a second vertex a hair from it.
        let touch_zone = i > 0
            && i + 1 < vals.len()
            && vals[i - 1].1.abs() >= 1e-10
            && vals[i + 1].1.abs() >= 1e-10
            && vals[i - 1].1.signum() == vals[i + 1].1.signum();
        if ft.abs() < 1e-10 && !touch_zone {
            out.push(t);
        }
        if i > 0 {
            prev = vals[i - 1];
            if prev.1.abs() >= 1e-10 && ft.abs() >= 1e-10 && prev.1.signum() != ft.signum() {
                let (mut a, mut b, mut fa) = (prev.0, t, prev.1);
                for _ in 0..100 {
                    let m = 0.5 * (a + b);
                    let fm = f(m);
                    if fm == 0.0 {
                        a = m;
                        b = m;
                        break;
                    }
                    if fm.signum() == fa.signum() {
                        a = m;
                        fa = fm;
                    } else {
                        b = m;
                    }
                    if (b - a).abs() < 1e-15 * (1.0 + a.abs()) {
                        break;
                    }
                }
                out.push(0.5 * (a + b));
            }
        }
        // Touching: a local minimum of |f| near zero between samples.
        if i > 0 && i + 1 < vals.len() {
            let (fa, fb, fc) = (vals[i - 1].1.abs(), ft.abs(), vals[i + 1].1.abs());
            if fb < fa && fb < fc && fb < 1e-3 && vals[i - 1].1.signum() == vals[i + 1].1.signum() {
                // Golden-section minimize |f|.
                let (mut a, mut b) = (vals[i - 1].0, vals[i + 1].0);
                for _ in 0..80 {
                    let m1 = a + (b - a) * 0.382;
                    let m2 = a + (b - a) * 0.618;
                    if f(m1).abs() < f(m2).abs() {
                        b = m2;
                    } else {
                        a = m1;
                    }
                }
                let m = 0.5 * (a + b);
                if f(m).abs() < tol::ROUNDOFF {
                    out.push(m);
                }
            }
        }
    }
    for t in searched {
        let p = c.point(t);
        if !out
            .iter()
            .any(|&x| (c.point(x) - p).norm() <= tol::CONFUSION)
        {
            out.push(t);
        }
    }
    // Stretches where the curve lies on the surface (three samples or more in a row within a
    // hair of it: an edge along a face it is tangent to): the distance's noise there gives no
    // crossings, only the stretch's ends do.
    let scale = 1.0
        + vals
            .iter()
            .map(|(t, _)| c.point(*t).coords.norm())
            .fold(0.0, f64::max);
    let on: Vec<bool> = vals.iter().map(|(_, v)| v.abs() <= 1e-8 * scale).collect();
    let mut i = 0;
    while i < on.len() {
        if !on[i] {
            i += 1;
            continue;
        }
        let mut e = i;
        while e + 1 < on.len() && on[e + 1] {
            e += 1;
        }
        if e >= i + 2 {
            let (lo, hi) = (vals[i].0, vals[e].0);
            out.retain(|t| *t <= lo || *t >= hi);
            out.extend([lo, hi]);
        }
        i = e + 1;
    }
    out.sort_by(f64::total_cmp);
    out.dedup_by(|a, b| (*a - *b).abs() < tol::ROUNDOFF);
    drop_seam_twin(c, t0, t1, &mut out);
    out
}

/// On a curve closed over [t0, t1] (a full circle), a crossing at its seam is one point: found
/// at both t0 and t1, the copy at t1 goes.
fn drop_seam_twin(c: &Curve, t0: f64, t1: f64, out: &mut Vec<f64>) {
    let (Some(&first), Some(&last)) = (out.first(), out.last()) else {
        return;
    };
    let span = (t1 - t0).abs();
    if out.len() > 1
        && (first - t0).abs() <= tol::ROUNDOFF * (1.0 + span)
        && (last - t1).abs() <= tol::ROUNDOFF * (1.0 + span)
        && (c.point(t0) - c.point(t1)).norm() <= tol::ROUNDOFF * (1.0 + c.point(t0).coords.norm())
    {
        out.pop();
    }
}

/// A root `t` of a line meeting a surface, polished where the line touches it: a double root of
/// the closed form is only good to the square root of the rounding error (a line tangent to a
/// cylinder: 3e-8 along it), while the touching point is where the signed distance is
/// stationary, found by Newton on its derivative.
fn touching_root(c: &Curve, s: &Surface, t: f64) -> f64 {
    let f = |x: f64| s.signed_distance(&c.point(x));
    if f(t).is_none() {
        return t;
    }
    let speed = c.d2(t).1.norm().max(tol::TINY);
    let h = 1e-5 / speed;
    let slope = |x: f64| (f(x + h).unwrap_or(0.0) - f(x - h).unwrap_or(0.0)) / (2.0 * h);
    if slope(t).abs() > 1e-3 * speed {
        return t;
    }
    let mut x = t;
    for _ in 0..20 {
        let g = slope(x);
        let g2 = (f(x + h).unwrap_or(0.0) - 2.0 * f(x).unwrap_or(0.0) + f(x - h).unwrap_or(0.0))
            / (h * h);
        if g2.abs() < tol::TINY {
            break;
        }
        let dx = -g / g2;
        x += dx;
        if dx.abs() < 1e-15 * (1.0 + x.abs()) {
            break;
        }
    }
    let (ft, fx) = (
        f(t).unwrap_or(f64::INFINITY).abs(),
        f(x).unwrap_or(f64::INFINITY).abs(),
    );
    if x.is_finite() && (x - t).abs() * speed < tol::POINT && fx <= ft + tol::ROUNDOFF_FINE {
        x
    } else {
        t
    }
}

/// Points where two bounded curves meet (within `tol`): (t on a, t on b). Lines and circles in
/// closed form; when those find nothing (a near miss off the plane or past an end, or an
/// overlap: its ends), and for other curves, by subdivision ([`super::ssi_cci`]).
pub fn curve_curve(
    a: &Curve,
    a0: f64,
    a1: f64,
    b: &Curve,
    b0: f64,
    b1: f64,
    tol: f64,
) -> Vec<(f64, f64)> {
    let closed_form = |c: &Curve| matches!(c, Curve::Line { .. } | Curve::Circle { .. });
    if closed_form(a) && closed_form(b) {
        let r = curve_curve_closed(a, a0, a1, b, b0, b1, tol);
        if !r.is_empty() {
            return r;
        }
        // Two segments nowhere within the tolerance of each other: nothing for the general
        // search to find either (it ran for every pair of a boolean's edges that missed).
        if let (Curve::Line { o: oa, d: da }, Curve::Line { o: ob, d: db }) = (a, b) {
            if segment_gap((oa, da, a0, a1), (ob, db, b0, b1)) > 2.0 * tol {
                return Vec::new();
            }
            // Else they come closest at an end of one (parallel, or crossing past an end): the
            // ends within the tolerance of the other, as the general search ends with (after
            // halving the pieces about them down to a millionth: milliseconds a pair, for lines
            // meeting at a shallow angle).
            if da.norm_squared() > 0.0 && db.norm_squared() > 0.0 {
                return overlap_ends(a, (a0, a1), b, (b0, b1), tol);
            }
        }
    }
    super::ssi_cci::curve_curve(a, a0, a1, b, b0, b1, tol)
}

/// Where two segments come within `tol` at an end: each one's ends lying on the other, one point to
/// each place, along `a`.
fn overlap_ends(a: &Curve, ra: (f64, f64), b: &Curve, rb: (f64, f64), tol: f64) -> Vec<(f64, f64)> {
    let (ra, rb) = (
        (ra.0.min(ra.1), ra.0.max(ra.1)),
        (rb.0.min(rb.1), rb.0.max(rb.1)),
    );
    let mut out: Vec<(f64, f64)> = Vec::new();
    for ta in [ra.0, ra.1] {
        let p = a.point(ta);
        let tb = b.closest(&p, rb.0, rb.1);
        if (b.point(tb) - p).norm() <= tol {
            out.push((ta, tb));
        }
    }
    for tb in [rb.0, rb.1] {
        let q = b.point(tb);
        let ta = a.closest(&q, ra.0, ra.1);
        if (a.point(ta) - q).norm() <= tol {
            out.push((ta, tb));
        }
    }
    let mut uniq: Vec<(f64, f64, P3)> = Vec::new();
    for (s, t) in out {
        let p = a.point(s);
        if !uniq.iter().any(|(_, _, q)| (q - p).norm() <= tol) {
            uniq.push((s, t, p));
        }
    }
    uniq.sort_by(|x, y| x.0.total_cmp(&y.0));
    uniq.into_iter().map(|(s, t, _)| (s, t)).collect()
}

/// The least distance between two segments, each a line's `o + d·t` over `t0..t1`: their
/// lines' closest points if within both, else the least from an end of one to the other.
fn segment_gap(a: (&P3, &V3, f64, f64), b: (&P3, &V3, f64, f64)) -> f64 {
    let (a, b) = (
        (a.0, a.1, a.2.min(a.3), a.2.max(a.3)),
        (b.0, b.1, b.2.min(b.3), b.2.max(b.3)),
    );
    let at = |(o, d, _, _): (&P3, &V3, f64, f64), t: f64| o + d * t;
    // From a point to a segment.
    let to = |p: P3, (o, d, t0, t1): (&P3, &V3, f64, f64)| {
        let dd = d.dot(d);
        let t = if dd > 0.0 {
            ((p - o).dot(d) / dd).clamp(t0, t1)
        } else {
            t0
        };
        (p - (o + d * t)).norm()
    };
    let mut gap = to(at(a, a.2), b)
        .min(to(at(a, a.3), b))
        .min(to(at(b, b.2), a))
        .min(to(at(b, b.3), a));
    let (da, db) = (a.1, b.1);
    let w = a.0 - b.0;
    let (aa, bb, cc) = (da.dot(da), da.dot(db), db.dot(db));
    let den = aa * cc - bb * bb;
    if den > 1e-14 * aa * cc {
        let (dd, ee) = (da.dot(&w), db.dot(&w));
        let s = (bb * ee - cc * dd) / den;
        let t = (aa * ee - bb * dd) / den;
        if (a.2..=a.3).contains(&s) && (b.2..=b.3).contains(&t) {
            gap = gap.min((at(a, s) - at(b, t)).norm());
        }
    }
    gap
}

/// [`curve_curve`] for lines and circles in closed form.
fn curve_curve_closed(
    a: &Curve,
    a0: f64,
    a1: f64,
    b: &Curve,
    b0: f64,
    b1: f64,
    tol: f64,
) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    // Line–line in closed form.
    if let (Curve::Line { o: oa, d: da }, Curve::Line { o: ob, d: db }) = (a, b) {
        let w = oa - ob;
        let (aa, bb, cc) = (da.dot(da), da.dot(db), db.dot(db));
        let (dd, ee) = (da.dot(&w), db.dot(&w));
        let den = aa * cc - bb * bb;
        if den.abs() < 1e-14 * aa * cc {
            return out; // parallel: overlaps are found from the end points
        }
        let s = (bb * ee - cc * dd) / den;
        let t = (aa * ee - bb * dd) / den;
        if s >= a0 - tol::ROUNDOFF
            && s <= a1 + tol::ROUNDOFF
            && t >= b0 - tol::ROUNDOFF
            && t <= b1 + tol::ROUNDOFF
        {
            let (p, q) = (a.point(s), b.point(t));
            if (p - q).norm() < tol {
                out.push((s.clamp(a0, a1), t.clamp(b0, b1)));
            }
        }
        return out;
    }
    // A line and a circle in closed form (either order).
    let line_circle =
        |l: &Curve, l0: f64, l1: f64, c: &Curve, c0: f64, c1: f64| -> Option<Vec<(f64, f64)>> {
            let (Curve::Line { o, d }, Curve::Circle { c: cc, x, y, r }) = (l, c) else {
                return None;
            };
            let n = x.cross(y);
            let dd = d.norm();
            let du = d / dd;
            let mut hits: Vec<P3> = Vec::new();
            if du.dot(&n).abs() < tol::ANGULAR_FINE {
                // In a plane parallel to the circle's: only meets it in the circle's plane.
                if (o - cc).dot(&n).abs() > tol {
                    return Some(vec![]);
                }
                let w = o - cc;
                let b = w.dot(&du);
                let disc = b * b - (w.norm_squared() - r * r);
                if disc < -2.0 * r * tol {
                    return Some(vec![]);
                }
                if disc.abs() <= 2.0 * r * tol * 1e-3 || disc <= 0.0 {
                    hits.push(o + du * (-b));
                } else {
                    let q = disc.sqrt();
                    hits.push(o + du * (-b - q));
                    hits.push(o + du * (-b + q));
                }
            } else {
                let s = (cc - o).dot(&n) / du.dot(&n);
                hits.push(o + du * s);
            }
            let mut out = Vec::new();
            for p in hits {
                let w = p - cc;
                if (w.norm() - r).abs() > tol {
                    continue;
                }
                let tl = (p - o).dot(d) / (dd * dd);
                let ang = w.dot(y).atan2(w.dot(x));
                let tc = super::curve::clamp_periodic(ang, c0, c1);
                if tl >= l0 - tol::ROUNDOFF
                    && tl <= l1 + tol::ROUNDOFF
                    && (c.point(tc) - p).norm() < tol
                {
                    out.push((tl.clamp(l0, l1), tc));
                }
            }
            Some(out)
        };
    if let Some(r) = line_circle(a, a0, a1, b, b0, b1) {
        return r;
    }
    if let Some(r) = line_circle(b, b0, b1, a, a0, a1) {
        return r.into_iter().map(|(x, y)| (y, x)).collect();
    }
    // Two circles in closed form.
    if let (
        &Curve::Circle {
            c: ca,
            x: xa,
            y: ya,
            r: ra,
        },
        &Curve::Circle {
            c: cb,
            x: xb,
            y: yb,
            r: rb,
        },
    ) = (a, b)
    {
        let (na, nb) = (xa.cross(&ya), xb.cross(&yb));
        let mut hits: Vec<P3> = Vec::new();
        if na.cross(&nb).norm() < tol::ANGULAR_FINE {
            // Parallel planes: they meet only in one plane.
            if (cb - ca).dot(&na).abs() > tol {
                return out;
            }
            let d = cb - ca;
            let d = d - na * d.dot(&na);
            let dl = d.norm();
            if dl < tol && (ra - rb).abs() < tol {
                return out; // the same circle: overlaps are found from the end points
            }
            if dl < tol::TINY || dl > ra + rb + tol || dl < (ra - rb).abs() - tol {
                return out;
            }
            // Radical line: distance along d from ca.
            let u = d / dl;
            let x = ((dl * dl + ra * ra - rb * rb) / (2.0 * dl)).clamp(-ra, ra);
            let h2 = ra * ra - x * x;
            let w = na.cross(&u);
            if h2 <= (ra * tol * 1e-3).max(0.0) {
                hits.push(ca + u * x);
            } else {
                let h = h2.sqrt();
                hits.push(ca + u * x + w * h);
                hits.push(ca + u * x - w * h);
            }
        } else {
            // A's points on B's plane: the line where the planes meet, through circle A.
            let dir = na.cross(&nb).normalize();
            // A point on both planes.
            let (da, db) = (na.dot(&ca.coords), nb.dot(&cb.coords));
            let m =
                nalgebra::Matrix3::from_rows(&[na.transpose(), nb.transpose(), dir.transpose()]);
            let Some(inv) = m.try_inverse() else {
                return out;
            };
            let o = P3::from(inv * V3::new(da, db, dir.dot(&ca.coords)));
            let w = o - ca;
            let bq = w.dot(&dir);
            let disc = bq * bq - (w.norm_squared() - ra * ra);
            if disc < -2.0 * ra * tol {
                return out;
            }
            if disc <= 0.0 {
                hits.push(o - dir * bq);
            } else {
                let q = disc.sqrt();
                hits.push(o + dir * (-bq - q));
                hits.push(o + dir * (-bq + q));
            }
        }
        for p in hits {
            let (wa, wb) = (p - ca, p - cb);
            if (wa.norm() - ra).abs() > tol || (wb.norm() - rb).abs() > tol {
                continue;
            }
            let ta = super::curve::clamp_periodic(wa.dot(&ya).atan2(wa.dot(&xa)), a0, a1);
            let tb = super::curve::clamp_periodic(wb.dot(&yb).atan2(wb.dot(&xb)), b0, b1);
            if (a.point(ta) - p).norm() < tol && (b.point(tb) - p).norm() < tol {
                out.push((ta, tb));
            }
        }
        return out;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines the closed form misses (parallel, or crossing past an end) give what the general
    /// search gives: the same places, within the tolerance, and nothing where they're apart;
    /// besides, the end two segments on one line share (which the search, halving them about
    /// it, misses: a boolean's split there is no split).
    #[test]
    fn lines_missed_in_closed_form_as_the_general_search() {
        let line = |o: [f64; 3], d: [f64; 3]| Curve::Line {
            o: P3::new(o[0], o[1], o[2]),
            d: V3::new(d[0], d[1], d[2]).normalize(),
        };
        let tol = 5e-7;
        let cases = [
            // One on the other, overlapping over half their lengths.
            (
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
                line([5.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
            ),
            // End to end on one line.
            (
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
                line([10.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 4.0),
            ),
            // One starting a hair past the other's end, at a shallow angle.
            (
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
                line([10.0 + 1e-8, 0.0, 0.0], [1.0, 3.4e-4, 0.0]),
                (0.0, 5.0),
            ),
            // Parallel, apart.
            (
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
                line([0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
            ),
            // Crossing past an end, apart.
            (
                line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
                (0.0, 10.0),
                line([12.0, -1.0, 0.0], [0.0, 1.0, 0.0]),
                (0.0, 2.0),
            ),
        ];
        for (i, (a, (a0, a1), b, (b0, b1))) in cases.iter().enumerate() {
            let fast = curve_curve(a, *a0, *a1, b, *b0, *b1, tol);
            let slow = super::super::ssi_cci::curve_curve(a, *a0, *a1, b, *b0, *b1, tol);
            let at = |r: &[(f64, f64)]| -> Vec<P3> { r.iter().map(|&(s, _)| a.point(s)).collect() };
            let (pf, ps) = (at(&fast), at(&slow));
            // Each place of either near one of the other's (the general search may give a
            // place twice, a few micrometres apart within an overlap).
            let shared_end =
                |&(s, t): &(f64, f64)| [*a0, *a1].contains(&s) && [*b0, *b1].contains(&t);
            for (p, r) in pf.iter().zip(&fast) {
                assert!(
                    ps.iter().any(|q| (p - q).norm() < 1e-2) || shared_end(r),
                    "case {i}: {fast:?} vs {slow:?}"
                );
            }
            for q in &ps {
                assert!(
                    pf.iter().any(|p| (p - q).norm() < 1e-2),
                    "case {i}: {fast:?} vs {slow:?}"
                );
            }
            for &(s, t) in &fast {
                assert!(
                    (a.point(s) - b.point(t)).norm() <= tol,
                    "case {i}: {fast:?}"
                );
            }
        }
    }

    /// Planes through a cone's apex: tangent along a ruling (one line), through two rulings, and
    /// meeting it at the apex alone, for a cone opening up (45°) and one past 90° (−135°, as an
    /// exporter writes a countersink); the lines on both surfaces, found in closed form (the
    /// marcher took ~40 ms over the tangent one).
    #[test]
    fn plane_through_a_cone_apex() {
        let bb = Bbox {
            min: P3::new(-10.0, -10.0, -10.0),
            max: P3::new(10.0, 10.0, 10.0),
        };
        for ang in [PI / 4.0, -3.0 * PI / 4.0] {
            // Radius 2 at the frame's origin: the apex 2 along the axis the way the radius falls.
            let cone = Surface::Cone {
                f: Frame::world(),
                r: 2.0,
                a: ang,
            };
            let apex = P3::new(0.0, 0.0, -2.0 * ang.cos() / ang.sin());
            for (tilt, want) in [(PI / 4.0, 1), (0.0, 2), (0.3, 2), (1.2, 0)] {
                let n = V3::new(tilt.cos(), 0.0, tilt.sin());
                let plane = Surface::Plane {
                    f: Frame::from_z(apex, n),
                };
                let r = surface_surface_tol(
                    &plane,
                    ((-20.0, 20.0), (-20.0, 20.0)),
                    &cone,
                    ((0.0, TAU), (-20.0, 20.0)),
                    &bb,
                    tol::POINT,
                );
                let curves: Vec<&IntCurve> = r.curves.iter().chain(&r.tangent).collect();
                assert_eq!(
                    curves.len(),
                    want,
                    "angle {ang}, tilt {tilt}: {} curves",
                    curves.len()
                );
                assert_eq!(r.work, 1, "angle {ang}, tilt {tilt}: marched");
                for c in curves {
                    for i in 0..=8 {
                        let p = c.c.point(c.t0 + (c.t1 - c.t0) * i as f64 / 8.0);
                        assert!(
                            plane.distance(&p) < 1e-9 && cone.distance(&p) < 1e-9,
                            "angle {ang}, tilt {tilt}: {p:?} off"
                        );
                    }
                }
            }
        }
    }

    /// A sphere's meridian (radius 50) through a cylinder of radius 1.25 it barely clips: the
    /// two crossings 1.4 apart lie between samples 4.9 apart, of one sign (nightlight sphere: the
    /// sphere's seam edge was left whole through a hole the cylinder cut, and the hole's loop
    /// crossed the face's outer loop in (u, v)).
    #[test]
    fn circle_clipping_a_thin_cylinder() {
        let c = Curve::Circle {
            c: P3::new(0.0, 0.0, 34.0),
            x: V3::new(1.0, 0.0, 0.0),
            y: V3::new(0.0, 0.0, 1.0),
            r: 50.0,
        };
        let (x, z) = (
            V3::new(0.6156614753256583, 0.0, -0.788010753606722),
            V3::new(-0.788010753606722, 0.0, -0.6156614753256583),
        );
        let cyl = Surface::Cylinder {
            f: Frame {
                o: P3::new(44.71834918036447, -1.0471735180964408, 59.600854680724225),
                x,
                y: V3::new(0.0, -1.0, 0.0),
                z,
            },
            r: 1.25,
        };
        let ts = curve_surface(&c, 0.0, PI / 2.0, &cyl);
        assert_eq!(ts.len(), 2, "{ts:?}");
        for t in ts {
            assert!(
                cyl.distance(&c.point(t)) < 1e-9,
                "{t}: {:e} off",
                cyl.distance(&c.point(t))
            );
        }
    }

    /// A full circle crossing a plane at its seam (t = 0 and t = 2π, one point): one root, in
    /// closed form and by the search (a cylinder through the seam).
    #[test]
    fn full_circle_crossing_at_its_seam() {
        let c = Curve::Circle {
            c: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            r: 2.0,
        };
        let plane = Surface::Plane {
            f: Frame::from_z(P3::new(2.0, 0.0, 0.0), V3::x()),
        };
        assert_eq!(curve_surface(&c, 0.0, TAU, &plane), vec![0.0]);
        let cyl = Surface::Cylinder {
            f: Frame::from_z(P3::new(2.0, 0.0, 0.0), V3::z()),
            r: 0.5,
        };
        let ts = curve_surface(&c, 0.0, TAU, &cyl);
        assert_eq!(ts.len(), 2, "{ts:?}");
    }

    /// A spline band running 7e-5 off a cylinder all along (a thread's root on its tube, the
    /// sweep's approximation a hair inside): no intersection, found without cutting the pair
    /// down to the smallest cells (the distance is all but constant over the band, which its
    /// second differences show).
    #[test]
    fn band_a_hair_off_a_cylinder() {
        let f = Frame::world();
        let cyl = Surface::Cylinder { f, r: 18.375 };
        let band = Surface::Nurbs(Arc::new(
            Surface::Cylinder {
                f,
                r: 18.375 - 7e-5,
            }
            .to_nurbs((0.0, 1.2), (-3.0, -1.0)),
        ));
        let bb = Bbox {
            min: P3::new(-20.0, -20.0, -4.0),
            max: P3::new(20.0, 20.0, 0.0),
        };
        let r = surface_surface_tol(
            &band,
            ((0.0, 1.2), (-3.0, -1.0)),
            &cyl,
            ((0.0, TAU), (-5.0, 1.0)),
            &bb,
            tol::POINT,
        );
        assert!(
            r.curves.is_empty() && r.coincident.is_empty(),
            "{} curves, {} coincident",
            r.curves.len(),
            r.coincident.len()
        );
        assert!(r.work < 20_000, "{} cell pairs", r.work);
    }

    /// A spline plate pierced by a thin cylinder whose axis runs between the plate's samples:
    /// the distance to the cylinder kinks at its axis, so the samples' second differences
    /// don't bound it there, and the pair must not be culled on them (tube adapter: a small
    /// hole lost, the boolean left open).
    #[test]
    fn thin_cylinder_through_a_spline_plate() {
        let f = Frame::world();
        let plate = Surface::Nurbs(Arc::new(
            Surface::Plane { f }.to_nurbs((-10.0, 10.0), (-10.0, 10.0)),
        ));
        let bb = Bbox {
            min: P3::new(-11.0, -11.0, -2.0),
            max: P3::new(11.0, 11.0, 2.0),
        };
        for (x, y) in [
            (1.3, 0.7),
            (0.3125, 0.3125),
            (5.3125, -2.8125),
            (-0.15625, 0.46875),
        ] {
            let cyl = Surface::Cylinder {
                f: Frame {
                    o: P3::new(x, y, 0.0),
                    ..f
                },
                r: 0.02,
            };
            let r = surface_surface_tol(
                &plate,
                ((-10.0, 10.0), (-10.0, 10.0)),
                &cyl,
                ((0.0, TAU), (-1.0, 1.0)),
                &bb,
                tol::POINT,
            );
            assert_eq!(
                r.curves.len(),
                1,
                "axis at ({x}, {y}): {} curves",
                r.curves.len()
            );
        }
    }

    /// A line through a patch of a surface of revolution (no implicit distance) between two of
    /// its samples: both crossings, from the search over the surface.
    #[test]
    fn line_crosses_a_revolved_patch_twice_close_together() {
        let m = Curve::Circle {
            c: P3::new(5.0, 0.0, 0.0),
            x: V3::x(),
            y: V3::z(),
            r: 1.0,
        };
        let s = Surface::Revolution {
            f: Frame::world(),
            m,
        };
        let line = Curve::Line {
            o: P3::new(0.0, 0.0, 0.99),
            d: V3::x(),
        };
        let ts = curve_surface(&line, 0.0, 4.9, &s);
        assert_eq!(ts.len(), 1, "{ts:?}");
        let ts = curve_surface(&line, 0.0, 10.0, &s);
        let near: Vec<f64> = ts
            .iter()
            .copied()
            .filter(|t| (t - 5.0).abs() < 1.0)
            .collect();
        assert_eq!(near.len(), 2, "{ts:?}");
        for t in near {
            assert!(s.distance(&line.point(t)) < 1e-9);
        }
    }

    /// Plane sections of a cone (ellipse, parabola, hyperbola): every point of the exact conic
    /// lies on both surfaces.
    #[test]
    fn plane_cone_conics_lie_on_both() {
        let fc = Frame {
            o: P3::new(1.0, -2.0, 0.5),
            x: V3::x(),
            y: V3::y(),
            z: V3::z(),
        };
        let (r, a) = (3.0, -0.4);
        let bb = Bbox {
            min: P3::new(-40.0, -40.0, -40.0),
            max: P3::new(40.0, 40.0, 40.0),
        };
        for (n, o) in [
            (V3::new(0.2, 0.1, 1.0), P3::new(0.0, 0.0, 1.0)),
            (V3::new(0.0, 1.0, 0.0), P3::new(0.0, -1.5, 0.0)),
            (
                V3::new((PI / 2.0 - 0.4).cos(), 0.0, (PI / 2.0 - 0.4).sin()),
                P3::new(2.0, 0.0, 0.0),
            ),
            (V3::new(1.0, 0.3, 0.6), P3::new(1.5, -2.0, 1.0)),
        ] {
            let fp = Frame::from_z(o, n.normalize());
            let curves = plane_cone(&fp, &fc, r, a, &bb).unwrap();
            assert!(!curves.is_empty(), "{n:?}");
            for ic in &curves {
                for i in 0..=200 {
                    let t = ic.t0 + (ic.t1 - ic.t0) * i as f64 / 200.0;
                    let p = ic.c.point(t);
                    assert!(
                        (p - fp.o).dot(&n.normalize()).abs() < 1e-9,
                        "off the plane {n:?}"
                    );
                    // Either nappe: (±ρ − r)·cos a = z·sin a.
                    let l = fc.local(&p);
                    let rho = l.x.hypot(l.y);
                    let off = [1.0, -1.0]
                        .map(|sg: f64| ((sg * rho - r) * a.cos() - l.z * a.sin()).abs())
                        .into_iter()
                        .fold(f64::INFINITY, f64::min);
                    assert!(off < 1e-9, "off the cone {n:?}: {off}");
                }
            }
        }
    }
}

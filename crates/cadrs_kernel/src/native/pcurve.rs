//! p-curves: a 3D curve lying on a surface as a curve in the surface's (u, v), parametrized by
//! the 3D curve's own parameter. Exact for curves in planes and for rims and rulings of
//! surfaces of revolution; otherwise an interpolating cubic through projected points, refined
//! until it is within 1e-10 of the projection.

use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::surface::Surface;
use super::tol;

fn line2(o: P2, d: V2) -> Curve {
    Curve::Line {
        o: P3::new(o.x, o.y, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    }
}

/// P-curves fitted (projected and interpolated) and the time it took (ns), counted with
/// `CADRS_PROFILE`.
pub(crate) static FITTED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
pub(crate) static FITTED_NS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The p-curve of `c` on [t0, t1] on `s`, its periodic coordinates shifted so its middle lies
/// near `near` (in the face's chart) when given.
pub fn pcurve(s: &Surface, c: &Curve, t0: f64, t1: f64, near: Option<P2>) -> Curve {
    pcurve_or(s, c, t0, t1, near, None)
}

/// [`pcurve`], with `fit` (a p-curve already known, an intersection curve's from its tracer)
/// in place of fitting projections where no exact p-curve applies.
pub fn pcurve_or(
    s: &Surface,
    c: &Curve,
    t0: f64,
    t1: f64,
    near: Option<P2>,
    fit: Option<Curve>,
) -> Curve {
    let pc = exact(s, c, t0, t1)
        .or_else(|| iso_line(s, c, t0, t1))
        .or(fit)
        .unwrap_or_else(|| {
            let t = super::boolean::profiling().then(web_time::Instant::now);
            let pc = fitted(s, c, t0, t1);
            if let Some(t) = t {
                FITTED.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                FITTED_NS.fetch_add(
                    t.elapsed().as_nanos() as u64,
                    std::sync::atomic::Ordering::Relaxed,
                );
            }
            pc
        });
    let pc = into_closed_range(s, pc, t0, t1, near);
    shift_into(s, pc, (t0 + t1) * 0.5, near)
}

/// The periods of a surface's closed directions (u, v): `TAU` where it is periodic, the
/// parameter range where a B-spline surface closes on itself without being periodic (a
/// revolved or skinned surface whose ends meet: its seam is the range's two ends).
pub fn closed_periods(s: &Surface) -> [Option<f64>; 2] {
    let Surface::Nurbs(n) = s else {
        return [s.u_periodic().then_some(TAU), s.v_periodic().then_some(TAU)];
    };
    let ((u0, u1), (v0, v1)) = n.range();
    let meets = |a: &dyn Fn(f64) -> (P3, P3)| {
        (0..5).all(|i| {
            let (p, q) = a(0.1 + 0.2 * i as f64);
            (p - q).norm() <= tol::REL * (1.0 + p.coords.norm())
        })
    };
    let uc = meets(&|f| {
        (
            s.eval(u0, v0 + (v1 - v0) * f),
            s.eval(u1, v0 + (v1 - v0) * f),
        )
    });
    let vc = meets(&|f| {
        (
            s.eval(u0 + (u1 - u0) * f, v0),
            s.eval(u0 + (u1 - u0) * f, v1),
        )
    });
    [uc.then_some(u1 - u0), vc.then_some(v1 - v0)]
}

/// The (u, v) on `s` of the point `p` of a curve being followed, from the previous point's
/// (`hint`): continuous with it across the seams of the closed directions (`periods`, as
/// [`closed_periods`] gives them). A hinted projection that doesn't reach the point (held
/// at the end of a closed B-spline's range while the curve goes on across its seam, or in
/// another local minimum) is redone from the whole surface.
pub fn project_along(
    s: &Surface,
    p: &P3,
    hint: Option<(f64, f64)>,
    periods: [Option<f64>; 2],
) -> P2 {
    let (u, v) = s.project(p, hint);
    let mut q = P2::new(u, v);
    let near_enough = 1e-7 * (1.0 + p.coords.norm());
    // A curve within a tolerance of the surface (a fitted intersection) is off it by more
    // than `near_enough` everywhere: the hinted foot stands where it is the true foot (the
    // miss square to the surface there) and near, not only where it reaches the point.
    // (Square within a millionth of the miss, or within a billionth of the point's size where
    // the miss is so small that a projection can't be told squarer: a foot a tenth of a
    // micron off was else never square enough, and every point of a fitted sweep's edge was
    // sought again over the whole surface.)
    let foot = |q: P2| -> bool {
        let (x, su, sv) = s.d1(q.x, q.y);
        let d = p - x;
        let n = d.norm();
        let square = (tol::REL_POINT * n).max(1e-9 * (1.0 + p.coords.norm()));
        n <= 1e-4 * (1.0 + p.coords.norm())
            && d.dot(&su).abs() <= square * su.norm()
            && d.dot(&sv).abs() <= square * sv.norm()
    };
    if hint.is_some() && (s.eval(q.x, q.y) - p).norm() > near_enough && !foot(q) {
        let (u2, v2) = s.project(p, None);
        if (s.eval(u2, v2) - p).norm() < (s.eval(q.x, q.y) - p).norm() * 0.5 {
            q = P2::new(u2, v2);
        }
    }
    if (s.eval(q.x, q.y) - p).norm() > near_enough
        && !foot(q)
        && let Some(r) = beyond_range(s, p, q)
    {
        q = r;
    }
    if let Some(h) = hint {
        // At a pole (a cone's apex, the end of a B-spline surface's collapsed row) one
        // coordinate is free: it continues the curve's.
        let (fu, fv) = free_at(s, q);
        if fu {
            q.x = h.0;
        }
        if fv {
            q.y = h.1;
        }
        if let Some(pu) = periods[0] {
            q.x = unwrap_near(q.x, h.0, pu);
        }
        if let Some(pv) = periods[1] {
            q.y = unwrap_near(q.y, h.1, pv);
        }
    }
    q
}

/// A B-spline surface's projection held at the end of its range while the point lies on the
/// surface's continuation past it (an intersection curve traced on the surface extended, as a
/// blend's tool surface is): Newton from there without the range's bounds, if it reaches the
/// point within a fifth of the range beyond.
fn beyond_range(s: &Surface, p: &P3, q: P2) -> Option<P2> {
    let Surface::Nurbs(n) = s else { return None };
    let ((u0, u1), (v0, v1)) = n.range();
    let at_end = |x: f64, a: f64, b: f64| {
        (x - a).abs() <= (b - a) * tol::REL || (x - b).abs() <= (b - a) * tol::REL
    };
    if !at_end(q.x, u0, u1) && !at_end(q.y, v0, v1) {
        return None;
    }
    let (mut u, mut v) = (q.x, q.y);
    for _ in 0..50 {
        let d = s.d2(u, v);
        let r = d[0] - p.coords;
        let (fu, fv) = (r.dot(&d[1]), r.dot(&d[2]));
        let a = d[1].norm_squared() + r.dot(&d[3]);
        let b = d[1].dot(&d[2]) + r.dot(&d[4]);
        let c = d[2].norm_squared() + r.dot(&d[5]);
        let det = a * c - b * b;
        if det.abs() <= tol::TINY {
            return None;
        }
        let (du, dv) = ((-fu * c + fv * b) / det, (-fv * a + fu * b) / det);
        u += du;
        v += dv;
        if du.abs() + dv.abs() < 1e-15 * (1.0 + u.abs() + v.abs()) {
            break;
        }
    }
    let (eu, ev) = ((u1 - u0) * 0.2, (v1 - v0) * 0.2);
    let ok =
        u.is_finite() && v.is_finite() && u > u0 - eu && u < u1 + eu && v > v0 - ev && v < v1 + ev;
    // It must reach the point (the extension merely passing nearer isn't the surface).
    (ok && (s.eval(u, v) - p).norm() <= tol::REL_POINT * (1.0 + p.coords.norm()))
        .then_some(P2::new(u, v))
}

/// Whether u (first) and v (second) are free at `q`: the surface doesn't move along them
/// there (a pole).
fn free_at(s: &Surface, q: P2) -> (bool, bool) {
    let (_, su, sv) = s.d1(q.x, q.y);
    let scale = tol::REL * (su.norm() + sv.norm()).max(tol::TINY);
    (su.norm() <= scale, sv.norm() <= scale)
}

/// Samples of a p-curve along a curve: the first ones, projected with nothing to continue,
/// at a pole take the free coordinate from the first sample that isn't at one.
fn settle_poles(s: &Surface, uvs: &mut [P2]) {
    let free: Vec<(bool, bool)> = uvs.iter().map(|q| free_at(s, *q)).collect();
    if let Some(k) = free.iter().position(|f| !f.0 && !f.1) {
        for i in (0..k).rev() {
            if free[i].0 {
                uvs[i].x = uvs[i + 1].x;
            }
            if free[i].1 {
                uvs[i].y = uvs[i + 1].y;
            }
        }
    }
}

/// A p-curve on a B-spline surface closed in a direction but not periodic, moved by whole
/// ranges so it lies in the surface's range (its evaluation doesn't wrap: a p-curve followed
/// continuously across the seam must be brought back). One lying along the seam fits both
/// ways: the copy nearest `near`. Other surfaces as they are.
fn into_closed_range(s: &Surface, pc: Curve, t0: f64, t1: f64, near: Option<P2>) -> Curve {
    let Surface::Nurbs(n) = s else { return pc };
    let periods = closed_periods(s);
    if periods == [None, None] {
        return pc;
    }
    let ((u0, u1), (v0, v1)) = n.range();
    let pts: Vec<P2> = (0..=16)
        .map(|i| pc.point(t0 + (t1 - t0) * i as f64 / 16.0))
        .map(|q| P2::new(q.x, q.y))
        .collect();
    // The shift (in whole periods) putting [lo, hi] most inside [a, b].
    let best = |lo: f64, hi: f64, a: f64, b: f64, per: f64, at: Option<f64>| -> f64 {
        let eps = (b - a) * tol::REL;
        let mut cands: Vec<(f64, f64, f64)> = Vec::new();
        for k in -2i32..=2 {
            let d = k as f64 * per;
            let out = (a - (lo + d)).max(0.0) + ((hi + d) - b).max(0.0);
            let off = at.map_or(k.abs() as f64, |m| (m - ((lo + hi) * 0.5 + d)).abs());
            cands.push((if out <= eps { 0.0 } else { out }, off, d));
        }
        cands.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
        cands[0].2
    };
    let mut du = 0.0;
    let mut dv = 0.0;
    if let Some(pu) = periods[0] {
        let (lo, hi) = pts
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                (a.min(q.x), b.max(q.x))
            });
        du = best(lo, hi, u0, u1, pu, near.map(|q| q.x));
    }
    if let Some(pv) = periods[1] {
        let (lo, hi) = pts
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), q| {
                (a.min(q.y), b.max(q.y))
            });
        dv = best(lo, hi, v0, v1, pv, near.map(|q| q.y));
    }
    if du == 0.0 && dv == 0.0 {
        return pc;
    }
    pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)))
}

/// Shifts a p-curve by whole periods so its point at `tm` lies nearest `near`.
pub fn shift_into(s: &Surface, pc: Curve, tm: f64, near: Option<P2>) -> Curve {
    let m = pc.point(tm);
    // A sphere's meridian p-curves repeat every turn in v: bring them into [−π/2, π/2].
    if let (Surface::Sphere { .. }, None) = (s, near) {
        let dv = -((m.y + PI / 2.0) / TAU).floor() * TAU;
        if dv != 0.0 {
            return pc.transformed(&Affine::translation(V3::new(0.0, dv, 0.0)));
        }
        return pc;
    }
    let Some(near) = near else { return pc };
    let du = if s.u_periodic() {
        ((near.x - m.x) / TAU).round() * TAU
    } else {
        0.0
    };
    let dv = if s.v_periodic() {
        ((near.y - m.y) / TAU).round() * TAU
    } else if matches!(s, Surface::Sphere { .. }) {
        -((m.y + PI / 2.0) / TAU).floor() * TAU
    } else {
        0.0
    };
    if du == 0.0 && dv == 0.0 {
        return pc;
    }
    pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)))
}

/// The angle of the direction `v` in the frame's xy plane.
fn ang(f: &Frame, v: &V3) -> f64 {
    v.dot(&f.y).atan2(v.dot(&f.x))
}

fn exact(s: &Surface, c: &Curve, t0: f64, t1: f64) -> Option<Curve> {
    match s {
        Surface::Plane { f } => {
            // Model → plane coordinates is affine (rows x, y, z).
            let m = nalgebra::Matrix3::from_rows(&[
                f.x.transpose(),
                f.y.transpose(),
                f.x.cross(&f.y).transpose(),
            ]);
            let a = Affine {
                m,
                t: -(m * f.o.coords),
            };
            let pc = match c {
                Curve::Circle { c: cc, x, y, r } => {
                    let (xl, yl) = (m * x, m * y);
                    Curve::Circle {
                        c: a.point(cc),
                        x: V3::new(xl.x, xl.y, 0.0),
                        y: V3::new(yl.x, yl.y, 0.0),
                        r: *r,
                    }
                }
                Curve::Ellipse {
                    c: cc,
                    x,
                    y,
                    a: ra,
                    b: rb,
                } => {
                    let (xl, yl) = (m * x, m * y);
                    Curve::Ellipse {
                        c: a.point(cc),
                        x: V3::new(xl.x, xl.y, 0.0),
                        y: V3::new(yl.x, yl.y, 0.0),
                        a: *ra,
                        b: *rb,
                    }
                }
                _ => c.transformed(&a),
            };
            Some(flatten(pc))
        }
        Surface::Cylinder { f, r } => match c {
            Curve::Line { o, d } if d.cross(&f.z).norm() < tol::REL_FINE * d.norm() => {
                let l = f.local(o);
                Some(line2(
                    P2::new(l.y.atan2(l.x), l.z),
                    V2::new(0.0, d.dot(&f.z)),
                ))
            }
            Curve::Circle { c: cc, x, y, r: rc }
                if coaxial_circle(f, cc, x, y) && (rc - r).abs() < 1e-7 =>
            {
                let sgn = x.cross(y).dot(&f.x.cross(&f.y)).signum();
                Some(line2(P2::new(ang(f, x), f.local(cc).z), V2::new(sgn, 0.0)))
            }
            _ => None,
        },
        Surface::Cone { f, r, a } => match c {
            Curve::Circle { c: cc, x, y, .. } if coaxial_circle(f, cc, x, y) => {
                let sgn = x.cross(y).dot(&f.x.cross(&f.y)).signum();
                let h = f.local(cc).z;
                Some(line2(P2::new(ang(f, x), h / a.cos()), V2::new(sgn, 0.0)))
            }
            Curve::Line { o, d } => {
                // A ruling: in a plane through the axis, along the cone's slope.
                let l = f.local(o);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let u = if rho > tol::ROUNDOFF {
                    l.y.atan2(l.x)
                } else {
                    let ld = V3::new(d.dot(&f.x), d.dot(&f.y), 0.0);
                    ld.y.atan2(ld.x)
                };
                let radial = f.x * u.cos() + f.y * u.sin();
                let ruling = radial * a.sin() + f.z * a.cos();
                if d.cross(&ruling).norm() > tol::REL * d.norm() {
                    return None;
                }
                let _ = r;
                let v0 = (rho - r) * a.sin() + l.z * a.cos();
                Some(line2(P2::new(u, v0), V2::new(0.0, d.dot(&ruling))))
            }
            _ => None,
        },
        Surface::Sphere { f, .. } => match c {
            Curve::Circle { c: cc, x, y, .. } if coaxial_circle(f, cc, x, y) => {
                let sgn = x.cross(y).dot(&f.x.cross(&f.y)).signum();
                let l = f.local(&c.point(0.0));
                let v = l.z.atan2((l.x * l.x + l.y * l.y).sqrt());
                Some(line2(P2::new(ang(f, x), v), V2::new(sgn, 0.0)))
            }
            Curve::Circle { c: cc, x, y, .. }
                if (cc - f.o).norm() < tol::ROUNDOFF
                    && x.cross(y).dot(&f.z).abs() < tol::ANGULAR_FINE =>
            {
                // A meridian: u const, v = ±t + v0, in the half plane holding the arc.
                let m = c.point((t0 + t1) * 0.5) - f.o;
                let side = m - f.z * m.dot(&f.z);
                if side.norm() < tol::ROUNDOFF {
                    return None;
                }
                // The arc must stay in that half plane (not run over a pole).
                for s in [0.0, 1.0] {
                    let q = c.point(t0 + (t1 - t0) * s) - f.o;
                    if (q - f.z * q.dot(&f.z)).dot(&side) < -tol::ROUNDOFF {
                        return None;
                    }
                }
                meridian_side(f, x, y, &side.normalize())
            }
            _ => None,
        },
        Surface::Torus { f, rmaj, rmin } => match c {
            Curve::Circle { c: cc, x, y, .. } if coaxial_circle(f, cc, x, y) => {
                let sgn = x.cross(y).dot(&f.x.cross(&f.y)).signum();
                let l = f.local(&c.point(0.0));
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                // On a spindle torus (minor radius past the major) a circle of the part past
                // the axis lies on the far meridian: u + π, at radius −ρ.
                let far = rmin > rmaj
                    && ((-rho - rmaj).hypot(l.z) - rmin).abs()
                        < ((rho - rmaj).hypot(l.z) - rmin).abs();
                let (u, v) = if far {
                    (ang(f, x) + PI, l.z.atan2(-rho - rmaj))
                } else {
                    (ang(f, x), l.z.atan2(rho - rmaj))
                };
                Some(line2(P2::new(u, v), V2::new(sgn, 0.0)))
            }
            Curve::Circle { c: cc, x, y, .. } if x.cross(y).dot(&f.z).abs() < tol::ANGULAR_FINE => {
                let l = f.local(cc);
                if l.z.abs() > tol::ROUNDOFF
                    || ((l.x * l.x + l.y * l.y).sqrt() - rmaj).abs() > tol::ROUNDOFF
                {
                    return None;
                }
                // The half plane of the circle's centre.
                let w = cc - f.o;
                let radial = (w - f.z * w.dot(&f.z)).normalize();
                meridian_side(f, x, y, &radial)
            }
            _ => None,
        },
        Surface::Revolution { f, m } => match c {
            Curve::Circle { c: cc, x, y, .. } if coaxial_circle(f, cc, x, y) => {
                let sgn = x.cross(y).dot(&f.x.cross(&f.y)).signum();
                let (_, v) = s.project(&c.point(0.0), None);
                // A meridian off the frame's xz plane sits at its own azimuth.
                let az = if super::surface::meridian_off_plane(f, m) {
                    super::surface::meridian_azimuth(f, m, v)
                } else {
                    0.0
                };
                Some(line2(P2::new(ang(f, x) - az, v), V2::new(sgn, 0.0)))
            }
            _ => None,
        },
        Surface::Extrusion { c: base, d } => match c {
            Curve::Line { o, d: dl } if dl.cross(d).norm() < tol::REL_FINE * dl.norm() => {
                let (u, v) = s.project(o, None);
                let _ = base;
                Some(line2(
                    P2::new(u, v),
                    V2::new(0.0, dl.dot(d) / d.norm_squared()),
                ))
            }
            _ => None,
        },
        Surface::Nurbs(_) => None,
    }
}

/// A sphere's meridian in the half plane of `radial`: u const, v = σt + v0.
fn meridian_side(f: &Frame, x: &V3, y: &V3, radial: &V3) -> Option<Curve> {
    let (xr, xz) = (x.dot(radial), x.dot(&f.z));
    let (yr, yz) = (y.dot(radial), y.dot(&f.z));
    let sig = (xr * yz - xz * yr).signum();
    let v0 = xz.atan2(xr);
    let u = ang(f, radial);
    Some(line2(P2::new(u, v0), V2::new(0.0, sig)))
}

fn coaxial_circle(f: &Frame, cc: &P3, x: &V3, y: &V3) -> bool {
    let n = x.cross(y);
    if n.cross(&f.z).norm() > 1e-10 {
        return false;
    }
    let w = cc - f.o;
    (w - f.z * w.dot(&f.z)).norm() < tol::ROUNDOFF
}

/// Drops the z coordinate of a 2D curve (numerical noise from projecting into a plane).
fn flatten(c: Curve) -> Curve {
    match c {
        Curve::Line { o, d } => Curve::Line {
            o: P3::new(o.x, o.y, 0.0),
            d: V3::new(d.x, d.y, 0.0),
        },
        Curve::Nurbs(n) => {
            let mut n = (*n).clone();
            for h in &mut n.cps {
                h.z = 0.0;
            }
            Curve::Nurbs(Arc::new(n))
        }
        other => other,
    }
}

/// A curve that runs along an iso-line of a B-spline or general surface with its parameter an
/// affine function of the iso-line's (a sweep's rails, a loft's rails): a straight p-curve.
pub(super) fn iso_line(s: &Surface, c: &Curve, t0: f64, t1: f64) -> Option<Curve> {
    if !matches!(
        s,
        Surface::Nurbs(_) | Surface::Extrusion { .. } | Surface::Revolution { .. }
    ) {
        return None;
    }
    let n = 8;
    let mut uv: Vec<P2> = Vec::with_capacity(n + 1);
    let mut hint: Option<(f64, f64)> = None;
    let periods = closed_periods(s);
    for i in 0..=n {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let p = c.point(t);
        let q = project_along(s, &p, hint, periods);
        let (u, v) = (q.x, q.y);
        if (s.eval(u, v) - p).norm() > tol::ROUNDOFF {
            return None;
        }
        hint = Some((u, v));
        uv.push(P2::new(u, v));
    }
    settle_poles(s, &mut uv);
    let (a, b) = (uv[0], uv[n]);
    let d = (b - a) / (t1 - t0);
    let line = Curve::Line {
        o: P3::new(a.x - d.x * t0, a.y - d.y * t0, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    };
    // Straight in uv and affine in t, and an iso-line (u or v constant).
    if d.x.abs() > tol::REL_FINE * d.y.abs().max(tol::TINY)
        && d.y.abs() > tol::REL_FINE * d.x.abs().max(tol::TINY)
    {
        return None;
    }
    let tol = 1e-10 * (1.0 + a.coords.norm() + b.coords.norm());
    for (i, q) in uv.iter().enumerate() {
        let t = t0 + (t1 - t0) * i as f64 / n as f64;
        let l = line.point(t);
        if (l.x - q.x).abs() > tol || (l.y - q.y).abs() > tol {
            return None;
        }
    }
    // And between the samples, on the surface.
    for i in 0..n {
        let t = t0 + (t1 - t0) * (i as f64 + 0.5) / n as f64;
        let l = line.point(t);
        if (s.eval(l.x, l.y) - c.point(t)).norm() > tol::ROUNDOFF {
            return None;
        }
    }
    Some(line)
}

/// Interpolates projected points with the curve's own parameters.
fn fitted(s: &Surface, c: &Curve, t0: f64, t1: f64) -> Curve {
    let pieces = c.pieces(t0, t1);
    let mut ts: Vec<f64> = Vec::new();
    // A few samples a piece to start (refined below where the fit is off).
    let n = (512 / pieces.len().max(1)).clamp(2, 16);
    for w in pieces.windows(2) {
        for i in 0..n {
            ts.push(w[0] + (w[1] - w[0]) * i as f64 / n as f64);
        }
    }
    ts.push(t1);
    // Followed continuously across the surface's seams (brought back into a closed
    // B-spline's range after: `into_closed_range`).
    let periods = closed_periods(s);
    let project =
        |t: f64, hint: Option<(f64, f64)>| -> P2 { project_along(s, &c.point(t), hint, periods) };
    // The samples' projections, each hinted by its neighbour's and kept across the rounds
    // (only the samples a round adds are projected).
    let mut uvs: Vec<P2> = Vec::with_capacity(ts.len());
    let mut hint: Option<(f64, f64)> = None;
    for &t in &ts {
        let q = project(t, hint);
        hint = Some((q.x, q.y));
        uvs.push(q);
    }
    settle_poles(s, &mut uvs);
    let fit_of = |ts: &[f64], uvs: &[P2]| {
        let pts: Vec<P3> = uvs.iter().map(|q| P3::new(q.x, q.y, 0.0)).collect();
        NurbsCurve::interpolate(&pts, ts, 3.min(pts.len() - 1), None, None)
    };
    // How near the fit keeps to the curve's projection: 1e-9, or 2e-10 of the curve's size
    // on a big model (the bound an intersection curve itself is fitted to).
    let tol = {
        let mut bb = Bbox::empty();
        for k in 0..=8 {
            bb.add(&c.point(t0 + (t1 - t0) * k as f64 / 8.0));
        }
        1e-9f64.max(2e-10 * bb.diagonal())
    };
    // Each interval's midpoint's projection (and its point), once made: an interval no round
    // splits keeps its midpoint and its hint, so its projection would come out the same.
    let mut mids: Vec<Option<(P2, P3)>> = vec![None; ts.len() - 1];
    for _round in 0..12 {
        let fit = fit_of(&ts, &uvs);
        // Check the midpoints.
        let mut ins: Vec<(usize, f64, P2)> = Vec::new();
        for k in 0..ts.len() - 1 {
            let tm = (ts[k] + ts[k + 1]) * 0.5;
            let f = fit.point(tm);
            // Measured in model space between the fit's point and the projection's (uv noise of
            // projections on B-spline surfaces would never settle; an intersection curve may lie
            // a little off the surface, which no p-curve can make up).
            let (q, at) = *mids[k].get_or_insert_with(|| {
                let q = project(tm, Some((uvs[k].x, uvs[k].y)));
                (q, s.eval(q.x, q.y))
            });
            let err = (s.eval(f.x, f.y) - at).norm();
            if err > tol {
                ins.push((k + 1, tm, q));
            }
        }
        if ins.is_empty() || ts.len() > 4096 {
            if !ins.is_empty() && std::env::var("CADRS_PCURVE_DEBUG").is_ok() {
                eprintln!(
                    "pcurve: no fit after {} points ({} off) of {c:?} on {s:?}",
                    ts.len(),
                    ins.len()
                );
            }
            return Curve::Nurbs(Arc::new(fit));
        }
        for (k, t, q) in ins.into_iter().rev() {
            ts.insert(k, t);
            uvs.insert(k, q);
            // (Interval k − 1 split in two, neither half's midpoint projected yet.)
            mids[k - 1] = None;
            mids.insert(k, None);
        }
    }
    Curve::Nurbs(Arc::new(fit_of(&ts, &uvs)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(s: &Surface, c: &Curve, t0: f64, t1: f64) {
        let pc = pcurve(s, c, t0, t1, None);
        for i in 0..=20 {
            let t = t0 + (t1 - t0) * i as f64 / 20.0;
            let q = pc.point(t);
            let p = s.eval(q.x, q.y);
            assert!(
                (p - c.point(t)).norm() < 1e-8,
                "{s:?} {c:?} at {t}: {}",
                (p - c.point(t)).norm()
            );
        }
    }

    /// On a B-spline cylinder closed in u (not periodic): an arc ending on the seam ends at the
    /// range's end it comes to, one starting there starts at the other, both in the range
    /// (regression: the seam end's projection took either end, the fit jumping a whole range).
    #[test]
    fn closed_bspline_seam_ends() {
        let f = Frame::from_z(P3::new(1.0, 2.0, 3.0), V3::new(0.0, 0.0, 1.0));
        let s = Surface::Nurbs(Arc::new(
            Surface::Cylinder { f, r: 5.0 }.to_nurbs((0.0, TAU), (0.0, 10.0)),
        ));
        let periods = closed_periods(&s);
        assert!(
            periods[0].is_some_and(|p| (p - TAU).abs() < 1e-9) && periods[1].is_none(),
            "{periods:?}"
        );
        let ((u0, u1), _) = s.param_box();
        let arc = Curve::Circle {
            c: f.o + f.z * 3.0,
            x: f.x,
            y: f.y,
            r: 5.0,
        };
        for (t0, t1, start, end) in [
            (PI, TAU, None, Some(u1)),
            (0.0, PI, Some(u0), None),
            (0.5, TAU, None, Some(u1)),
        ] {
            check(&s, &arc, t0, t1);
            let pc = pcurve(&s, &arc, t0, t1, None);
            for i in 0..=20 {
                let q = pc.point(t0 + (t1 - t0) * i as f64 / 20.0);
                assert!(
                    q.x >= u0 - 1e-9 && q.x <= u1 + 1e-9,
                    "{t0}..{t1}: u {} out of the range",
                    q.x
                );
            }
            if let Some(u) = start {
                assert!(
                    (pc.point(t0).x - u).abs() < 1e-7,
                    "{t0}..{t1}: starts at {}",
                    pc.point(t0).x
                );
            }
            if let Some(u) = end {
                assert!(
                    (pc.point(t1).x - u).abs() < 1e-7,
                    "{t0}..{t1}: ends at {}",
                    pc.point(t1).x
                );
            }
        }
        // Clockwise from the seam: it starts at the range's end and runs back into it.
        let cw = Curve::Circle {
            c: f.o + f.z * 3.0,
            x: f.x,
            y: -f.y,
            r: 5.0,
        };
        check(&s, &cw, 0.0, PI);
        let pc = pcurve(&s, &cw, 0.0, PI, None);
        assert!(
            (pc.point(0.0).x - u1).abs() < 1e-7 && (pc.point(PI).x - PI).abs() < 1e-6,
            "{:?} .. {:?}",
            pc.point(0.0),
            pc.point(PI)
        );
    }

    /// A ruling of a B-spline cone up to its apex keeps one u to the end (regression: the apex,
    /// where u is free, took an arbitrary one and bent the p-curve).
    #[test]
    fn ruling_to_a_pole() {
        let f = Frame::from_z(P3::origin(), V3::new(0.0, 0.0, 1.0));
        let a = -(0.5f64).atan();
        let cone = Surface::Cone { f, r: 5.0, a };
        let apex_v = -5.0 / a.sin();
        let s = Surface::Nurbs(Arc::new(cone.to_nurbs((0.0, TAU), (0.0, apex_v))));
        let (base, apex) = (cone.eval(1.0, 0.0), cone.eval(1.0, apex_v));
        let (line, len) = Curve::line(base, apex);
        let pc = pcurve(&s, &line, 0.0, len, None);
        let u = pc.point(0.0).x;
        for i in 0..=20 {
            let q = pc.point(len * i as f64 / 20.0);
            assert!(
                (q.x - u).abs() < 1e-6,
                "u {} at {i}/20, {} at the base",
                q.x,
                u
            );
            assert!((s.eval(q.x, q.y) - line.point(len * i as f64 / 20.0)).norm() < 1e-8);
        }
    }

    #[test]
    fn pcurves_lie_on_their_curves() {
        let f = Frame::from_xz(
            P3::new(1.0, 2.0, 3.0),
            V3::new(1.0, 0.2, 0.0),
            V3::new(0.1, 0.0, 1.0),
        );
        let cyl = Surface::Cylinder { f, r: 5.0 };
        let rim = Curve::Circle {
            c: f.o + f.z * 2.0,
            x: f.y,
            y: -f.x,
            r: 5.0,
        };
        check(&cyl, &rim, 0.3, 4.0);
        let ruling = Curve::Line {
            o: f.o + f.x * 5.0,
            d: -f.z,
        };
        check(&cyl, &ruling, -3.0, 2.0);
        // An oblique ellipse: fitted.
        let pl = Surface::Plane {
            f: Frame::from_z(f.o, V3::new(0.3, 0.1, 1.0)),
        };
        let e = super::super::ssi::surface_surface(
            &pl,
            ((0.0, 1.0), (0.0, 1.0)),
            &cyl,
            ((0.0, TAU), (-10.0, 10.0)),
            &Bbox {
                min: P3::new(-20.0, -20.0, -20.0),
                max: P3::new(20.0, 20.0, 20.0),
            },
        );
        assert_eq!(e.len(), 1);
        check(&cyl, &e[0].c, 0.0, TAU);
        check(&pl, &e[0].c, 0.0, TAU);
        let sph = Surface::Sphere { f, r: 5.0 };
        let lat = Curve::Circle {
            c: f.o + f.z * 3.0,
            x: f.x,
            y: f.y,
            r: 4.0,
        };
        check(&sph, &lat, 0.0, TAU);
        let mer = Curve::Circle {
            c: f.o,
            x: f.x,
            y: f.z,
            r: 5.0,
        };
        check(&sph, &mer, -1.0, 1.0);
        let tor = Surface::Torus {
            f,
            rmaj: 10.0,
            rmin: 2.0,
        };
        let m2 = Curve::Circle {
            c: f.o + f.y * 10.0,
            x: f.y,
            y: f.z,
            r: 2.0,
        };
        check(&tor, &m2, 0.0, TAU);
        let cone = Surface::Cone { f, r: 3.0, a: 0.3 };
        let ruling = Curve::Line {
            o: cone.eval(1.0, 0.0),
            d: cone.d1(1.0, 0.0).2,
        };
        check(&cone, &ruling, -2.0, 2.0);
    }
}

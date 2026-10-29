//! Exact surfaces with their (u, v) parametrizations.
//!
//! - Plane: `o + u·x + v·y`.
//! - Cylinder: `o + r(cos u·x + sin u·y) + v·z`.
//! - Cone: `o + (r + v sin a)(cos u·x + sin u·y) + v cos a·z` (a: the semi-angle).
//! - Sphere: `o + r cos v(cos u·x + sin u·y) + r sin v·z`, v in [−π/2, π/2].
//! - Torus: `o + (R + r cos v)(cos u·x + sin u·y) + r sin v·z`.
//! - Revolution: the meridian curve (in the frame's xz half plane) turned by u about z. A
//!   meridian off that plane (a curve skew to the axis turned about it: a revolve about an axis
//!   off the profile's plane) is the same `S(u, v) = Rot_z(u)·m(v)`; [`Surface::project`]
//!   handles it by its meridian's (ρ, h) profile ([`meridian_off_plane`]).
//! - Extrusion: `C(u) + v·d`.
//!
//! The natural normal is Su × Sv; for the analytic surfaces it points away from the axis
//! (outward from the material of a solid cylinder etc.).

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use super::curve::Curve;
use super::math::*;
use super::nurbs::{H, NurbsCurve, NurbsSurface};
use super::tol;
use crate::SurfaceKind;

/// An exact surface with its parametrization `(u, v) ↦ point`. The analytic ones are placed by
/// a frame `f` (origin `o`, axes `x`, `y`, `z`); u is the angle about `f.z` for the surfaces of
/// revolution.
#[derive(Debug, Clone, PartialEq)]
#[allow(
    missing_docs,
    reason = "the fields are the symbols of each variant's formula"
)]
pub enum Surface {
    /// `o + u·x + v·y`.
    Plane { f: Frame },
    /// `o + r(cos u·x + sin u·y) + v·z`.
    Cylinder { f: Frame, r: f64 },
    /// `o + (r + v sin a)(cos u·x + sin u·y) + v cos a·z`: radius `r` at v = 0, half-angle `a`.
    Cone { f: Frame, r: f64, a: f64 },
    /// `o + r cos v(cos u·x + sin u·y) + r sin v·z`, v the latitude.
    Sphere { f: Frame, r: f64 },
    /// `o + (rmaj + rmin cos v)(cos u·x + sin u·y) + rmin sin v·z`.
    Torus { f: Frame, rmaj: f64, rmin: f64 },
    /// The meridian curve `m(v)` turned by u about `f.z`.
    Revolution { f: Frame, m: Curve },
    /// `c(u) + v·d`: the curve swept along `d`.
    Extrusion { c: Curve, d: V3 },
    /// A rational B-spline surface.
    Nurbs(Arc<NurbsSurface>),
}

/// Point and derivatives: S, Su, Sv, Suu, Suv, Svv.
pub type D2 = [V3; 6];

/// The surface a kept grid ([`Surface::grid_points`]) is of: a B-spline surface, or an extrusion
/// of a B-spline curve along a direction.
enum GridOf {
    Nurbs(Weak<NurbsSurface>),
    Extrusion(Weak<NurbsCurve>, V3),
}

impl GridOf {
    fn is(&self, other: &GridOf) -> bool {
        match (self, other) {
            (GridOf::Nurbs(a), GridOf::Nurbs(b)) => Weak::ptr_eq(a, b),
            (GridOf::Extrusion(a, d), GridOf::Extrusion(b, e)) => Weak::ptr_eq(a, b) && d == e,
            _ => false,
        }
    }
}

impl Surface {
    /// The kind the API reports ([`SurfaceKind`]).
    pub fn kind(&self) -> SurfaceKind {
        match self {
            Surface::Plane { .. } => SurfaceKind::Plane,
            Surface::Cylinder { .. } => SurfaceKind::Cylinder,
            Surface::Cone { .. } => SurfaceKind::Cone,
            Surface::Sphere { .. } => SurfaceKind::Sphere,
            Surface::Torus { .. } => SurfaceKind::Torus,
            _ => SurfaceKind::Other,
        }
    }

    /// The placing frame of an analytic surface or a surface of revolution.
    pub fn frame(&self) -> Option<&Frame> {
        match self {
            Surface::Plane { f }
            | Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Sphere { f, .. }
            | Surface::Torus { f, .. }
            | Surface::Revolution { f, .. } => Some(f),
            _ => None,
        }
    }

    /// Whether u closes with period 2π.
    pub fn u_periodic(&self) -> bool {
        match self {
            Surface::Plane { .. } | Surface::Nurbs(_) => false,
            Surface::Extrusion { c, .. } => c.is_periodic(),
            _ => true,
        }
    }

    /// Whether v closes with period 2π (a torus).
    pub fn v_periodic(&self) -> bool {
        matches!(self, Surface::Torus { .. })
    }

    /// The point at (u, v).
    pub fn eval(&self, u: f64, v: f64) -> P3 {
        match self {
            Surface::Plane { f } => f.o + f.x * u + f.y * v,
            Surface::Cylinder { f, r } => f.o + (f.x * u.cos() + f.y * u.sin()) * *r + f.z * v,
            Surface::Cone { f, r, a } => {
                f.o + (f.x * u.cos() + f.y * u.sin()) * (r + v * a.sin()) + f.z * (v * a.cos())
            }
            Surface::Sphere { f, r } => {
                f.o + (f.x * u.cos() + f.y * u.sin()) * (r * v.cos()) + f.z * (r * v.sin())
            }
            Surface::Torus { f, rmaj, rmin } => {
                f.o + (f.x * u.cos() + f.y * u.sin()) * (rmaj + rmin * v.cos())
                    + f.z * (rmin * v.sin())
            }
            Surface::Nurbs(n) => n.point(u, v),
            // (The point as [`Self::d2`] makes it, to the bit: a curve's point is its
            // derivatives' first, the same sums.)
            Surface::Extrusion { c, d } => c.point(u) + d * v,
            Surface::Revolution { f, m } => {
                let l = f.local(&m.point(v));
                let (s, c) = u.sin_cos();
                f.world_point(&V3::new(l.x * c - l.y * s, l.x * s + l.y * c, l.z))
            }
        }
    }

    /// The point and the first derivatives along u and v.
    pub fn d1(&self, u: f64, v: f64) -> (P3, V3, V3) {
        if let Surface::Nurbs(n) = self {
            return n.ders1(u, v);
        }
        let d = self.d2(u, v);
        (P3::from(d[0]), d[1], d[2])
    }

    /// The point and the first and second derivatives ([`D2`]).
    pub fn d2(&self, u: f64, v: f64) -> D2 {
        match self {
            Surface::Plane { f } => [
                (f.o + f.x * u + f.y * v).coords,
                f.x,
                f.y,
                V3::zeros(),
                V3::zeros(),
                V3::zeros(),
            ],
            Surface::Cylinder { f, r } => {
                let (s, c) = u.sin_cos();
                let rho = f.x * c + f.y * s;
                let phi = f.y * c - f.x * s;
                [
                    (f.o + rho * *r + f.z * v).coords,
                    phi * *r,
                    f.z,
                    -rho * *r,
                    V3::zeros(),
                    V3::zeros(),
                ]
            }
            Surface::Cone { f, r, a } => {
                let (s, c) = u.sin_cos();
                let rho = f.x * c + f.y * s;
                let phi = f.y * c - f.x * s;
                let (sa, ca) = a.sin_cos();
                let rr = r + v * sa;
                [
                    (f.o + rho * rr + f.z * (v * ca)).coords,
                    phi * rr,
                    rho * sa + f.z * ca,
                    -rho * rr,
                    phi * sa,
                    V3::zeros(),
                ]
            }
            Surface::Sphere { f, r } => {
                let (s, c) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let rho = f.x * c + f.y * s;
                let phi = f.y * c - f.x * s;
                [
                    (f.o + rho * (r * cv) + f.z * (r * sv)).coords,
                    phi * (r * cv),
                    (-rho * sv + f.z * cv) * *r,
                    -rho * (r * cv),
                    -phi * (r * sv),
                    (-rho * cv - f.z * sv) * *r,
                ]
            }
            Surface::Torus { f, rmaj, rmin } => {
                let (s, c) = u.sin_cos();
                let (sv, cv) = v.sin_cos();
                let rho = f.x * c + f.y * s;
                let phi = f.y * c - f.x * s;
                let rr = rmaj + rmin * cv;
                [
                    (f.o + rho * rr + f.z * (rmin * sv)).coords,
                    phi * rr,
                    (-rho * sv + f.z * cv) * *rmin,
                    -rho * rr,
                    -phi * (rmin * sv),
                    (-rho * cv - f.z * sv) * *rmin,
                ]
            }
            Surface::Revolution { f, m } => {
                let (p, d1, d2) = m.d2(v);
                let l = f.local(&p);
                let ld1 = V3::new(d1.dot(&f.x), d1.dot(&f.y), d1.dot(&f.z));
                let ld2 = V3::new(d2.dot(&f.x), d2.dot(&f.y), d2.dot(&f.z));
                let (s, c) = u.sin_cos();
                let rot = |q: &V3| V3::new(q.x * c - q.y * s, q.x * s + q.y * c, q.z);
                let drot = |q: &V3| V3::new(-q.x * s - q.y * c, q.x * c - q.y * s, 0.0);
                let ddrot = |q: &V3| V3::new(-q.x * c + q.y * s, -q.x * s - q.y * c, 0.0);
                let w = |q: V3| f.world_vec(&q);
                [
                    f.world_point(&rot(&l)).coords,
                    w(drot(&l)),
                    w(rot(&ld1)),
                    w(ddrot(&l)),
                    w(drot(&ld1)),
                    w(rot(&ld2)),
                ]
            }
            Surface::Extrusion { c, d } => {
                let (p, d1, d2) = c.d2(u);
                [(p + d * v).coords, d1, *d, d2, V3::zeros(), V3::zeros()]
            }
            Surface::Nurbs(n) => n.ders2(u, v),
        }
    }

    /// The natural (Su × Sv) normal, unit length; falls back to neighbouring points at poles.
    pub fn normal(&self, u: f64, v: f64) -> V3 {
        let d = self.d2(u, v);
        let n = d[1].cross(&d[2]);
        let l = n.norm();
        if l > 1e-14 {
            return n / l;
        }
        // Degenerate point (a pole or apex): look a little inside.
        match self {
            Surface::Sphere { f, .. } => {
                let p = P3::from(d[0]);
                let l = f.local(&p);
                if l.z >= 0.0 { f.z } else { -f.z }
            }
            Surface::Cone { .. } => {
                // Apex: the average normal is along −z·sin a (outward-ish); use a nearby point.
                let e = tol::DIFF_STEP;
                let d = self.d2(u, v + e);
                d[1].cross(&d[2]).normalize()
            }
            _ => {
                let e = 1e-7;
                for (du, dv) in [(e, 0.0), (0.0, e), (-e, 0.0), (0.0, -e), (e, e)] {
                    let d = self.d2(u + du, v + dv);
                    let n = d[1].cross(&d[2]);
                    if n.norm() > 1e-14 {
                        return n.normalize();
                    }
                }
                V3::z()
            }
        }
    }

    /// The (u, v) of the point of the surface nearest `p`; periodic coordinates near `hint` when
    /// given, else in [0, 2π).
    pub fn project(&self, p: &P3, hint: Option<(f64, f64)>) -> (f64, f64) {
        let ang = |f: &Frame| {
            let l = f.local(p);
            let a = l.y.atan2(l.x);
            match hint {
                Some((hu, _)) => unwrap_near(a, hu, TAU),
                None => wrap(a, 0.0, TAU),
            }
        };
        match self {
            Surface::Plane { f } => {
                let l = f.local(p);
                (l.x, l.y)
            }
            Surface::Cylinder { f, .. } => (ang(f), f.local(p).z),
            Surface::Cone { f, r, a } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let (sa, ca) = a.sin_cos();
                let v = (rho - r) * sa + l.z * ca;
                // A point on the cone's other nappe (past the apex) lies on the generator
                // across the axis: half a turn round. (Only for a point well off this side's
                // generator and much nearer that one: a point near this side, or inside the cone
                // by its axis, keeps this side's foot, as faces expect.)
                let (d_here, d_across) = (
                    ((rho - r) * ca - l.z * sa).abs(),
                    ((-rho - r) * ca - l.z * sa).abs(),
                );
                if d_here > tol::REL_POINT * (1.0 + p.coords.norm()) && d_across < 0.5 * d_here {
                    let u = ang(f) + PI;
                    let u = match hint {
                        Some((hu, _)) => unwrap_near(u, hu, TAU),
                        None => wrap(u, 0.0, TAU),
                    };
                    return (u, (-rho - r) * sa + l.z * ca);
                }
                // At the apex side (v below the apex) atan2 is meaningless: keep the hint.
                (ang(f), v)
            }
            Surface::Sphere { f, .. } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let v = l.z.atan2(rho);
                let u = if rho < tol::ROUNDOFF_FINE {
                    hint.map(|h| h.0).unwrap_or(0.0)
                } else {
                    ang(f)
                };
                (u, v)
            }
            Surface::Torus { f, rmaj, rmin } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                // A spindle torus (minor radius past the major: a revolved arc crossing its
                // axis) reaches each side of the axis from both meridians: the point is on the
                // far one (u + π, at radius −ρ) when that circle runs nearer it.
                let (mut u, mut v) = (ang(f), l.z.atan2(rho - rmaj));
                if rmin > rmaj {
                    let near = ((rho - rmaj).hypot(l.z) - rmin).abs();
                    let far = ((-rho - rmaj).hypot(l.z) - rmin).abs();
                    let hint_far = hint.is_some_and(|(hu, _)| {
                        (unwrap_near(u + PI, hu, TAU) - hu).abs() < (u - hu).abs()
                    });
                    if far < near - tol::CONFUSION || (far <= near + tol::CONFUSION && hint_far) {
                        u = match hint {
                            Some((hu, _)) => unwrap_near(u + PI, hu, TAU),
                            None => wrap(u + PI, 0.0, TAU),
                        };
                        v = l.z.atan2(-rho - rmaj);
                    }
                }
                let v = match hint {
                    Some((_, hv)) => unwrap_near(v, hv, TAU),
                    None => wrap(v, -PI, TAU),
                };
                (u, v)
            }
            Surface::Revolution { f, m } if meridian_off_plane(f, m) => {
                project_off_plane(f, m, p, hint)
            }
            Surface::Revolution { f, m } => {
                let u = ang(f);
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let q = f.world_point(&V3::new(rho, 0.0, l.z));
                let (a, b) = match m.nurbs_range() {
                    Some(r) => r,
                    None if m.is_periodic() => {
                        let c = hint.map(|h| h.1).unwrap_or(0.0);
                        (c - PI, c + PI)
                    }
                    None => (-1e12, 1e12),
                };
                // Hinted: Newton on the meridian from the hint (a projection that follows a
                // curve along; the global search samples the whole meridian every time).
                let local = hint.and_then(|(_, hv)| {
                    let mut t = hv.clamp(a, b);
                    for _ in 0..20 {
                        let (c, d1, d2) = m.d2(t);
                        let r = c - q;
                        let df = d1.norm_squared() + r.dot(&d2);
                        if df <= tol::TINY {
                            return None;
                        }
                        let step = r.dot(&d1) / df;
                        let next = (t - step).clamp(a, b);
                        // Converged, or held at an end of the meridian.
                        if (next - t).abs() <= tol::REL_FINE * (1.0 + t.abs()) {
                            return Some(next);
                        }
                        t = next;
                    }
                    None
                });
                let v = local.unwrap_or_else(|| m.closest(&q, a, b));
                (u, v)
            }
            Surface::Extrusion { .. } | Surface::Nurbs(_) => self.project_newton(p, hint),
        }
    }

    /// The closest point from a coarse `n` × `n` grid start (a cheap check of a hinted
    /// projection; surfaces with a closed form project exactly).
    pub fn project_coarse(&self, p: &P3, n: usize) -> (f64, f64) {
        match self {
            Surface::Extrusion { .. } | Surface::Nurbs(_) => self.project_grid(p, None, n),
            _ => self.project(p, None),
        }
    }

    /// Generic closest point by sampling and Newton (extrusions and NURBS).
    fn project_newton(&self, p: &P3, hint: Option<(f64, f64)>) -> (f64, f64) {
        self.project_grid(p, hint, 24)
    }

    fn project_grid(&self, p: &P3, hint: Option<(f64, f64)>, n: usize) -> (f64, f64) {
        let ((u0, u1), (v0, v1)) = self.param_box();
        // Unhinted: Newton from the nearest grid point, and when that stops short of `p` (in
        // another basin of the distance: a surface curving back on itself between grid
        // points), from the next nearest local minima of the grid too.
        let mut others: Vec<(f64, (f64, f64))> = Vec::new();
        // The nearest grid point (squared distance): no answer may be farther.
        let mut grid_best: Option<(f64, (f64, f64))> = None;
        let (u, v) = match hint {
            Some(h) => h,
            None => {
                let at = |i: usize, j: usize| {
                    (
                        u0 + (u1 - u0) * i as f64 / n as f64,
                        v0 + (v1 - v0) * j as f64 / n as f64,
                    )
                };
                let grid = self.grid_points(n, &at);
                let d: Vec<f64> = grid.iter().map(|g| (g - p).norm_squared()).collect();
                // The grid's local minima (no neighbour nearer), row by row.
                let w = n + 1;
                for i in 0..w {
                    let rows = i.saturating_sub(1)..(i + 1).min(n) + 1;
                    for j in 0..w {
                        let x = d[i * w + j];
                        let (b0, b1) = (j.saturating_sub(1), (j + 1).min(n) + 1);
                        let lowest = rows
                            .clone()
                            .all(|a| d[a * w + b0..a * w + b1].iter().all(|&y| y >= x));
                        if lowest {
                            others.push((x, at(i, j)));
                        }
                    }
                }
                others.sort_by(|a, b| a.0.total_cmp(&b.0));
                grid_best = others.first().copied();
                if others.is_empty() {
                    (u0, v0)
                } else {
                    others.remove(0).1
                }
            }
        };
        let newton = |mut u: f64, mut v: f64| {
            for _ in 0..50 {
                let d = self.d2(u, v);
                let r = d[0] - p.coords;
                let fu = r.dot(&d[1]);
                let fv = r.dot(&d[2]);
                let a = d[1].norm_squared() + r.dot(&d[3]);
                let b = d[1].dot(&d[2]) + r.dot(&d[4]);
                let c = d[2].norm_squared() + r.dot(&d[5]);
                let Some((du, dv)) = solve2(a, b, b, c, -fu, -fv) else {
                    break;
                };
                let (mut nu, mut nv) = (
                    self.clamp_u(u + du, u0, u1),
                    (v + dv).clamp(v0.min(v1), v1.max(v0)),
                );
                // Held at a u end (or a v end): the joint step's other half aimed past it, so
                // step the free direction alone (the closest point along the boundary).
                if nu != u + du && c > tol::TINY {
                    nv = (v - fv / c).clamp(v0.min(v1), v1.max(v0));
                } else if nv != v + dv && a > tol::TINY {
                    nu = self.clamp_u(u - fu / a, u0, u1);
                }
                // Only steps that bring the point closer (far from it the distance's Hessian
                // needn't be positive and Newton can climb): halved until they do, else a step
                // down the gradient (scaled per direction), halved the same way.
                // (A step down to rounding noise has converged: no point checking it, as the
                // distance it changes is noise too.)
                if (nu - u).abs() + (nv - v).abs() < tol::REL_FINE * (1.0 + u.abs() + v.abs()) {
                    (u, v) = (nu, nv);
                    break;
                }
                let here = r.norm_squared();
                let closer = |nu: f64, nv: f64| (self.eval(nu, nv) - p).norm_squared() < here;
                let mut tries = 0;
                while !closer(nu, nv) && tries < 30 {
                    (nu, nv) = (u + (nu - u) * 0.5, v + (nv - v) * 0.5);
                    tries += 1;
                }
                if tries == 30 {
                    let (su, sv) = (
                        d[1].norm_squared().max(tol::TINY),
                        d[2].norm_squared().max(tol::TINY),
                    );
                    (nu, nv) = (
                        self.clamp_u(u - fu / su, u0, u1),
                        (v - fv / sv).clamp(v0.min(v1), v1.max(v0)),
                    );
                    tries = 0;
                    while !closer(nu, nv) && tries < 30 {
                        (nu, nv) = (u + (nu - u) * 0.5, v + (nv - v) * 0.5);
                        tries += 1;
                    }
                    if tries == 30 {
                        break;
                    }
                }
                let step = (nu - u).abs() + (nv - v).abs();
                u = nu;
                v = nv;
                if step < 1e-15 {
                    break;
                }
            }
            (u, v)
        };
        let (mut u, mut v) = newton(u, v);
        let near_enough = 1e-7 * (1.0 + p.coords.norm());
        for (_, (a, b)) in others.into_iter().take(4) {
            let off = (self.eval(u, v) - p).norm();
            if off <= near_enough {
                break;
            }
            let (a, b) = newton(a, b);
            if (self.eval(a, b) - p).norm() < off {
                (u, v) = (a, b);
            }
        }
        // Stopped on a v end of a surface closed in v (a polar patch's seam): the point may lie
        // just across the seam, so go on from the other end too.
        if (v == v0 || v == v1) && (self.eval(u, v) - p).norm() > tol::ROUNDOFF {
            let closed = (0..3).all(|i| {
                let a = u0 + (u1 - u0) * (0.2 + 0.3 * i as f64);
                (self.eval(a, v0) - self.eval(a, v1)).norm() < tol::ROUNDOFF
            });
            if closed {
                let (u2, v2) = newton(u, if v == v0 { v1 } else { v0 });
                if (self.eval(u2, v2) - p).norm() < (self.eval(u, v) - p).norm() {
                    (u, v) = (u2, v2);
                }
            }
        }
        // The same across a u seam (a B-spline tube closed in u but not periodic: Newton
        // clamped at one end of u where the point lies just past the other).
        if (u == u0 || u == u1)
            && !self.u_periodic()
            && (self.eval(u, v) - p).norm() > tol::ROUNDOFF
        {
            let closed = (0..3).all(|i| {
                let b = v0 + (v1 - v0) * (0.2 + 0.3 * i as f64);
                (self.eval(u0, b) - self.eval(u1, b)).norm() < tol::ROUNDOFF
            });
            if closed {
                let (u2, v2) = newton(if u == u0 { u1 } else { u0 }, v);
                if (self.eval(u2, v2) - p).norm() < (self.eval(u, v) - p).norm() {
                    (u, v) = (u2, v2);
                }
            }
        }
        // Unhinted and farther than the nearest grid point (Newton left its basin), or held off
        // the surface at a seam of a spline closed across it (the grid's basins there lie on
        // both sides): a grid four times as fine, else that grid point.
        let off_at_seam = || {
            let closed_at = |a: P3, b: P3| (a - b).norm() < tol::ROUNDOFF;
            let probe = [0.2, 0.5, 0.8];
            let at_u = (u == u0 || u == u1)
                && probe.iter().all(|f| {
                    closed_at(
                        self.eval(u0, v0 + (v1 - v0) * f),
                        self.eval(u1, v0 + (v1 - v0) * f),
                    )
                });
            let at_v = (v == v0 || v == v1)
                && probe.iter().all(|f| {
                    closed_at(
                        self.eval(u0 + (u1 - u0) * f, v0),
                        self.eval(u0 + (u1 - u0) * f, v1),
                    )
                });
            (at_u || at_v) && (self.eval(u, v) - p).norm() > near_enough
        };
        if let Some((gd, guv)) = grid_best
            && ((self.eval(u, v) - p).norm_squared() > gd || off_at_seam())
        {
            if n < 96 {
                let (u2, v2) = self.project_grid(p, None, 4 * n);
                if (self.eval(u2, v2) - p).norm_squared() < (self.eval(u, v) - p).norm_squared() {
                    (u, v) = (u2, v2);
                }
            }
            if (self.eval(u, v) - p).norm_squared() > gd {
                (u, v) = guv;
            }
        }
        (u, v)
    }

    /// The points of [`Self::project_grid`]'s (n + 1)² grid, row by row (`at` gives a node's
    /// parameters). They don't depend on the point projected, and a B-spline surface (or an
    /// extruded B-spline) is projected unhinted many times over (fitting p-curves, seeding
    /// intersections, a fillet's balls against a wall): its grids are kept, per thread, for the
    /// last few surfaces (known by their `Arc`; the `Weak` held keeps the allocation, so no
    /// other surface can come to share its address).
    fn grid_points(&self, n: usize, at: &dyn Fn(usize, usize) -> (f64, f64)) -> Rc<[P3]> {
        let compute = || -> Rc<[P3]> {
            // An extrusion's points of a row differ by v·d alone: its curve once per row, not
            // per point (each point as `eval` makes it).
            if let Surface::Extrusion { c, d } = self {
                return (0..=n)
                    .flat_map(|i| {
                        let p = c.point(at(i, 0).0);
                        (0..=n).map(move |j| p + d * at(i, j).1)
                    })
                    .collect();
            }
            (0..=n)
                .flat_map(|i| (0..=n).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let (a, b) = at(i, j);
                    self.eval(a, b)
                })
                .collect()
        };
        let key = match self {
            Surface::Nurbs(ns) => GridOf::Nurbs(Arc::downgrade(ns)),
            Surface::Extrusion {
                c: Curve::Nurbs(c),
                d,
            } => GridOf::Extrusion(Arc::downgrade(c), *d),
            _ => return compute(),
        };
        /// The grids kept per thread.
        const KEPT: usize = 8;
        type Kept = Vec<(GridOf, usize, Rc<[P3]>)>;
        thread_local! {
            static GRIDS: RefCell<(Kept, usize)> = const { RefCell::new((Vec::new(), 0)) };
        }
        if let Some(g) = GRIDS.with_borrow(|(kept, _)| {
            kept.iter()
                .find(|(w, m, _)| *m == n && w.is(&key))
                .map(|k| k.2.clone())
        }) {
            return g;
        }
        let g = compute();
        GRIDS.with_borrow_mut(|(kept, next)| {
            let entry = (key, n, g.clone());
            if kept.len() < KEPT {
                kept.push(entry);
            } else {
                kept[*next] = entry;
                *next = (*next + 1) % KEPT;
            }
        });
        g
    }

    fn clamp_u(&self, u: f64, u0: f64, u1: f64) -> f64 {
        if self.u_periodic() {
            u
        } else {
            u.clamp(u0, u1)
        }
    }

    /// The parameter box used for sampling (NURBS: its range; extrusions: curve range × ±1e4).
    /// A box sure to hold the surface over the parameter rectangle u × v. Samples on a grid
    /// grown by the bilinear interpolation's error bound, h²/8 · max |S''| along each
    /// direction (analytic for the quadrics and the torus); a B-spline's control-point hull;
    /// an extrusion's base curve box swept; a revolution's meridian box turned all round.
    pub fn patch_bbox(&self, u: (f64, f64), v: (f64, f64)) -> Bbox {
        let mut bb = Bbox::empty();
        match self {
            Surface::Nurbs(n) => {
                // A rectangle running past the range (a closed surface's p-curves continue
                // across its seam) covers the whole range that way.
                let ((ru0, ru1), (rv0, rv1)) = n.range();
                let (eu, ev) = ((ru1 - ru0) * tol::REL, (rv1 - rv0) * tol::REL);
                let u = if u.0 < ru0 - eu || u.1 > ru1 + eu {
                    (ru0, ru1)
                } else {
                    u
                };
                let v = if v.0 < rv0 - ev || v.1 > rv1 + ev {
                    (rv0, rv1)
                } else {
                    v
                };
                for h in n.sub_net(u, v).iter().flatten() {
                    bb.add(&P3::new(h.x / h.w, h.y / h.w, h.z / h.w));
                }
                return bb;
            }
            Surface::Extrusion { c, d } => {
                let cb = super::boolean::curve_bbox(c, u.0, u.1);
                for t in [v.0, v.1] {
                    bb.add(&(cb.min + d * t));
                    bb.add(&(cb.max + d * t));
                }
                return bb;
            }
            Surface::Revolution { f, m } => {
                let mb = super::boolean::curve_bbox(m, v.0, v.1);
                let (mut rho, mut h) = (0.0f64, (f64::MAX, f64::MIN));
                for i in 0..8 {
                    let c = P3::new(
                        if i & 1 == 0 { mb.min.x } else { mb.max.x },
                        if i & 2 == 0 { mb.min.y } else { mb.max.y },
                        if i & 4 == 0 { mb.min.z } else { mb.max.z },
                    );
                    let l = f.local(&c);
                    rho = rho.max((l.x * l.x + l.y * l.y).sqrt());
                    h = (h.0.min(l.z), h.1.max(l.z));
                }
                for (a, b) in [(-1.0, -1.0), (-1.0, 1.0), (1.0, -1.0), (1.0, 1.0)] {
                    for z in [h.0, h.1] {
                        bb.add(&(f.o + f.x * (a * rho) + f.y * (b * rho) + f.z * z));
                    }
                }
                return bb;
            }
            _ => {}
        }
        // Bounds on |S_uu| and |S_vv| over the rectangle.
        let (auu, avv) = match self {
            Surface::Plane { .. } => (0.0, 0.0),
            Surface::Cylinder { r, .. } => (r.abs(), 0.0),
            Surface::Cone { r, a, .. } => (
                (r + v.0 * a.sin()).abs().max((r + v.1 * a.sin()).abs()),
                0.0,
            ),
            Surface::Sphere { r, .. } => (r.abs(), r.abs()),
            Surface::Torus { rmaj, rmin, .. } => (rmaj.abs() + rmin.abs(), rmin.abs()),
            _ => unreachable!(),
        };
        let n = 8;
        let (hu, hv) = ((u.1 - u.0) / n as f64, (v.1 - v.0) / n as f64);
        for i in 0..=n {
            for j in 0..=n {
                bb.add(&self.eval(u.0 + hu * i as f64, v.0 + hv * j as f64));
            }
        }
        let e = auu * hu * hu / 8.0 + avv * hv * hv / 8.0;
        bb.grow(e * (1.0 + tol::REL) + tol::ROUNDOFF_FINE)
    }

    /// The surface over the parameter rectangle u × v as an exact NURBS surface on the same
    /// parameters (a B-spline surface as it is): a plane bilinear; a cylinder, cone, sphere,
    /// torus or revolution its meridian turned by a rational quadratic arc (the meridian's
    /// control points rotated by the arc's, weights multiplied); an extrusion its curve swept.
    pub fn to_nurbs(&self, u: (f64, f64), v: (f64, f64)) -> NurbsSurface {
        let h = |p: P3, w: f64| H::new(p.x * w, p.y * w, p.z * w, w);
        let line = |a: P3, b: P3| NurbsCurve::bezier(&[a, b], &[1.0, 1.0]).reparam(v.0, v.1);
        // A local meridian (x radial, z along the axis) turned about f.z.
        let revolve = |f: &Frame, m: &NurbsCurve| {
            let arc = super::curve::conic_nurbs(P3::origin(), V3::x(), V3::y(), u.0, u.1);
            let cps = arc
                .cps
                .iter()
                .map(|a| {
                    let (ax, ay, wa) = (a.x / a.w, a.y / a.w, a.w);
                    m.cps
                        .iter()
                        .map(|q| {
                            let (mx, my, mz, wm) = (q.x / q.w, q.y / q.w, q.z / q.w, q.w);
                            let p = f.o
                                + f.x * (mx * ax - my * ay)
                                + f.y * (mx * ay + my * ax)
                                + f.z * mz;
                            h(p, wa * wm)
                        })
                        .collect()
                })
                .collect();
            NurbsSurface {
                du: arc.degree,
                dv: m.degree,
                ku: arc.knots.clone(),
                kv: m.knots.clone(),
                cps,
            }
        };
        let o = P3::origin();
        match self {
            Surface::Plane { .. } => {
                let c = |a: f64, b: f64| h(self.eval(a, b), 1.0);
                NurbsSurface {
                    du: 1,
                    dv: 1,
                    ku: vec![u.0, u.0, u.1, u.1],
                    kv: vec![v.0, v.0, v.1, v.1],
                    cps: vec![
                        vec![c(u.0, v.0), c(u.0, v.1)],
                        vec![c(u.1, v.0), c(u.1, v.1)],
                    ],
                }
            }
            Surface::Cylinder { f, r } => {
                revolve(f, &line(P3::new(*r, 0.0, v.0), P3::new(*r, 0.0, v.1)))
            }
            Surface::Cone { f, r, a } => {
                let m = |t: f64| P3::new(r + t * a.sin(), 0.0, t * a.cos());
                revolve(f, &line(m(v.0), m(v.1)))
            }
            Surface::Sphere { f, r } => revolve(
                f,
                &super::curve::conic_nurbs(o, V3::x() * *r, V3::z() * *r, v.0, v.1),
            ),
            Surface::Torus { f, rmaj, rmin } => revolve(
                f,
                &super::curve::conic_nurbs(
                    P3::new(*rmaj, 0.0, 0.0),
                    V3::x() * *rmin,
                    V3::z() * *rmin,
                    v.0,
                    v.1,
                ),
            ),
            Surface::Revolution { f, m } => {
                let mut mc = m.to_nurbs(v.0, v.1);
                for q in mc.cps.iter_mut() {
                    let l = f.local(&P3::new(q.x / q.w, q.y / q.w, q.z / q.w));
                    *q = h(P3::from(l), q.w);
                }
                revolve(f, &mc)
            }
            Surface::Extrusion { c, d } => {
                let cn = c.to_nurbs(u.0, u.1);
                let cps = cn
                    .cps
                    .iter()
                    .map(|q| {
                        let p = P3::new(q.x / q.w, q.y / q.w, q.z / q.w);
                        vec![h(p + d * v.0, q.w), h(p + d * v.1, q.w)]
                    })
                    .collect();
                NurbsSurface {
                    du: cn.degree,
                    dv: 1,
                    ku: cn.knots.clone(),
                    kv: vec![v.0, v.0, v.1, v.1],
                    cps,
                }
            }
            Surface::Nurbs(n) => (**n).clone(),
        }
    }

    /// The parameter ranges (an unbounded direction as ±1e4; a periodic one as one period).
    pub fn param_box(&self) -> ((f64, f64), (f64, f64)) {
        let open = (-1e4, 1e4);
        match self {
            Surface::Nurbs(n) => n.range(),
            Surface::Plane { .. } => (open, open),
            Surface::Extrusion { c, .. } => (
                c.nurbs_range()
                    .unwrap_or(if c.is_periodic() { (0.0, TAU) } else { open }),
                open,
            ),
            Surface::Sphere { .. } => ((0.0, TAU), (-PI / 2.0, PI / 2.0)),
            Surface::Torus { .. } => ((0.0, TAU), (-PI, PI)),
            Surface::Revolution { m, .. } => (
                (0.0, TAU),
                m.nurbs_range()
                    .unwrap_or(if m.is_periodic() { (0.0, TAU) } else { open }),
            ),
            Surface::Cylinder { .. } | Surface::Cone { .. } => ((0.0, TAU), open),
        }
    }

    /// Signed distance from `p` to an analytic surface (positive on the natural normal's side).
    pub fn signed_distance(&self, p: &P3) -> Option<f64> {
        Some(match self {
            Surface::Plane { f } => (p - f.o).dot(&f.x.cross(&f.y)),
            Surface::Cylinder { f, r } => {
                let l = f.local(p);
                (l.x * l.x + l.y * l.y).sqrt() - r
            }
            Surface::Cone { f, r, a } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let (sa, ca) = a.sin_cos();
                // Distance to the line (r + v sa, v ca) in the meridian plane, normal (ca, −sa).
                (rho - r) * ca - l.z * sa
            }
            Surface::Sphere { f, r } => (p - f.o).norm() - r,
            Surface::Torus { f, rmaj, rmin } => {
                let l = f.local(p);
                let rho = (l.x * l.x + l.y * l.y).sqrt();
                let near = (rho - rmaj).hypot(l.z) - rmin;
                // A spindle torus (minor radius past the major) is also the far meridian's
                // turn, at radius −ρ: the nearer of the two.
                let far = (rho + rmaj).hypot(l.z) - rmin;
                if rmin > rmaj && far.abs() < near.abs() {
                    far
                } else {
                    near
                }
            }
            _ => return None,
        })
    }

    /// Distance from `p` to the surface (exact for analytic surfaces, by projection otherwise).
    pub fn distance(&self, p: &P3) -> f64 {
        if let Some(d) = self.signed_distance(p) {
            return d.abs();
        }
        let (u, v) = self.project(p, None);
        (self.eval(u, v) - p).norm()
    }

    /// The surface moved by `m`, with the same parametrization.
    pub fn transformed(&self, m: &Affine) -> Surface {
        let s = m.scale();
        let tf = |f: &Frame| {
            let g = f.transformed(m);
            Frame {
                o: g.o,
                x: g.x / s,
                y: g.y / s,
                z: g.z / s,
            }
        };
        match self {
            Surface::Plane { f } => Surface::Plane { f: tf(f) },
            Surface::Cylinder { f, r } => Surface::Cylinder { f: tf(f), r: r * s },
            Surface::Cone { f, r, a } => Surface::Cone {
                f: tf(f),
                r: r * s,
                a: *a,
            },
            Surface::Sphere { f, r } => Surface::Sphere { f: tf(f), r: r * s },
            Surface::Torus { f, rmaj, rmin } => Surface::Torus {
                f: tf(f),
                rmaj: rmaj * s,
                rmin: rmin * s,
            },
            Surface::Revolution { f, m: c } => Surface::Revolution {
                f: tf(f),
                m: c.transformed(m),
            },
            Surface::Extrusion { c, d } => Surface::Extrusion {
                c: c.transformed(m),
                d: m.vector(d),
            },
            Surface::Nurbs(n) => Surface::Nurbs(Arc::new(n.transformed(m))),
        }
    }

    /// Typical lengths per unit of u and v (for isotropic meshing and step sizes).
    pub fn metric(&self) -> (f64, f64) {
        match self {
            Surface::Plane { .. } => (1.0, 1.0),
            Surface::Cylinder { r, .. } => (*r, 1.0),
            Surface::Cone { r, .. } => (r.abs().max(1e-3), 1.0),
            Surface::Sphere { r, .. } => (*r, *r),
            Surface::Torus { rmaj, rmin, .. } => (rmaj + rmin, *rmin),
            _ => (1.0, 1.0),
        }
    }

    /// The axis of a surface of revolution.
    pub fn axis(&self) -> Option<(P3, V3)> {
        match self {
            Surface::Cylinder { f, .. }
            | Surface::Cone { f, .. }
            | Surface::Sphere { f, .. }
            | Surface::Torus { f, .. }
            | Surface::Revolution { f, .. } => Some((f.o, f.z)),
            _ => None,
        }
    }

    /// The u = const iso curve, parametrized by v.
    pub fn iso_u(&self, u: f64) -> Option<Curve> {
        Some(match self {
            Surface::Plane { f } => Curve::Line {
                o: f.o + f.x * u,
                d: f.y,
            },
            Surface::Cylinder { f, r } => Curve::Line {
                o: f.o + (f.x * u.cos() + f.y * u.sin()) * *r,
                d: f.z,
            },
            Surface::Cone { f, r, a } => {
                let rho = f.x * u.cos() + f.y * u.sin();
                Curve::Line {
                    o: f.o + rho * *r,
                    d: rho * a.sin() + f.z * a.cos(),
                }
            }
            Surface::Sphere { f, r } => Curve::Circle {
                c: f.o,
                x: f.x * u.cos() + f.y * u.sin(),
                y: f.z,
                r: *r,
            },
            Surface::Torus { f, rmaj, rmin } => {
                let rho = f.x * u.cos() + f.y * u.sin();
                Curve::Circle {
                    c: f.o + rho * *rmaj,
                    x: rho,
                    y: f.z,
                    r: *rmin,
                }
            }
            _ => return None,
        })
    }

    /// The v = const iso curve, parametrized by u.
    pub fn iso_v(&self, v: f64) -> Option<Curve> {
        Some(match self {
            Surface::Plane { f } => Curve::Line {
                o: f.o + f.y * v,
                d: f.x,
            },
            Surface::Cylinder { f, r } => Curve::Circle {
                c: f.o + f.z * v,
                x: f.x,
                y: f.y,
                r: *r,
            },
            Surface::Cone { f, r, a } => Curve::Circle {
                c: f.o + f.z * (v * a.cos()),
                x: f.x,
                y: f.y,
                r: r + v * a.sin(),
            },
            Surface::Sphere { f, r } => Curve::Circle {
                c: f.o + f.z * (r * v.sin()),
                x: f.x,
                y: f.y,
                r: r * v.cos(),
            },
            Surface::Torus { f, rmaj, rmin } => Curve::Circle {
                c: f.o + f.z * (rmin * v.sin()),
                x: f.x,
                y: f.y,
                r: rmaj + rmin * v.cos(),
            },
            Surface::Extrusion { c, d } => c.transformed(&Affine::translation(d * v)),
            _ => return None,
        })
    }

    /// Whether two surfaces are the same point set (same plane, cylinder, ...). `Some(true)`
    /// when their natural normals agree, `Some(false)` when opposite.
    pub fn same_domain(&self, o: &Surface, tol: f64) -> Option<bool> {
        let par = |a: &V3, b: &V3| -> Option<bool> {
            let d = a.dot(b);
            if d > 1.0 - tol::ANGULAR {
                Some(true)
            } else if d < -1.0 + tol::ANGULAR {
                Some(false)
            } else {
                None
            }
        };
        let on_axis = |fa: &Frame, fb: &Frame| -> bool {
            let d = fb.o - fa.o;
            (d - fa.z * d.dot(&fa.z)).norm() < tol
        };
        match (self, o) {
            (Surface::Plane { f: a }, Surface::Plane { f: b }) => {
                let (na, nb) = (a.x.cross(&a.y), b.x.cross(&b.y));
                let same = par(&na, &nb)?;
                if (b.o - a.o).dot(&na).abs() < tol {
                    Some(same)
                } else {
                    None
                }
            }
            (Surface::Cylinder { f: a, r: ra }, Surface::Cylinder { f: b, r: rb }) => {
                par(&a.z, &b.z)?;
                if (ra - rb).abs() < tol && on_axis(a, b) {
                    // The natural normal points away from the axis exactly when the frame is
                    // right-handed.
                    Some(a.right_handed() == b.right_handed())
                } else {
                    None
                }
            }
            (Surface::Sphere { f: a, r: ra }, Surface::Sphere { f: b, r: rb }) => {
                if (ra - rb).abs() < tol && (a.o - b.o).norm() < tol {
                    Some(a.right_handed() == b.right_handed())
                } else {
                    None
                }
            }
            (
                Surface::Torus {
                    f: a,
                    rmaj: ra,
                    rmin: sa,
                },
                Surface::Torus {
                    f: b,
                    rmaj: rb,
                    rmin: sb,
                },
            ) => {
                par(&a.z, &b.z)?;
                if (ra - rb).abs() < tol && (sa - sb).abs() < tol && (a.o - b.o).norm() < tol {
                    Some(a.right_handed() == b.right_handed())
                } else {
                    None
                }
            }
            (Surface::Cone { f: a, r: ra, a: aa }, Surface::Cone { f: b, r: rb, a: ab }) => {
                // Compare the apex and the opening direction.
                let apex = |f: &Frame, r: f64, ang: f64| f.o - f.z * (r / ang.tan());
                let (pa, pb) = (apex(a, *ra, *aa), apex(b, *rb, *ab));
                if (pa - pb).norm() > tol * 10.0 {
                    return None;
                }
                let da = a.z * aa.cos() + V3::zeros();
                let _ = da;
                let open_a = if aa.sin() > 0.0 { a.z } else { -a.z };
                let open_b = if ab.sin() > 0.0 { b.z } else { -b.z };
                if open_a.dot(&open_b) < 1.0 - tol::ANGULAR
                    || (aa.sin().abs() - ab.sin().abs()).abs() > tol::ANGULAR
                {
                    return None;
                }
                Some(a.right_handed() == b.right_handed())
            }
            (Surface::Revolution { f: a, m: ma }, Surface::Revolution { f: b, m: mb }) => {
                // About the same axis, each meridian on the other surface (two lines that are
                // the same hyperboloid's rulings, a revolve's side swept twice).
                par(&a.z, &b.z)?;
                if !on_axis(a, b) {
                    return None;
                }
                let samples = |m: &Curve| -> Vec<f64> {
                    match (m.nurbs_range(), m) {
                        (Some((t0, t1)), _) => {
                            (0..=8).map(|i| t0 + (t1 - t0) * i as f64 / 8.0).collect()
                        }
                        (None, Curve::Line { d, .. }) => [-10.0, -3.0, -1.0, 0.0, 1.0, 3.0, 10.0]
                            .iter()
                            .map(|x| x / d.norm().max(tol::TINY))
                            .collect(),
                        _ => (0..8).map(|i| TAU * i as f64 / 8.0).collect(),
                    }
                };
                for (s, m) in [(self, mb), (o, ma)] {
                    for t in samples(m) {
                        let p = m.point(t);
                        let (u, v) = s.project(&p, None);
                        if (s.eval(u, v) - p).norm() > tol {
                            return None;
                        }
                    }
                }
                let p = mb.point(samples(mb)[3]);
                let ((ua, va), (ub, vb)) = (self.project(&p, None), o.project(&p, None));
                Some(self.normal(ua, va).dot(&o.normal(ub, vb)) > 0.0)
            }
            _ => sampled_same_domain(self, o, tol),
        }
    }
}

/// Whether a surface is compared by samples (B-splines and extrusions: no closed form says
/// when two are one point set).
fn sampled_kind(s: &Surface) -> bool {
    matches!(s, Surface::Nurbs(_) | Surface::Extrusion { .. })
}

/// Parameters of a curve to sample a sweep of it at: its range, a whole turn, or a stretch of
/// a line.
fn curve_samples(c: &Curve, n: usize) -> Vec<f64> {
    let (t0, t1) = match (c.nurbs_range(), c) {
        (Some(r), _) => r,
        (None, Curve::Line { d, .. }) => (
            -10.0 / d.norm().max(tol::TINY),
            10.0 / d.norm().max(tol::TINY),
        ),
        _ => (0.0, TAU),
    };
    (0..=n)
        .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
        .collect()
}

/// Points of a B-spline or an extrusion to test against another surface, by rows (each row a
/// curve of the surface, so a projection can follow it): a B-spline over its range; an
/// extrusion along its base curve at three heights.
fn coincidence_rows(s: &Surface) -> Vec<Vec<(f64, f64)>> {
    match s {
        Surface::Nurbs(n) => {
            let ((u0, u1), (v0, v1)) = n.range();
            let k = 8;
            (0..=k)
                .map(|i| {
                    let v = v0 + (v1 - v0) * i as f64 / k as f64;
                    (0..=k)
                        .map(|j| (u0 + (u1 - u0) * j as f64 / k as f64, v))
                        .collect()
                })
                .collect()
        }
        Surface::Extrusion { c, d } => {
            let ts = curve_samples(c, 12);
            let size = {
                let mut bb = Bbox::empty();
                for &t in &ts {
                    bb.add(&c.point(t));
                }
                bb.diagonal().max(1.0)
            };
            let h = size / d.norm().max(tol::TINY);
            [-h, 0.0, h]
                .iter()
                .map(|&v| ts.iter().map(|&u| (u, v)).collect())
                .collect()
        }
        _ => Vec::new(),
    }
}

/// How the points of `a` (its [`coincidence_rows`]) lie on `b`: how many are on it (within
/// `tol`), and whether one is off it where `b` is (not merely beyond the edge of `b`'s
/// parameter range, where `b` stops: surfaces that coincide over part of each). Each
/// projection starts from the row's previous one; points beyond `b`'s box are passed over.
/// None as soon as a point is off; otherwise the count and a point that is on (for the
/// orientation).
fn points_on(a: &Surface, b: &Surface, tol: f64) -> Option<(usize, Option<P3>)> {
    let range = match b {
        Surface::Nurbs(n) => Some(n.range()),
        Surface::Extrusion { c, .. } => c
            .nurbs_range()
            .map(|r| (r, (f64::NEG_INFINITY, f64::INFINITY))),
        _ => None,
    };
    let bbox = range.map(|((u0, u1), (v0, v1))| {
        let v = if v0.is_finite() {
            (v0, v1)
        } else {
            (-1e4, 1e4)
        };
        b.patch_bbox((u0, u1), v).grow(tol)
    });
    let at_edge = |x: f64, lo: f64, hi: f64| {
        lo.is_finite()
            && ((x - lo).abs() <= (hi - lo).abs() * tol::REL
                || (x - hi).abs() <= (hi - lo).abs() * tol::REL)
    };
    let mut on = 0;
    let mut first: Option<P3> = None;
    for row in coincidence_rows(a) {
        let mut hint: Option<(f64, f64)> = None;
        for (u, v) in row {
            let p = a.eval(u, v);
            if bbox.as_ref().is_some_and(|bb| !bb.contains(&p)) {
                hint = None;
                continue;
            }
            let q = b.project(&p, hint);
            let q = if (b.eval(q.0, q.1) - p).norm() > tol && hint.is_some() {
                b.project(&p, None)
            } else {
                q
            };
            hint = Some(q);
            if (b.eval(q.0, q.1) - p).norm() <= tol {
                on += 1;
                first.get_or_insert(p);
                continue;
            }
            let beyond = range
                .is_some_and(|((u0, u1), (v0, v1))| at_edge(q.0, u0, u1) || at_edge(q.1, v0, v1));
            if !beyond {
                return None;
            }
        }
    }
    Some((on, first))
}

/// [`Surface::same_domain`] for B-splines and extrusions, with each other or with the closed-form
/// surfaces: the same point set where both are, tested on samples (robust to the surfaces'
/// parametrizations and to their covering different extents: a slab's wall over part of the
/// body's). Every sample of either lying where the other is must be on it, within `tol`, and
/// enough must be (a few, from one side at least). Extrusions must also run the same way. A
/// closed-form surface goes on indefinitely (or closes), so only the other's samples are tested
/// on it. The orientation is the normals' at a common point.
fn sampled_same_domain(a: &Surface, b: &Surface, tol: f64) -> Option<bool> {
    if !sampled_kind(a) && !sampled_kind(b) {
        return None;
    }
    if let (Surface::Extrusion { d: da, .. }, Surface::Extrusion { d: db, .. }) = (a, b)
        && da.normalize().cross(&db.normalize()).norm() > tol::ANGULAR
    {
        return None;
    }
    let mut common: Option<P3> = None;
    let mut count = 0;
    for (x, y) in [(a, b), (b, a)] {
        if !sampled_kind(x) {
            continue;
        }
        let (n, p) = points_on(x, y, tol)?;
        count = count.max(n);
        if common.is_none() {
            common = p;
        }
    }
    if count < 3 {
        return None;
    }
    let p = common?;
    let ((ua, va), (ub, vb)) = (a.project(&p, None), b.project(&p, None));
    let (na, nb) = (a.normal(ua, va), b.normal(ub, vb));
    let d = na.dot(&nb);
    if d.abs() < 0.5 {
        return None;
    }
    Some(d > 0.0)
}

/// Whether a surface of revolution's meridian leaves its frame's xz plane (a curve skew to the
/// axis). The usual meridian lies in it, and the rest of the kernel takes v from the point
/// turned into that plane.
pub fn meridian_off_plane(f: &Frame, m: &Curve) -> bool {
    let off = |p: &P3| {
        let l = f.local(p);
        l.y.abs() > tol::REL * (1.0 + l.norm())
    };
    let off_v = |d: &V3| f.y.dot(d).abs() > tol::REL_FINE * d.norm();
    match m {
        Curve::Line { o, d } => off(o) || off_v(d),
        Curve::Circle { c, x, y, .. } | Curve::Ellipse { c, x, y, .. } => {
            off(c) || off_v(x) || off_v(y)
        }
        Curve::Nurbs(n) => n.cps.iter().any(|h| off(&super::nurbs::dehom(h))),
    }
}

/// The azimuth (about the frame's z, from its x) of a surface of revolution's meridian point at
/// `v`: 0 for a meridian in the xz half plane.
pub fn meridian_azimuth(f: &Frame, m: &Curve, v: f64) -> f64 {
    let l = f.local(&m.point(v));
    if l.x.hypot(l.y) < tol::TINY {
        0.0
    } else {
        l.y.atan2(l.x)
    }
}

/// [`Surface::project`] on a surface of revolution whose meridian is off its frame's xz plane:
/// v makes the meridian's (ρ, h) nearest the point's (the distance from a point to a circle
/// about the axis is that of their (ρ, h)), u turns the meridian point to the point's azimuth.
fn project_off_plane(f: &Frame, m: &Curve, p: &P3, hint: Option<(f64, f64)>) -> (f64, f64) {
    let l = f.local(p);
    let (rho, h) = (l.x.hypot(l.y), l.z);
    let (a, b) = match m.nurbs_range() {
        Some(r) => r,
        None if m.is_periodic() => {
            let c = hint.map(|h| h.1).unwrap_or(0.0);
            (c - PI, c + PI)
        }
        None => (-1e12, 1e12),
    };
    // (ρ, h) of the meridian at v and the half gradient of the squared distance, with its
    // derivative.
    let at = |v: f64| {
        let (q, d1, d2) = m.d2(v);
        let (q, d1, d2) = (
            f.local(&q),
            V3::new(d1.dot(&f.x), d1.dot(&f.y), d1.dot(&f.z)),
            V3::new(d2.dot(&f.x), d2.dot(&f.y), d2.dot(&f.z)),
        );
        let r = q.x.hypot(q.y).max(tol::TINY);
        let r1 = (q.x * d1.x + q.y * d1.y) / r;
        let r2 = (d1.x * d1.x + d1.y * d1.y + q.x * d2.x + q.y * d2.y) / r - r1 * r1 / r;
        let (dr, dh) = (r - rho, q.z - h);
        (
            dr * dr + dh * dh,
            dr * r1 + dh * d1.z,
            r1 * r1 + dr * r2 + d1.z * d1.z + dh * d2.z,
        )
    };
    let newton = |s: f64| {
        let mut v = s.clamp(a, b);
        for _ in 0..40 {
            let (_, g, gp) = at(v);
            if !(gp.abs() > tol::TINY) {
                return (v, false);
            }
            let nv = (v - g / gp).clamp(a, b);
            let step = (nv - v).abs();
            v = nv;
            if step < 1e-15 * (1.0 + v.abs()) {
                return (v, true);
            }
        }
        (v, false)
    };
    let finish = |v: f64| {
        let az = l.y.atan2(l.x) - meridian_azimuth(f, m, v);
        let u = match hint {
            Some((hu, _)) => unwrap_near(az, hu, TAU),
            None => wrap(az, 0.0, TAU),
        };
        (u, v)
    };
    // Hinted (a projection that follows a curve along): Newton from the hint, when it settles
    // on a minimum; the many starts below are for a point with nothing to go by.
    if let Some((_, hv)) = hint {
        let (v, settled) = newton(hv);
        if settled && at(v).2 > 0.0 {
            return finish(v);
        }
    }
    let mut starts: Vec<f64> = Vec::new();
    if let Some((_, hv)) = hint {
        starts.push(hv);
    }
    starts.push(m.closest(p, a, b));
    match m {
        Curve::Line { o, d } => {
            // Where the line's height is the point's, and where its distance from the axis is.
            let (lo, ld) = (f.local(o), V3::new(d.dot(&f.x), d.dot(&f.y), d.dot(&f.z)));
            if ld.z.abs() > tol::TINY {
                starts.push((h - lo.z) / ld.z);
            }
            let (qa, qb, qc) = (
                ld.x * ld.x + ld.y * ld.y,
                2.0 * (lo.x * ld.x + lo.y * ld.y),
                lo.x * lo.x + lo.y * lo.y - rho * rho,
            );
            if qa > tol::TINY {
                let disc = qb * qb - 4.0 * qa * qc;
                let sq = disc.max(0.0).sqrt();
                starts.extend([(-qb - sq) / (2.0 * qa), (-qb + sq) / (2.0 * qa)]);
            }
        }
        _ => starts.extend((0..=32).map(|i| a + (b - a) * i as f64 / 32.0)),
    }
    let mut best = (f64::INFINITY, starts[0].clamp(a, b));
    for s in starts {
        let (v, _) = newton(s);
        let d = at(v).0;
        if d < best.0 {
            best = (d, v);
        }
    }
    finish(best.1)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point on a cone's other nappe (past its apex) projects onto itself (regression: the
    /// projection took the generator on its own side of the axis, 1.41 away).
    #[test]
    fn projection_onto_a_cone_past_its_apex() {
        let f = Frame::from_z(P3::origin(), V3::z());
        let cone = Surface::Cone {
            f,
            r: 0.0,
            a: -std::f64::consts::FRAC_PI_4,
        };
        for (u, v) in [(0.3, 2.0), (2.0, -1.5), (4.0, 0.7)] {
            let p = cone.eval(u, v);
            let (pu, pv) = cone.project(&p, None);
            assert!(
                (cone.eval(pu, pv) - p).norm() < 1e-12,
                "({u}, {v}): {:?} projects to {:?}",
                p,
                cone.eval(pu, pv)
            );
        }
    }

    /// Projection onto a B-spline surface that folds back on itself between the points of the
    /// start grid (a meander extruded): every point of the surface projects onto itself
    /// (regression: Newton from the nearest grid point stopped on the neighbouring fold).
    #[test]
    fn projection_onto_a_folded_surface() {
        let mut pts = Vec::new();
        for k in 0..4 {
            let y = k as f64 * 1.0;
            let xs = if k % 2 == 0 { [0.0, 10.0] } else { [10.0, 0.0] };
            pts.push(P3::new(xs[0], y, 0.0));
            pts.push(P3::new(xs[1], y, 0.0));
        }
        let t: Vec<f64> = (0..pts.len()).map(|i| i as f64).collect();
        let c = NurbsCurve::interpolate(&pts, &t, 3, None, None);
        let cps: Vec<Vec<H>> = c
            .cps
            .iter()
            .map(|h| vec![*h, h + H::new(0.0, 0.0, h.w, 0.0)])
            .collect();
        let s = Surface::Nurbs(Arc::new(
            NurbsSurface::new(c.degree, 1, c.knots.clone(), vec![0.0, 0.0, 1.0, 1.0], cps).unwrap(),
        ));
        let ((u0, u1), _) = s.param_box();
        for i in 0..=200 {
            let p = s.eval(u0 + (u1 - u0) * i as f64 / 200.0, 0.37);
            let (u, v) = s.project(&p, None);
            assert!(
                (s.eval(u, v) - p).norm() < 1e-9,
                "{p:?} projects {:.3e} away",
                (s.eval(u, v) - p).norm()
            );
        }
    }

    /// Coincidence of B-spline and extrusion surfaces, sampled: an extrusion of a spline and the
    /// same spline extruded twice as far; a B-spline cylinder patch and the cylinder; two patches
    /// of one cylinder over different, overlapping ranges (and parametrizations). Surfaces only
    /// near each other, or crossing, aren't.
    #[test]
    fn sampled_same_domain() {
        let pts: Vec<P3> = [(0.0, 0.0), (1.0, 2.0), (3.0, 2.5), (5.0, 1.0)]
            .iter()
            .map(|&(x, y)| P3::new(x, y, 0.0))
            .collect();
        let c = Curve::Nurbs(Arc::new(NurbsCurve::interpolate(
            &pts,
            &[0.0, 1.0, 2.0, 3.0],
            3,
            None,
            None,
        )));
        let e1 = Surface::Extrusion {
            c: c.clone(),
            d: V3::new(0.0, 0.0, 1.0),
        };
        let e2 = Surface::Extrusion {
            c: c.clone(),
            d: V3::new(0.0, 0.0, 2.0),
        };
        assert_eq!(e1.same_domain(&e2, 1e-7), Some(true));
        let off = Surface::Extrusion {
            c: c.transformed(&Affine::translation(V3::new(0.0, 0.01, 0.0))),
            d: V3::new(0.0, 0.0, 1.0),
        };
        assert_eq!(e1.same_domain(&off, 1e-7), None);
        let tilted = Surface::Extrusion {
            c,
            d: V3::new(0.0, 0.3, 1.0),
        };
        assert_eq!(e1.same_domain(&tilted, 1e-7), None);
        let f = Frame::from_z(P3::new(1.0, 2.0, 3.0), V3::new(0.0, 0.0, 1.0));
        let cyl = Surface::Cylinder { f, r: 4.0 };
        let patch = Surface::Nurbs(Arc::new(cyl.to_nurbs((0.0, PI), (0.0, 5.0))));
        assert_eq!(patch.same_domain(&cyl, 1e-7), Some(true));
        assert_eq!(cyl.same_domain(&patch, 1e-7), Some(true));
        assert_eq!(
            patch.same_domain(&Surface::Cylinder { f, r: 4.01 }, 1e-7),
            None
        );
        let other = Surface::Nurbs(Arc::new(cyl.to_nurbs((PI / 2.0, 1.5 * PI), (2.0, 8.0))));
        assert_eq!(patch.same_domain(&other, 1e-7), Some(true));
        let crossing = Surface::Nurbs(Arc::new(
            Surface::Cylinder {
                f: Frame::from_z(P3::new(4.0, 2.0, 3.0), V3::new(0.0, 0.0, 1.0)),
                r: 4.0,
            }
            .to_nurbs((0.0, TAU), (0.0, 5.0)),
        ));
        assert_eq!(patch.same_domain(&crossing, 1e-7), None);
        let plane = Surface::Plane { f };
        let flat = Surface::Nurbs(Arc::new(plane.to_nurbs((-1.0, 1.0), (-1.0, 1.0))));
        assert_eq!(flat.same_domain(&plane, 1e-7), Some(true));
    }

    fn check_derivs(s: &Surface, u: f64, v: f64) {
        let h = 1e-6;
        let d = s.d2(u, v);
        let num_u = (s.eval(u + h, v) - s.eval(u - h, v)) / (2.0 * h);
        let num_v = (s.eval(u, v + h) - s.eval(u, v - h)) / (2.0 * h);
        assert!((num_u - d[1]).norm() < 1e-6, "{s:?} Su {num_u} {}", d[1]);
        assert!((num_v - d[2]).norm() < 1e-6, "{s:?} Sv");
        let duu = (s.d2(u + h, v)[1] - s.d2(u - h, v)[1]) / (2.0 * h);
        let duv = (s.d2(u, v + h)[1] - s.d2(u, v - h)[1]) / (2.0 * h);
        let dvv = (s.d2(u, v + h)[2] - s.d2(u, v - h)[2]) / (2.0 * h);
        assert!((duu - d[3]).norm() < 1e-5, "{s:?} Suu");
        assert!((duv - d[4]).norm() < 1e-5, "{s:?} Suv");
        assert!((dvv - d[5]).norm() < 1e-5, "{s:?} Svv");
        let (pu, pv) = s.project(&s.eval(u, v), Some((u, v)));
        assert!(
            (s.eval(pu, pv) - s.eval(u, v)).norm() < 1e-9,
            "{s:?} project"
        );
    }

    #[test]
    fn analytic_derivatives_and_projection() {
        let f = Frame::from_xz(
            P3::new(1.0, 2.0, 3.0),
            V3::new(1.0, 1.0, 0.0),
            V3::new(0.0, 0.3, 1.0),
        );
        let m = Curve::Circle {
            c: f.o + f.x * 5.0,
            x: f.x,
            y: f.z,
            r: 2.0,
        };
        for s in [
            Surface::Plane { f },
            Surface::Cylinder { f, r: 3.0 },
            Surface::Cone { f, r: 3.0, a: 0.4 },
            Surface::Sphere { f, r: 3.0 },
            Surface::Torus {
                f,
                rmaj: 5.0,
                rmin: 2.0,
            },
            Surface::Revolution { f, m },
            Surface::Extrusion {
                c: Curve::Ellipse {
                    c: P3::origin(),
                    x: V3::x(),
                    y: V3::y(),
                    a: 3.0,
                    b: 2.0,
                },
                d: V3::new(0.1, 0.0, 1.0),
            },
        ] {
            check_derivs(&s, 0.7, 0.4);
            if let Some(d) = s.signed_distance(&(s.eval(0.7, 0.4) + s.normal(0.7, 0.4) * 0.5)) {
                assert!((d - 0.5).abs() < 1e-12, "{s:?} distance {d}");
            }
        }
    }

    #[test]
    fn nurbs_forms_match_the_surfaces() {
        let f = Frame {
            o: P3::new(1.0, -2.0, 0.5),
            x: V3::new(0.6, 0.8, 0.0),
            y: V3::new(-0.8, 0.6, 0.0),
            z: V3::z(),
        };
        // (A revolution's meridian lies in its frame's x–z half plane.)
        let m = |r: f64, z: f64| f.o + f.x * r + f.z * z;
        let meridian = Curve::nurbs(NurbsCurve::bezier(
            &[m(3.0, 0.0), m(5.0, 1.5), m(3.4, 3.5)],
            &[1.0, 1.0, 1.0],
        ));
        let surfs = [
            Surface::Plane { f },
            Surface::Cylinder { f, r: 2.0 },
            Surface::Cone { f, r: 2.0, a: 0.3 },
            Surface::Sphere { f, r: 3.0 },
            Surface::Torus {
                f,
                rmaj: 5.0,
                rmin: 1.5,
            },
            Surface::Revolution { f, m: meridian },
            Surface::Extrusion {
                c: Curve::Ellipse {
                    c: P3::new(0.0, 0.0, 0.0),
                    x: V3::x(),
                    y: V3::y(),
                    a: 3.0,
                    b: 2.0,
                },
                d: V3::new(0.1, 0.0, 1.0),
            },
        ];
        for s in &surfs {
            let (u, v) = match s {
                Surface::Sphere { .. } => ((0.3, 4.0), (-1.2, 1.4)),
                Surface::Revolution { .. } => ((0.3, 4.0), (0.0, 1.0)),
                _ => ((0.3, 4.0), (-1.0, 2.5)),
            };
            let n = s.to_nurbs(u, v);
            for i in 0..=8 {
                for j in 0..=8 {
                    let (a, b) = (
                        u.0 + (u.1 - u.0) * i as f64 / 8.0,
                        v.0 + (v.1 - v.0) * j as f64 / 8.0,
                    );
                    let want = s.eval(a, b);
                    // The NURBS form's parameters differ inside an arc (rational): compare points
                    // by distance to the surface and the corners exactly.
                    let got = n.ders2(a, b)[0];
                    let p = P3::from(got);
                    assert!(
                        s.distance(&p) < 1e-9,
                        "{} off the surface by {} at {i} {j}: {p:?} vs {want:?}",
                        format!("{s:?}").chars().take(12).collect::<String>(),
                        s.distance(&p)
                    );
                    if (i == 0 || i == 8) && (j == 0 || j == 8) {
                        assert!(
                            (p - want).norm() < 1e-9,
                            "{:?} corner {p:?} vs {want:?}",
                            s.kind()
                        );
                    }
                }
            }
        }
    }
}

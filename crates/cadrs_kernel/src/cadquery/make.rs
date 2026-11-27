//! CadQuery's constructors (`Edge.make…`, `Wire.make…`, `Face.makeFromWires`, `Solid.make…`)
//! and the wire and planar-face bodies operations take their inputs as.

use super::geom::{Plane, default_x};
use super::shape::{EdgeGeom, Shape};
use super::{CqError, CqResult, op1, value_err};
use crate::native::curve::Curve;
use crate::native::math::{Affine, Frame, P3, V3};
use crate::native::nurbs::NurbsCurve;
use crate::native::surface::Surface;
use crate::native::topo::{Body, Coedge, Face, Shell};
use crate::{Axis, Kernel, Primitive};
use nalgebra::{Matrix3, Unit};
use std::f64::consts::{PI, TAU};

/// How far apart two ends may be and still meet (OCCT's `Precision::Confusion` scaled up a little
/// for coordinates far from the origin).
pub(crate) fn meet_tol(p: &V3) -> f64 {
    1e-7 * (1.0 + p.norm()).max(10.0)
}

/// A wire body of edges joined end to end.
pub(crate) fn wire_body(edges: &[EdgeGeom]) -> CqResult<Body> {
    let curves: Vec<(Curve, f64, f64)> = edges
        .iter()
        .map(|g| (g.curve.clone(), g.t0, g.t1))
        .collect();
    Ok(crate::native::kernel::wire_of_curves(&curves)?)
}

// -------------------------------------------------------------------- edges

/// `Edge.makeCircle(radius, pnt, dir, angle1, angle2, orientation)` (angles in degrees, from
/// OCCT's x direction of `dir`).
pub fn circle(radius: f64, pnt: V3, dir: V3, a1: f64, a2: f64, ccw: bool) -> CqResult<EdgeGeom> {
    if radius <= 0.0 {
        return value_err!("gp_Circ: negative or null radius");
    }
    if dir.norm() == 0.0 {
        return value_err!("gp_Dir() - input vector has zero norm");
    }
    let z = dir.normalize();
    let x = default_x(&z);
    let y = z.cross(&x);
    let c = Curve::Circle {
        c: P3::from(pnt),
        x,
        y,
        r: radius,
    };
    if a1 == a2 {
        return Ok(EdgeGeom {
            curve: c,
            t0: 0.0,
            t1: TAU,
        });
    }
    let (mut t0, mut t1) = (a1.to_radians(), a2.to_radians());
    if !ccw {
        std::mem::swap(&mut t0, &mut t1);
    }
    while t1 <= t0 {
        t1 += TAU;
    }
    while t1 - t0 > TAU + 1e-12 {
        t1 -= TAU;
    }
    let g = EdgeGeom { curve: c, t0, t1 };
    Ok(if ccw { g } else { g.reversed() })
}

/// `Edge.makeEllipse(x_radius, y_radius, pnt, dir, xdir, angle1, angle2, sense)`.
#[allow(clippy::too_many_arguments)]
pub fn ellipse(
    rx: f64,
    ry: f64,
    pnt: V3,
    dir: V3,
    xdir: V3,
    a1: f64,
    a2: f64,
    ccw: bool,
) -> CqResult<EdgeGeom> {
    if rx <= 0.0 || ry <= 0.0 {
        return value_err!("gp_Elips: negative or null radius");
    }
    let z = dir.normalize();
    let fr = Frame::from_xz(P3::from(pnt), xdir, z);
    let (x, y) = (fr.x, fr.y);
    let full = a1 == a2;
    let (mut t0, mut t1) = if full {
        (0.0, TAU)
    } else {
        (a1.to_radians(), a2.to_radians())
    };
    if !full {
        if !ccw {
            std::mem::swap(&mut t0, &mut t1);
        }
        while t1 <= t0 {
            t1 += TAU;
        }
    }
    // The kernel's ellipses have their major axis along x.
    let g = if ry > rx {
        EdgeGeom {
            curve: Curve::Ellipse {
                c: P3::from(pnt),
                x: y,
                y: -x,
                a: ry,
                b: rx,
            },
            t0: t0 - PI / 2.0,
            t1: t1 - PI / 2.0,
        }
    } else {
        EdgeGeom {
            curve: Curve::Ellipse {
                c: P3::from(pnt),
                x,
                y,
                a: rx,
                b: ry,
            },
            t0,
            t1,
        }
    };
    Ok(if full || ccw { g } else { g.reversed() })
}

/// `Edge.makeThreePointArc(v1, v2, v3)`: the arc from `a` through `m` to `b`.
pub fn three_point_arc(a: V3, m: V3, b: V3) -> CqResult<EdgeGeom> {
    let (u, v) = (m - a, b - a);
    let n = u.cross(&v);
    let nn = n.norm_squared();
    if nn <= 1e-24 * (u.norm_squared() * v.norm_squared()).max(1e-300) {
        return value_err!("gce_ColinearPoints: the three points of the arc are colinear");
    }
    // The circumcentre.
    let c = a + (v.norm_squared() * n.cross(&u) + u.norm_squared() * v.cross(&n)) / (2.0 * nn);
    let r = (a - c).norm();
    let z = n.normalize();
    let x = (a - c) / r;
    let y = z.cross(&x);
    let ang = |p: V3| {
        let d = p - c;
        let t = d.dot(&y).atan2(d.dot(&x));
        if t <= 0.0 { t + TAU } else { t }
    };
    let t1 = ang(b);
    Ok(EdgeGeom {
        curve: Curve::Circle {
            c: P3::from(c),
            x,
            y,
            r,
        },
        t0: 0.0,
        t1,
    })
}

/// `Edge.makeTangentArc(v1, tangent, v3)`: the arc from `a` leaving along `t`, ending at `b`.
pub fn tangent_arc(a: V3, t: V3, b: V3) -> CqResult<EdgeGeom> {
    let d = b - a;
    let n = t.cross(&d);
    if n.norm() <= 1e-12 * t.norm() * d.norm() {
        return EdgeGeom::line(a, b);
    }
    let z = n.normalize();
    let tt = t.normalize();
    let u = z.cross(&tt);
    let r = d.norm_squared() / (2.0 * d.dot(&u));
    let c = a + u * r;
    let x = (a - c) / r.abs();
    let y = z.cross(&x);
    let r = r.abs();
    let e = b - c;
    let mut t1 = e.dot(&y).atan2(e.dot(&x));
    if t1 <= 0.0 {
        t1 += TAU;
    }
    Ok(EdgeGeom {
        curve: Curve::Circle {
            c: P3::from(c),
            x,
            y,
            r,
        },
        t0: 0.0,
        t1,
    })
}

/// Chord-length parameters (OCCT's `GeomAPI_Interpolate`'s default), not normalized.
fn chord_params(pts: &[V3]) -> Vec<f64> {
    let mut t = vec![0.0];
    for w in pts.windows(2) {
        let d = (w[1] - w[0]).norm();
        t.push(t.last().unwrap() + d.max(1e-12));
    }
    t
}

/// `Edge.makeSpline(points, tangents, periodic, parameters, scale)`: the cubic through them.
pub fn spline(
    pts: &[V3],
    tangents: Option<&[Option<V3>]>,
    periodic: bool,
    params: Option<&[f64]>,
    scale: bool,
) -> CqResult<EdgeGeom> {
    if pts.len() < 2 {
        return value_err!("B-spline interpolation needs at least two points");
    }
    let p3: Vec<P3> = pts.iter().map(|v| P3::from(*v)).collect();
    if periodic {
        let mut t = match params {
            Some(p) => p.to_vec(),
            None => {
                let mut c = pts.to_vec();
                c.push(pts[0]);
                chord_params(&c)
            }
        };
        if t.len() == pts.len() {
            let last = t[t.len() - 1] + (pts[pts.len() - 1] - pts[0]).norm();
            t.push(last);
        }
        let c = NurbsCurve::interpolate_periodic(&p3, &t, 3.min(pts.len()))
            .ok_or_else(|| CqError::Value("B-spline interpolation failed".into()))?;
        let (a, b) = c.range();
        return Ok(EdgeGeom {
            curve: Curve::nurbs(c),
            t0: a,
            t1: b,
        });
    }
    let t: Vec<f64> = match params {
        Some(p) if p.len() == pts.len() => p.to_vec(),
        Some(p) => {
            return value_err!(
                "There must be one parameter for each interpolation point (plus one if periodic), or none specified. Parameter count: {}, point count: {}",
                p.len(),
                pts.len()
            );
        }
        None => chord_params(pts),
    };
    let span = t[t.len() - 1] - t[0];
    // With scale, a tangent is scaled to the parameter's speed: the chord's length per unit.
    let scaled = |v: V3| {
        if scale {
            let n = v.norm();
            if n > 0.0 {
                v / n * (span / span.max(1e-300)).max(1.0)
            } else {
                v
            }
        } else {
            v
        }
    };
    let (d0, d1) = match tangents {
        Some(tg) if tg.len() == 2 && pts.len() != 2 => (tg[0].map(scaled), tg[1].map(scaled)),
        Some(tg) if tg.len() == pts.len() => (tg[0].map(scaled), tg[tg.len() - 1].map(scaled)),
        Some(tg) if tg.len() == 2 => (tg[0].map(scaled), tg[1].map(scaled)),
        Some(tg) => {
            return value_err!(
                "There must be one tangent for each interpolation point, or just two end point tangents. Tangent count: {}, point count: {}",
                tg.len(),
                pts.len()
            );
        }
        None => (None, None),
    };
    let deg = 3
        .min(pts.len() - 1 + d0.is_some() as usize + d1.is_some() as usize)
        .max(1);
    let c = NurbsCurve::interpolate(&p3, &t, deg, d0, d1);
    let (a, b) = c.range();
    Ok(EdgeGeom {
        curve: Curve::nurbs(c),
        t0: a,
        t1: b,
    })
}

/// `Edge.makeSplineApprox(points, tol, …)`: a B-spline within `tol` of the points (the cubic
/// through them, its knots then thinned while it stays within `tol`).
pub fn spline_approx(pts: &[V3], tol: f64) -> CqResult<EdgeGeom> {
    if pts.len() < 2 {
        return value_err!("B-spline approximation failed");
    }
    // Drop repeated points (a parametric curve may stand still).
    let mut q: Vec<V3> = Vec::with_capacity(pts.len());
    for p in pts {
        if q.last()
            .is_none_or(|l: &V3| (p - l).norm() > 1e-12 * (1.0 + p.norm()))
        {
            q.push(*p);
        }
    }
    if q.len() < 2 {
        return value_err!("B-spline approximation failed");
    }
    let t = chord_params(&q);
    let p3: Vec<P3> = q.iter().map(|v| P3::from(*v)).collect();
    let c = NurbsCurve::interpolate(&p3, &t, 3.min(q.len() - 1), None, None);
    let sites: Vec<(f64, P3)> = t.iter().copied().zip(p3.iter().copied()).collect();
    let c = if q.len() > 8 {
        c.without_knots(tol.max(1e-9), &sites)
    } else {
        c
    };
    let (a, b) = c.range();
    Ok(EdgeGeom {
        curve: Curve::nurbs(c),
        t0: a,
        t1: b,
    })
}

/// `Edge.makeBezier(points)`.
pub fn bezier(pts: &[V3]) -> CqResult<EdgeGeom> {
    if pts.len() < 2 {
        return value_err!("a Bézier curve needs two points or more");
    }
    let p3: Vec<P3> = pts.iter().map(|v| P3::from(*v)).collect();
    let c = NurbsCurve::try_bezier(&p3, &vec![1.0; p3.len()])?;
    let (a, b) = c.range();
    Ok(EdgeGeom {
        curve: Curve::nurbs(c),
        t0: a,
        t1: b,
    })
}

/// `Wire.makeHelix(pitch, height, radius, center, dir, angle, lefthand)`: on a cylinder, or on
/// the cone of half-angle `angle` (degrees) when it isn't 360.
pub fn helix(
    pitch: f64,
    height: f64,
    radius: f64,
    center: V3,
    dir: V3,
    angle: f64,
    lefthand: bool,
) -> CqResult<Vec<EdgeGeom>> {
    if pitch == 0.0 || height == 0.0 {
        return value_err!("a helix needs a pitch and a height");
    }
    let z = dir.normalize();
    let turns = height / pitch;
    let (r1, rise) = if angle == 360.0 {
        (radius, height)
    } else {
        let a = angle.to_radians();
        (radius + height * a.sin(), height * a.cos())
    };
    let h = crate::Helix {
        axis: Axis {
            origin: P3::from(center),
            dir: Unit::new_normalize(z),
        },
        x: Unit::new_normalize(default_x(&z)),
        r0: radius,
        r1,
        height: rise,
        turns: if lefthand { -turns } else { turns },
        start_angle: 0.0,
    };
    let mut out = Vec::new();
    for pc in h.path() {
        if let crate::PathCurve::Bezier3 { poles, weights } = pc {
            let w = if weights.is_empty() {
                vec![1.0; poles.len()]
            } else {
                weights
            };
            let c = NurbsCurve::try_bezier(&poles, &w)?;
            let (a, b) = c.range();
            out.push(EdgeGeom {
                curve: Curve::nurbs(c),
                t0: a,
                t1: b,
            });
        }
    }
    // One edge, as OCCT makes it: the pieces joined.
    let mut joined: Option<NurbsCurve> = None;
    for g in &out {
        let Curve::Nurbs(n) = &g.curve else { continue };
        joined = Some(match joined {
            None => n.as_ref().clone(),
            Some(j) => j.joined(n),
        });
    }
    match joined {
        Some(j) => {
            let (a, b) = j.range();
            Ok(vec![EdgeGeom {
                curve: Curve::nurbs(j),
                t0: a,
                t1: b,
            }])
        }
        None => value_err!("the helix has no pieces"),
    }
}

// -------------------------------------------------------------------- wires

/// `Wire.makePolygon(points, close)`: segments between consecutive points (repeats skipped).
pub fn polygon(pts: &[V3], close: bool) -> CqResult<Vec<EdgeGeom>> {
    // (Points within OCCT's `Precision::Confusion` of the last are the same point.)
    const CONFUSION: f64 = 1e-7;
    let mut q: Vec<V3> = Vec::with_capacity(pts.len() + 1);
    for p in pts {
        if q.last().is_none_or(|l: &V3| (p - l).norm() > CONFUSION) {
            q.push(*p);
        }
    }
    if close && q.len() > 2 && (q[0] - q[q.len() - 1]).norm() > CONFUSION {
        q.push(q[0]);
    }
    if q.len() < 2 {
        return value_err!("BRepBuilderAPI_MakePolygon: not enough points");
    }
    q.windows(2).map(|w| EdgeGeom::line(w[0], w[1])).collect()
}

/// Chains edges end to end into wires (`Wire.combine`, OCCT's `ConnectEdgesToWires`): each
/// wire as long as its edges join, its edges turned to run on from one another.
pub fn chain_edges(mut edges: Vec<EdgeGeom>, tol: Option<f64>) -> Vec<Vec<EdgeGeom>> {
    let mut out = Vec::new();
    let near = |a: &V3, b: &V3| (a - b).norm() <= tol.unwrap_or_else(|| meet_tol(a).max(1e-6));
    while !edges.is_empty() {
        let mut w = vec![edges.remove(0)];
        loop {
            let (head, tail) = (w[0].start(), w[w.len() - 1].end());
            if near(&head, &tail) && w.len() > 1 {
                break;
            }
            let mut found = false;
            for i in 0..edges.len() {
                let e = &edges[i];
                if near(&e.start(), &tail) {
                    w.push(edges.remove(i));
                    found = true;
                } else if near(&e.end(), &tail) {
                    let r = edges.remove(i).reversed();
                    w.push(r);
                    found = true;
                } else if near(&e.end(), &head) {
                    w.insert(0, edges.remove(i));
                    found = true;
                } else if near(&e.start(), &head) {
                    let r = edges.remove(i).reversed();
                    w.insert(0, r);
                    found = true;
                }
                if found {
                    break;
                }
            }
            if !found {
                break;
            }
        }
        out.push(w);
    }
    out
}

/// `Wire.assembleEdges(edges)`: one wire of them all (in an order that joins them).
pub fn assemble(mut edges: Vec<EdgeGeom>) -> CqResult<Vec<EdgeGeom>> {
    // As OCCT's MakeWire given a list: the wire grows by every edge meeting its ends, the
    // edges in their order where they follow on, and on past a point it has come back to (a
    // profile round an annulus that goes out to its bore and back along one cut: the bore and
    // both runs of the cut are the wire's too). Edges it never meets are left out (OCCT warns).
    if edges.is_empty() {
        return Ok(edges);
    }
    let near = |a: &V3, b: &V3| (a - b).norm() <= meet_tol(a).max(1e-6);
    let mut w = vec![edges.remove(0)];
    loop {
        let (head, tail) = (w[0].start(), w[w.len() - 1].end());
        // The next edge in order first, then any at the tail, then any at the head.
        let pick = (0..edges.len())
            .find_map(|i| {
                let e = &edges[i];
                if near(&e.start(), &tail) {
                    Some((i, false, false))
                } else if near(&e.end(), &tail) {
                    Some((i, true, false))
                } else {
                    None
                }
            })
            .or_else(|| {
                (0..edges.len()).find_map(|i| {
                    let e = &edges[i];
                    if near(&e.end(), &head) {
                        Some((i, false, true))
                    } else if near(&e.start(), &head) {
                        Some((i, true, true))
                    } else {
                        None
                    }
                })
            });
        let Some((i, rev, front)) = pick else { break };
        let e = edges.remove(i);
        let e = if rev { e.reversed() } else { e };
        if front {
            w.insert(0, e);
        } else {
            w.push(e);
        }
    }
    Ok(w)
}

/// The plane a set of closed wires lies on: `hint` when they lie on it, else the plane fitted
/// to the first (its normal by Newell's method, turned towards `hint`'s). An error when they
/// aren't planar.
pub fn wires_plane(wires: &[Vec<EdgeGeom>], hint: Option<&Plane>) -> CqResult<Plane> {
    let pts: Vec<Vec<V3>> = wires
        .iter()
        .map(|w| w.iter().flat_map(|g| g.polyline(1e-3)).collect())
        .collect();
    let all: Vec<V3> = pts.iter().flatten().copied().collect();
    let size = super::geom::BoundBox::of_points(all.iter())
        .map(|b| b.diagonal())
        .unwrap_or(1.0)
        .max(1e-9);
    let tol = 1e-6 * size.max(1.0);
    let on = |o: V3, n: V3| {
        wires.iter().all(|w| {
            w.iter().all(|g| {
                let ok = g
                    .polyline(size * 1e-4)
                    .iter()
                    .all(|q| (q - o).dot(&n).abs() <= tol);
                // A line's or a conic's plane must also contain its direction or axis.
                ok && match &g.curve {
                    Curve::Circle { x, y, .. } | Curve::Ellipse { x, y, .. } => {
                        x.dot(&n).abs() <= 1e-9 && y.dot(&n).abs() <= 1e-9
                    }
                    _ => true,
                }
            })
        })
    };
    if let Some(h) = hint
        && on(h.origin, h.z_dir)
    {
        return Ok(*h);
    }
    // A lone circle or ellipse: its own plane, exactly.
    if let [g] = wires[0].as_slice()
        && let Curve::Circle { c, x, y, .. } | Curve::Ellipse { c, x, y, .. } = &g.curve
    {
        let mut n = x.cross(y).normalize();
        if let Some(h) = hint
            && n.dot(&h.z_dir) < 0.0
        {
            n = -n;
        }
        if on(c.coords, n) {
            let xd = hint
                .map(|h| h.x_dir - n * h.x_dir.dot(&n))
                .filter(|x| x.norm() > 1e-9)
                .unwrap_or(*x);
            return Ok(Plane::of_axes(c.coords, xd, n));
        }
    }
    // Newell's normal of the first wire's polygon (about its centroid: coordinates far from
    // the origin cancel in the sums otherwise).
    let first = &pts[0];
    let cen = first.iter().sum::<V3>() / first.len() as f64;
    let mut n = V3::zeros();
    for i in 0..first.len() {
        let (a, b) = (first[i] - cen, first[(i + 1) % first.len()] - cen);
        n += V3::new(
            (a.y - b.y) * (a.z + b.z),
            (a.z - b.z) * (a.x + b.x),
            (a.x - b.x) * (a.y + b.y),
        );
    }
    if n.norm() <= 1e-12 * size * size {
        // A single conic: its own plane.
        match &wires[0][0].curve {
            Curve::Circle { x, y, .. } | Curve::Ellipse { x, y, .. } => n = x.cross(y),
            _ => {
                // Any plane holding a straight or flat open run: through three points apart.
                let o = first[0];
                let far = first
                    .iter()
                    .max_by(|a, b| (*a - o).norm().total_cmp(&(*b - o).norm()))
                    .copied()
                    .unwrap_or(o);
                let d = far - o;
                n = first
                    .iter()
                    .map(|q| d.cross(&(q - o)))
                    .max_by(|a, b| a.norm().total_cmp(&b.norm()))
                    .unwrap_or_default();
                if n.norm() <= 1e-12 * size * size {
                    return value_err!("Cannot build face(s): wires not planar");
                }
            }
        }
    }
    let mut n = n.normalize();
    if let Some(h) = hint
        && n.dot(&h.z_dir) < 0.0
    {
        n = -n;
    }
    let o = all.iter().sum::<V3>() / all.len() as f64;
    if !on(o, n) {
        return value_err!("Cannot build face(s): wires not planar");
    }
    let x = hint
        .map(|h| h.x_dir - n * h.x_dir.dot(&n))
        .filter(|x| x.norm() > 1e-9)
        .unwrap_or_else(|| default_x(&n));
    Ok(Plane::of_axes(o, x, n))
}

/// The signed area a closed wire encloses in a plane (positive running counter-clockwise about
/// its normal).
pub fn signed_area(w: &[EdgeGeom], pl: &Plane) -> f64 {
    let pts: Vec<V3> = w
        .iter()
        .flat_map(|g| {
            let mut p = g.polyline(1e-4 * (1.0 + g.length()));
            p.pop();
            p
        })
        .collect();
    let (x, y, _) = pl.frame();
    let q: Vec<(f64, f64)> = pts
        .iter()
        .map(|p| ((p - pl.origin).dot(&x), (p - pl.origin).dot(&y)))
        .collect();
    let mut a = 0.0;
    for i in 0..q.len() {
        let (p0, p1) = (q[i], q[(i + 1) % q.len()]);
        a += p0.0 * p1.1 - p1.0 * p0.1;
    }
    0.5 * a
}

/// Whether point `q` (in the plane) is inside the closed wire `w`.
pub fn inside(q: V3, w: &[EdgeGeom], pl: &Plane) -> bool {
    let pts: Vec<V3> = w
        .iter()
        .flat_map(|g| {
            let mut p = g.polyline(1e-4 * (1.0 + g.length()));
            p.pop();
            p
        })
        .collect();
    let (x, y, _) = pl.frame();
    let poly: Vec<(f64, f64)> = pts
        .iter()
        .map(|p| ((p - pl.origin).dot(&x), (p - pl.origin).dot(&y)))
        .collect();
    let (qx, qy) = ((q - pl.origin).dot(&x), (q - pl.origin).dot(&y));
    let mut c = false;
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        if (a.1 > qy) != (b.1 > qy) {
            let xc = a.0 + (qy - a.1) * (b.0 - a.0) / (b.1 - a.1);
            if xc > qx {
                c = !c;
            }
        }
    }
    c
}

/// A closed wire turned to run counter-clockwise (`ccw`) or clockwise about the plane's normal.
pub fn oriented(w: &[EdgeGeom], pl: &Plane, ccw: bool) -> Vec<EdgeGeom> {
    if (signed_area(w, pl) >= 0.0) == ccw {
        w.to_vec()
    } else {
        w.iter().rev().map(|g| g.reversed()).collect()
    }
}

/// A planar face body bounded by an outer wire and holes on a plane (its normal the plane's).
pub fn planar_face(pl: &Plane, outer: &[EdgeGeom], holes: &[Vec<EdgeGeom>]) -> CqResult<Body> {
    let (x, _, z) = pl.frame();
    let fr = Frame::from_xz(P3::from(pl.origin), x, z);
    let surf = Surface::Plane { f: fr };
    let mut b = Body::default();
    let mut loops = Vec::new();
    let all: Vec<Vec<EdgeGeom>> = std::iter::once(oriented(outer, pl, true))
        .chain(holes.iter().map(|h| oriented(h, pl, false)))
        .collect();
    for w in &all {
        if w.is_empty() {
            continue;
        }
        let first = w[0].start();
        let last = w[w.len() - 1].end();
        if (first - last).norm() > 1e-4 * (1.0 + first.norm()) {
            return value_err!("Cannot build face(s): wire is not closed");
        }
        let v0 = b.add_vertex(P3::from(first));
        let mut at = v0;
        let mut lp = Vec::new();
        for (i, g) in w.iter().enumerate() {
            let next = if i + 1 == w.len() {
                v0
            } else {
                b.add_vertex(P3::from(g.end()))
            };
            let e = b.add_edge(g.curve.clone(), g.t0, g.t1, at, next);
            let pc = crate::native::pcurve::pcurve(&surf, &g.curve, g.t0, g.t1, None);
            lp.push(Coedge {
                edge: e,
                fwd: true,
                pc,
            });
            at = next;
        }
        loops.push(lp);
    }
    b.faces.push(Face {
        surf,
        sense: true,
        loops,
    });
    b.shells.push(Shell {
        faces: vec![0],
        closed: false,
    });
    Ok(b)
}

/// A planar face as wires: its plane, its outer wire and its holes.
pub type PlanarFace = (Plane, Vec<EdgeGeom>, Vec<Vec<EdgeGeom>>);

/// Faces from closed wires (`wiresToFaces`, OCCT's face maker with its orientation fix): a wire
/// inside another (an odd number of others) is a hole in the innermost one holding it; the rest
/// are faces of their own.
pub fn wires_to_faces(wires: &[Vec<EdgeGeom>], hint: Option<&Plane>) -> CqResult<Vec<PlanarFace>> {
    if wires.is_empty() {
        return value_err!("No pending wires present");
    }
    for w in wires {
        let (a, b) = (w[0].start(), w[w.len() - 1].end());
        if (a - b).norm() > 1e-4 * (1.0 + a.norm()) {
            return value_err!("Cannot build face(s): outer wire is not closed");
        }
    }
    let pl = wires_plane(wires, hint)?;
    let n = wires.len();
    if n == 1 {
        return Ok(vec![(pl, wires[0].clone(), vec![])]);
    }
    let areas: Vec<f64> = wires.iter().map(|w| signed_area(w, &pl).abs()).collect();
    // Wire j lies inside wire i when all of it does: nested wires don't cross, and overlapping
    // ones (pushPoints' shapes too close together) are faces of their own, united later.
    let probe: Vec<Vec<V3>> = wires
        .iter()
        .map(|w| {
            w.iter()
                .map(|g| g.point(0.5 * (g.t0 + g.t1)))
                .chain(w.iter().map(|g| g.start()))
                .collect()
        })
        .collect();
    let mut parent: Vec<Option<usize>> = vec![None; n];
    let mut depth = vec![0usize; n];
    for j in 0..n {
        let mut best: Option<usize> = None;
        for i in 0..n {
            if i == j || areas[i] <= areas[j] {
                continue;
            }
            if probe[j].iter().all(|q| inside(*q, &wires[i], &pl)) {
                depth[j] += 1;
                if best.is_none_or(|b| areas[i] < areas[b]) {
                    best = Some(i);
                }
            }
        }
        parent[j] = best;
    }
    let mut out = Vec::new();
    for i in 0..n {
        if depth[i] % 2 == 0 {
            let holes: Vec<Vec<EdgeGeom>> = (0..n)
                .filter(|&j| parent[j] == Some(i) && depth[j] % 2 == 1)
                .map(|j| wires[j].clone())
                .collect();
            out.push((pl, wires[i].clone(), holes));
        }
    }
    Ok(out)
}

/// `Face.makeFromWires(outer, inners)`, as faces (OCCT may make several).
pub fn faces_from_wires(wires: &[Vec<EdgeGeom>], hint: Option<&Plane>) -> CqResult<Vec<Shape>> {
    wires_to_faces(wires, hint)?
        .into_iter()
        .map(|(pl, o, h)| Ok(Shape::of_native(planar_face(&pl, &o, &h)?)))
        .collect()
}

// -------------------------------------------------------------------- solids

/// The frame OCCT's `gp_Ax2(pnt, dir)` makes.
fn ax2(_pnt: V3, dir: V3) -> CqResult<(V3, V3, V3)> {
    if dir.norm() == 0.0 {
        return value_err!("gp_Dir() - input vector has zero norm");
    }
    let z = dir.normalize();
    let x = default_x(&z);
    Ok((x, z.cross(&x), z))
}

fn placed(s: Shape, pnt: V3, x: V3, y: V3, z: V3) -> CqResult<Shape> {
    let m = Affine {
        m: Matrix3::from_columns(&[x, y, z]),
        t: pnt,
    };
    if m == Affine::identity() {
        Ok(s)
    } else {
        s.transformed(&m)
    }
}

fn primitive(p: Primitive) -> CqResult<Shape> {
    Ok(Shape::of_body(op1(|k| k.primitive(&p))?))
}

/// `Solid.makeBox(length, width, height, pnt, dir)`.
pub fn make_box(l: f64, w: f64, h: f64, pnt: V3, dir: V3) -> CqResult<Shape> {
    if l <= 0.0 || w <= 0.0 || h <= 0.0 {
        return value_err!("BRepPrimAPI_MakeBox: a dimension is not positive");
    }
    let (x, y, z) = ax2(pnt, dir)?;
    let b = primitive(Primitive::Box {
        min: P3::origin(),
        max: P3::new(l, w, h),
    })?;
    placed(b, pnt, x, y, z)
}

/// The solid of a profile region turned about the frame's z axis by `deg`: the region given in
/// (radius, height) coordinates of the frame's xz plane.
fn turned(pts_rz: Vec<crate::Curve2>, pnt: V3, x: V3, z: V3, deg: f64) -> CqResult<Shape> {
    let plane = crate::Plane {
        origin: P3::from(pnt),
        x_dir: Unit::new_normalize(x),
        normal: Unit::new_normalize(x.cross(&z)),
    };
    // (The plane's y axis, normal × x, is z.)
    let profile = crate::Profile::new(
        plane,
        vec![crate::Region {
            outer: crate::Loop { curves: pts_rz },
            holes: vec![],
            source: None,
        }],
    );
    let axis = Axis {
        origin: P3::from(pnt),
        dir: Unit::new_normalize(z),
    };
    let full = (deg - 360.0).abs() < 1e-12 || deg >= 360.0;
    let mut spec =
        crate::RevolveSpec::solid(axis, if full { None } else { Some(deg.to_radians()) });
    spec.full = full;
    Ok(Shape::of_body(op1(|k| k.revolve_with(&profile, &spec))?))
}

fn p2(x: f64, y: f64) -> nalgebra::Point2<f64> {
    nalgebra::Point2::new(x, y)
}

fn line2(a: (f64, f64), b: (f64, f64)) -> crate::Curve2 {
    crate::Curve2::Line {
        a: p2(a.0, a.1),
        b: p2(b.0, b.1),
        source: None,
    }
}

/// `Solid.makeCylinder(radius, height, pnt, dir, angleDegrees)`.
pub fn cylinder(r: f64, h: f64, pnt: V3, dir: V3, deg: f64) -> CqResult<Shape> {
    if r <= 0.0 || h == 0.0 {
        return value_err!("BRepPrimAPI_MakeCylinder: radius or height is not positive");
    }
    let (x, _, z) = ax2(pnt, dir)?;
    let (h, z, pnt) = if h < 0.0 {
        (-h, z, pnt + z * h)
    } else {
        (h, z, pnt)
    };
    if deg >= 360.0 {
        return primitive(Primitive::Cylinder {
            base: P3::from(pnt),
            axis: Unit::new_normalize(z),
            radius: r,
            height: h,
        });
    }
    turned(
        vec![
            line2((0.0, 0.0), (r, 0.0)),
            line2((r, 0.0), (r, h)),
            line2((r, h), (0.0, h)),
            line2((0.0, h), (0.0, 0.0)),
        ],
        pnt,
        x,
        z,
        deg,
    )
}

/// `Solid.makeCone(radius1, radius2, height, pnt, dir, angleDegrees)`.
pub fn cone(r1: f64, r2: f64, h: f64, pnt: V3, dir: V3, deg: f64) -> CqResult<Shape> {
    // (Radii within rounding of zero are an apex.)
    let snap = |r: f64| {
        if r.abs() <= 1e-9 * r1.abs().max(r2.abs()).max(1.0) {
            0.0
        } else {
            r
        }
    };
    let (r1, r2) = (snap(r1), snap(r2));
    if r1 < 0.0 || r2 < 0.0 || (r1 == 0.0 && r2 == 0.0) || h <= 0.0 {
        return value_err!("BRepPrimAPI_MakeCone: bad radii or height");
    }
    let (x, _, z) = ax2(pnt, dir)?;
    if (r1 - r2).abs() < 1e-12 {
        return cylinder(r1, h, pnt, dir, deg);
    }
    if deg >= 360.0 {
        return primitive(Primitive::Cone {
            base: P3::from(pnt),
            axis: Unit::new_normalize(z),
            base_radius: r1,
            top_radius: r2,
            height: h,
        });
    }
    let mut c = vec![
        line2((0.0, 0.0), (r1, 0.0)),
        line2((r1, 0.0), (r2, h)),
        line2((r2, h), (0.0, h)),
        line2((0.0, h), (0.0, 0.0)),
    ];
    c.retain(|l| match l {
        crate::Curve2::Line { a, b, .. } => (a - b).norm() > 0.0,
        _ => true,
    });
    turned(c, pnt, x, z, deg)
}

/// `Solid.makeSphere(radius, pnt, dir, angle1, angle2, angle3)`: latitudes `a1`..`a2`, turned
/// `a3` degrees.
pub fn sphere(r: f64, pnt: V3, dir: V3, a1: f64, a2: f64, a3: f64) -> CqResult<Shape> {
    if r <= 0.0 {
        return value_err!("BRepPrimAPI_MakeSphere: radius is not positive");
    }
    let (x, _, z) = ax2(pnt, dir)?;
    let (a1, a2) = (a1.max(-90.0), a2.min(90.0));
    if a1 >= a2 {
        return value_err!("BRepPrimAPI_MakeSphere: bad angles");
    }
    if a1 <= -90.0 && a2 >= 90.0 && a3 >= 360.0 {
        return primitive(Primitive::Sphere {
            center: P3::from(pnt),
            radius: r,
            axis: Unit::new_normalize(z),
            seam: Unit::new_normalize(x),
        });
    }
    let (t1, t2) = (a1.to_radians(), a2.to_radians());
    let mut c = Vec::new();
    // The arc from latitude t1 up to t2 (centre at the origin, x = radius, y = height).
    c.push(crate::Curve2::Arc {
        center: p2(0.0, 0.0),
        radius: r,
        start_angle: t1,
        sweep: t2 - t1,
        source: None,
    });
    let top = (r * t2.cos(), r * t2.sin());
    let bot = (r * t1.cos(), r * t1.sin());
    if top.0 > 1e-12 * r {
        c.push(line2(top, (0.0, top.1)));
    }
    c.push(line2((0.0, top.1), (0.0, bot.1)));
    if bot.0 > 1e-12 * r {
        c.push(line2((0.0, bot.1), bot));
    }
    turned(c, pnt, x, z, a3)
}

/// `Solid.makeTorus(radius1, radius2, pnt, dir, angle1, angle2)`.
pub fn torus(r1: f64, r2: f64, pnt: V3, dir: V3) -> CqResult<Shape> {
    if r2 <= 0.0 || r1 <= 0.0 {
        return value_err!("BRepPrimAPI_MakeTorus: bad radii");
    }
    let (_, _, z) = ax2(pnt, dir)?;
    primitive(Primitive::Torus {
        center: P3::from(pnt),
        axis: Unit::new_normalize(z),
        major: r1,
        minor: r2,
        spindle: Default::default(),
    })
}

/// `Solid.makeWedge(dx, dy, dz, xmin, zmin, xmax, zmax, pnt, dir)`: the box `dx × dy × dz` whose
/// face at y = dy shrinks to `[xmin, xmax] × [zmin, zmax]`.
#[allow(clippy::too_many_arguments)]
pub fn wedge(
    dx: f64,
    dy: f64,
    dz: f64,
    xmin: f64,
    zmin: f64,
    xmax: f64,
    zmax: f64,
    pnt: V3,
    dir: V3,
) -> CqResult<Shape> {
    if dx <= 0.0 || dy <= 0.0 || dz <= 0.0 || xmax < xmin || zmax < zmin {
        return value_err!("BRepPrimAPI_MakeWedge: bad dimensions");
    }
    let (x, y, z) = ax2(pnt, dir)?;
    let v = |a: f64, b: f64, c: f64| P3::new(a, b, c);
    let bot = [
        v(0.0, 0.0, 0.0),
        v(dx, 0.0, 0.0),
        v(dx, 0.0, dz),
        v(0.0, 0.0, dz),
    ];
    let top = [
        v(xmin, dy, zmin),
        v(xmax, dy, zmin),
        v(xmax, dy, zmax),
        v(xmin, dy, zmax),
    ];
    let mut tris = Vec::new();
    let mut quad = |a: P3, b: P3, c: P3, d: P3| {
        for t in [[a, b, c], [a, c, d]] {
            let n = (t[1] - t[0]).cross(&(t[2] - t[0]));
            if n.norm() > 1e-12 {
                tris.push(t);
            }
        }
    };
    quad(bot[0], bot[1], bot[2], bot[3]);
    quad(top[3], top[2], top[1], top[0]);
    for i in 0..4 {
        let j = (i + 1) % 4;
        quad(bot[j], bot[i], top[i], top[j]);
    }
    let id = super::session(|k| k.mesh_solid(&tris, 1e-9))?;
    let s = Shape::of_body(super::adopt(id));
    let s = super::ops::clean(&s)?;
    placed(s, pnt, x, y, z)
}

/// `Face.makeFromWires(outer, holes)` on a plane: one planar face.
pub fn face(pl: &Plane, outer: &[EdgeGeom], holes: &[Vec<EdgeGeom>]) -> CqResult<Shape> {
    Ok(Shape::of_native(planar_face(pl, outer, holes)?))
}

// -------------------------------------------------------------------- text

/// The closed wires of a string's glyph outlines (`Compound.makeText`'s flat text) in plane
/// coordinates (z = 0): `font` the font file's bytes, `size` the em height (as OCCT's
/// FreeType size), aligned about the origin as `halign` ("left", "center", "right") and
/// `valign` ("bottom", "center", "top") say. TrueType's quadratic pieces and CFF's cubics are
/// exact Bézier edges.
pub fn text_wires(
    font: &[u8],
    text: &str,
    size: f64,
    halign: &str,
    valign: &str,
) -> CqResult<Vec<Vec<EdgeGeom>>> {
    let face = ttf_parser::Face::parse(font, 0)
        .map_err(|e| CqError::Value(format!("cannot read the font: {e}")))?;
    let s = size / face.units_per_em() as f64;
    struct Pen {
        wires: Vec<Vec<EdgeGeom>>,
        cur: Vec<EdgeGeom>,
        at: V3,
        start: V3,
        x: f64,
        s: f64,
    }
    impl Pen {
        fn p(&self, x: f32, y: f32) -> V3 {
            V3::new(self.x + x as f64 * self.s, y as f64 * self.s, 0.0)
        }
        fn push(&mut self, g: CqResult<EdgeGeom>, to: V3) {
            if let Ok(g) = g {
                self.cur.push(g);
            }
            self.at = to;
        }
        fn close_wire(&mut self) {
            if (self.at - self.start).norm() > 1e-9 {
                let (a, b) = (self.at, self.start);
                self.push(EdgeGeom::line(a, b), b);
            }
            if !self.cur.is_empty() {
                self.wires.push(std::mem::take(&mut self.cur));
            }
        }
    }
    impl ttf_parser::OutlineBuilder for Pen {
        fn move_to(&mut self, x: f32, y: f32) {
            if !self.cur.is_empty() {
                self.close_wire();
            }
            self.at = self.p(x, y);
            self.start = self.at;
        }
        fn line_to(&mut self, x: f32, y: f32) {
            let q = self.p(x, y);
            let a = self.at;
            self.push(EdgeGeom::line(a, q), q);
        }
        fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
            let (c, q) = (self.p(x1, y1), self.p(x, y));
            let a = self.at;
            self.push(bezier(&[a, c, q]), q);
        }
        fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
            let (c1, c2, q) = (self.p(x1, y1), self.p(x2, y2), self.p(x, y));
            let a = self.at;
            self.push(bezier(&[a, c1, c2, q]), q);
        }
        fn close(&mut self) {
            self.close_wire();
        }
    }
    let mut pen = Pen {
        wires: vec![],
        cur: vec![],
        at: V3::zeros(),
        start: V3::zeros(),
        x: 0.0,
        s,
    };
    let space = face
        .glyph_index(' ')
        .and_then(|g| face.glyph_hor_advance(g))
        .unwrap_or(face.units_per_em() / 4) as f64
        * s;
    for ch in text.chars() {
        match face.glyph_index(ch) {
            Some(g) => {
                face.outline_glyph(g, &mut pen);
                pen.close_wire();
                pen.x += face.glyph_hor_advance(g).map_or(space, |a| a as f64 * s);
            }
            None => pen.x += space,
        }
    }
    let width = pen.x;
    let (asc, desc) = (face.ascender() as f64 * s, face.descender() as f64 * s);
    let dx = match halign {
        "left" => 0.0,
        "right" => -width,
        _ => -width / 2.0,
    };
    let dy = match valign {
        "bottom" => -desc,
        "top" => -asc,
        _ => -(asc + desc) / 2.0,
    };
    let m = Affine::translation(V3::new(dx, dy, 0.0));
    Ok(pen
        .wires
        .into_iter()
        .map(|w| w.into_iter().map(|g| g.transformed(&m)).collect())
        .collect())
}

/// `Compound.makeText(text, size, height, …, position)`: the glyphs' faces on the plane,
/// extruded `height` along its normal (flat when 0).
pub fn text(
    font: &[u8],
    txt: &str,
    size: f64,
    height: f64,
    halign: &str,
    valign: &str,
    pl: &Plane,
) -> CqResult<Shape> {
    let wires = text_wires(font, txt, size, halign, valign)?;
    if wires.is_empty() {
        return Ok(Shape::compound(vec![]));
    }
    let to_world = pl.to_world_map();
    let wires: Vec<Vec<EdgeGeom>> = wires
        .into_iter()
        .map(|w| w.into_iter().map(|g| g.transformed(&to_world)).collect())
        .collect();
    let faces = faces_from_wires(&wires, Some(pl))?;
    if height == 0.0 {
        return Ok(Shape::compound(faces));
    }
    super::ops::extrude(&faces, pl.z_dir * height, 0.0)
}

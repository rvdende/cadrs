//! A body under any affine map: stretched along axes (a ball into an ellipsoid), sheared or
//! mirrored. A similarity (a rigid motion, scaled uniformly or mirrored) keeps every surface's
//! kind ([`Body::transformed`]); any other map keeps planes, lines and conics (a circle becomes an
//! ellipse) and turns the curved surfaces into their exact NURBS forms, moved. The body is then
//! built again from its new geometry ([`XBuilder`]), which makes the p-curves, seams and poles.

use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::topo::Body;
use super::xbuild::{XBuilder, XFace};
use crate::{KernelError, Result};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

/// Whether `m` keeps angles: its linear part a scaled rotation or reflection.
fn similarity(m: &Affine) -> bool {
    let k = m.scale();
    if k <= 0.0 {
        return false;
    }
    let q = m.m.transpose() * m.m / (k * k);
    (q - nalgebra::Matrix3::identity()).abs().max() < 1e-12
}

/// The ellipse c + A cos t + B sin t (A and B conjugate semi-diameters, square or not) with its
/// axes square: (c, x, y, a, b) and the phase φ that turns the old parameter t into the new
/// one, t + φ.
fn ellipse(c: P3, a: V3, b: V3) -> (Curve, f64) {
    // A cos(s − φ) + B sin(s − φ) = P cos s + Q sin s, P = A cos φ − B sin φ, Q = A sin φ +
    // B cos φ, with P · Q = 0.
    let phi = 0.5 * (-2.0 * a.dot(&b)).atan2(a.norm_squared() - b.norm_squared());
    let (s, co) = phi.sin_cos();
    let p = a * co - b * s;
    let q = a * s + b * co;
    let (ra, rb) = (p.norm(), q.norm());
    (
        Curve::Ellipse {
            c,
            x: p / ra,
            y: q / rb,
            a: ra,
            b: rb,
        },
        phi,
    )
}

/// The curve under `m` (a line a line, a conic an ellipse, a B-spline its control points moved).
fn curve(c: &Curve, m: &Affine) -> Curve {
    match c {
        Curve::Line { .. } | Curve::Nurbs(_) => c.transformed(m),
        Curve::Circle { c, x, y, r } => ellipse(m.point(c), m.vector(x) * *r, m.vector(y) * *r).0,
        Curve::Ellipse { c, x, y, a, b } => {
            ellipse(m.point(c), m.vector(x) * *a, m.vector(y) * *b).0
        }
    }
}

/// The parameter box a face covers on its surface (from its p-curves), a closed direction's
/// whole period where it wraps round, a little margin elsewhere (inside a sphere's poles).
fn face_box(b: &Body, fi: usize) -> ((f64, f64), (f64, f64)) {
    let f = &b.faces[fi];
    let (mut u0, mut u1, mut v0, mut v1) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for c in f.loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for i in 0..=32 {
            let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 32.0);
            u0 = u0.min(q.x);
            u1 = u1.max(q.x);
            v0 = v0.min(q.y);
            v1 = v1.max(q.y);
        }
    }
    let ((pu0, pu1), (pv0, pv1)) = f.surf.param_box();
    let periodic_u = matches!(
        f.surf,
        Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Revolution { .. }
    );
    let periodic_v = matches!(f.surf, Surface::Torus { .. });
    let fit = |lo: f64, hi: f64, periodic: bool, (a, z): (f64, f64)| {
        if periodic && hi - lo >= TAU - 1e-9 {
            return (lo, lo + TAU);
        }
        let pad = 1e-3 * (hi - lo).max(1e-6);
        let (lo, hi) = (lo - pad, hi + pad);
        if periodic {
            // Within one period.
            let w = (hi - lo).min(TAU);
            (lo, lo + w)
        } else {
            (lo.max(a), hi.min(z))
        }
    };
    let u = fit(u0, u1, periodic_u, (pu0, pu1));
    let v = match f.surf {
        // (Exactly to a pole the face reaches: past it the meridian turns back.)
        Surface::Sphere { .. } => {
            let at = |x: f64| {
                if (x + PI / 2.0).abs() < 1e-9 {
                    -PI / 2.0
                } else if (x - PI / 2.0).abs() < 1e-9 {
                    PI / 2.0
                } else {
                    x
                }
            };
            let (a, z) = fit(at(v0), at(v1), false, (pv0, pv1));
            (a.max(-PI / 2.0), z.min(PI / 2.0))
        }
        _ => fit(v0, v1, periodic_v, (pv0, pv1)),
    };
    (u, v)
}

/// The surface under `m`: a plane a plane (its frame square again), any other its NURBS form over
/// the face's box, moved.
fn surface(b: &Body, fi: usize, m: &Affine) -> Surface {
    let s = &b.faces[fi].surf;
    match s {
        Surface::Plane { f } => {
            let (x, y) = (m.vector(&f.x), m.vector(&f.y));
            let z = x.cross(&y).normalize();
            let x = x.normalize();
            Surface::Plane {
                f: Frame {
                    o: m.point(&f.o),
                    x,
                    y: z.cross(&x),
                    z,
                },
            }
        }
        Surface::Nurbs(n) => Surface::Nurbs(Arc::new(n.transformed(m))),
        _ => {
            let (u, v) = face_box(b, fi);
            Surface::Nurbs(Arc::new(s.to_nurbs(u, v).transformed(m)))
        }
    }
}

/// `b` under the affine map `m`.
///
/// # Errors
/// [`KernelError::InvalidParameter`] for a singular map, or [`KernelError::InvalidResult`]
/// when the body built again isn't valid.
pub fn affine_body(b: &Body, m: &Affine) -> Result<Body> {
    let det = m.det();
    if !(det.is_finite() && det.abs() > 1e-15) {
        return Err(KernelError::InvalidParameter(
            "The transform must not flatten the part".into(),
        ));
    }
    if similarity(m) {
        return Ok(b.transformed(m));
    }
    let mirror = det < 0.0;
    let mut xb = XBuilder::new();
    let verts: Vec<usize> = b.verts.iter().map(|v| xb.vertex(m.point(&v.p))).collect();
    let mut edges: Vec<Option<usize>> = vec![None; b.edges.len()];
    let stretch = m.m.norm();
    for (i, e) in b.edges.iter().enumerate() {
        if e.degenerate {
            continue;
        }
        let Some(id) = xb.edge(curve(&e.curve, m), verts[e.v[0]], verts[e.v[1]]) else {
            continue;
        };
        xb.body.edges[id].tol = e.tol * stretch.max(1.0);
        edges[i] = Some(id);
    }
    let mut faces: Vec<usize> = Vec::with_capacity(b.faces.len());
    for (fi, f) in b.faces.iter().enumerate() {
        let loops = f
            .loops
            .iter()
            .map(|l| {
                let mut l: Vec<(usize, bool)> = l
                    .iter()
                    .filter_map(|c| edges[c.edge].map(|e| (e, c.fwd != mirror)))
                    .collect();
                if mirror {
                    l.reverse();
                }
                l
            })
            .filter(|l| !l.is_empty())
            .collect();
        faces.push(xb.face(XFace {
            surf: surface(b, fi, m),
            sense: f.sense != mirror,
            loops,
        }));
    }
    for sh in &b.shells {
        xb.shell(sh.faces.iter().map(|&f| faces[f]).collect(), sh.closed);
    }
    for so in &b.solids {
        xb.solid(so.shells.clone());
    }
    let (mut body, warnings) = xb.finish();
    let size = body.bbox().diagonal();
    body.fit_tolerances_searched(1e-5 * (1.0 + size));
    let issues = body.check();
    if !issues.is_empty() || body.faces.len() != b.faces.len() {
        return Err(KernelError::InvalidResult {
            op: "transform",
            why: format!(
                "the transformed part isn't valid: {}",
                issues
                    .first()
                    .cloned()
                    .or_else(|| warnings.first().cloned())
                    .unwrap_or_else(|| "faces were lost".into())
            ),
        });
    }
    Ok(body)
}

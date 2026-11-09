//! Insets of prisms with curved sides: a body between two parallel planar caps whose other faces
//! are all ruled square to them (planes, cylinders and extrusions along the caps' normal), its
//! sides moved in by one distance, is its outline's 2D inward offset ([`super::wire`]: exact,
//! trimmed where the outline curves more tightly than the distance, and rounded about its
//! reflex corners) extruded between the caps as they moved. That is the body's rounded inset,
//! the points at least the distance inside (the distance to a prism's outside is the smaller of
//! the distances to its outline and to its caps), and also its sharp one when the outline has
//! no reflex corner.

use crate::native::curve::Curve;
use crate::native::kernel::NativeKernel;
use crate::native::math::*;
use crate::native::nurbs::dehom;
use crate::native::surface::Surface;
use crate::native::tol;
use crate::native::topo::{Body, co_range};
use crate::*;

use super::wire::{Piece2, offset_wire};

/// The inset of a prism with a curved side (see the module): None when `b` isn't one, its sides
/// don't all move in by one distance, or (`rounded` false: the sharp inset) its outline has a
/// reflex corner, which the sharp inset mitres; with `folding_only`, also None unless the
/// outline curves more tightly than the distance somewhere (the case the faces' own offsets
/// can't make: there they fold over themselves).
pub fn curved_prism_inset(
    b: &Body,
    dist: &[f64],
    rounded: bool,
    folding_only: bool,
) -> Option<Result<Body>> {
    if b.solids.len() != 1 || b.faces.len() < 3 {
        return None;
    }
    let plane_n = |f: usize| -> Option<V3> {
        let face = &b.faces[f];
        let Surface::Plane { f: fr } = &face.surf else {
            return None;
        };
        Some(fr.x.cross(&fr.y).normalize() * if face.sense { 1.0 } else { -1.0 })
    };
    let normals: Vec<Option<V3>> = (0..b.faces.len()).map(plane_n).collect();
    // Ruled square to the caps: a plane along n, a cylinder or an extrusion about it.
    let ruled = |f: usize, n: &V3| match &b.faces[f].surf {
        Surface::Plane { .. } => normals[f].is_some_and(|m| m.dot(n).abs() < tol::ANGULAR_FINE),
        Surface::Cylinder { f: fr, .. } => fr.z.cross(n).norm() < tol::ANGULAR_FINE,
        Surface::Extrusion { d, .. } => d.normalize().cross(n).norm() < tol::ANGULAR_FINE,
        _ => false,
    };
    let (bot, top) = (0..b.faces.len())
        .flat_map(|i| (0..b.faces.len()).map(move |j| (i, j)))
        .find(|&(i, j)| {
            let (Some(ni), Some(nj)) = (normals[i], normals[j]) else {
                return false;
            };
            i != j
                && ni.dot(&nj) < -1.0 + tol::ANGULAR_FINE
                && (0..b.faces.len()).all(|k| k == i || k == j || ruled(k, &-ni))
        })?;
    if b.faces[bot].loops.len() != 1 || b.faces[top].loops.len() != 1 {
        return None;
    }
    let sides: Vec<usize> = (0..b.faces.len())
        .filter(|&f| f != bot && f != top)
        .collect();
    if sides.iter().all(|&f| normals[f].is_some()) {
        // (Plane sides only: `prism_offset`'s.)
        return None;
    }
    let s = dist[sides[0]];
    if s >= 0.0
        || sides
            .iter()
            .any(|&f| (dist[f] - s).abs() > tol::ROUNDOFF_FINE)
    {
        return None;
    }
    let t = -s;
    let n = -normals[bot]?;
    let o = b.verts[b.edges[b.faces[bot].loops[0][0].edge].v[0]].p;
    let x = any_perpendicular(&n).normalize();
    let y = n.cross(&x);
    let h = (b.verts[b.edges[b.faces[top].loops[0][0].edge].v[0]].p - o).dot(&n);
    // Every vertex on one cap or the other: the sides run the whole height.
    let size = b.bbox().diagonal().max(1.0);
    if h <= tol::ROUNDOFF
        || b.verts.iter().any(|v| {
            let z = (v.p - o).dot(&n);
            z.abs() > tol::ROUNDOFF * size && (z - h).abs() > tol::ROUNDOFF * size
        })
    {
        return None;
    }
    // The outline: the bottom loop in the frame (x, y, n) at o, each edge run as its coedge.
    let to_l = Affine {
        m: nalgebra::Matrix3::from_rows(&[x.transpose(), y.transpose(), n.transpose()]),
        t: V3::zeros(),
    };
    let to_l = Affine {
        t: -(to_l.m * o.coords),
        ..to_l
    };
    let mut outline: Vec<(Curve, f64, f64)> = Vec::new();
    for c in &b.faces[bot].loops[0] {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        let (t0, t1) = co_range(e, c.fwd);
        outline.push((e.curve.transformed(&to_l), t0, t1));
    }
    // Its turning: counter-clockwise about n (the bottom face's own loop runs the other way).
    let mut area = 0.0;
    for (c, t0, t1) in &outline {
        for k in 0..32 {
            let (p, q) = (
                c.point(t0 + (t1 - t0) * k as f64 / 32.0),
                c.point(t0 + (t1 - t0) * (k + 1) as f64 / 32.0),
            );
            area += p.x * q.y - q.x * p.y;
        }
    }
    let inward = if area > 0.0 { 1.0 } else { -1.0 };
    // Curvature towards the inside along an edge, and the edge's ends' tangents.
    let bend = |c: &Curve, u: f64, dir: f64| {
        let (_, d1, d2) = c.d2(u);
        let v = V2::new(d1.x, d1.y) * dir;
        let a = V2::new(d2.x, d2.y);
        let sp = v.norm().max(tol::TINY);
        (v.x * a.y - v.y * a.x) / (sp * sp * sp) * inward
    };
    if !rounded {
        // A reflex corner (the outline turning outwards) is mitred by the sharp inset.
        let k = outline.len();
        for i in 0..k {
            let (c, a0, a1) = &outline[i];
            let (d, b0, b1) = &outline[(i + 1) % k];
            let ta = c.tangent(*a1) * (a1 - a0).signum();
            let tb = d.tangent(*b0) * (b1 - b0).signum();
            let turn = (ta.x * tb.y - ta.y * tb.x) * inward;
            if turn < -tol::ANGULAR * ta.norm() * tb.norm() {
                return None;
            }
        }
    }
    if folding_only {
        let folds = outline.iter().any(|(c, t0, t1)| {
            !matches!(c, Curve::Line { .. } | Curve::Circle { .. })
                && (0..=256).any(|k| {
                    let u = t0 + (t1 - t0) * k as f64 / 256.0;
                    bend(c, u, (t1 - t0).signum()) * t > 1.0
                })
        });
        if !folds {
            return None;
        }
    }
    Some(extruded_offset(
        &outline,
        t,
        o,
        x,
        n,
        -dist[bot],
        h + dist[top],
    ))
}

/// The outline (a closed wire in the plane z = 0) offset in by `t` and extruded from z0 to z1
/// along n, in the frame (x, n × x, n) at o.
fn extruded_offset(
    outline: &[(Curve, f64, f64)],
    t: f64,
    o: P3,
    x: V3,
    n: V3,
    z0: f64,
    z1: f64,
) -> Result<Body> {
    if z1 - z0 <= tol::ROUNDOFF {
        return Err(KernelError::TooLarge {
            op: "offset",
            why: "the offset prism has no height".into(),
        });
    }
    let loops = offset_wire(outline, -t)?;
    if loops.is_empty() {
        return Err(KernelError::TooLarge {
            op: "offset",
            why: "the offset profile vanishes".into(),
        });
    }
    let mut regions = Vec::new();
    for lp in loops {
        // Counter-clockwise (an inset of one outline has no holes: every loop bounds a piece).
        let mut a = 0.0;
        for p in &lp {
            for k in 0..16 {
                let (u, v) = (
                    p.c.point(p.t0 + (p.t1 - p.t0) * k as f64 / 16.0),
                    p.c.point(p.t0 + (p.t1 - p.t0) * (k + 1) as f64 / 16.0),
                );
                a += u.x * v.y - v.x * u.y;
            }
        }
        let pieces: Vec<Piece2> = if a >= 0.0 {
            lp
        } else {
            lp.iter()
                .rev()
                .map(|p| {
                    let (c, t0, t1) = p.c.reversed(p.t0, p.t1);
                    Piece2 { c, t0, t1 }
                })
                .collect()
        };
        let whole = pieces.len() == 1;
        let curves = pieces
            .iter()
            .map(|p| curve2(p, whole))
            .collect::<Option<Vec<_>>>()
            .ok_or(KernelError::Unsupported("this offset profile's curves"))?;
        regions.push(Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        });
    }
    let plane = Plane {
        origin: o + n * z0,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(n),
    };
    let prof = Profile::new(plane, regions);
    let mut kk = NativeKernel::new();
    let r = crate::Kernel::extrude(&mut kk, &prof, Extent::Blind(z1 - z0))?;
    let [id] = r.bodies[..] else {
        return Err(KernelError::InvalidResult {
            op: "offset",
            why: "the offset prism isn't one body".into(),
        });
    };
    Ok((**kk.get(id)?).clone())
}

/// A piece of the offset (in the plane z = 0, run from t0 to t1) as a profile curve; `whole`
/// when it is the loop's only piece (a whole circle).
fn curve2(p: &Piece2, whole: bool) -> Option<Curve2> {
    let q = |v: &P3| nalgebra::Point2::new(v.x, v.y);
    Some(match &p.c {
        Curve::Line { .. } => Curve2::Line {
            a: q(&p.c.point(p.t0)),
            b: q(&p.c.point(p.t1)),
            source: None,
        },
        Curve::Circle { c, x, y, r } => {
            let dir = if x.cross(y).z > 0.0 { 1.0 } else { -1.0 };
            if whole && (p.t1 - p.t0).abs() >= std::f64::consts::TAU - 1e-9 {
                return Some(Curve2::Circle {
                    center: q(c),
                    radius: r * dir,
                    source: None,
                });
            }
            let s = p.c.point(p.t0) - c;
            Curve2::Arc {
                center: q(c),
                radius: *r,
                start_angle: s.y.atan2(s.x),
                sweep: (p.t1 - p.t0) * dir,
                source: None,
            }
        }
        Curve::Ellipse { c, x, y, a, b } => {
            let dir = if x.cross(y).z > 0.0 { 1.0 } else { -1.0 };
            Curve2::EllipseArc {
                center: q(c),
                major_radius: *a,
                minor_radius: b * dir,
                rotation: x.y.atan2(x.x),
                start: p.t0,
                sweep: p.t1 - p.t0,
                source: None,
            }
        }
        Curve::Nurbs(c) => {
            let c = c.restricted(p.t0, p.t1);
            let rational = c.is_rational();
            Curve2::Spline {
                degree: c.degree,
                knots: c.knots.clone(),
                poles: c.cps.iter().map(|h| q(&dehom(h))).collect(),
                weights: if rational {
                    c.cps.iter().map(|h| h.w).collect()
                } else {
                    vec![]
                },
                source: None,
            }
        }
    })
}

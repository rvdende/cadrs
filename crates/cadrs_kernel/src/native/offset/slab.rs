//! The slab of a face: the solid swept by the face moving along its normals by a distance (out
//! of the body for d > 0, into it for d < 0). Bounded by the face, its offset (the same loops
//! on the offset surface), and a side face between each boundary edge and its offset curve:
//! a plane between parallel lines or coplanar circles, else the ruled surface. The edges at the
//! vertices are the normal lines. Built through [`XBuilder`], which makes the p-curves, seams
//! and senses.

use crate::native::curve::Curve;
use crate::native::math::*;
use crate::native::nurbs::NurbsCurve;
use crate::native::surface::Surface;
use crate::native::tol;
use crate::native::topo::Body;
use crate::native::xbuild::{XBuilder, XFace};
use crate::{KernelError, Result};
use std::collections::HashMap;
use std::sync::Arc;

fn unsupported(m: &'static str) -> KernelError {
    KernelError::Unsupported(m)
}

/// The face's outward normal at a point near it.
fn normal_at(s: &Surface, sense: bool, p: &P3) -> V3 {
    let (u, v) = s.project(p, None);
    s.normal(u, v).normalize() * if sense { 1.0 } else { -1.0 }
}

/// The curve `c` moved by `d` along the face's normal, with the same parameters where that is
/// exact (lines, circles), else a cubic through samples on [t0, t1].
fn offset_curve(
    normal: &dyn Fn(f64) -> V3,
    c: &Curve,
    t0: f64,
    t1: f64,
    d: f64,
    fit: f64,
) -> Curve {
    let at = |t: f64| c.point(t) + end_normal(normal, t, t0, t1) * d;
    let tol = tol::REL * (1.0 + d.abs() + (c.point(t0) - c.point(t1)).norm());
    match c {
        Curve::Line { o, d: dir } => {
            let (a, b) = (at(t0), at(t1));
            let mid = at(0.5 * (t0 + t1));
            let shift = a - c.point(t0);
            if (b - c.point(t1) - shift).norm() < tol
                && (mid - c.point(0.5 * (t0 + t1)) - shift).norm() < tol
            {
                return Curve::Line {
                    o: o + shift,
                    d: *dir,
                };
            }
        }
        Curve::Circle { .. } => {
            // A circle again when three offset points' circle holds the others.
            let (p0, p1, p2) = (
                at(t0),
                at(t0 + (t1 - t0) / 3.0),
                at(t0 + 2.0 * (t1 - t0) / 3.0),
            );
            if let Some((cc, nn, r)) = circle3(&p0, &p1, &p2) {
                // (The axes from points at 0 and π/2, which may lie off the edge.)
                let x = (at(0.0) - cc) / r;
                let y = (at(std::f64::consts::FRAC_PI_2) - cc) / r;
                let cand = Curve::Circle { c: cc, x, y, r };
                let ok = (0..=8).all(|i| {
                    let t = t0 + (t1 - t0) * i as f64 / 8.0;
                    (cand.point(t) - at(t)).norm() < tol * 1e3
                }) && x.dot(&nn).abs() < tol::ANGULAR;
                if ok {
                    return cand;
                }
            }
        }
        _ => {}
    }
    // A cubic through offset points, their number doubled (to 256) until it holds the offset
    // between them too, to `fit` (a spline edge's knots, or a spline face's, kink the offset).
    let mut n = 32;
    loop {
        let params: Vec<f64> = (0..=n)
            .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
            .collect();
        let pts: Vec<P3> = params.iter().map(|&t| at(t)).collect();
        let c = Curve::Nurbs(Arc::new(NurbsCurve::interpolate(
            &pts, &params, 3, None, None,
        )));
        let err = params
            .windows(2)
            .map(|w| (c.point(0.5 * (w[0] + w[1])) - at(0.5 * (w[0] + w[1]))).norm())
            .fold(0.0, f64::max);
        if err < fit || n >= 256 {
            return c;
        }
        n *= 2;
    }
}

/// The normal along a curve at t (`normal(t)`), but at an end where the surface's normals turn
/// round (a pole of a cone-like apex, where any one is as good as another) the limit along the
/// curve: the normal a hair inside.
fn end_normal(normal: &dyn Fn(f64) -> V3, t: f64, t0: f64, t1: f64) -> V3 {
    let n = normal(t);
    let step = tol::REL * (t1 - t0);
    let inner = if t == t0 {
        t0 + step
    } else if t == t1 {
        t1 - step
    } else {
        return n;
    };
    let m = normal(inner);
    if (m - n).norm() > tol::POINT { m } else { n }
}

/// How closely a fitted offset curve follows the true offset (mm, relative to the distance),
/// at least: a tenth of the edge's tolerance is close enough.
const FIT: f64 = 1e-9;

/// How far the curve strays from the surface over [t0, t1] (sampled).
fn stray(s: &Surface, c: &Curve, t0: f64, t1: f64) -> f64 {
    (0..=64)
        .map(|i| s.distance(&c.point(t0 + (t1 - t0) * i as f64 / 64.0)))
        .fold(0.0, f64::max)
}

/// The circle through three points: centre, normal, radius.
fn circle3(a: &P3, b: &P3, c: &P3) -> Option<(P3, V3, f64)> {
    let (u, v) = (b - a, c - a);
    let w = u.cross(&v);
    let ww = w.norm_squared();
    if ww < 1e-24 {
        return None;
    }
    let cc = a + (v * u.norm_squared() - u * v.norm_squared()).cross(&w) * (0.5 / ww);
    Some((cc, w.normalize(), (a - cc).norm()))
}

/// The side surface between an edge and its offset: a plane when they lie in one, else ruled.
fn side_surface(c: &Curve, t0: f64, t1: f64, o: &Curve) -> Surface {
    let pts: Vec<P3> = (0..=8)
        .flat_map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / 8.0;
            [c.point(t), o.point(t)]
        })
        .collect();
    let p0 = pts[0];
    let far = pts
        .iter()
        .max_by(|a, b| (*a - p0).norm().total_cmp(&(*b - p0).norm()))
        .copied()
        .unwrap_or(p0);
    if let Some(x) = (far - p0).try_normalize(tol::DEGENERATE)
        && let Some(n) = pts
            .iter()
            .map(|p| x.cross(&(p - p0)))
            .max_by(|a, b| a.norm().total_cmp(&b.norm()))
            .and_then(|n| n.try_normalize(tol::DEGENERATE))
    {
        let size = (far - p0).norm();
        if pts
            .iter()
            .all(|p| (p - p0).dot(&n).abs() < tol::REL * (1.0 + size))
        {
            return Surface::Plane {
                f: Frame {
                    o: p0,
                    x,
                    y: n.cross(&x),
                    z: n,
                },
            };
        }
    }
    let (mut a, mut b) = (c.to_nurbs(t0, t1), o.to_nurbs(t0, t1));
    while a.degree < b.degree {
        a = a.elevated();
    }
    while b.degree < a.degree {
        b = b.elevated();
    }
    let (a, b) = (a.reparam(0.0, 1.0), b.reparam(0.0, 1.0));
    let mut curves = vec![a, b];
    if !crate::native::loft_build::make_compatible(&mut curves) {
        // Different knots that won't merge: sample both.
        let n = 32;
        let pa: Vec<P3> = (0..=n)
            .map(|i| c.point(t0 + (t1 - t0) * i as f64 / n as f64))
            .collect();
        let pb: Vec<P3> = (0..=n)
            .map(|i| o.point(t0 + (t1 - t0) * i as f64 / n as f64))
            .collect();
        let prm: Vec<f64> = (0..=n).map(|i| i as f64 / n as f64).collect();
        curves = vec![
            NurbsCurve::interpolate(&pa, &prm, 3, None, None),
            NurbsCurve::interpolate(&pb, &prm, 3, None, None),
        ];
    }
    Surface::Nurbs(Arc::new(crate::native::loft_build::skin(
        &curves,
        &[0.0, 1.0],
    )))
}

/// How a slab's face is swept.
#[derive(Clone, Copy)]
enum Sweep {
    /// By a distance along its outward normals (out of the body when positive).
    Normal(f64),
    /// Translated by a vector, out of the body or into it (Move face's sweeps:
    /// [`face_prism`]).
    Along { v: V3, out: bool },
}

/// The slab of face `fi` of `b` swept by `d` along its outward normals.
pub fn face_slab(b: &Body, fi: usize, d: f64) -> Result<Body> {
    slab(b, fi, Sweep::Normal(d)).map(|(body, _)| body)
}

/// The solid face `fi` of `b` sweeps moving by `v` (a straight translation): bounded by the
/// face, the face moved, and a side between each boundary edge and the edge moved (a plane, a
/// cylinder for a circle moved along its axis, else a ruled surface). `out`: the move goes out
/// of the body (the face's outward normal and `v` agree), else into it; the caller checks that
/// they agree in sign all over the face (`v` never runs along it inside). With, for each face of
/// the prism, the edge of `b` its side was swept from.
pub fn face_prism(b: &Body, fi: usize, v: V3, out: bool) -> Result<(Body, Vec<Option<usize>>)> {
    slab(b, fi, Sweep::Along { v, out })
}

/// The slab, and for each of its faces the edge of `b` it is the side of (none for the ends).
fn slab(b: &Body, fi: usize, how: Sweep) -> Result<(Body, Vec<Option<usize>>)> {
    let f = &b.faces[fi];
    // The sweep's length, and whether it goes out of the body.
    let (d, out) = match how {
        Sweep::Normal(d) => (d, d > 0.0),
        Sweep::Along { v, out } => (v.norm(), out),
    };
    // A B-spline face that is a plane, cylinder, cone or sphere is slabbed as one: its offset
    // is exact, and its poles (a cone's apex as a collapsed side) are ones the build knows.
    let seamless = {
        let mut seen = std::collections::HashSet::new();
        f.loops.iter().flatten().all(|c| seen.insert(c.edge))
    };
    let (surf, sense) = match &f.surf {
        // (Through its poles only when no edge is the B-spline's seam: a recognised surface's own
        // seam lies elsewhere, and a loop running out along the B-spline's and back would be a
        // slit on it.)
        Surface::Nurbs(n) => match if seamless {
            crate::native::recognize::surface_through_poles(n)
        } else {
            crate::native::recognize::surface(n)
        } {
            Some(a) => {
                let at = f
                    .loops
                    .iter()
                    .flatten()
                    .map(|c| &b.edges[c.edge])
                    .find(|e| !e.degenerate)
                    .map(|e| e.point(e.mid_t()));
                match at {
                    Some(p) => {
                        let ((u, v), (ua, va)) = (f.surf.project(&p, None), a.project(&p, None));
                        let same = f.surf.normal(u, v).dot(&a.normal(ua, va)) > 0.0;
                        (a, f.sense == same)
                    }
                    None => (f.surf.clone(), f.sense),
                }
            }
            None => (f.surf.clone(), f.sense),
        },
        s => (s.clone(), f.sense),
    };
    let shift = |v: V3| Affine::translation(v);
    let top_surf = match how {
        Sweep::Normal(d) => super::sharp::offset_surface(&surf, sense, d)
            .ok_or(unsupported("slabs of this kind of face"))?,
        Sweep::Along { v, .. } => surf.transformed(&shift(v)),
    };
    // The outward normal along a coedge at t: from its p-curve on the face's own surface (where
    // a projection near a pole could land on any meridian), else projected on the recognised one.
    let own = !matches!(f.surf, Surface::Nurbs(_)) || matches!(surf, Surface::Nurbs(_));
    let normal_on = |c: &crate::native::topo::Coedge, t: f64| -> V3 {
        if own {
            let q = c.pc.point(t);
            f.surf.normal(q.x, q.y).normalize() * if f.sense { 1.0 } else { -1.0 }
        } else {
            normal_at(&surf, sense, &b.edges[c.edge].point(t))
        }
    };
    let mut xb = XBuilder::new();
    let mut vb: HashMap<usize, usize> = HashMap::new();
    let mut vt: HashMap<usize, usize> = HashMap::new();
    let mut rails: HashMap<usize, usize> = HashMap::new();
    let mut eb: HashMap<usize, usize> = HashMap::new();
    let mut et: HashMap<usize, usize> = HashMap::new();
    // How often each edge is used by the face (a seam twice).
    let mut uses: HashMap<usize, usize> = HashMap::new();
    for c in f.loops.iter().flatten() {
        *uses.entry(c.edge).or_default() += 1;
    }
    for c in f.loops.iter().flatten() {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        for (k, v) in e.v.into_iter().enumerate() {
            // The normal as the edge's offset has it at this end (see `end_normal`).
            let n = end_normal(&|t| normal_on(c, t), e.t[k], e.t[0], e.t[1]);
            if let std::collections::hash_map::Entry::Vacant(e) = vb.entry(v) {
                let p = b.verts[v].p;
                let q = match how {
                    Sweep::Normal(d) => p + n * d,
                    Sweep::Along { v, .. } => p + v,
                };
                // (A read model's vertex lies up to its tolerance off its edges: the edges'
                // tolerances, below, cover that.)
                e.insert(xb.vertex(p));
                vt.insert(v, xb.vertex(q));
            }
        }
    }
    // Each side, with the edge it is swept from (none for a pole's fan).
    let mut sides: Vec<(XFace, Option<usize>)> = Vec::new();
    for c in f.loops.iter().flatten() {
        let ei = c.edge;
        let e = &b.edges[ei];
        if e.degenerate || eb.contains_key(&ei) {
            continue;
        }
        let (v0, v1) = (e.v[0], e.v[1]);
        let oc = match how {
            Sweep::Normal(d) => offset_curve(
                &|t| normal_on(c, t),
                &e.curve,
                e.t[0],
                e.t[1],
                d,
                (FIT * (1.0 + d.abs())).max(0.1 * e.tol),
            ),
            Sweep::Along { v, .. } => e.curve.transformed(&shift(v)),
        };
        let a = xb
            .edge(e.curve.clone(), vb[&v0], vb[&v1])
            .ok_or(unsupported("an empty edge in a slab"))?;
        // A seam of a B-spline face (used twice, along an iso line) offsets onto the offset
        // surface's seam: that iso line itself, so its two uses lie on the seam's two sides
        // (a fit strays off it, most near a pole, where its p-curve can't say which side).
        let seam_iso = match &top_surf {
            Surface::Nurbs(tn)
                if matches!(how, Sweep::Normal(_))
                    && own
                    && uses[&ei] > 1
                    && matches!(f.surf, Surface::Nurbs(_)) =>
            {
                let (q0, q1) = (c.pc.point(e.t[0]), c.pc.point(e.t[1]));
                let tol = tol::REL_FINE * (1.0 + q0.coords.norm() + q1.coords.norm());
                let iso = |along_v: bool, at: f64| {
                    Curve::Nurbs(Arc::new(crate::native::local::nurbs_iso(tn, at, along_v)))
                };
                if (q0.y - q1.y).abs() < tol
                    && (0..=8).all(|i| {
                        (c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0).y - q0.y).abs()
                            < tol
                    })
                {
                    Some(iso(false, q0.y))
                } else if (q0.x - q1.x).abs() < tol
                    && (0..=8).all(|i| {
                        (c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0).x - q0.x).abs()
                            < tol
                    })
                {
                    Some(iso(true, q0.x))
                } else {
                    None
                }
            }
            _ => None,
        };
        let (t, oc) = match seam_iso
            .and_then(|iso| xb.edge(iso.clone(), vt[&v0], vt[&v1]).map(|t| (t, iso)))
        {
            Some((t, iso)) => {
                // Its ends off the slab's offset vertices by the offset surface's fit.
                let ed = &xb.body.edges[t];
                let gap = (ed.point(ed.t[0]) - xb.body.verts[ed.v[0]].p)
                    .norm()
                    .max((ed.point(ed.t[1]) - xb.body.verts[ed.v[1]].p).norm());
                xb.body.edges[t].tol = e.tol.max(1.5 * gap);
                (t, iso)
            }
            None => {
                let t = xb
                    .edge(oc.clone(), vt[&v0], vt[&v1])
                    .ok_or(unsupported("an empty offset edge in a slab"))?;
                // Its offset strays from the offset surface by as much as the face's edge, or by
                // the offset surface's fit where that is approximate.
                xb.body.edges[t].tol = e.tol.max(1.5 * stray(&top_surf, &oc, e.t[0], e.t[1]));
                (t, oc)
            }
        };
        // The face's own edge keeps its tolerance.
        xb.body.edges[a].tol = e.tol;
        eb.insert(ei, a);
        et.insert(ei, t);
        // A seam (used twice) bounds no side.
        if uses[&ei] > 1 {
            continue;
        }
        let surf = match (how, &e.curve) {
            // A circle moved along its axis sweeps its cylinder (one surface with a cylinder
            // of the body's it continues, for booleans to merge).
            (Sweep::Along { v, .. }, Curve::Circle { c, x, y, r })
                if x.cross(y).normalize().cross(&v.normalize()).norm() < tol::ANGULAR_POINT =>
            {
                let z = x.cross(y).normalize();
                Surface::Cylinder {
                    f: Frame {
                        o: *c,
                        x: x.normalize(),
                        y: z.cross(&x.normalize()),
                        z,
                    },
                    r: *r,
                }
            }
            _ => side_surface(&e.curve, e.t[0], e.t[1], &oc),
        };
        // A side sampled where the curves' knots won't merge strays by its fit.
        let side =
            1.5 * stray(&surf, &e.curve, e.t[0], e.t[1]).max(stray(&surf, &oc, e.t[0], e.t[1]));
        for x in [a, t] {
            xb.body.edges[x].tol = xb.body.edges[x].tol.max(side);
        }
        let mut rail = |v: usize, xb: &mut XBuilder| -> Result<usize> {
            if let Some(r) = rails.get(&v) {
                return Ok(*r);
            }
            let (p, q) = (xb.body.verts[vb[&v]].p, xb.body.verts[vt[&v]].p);
            let (line, _) = Curve::line(p, q);
            let r = xb
                .edge(line, vb[&v], vt[&v])
                .ok_or(unsupported("a slab of no thickness"))?;
            xb.body.edges[r].tol = b.verts[v].tol;
            rails.insert(v, r);
            Ok(r)
        };
        if v0 == v1 {
            // A closed edge: an annulus between it and its offset in a plane, else a band
            // round the ruled surface, closed by a seam along the rail at its vertex (the
            // surface is closed but not periodic, so its loops can't wrap round it).
            if matches!(surf, Surface::Plane { .. }) {
                sides.push((
                    XFace {
                        surf,
                        sense: true,
                        loops: vec![vec![(a, true)], vec![(t, false)]],
                    },
                    Some(ei),
                ));
            } else {
                let r = rail(v0, &mut xb)?;
                sides.push((
                    XFace {
                        surf,
                        sense: true,
                        loops: vec![vec![(a, true), (r, true), (t, false), (r, false)]],
                    },
                    Some(ei),
                ));
            }
            continue;
        }
        let (r0, r1) = (rail(v0, &mut xb)?, rail(v1, &mut xb)?);
        sides.push((
            XFace {
                surf,
                sense: true,
                loops: vec![vec![(a, true), (r1, true), (t, false), (r0, false)]],
            },
            Some(ei),
        ));
    }
    // A pole where the face's normals turn round (a B-spline closing to a cone-like apex): the
    // normals there sweep a fan, so the offset of the point is a closed curve, an edge of the
    // offset face, and the fan a side between the point and it.
    for c in f.loops.iter().flatten() {
        let (ei, e) = (c.edge, &b.edges[c.edge]);
        if !e.degenerate || et.contains_key(&ei) || matches!(how, Sweep::Along { .. }) {
            continue;
        }
        let v = e.v[0];
        let (Some(&apex), Some(&top)) = (vb.get(&v), vt.get(&v)) else {
            continue;
        };
        let p = b.verts[v].p;
        // Along the pole's p-curve, the normal from just inside the face.
        let at = |t: f64| {
            let q = c.pc.point(t);
            p + f.surf.normal(q.x, q.y).normalize() * if f.sense { d } else { -d }
        };
        let (t0, t1) = (e.t[0], e.t[1]);
        let n = 64;
        let params: Vec<f64> = (0..=n)
            .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
            .collect();
        let pts: Vec<P3> = params.iter().map(|&t| at(t)).collect();
        let q = xb.body.verts[top].p;
        let size = (pts[n / 2] - q).norm();
        // A point after all (a smooth pole), or a fan that doesn't start at the offset vertex.
        let tol = 1e-7 * (1.0 + d.abs());
        if size < tol || (pts[0] - q).norm() > tol || (pts[n] - q).norm() > tol {
            continue;
        }
        let fan_top = Curve::Nurbs(Arc::new(NurbsCurve::interpolate(
            &pts, &params, 3, None, None,
        )));
        let Some(t) = xb.edge(fan_top.clone(), top, top) else {
            continue;
        };
        // (The offset surface is a fit, loosest where the normals turn fastest: its distance
        // from the fan's curve searched finely.)
        let off = (0..=256)
            .map(|i| top_surf.distance(&fan_top.point(t0 + (t1 - t0) * i as f64 / 256.0)))
            .fold(0.0, f64::max);
        xb.body.edges[t].tol = b.verts[v].tol.max(2.0 * off);
        et.insert(ei, t);
        let r = match rails.get(&v) {
            Some(r) => *r,
            None => {
                let (line, _) = Curve::line(p, q);
                let r = xb
                    .edge(line, apex, top)
                    .ok_or(unsupported("a slab of no thickness"))?;
                rails.insert(v, r);
                r
            }
        };
        let surf = side_surface(&e.curve, t0, t1, &fan_top);
        sides.push((
            XFace {
                surf,
                sense: true,
                loops: vec![vec![(r, true), (t, false), (r, false)]],
            },
            None,
        ));
    }
    let map_loops = |m: &HashMap<usize, usize>| -> Vec<Vec<(usize, bool)>> {
        f.loops
            .iter()
            .map(|l| {
                l.iter()
                    .filter(|c| m.contains_key(&c.edge))
                    .map(|c| (m[&c.edge], c.fwd))
                    .collect()
            })
            .filter(|l: &Vec<(usize, bool)>| !l.is_empty())
            .collect()
    };
    let mut faces = vec![
        XFace {
            surf: surf.clone(),
            sense: if out { !sense } else { sense },
            loops: map_loops(&eb),
        },
        XFace {
            surf: top_surf,
            sense: if out { sense } else { !sense },
            loops: map_loops(&et),
        },
    ];
    let swept_from: Vec<Option<usize>> = [None, None]
        .into_iter()
        .chain(sides.iter().map(|(_, e)| *e))
        .collect();
    faces.extend(sides.into_iter().map(|(f, _)| f));
    // The faces in groups joined by their edges: one closed shell, or for a face with no side
    // (a whole sphere, closed by its seam and poles) the face and its offset apart, the outer
    // one bounding the slab and the other its void.
    let mut group: Vec<usize> = (0..faces.len()).collect();
    let root = |g: &[usize], mut i: usize| {
        while g[i] != i {
            i = g[i];
        }
        i
    };
    let mut by_edge: HashMap<usize, usize> = HashMap::new();
    for (i, f) in faces.iter().enumerate() {
        for &(e, _) in f.loops.iter().flatten() {
            let j = *by_edge.entry(e).or_insert(i);
            let (ri, rj) = (root(&group, i), root(&group, j));
            group[ri.max(rj)] = ri.min(rj);
        }
    }
    let mut shells: Vec<(f64, Vec<usize>)> = Vec::new();
    let mut roots: Vec<usize> = Vec::new();
    for i in 0..faces.len() {
        let r = root(&group, i);
        let k = roots.iter().position(|&x| x == r).unwrap_or_else(|| {
            roots.push(r);
            shells.push((0.0, Vec::new()));
            roots.len() - 1
        });
        shells[k].1.push(i);
    }
    // The outer shell (the largest box) first.
    for (size, members) in shells.iter_mut() {
        let mut bb = Bbox::empty();
        for &(e, _) in members
            .iter()
            .flat_map(|&i| faces[i].loops.iter().flatten())
        {
            let ed = &xb.body.edges[e];
            (0..=4).for_each(|k| bb.add(&ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * k as f64 / 4.0)));
        }
        *size = bb.diagonal();
    }
    shells.sort_by(|a, b| b.0.total_cmp(&a.0));
    let mut ids: Vec<Option<XFace>> = faces.into_iter().map(Some).collect();
    // The builder's ids of the face itself and its offset, and of each face.
    let mut ends = [usize::MAX; 2];
    let mut built = vec![usize::MAX; swept_from.len()];
    let shells: Vec<usize> = shells
        .into_iter()
        .map(|(_, members)| {
            let mut fs = Vec::new();
            for i in members {
                if let Some(f) = ids[i].take() {
                    let id = xb.face(f);
                    if i < 2 {
                        ends[i] = id;
                    }
                    built[i] = id;
                    fs.push(id);
                }
            }
            xb.shell(fs, true)
        })
        .collect();
    xb.solid(shells);
    let (mut body, warnings, map) = xb.finish_mapped();
    // Tolerances to the gaps the fits leave (an offset surface's fit against the offset
    // vertices and curves), as a full check measures them.
    let size = body.bbox().diagonal();
    body.fit_tolerances_searched(1e-5 * (1.0 + size));
    if std::env::var_os("CADRS_SLAB_DEBUG").is_some() {
        eprintln!(
            "slab of face {fi} ({:?}) by {d}: {warnings:?}",
            f.surf.kind()
        );
    }
    let mut issues = body.check();
    // A slab whose faces all point in (its closed shell oriented the wrong way round): turned
    // inside out.
    if !issues.is_empty() && issues.iter().all(|s| s.contains("encloses a volume")) {
        body.faces.iter_mut().for_each(Body::reverse_face);
        issues = body.check();
    }
    if !issues.is_empty() || body.solids.is_empty() {
        return Err(KernelError::InvalidResult {
            op: "slab",
            why: format!(
                "the face's slab isn't valid: {}",
                issues.first().map_or("no solid", |s| s.as_str())
            ),
        });
    }
    let volume = crate::native::mass::body_volume(&body);
    if volume <= 0.0 {
        return Err(KernelError::InvalidResult {
            op: "slab",
            why: "the face's slab has no volume".into(),
        });
    }
    // Its volume the prismoid's, d × the mean of its two ends' areas, but for the faces' curving
    // (exact on a cylinder, 4/3 of it at most on a sphere offset by its radius): far off, the
    // offset face's loops bound another region than the face's (a wrong body, not an error).
    let area = |k: usize| {
        map.get(ends[k])
            .copied()
            .flatten()
            .map_or(0.0, |x| crate::native::mass::face_area(&body, x).0.abs())
    };
    let prismoid = d.abs() * 0.5 * (area(0) + area(1));
    // A translated face sweeps its area seen along the move: a flat one exactly its area times
    // the move across it, a curved one at most its area times the move.
    let fits = match how {
        Sweep::Normal(_) => volume < 2.0 * prismoid && volume > 0.5 * prismoid,
        Sweep::Along { v, .. } => match &surf {
            Surface::Plane { f: fr } => {
                let across = area(0) * v.dot(&fr.z.normalize()).abs();
                (volume - across).abs() <= 1e-6 * (1.0 + across)
            }
            _ => volume <= prismoid * (1.0 + 1e-6),
        },
    };
    if !fits {
        return Err(KernelError::InvalidResult {
            op: "slab",
            why: format!(
                "the face's slab encloses {volume:.6e}, not about its ends' {prismoid:.6e}"
            ),
        });
    }
    let mut edges = vec![None; body.faces.len()];
    for (i, &e) in swept_from.iter().enumerate() {
        if let Some(Some(x)) = map.get(built[i]).copied() {
            edges[x] = e;
        }
    }
    Ok((body, edges))
}

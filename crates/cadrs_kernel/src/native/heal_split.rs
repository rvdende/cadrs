//! Faces split along iso-parametric lines, and the shape upgrades built on it (OCCT's
//! `ShapeUpgrade` face dividers, behind DRAW's `DT_SplitAngle`, `DT_ClosedSplit`,
//! `DT_ShapeDivide` and `DT_ShapeConvert`):
//! - [`split_angle`]: faces of surfaces of revolution cut so none turns through more than an
//!   angle (`ShapeUpgrade_ShapeDivideAngle`);
//! - [`split_closed`]: faces closed on themselves (a seam) cut in two across each closed
//!   direction (`ShapeUpgrade_ShapeDivideClosed`);
//! - [`split_continuity`]: faces and edges cut where their B-spline is only C0
//!   (`ShapeUpgrade_ShapeDivideContinuity`);
//! - [`split_knots`]: faces and edges cut at every knot of their B-spline (the patches of
//!   `ShapeUpgrade_ShapeConvertToBezier`).
//!
//! A face is cut at values of one parameter: where each loop's p-curves cross the iso-line the
//! edges are split (the neighbouring faces share the new vertices), the iso-line's stretches
//! inside the face become new edges (their 3D curve the surface's exact iso-curve, their
//! p-curve a line), and the face's loops and the cuts are traced into faces
//! ([`super::split::split_face`]). A face whose pieces don't add up to its area is left whole.

use super::classify::{InFace, face_polygons, in_face, periodic_candidates};
use super::curve::Curve;
use super::math::*;
use super::nurbs::{H, NurbsCurve, basis_ders, find_span};
use super::split::Arc as SArc;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Edge, Face};
use crate::Result;

/// A parameter direction of a surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dir {
    /// Lines u = const.
    U,
    /// Lines v = const.
    V,
}

/// The uv box of polygons.
fn uv_box(polys: &[Vec<P2>]) -> Option<((f64, f64), (f64, f64))> {
    let mut u = (f64::INFINITY, f64::NEG_INFINITY);
    let mut v = (f64::INFINITY, f64::NEG_INFINITY);
    for q in polys.iter().flatten() {
        u = (u.0.min(q.x), u.1.max(q.x));
        v = (v.0.min(q.y), v.1.max(q.y));
    }
    (u.0 <= u.1 && v.0 <= v.1).then_some((u, v))
}

/// Face `fi`'s box in (u, v), from its loops (the outer loop's chart).
pub fn face_uv_box(b: &Body, fi: usize) -> Option<((f64, f64), (f64, f64))> {
    uv_box(&face_polygons(b, &b.faces[fi]))
}

/// The point of homogeneous B-spline control points `cps` (degree `p`, `knots`) at `t`, still
/// homogeneous.
fn hom_point(p: usize, knots: &[f64], cps: &[H], t: f64) -> H {
    let span = find_span(cps.len() - 1, p, t, knots);
    let n = &basis_ders(span, t, p, 0, knots)[0];
    let mut h = H::zeros();
    for (i, w) in n.iter().enumerate() {
        h += cps[span - p + i] * *w;
    }
    h
}

/// The surface's iso-line at `c` in direction `dir` as a 3D curve parametrized by the other
/// parameter (the edge of an iso cut, its p-curve a line): exact for every kind of surface.
pub fn iso_curve(s: &Surface, dir: Dir, c: f64) -> Option<Curve> {
    let rot = |f: &Frame, a: f64| {
        let (sa, ca) = a.sin_cos();
        f.x * ca + f.y * sa
    };
    Some(match (s, dir) {
        (Surface::Plane { f }, Dir::U) => Curve::Line {
            o: f.o + f.x * c,
            d: f.y,
        },
        (Surface::Plane { f }, Dir::V) => Curve::Line {
            o: f.o + f.y * c,
            d: f.x,
        },
        (Surface::Cylinder { f, r }, Dir::U) => Curve::Line {
            o: f.o + rot(f, c) * *r,
            d: f.z,
        },
        (Surface::Cylinder { f, r }, Dir::V) => Curve::Circle {
            c: f.o + f.z * c,
            x: f.x,
            y: f.y,
            r: *r,
        },
        (Surface::Cone { f, r, a }, Dir::U) => Curve::Line {
            o: f.o + rot(f, c) * *r,
            d: rot(f, c) * a.sin() + f.z * a.cos(),
        },
        (Surface::Cone { f, r, a }, Dir::V) => {
            let rr = r + c * a.sin();
            if rr.abs() < tol::DEGENERATE {
                return None;
            }
            // (A negative radius is the circle half a turn on.)
            Curve::Circle {
                c: f.o + f.z * (c * a.cos()),
                x: f.x * rr.signum(),
                y: f.y * rr.signum(),
                r: rr.abs(),
            }
        }
        (Surface::Sphere { f, r }, Dir::U) => Curve::Circle {
            c: f.o,
            x: rot(f, c),
            y: f.z,
            r: *r,
        },
        (Surface::Sphere { f, r }, Dir::V) => {
            let rr = r * c.cos();
            if rr.abs() < tol::REL_FINE * r {
                return None;
            }
            Curve::Circle {
                c: f.o + f.z * (r * c.sin()),
                x: f.x * rr.signum(),
                y: f.y * rr.signum(),
                r: rr.abs(),
            }
        }
        (Surface::Torus { f, rmaj, rmin }, Dir::U) => Curve::Circle {
            c: f.o + rot(f, c) * *rmaj,
            x: rot(f, c),
            y: f.z,
            r: *rmin,
        },
        (Surface::Torus { f, rmaj, rmin }, Dir::V) => {
            let rr = rmaj + rmin * c.cos();
            if rr.abs() < tol::DEGENERATE {
                return None;
            }
            Curve::Circle {
                c: f.o + f.z * (rmin * c.sin()),
                x: f.x * rr.signum(),
                y: f.y * rr.signum(),
                r: rr.abs(),
            }
        }
        (Surface::Revolution { f, m }, Dir::U) => {
            // The meridian turned by c about the axis: F·Rz(c)·Fᵀ (a rotation for either
            // handedness of F).
            let fm = nalgebra::Matrix3::from_columns(&[f.x, f.y, f.z]);
            let (sc, cc) = c.sin_cos();
            let rz = nalgebra::Matrix3::new(cc, -sc, 0.0, sc, cc, 0.0, 0.0, 0.0, 1.0);
            let m3 = fm * rz * fm.transpose();
            m.transformed(&Affine {
                m: m3,
                t: f.o.coords - m3 * f.o.coords,
            })
        }
        (Surface::Revolution { f, m }, Dir::V) => {
            let l = f.local(&m.point(c));
            let rho = (l.x * l.x + l.y * l.y).sqrt();
            if rho < tol::DEGENERATE {
                return None;
            }
            // The meridian's point at u = 0 lies at angle φ in the frame's xy plane.
            let phi = l.y.atan2(l.x);
            Curve::Circle {
                c: f.o + f.z * l.z,
                x: rot(f, phi),
                y: rot(f, phi + PI / 2.0),
                r: rho,
            }
        }
        (Surface::Extrusion { c: base, d }, Dir::U) => Curve::Line {
            o: base.point(c),
            d: *d,
        },
        (Surface::Extrusion { c: base, d }, Dir::V) => {
            base.transformed(&Affine::translation(d * c))
        }
        (Surface::Nurbs(n), Dir::U) => {
            let cps: Vec<H> = (0..n.cps[0].len())
                .map(|j| {
                    hom_point(
                        n.du,
                        &n.ku,
                        &n.cps.iter().map(|r| r[j]).collect::<Vec<_>>(),
                        c,
                    )
                })
                .collect();
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve {
                degree: n.dv,
                knots: n.kv.clone(),
                cps,
            }))
        }
        (Surface::Nurbs(n), Dir::V) => {
            let cps: Vec<H> = n.cps.iter().map(|r| hom_point(n.dv, &n.kv, r, c)).collect();
            Curve::Nurbs(std::sync::Arc::new(NurbsCurve {
                degree: n.du,
                knots: n.ku.clone(),
                cps,
            }))
        }
    })
}

/// A crossing of the iso-line by a loop: the edge, its parameter there, the coordinate along
/// the iso-line, and the vertex when it is an edge's end (or a pole).
struct Hit {
    edge: usize,
    t: f64,
    along: f64,
    vertex: Option<usize>,
}

/// The parameters in [t0, t1] where g crosses zero (sampled, then bisected), with whether each
/// is at an end.
fn roots(g: &dyn Fn(f64) -> f64, t0: f64, t1: f64, n: usize) -> Vec<f64> {
    let mut out = Vec::new();
    let ts: Vec<f64> = (0..=n)
        .map(|i| t0 + (t1 - t0) * i as f64 / n as f64)
        .collect();
    let gs: Vec<f64> = ts.iter().map(|&t| g(t)).collect();
    for i in 0..n {
        let (a, b) = (gs[i], gs[i + 1]);
        if a == 0.0 {
            out.push(ts[i]);
            continue;
        }
        if a * b < 0.0 {
            let (mut lo, mut hi, mut glo) = (ts[i], ts[i + 1], a);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                let gm = g(mid);
                if gm == 0.0 {
                    lo = mid;
                    hi = mid;
                    break;
                }
                if (gm < 0.0) == (glo < 0.0) {
                    lo = mid;
                    glo = gm;
                } else {
                    hi = mid;
                }
            }
            out.push(0.5 * (lo + hi));
        }
    }
    if gs[n] == 0.0 {
        out.push(ts[n]);
    }
    out
}

/// Splits face `fi` of `b` along the iso-lines at `values` of direction `dir` (values outside
/// the face's range are skipped). Returns the faces it became (`fi` first); the body is left as
/// it was when the cut doesn't give a sound division.
pub fn split_face_iso(b: &mut Body, fi: usize, dir: Dir, values: &[f64]) -> Result<Vec<usize>> {
    let mut trial = b.clone();
    match split_in(&mut trial, fi, dir, values) {
        Some(faces) if faces.len() > 1 => {
            *b = trial;
            Ok(faces)
        }
        _ => Ok(vec![fi]),
    }
}

/// [`split_face_iso`] on `b` itself (`None`: the cut failed, `b` is half done).
fn split_in(b: &mut Body, fi: usize, dir: Dir, values: &[f64]) -> Option<Vec<usize>> {
    let surf = b.faces[fi].surf.clone();
    let polys = face_polygons(b, &b.faces[fi]);
    let ((u0, u1), (v0, v1)) = uv_box(&polys)?;
    let ((a0, a1), (s0, s1)) = match dir {
        Dir::U => ((u0, u1), (v0, v1)),
        Dir::V => ((v0, v1), (u0, u1)),
    };
    let (per_a, per_s) = match dir {
        Dir::U => (surf.u_periodic(), surf.v_periodic()),
        Dir::V => (surf.v_periodic(), surf.u_periodic()),
    };
    let coord = |p: P3| match dir {
        Dir::U => (p.x, p.y),
        Dir::V => (p.y, p.x),
    };
    let eps = 1e-7 * (1.0 + (a1 - a0).abs());
    let mut values: Vec<f64> = values
        .iter()
        .copied()
        .filter(|&c| c > a0 + eps && c < a1 - eps)
        .collect();
    values.sort_by(f64::total_cmp);
    values.dedup_by(|x, y| (*x - *y).abs() <= eps);
    if values.is_empty() {
        return None;
    }
    let size = b.face_bbox(fi).diagonal().max(tol::ROUNDOFF);
    let area0 = super::mass::face_area(b, fi).0;
    // 1. Where the loops cross each iso-line.
    let mut hits: Vec<Vec<Hit>> = values.iter().map(|_| Vec::new()).collect();
    for c in b.faces[fi].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        let n = if matches!(c.pc, Curve::Line { .. }) {
            2
        } else {
            48
        };
        let (lo, hi) = (e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
        // The coedge's range of the cut coordinate (whole periods of a periodic one are tried).
        let samples: Vec<f64> = (0..=16)
            .map(|i| coord(c.pc.point(lo + (hi - lo) * i as f64 / 16.0)).0)
            .collect();
        let (mn, mx) = samples
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &x| {
                (a.min(x), b.max(x))
            });
        for (k, &val) in values.iter().enumerate() {
            let shifts: Vec<f64> = if per_a {
                (-3..=3)
                    .map(|j| val + TAU * j as f64)
                    .filter(|x| *x >= mn - 1.0 && *x <= mx + 1.0)
                    .collect()
            } else {
                vec![val]
            };
            for target in shifts {
                let g = |t: f64| coord(c.pc.point(t)).0 - target;
                for t in roots(&g, lo, hi, n) {
                    let along = coord(c.pc.point(t)).1;
                    // (A pole's crossing is its vertex.)
                    let vertex = if e.degenerate
                        || (t - e.t[0]).abs() <= tol::REL * (1.0 + (hi - lo))
                        || (e.point(t) - b.verts[e.v[0]].p).norm() <= tol::REL * size
                    {
                        Some(e.v[0])
                    } else if (t - e.t[1]).abs() <= tol::REL * (1.0 + (hi - lo))
                        || (e.point(t) - b.verts[e.v[1]].p).norm() <= tol::REL * size
                    {
                        Some(e.v[1])
                    } else {
                        None
                    };
                    hits[k].push(Hit {
                        edge: c.edge,
                        t,
                        along,
                        vertex,
                    });
                }
            }
        }
    }
    // 2. Edges split where they cross (the highest parameter first, so the earlier pieces keep
    // the edge's index).
    let mut splits: Vec<(usize, f64)> = hits
        .iter()
        .flatten()
        .filter(|h| h.vertex.is_none())
        .map(|h| (h.edge, h.t))
        .collect();
    splits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.total_cmp(&a.1)));
    splits.dedup_by(|x, y| x.0 == y.0 && (x.1 - y.1).abs() <= tol::REL_FINE * (1.0 + x.1.abs()));
    let mut made: Vec<((usize, f64), usize)> = Vec::new();
    for &(e, t) in &splits {
        let v = super::unify::split_edge(b, e, t);
        made.push(((e, t), v));
    }
    for h in hits.iter_mut().flatten() {
        if h.vertex.is_none() {
            h.vertex = made
                .iter()
                .find(|((e, t), _)| {
                    *e == h.edge && (*t - h.t).abs() <= tol::REL_FINE * (1.0 + t.abs())
                })
                .map(|x| x.1);
        }
    }
    // 3. The stretches of each iso-line inside the face: new edges.
    let face_polys = face_polygons(b, &b.faces[fi]);
    let mut cuts: Vec<SArc> = Vec::new();
    for (k, &val) in values.iter().enumerate() {
        let mut pts: Vec<(f64, usize)> = hits[k]
            .iter()
            .filter_map(|h| {
                let mut s = h.along;
                if per_s {
                    while s < s0 - tol::ROUNDOFF {
                        s += TAU;
                    }
                    while s > s1 + tol::ROUNDOFF {
                        s -= TAU;
                    }
                }
                Some((s, h.vertex?))
            })
            .collect();
        pts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        pts.dedup_by(|x, y| x.1 == y.1 && (x.0 - y.0).abs() <= tol::REL * (1.0 + (s1 - s0)));
        let curve = iso_curve(&surf, dir, val)?;
        for w in pts.windows(2) {
            let ((sa, va), (sb, vb)) = (w[0], w[1]);
            if sb - sa <= tol::REL * (1.0 + (s1 - s0)) {
                continue;
            }
            let mid = 0.5 * (sa + sb);
            let uv = match dir {
                Dir::U => P2::new(val, mid),
                Dir::V => P2::new(mid, val),
            };
            if !periodic_candidates(&surf, uv.x, uv.y, &face_polys)
                .into_iter()
                .any(|q| in_face(&face_polys, &surf, q, 1e-18) == InFace::Inside)
            {
                continue;
            }
            let pc = match dir {
                Dir::U => Curve::Line {
                    o: P3::new(val, 0.0, 0.0),
                    d: V3::y(),
                },
                Dir::V => Curve::Line {
                    o: P3::new(0.0, val, 0.0),
                    d: V3::x(),
                },
            };
            b.edges.push(Edge {
                curve: curve.clone(),
                t: [sa, sb],
                v: [va, vb],
                degenerate: false,
                tol: super::topo::TOL_MIN,
            });
            cuts.push(SArc {
                edge: b.edges.len() - 1,
                fwd: true,
                pc,
                t: [sa, sb],
                v: [va, vb],
                cut: true,
                deg: false,
            });
        }
    }
    if cuts.is_empty() {
        return None;
    }
    trace_pieces(b, fi, cuts, area0)
}

/// Face `fi` of `b` traced into pieces along `cuts` (new edges already in `b`, ending at
/// vertices of its loops): the pieces replace it (`fi` first, the rest added to its shell).
/// `None` when a piece's loop doesn't close or the pieces' areas don't add up to `area0`.
pub(crate) fn trace_pieces(
    b: &mut Body,
    fi: usize,
    cuts: Vec<SArc>,
    area0: f64,
) -> Option<Vec<usize>> {
    // 4. The face traced into pieces.
    let f = b.faces[fi].clone();
    let boundary: Vec<Vec<SArc>> = f
        .loops
        .iter()
        .map(|l| {
            l.iter()
                .map(|c| {
                    let e = &b.edges[c.edge];
                    SArc {
                        edge: c.edge,
                        fwd: c.fwd,
                        pc: c.pc.clone(),
                        t: e.t,
                        v: [b.co_start(c), b.co_end(c)],
                        cut: false,
                        deg: e.degenerate,
                    }
                })
                .collect()
        })
        .collect();
    let res = super::split::split_face(&f.surf, !f.sense, &boundary, &cuts);
    if res.faces.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    let mut deg_pieces: Vec<((usize, i64, i64), usize)> = Vec::new();
    for loops in res.faces {
        let mut face = Face {
            surf: f.surf.clone(),
            sense: f.sense,
            loops: Vec::new(),
        };
        for l in loops {
            let mut coedges = Vec::new();
            for a in l {
                let mut edge = a.edge;
                let (lo, hi) = (a.t[0].min(a.t[1]), a.t[0].max(a.t[1]));
                let e = &b.edges[edge];
                if a.deg
                    && ((lo - e.t[0]).abs() > tol::ROUNDOFF_FINE
                        || (hi - e.t[1]).abs() > tol::ROUNDOFF_FINE)
                {
                    // A piece of a pole: an edge of its own.
                    let key = (edge, (lo * 1e9).round() as i64, (hi * 1e9).round() as i64);
                    edge = match deg_pieces.iter().find(|(k, _)| *k == key) {
                        Some(&(_, x)) => x,
                        None => {
                            b.edges.push(Edge {
                                t: [lo, hi],
                                ..e.clone()
                            });
                            deg_pieces.push((key, b.edges.len() - 1));
                            b.edges.len() - 1
                        }
                    };
                }
                coedges.push(Coedge {
                    edge,
                    fwd: a.fwd,
                    pc: a.pc,
                });
            }
            face.loops.push(coedges);
        }
        if out.is_empty() {
            b.faces[fi] = face;
            out.push(fi);
        } else {
            b.faces.push(face);
            out.push(b.faces.len() - 1);
        }
    }
    // Every piece's loops close, and the pieces make up the face.
    for &g in &out {
        for l in &b.faces[g].loops {
            for k in 0..l.len() {
                if b.co_end(&l[k]) != b.co_start(&l[(k + 1) % l.len()]) {
                    return None;
                }
            }
        }
    }
    let area: f64 = out.iter().map(|&g| super::mass::face_area(b, g).0).sum();
    if !((area - area0).abs() <= tol::REL_POINT * area0.abs().max(tol::DEGENERATE)) {
        return None;
    }
    for s in &mut b.shells {
        if s.faces.contains(&fi) {
            s.faces.extend(out[1..].iter().copied());
        }
    }
    Some(out)
}

/// The angle a face of a surface of revolution turns through about its axis (`None` for other
/// surfaces): u is the angle on cylinders, cones, spheres, tori and surfaces of revolution.
fn turns(s: &Surface) -> bool {
    matches!(
        s,
        Surface::Cylinder { .. }
            | Surface::Cone { .. }
            | Surface::Sphere { .. }
            | Surface::Torus { .. }
            | Surface::Revolution { .. }
    )
}

/// Cuts the faces of surfaces of revolution so none spans more than `max_angle` (radians) about
/// its axis: a face over an angle a is cut into ⌈a / max_angle⌉ equal sectors. Returns, for
/// each face of the result, the face of `b` it is a piece of.
pub fn split_angle(b: &mut Body, max_angle: f64) -> Result<Vec<usize>> {
    let max_angle = max_angle.max(1e-3);
    each_face(b, |b, fi| {
        if !turns(&b.faces[fi].surf) {
            return Ok(vec![fi]);
        }
        let Some(((u0, u1), _)) = face_uv_box(b, fi) else {
            return Ok(vec![fi]);
        };
        let span = u1 - u0;
        if span <= max_angle * (1.0 + tol::REL) {
            return Ok(vec![fi]);
        }
        let n = (span / max_angle - tol::ROUNDOFF).ceil() as usize;
        let values: Vec<f64> = (1..n).map(|k| u0 + span * k as f64 / n as f64).collect();
        split_face_iso(b, fi, Dir::U, &values)
    })
}

/// Whether face `fi` closes on itself across direction `dir`: an edge it uses twice with
/// p-curves a period apart in that direction (a seam).
fn closed_across(b: &Body, fi: usize, dir: Dir) -> bool {
    let f = &b.faces[fi];
    let cs: Vec<&Coedge> = f.loops.iter().flatten().collect();
    cs.iter().enumerate().any(|(i, c)| {
        let e = &b.edges[c.edge];
        !e.degenerate
            && cs[i + 1..].iter().any(|d| {
                d.edge == c.edge && {
                    let t = e.mid_t();
                    let (p, q) = (c.pc.point(t), d.pc.point(t));
                    let (dp, other) = match dir {
                        Dir::U => ((p.x - q.x).abs(), (p.y - q.y).abs()),
                        Dir::V => ((p.y - q.y).abs(), (p.x - q.x).abs()),
                    };
                    dp > tol::POINT && other <= tol::REL_POINT * (1.0 + dp)
                }
            })
    })
}

/// Cuts each face that closes on itself (a seam in u or v) into two across each closed
/// direction, at the middle of its range. Returns each result face's face of `b`.
pub fn split_closed(b: &mut Body) -> Result<Vec<usize>> {
    each_face(b, |b, fi| {
        let mut pieces = vec![fi];
        for dir in [Dir::U, Dir::V] {
            let mut next = Vec::new();
            for g in pieces {
                if !closed_across(b, g, dir) {
                    next.push(g);
                    continue;
                }
                let Some(((u0, u1), (v0, v1))) = face_uv_box(b, g) else {
                    next.push(g);
                    continue;
                };
                let mid = match dir {
                    Dir::U => 0.5 * (u0 + u1),
                    Dir::V => 0.5 * (v0 + v1),
                };
                next.extend(split_face_iso(b, g, dir, &[mid])?);
            }
            pieces = next;
        }
        Ok(pieces)
    })
}

/// The knots of a B-spline where it is only C0 (multiplicity at least its degree) and not
/// tangent-continuous either, inside (a, b).
fn c0_knots(
    degree: usize,
    knots: &[f64],
    a: f64,
    b: f64,
    tangent: &dyn Fn(f64, bool) -> V3,
) -> Vec<f64> {
    distinct_knots(knots, a, b)
        .into_iter()
        .filter(|&k| {
            let m = knots
                .iter()
                .filter(|x| (**x - k).abs() <= tol::REL_FINE * (1.0 + k.abs()))
                .count();
            m >= degree && {
                let (l, r) = (tangent(k, false), tangent(k, true));
                (l - r).norm() > tol::REL * (l.norm() + r.norm())
            }
        })
        .collect()
}

/// The distinct knot values strictly inside (a, b).
fn distinct_knots(knots: &[f64], a: f64, b: f64) -> Vec<f64> {
    let eps = tol::REL * (1.0 + (b - a).abs());
    let mut out: Vec<f64> = knots
        .iter()
        .copied()
        .filter(|&k| k > a + eps && k < b - eps)
        .collect();
    out.dedup_by(|x, y| (*x - *y).abs() <= tol::REL_FINE * (1.0 + x.abs()));
    out
}

/// A B-spline curve's derivative at `t` from the left (`right` false) or the right.
fn side_tangent(n: &NurbsCurve, t: f64, right: bool) -> V3 {
    let (lo, hi) = n.range();
    let h = 1e-7 * (hi - lo);
    let s = if right {
        (t + h).min(hi)
    } else {
        (t - h).max(lo)
    };
    n.ders(s, 1)[1]
}

/// Cuts edges and faces where their B-splines are only C0, so each edge's curve and each face's
/// surface is at least C1 over it, and restricts edge curves to their edges. Returns each
/// result face's face of `b`.
pub fn split_continuity(b: &mut Body) -> Result<Vec<usize>> {
    split_edges_at(b, &|n, a, z| {
        c0_knots(n.degree, &n.knots, a, z, &|t, r| side_tangent(n, t, r))
    });
    each_face(b, |b, fi| {
        let Surface::Nurbs(n) = b.faces[fi].surf.clone() else {
            return Ok(vec![fi]);
        };
        let Some(((u0, u1), (v0, v1))) = face_uv_box(b, fi) else {
            return Ok(vec![fi]);
        };
        let (vm, um) = (0.5 * (v0 + v1), 0.5 * (u0 + u1));
        let du = |u: f64, right: bool| {
            let h = 1e-7 * (u1 - u0).max(tol::DEGENERATE);
            let s = if right { u + h } else { u - h };
            n.ders1(s, vm).1
        };
        let dv = |v: f64, right: bool| {
            let h = 1e-7 * (v1 - v0).max(tol::DEGENERATE);
            let s = if right { v + h } else { v - h };
            n.ders1(um, s).2
        };
        let us = c0_knots(n.du, &n.ku, u0, u1, &du);
        let vs = c0_knots(n.dv, &n.kv, v0, v1, &dv);
        split_both(b, fi, &us, &vs)
    })
}

/// Cuts edges and faces at every knot of their B-splines (curves and surfaces of one span each:
/// Bézier pieces), restricting each curve and surface to its piece. Returns each result face's
/// face of `b`.
pub fn split_knots(b: &mut Body) -> Result<Vec<usize>> {
    split_edges_at(b, &|n, a, z| distinct_knots(&n.knots, a, z));
    let from = each_face(b, |b, fi| {
        let Surface::Nurbs(n) = b.faces[fi].surf.clone() else {
            return Ok(vec![fi]);
        };
        let Some(((u0, u1), (v0, v1))) = face_uv_box(b, fi) else {
            return Ok(vec![fi]);
        };
        split_both(
            b,
            fi,
            &distinct_knots(&n.ku, u0, u1),
            &distinct_knots(&n.kv, v0, v1),
        )
    })?;
    // Each face's surface restricted to the span it lies in.
    for fi in 0..b.faces.len() {
        let Surface::Nurbs(n) = &b.faces[fi].surf else {
            continue;
        };
        let Some(((u0, u1), (v0, v1))) = face_uv_box(b, fi) else {
            continue;
        };
        let span = |knots: &[f64], a: f64, z: f64| {
            let m = 0.5 * (a + z);
            let lo = knots
                .iter()
                .copied()
                .filter(|&k| k <= m)
                .fold(f64::NEG_INFINITY, f64::max);
            let hi = knots
                .iter()
                .copied()
                .filter(|&k| k > m)
                .fold(f64::INFINITY, f64::min);
            (lo, hi)
        };
        let (su, sv) = (span(&n.ku, u0, u1), span(&n.kv, v0, v1));
        if !(su.0.is_finite() && su.1.is_finite() && sv.0.is_finite() && sv.1.is_finite()) {
            continue;
        }
        if let Some(r) = restrict_surface(n, su, sv) {
            b.faces[fi].surf = Surface::Nurbs(std::sync::Arc::new(r));
        }
    }
    Ok(from)
}

/// A B-spline surface restricted to the parameter box u × v (the same parametrization).
pub fn restrict_surface(
    n: &super::nurbs::NurbsSurface,
    u: (f64, f64),
    v: (f64, f64),
) -> Option<super::nurbs::NurbsSurface> {
    let rows: Vec<NurbsCurve> = n
        .cps
        .iter()
        .map(|r| {
            NurbsCurve {
                degree: n.dv,
                knots: n.kv.clone(),
                cps: r.clone(),
            }
            .restricted(v.0, v.1)
        })
        .collect();
    let kv = rows.first()?.knots.clone();
    let nv = rows[0].cps.len();
    let cols: Vec<NurbsCurve> = (0..nv)
        .map(|j| {
            NurbsCurve {
                degree: n.du,
                knots: n.ku.clone(),
                cps: rows.iter().map(|r| r.cps[j]).collect(),
            }
            .restricted(u.0, u.1)
        })
        .collect();
    let ku = cols.first()?.knots.clone();
    let nu = cols[0].cps.len();
    let cps: Vec<Vec<H>> = (0..nu)
        .map(|i| cols.iter().map(|c| c.cps[i]).collect())
        .collect();
    super::nurbs::NurbsSurface::new(n.du, n.dv, ku, kv, cps).ok()
}

/// Splits face `fi` along u at `us`, then each piece along v at `vs`.
fn split_both(b: &mut Body, fi: usize, us: &[f64], vs: &[f64]) -> Result<Vec<usize>> {
    let pieces = if us.is_empty() {
        vec![fi]
    } else {
        split_face_iso(b, fi, Dir::U, us)?
    };
    if vs.is_empty() {
        return Ok(pieces);
    }
    let mut out = Vec::new();
    for g in pieces {
        out.extend(split_face_iso(b, g, Dir::V, vs)?);
    }
    Ok(out)
}

/// Splits each edge with a B-spline curve at the parameters `at` gives inside its range, then
/// restricts every B-spline edge curve to its edge.
fn split_edges_at(b: &mut Body, at: &dyn Fn(&NurbsCurve, f64, f64) -> Vec<f64>) {
    let n0 = b.edges.len();
    for ei in 0..n0 {
        let e = &b.edges[ei];
        let Curve::Nurbs(n) = &e.curve else { continue };
        if e.degenerate {
            continue;
        }
        let mut ts = at(n, e.t[0], e.t[1]);
        ts.sort_by(|x, y| y.total_cmp(x));
        for t in ts {
            super::unify::split_edge(b, ei, t);
        }
    }
    for e in &mut b.edges {
        if let Curve::Nurbs(n) = &e.curve
            && !e.degenerate
        {
            let (lo, hi) = n.range();
            if e.t[0] > lo + tol::REL_FINE * (hi - lo) || e.t[1] < hi - tol::REL_FINE * (hi - lo) {
                e.curve = Curve::Nurbs(std::sync::Arc::new(n.restricted(e.t[0], e.t[1])));
            }
        }
    }
}

/// Runs `split` on every face of `b` (the faces it adds are not visited again); returns, for
/// each face of the result, the face of `b` it came from.
fn each_face(
    b: &mut Body,
    mut split: impl FnMut(&mut Body, usize) -> Result<Vec<usize>>,
) -> Result<Vec<usize>> {
    let n = b.faces.len();
    let mut from: Vec<usize> = (0..n).collect();
    for fi in 0..n {
        let pieces = split(b, fi)?;
        from.resize(b.faces.len(), 0);
        for g in pieces {
            from[g] = fi;
        }
    }
    Ok(from)
}

/// Whether an edge's curve (over the edge) or a face's surface has a C0 break: what
/// [`split_continuity`] removes. Counts (curves, surfaces).
pub fn count_c0(b: &Body) -> (usize, usize) {
    let curves = b
        .edges
        .iter()
        .filter(|e| !e.degenerate)
        .filter(|e| match &e.curve {
            Curve::Nurbs(n) => !c0_knots(n.degree, &n.knots, e.t[0], e.t[1], &|t, r| {
                side_tangent(n, t, r)
            })
            .is_empty(),
            _ => false,
        })
        .count();
    let surfaces = b
        .faces
        .iter()
        .filter(|f| match &f.surf {
            Surface::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.range();
                let mid = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
                !c0_knots(n.du, &n.ku, u0, u1, &|u, r| {
                    n.ders1(u + if r { tol::REL } else { -tol::REL } * (u1 - u0), mid.1)
                        .1
                })
                .is_empty()
                    || !c0_knots(n.dv, &n.kv, v0, v1, &|v, r| {
                        n.ders1(mid.0, v + if r { tol::REL } else { -tol::REL } * (v1 - v0))
                            .2
                    })
                    .is_empty()
            }
            _ => false,
        })
        .count();
    (curves, surfaces)
}

/// The B-spline surfaces of more than one span (OCCT's statshape `BSplineSurface`, which a
/// Bézier patch isn't).
pub fn count_bspline_surfaces(b: &Body) -> usize {
    b.faces
        .iter()
        .filter(|f| match &f.surf {
            Surface::Nurbs(n) => {
                let ((u0, u1), (v0, v1)) = n.range();
                !distinct_knots(&n.ku, u0, u1).is_empty()
                    || !distinct_knots(&n.kv, v0, v1).is_empty()
            }
            _ => false,
        })
        .count()
}

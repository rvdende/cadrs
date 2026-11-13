//! Local operations that keep a body's topology and change its faces' surfaces: drafts (faces
//! turned about a neutral plane), Move face (faces moved rigidly), faces given new surfaces,
//! and the sharp offsets of [`super::offset::sharp`] (faces moved along their normals), which
//! are made with them. Vertices are recomputed as the
//! points where their faces' new surfaces meet, edges as the new surfaces' intersection curves
//! between those points (a seam as the new surface's iso-line), p-curves from the new edges.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge};
use crate::*;

/// The faces around each vertex (distinct).
fn vertex_faces(b: &Body) -> Vec<Vec<usize>> {
    let mut out = vec![Vec::new(); b.verts.len()];
    for (fi, f) in b.faces.iter().enumerate() {
        for c in f.loops.iter().flatten() {
            for v in b.edges[c.edge].v {
                if !out[v].contains(&fi) {
                    out[v].push(fi);
                }
            }
        }
    }
    out
}

/// A point on all the surfaces near `p` (Newton on their signed distances, least squares when
/// more than three meet).
fn meet(surfs: &[&Surface], p: P3) -> Option<P3> {
    let mut x = p;
    for _ in 0..60 {
        let mut jt = nalgebra::DMatrix::<f64>::zeros(surfs.len(), 3);
        let mut r = nalgebra::DVector::<f64>::zeros(surfs.len());
        for (i, s) in surfs.iter().enumerate() {
            let d = super::ssi::sdist(s, &x);
            let e = tol::DIFF_STEP;
            let g = V3::new(
                (super::ssi::sdist(s, &(x + V3::x() * e))
                    - super::ssi::sdist(s, &(x - V3::x() * e)))
                    / (2.0 * e),
                (super::ssi::sdist(s, &(x + V3::y() * e))
                    - super::ssi::sdist(s, &(x - V3::y() * e)))
                    / (2.0 * e),
                (super::ssi::sdist(s, &(x + V3::z() * e))
                    - super::ssi::sdist(s, &(x - V3::z() * e)))
                    / (2.0 * e),
            );
            jt[(i, 0)] = g.x;
            jt[(i, 1)] = g.y;
            jt[(i, 2)] = g.z;
            r[i] = -d;
        }
        // Minimum-norm least-squares step.
        let svd = jt.clone().svd(true, true);
        let dx = svd.solve(&r, tol::ROUNDOFF_FINE).ok()?;
        let step = V3::new(dx[0], dx[1], dx[2]);
        x += step;
        if step.norm() < 1e-14 * (1.0 + x.coords.norm()) {
            break;
        }
    }
    let worst = surfs.iter().map(|s| s.distance(&x)).fold(0.0, f64::max);
    (worst < 1e-7).then_some(x)
}

/// The straight edges whose two faces meet tangentially and still do on their new surfaces,
/// with their new lines. Intersecting tangent surfaces is ill-conditioned, but the line is
/// known: each of its points moved along the faces' common normal onto the new surfaces (a
/// drafted or offset tangent pair moves together).
fn tangent_lines(b: &Body, surfs: &[Surface], ef: &[Vec<usize>]) -> Vec<(usize, Curve)> {
    let normal = |s: &Surface, sense: bool, p: &P3| {
        let (u, v) = s.project(p, None);
        s.normal(u, v) * if sense { 1.0 } else { -1.0 }
    };
    let mut out = Vec::new();
    for (e, ed) in b.edges.iter().enumerate() {
        let fs = &ef[e];
        let Curve::Line { d, .. } = ed.curve else {
            continue;
        };
        if ed.degenerate
            || fs.len() != 2
            || fs[0] == fs[1]
            || fs.iter().all(|&f| surfs[f] == b.faces[f].surf)
        {
            continue;
        }
        let (fa, fb) = (&b.faces[fs[0]], &b.faces[fs[1]]);
        let len = (ed.t[1] - ed.t[0]).abs();
        let ts = [ed.t[0], ed.t[0] + (ed.t[1] - ed.t[0]) * 0.5, ed.t[1]];
        let mut moved = Vec::new();
        for &t in &ts {
            let q = ed.point(t);
            let (na, nb) = (
                normal(&fa.surf, fa.sense, &q),
                normal(&fb.surf, fb.sense, &q),
            );
            if na.dot(&nb) < 1.0 - tol::ANGULAR {
                break;
            }
            // Along the normal onto the first new surface (Newton on the signed distance).
            let mut s = 0.0;
            for _ in 0..30 {
                let f = super::ssi::sdist(&surfs[fs[0]], &(q + na * s));
                let h = tol::DIFF_STEP * (1.0 + len);
                let g = (super::ssi::sdist(&surfs[fs[0]], &(q + na * (s + h)))
                    - super::ssi::sdist(&surfs[fs[0]], &(q + na * (s - h))))
                    / (2.0 * h);
                if g.abs() < tol::DEGENERATE {
                    break;
                }
                let ds = f / g;
                s -= ds;
                if ds.abs() < 1e-15 * (1.0 + len) {
                    break;
                }
            }
            moved.push(q + na * s);
        }
        if moved.len() != 3 {
            continue;
        }
        let Some(dir) = (moved[2] - moved[0]).try_normalize(tol::DEGENERATE) else {
            continue;
        };
        let dir = if dir.dot(&d) < 0.0 { -dir } else { dir };
        let line = Curve::Line {
            o: moved[0],
            d: dir,
        };
        // Still a line on both new surfaces, and still tangent there.
        let ok = moved.iter().all(|p| {
            let off = (p - moved[0]) - dir * (p - moved[0]).dot(&dir);
            off.norm() < tol::REL * (1.0 + len)
                && fs
                    .iter()
                    .all(|&f| surfs[f].distance(p) < tol::REL * (1.0 + len))
                && normal(&surfs[fs[0]], true, p)
                    .cross(&normal(&surfs[fs[1]], true, p))
                    .norm()
                    < 1e-7
        });
        if ok {
            out.push((e, line));
        }
    }
    out
}

/// The point on the curve `c` near `p` where it crosses the surfaces (Gauss-Newton on the
/// curve's parameter).
fn line_meet(c: &Curve, surfs: &[&Surface], p: P3) -> Option<P3> {
    if surfs.is_empty() {
        return None;
    }
    let h = tol::DIFF_STEP * (1.0 + p.coords.norm());
    let newton = |mut t: f64| -> Option<f64> {
        for _ in 0..50 {
            let (mut num, mut den) = (0.0, 0.0);
            for s in surfs {
                let d = super::ssi::sdist(s, &c.point(t));
                let g = (super::ssi::sdist(s, &c.point(t + h))
                    - super::ssi::sdist(s, &c.point(t - h)))
                    / (2.0 * h);
                num += d * g;
                den += g * g;
            }
            if den < 1e-24 {
                return None;
            }
            let dt = num / den;
            t -= dt;
            if dt.abs() < 1e-15 * (1.0 + t.abs()) {
                break;
            }
        }
        let q = c.point(t);
        surfs.iter().all(|s| s.distance(&q) < 1e-7).then_some(t)
    };
    // (A circle's parameter within a turn: wrapped to the far end of a huge range, its
    // angle has lost its digits; a B-spline's within its range: sampled past it, it is
    // nothing like itself.)
    let start = match c.nurbs_range() {
        _ if c.is_periodic() => conic_closest(c, &p, -PI, PI),
        Some((a, b)) => c.closest(&p, a, b),
        None => c.closest(&p, -1e12, 1e12),
    };
    let t = newton(start)?;
    // The crossing nearest the start along the curve: a Newton step can leap over it to a
    // farther one (an intersection loop of a torus crosses a plane twice). The first
    // surface's distance changing sign between the start and the crossing found marks it.
    let f = |t: f64| super::ssi::sdist(surfs[0], &c.point(t));
    const N: usize = 32;
    let mut prev = (start, f(start));
    for i in 1..N {
        let ti = start + (t - start) * i as f64 / N as f64;
        let fi = f(ti);
        if prev.1 * fi < 0.0 {
            let (mut a, mut z) = (prev.0, ti);
            for _ in 0..60 {
                let m = 0.5 * (a + z);
                if f(m) * f(a) <= 0.0 { z = m } else { a = m }
            }
            if let Some(near) = newton(0.5 * (a + z)) {
                return Some(c.point(near));
            }
            break;
        }
        prev = (ti, fi);
    }
    Some(c.point(t))
}

/// On a B-spline surface closed in u or v (not periodic: its p-curves stay in its range), the
/// p-curve `pc` over [t0, t1] moved across the range when its start is a whole range from `at`
/// (the previous coedge's end) and stays in the range moved: a seam's other use.
fn closed_spline_shift(s: &Surface, pc: &Curve, t0: f64, t1: f64, at: [f64; 2]) -> Option<Curve> {
    let Surface::Nurbs(n) = s else { return None };
    let ((u0, u1), (v0, v1)) = n.range();
    let (q, r) = (pc.point(t0), pc.point(t1));
    let (q, r) = ([q.x, q.y], [r.x, r.y]);
    let mut shift = [0.0; 2];
    for (k, (a, b), closed) in [
        (
            0,
            (u0, u1),
            (0..=4).all(|j| {
                (s.eval(u0, v0 + (v1 - v0) * j as f64 / 4.0)
                    - s.eval(u1, v0 + (v1 - v0) * j as f64 / 4.0))
                .norm()
                    < tol::ROUNDOFF
            }),
        ),
        (
            1,
            (v0, v1),
            (0..=4).all(|i| {
                (s.eval(u0 + (u1 - u0) * i as f64 / 4.0, v0)
                    - s.eval(u0 + (u1 - u0) * i as f64 / 4.0, v1))
                .norm()
                    < tol::ROUNDOFF
            }),
        ),
    ] {
        let w = b - a;
        if !closed || w <= 0.0 {
            continue;
        }
        let m = ((at[k] - q[k]) / w).round();
        let tol = tol::REL * w;
        let inside = |x: f64| x >= a - tol && x <= b + tol;
        if m != 0.0 && inside(q[k] + m * w) && inside(r[k] + m * w) {
            shift[k] = m * w;
        }
    }
    (shift != [0.0; 2])
        .then(|| pc.transformed(&Affine::translation(V3::new(shift[0], shift[1], 0.0))))
}

/// The exact iso curve of a B-spline surface: u = `t` (running along v) when `along_v`, else
/// v = `t` (along u). Its poles are the surface's rows blended by the other direction's basis
/// at `t`, in homogeneous coordinates.
pub(super) fn nurbs_iso(n: &super::nurbs::NurbsSurface, t: f64, along_v: bool) -> NurbsCurve {
    let (deg, knots, count) = if along_v {
        (n.du, &n.ku, n.cps.len())
    } else {
        (n.dv, &n.kv, n.cps[0].len())
    };
    let span = super::nurbs::find_span(count - 1, deg, t, knots);
    let basis = &super::nurbs::basis_ders(span, t, deg, 0, knots)[0];
    if along_v {
        let cps = (0..n.cps[0].len())
            .map(|j| (0..=deg).map(|k| n.cps[span - deg + k][j] * basis[k]).sum())
            .collect();
        NurbsCurve {
            degree: n.dv,
            knots: n.kv.clone(),
            cps,
        }
    } else {
        let cps = (0..n.cps.len())
            .map(|i| (0..=deg).map(|k| n.cps[i][span - deg + k] * basis[k]).sum())
            .collect();
        NurbsCurve {
            degree: n.du,
            knots: n.ku.clone(),
            cps,
        }
    }
}

/// The points where a surface collapses: a cone's apex, a sphere's poles.
fn singular_points(s: &Surface) -> Vec<P3> {
    match s {
        Surface::Cone { f, r, a } if a.sin().abs() > tol::ANGULAR_FINE => {
            vec![f.o + f.z * (-r / a.tan())]
        }
        Surface::Sphere { f, r } => vec![f.o + f.z * *r, f.o - f.z * *r],
        _ => vec![],
    }
}

/// A part of a surface's parameters: `((u0, u1), (v0, v1))`.
pub type UvBox = ((f64, f64), (f64, f64));

/// Rebuilds the body's geometry with new surfaces for its faces (same topology).
pub fn replace_surfaces(b: &Body, surfs: &[Surface]) -> Result<Body> {
    replace_surfaces_in(b, surfs, &[])
}

/// A straight edge between two surfaces swept along its direction (extrusions, cylinders and
/// planes along it) stays a line along it: the one through the new end (exact; marching along
/// two parallel sweeps finds nothing to follow when their profiles cross at a point).
fn swept_line(surfs: &[Surface], fs: &[usize], old: &Curve, p0: &P3, p1: &P3) -> Option<Curve> {
    let Curve::Line { d, .. } = old else {
        return None;
    };
    let d = d.try_normalize(tol::DEGENERATE)?;
    let along = |s: &Surface| match s {
        Surface::Extrusion { d: e, .. } => e.normalize().cross(&d).norm() < tol::ANGULAR_FINE,
        Surface::Cylinder { f, .. } => f.z.cross(&d).norm() < tol::ANGULAR_FINE,
        Surface::Plane { f } => f.x.cross(&f.y).dot(&d).abs() < tol::ANGULAR_FINE,
        _ => false,
    };
    if fs.len() != 2
        || fs[0] == fs[1]
        || !fs.iter().all(|&f| along(&surfs[f]))
        || fs
            .iter()
            .all(|&f| matches!(surfs[f], Surface::Plane { .. }))
    {
        return None;
    }
    let line = Curve::Line { o: *p0, d };
    let w = p1 - p0;
    let size = 1.0 + w.norm() + p0.coords.norm();
    let on = (w - d * w.dot(&d)).norm() < tol::REL * size
        && fs
            .iter()
            .all(|&f| [p0, p1].iter().all(|p| surfs[f].distance(p) < 1e-7 * size));
    on.then_some(line)
}

/// A face round a periodic surface whose seam now runs through one of its holes (the moved
/// faces grew the hole across it): the seam moved round to where no hole is. Its ends move
/// along the closed edges they lie on (the circles bounding a cylinder, the other seam of a
/// torus), which are re-ranged to start there; the p-curves are recomputed after. Left as it
/// is when the seam isn't an iso line between closed edges.
fn reseat_seam(out: &mut Body, fi: usize) {
    let s = out.faces[fi].surf.clone();
    if !s.u_periodic() || out.faces[fi].loops.len() < 2 {
        return;
    }
    let ef = out.edge_faces();
    // The seam across u: an edge the face uses twice along which u is constant.
    let wrap = |x: f64| x - (x / TAU).floor() * TAU;
    let u_of = |e: usize| -> Option<f64> {
        let se = &out.edges[e];
        let us: Vec<f64> = (0..=4)
            .map(|i| {
                s.project(
                    &se.point(se.t[0] + (se.t[1] - se.t[0]) * i as f64 / 4.0),
                    None,
                )
                .0
            })
            .collect();
        us.iter()
            .all(|u| {
                let d = wrap(u - us[0]);
                d.min(TAU - d) < 1e-7
            })
            .then_some(us[0])
    };
    let Some((seam, u0)) = out.faces[fi].loops[0]
        .iter()
        .map(|c| c.edge)
        .filter(|&e| {
            !out.edges[e].degenerate && ef[e].len() == 2 && ef[e][0] == fi && ef[e][1] == fi
        })
        .find_map(|e| u_of(e).map(|u| (e, u)))
    else {
        return;
    };
    // Each hole's span in u (unwrapped along it), as offsets past the seam in [0, 2π).
    let mut spans: Vec<(f64, f64)> = Vec::new();
    let mut crossed = false;
    for l in out.faces[fi].loops.iter().skip(1) {
        let mut prev: Option<f64> = None;
        let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
        for c in l {
            let e = &out.edges[c.edge];
            if e.degenerate {
                continue;
            }
            for i in 0..=16 {
                let mut u = s
                    .project(&e.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 16.0), None)
                    .0;
                if let Some(p) = prev {
                    u = p + ((u - p + PI).rem_euclid(TAU) - PI);
                }
                prev = Some(u);
                lo = lo.min(u);
                hi = hi.max(u);
            }
        }
        if hi - lo >= TAU - tol::ANGULAR {
            return;
        }
        let a = wrap(lo - u0);
        if a + (hi - lo) > TAU {
            crossed = true;
        }
        spans.push((a, a + (hi - lo)));
    }
    if !crossed {
        return;
    }
    // The middle of the widest gap between the holes' spans (round the turn).
    let mut ends: Vec<(f64, f64)> = spans.iter().map(|&(a, b)| (a, b)).collect();
    ends.sort_by(|x, y| x.0.total_cmp(&y.0));
    let mut best = (0.0, None);
    for i in 0..ends.len() {
        let end = ends[..=i]
            .iter()
            .map(|x| x.1)
            .fold(f64::NEG_INFINITY, f64::max);
        let next = if i + 1 < ends.len() {
            ends[i + 1].0
        } else {
            ends[0].0 + TAU
        };
        if next - end > best.0 {
            best = (next - end, Some(0.5 * (end + next)));
        }
    }
    let Some(gap) = best.1 else { return };
    let u_new = u0 + gap;
    let Some(iso) = s.iso_u(u_new) else { return };
    // The seam's ends: each on closed edges only, which move with it.
    let ends_v = out.edges[seam].v;
    let mut moved: Vec<(usize, P3)> = Vec::new();
    for v in ends_v {
        if moved.iter().any(|(w, _)| *w == v) {
            continue;
        }
        let others: Vec<usize> = (0..out.edges.len())
            .filter(|&e| e != seam && !out.edges[e].degenerate && out.edges[e].v.contains(&v))
            .collect();
        if others.iter().any(|&e| {
            out.edges[e].v[0] != out.edges[e].v[1]
                || !matches!(out.edges[e].curve, Curve::Circle { .. })
        }) {
            return;
        }
        let (_, vv) = s.project(&out.verts[v].p, None);
        // On the closed edges exactly (the surface's projection is good only to its tolerance).
        let q = others.iter().fold(s.eval(u_new, vv), |q, &e| {
            let ed = &out.edges[e];
            ed.curve
                .point(ed.curve.closest(&q, ed.t[0] - PI, ed.t[1] + PI))
        });
        moved.push((v, q));
    }
    for &(v, p) in &moved {
        out.verts[v].p = p;
        for e in 0..out.edges.len() {
            let ed = &out.edges[e];
            if e == seam || ed.degenerate || ed.v != [v, v] {
                continue;
            }
            let span = ed.t[1] - ed.t[0];
            let t0 = ed.curve.closest(&p, ed.t[0] - TAU, ed.t[1] + TAU);
            out.edges[e].t = [t0, t0 + span];
        }
    }
    // The seam: the iso line between its (moved) ends, in its old direction.
    let ed = &out.edges[seam];
    let (p0, p1) = (out.verts[ed.v[0]].p, out.verts[ed.v[1]].p);
    let old_dir = ed.curve.tangent(ed.mid_t());
    let (t0, t1) = if ed.v[0] == ed.v[1] {
        let t = iso.closest(&p0, -PI, 3.0 * PI);
        match iso {
            Curve::Circle { .. } => (t, t + TAU),
            _ => return,
        }
    } else {
        let (a, z) = if matches!(iso, Curve::Circle { .. }) {
            (-PI, PI)
        } else {
            (-1e12, 1e12)
        };
        (iso.closest(&p0, a, z), iso.closest(&p1, a, z))
    };
    let (iso, t0, t1) =
        if t1 < t0 || iso.tangent(0.5 * (t0 + t1)).dot(&old_dir) < 0.0 && ed.v[0] == ed.v[1] {
            let (c, a, b) = iso.reversed(t0, t1);
            (c, a.min(b), a.max(b))
        } else {
            (iso, t0, t1)
        };
    out.edges[seam].curve = iso;
    out.edges[seam].t = [t0, t1];
    // A pole's point edge spans the turn from the seam: it turns with it.
    let du = u_new - u0;
    for l in out.faces[fi].loops.iter_mut() {
        for c in l.iter_mut() {
            if out.edges[c.edge].degenerate
                && out.edges[c.edge].v.iter().any(|v| ends_v.contains(v))
            {
                c.pc =
                    c.pc.transformed(&Affine::translation(V3::new(du, 0.0, 0.0)));
            }
        }
    }
}

/// Seams of kept periodic faces that the moved faces' corners leave behind (where a seam
/// lies is arbitrary: it moves to the surface's iso line through its new end), for a seam (an
/// iso-u line, or a torus's iso-v line) of an unchanged face:
/// - an end the moved faces carried off the seam's line (their corner no longer on it): the
///   seam moves through it, its other end a pole or, when it lies only on closed circles (a
///   cone's rim), slid round them onto the new line;
/// - an end on a kept circle between two of its arcs that a moved face's corner slides past
///   along the circle (a drafted wall's foot crossing a sphere's or torus's seam): the end
///   moves along the circle to the middle of the arc its two edges now span, and the seam
///   with it, when its other end is the same vertex or a pole.
///
/// A pole's point edge on the face turns with its seam. Returns the seams moved, whose curves
/// are recomputed.
fn slide_seam_ends(b: &Body, out: &mut Body, surfs: &[Surface], ef: &[Vec<usize>]) -> Vec<usize> {
    let kept = |e: usize| ef[e].iter().all(|&f| surfs[f] == b.faces[f].surf);
    let mut seams = Vec::new();
    for s in 0..b.edges.len() {
        let se = &b.edges[s];
        if se.degenerate || ef[s].len() != 2 || ef[s][0] != ef[s][1] || !kept(s) {
            continue;
        }
        let f = ef[s][0];
        let surf = &b.faces[f].surf;
        let size = 1.0 + se.point(se.t[0]).coords.norm();
        let poles = singular_points(surf);
        let at_pole = |p: &P3| poles.iter().any(|q| (q - p).norm() < tol::REL * size);
        // An iso-u line round u, or an iso-v line round v (a torus's other seam).
        let uv: Vec<(f64, f64)> = (0..=4)
            .map(|i| {
                surf.project(
                    &se.point(se.t[0] + (se.t[1] - se.t[0]) * i as f64 / 4.0),
                    None,
                )
            })
            .collect();
        let flat = |x: &dyn Fn(&(f64, f64)) -> f64| {
            uv.iter().all(|q| {
                let d = (x(q) - x(&uv[0])).rem_euclid(TAU);
                d.min(TAU - d) <= 1e-7
            })
        };
        let along_u = surf.u_periodic() && flat(&|q| q.0);
        if !along_u && !(surf.v_periodic() && flat(&|q| q.1)) {
            continue;
        }
        let iso_at = |x: f64| {
            if along_u {
                surf.iso_u(x)
            } else {
                surf.iso_v(x)
            }
        };
        let coord = |p: &P3| {
            let (u, v) = surf.project(p, None);
            if along_u { u } else { v }
        };
        let x0 = if along_u { uv[0].0 } else { uv[0].1 };
        // An end the moved faces carried off the seam (their corner no longer on its iso
        // line): the seam moves to the iso line through it, and its other end, when it lies
        // only on closed circles (a cylinder's or cone's rim), slides round them onto it.
        let near_on = |c: &Curve, p: &P3| {
            c.point(if c.is_periodic() {
                c.closest(p, -PI, PI)
            } else {
                c.closest(p, -1e12, 1e12)
            })
        };
        let iso_dist = |p: &P3| iso_at(x0).map_or(0.0, |c| (near_on(&c, p) - p).norm());
        let off =
            (0..2).find(|&k| se.v[0] != se.v[1] && iso_dist(&out.verts[se.v[k]].p) > 1e-7 * size);
        if let Some(k) = off {
            let (v, o) = (se.v[k], se.v[1 - k]);
            let x_new = coord(&out.verts[v].p);
            let rims: Vec<usize> = (0..b.edges.len())
                .filter(|&e| e != s && !b.edges[e].degenerate && b.edges[e].v.contains(&o))
                .collect();
            let closed = !rims.is_empty()
                && rims.iter().all(|&e| {
                    b.edges[e].v == [o, o]
                        && matches!(b.edges[e].curve, Curve::Circle { .. })
                        && kept(e)
                });
            if at_pole(&b.verts[o].p) {
                seams.push(s);
            } else if closed && let Some(iso) = iso_at(x_new) {
                let q = rims.iter().fold(near_on(&iso, &out.verts[o].p), |q, &e| {
                    near_on(&b.edges[e].curve, &q)
                });
                if (near_on(&iso, &q) - q).norm() < 1e-7 * size {
                    out.verts[o].p = q;
                    seams.push(s);
                }
            }
            continue;
        }
        for k in 0..2 {
            let v = se.v[k];
            let o = se.v[1 - k];
            if at_pole(&b.verts[v].p) || !(o == v || at_pole(&b.verts[o].p)) {
                continue;
            }
            let others: Vec<usize> = (0..b.edges.len())
                .filter(|&e| e != s && !b.edges[e].degenerate && b.edges[e].v.contains(&v))
                .collect();
            let [x, y] = others[..] else { continue };
            let (a, z) = if b.edges[x].v[1] == v && b.edges[y].v[0] == v {
                (x, y)
            } else if b.edges[y].v[1] == v && b.edges[x].v[0] == v {
                (y, x)
            } else {
                continue;
            };
            let (ea, ez) = (&b.edges[a], &b.edges[z]);
            if ea.v[0] == v
                || ez.v[1] == v
                || ea.curve != ez.curve
                || !matches!(ea.curve, Curve::Circle { .. })
                || !kept(a)
                || !kept(z)
            {
                continue;
            }
            // The arc the two now span, from a's start round to z's end.
            let c = &ea.curve;
            let t0 = unwrap_near(c.closest(&out.verts[ea.v[0]].p, -PI, PI), ea.t[0], TAU);
            let t1 = t0 + (c.closest(&out.verts[ez.v[1]].p, -PI, PI) - t0).rem_euclid(TAU);
            let rel = (ea.t[1] - t0).rem_euclid(TAU);
            if rel > tol::ROUNDOFF && rel < t1 - t0 - tol::ROUNDOFF {
                continue;
            }
            let q = c.point(0.5 * (t0 + t1));
            let dx = coord(&q) - coord(&b.verts[v].p);
            let shift = if along_u {
                V3::new(dx, 0.0, 0.0)
            } else {
                V3::new(0.0, dx, 0.0)
            };
            out.verts[v].p = q;
            seams.push(s);
            for l in out.faces[f].loops.iter_mut() {
                for co in l.iter_mut() {
                    if b.edges[co.edge].degenerate && b.edges[co.edge].v.contains(&o) && o != v {
                        co.pc = co.pc.transformed(&Affine::translation(shift));
                    }
                }
            }
            break;
        }
    }
    seams
}

/// A hole the moved faces grew over a pole of its face (a cone's foot on a sphere widened past
/// the sphere's pole): the pole is no longer on the face. The hole's closed edge is cut where
/// the seam's line crosses it (a new vertex), the seam ends there instead of at the pole, and
/// the hole joins the outer loop in the pole's place (the pole's point edge left unused).
/// Only a hole of one closed edge and a seam to the pole. Returns the edges added, each with
/// the edge it was cut from.
fn swallow_poles(out: &mut Body) -> Vec<(usize, usize)> {
    let mut cut = Vec::new();
    for fi in 0..out.faces.len() {
        let mut li = 1;
        while li < out.faces[fi].loops.len() {
            match swallow_pole(out, fi, li) {
                Some(x) => cut.push(x),
                None => li += 1,
            }
        }
    }
    cut
}

/// [`swallow_poles`] for hole `li` of face `fi`: the edge added and the one it was cut from.
fn swallow_pole(out: &mut Body, fi: usize, li: usize) -> Option<(usize, usize)> {
    let surf = out.faces[fi].surf.clone();
    let poles = singular_points(&surf);
    let [hc] = &out.faces[fi].loops[li][..] else {
        return None;
    };
    let h = hc.edge;
    let he = out.edges[h].clone();
    if poles.is_empty() || !surf.u_periodic() || he.degenerate || he.v[0] != he.v[1] {
        return None;
    }
    // The turn the hole makes round the surface's axis, and its middle.
    const N: usize = 64;
    let pts: Vec<P3> = (0..=N)
        .map(|i| he.point(he.t[0] + (he.t[1] - he.t[0]) * i as f64 / N as f64))
        .collect();
    let us: Vec<f64> = pts.iter().map(|p| surf.project(p, None).0).collect();
    let turn: f64 = us
        .windows(2)
        .map(|w| (w[1] - w[0] + PI).rem_euclid(TAU) - PI)
        .sum();
    if turn.abs() < PI {
        return None;
    }
    let mid = P3::from(pts.iter().fold(V3::zeros(), |a, p| a + p.coords) / pts.len() as f64);
    let pole = poles
        .into_iter()
        .min_by(|a, c| (a - mid).norm().total_cmp(&(c - mid).norm()))?;
    let size = 1.0 + pole.coords.norm() + (pts[0] - pole).norm();
    // The outer loop's point edge at that pole, between the two uses of the seam.
    let outer = &out.faces[fi].loops[0];
    let n = outer.len();
    let kd = outer.iter().position(|c| {
        out.edges[c.edge].degenerate
            && (out.verts[out.edges[c.edge].v[0]].p - pole).norm() < 1e-7 * size
    })?;
    let pv = out.edges[outer[kd].edge].v[0];
    let s = outer[(kd + n - 1) % n].edge;
    if s != outer[(kd + 1) % n].edge || out.edges[s].degenerate || !out.edges[s].v.contains(&pv) {
        return None;
    }
    let se = out.edges[s].clone();
    // Where the seam's line crosses the hole's edge: the hole's point at the seam's u.
    let (us_seam, _) = surf.project(&se.point(se.mid_t()), None);
    let off = |t: f64| (surf.project(&he.point(t), None).0 - us_seam + PI).rem_euclid(TAU) - PI;
    let ts: Vec<f64> = (0..=4 * N)
        .map(|i| he.t[0] + (he.t[1] - he.t[0]) * i as f64 / (4 * N) as f64)
        .collect();
    let k = ts.windows(2).position(|w| {
        let (a, z) = (off(w[0]), off(w[1]));
        a * z <= 0.0 && (a - z).abs() < PI
    })?;
    let (mut a, mut z) = (ts[k], ts[k + 1]);
    for _ in 0..80 {
        let m = 0.5 * (a + z);
        if off(a) * off(m) <= 0.0 { z = m } else { a = m }
    }
    let th = 0.5 * (a + z);
    let q = he.point(th);
    let tq = match se.curve {
        Curve::Circle { .. } => unwrap_near(
            se.curve.closest(&q, -PI, PI),
            se.t[if se.v[0] == pv { 0 } else { 1 }],
            TAU,
        ),
        _ => se.curve.closest(&q, -1e12, 1e12),
    };
    if (se.curve.point(tq) - q).norm() > 1e-7 * size || th <= he.t[0] || th >= he.t[1] {
        return None;
    }
    // The new vertex, the hole's edge cut there.
    let nv = out.verts.len();
    out.verts.push(super::topo::Vertex {
        p: q,
        tol: out.verts[he.v[0]].tol,
    });
    let hv = he.v[0];
    out.edges[h].t = [he.t[0], th];
    out.edges[h].v = [hv, nv];
    let h2 = out.edges.len();
    out.edges.push(super::topo::Edge {
        t: [th, he.t[1]],
        v: [nv, hv],
        ..he.clone()
    });
    for g in out.faces.iter_mut() {
        for l in g.loops.iter_mut() {
            let mut nl = Vec::with_capacity(l.len() + 1);
            for c in l.iter() {
                if c.edge != h {
                    nl.push(c.clone());
                } else if c.fwd {
                    nl.push(c.clone());
                    nl.push(Coedge {
                        edge: h2,
                        ..c.clone()
                    });
                } else {
                    nl.push(Coedge {
                        edge: h2,
                        ..c.clone()
                    });
                    nl.push(c.clone());
                }
            }
            *l = nl;
        }
    }
    // The seam from the new vertex.
    let k_end = if se.v[0] == pv { 0 } else { 1 };
    out.edges[s].v[k_end] = nv;
    out.edges[s].t[k_end] = tq;
    if out.edges[s].t[0] >= out.edges[s].t[1] {
        return None;
    }
    // The hole, from the new vertex round to it, in the pole's place.
    let hole = out.faces[fi].loops.remove(li);
    let start = hole
        .iter()
        .position(|c| out.edges[c.edge].v[if c.fwd { 0 } else { 1 }] == nv)?;
    let ring: Vec<Coedge> = hole[start..]
        .iter()
        .chain(hole[..start].iter())
        .cloned()
        .collect();
    out.faces[fi].loops[0].splice(kd..=kd, ring);
    Some((h2, h))
}

/// Whether `p` projects inside the part `dom` of the surface (an angle modulo a turn).
fn in_domain(s: &Surface, dom: &UvBox, p: &P3) -> bool {
    let (u, v) = s.project(p, None);
    let inside = |x: f64, (a, b): (f64, f64), periodic: bool| {
        let tol = tol::REL * (1.0 + (b - a).abs() + a.abs());
        let x = if periodic {
            a + (x - a).rem_euclid(TAU)
        } else {
            x
        };
        x >= a - tol && x <= b + tol || (periodic && x - TAU >= a - tol && x - TAU <= b + tol)
    };
    inside(u, dom.0, s.u_periodic()) && inside(v, dom.1, s.v_periodic())
}

/// [`replace_surfaces`], with the part of each face's new surface the face lies on (by face;
/// missing or None: anywhere). Where a new surface meets a neighbour more than once, each edge
/// is the crossing inside it.
/// The B-spline surfaces' corners that lie on every one of `fs` (within 1e-9 of the size), the
/// one nearest `old`: where a moved patch's corner is the corner of the solid (a patch laid on a
/// block, its edges on the block's sides) no intersection need be searched for.
fn patch_corner(fs: &[&Surface], old: P3) -> Option<P3> {
    let mut best: Option<(f64, P3)> = None;
    for (i, s) in fs.iter().enumerate() {
        let Surface::Nurbs(n) = s else { continue };
        let ((u0, u1), (v0, v1)) = n.range();
        for (u, v) in [(u0, v0), (u1, v0), (u0, v1), (u1, v1)] {
            let p = n.point(u, v);
            let size = 1.0 + p.coords.norm();
            let on = fs.iter().enumerate().all(|(j, o)| {
                j == i || matches!(o, Surface::Plane { .. }) && o.distance(&p) < 1e-9 * size
            });
            let d = (p - old).norm();
            if on && best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                best = Some((d, p));
            }
        }
    }
    best.map(|(_, p)| p)
}

/// The boundary curve of a B-spline surface between `p0` and `p1` whose poles all lie in a
/// plane, where the edge is between the two: their exact intersection along it, with no
/// tracing (a patch whose edges were drawn in a block's sides).
fn patch_boundary_in_plane(a: &Surface, b: &Surface, p0: &P3, p1: &P3) -> Option<Curve> {
    let (n, plane) = match (a, b) {
        (Surface::Nurbs(n), Surface::Plane { f }) | (Surface::Plane { f }, Surface::Nurbs(n)) => {
            (n, f)
        }
        _ => return None,
    };
    let ((u0, u1), (v0, v1)) = n.range();
    let size = 1.0 + p0.coords.norm() + p1.coords.norm();
    let near = |p: &P3, q: &P3| (p - q).norm() < tol::POINT;
    for (t, along_v) in [(u0, true), (u1, true), (v0, false), (v1, false)] {
        let iso = nurbs_iso(n, t, along_v);
        let flat = iso.cps.iter().all(|h| {
            h.w > 0.0 && (super::nurbs::dehom(h) - plane.o).dot(&plane.z).abs() < 1e-9 * size
        });
        if !flat {
            continue;
        }
        let (c0, c1) = iso.range();
        let curve = Curve::Nurbs(std::sync::Arc::new(iso));
        let (e0, e1) = (curve.point(c0), curve.point(c1));
        let ends = (near(&e0, p0) || near(&e0, p1)) && (near(&e1, p0) || near(&e1, p1));
        let inner = || {
            [p0, p1].iter().all(|p| {
                let t = curve.closest(p, c0, c1);
                near(&curve.point(t), p)
            })
        };
        if ends || inner() {
            return Some(curve);
        }
    }
    None
}

pub fn replace_surfaces_in(b: &Body, surfs: &[Surface], domains: &[Option<UvBox>]) -> Result<Body> {
    replace_surfaces_checked(b, surfs, domains, true)
}

/// [`replace_surfaces_in`], its result checked with the solids' orientation (`volumes`) or
/// without: a caller that settles the new faces' senses itself and checks the volume after
/// (the orientation test casts rays at every face, which on a heavy spline costs more than the
/// rest of the operation).
fn replace_surfaces_checked(
    b: &Body,
    surfs: &[Surface],
    domains: &[Option<UvBox>],
    volumes: bool,
) -> Result<Body> {
    let dom = |f: usize| domains.get(f).copied().flatten();
    let mut prof = super::boolean::Prof::new();
    let mut out = b.clone();
    let vf = vertex_faces(b);
    let ef = b.edge_faces();
    let tangent = tangent_lines(b, surfs, &ef);
    // Vertices.
    for v in 0..b.verts.len() {
        let fs: Vec<&Surface> = vf[v].iter().map(|&f| &surfs[f]).collect();
        if fs.is_empty() {
            continue;
        }
        let moved = vf[v].iter().any(|&f| surfs[f] != b.faces[f].surf);
        if !moved {
            continue;
        }
        // On a line where two faces stay tangent: where it crosses the other faces (the
        // tangent pair alone pins nothing along the line).
        let on_line = tangent
            .iter()
            .find(|(e, _)| b.edges[*e].v.contains(&v))
            .and_then(|(e, c)| {
                let others: Vec<&Surface> = vf[v]
                    .iter()
                    .filter(|f| !ef[*e].contains(f))
                    .map(|&f| &surfs[f])
                    .collect();
                let p = line_meet(c, &others, b.verts[v].p)?;
                fs.iter().all(|s| s.distance(&p) < 1e-7).then_some(p)
            });
        // On a seam: on the new surface's seam line (the faces around pin it only to their
        // intersection curve).
        let on_seam = || {
            let e = (0..b.edges.len()).find(|&e| {
                b.edges[e].v.contains(&v)
                    && !b.edges[e].degenerate
                    && ef[e].len() == 2
                    && ef[e][0] == ef[e][1]
            })?;
            let f = ef[e][0];
            let (u, _) = b.faces[f].surf.project(&b.verts[v].p, None);
            let iso = surfs[f].iso_u(u)?;
            let others: Vec<&Surface> = vf[v]
                .iter()
                .filter(|&&g| g != f)
                .map(|&g| &surfs[g])
                .collect();
            let p = line_meet(&iso, &others, b.verts[v].p)?;
            fs.iter().all(|s| s.distance(&p) < 1e-7).then_some(p)
        };
        // On an edge whose faces all stay: on that edge's curve, where it crosses the moved
        // faces.
        let on_kept = || {
            let e = (0..b.edges.len()).find(|&e| {
                b.edges[e].v.contains(&v)
                    && !b.edges[e].degenerate
                    && ef[e].iter().all(|&f| surfs[f] == b.faces[f].surf)
            })?;
            let others: Vec<&Surface> = vf[v]
                .iter()
                .filter(|&&g| surfs[g] != b.faces[g].surf)
                .map(|&g| &surfs[g])
                .collect();
            let p = line_meet(&b.edges[e].curve, &others, b.verts[v].p)?;
            fs.iter().all(|s| s.distance(&p) < 1e-7).then_some(p)
        };
        // At a pole or apex (the end of a point edge): the new surface's own, nearest.
        let on_pole = || {
            let f = (0..b.edges.len())
                .filter(|&e| b.edges[e].degenerate && b.edges[e].v.contains(&v))
                .flat_map(|e| ef[e].iter().copied())
                .next()?;
            let old = b.verts[v].p;
            let p = singular_points(&surfs[f])
                .into_iter()
                .min_by(|x, y| (x - old).norm().total_cmp(&(y - old).norm()))?;
            fs.iter().all(|s| s.distance(&p) < 1e-7).then_some(p)
        };
        let p = on_pole()
            .or(on_line)
            .or_else(on_seam)
            .or_else(|| patch_corner(&fs, b.verts[v].p))
            .or_else(on_kept)
            .or_else(|| meet(&fs, b.verts[v].p))
            .ok_or_else(|| KernelError::InvalidGeometry {
                op: "local operation",
                why: "the moved faces no longer meet at a corner".into(),
            })?;
        out.verts[v].p = p;
    }
    let seams = slide_seam_ends(b, &mut out, surfs, &ef);
    prof.mark("vertices");
    // Edges.
    for e in 0..b.edges.len() {
        let ed = &b.edges[e];
        let fs = &ef[e];
        let moved = fs.iter().any(|&f| surfs[f] != b.faces[f].surf) || seams.contains(&e);
        let (p0, p1) = (out.verts[ed.v[0]].p, out.verts[ed.v[1]].p);
        if !moved {
            // Same curve; the ends may have moved along it.
            if ed.degenerate
                || ((p0 - b.verts[ed.v[0]].p).norm() <= tol::DEGENERATE
                    && (p1 - b.verts[ed.v[1]].p).norm() <= tol::DEGENERATE)
            {
                continue;
            }
            if let Some(t) = slide_range(ed, &p0, &p1) {
                out.edges[e].t = t;
                continue;
            }
            // An end slid off the curve (past a bounded B-spline's end, or off a seam the
            // moved faces no longer reach): between two faces, recomputed below as their
            // intersection (a seam as the iso line through its end); else as near as the
            // curve gets.
            if fs.len() != 2 {
                let q = ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * 0.25);
                let dir0 = ed
                    .curve
                    .tangent(ed.t[0])
                    .try_normalize(tol::DEGENERATE)
                    .unwrap_or_else(V3::zeros);
                let (t0, t1) = param_range(&ed.curve, &p0, &p1, ed.v[0] == ed.v[1], &q, &dir0);
                out.edges[e].t = if t0 <= t1 { [t0, t1] } else { [t1, t0] };
                continue;
            }
        }
        if ed.degenerate {
            out.edges[e].curve = Curve::Line {
                o: p0,
                d: V3::zeros(),
            };
            continue;
        }
        let old_mid = ed.point(ed.mid_t());
        let curve = if let Some((_, c)) = tangent.iter().find(|(x, _)| *x == e) {
            c.clone()
        } else if let Some(c) = swept_line(surfs, fs, &ed.curve, &p0, &p1) {
            c
        } else if let Some(c) = (fs.len() == 2)
            .then(|| patch_boundary_in_plane(&surfs[fs[0]], &surfs[fs[1]], &p0, &p1))
            .flatten()
        {
            c
        } else if fs.len() == 2 && fs[0] == fs[1] {
            // A seam: the new surface's iso-u line through the vertex (on a closed B-spline,
            // the iso line along the old seam's constant parameter, at the side it lies on).
            let s = &surfs[fs[0]];
            match s {
                Surface::Nurbs(n) => {
                    let old = &b.faces[fs[0]].surf;
                    let (ua, va) = old.project(&ed.point(ed.t[0]), None);
                    let (ub, vb) = old.project(&ed.point(ed.mid_t()), None);
                    let along_v = (ua - ub).abs() <= (va - vb).abs();
                    let (u, v) = s.project(&p0, None);
                    let ((u0, u1), (v0, v1)) = n.range();
                    let snap = |x: f64, a: f64, b: f64| {
                        if (x - a).abs() < tol::REL_POINT * (b - a) {
                            a
                        } else if (x - b).abs() < tol::REL_POINT * (b - a) {
                            b
                        } else {
                            x
                        }
                    };
                    let iso = if along_v {
                        nurbs_iso(n, snap(u, u0, u1), true)
                    } else {
                        nurbs_iso(n, snap(v, v0, v1), false)
                    };
                    Curve::Nurbs(std::sync::Arc::new(iso))
                }
                _ => {
                    // (Through the end that isn't a pole: every iso line passes a pole. An
                    // iso-v line where the old seam ran round u: a torus's other seam.)
                    let size = 1.0 + p0.coords.norm();
                    let p = if singular_points(s)
                        .iter()
                        .any(|q| (q - p0).norm() < tol::REL * size)
                    {
                        p1
                    } else {
                        p0
                    };
                    let old = &b.faces[fs[0]].surf;
                    let (ua, va) = old.project(&ed.point(ed.t[0]), None);
                    let (ub, vb) = old.project(&ed.point(ed.mid_t()), None);
                    let turn = |d: f64| {
                        let d = d.rem_euclid(TAU);
                        d.min(TAU - d)
                    };
                    let (u, v) = s.project(&p, None);
                    let iso = if s.v_periodic() && turn(ua - ub) > turn(va - vb) {
                        s.iso_v(v)
                    } else {
                        s.iso_u(u)
                    };
                    iso.ok_or(KernelError::Unsupported("moving a seam of this surface"))?
                }
            }
        } else if fs.len() == 2 {
            let (sa, sb) = (&surfs[fs[0]], &surfs[fs[1]]);
            let mut bb = Bbox::empty();
            bb.add(&p0);
            bb.add(&p1);
            bb.add(&old_mid);
            for i in 0..=8 {
                bb.add(&ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * i as f64 / 8.0));
            }
            let bb = bb.grow(bb.diagonal() * 0.5 + 1.0);
            // Each surface's parameters over the search box (its corners projected), so an
            // unbounded direction is sampled only where the edge can be.
            let box_of = |s: &Surface| {
                let ((u0, u1), (v0, v1)) = s.param_box();
                let (mut lu, mut lv) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
                // A periodic direction's angles unwrapped about the first corner's.
                let (mut au, mut av) = ((f64::MAX, f64::MIN), (f64::MAX, f64::MIN));
                let mut first: Option<(f64, f64)> = None;
                for i in 0..8 {
                    let c = P3::new(
                        if i & 1 == 0 { bb.min.x } else { bb.max.x },
                        if i & 2 == 0 { bb.min.y } else { bb.max.y },
                        if i & 4 == 0 { bb.min.z } else { bb.max.z },
                    );
                    let (u, v) = s.project(&c, None);
                    lu = (lu.0.min(u), lu.1.max(u));
                    lv = (lv.0.min(v), lv.1.max(v));
                    let f = *first.get_or_insert((u, v));
                    let (wu, wv) = (unwrap_near(u, f.0, TAU), unwrap_near(v, f.1, TAU));
                    au = (au.0.min(wu), au.1.max(wu));
                    av = (av.0.min(wv), av.1.max(wv));
                }
                let pad = |(a, b): (f64, f64)| ((a - (b - a) * 0.1), (b + (b - a) * 0.1));
                let (lu, lv) = (pad(lu), pad(lv));
                // Round a periodic direction, the part the box sees when it sees under a
                // quarter turn (a huge drafted cylinder's edge spans a sliver of it, which a
                // grid over the whole turn steps over), else all of it.
                let turn = |(a, b): (f64, f64), whole: (f64, f64)| {
                    if b - a < 0.5 * PI {
                        (a - 0.25 * (b - a) - 1e-3, b + 0.25 * (b - a) + 1e-3)
                    } else {
                        whole
                    }
                };
                let u = if s.u_periodic() {
                    turn(au, (u0, u1))
                } else if u1 - u0 < 1e3 {
                    (u0, u1)
                } else {
                    (u0.max(lu.0), u1.min(lu.1))
                };
                let v = if s.v_periodic() {
                    turn(av, (v0, v1))
                } else if v1 - v0 < 1e3 {
                    (v0, v1)
                } else {
                    (v0.max(lv.0), v1.min(lv.1))
                };
                (u, v)
            };
            let cs = super::ssi::surface_surface(sa, box_of(sa), sb, box_of(sb), &bb);
            // The component through both new end points (nearest the old middle).
            let mut best: Option<(f64, Curve)> = None;
            for ic in cs {
                let t0 = ic.c.closest(&p0, ic.t0, ic.t1);
                let t1 = ic.c.closest(&p1, ic.t0, ic.t1);
                let d = (ic.c.point(t0) - p0).norm() + (ic.c.point(t1) - p1).norm();
                if best.as_ref().is_none_or(|(bd, _)| d < *bd) {
                    best = Some((d, ic.c));
                }
            }
            if std::env::var_os("CADRS_DRAFT_DEBUG").is_some() {
                eprintln!(
                    "edge {e} faces {fs:?} ({:?} × {:?}): {:?}",
                    sa.kind(),
                    sb.kind(),
                    best.as_ref().map(|b| (
                        b.0,
                        format!("{:?}", b.1).chars().take(12).collect::<String>()
                    ))
                );
            }
            match best {
                Some((d, c)) if d <= tol::POINT => c,
                // None through both ends (the general intersection's sampling stepped over a
                // sliver of it: a huge drafted cylinder's arc): traced between them.
                _ => trace_between(sa, sb, ed, (b.verts[ed.v[0]].p, b.verts[ed.v[1]].p), p0, p1)
                    .ok_or_else(|| KernelError::InvalidGeometry {
                        op: "local operation",
                        why: "moved faces no longer meet along an edge".into(),
                    })?,
            }
        } else {
            return Err(KernelError::Unsupported(
                "moving faces of a non-manifold edge",
            ));
        };
        // Parameters at the new ends, in the old edge's direction.
        let quarter = ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * 0.25);
        // The way the old edge left its start (its direction survives the move better than
        // any of its points).
        let dir0 = ed
            .curve
            .tangent(ed.t[0])
            .try_normalize(tol::DEGENERATE)
            .unwrap_or_else(V3::zeros);
        let (curve, (t0, t1)) = match closed_arc(&curve, &p0, &p1, ed.v[0] == ed.v[1], &dir0) {
            Some((c, t0, t1)) => (c, (t0, t1)),
            None => {
                let r = match &curve {
                    Curve::Nurbs(_) => nurbs_arc(&curve, &p0, &p1, &dir0),
                    _ => None,
                };
                let r = r.unwrap_or_else(|| {
                    param_range(&curve, &p0, &p1, ed.v[0] == ed.v[1], &quarter, &dir0)
                });
                (curve, r)
            }
        };
        // The arc must lie in its faces' domains: else the other way round (a circle crossing
        // a plane twice runs both ways between the crossings).
        let inside = |c: &Curve, t0: f64, t1: f64| {
            fs.iter().all(|&f| {
                dom(f).is_none_or(|d| {
                    [0.25, 0.5, 0.75]
                        .iter()
                        .all(|s| in_domain(&surfs[f], &d, &c.point(t0 + (t1 - t0) * s)))
                })
            })
        };
        let (t0, t1) = match &curve {
            Curve::Circle { .. } | Curve::Ellipse { .. }
                if ed.v[0] != ed.v[1] && !inside(&curve, t0, t1) =>
            {
                let other = if t1 > t0 {
                    (t0, t1 - TAU)
                } else {
                    (t0, t1 + TAU)
                };
                if inside(&curve, other.0, other.1) {
                    other
                } else {
                    (t0, t1)
                }
            }
            Curve::Nurbs(_) if !inside(&curve, t0, t1) => nurbs_arc(&curve, &p0, &p1, &-dir0)
                .filter(|(a, z)| inside(&curve, *a, *z))
                .unwrap_or((t0, t1)),
            _ => (t0, t1),
        };
        let (curve, t0, t1) = if t1 < t0 {
            let (c, a, b2) = curve.reversed(t0, t1);
            (c, b2.min(a), a.max(b2))
        } else {
            (curve, t0, t1)
        };
        let (t0, t1) = if t1 < t0 { (t1, t0) } else { (t0, t1) };
        out.edges[e].curve = curve;
        out.edges[e].t = [t0, t1];
    }
    prof.mark("edges");
    for (fi, s) in surfs.iter().enumerate() {
        out.faces[fi].surf = s.clone();
    }
    for fi in 0..out.faces.len() {
        reseat_seam(&mut out, fi);
    }
    let cut = swallow_poles(&mut out);
    // p-curves, continuous along each loop.
    for fi in 0..out.faces.len() {
        let surf = out.faces[fi].surf.clone();
        for li in 0..out.faces[fi].loops.len() {
            let lp = out.faces[fi].loops[li].clone();
            let mut at: Option<P2> = None;
            let mut nl: Vec<Coedge> = Vec::new();
            // Seams on periodic faces: the second use of an edge goes a period on.
            for c in &lp {
                let e = &out.edges[c.edge];
                // (An edge `swallow_poles` cut off another is part of that one.)
                let old = &b.edges[cut.iter().find(|x| x.0 == c.edge).map_or(c.edge, |x| x.1)];
                // Unchanged on an unchanged surface keeps its p-curve (a pole's spans the
                // surface's u, which no projection recovers).
                // (Likewise an unchanged curve trimmed shorter: a p-curve is a function of the
                // curve's parameter, and projecting can land on another of the surface's
                // parameters for a point, on a torus's axis.)
                let within = e.t == old.t || (e.t[0] >= old.t[0] && e.t[1] <= old.t[1]);
                let same = surf == b.faces[fi].surf
                    && (old.degenerate || (within && e.curve == old.curve));
                let mut pc = if same {
                    c.pc.clone()
                } else {
                    super::pcurve::pcurve(&surf, &e.curve, e.t[0], e.t[1], None)
                };
                let (t0, t1) = super::topo::co_range(e, c.fwd);
                if let Some(a) = at {
                    let q = pc.point(t0);
                    let du = if surf.u_periodic() {
                        ((a.x - q.x) / TAU).round() * TAU
                    } else {
                        0.0
                    };
                    let dv = if surf.v_periodic() {
                        ((a.y - q.y) / TAU).round() * TAU
                    } else {
                        0.0
                    };
                    if du != 0.0 || dv != 0.0 {
                        pc = pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                    }
                    if let Some(shifted) = closed_spline_shift(&surf, &pc, t0, t1, [a.x, a.y]) {
                        pc = shifted;
                    }
                }
                let (_, t1) = super::topo::co_range(e, c.fwd);
                let q = pc.point(t1);
                at = Some(P2::new(q.x, q.y));
                nl.push(Coedge {
                    edge: c.edge,
                    fwd: c.fwd,
                    pc,
                });
            }
            // On a periodic surface changed in place (its frame kept: an offset, not a turn), each
            // p-curve a whole number of periods from where it was, so the loop keeps the old
            // one's layout (projections land a seam's two uses on the same side of it).
            let in_place = surf != b.faces[fi].surf
                && std::mem::discriminant(&surf) == std::mem::discriminant(&b.faces[fi].surf)
                && surf.frame().is_some()
                && surf.frame() == b.faces[fi].surf.frame();
            if in_place && (surf.u_periodic() || surf.v_periodic()) && nl.len() == lp.len() {
                for i in 0..nl.len() {
                    let e = nl[i].edge;
                    if e >= b.edges.len() || out.edges[e].degenerate || b.edges[e].degenerate {
                        continue;
                    }
                    let d =
                        lp[i].pc.point(b.edges[e].mid_t()) - nl[i].pc.point(out.edges[e].mid_t());
                    let period = |x: f64, periodic: bool| {
                        if periodic {
                            (x / TAU).round() * TAU
                        } else {
                            0.0
                        }
                    };
                    let (du, dv) = (
                        period(d.x, surf.u_periodic()),
                        period(d.y, surf.v_periodic()),
                    );
                    if du != 0.0 || dv != 0.0 {
                        nl[i].pc = nl[i]
                            .pc
                            .transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                    }
                }
            }
            // A pole's point edge on a new periodic surface (a B-spline cone become a cone), or
            // on a face whose seam moved: no projection gives its p-curve. It runs from where
            // the loop arrives at the pole to where it leaves, across the turn the loop needs to
            // close (the coedges after it a period on).
            let seam_moved = nl.iter().any(|c| {
                c.edge < ef.len()
                    && ef[c.edge].len() == 2
                    && ef[c.edge][0] == fi
                    && ef[c.edge][1] == fi
                    && !out.edges[c.edge].degenerate
                    && out.edges[c.edge].curve != b.edges[c.edge].curve
            });
            if (surf != b.faces[fi].surf || seam_moved)
                && (surf.u_periodic() || surf.v_periodic())
                && let Some(k) = nl.iter().position(|c| out.edges[c.edge].degenerate)
            {
                let n = nl.len();
                let end_of = |c: &Coedge| {
                    let (_, t1) = super::topo::co_range(&out.edges[c.edge], c.fwd);
                    c.pc.point(t1)
                };
                let start_of = |c: &Coedge| {
                    let (t0, _) = super::topo::co_range(&out.edges[c.edge], c.fwd);
                    c.pc.point(t0)
                };
                let period = |d: f64, periodic: bool| {
                    if periodic {
                        (d / TAU).round() * TAU
                    } else {
                        0.0
                    }
                };
                let g = start_of(&nl[0]) - end_of(&nl[n - 1]);
                let (gu, gv) = (
                    period(g.x, surf.u_periodic()),
                    period(g.y, surf.v_periodic()),
                );
                if gu != 0.0 || gv != 0.0 {
                    for c in nl.iter_mut().skip(k + 1) {
                        c.pc = c.pc.transformed(&Affine::translation(V3::new(gu, gv, 0.0)));
                    }
                }
                // (Every pole of the loop: a sphere's face can reach both.)
                let poles: Vec<usize> = (k..n)
                    .filter(|&j| out.edges[nl[j].edge].degenerate)
                    .collect();
                for k in poles {
                    let a = if k > 0 {
                        end_of(&nl[k - 1])
                    } else {
                        end_of(&nl[n - 1]) - V3::new(gu, gv, 0.0)
                    };
                    let z = if k + 1 < n {
                        start_of(&nl[k + 1])
                    } else {
                        start_of(&nl[0])
                    };
                    let e = &out.edges[nl[k].edge];
                    let (t0, t1) = super::topo::co_range(e, nl[k].fwd);
                    if (t1 - t0).abs() > 0.0 {
                        let d = (z - a) / (t1 - t0);
                        nl[k].pc = Curve::Line {
                            o: P3::new(a.x - d.x * t0, a.y - d.y * t0, 0.0),
                            d: V3::new(d.x, d.y, 0.0),
                        };
                    }
                }
            }
            out.faces[fi].loops[li] = nl;
        }
    }
    // Seams: both coedges of a seam got the same p-curve; shift the one that should be a
    // period away so the loop closes (on a closed spline, to the other side of its range).
    for fi in 0..out.faces.len() {
        let surf = out.faces[fi].surf.clone();
        if !surf.u_periodic() {
            for li in 0..out.faces[fi].loops.len() {
                let n = out.faces[fi].loops[li].len();
                for k in 0..n {
                    let prev = out.faces[fi].loops[li][(k + n - 1) % n].clone();
                    let cur = out.faces[fi].loops[li][k].clone();
                    let (_, pt1) = super::topo::co_range(&out.edges[prev.edge], prev.fwd);
                    let (ct0, ct1) = super::topo::co_range(&out.edges[cur.edge], cur.fwd);
                    if let Some(shifted) = closed_spline_shift(&surf, &cur.pc, ct0, ct1, {
                        let a = prev.pc.point(pt1);
                        [a.x, a.y]
                    }) {
                        out.faces[fi].loops[li][k].pc = shifted;
                    }
                }
            }
            continue;
        }
        for li in 0..out.faces[fi].loops.len() {
            let n = out.faces[fi].loops[li].len();
            for k in 0..n {
                let prev = out.faces[fi].loops[li][(k + n - 1) % n].clone();
                let cur = out.faces[fi].loops[li][k].clone();
                let pe = &out.edges[prev.edge];
                let ce = &out.edges[cur.edge];
                let (_, pt1) = super::topo::co_range(pe, prev.fwd);
                let (ct0, _) = super::topo::co_range(ce, cur.fwd);
                let a = prev.pc.point(pt1);
                let q = cur.pc.point(ct0);
                let du = ((a.x - q.x) / TAU).round() * TAU;
                if du != 0.0 {
                    out.faces[fi].loops[li][k].pc = cur
                        .pc
                        .transformed(&Affine::translation(V3::new(du, 0.0, 0.0)));
                }
            }
        }
    }
    // Holes on periodic faces: each moved by whole periods to lie within the outer loop's range
    // (the loops are traced one by one, each from where its first p-curve happened to land).
    for f in out.faces.iter_mut() {
        if f.loops.len() < 2 || !(f.surf.u_periodic() || f.surf.v_periodic()) {
            continue;
        }
        let range = |l: &[Coedge], edges: &[super::topo::Edge]| {
            let mut r = (
                (f64::INFINITY, f64::NEG_INFINITY),
                (f64::INFINITY, f64::NEG_INFINITY),
            );
            for c in l {
                let e = &edges[c.edge];
                for i in 0..=8 {
                    let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0);
                    r = (
                        (r.0.0.min(q.x), r.0.1.max(q.x)),
                        (r.1.0.min(q.y), r.1.1.max(q.y)),
                    );
                }
            }
            r
        };
        let outer = range(&f.loops[0], &out.edges);
        for li in 1..f.loops.len() {
            let hole = range(&f.loops[li], &out.edges);
            let shift = |(a, b): (f64, f64), (ha, hb): (f64, f64), periodic: bool| {
                if !periodic {
                    return 0.0;
                }
                let mid = 0.5 * (ha + hb);
                if mid >= a && mid <= b {
                    0.0
                } else {
                    ((0.5 * (a + b) - mid) / TAU).round() * TAU
                }
            };
            let (du, dv) = (
                shift(outer.0, hole.0, f.surf.u_periodic()),
                shift(outer.1, hole.1, f.surf.v_periodic()),
            );
            if du != 0.0 || dv != 0.0 {
                for c in f.loops[li].iter_mut() {
                    c.pc = c.pc.transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                }
            }
        }
    }
    super::build::fix_senses(&mut out, 0..b.faces.len());
    prof.mark("p-curves");
    // Tolerances: an edge between moved faces is a new intersection curve, its p-curves fits
    // on the new surfaces: its tolerance is how far they stray from one another (not the old
    // edge's, which a read model sets for its own geometry); likewise a moved vertex's.
    let changed: Vec<bool> = (0..b.faces.len())
        .map(|f| surfs[f] != b.faces[f].surf)
        .collect();
    let mut stray = vec![0.0f64; out.edges.len()];
    for f in out.faces.iter() {
        for c in f.loops.iter().flatten() {
            let e = &out.edges[c.edge];
            if e.degenerate {
                continue;
            }
            for i in 0..=8 {
                let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 8.0;
                let uv = c.pc.point(t);
                let p = e.point(t);
                stray[c.edge] = stray[c.edge]
                    .max((f.surf.eval(uv.x, uv.y) - p).norm())
                    .max(f.surf.distance(&p));
            }
        }
    }
    for (e, x) in stray.iter().enumerate() {
        if ef.get(e).is_none_or(|fs| fs.iter().any(|&f| changed[f])) && !out.edges[e].degenerate {
            out.edges[e].tol = (1.5 * x).max(super::topo::TOL_MIN);
        }
    }
    for v in 0..out.verts.len() {
        let fs: &[usize] = vf.get(v).map_or(&[], |x| x.as_slice());
        if v < vf.len() && !fs.iter().any(|&f| changed[f]) {
            continue;
        }
        let p = out.verts[v].p;
        let mut d = fs
            .iter()
            .map(|&f| out.faces[f].surf.distance(&p))
            .fold(0.0, f64::max);
        for e in out.edges.iter().filter(|e| !e.degenerate) {
            for k in 0..2 {
                if e.v[k] == v {
                    d = d.max((e.point(e.t[k]) - p).norm());
                }
            }
        }
        out.verts[v].tol = (1.5 * d).max(super::topo::TOL_MIN);
    }
    if !cut.is_empty() {
        // (The swallowed poles' point edges and vertices.)
        super::unify::compact(&mut out);
    }
    prof.mark("tolerances");
    let errs = if volumes {
        out.check()
    } else {
        out.check_structure()
    };
    prof.mark("check");
    prof.report(&format!("replace surfaces ({} faces)", out.faces.len()));
    if !errs.is_empty() {
        if std::env::var_os("CADRS_DRAFT_DEBUG").is_some() {
            for (fi, f) in out.faces.iter().enumerate() {
                for l in &f.loops {
                    for c in l {
                        let e = &out.edges[c.edge];
                        let (a, z) = super::topo::co_range(e, c.fwd);
                        let old = b.edges.get(c.edge).map(|x| x.t);
                        eprintln!(
                            "face {fi} edge {} fwd {} t {:?} (old {:?}) pc {:?} -> {:?} {}",
                            c.edge,
                            c.fwd,
                            e.t,
                            old,
                            c.pc.point(a),
                            c.pc.point(z),
                            format!("{:?}", e.curve)
                                .chars()
                                .take(60)
                                .collect::<String>()
                        );
                    }
                }
            }
        }
        return Err(KernelError::InvalidResult {
            op: "local operation",
            why: format!("the moved faces don't close: {}", errs[0]),
        });
    }
    Ok(out)
}

/// The solid bounded by the planes `surfs` of a convex polyhedron `b` (each face's, with the
/// material on the side it is now): the half-spaces' common part, with the face of `b` each of its
/// faces lies on. None when `b` is not a convex polyhedron (a face curved, or a vertex outside
/// some face's plane), or the planes bound no solid within reach of `b`.
pub(crate) fn planes_bound(b: &Body, surfs: &[Surface]) -> Option<Result<(Body, Vec<usize>)>> {
    let bb = b.bbox();
    let size = bb.diagonal();
    let close = tol::POINT.max(tol::REL * size);
    // Each face's plane: a point on it and its outward normal, before and after.
    let plane = |s: &Surface, sense: bool| match s {
        Surface::Plane { f } => Some((
            f.o,
            f.x.cross(&f.y).normalize() * if sense { 1.0 } else { -1.0 },
        )),
        _ => None,
    };
    let mut before = Vec::new();
    let mut after = Vec::new();
    for (f, s) in b.faces.iter().zip(surfs) {
        before.push(plane(&f.surf, f.sense)?);
        after.push(plane(s, f.sense)?);
    }
    if b.verts
        .iter()
        .any(|v| before.iter().any(|(o, n)| (v.p - o).dot(n) > close))
    {
        return None;
    }
    let mut reach = bb;
    reach.add(&(bb.min - V3::repeat(size)));
    reach.add(&(bb.max + V3::repeat(size)));
    Some((|| {
        let mut acc: Option<Body> = None;
        for (o, n) in &after {
            let h = super::ops::half_space_box(*o, -n, &reach)?;
            acc = Some(match acc {
                None => h,
                Some(a) => super::boolean::boolean_bodies(&a, &h, BoolOp::Intersect)?.body,
            });
        }
        let out =
            acc.filter(|a| !a.faces.is_empty())
                .ok_or_else(|| KernelError::InvalidResult {
                    op: "draft",
                    why: "the drafted planes bound nothing".into(),
                })?;
        // Every face on one of the planes (none on a half-space box's far sides).
        let mut from = Vec::new();
        for f in &out.faces {
            let (o, n) = plane(&f.surf, f.sense).ok_or_else(|| KernelError::InvalidResult {
                op: "draft",
                why: "a curved face bounding planes".into(),
            })?;
            let j = after
                .iter()
                .position(|(q, m)| m.dot(&n) > 1.0 - tol::ANGULAR && (o - q).dot(m).abs() < close)
                .ok_or_else(|| KernelError::InvalidResult {
                    op: "draft",
                    why: "the drafted planes don't close round the part".into(),
                })?;
            from.push(j);
        }
        Ok((out, from))
    })())
}

/// `Kernel::replace_faces`: faces given new surfaces, each keeping the side its material is
/// on (SMLib's `SmBrep::LocalOperation` keeps the face's use).
pub fn replace_faces(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceReplacement],
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    let mut surfs: Vec<Surface> = b.faces.iter().map(|f| f.surf.clone()).collect();
    let mut domains: Vec<Option<UvBox>> = vec![None; b.faces.len()];
    for r in faces {
        let fi = r.face.0 as usize;
        if fi >= b.faces.len() {
            return Err(KernelError::InvalidParameter(format!(
                "unknown {:?}",
                r.face
            )));
        }
        surfs[fi] = r.surface.clone();
        domains[fi] = r.domain;
    }
    let mut prof = super::boolean::Prof::new();
    // (Its faces' senses are settled below, and the volume checked after.)
    let mut out = replace_surfaces_checked(&b, &surfs, &domains, false)?;
    prof.mark("surfaces");
    // The outward normal at the middle of each new face agrees with the old face's at its own.
    let outward = |body: &Body, fi: usize, p: &P3| {
        let f = &body.faces[fi];
        let (u, v) = f.surf.project(p, None);
        f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
    };
    let mut flipped = false;
    for r in faces {
        let fi = r.face.0 as usize;
        let middle = |body: &Body| {
            // (A point well inside the face is all the normal is wanted at: a coarse mesh.)
            super::smb::face_centroids_within(&super::kernel::subset(body, &[fi], false), 0.05)
                .first()
                .copied()
        };
        let Some(was) = middle(&b) else {
            continue;
        };
        // The new face's point beside the old one's middle: the replacement lies where the old
        // face was, and meshing a heavy new surface only to find a point on it costs more than
        // the rest of the operation (else its own middle, as the old face's).
        let near = {
            let s = &out.faces[fi].surf;
            let (u, v) = s.project(&was, None);
            let p = s.eval(u, v);
            let size = 1.0 + b.bbox().diagonal();
            (super::blend::on_face(&out, fi, &p) && (p - was).norm() < 0.25 * size).then_some(p)
        };
        let Some(now) = near.or_else(|| middle(&out)) else {
            continue;
        };
        let before = outward(&b, fi, &was);
        if outward(&out, fi, &now).dot(&before) < 0.0 {
            out.faces[fi].sense = !out.faces[fi].sense;
            flipped = true;
        }
    }
    prof.mark("senses");
    // (The body as made is checked but for its solids' orientation: only a face turned since
    // needs another look, and one solid's is its volume's sign, measured below.)
    let errs = if flipped {
        out.check_structure()
    } else {
        Vec::new()
    };
    let errs = if errs.is_empty() && out.solids.len() > 1 {
        out.check()
    } else {
        errs
    };
    prof.mark("check");
    if let Some(e) = errs.first() {
        return Err(KernelError::InvalidResult {
            op: "replace faces",
            why: format!("the replaced faces don't close: {e}"),
        });
    }
    if super::mass::body_volume(&out) <= 0.0 {
        return Err(KernelError::InvalidResult {
            op: "replace faces",
            why: "the replaced faces turn the body inside out".into(),
        });
    }
    prof.mark("volume");
    prof.report("replace faces");
    let nf = out.faces.len();
    let id = k.insert(out);
    let history = History {
        modified: (0..nf)
            .map(|i| {
                (
                    FaceId(i as u64),
                    InputFace {
                        body,
                        face: FaceId(i as u64),
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// `Kernel::move_faces`: Move face's Translate and Rotate. The faces' surfaces moved by the
/// rigid `motion` and the body's edges and vertices recomputed where they meet their
/// neighbours ([`replace_surfaces_in`]), each moved face keeping the side its material is on.
/// A move that changes the topology is refused (`InvalidParameter`), as Move face's offset in
/// [`super::offset::offset`].
pub fn move_faces(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceId],
    motion: &Motion,
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    if faces.is_empty() {
        return Err(KernelError::InvalidParameter(
            "Move face needs faces to move".into(),
        ));
    }
    if let Some(f) = faces.iter().find(|f| f.0 as usize >= b.faces.len()) {
        return Err(KernelError::InvalidParameter(format!("unknown {f:?}")));
    }
    // A rigid motion: orthonormal, no reflection (a reflected face would turn inside out).
    let lin = motion.linear;
    let finite = lin
        .iter()
        .chain(motion.translation.iter())
        .all(|x| x.is_finite());
    if !finite
        || (lin.transpose() * lin - nalgebra::Matrix3::identity()).norm() > tol::ANGULAR.sqrt()
        || motion.is_reflection()
    {
        return Err(KernelError::InvalidParameter(
            "Move face takes a translation or a rotation".into(),
        ));
    }
    let m = Affine {
        m: lin,
        t: motion.translation,
    };
    let mut surfs: Vec<Surface> = b.faces.iter().map(|f| f.surf.clone()).collect();
    for f in faces {
        surfs[f.0 as usize] = b.faces[f.0 as usize].surf.transformed(&m);
    }
    let too_far =
        |why: &str| KernelError::InvalidParameter(format!("The faces can't move that far: {why}"));
    // (The full check below covers the solids' orientation, once the senses are settled.)
    let mut out = replace_surfaces_checked(&b, &surfs, &[], false).map_err(|e| match e {
        KernelError::InvalidGeometry { .. } | KernelError::InvalidResult { .. } => {
            too_far("the faces round them no longer meet them")
        }
        e => e,
    })?;
    // Each moved face keeps its material on the side it was (its outward normal moved along).
    for f in faces {
        let fi = f.0 as usize;
        let middle = |body: &Body| {
            // (A point well inside the face is all the normal is wanted at: a coarse mesh.)
            super::smb::face_centroids_within(&super::kernel::subset(body, &[fi], false), 0.05)
                .first()
                .copied()
        };
        let Some(was) = middle(&b) else {
            continue;
        };
        // The new face's point beside the old one's middle: the replacement lies where the old
        // face was, and meshing a heavy new surface only to find a point on it costs more than
        // the rest of the operation (else its own middle, as the old face's).
        let near = {
            let s = &out.faces[fi].surf;
            let (u, v) = s.project(&was, None);
            let p = s.eval(u, v);
            let size = 1.0 + b.bbox().diagonal();
            (super::blend::on_face(&out, fi, &p) && (p - was).norm() < 0.25 * size).then_some(p)
        };
        let Some(now) = near.or_else(|| middle(&out)) else {
            continue;
        };
        let outward = |body: &Body, p: &P3| {
            let face = &body.faces[fi];
            let (u, v) = face.surf.project(p, None);
            face.surf.normal(u, v) * if face.sense { 1.0 } else { -1.0 }
        };
        if outward(&out, &now).dot(&m.vector(&outward(&b, &was))) < 0.0 {
            out.faces[fi].sense = !out.faces[fi].sense;
        }
    }
    if !out.check_with(super::topo::CheckLevel::Full).is_empty() {
        return Err(too_far("a face would run into another face"));
    }
    if super::mass::body_volume(&out) <= 0.0 {
        return Err(too_far("the body would turn inside out"));
    }
    let nf = out.faces.len();
    let id = k.insert(out);
    let history = History {
        modified: (0..nf)
            .map(|i| {
                (
                    FaceId(i as u64),
                    InputFace {
                        body,
                        face: FaceId(i as u64),
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// The intersection of two surfaces between the moved ends `p0` and `p1` of the edge `ed`
/// (whose ends were `old`), traced: points on both surfaces across the old edge's way at its
/// points carried along with its ends, a cubic through them, their number doubled until the
/// cubic lies on both surfaces between them too. None for a closed edge, or where the
/// surfaces don't meet across the old edge's way.
fn trace_between(
    sa: &Surface,
    sb: &Surface,
    ed: &super::topo::Edge,
    old: (P3, P3),
    p0: P3,
    p1: P3,
) -> Option<Curve> {
    if ed.v[0] == ed.v[1] {
        return None;
    }
    let size = 1.0 + p0.coords.norm().max(p1.coords.norm());
    let mut n = 16;
    loop {
        let mut pts = vec![p0];
        for i in 1..n {
            let f = i as f64 / n as f64;
            let t = ed.t[0] + (ed.t[1] - ed.t[0]) * f;
            let q = ed.point(t) + (p0 - old.0) * (1.0 - f) + (p1 - old.1) * f;
            let cut = Surface::Plane {
                f: Frame::from_z(q, ed.curve.tangent(t).try_normalize(tol::DEGENERATE)?),
            };
            pts.push(meet(&[sa, sb, &cut], q)?);
        }
        pts.push(p1);
        let mut ts = vec![0.0];
        for w in pts.windows(2) {
            ts.push(ts[ts.len() - 1] + (w[1] - w[0]).norm());
        }
        if ts[n] < tol::REL * size {
            return None;
        }
        let c = Curve::Nurbs(std::sync::Arc::new(NurbsCurve::interpolate(
            &pts, &ts, 3, None, None,
        )));
        let on = ts.windows(2).all(|w| {
            let m = c.point(0.5 * (w[0] + w[1]));
            sa.distance(&m) < 1e-7 * size && sb.distance(&m) < 1e-7 * size
        });
        if on {
            return Some(c);
        }
        if n >= 256 {
            return None;
        }
        n *= 2;
    }
}

/// On a closed B-spline (an intersection loop), the arc from `p0` to `p1` leaving `p0` along
/// `dir0` (the whole loop for a closed edge) as one parameter range: the curve restarted at
/// `p0`, so the arc never runs through its start.
fn closed_arc(
    c: &Curve,
    p0: &P3,
    p1: &P3,
    closed_edge: bool,
    dir0: &V3,
) -> Option<(Curve, f64, f64)> {
    let Curve::Nurbs(n) = c else { return None };
    let (a, b) = n.range();
    let (s, e) = (n.point(a), n.point(b));
    if (s - e).norm() > tol::REL * (1.0 + s.coords.norm()) {
        return None;
    }
    let t0 = nurbs_passes(c, p0)
        .into_iter()
        .next()
        .unwrap_or_else(|| c.closest(p0, a, b));
    let r = n.restarted(t0).ok()?;
    let (a, b) = r.range();
    let rc = Curve::Nurbs(std::sync::Arc::new(r));
    let forward = rc.tangent(a).dot(dir0) >= 0.0;
    if closed_edge {
        return Some(if forward { (rc, a, b) } else { (rc, b, a) });
    }
    let t1 = rc.closest(p1, a, b);
    Some(if forward { (rc, a, t1) } else { (rc, b, t1) })
}

/// The range of a kept edge (its curve unchanged) whose ends slid along the curve to `p0`
/// and `p1`, still running from `p0` to `p1` with the parameter rising: on a circle or ellipse
/// the arc from `p0` round the way the old one went (its quarter point needn't be on the new
/// arc when an end slid past it); on a B-spline, of the parameters where the curve passes
/// each end, the nearest the old one (a closed intersection loop passes its start at both
/// ends of its range). None when an end isn't on the curve (it slid past a bounded
/// B-spline's end: the curve must be recomputed).
fn slide_range(ed: &super::topo::Edge, p0: &P3, p1: &P3) -> Option<[f64; 2]> {
    let c = &ed.curve;
    let size = 1.0 + p0.coords.norm().max(p1.coords.norm());
    let on = |t: f64, p: &P3| (c.point(t) - p).norm() < 1e-7 * size;
    match c {
        Curve::Line { .. } => {
            let (a, z) = (c.closest(p0, -1e12, 1e12), c.closest(p1, -1e12, 1e12));
            (on(a, p0) && on(z, p1)).then_some(if a <= z { [a, z] } else { [z, a] })
        }
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let t0 = unwrap_near(conic_closest(c, p0, -PI, PI), ed.t[0], TAU);
            let t1 = if ed.v[0] == ed.v[1] {
                t0 + (ed.t[1] - ed.t[0])
            } else {
                t0 + (conic_closest(c, p1, -PI, PI) - t0).rem_euclid(TAU)
            };
            (t1 > t0 && on(t0, p0) && on(t1, p1)).then_some([t0, t1])
        }
        Curve::Nurbs(n) => {
            let (a, z) = n.range();
            let loop_ = (c.point(a) - c.point(z)).norm() < tol::REL * size;
            let passes = |p: &P3| {
                let mut ts = nurbs_passes(c, p);
                if loop_ {
                    let ends: Vec<f64> = ts
                        .iter()
                        .filter_map(|&x| {
                            if (x - a).abs() < tol::REL * (z - a) {
                                Some(z)
                            } else if (z - x).abs() < tol::REL * (z - a) {
                                Some(a)
                            } else {
                                None
                            }
                        })
                        .collect();
                    ts.extend(ends);
                }
                ts
            };
            let nearest = |ts: Vec<f64>, old: f64, after: f64| {
                ts.into_iter()
                    .filter(|&x| x > after)
                    .min_by(|x, y| (x - old).abs().total_cmp(&(y - old).abs()))
            };
            let t0 = nearest(passes(p0), ed.t[0], f64::NEG_INFINITY)?;
            let t1 = nearest(passes(p1), ed.t[1], t0)?;
            Some([t0, t1])
        }
    }
}

/// The parameter of the point of the curve nearest `p` in [lo, hi]. On an ellipse, from the
/// angle of `p` in the ellipse's own scaled frame (exact on the curve), refined near it:
/// sampling a long thin ellipse evenly in its parameter steps over the near side, and Newton
/// from the nearest sample then settles on the far side's local minimum.
fn conic_closest(c: &Curve, p: &P3, lo: f64, hi: f64) -> f64 {
    match c {
        Curve::Ellipse { c: o, x, y, a, b } => {
            let v = p - o;
            let g = (v.dot(y) / b).atan2(v.dot(x) / a);
            super::curve::clamp_periodic(c.closest(p, g - PI / 8.0, g + PI / 8.0), lo, hi)
        }
        _ => c.closest(p, lo, hi),
    }
}

/// Parameters of two points on a curve; a closed edge spans a whole turn from p0. On a
/// circle or ellipse, the arc leaving `p0` along `dir0` (the old edge's direction there) when
/// the curve's tangent there is clearly along or against it, else the arc through `quarter`
/// (the old edge's quarter point: an end that moved past it leaves it off the new arc).
fn param_range(c: &Curve, p0: &P3, p1: &P3, closed: bool, quarter: &P3, dir0: &V3) -> (f64, f64) {
    match c {
        Curve::Circle { .. } | Curve::Ellipse { .. } => {
            let t0 = conic_closest(c, p0, -PI, PI);
            if closed {
                // The direction from the old edge's quarter point.
                let tq = conic_closest(c, quarter, t0 - PI, t0 + PI);
                if tq >= t0 {
                    (t0, t0 + TAU)
                } else {
                    (t0, t0 - TAU)
                }
            } else {
                let t1 = conic_closest(c, p1, t0 - PI, t0 + PI);
                let tan = c.tangent(t0);
                let along = tan.dot(dir0) / tan.norm().max(1e-300);
                if along.abs() > 0.5 {
                    return if along > 0.0 {
                        (t0, t0 + (t1 - t0).rem_euclid(TAU))
                    } else {
                        (t0, t0 - (t0 - t1).rem_euclid(TAU))
                    };
                }
                // Else the way round through the old quarter point.
                let tq = conic_closest(c, quarter, t0 - TAU, t0 + TAU);
                let between = |a: f64, b: f64, x: f64| (x - a) * (x - b) <= 0.0;
                if between(t0, t1, unwrap_near(tq, (t0 + t1) / 2.0, TAU)) {
                    (t0, t1)
                } else if t1 > t0 {
                    (t0, t1 - TAU)
                } else {
                    (t0, t1 + TAU)
                }
            }
        }
        _ => {
            let (a, b) = c.nurbs_range().unwrap_or((-1e7, 1e7));
            (c.closest(p0, a, b), c.closest(p1, a, b))
        }
    }
}

/// The parameters where a B-spline passes through `p` (more than one where it runs back near
/// itself).
fn nurbs_passes(c: &Curve, p: &P3) -> Vec<f64> {
    nurbs_samples(c).map_or_else(Vec::new, |sm| nurbs_passes_in(c, &sm, p))
}

/// A B-spline's points at evenly spaced parameters, for [`nurbs_passes_in`] (the same for
/// every point it is asked about).
fn nurbs_samples(c: &Curve) -> Option<(Vec<f64>, Vec<P3>)> {
    const N: usize = 400;
    let (a, b) = c.nurbs_range()?;
    let ts: Vec<f64> = (0..=N).map(|i| a + (b - a) * i as f64 / N as f64).collect();
    let pts: Vec<P3> = ts.iter().map(|&t| c.point(t)).collect();
    Some((ts, pts))
}

/// [`nurbs_passes`] with the curve sampled already ([`nurbs_samples`]).
fn nurbs_passes_in(c: &Curve, (ts, pts): &(Vec<f64>, Vec<P3>), p: &P3) -> Vec<f64> {
    let n = ts.len() - 1;
    let (a, b) = (ts[0], ts[n]);
    let scale = 1.0 + pts.iter().map(|q| q.coords.norm()).fold(0.0, f64::max);
    let d: Vec<f64> = pts.iter().map(|q| (q - p).norm()).collect();
    let mut out: Vec<f64> = Vec::new();
    for i in 0..=n {
        let left = if i == 0 { f64::MAX } else { d[i - 1] };
        let right = if i == n { f64::MAX } else { d[i + 1] };
        if d[i] <= left && d[i] <= right {
            let t = c.closest(p, ts[i.saturating_sub(1)], ts[(i + 1).min(n)]);
            if (c.point(t) - p).norm() < tol::REL_POINT * scale
                && !out.iter().any(|x| (x - t).abs() < (b - a) * tol::REL_POINT)
            {
                out.push(t);
            }
        }
    }
    out
}

/// On a B-spline, the arc from `p0` to `p1` leaving `p0` along `dir0`: of the parameters where
/// the curve passes each end, the shortest pair running that way.
fn nurbs_arc(c: &Curve, p0: &P3, p1: &P3, dir0: &V3) -> Option<(f64, f64)> {
    let sm = nurbs_samples(c)?;
    let (s0, s1) = (nurbs_passes_in(c, &sm, p0), nurbs_passes_in(c, &sm, p1));
    let mut best: Option<(f64, (f64, f64))> = None;
    for &t0 in &s0 {
        for &t1 in &s1 {
            if c.tangent(t0).dot(dir0) * (t1 - t0) <= 0.0 {
                continue;
            }
            if best.is_none_or(|(w, _)| (t1 - t0).abs() < w) {
                best = Some(((t1 - t0).abs(), (t0, t1)));
            }
        }
    }
    best.map(|(_, r)| r)
}

/// The faces reached from `seed` across smooth edges (the faces' normals agree along them).
pub fn tangent_closure(b: &Body, mut seed: Vec<usize>) -> Vec<usize> {
    let ef = b.edge_faces();
    let normal = |fi: usize, p: &P3| {
        let f = &b.faces[fi];
        let (u, v) = f.surf.project(p, None);
        f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
    };
    let smooth = |e: usize| {
        let ed = &b.edges[e];
        let fs = &ef[e];
        if ed.degenerate || fs.len() != 2 || fs[0] == fs[1] {
            return None;
        }
        let ok = [0.25, 0.5, 0.75].iter().all(|&s| {
            let p = ed.point(ed.t[0] + (ed.t[1] - ed.t[0]) * s);
            normal(fs[0], &p).dot(&normal(fs[1], &p)) > 1.0 - tol::ANGULAR_POINT
        });
        ok.then_some((fs[0], fs[1]))
    };
    let pairs: Vec<(usize, usize)> = (0..b.edges.len()).filter_map(smooth).collect();
    let mut i = 0;
    while i < seed.len() {
        let f = seed[i];
        for &(x, y) in &pairs {
            let o = if x == f {
                y
            } else if y == f {
                x
            } else {
                continue;
            };
            if !seed.contains(&o) {
                seed.push(o);
            }
        }
        i += 1;
    }
    seed
}

/// Draft: the faces turned about the neutral plane by the angle, leaning in as they rise along
/// the pull direction.
pub fn draft(k: &mut NativeKernel, body: BodyId, spec: &DraftSpec) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    let pull = spec.pull.into_inner();
    let mut surfs: Vec<Surface> = b.faces.iter().map(|f| f.surf.clone()).collect();
    let mut faces: Vec<usize> = Vec::new();
    for fid in &spec.faces {
        let fi = fid.0 as usize;
        b.faces
            .get(fi)
            .ok_or_else(|| KernelError::InvalidParameter(format!("unknown {fid:?}")))?;
        if !faces.contains(&fi) {
            faces.push(fi);
        }
    }
    if spec.tangent_propagation {
        faces = tangent_closure(&b, faces);
    }
    for fi in faces {
        let f = &b.faces[fi];
        surfs[fi] = match &f.surf {
            Surface::Plane { f: fr } => {
                let n = fr.x.cross(&fr.y) * if f.sense { 1.0 } else { -1.0 };
                if n.cross(&pull).norm() < tol::ANGULAR {
                    return Err(KernelError::InvalidParameter(
                        "A face square to the pull direction can't be drafted".into(),
                    ));
                }
                // The line where the face meets the neutral plane (the hinge), and the face
                // turned about it to make the angle with the pull (OCCT's: absolute, so a face
                // already inclined takes the angle given), its outward normal tipping towards
                // the pull (the face leans in as it rises).
                let nn = spec.neutral_normal.map_or(pull, |x| x.into_inner());
                let t = nn.cross(&n).try_normalize(tol::ANGULAR).ok_or_else(|| {
                    KernelError::InvalidParameter(
                        "A face parallel to the neutral plane can't be drafted".into(),
                    )
                })?;
                let m = t.cross(&n);
                let alpha = -(fr.o - spec.neutral).dot(&nn) / m.dot(&nn);
                let on = fr.o + m * alpha;
                // The new normal turns about the hinge (n cos θ + m sin θ) until its component
                // along the pull is sin a; of the two turns, the smaller.
                let (ca, cb) = (n.dot(&pull), m.dot(&pull));
                let r = (ca * ca + cb * cb).sqrt();
                if r < spec.angle.sin().abs() - tol::ANGULAR_FINE {
                    return Err(KernelError::InvalidParameter(
                        "the face can't reach that angle turning about its hinge".into(),
                    ));
                }
                let phi = cb.atan2(ca);
                let delta = (spec.angle.sin() / r).clamp(-1.0, 1.0).acos();
                let wrap = |x: f64| (x + PI).rem_euclid(TAU) - PI;
                let (th1, th2) = (wrap(phi + delta), wrap(phi - delta));
                let th = if th1.abs() <= th2.abs() { th1 } else { th2 };
                let n2 = n * th.cos() + m * th.sin();
                let natural = if f.sense { n2 } else { -n2 };
                Surface::Plane {
                    f: Frame {
                        o: on,
                        x: t,
                        y: natural.cross(&t),
                        z: natural,
                    },
                }
            }
            Surface::Cylinder { f: fr, .. } | Surface::Cone { f: fr, .. } => {
                if spec.neutral_normal.is_some() {
                    return Err(KernelError::Unsupported(
                        "drafting a cylinder or cone about a neutral plane at an angle to the pull",
                    ));
                }
                if fr.z.cross(&pull).norm() > tol::ANGULAR {
                    return Err(KernelError::Unsupported(
                        "drafting a cylinder or cone not along the pull",
                    ));
                }
                let (r, a0) = match &f.surf {
                    Surface::Cone { r, a, .. } => (*r, *a),
                    Surface::Cylinder { r, .. } => (*r, 0.0),
                    _ => unreachable!(),
                };
                // A cone through the face's circle at the neutral plane, narrowing along the
                // pull for a positive angle on an outward-facing face (the angle absolute, as
                // OCCT's: a cone takes the angle given, whatever its own).
                let h0 = (spec.neutral - fr.o).dot(&fr.z);
                let r0 = r + h0 / a0.cos() * a0.sin();
                if r0 <= 0.0 {
                    return Err(KernelError::InvalidGeometry {
                        op: "draft",
                        why: "the cone's apex is on the far side of the neutral plane".into(),
                    });
                }
                // (A cone's natural normal points away from its axis while its half-angle's
                // cosine is positive: a read cone can have |a| past a right angle.)
                let natural_out = !matches!(f.surf, Surface::Cone { a, .. } if a.cos() < 0.0);
                let outward = (f.sense == fr.right_handed()) == natural_out;
                let s_pull = fr.z.dot(&pull).signum();
                // v along the generator; radius r + v sin a with a negative to narrow as z rises.
                let a = -spec.angle * s_pull * if outward { 1.0 } else { -1.0 };
                Surface::Cone {
                    f: Frame {
                        o: fr.o + fr.z * h0,
                        ..*fr
                    },
                    r: r0,
                    a,
                }
            }
            other => {
                if std::env::var_os("CADRS_DRAFT_DEBUG").is_some() {
                    eprintln!("draft: unsupported face {fi}: {:?}", other.kind());
                }
                return Err(KernelError::Unsupported("drafting this kind of face"));
            }
        };
    }
    let (out, from) = match replace_surfaces(&b, &surfs) {
        Ok(out) => {
            let n = out.faces.len();
            (out, (0..n).collect::<Vec<_>>())
        }
        // Faces the draft swallows (a box's top, where its sides meet below it): of a convex
        // polyhedron, the solid the planes bound, as they are after the draft.
        Err(e) => match planes_bound(&b, &surfs) {
            Some(Ok(r)) => r,
            Some(Err(_)) | None => return Err(e),
        },
    };
    let id = k.insert(out);
    let history = History {
        modified: from
            .iter()
            .enumerate()
            .map(|(i, &j)| {
                (
                    FaceId(i as u64),
                    InputFace {
                        body,
                        face: FaceId(j as u64),
                    },
                )
            })
            .collect(),
        ..Default::default()
    };
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

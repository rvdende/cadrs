//! Curves on faces: curves projected onto faces along a direction (Onshape's Projected curve),
//! and faces split along curves that lie on them (Onshape's Split of faces by a curve).
//!
//! A projection casts a line along the direction from points of the curve, every few degrees
//! of it, and keeps where each first meets one of the faces; the points are approximated by a
//! cubic B-spline within a hair of them (a curve leaving the faces breaks there).
//!
//! A split takes the stretch of the curve that runs across the face (its points' feet on the
//! face's surface inside the face), from where it meets the face's boundary to where it leaves
//! it: those boundary edges are split there, the stretch is a new edge (its p-curve the feet's
//! (u, v) interpolated at the curve's parameters) and the face is traced into pieces along it
//! ([`super::heal_split::trace_pieces`]).

use std::sync::Arc;

use super::classify::{InFace, face_polygons, in_face};
use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::{NurbsCurve, basis_ders, find_span};
use super::split::Arc as SArc;
use super::sweep::path::{Seg, chain, path_segments};
use super::tol;
use super::topo::{Body, Edge};
use crate::*;

fn invalid(why: &str) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// Samples of each curve of a projection.
const PROJECT_SAMPLES: usize = 160;
/// Samples of a cut along its curve.
const CUT_SAMPLES: usize = 200;

/// The curves projected onto the faces (see the module docs): one B-spline per stretch that
/// lands on them.
pub fn project_curve(k: &NativeKernel, spec: &ProjectCurveSpec) -> Result<Vec<Bspline3>> {
    let d = spec
        .direction
        .try_normalize(tol::DEGENERATE)
        .ok_or_else(|| invalid("A projection needs a direction"))?;
    if spec.faces.is_empty() {
        return Err(invalid("Select the faces to project onto"));
    }
    let segs = path_segments(k, &spec.curves)?;
    let (segs, _) = chain(segs).map_err(|_| invalid("The curves must join end to end"))?;
    let mut targets: Vec<Target> = Vec::new();
    let mut size: f64 = 1.0;
    for (b, f) in &spec.faces {
        let body = k.get(*b)?.clone();
        let fi = f.0 as usize;
        if fi >= body.faces.len() {
            return Err(invalid("A face to project onto no longer exists"));
        }
        size = size.max(body.face_bbox(fi).diagonal());
        targets.push(Target::new(body, fi));
    }
    for s in &segs {
        size = size.max(s.start().coords.norm()).max(s.end().coords.norm());
    }
    let reach = 4.0 * size;
    let near = 1e-7 * size;
    let cast = |p: P3| -> Option<P3> {
        // The nearest hit along the direction, else the nearest the other way.
        let mut best: Option<(f64, P3)> = None;
        for target in &targets {
            for (t, q) in target.hits(p, d, near) {
                let key = if t >= -near { t } else { reach - t };
                if best.is_none_or(|(k, _)| key < k) {
                    best = Some((key, q));
                }
            }
        }
        best.map(|b| b.1)
    };
    // (A line through where two faces meet may miss both: a miss or two in a row is bridged.)
    let mut pieces: Vec<Vec<P3>> = vec![Vec::new()];
    let mut misses = 0;
    for (si, s) in segs.iter().enumerate() {
        for j in 0..=PROJECT_SAMPLES {
            if si > 0 && j == 0 {
                continue;
            }
            let p = point_at(s, j as f64 / PROJECT_SAMPLES as f64);
            match cast(p) {
                Some(q) => {
                    pieces.last_mut().unwrap().push(q);
                    misses = 0;
                }
                None => {
                    misses += 1;
                    if misses == 3 && !pieces.last().unwrap().is_empty() {
                        pieces.push(Vec::new());
                    }
                }
            }
        }
    }
    let out: Vec<Bspline3> = pieces
        .into_iter()
        .filter(|p| p.len() >= 4)
        .map(|pts| approximate(&pts, 1e-6 * size).to_bspline())
        .collect();
    if out.is_empty() {
        return Err(invalid("The curves don't land on the faces"));
    }
    Ok(out)
}

/// A face a projection lands on, with a grid of its surface's points over its (u, v) box: the
/// starts for finding where a line crosses it (every crossing, though faces of one surface
/// share it).
struct Target {
    body: Arc<Body>,
    face: usize,
    polys: Vec<Vec<P2>>,
    grid: Vec<(P2, P3)>,
}

impl Target {
    const GRID: usize = 24;

    fn new(body: Arc<Body>, face: usize) -> Self {
        let polys = face_polygons(&body, &body.faces[face]);
        let f = &body.faces[face];
        let mut grid = Vec::new();
        if let Some(((u0, u1), (v0, v1))) = super::heal_split::face_uv_box(&body, face) {
            for i in 0..=Self::GRID {
                for j in 0..=Self::GRID {
                    let uv = P2::new(
                        u0 + (u1 - u0) * i as f64 / Self::GRID as f64,
                        v0 + (v1 - v0) * j as f64 / Self::GRID as f64,
                    );
                    grid.push((uv, f.surf.eval(uv.x, uv.y)));
                }
            }
        }
        Self {
            body,
            face,
            polys,
            grid,
        }
    }

    /// Where the line through `p` along the unit `d` crosses the face: (its parameter, the
    /// point), by Newton's method from the grid points nearest the line.
    fn hits(&self, p: P3, d: V3, tol: f64) -> Vec<(f64, P3)> {
        let f = &self.body.faces[self.face];
        let off = |q: &P3| {
            let w = q - p;
            (w - d * w.dot(&d)).norm()
        };
        let mut seeds: Vec<(f64, P2)> = self.grid.iter().map(|(uv, q)| (off(q), *uv)).collect();
        seeds.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut out: Vec<(f64, P3)> = Vec::new();
        for (_, uv0) in seeds.into_iter().take(6) {
            let (mut u, mut v) = (uv0.x, uv0.y);
            let mut t = (f.surf.eval(u, v) - p).dot(&d);
            let mut done = false;
            for _ in 0..40 {
                let (q, su, sv) = f.surf.d1(u, v);
                let r = q - (p + d * t);
                if r.norm() < 1e-10 * (1.0 + q.coords.norm()) {
                    done = true;
                    break;
                }
                let m = nalgebra::Matrix3::from_columns(&[su, sv, -d]);
                let Some(x) = m.lu().solve(&(-r)) else { break };
                u += x[0];
                v += x[1];
                t += x[2];
            }
            if !done {
                continue;
            }
            let q = p + d * t;
            if in_face(&self.polys, &f.surf, P2::new(u, v), tol) == InFace::Outside {
                continue;
            }
            if !out.iter().any(|(s, _)| (s - t).abs() < 1e-6) {
                out.push((t, q));
            }
        }
        out
    }
}

/// The point a fraction `f` along a segment in its direction of travel.
fn point_at(s: &Seg, f: f64) -> P3 {
    let t = if s.rev {
        s.t1 + (s.t0 - s.t1) * f
    } else {
        s.t0 + (s.t1 - s.t0) * f
    };
    s.c.point(t)
}

/// A cubic B-spline through the ends of `pts` and within `tol` of the rest (at chord-length
/// parameters on [0, 1]): least squares on uniform spans, doubled until it fits.
pub(crate) fn approximate(pts: &[P3], tol: f64) -> NurbsCurve {
    let mut params = vec![0.0];
    for w in pts.windows(2) {
        params.push(params.last().unwrap() + (w[1] - w[0]).norm().max(tol::DEGENERATE));
    }
    let total = *params.last().unwrap();
    for p in &mut params {
        *p /= total;
    }
    let deg = 3;
    let mut spans = 4;
    loop {
        let c = least_squares(pts, &params, deg, spans);
        let worst = pts
            .iter()
            .zip(&params)
            .map(|(p, t)| (c.point(*t) - p).norm())
            .fold(0.0, f64::max);
        if worst <= tol || spans * 2 + deg >= pts.len() {
            return if worst <= tol {
                c
            } else {
                NurbsCurve::interpolate(pts, &params, deg, None, None)
            };
        }
        spans *= 2;
    }
}

/// The least-squares cubic on `spans` uniform spans through `pts` at `params`, its ends on the
/// first and last point.
fn least_squares(pts: &[P3], params: &[f64], deg: usize, spans: usize) -> NurbsCurve {
    let mut knots = vec![0.0; deg + 1];
    knots.extend((1..spans).map(|i| i as f64 / spans as f64));
    knots.extend(std::iter::repeat_n(1.0, deg + 1));
    let n = knots.len() - deg - 1;
    let mut a = nalgebra::DMatrix::<f64>::zeros(pts.len(), n);
    let mut rhs = nalgebra::DMatrix::<f64>::zeros(pts.len(), 3);
    for (r, (p, &t)) in pts.iter().zip(params).enumerate() {
        // The ends weigh as much as all the rest, so the curve runs through them.
        let w = if r == 0 || r + 1 == pts.len() {
            1e4
        } else {
            1.0
        };
        let span = find_span(n - 1, deg, t, &knots);
        let b = basis_ders(span, t, deg, 0, &knots);
        for (j, x) in b[0].iter().enumerate() {
            a[(r, span - deg + j)] = x * w;
        }
        for c in 0..3 {
            rhs[(r, c)] = p[c] * w;
        }
    }
    let sol = a
        .svd(true, true)
        .solve(&rhs, tol::ROUNDOFF_FINE)
        .unwrap_or_else(|_| nalgebra::DMatrix::zeros(n, 3));
    let mut cps: Vec<P3> = (0..n)
        .map(|i| P3::new(sol[(i, 0)], sol[(i, 1)], sol[(i, 2)]))
        .collect();
    cps[0] = pts[0];
    cps[n - 1] = pts[pts.len() - 1];
    NurbsCurve {
        degree: deg,
        knots,
        cps: cps.iter().map(|p| super::nurbs::hom(p, 1.0)).collect(),
    }
}

/// The faces of `body` split along the curves (see the module docs): each face, and the pieces
/// earlier curves cut it into, cut where a curve runs across it.
pub fn split_faces_by_curves(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceId],
    curves: &[PathCurve],
) -> Result<OpResult> {
    let segs = path_segments(k, curves)?;
    let mut b = (**k.get(body)?).clone();
    // The face of `body` each face is (a piece of).
    let mut from: Vec<usize> = (0..b.faces.len()).collect();
    let mut cut = false;
    for f in faces {
        let fi = f.0 as usize;
        if fi >= b.faces.len() {
            return Err(invalid("A face to split no longer exists"));
        }
        for s in &segs {
            let pieces: Vec<usize> = (0..b.faces.len()).filter(|&g| from[g] == fi).collect();
            for g in pieces {
                if let Some(out) = split_face_curve(&mut b, g, &s.c, s.t0, s.t1) {
                    from.extend(out[1..].iter().map(|_| fi));
                    cut = true;
                }
            }
        }
    }
    if !cut {
        return Err(invalid("The split curve doesn't run across the faces"));
    }
    b.fit_tolerances(1e-3);
    let history = History {
        modified: from
            .iter()
            .enumerate()
            .map(|(i, f)| {
                (
                    FaceId(i as u64),
                    InputFace {
                        body,
                        face: FaceId(*f as u64),
                    },
                )
            })
            .collect(),
        ..History::default()
    };
    let id = k.insert(b);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// Splits face `fi` of `b` along the stretch of curve `c` (over [t0, t1]) that runs across it,
/// boundary to boundary. Returns the faces it became (`fi` first); `None`, leaving `b` as it
/// was, when the curve doesn't run across the face.
pub(crate) fn split_face_curve(
    b: &mut Body,
    fi: usize,
    c: &Curve,
    t0: f64,
    t1: f64,
) -> Option<Vec<usize>> {
    let mut trial = b.clone();
    let out = split_in(&mut trial, fi, c, t0, t1)?;
    *b = trial;
    Some(out)
}

fn split_in(b: &mut Body, fi: usize, c: &Curve, t0: f64, t1: f64) -> Option<Vec<usize>> {
    let f = b.faces[fi].clone();
    let surf = f.surf.clone();
    let polys = face_polygons(b, &f);
    let size = b.face_bbox(fi).diagonal().max(1.0);
    // (A curve fitted to points on the face, as a blend's rail, strays a little between them.)
    let on = 1e-4 * size;
    let area0 = super::mass::face_area(b, fi).0;
    // 1. Which samples of the curve lie on the face.
    let at = |i: usize| t0 + (t1 - t0) * i as f64 / CUT_SAMPLES as f64;
    let mut hint: Option<(f64, f64)> = None;
    let mut inside = Vec::with_capacity(CUT_SAMPLES + 1);
    for i in 0..=CUT_SAMPLES {
        let p = c.point(at(i));
        let (u, v) = surf.project(&p, hint);
        hint = Some((u, v));
        let lies = (surf.eval(u, v) - p).norm() < on
            && in_face(&polys, &surf, P2::new(u, v), on) != InFace::Outside;
        inside.push(lies);
    }
    let first = inside.iter().position(|x| *x)?;
    let last = inside.iter().rposition(|x| *x)?;
    if last <= first || inside[first..=last].iter().any(|x| !x) {
        return None;
    }
    // Where it enters and leaves the face, between an outside sample and an inside one.
    let lies = |t: f64| -> bool {
        let p = c.point(t);
        let (u, v) = surf.project(&p, None);
        (surf.eval(u, v) - p).norm() < on
            && in_face(&polys, &surf, P2::new(u, v), on) != InFace::Outside
    };
    let edge_of = |out: f64, inn: f64| -> f64 {
        let (mut a, mut z) = (out, inn);
        for _ in 0..60 {
            let m = 0.5 * (a + z);
            if lies(m) {
                z = m;
            } else {
                a = m;
            }
        }
        z
    };
    let ta = if first == 0 {
        t0
    } else {
        edge_of(at(first - 1), at(first))
    };
    let tb = if last == CUT_SAMPLES {
        t1
    } else {
        edge_of(at(last + 1), at(last))
    };
    // 2. The boundary edges split where it meets them (each end moved onto its edge: where
    // the curve comes nearest it).
    let mut ends = [0usize; 2];
    let mut at_ends = [ta, tb];
    let window = 2.0 * (t1 - t0) / CUT_SAMPLES as f64;
    for (k, t) in [ta, tb].into_iter().enumerate() {
        let p = c.point(t);
        let mut best: Option<(f64, usize, f64)> = None;
        for co in b.faces[fi].loops.iter().flatten() {
            let e = &b.edges[co.edge];
            if e.degenerate {
                continue;
            }
            let (lo, hi) = (e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
            let s = e.curve.closest(&p, lo, hi);
            let d = (e.point(s) - p).norm();
            if best.is_none_or(|(bd, ..)| d < bd) {
                best = Some((d, co.edge, s));
            }
        }
        let (d, e, mut s) = best?;
        if d > 1e-3 * size {
            // It ends inside the face: no cut.
            return None;
        }
        let ed = &b.edges[e];
        let (lo, hi) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
        let mut t = t;
        for _ in 0..30 {
            let near = c.closest(&ed.point(s), (t - window).max(t0), (t + window).min(t1));
            let s2 = ed.curve.closest(&c.point(near), lo, hi);
            let done = (near - t).abs() < 1e-14 * (1.0 + t.abs()) && (s2 - s).abs() < 1e-14;
            t = near;
            s = s2;
            if done {
                break;
            }
        }
        at_ends[k] = t;
        ends[k] = if (ed.point(s) - b.verts[ed.v[0]].p).norm() < on {
            ed.v[0]
        } else if (ed.point(s) - b.verts[ed.v[1]].p).norm() < on {
            ed.v[1]
        } else {
            super::unify::split_edge(b, e, s)
        };
    }
    if ends[0] == ends[1] {
        return None;
    }
    let [ta, tb] = at_ends;
    // 3. The cut: the curve between, its p-curve its feet's (u, v).
    let m = 64;
    let ts: Vec<f64> = (0..=m)
        .map(|i| ta + (tb - ta) * i as f64 / m as f64)
        .collect();
    let mut hint: Option<(f64, f64)> = None;
    let uv: Vec<P3> = ts
        .iter()
        .map(|t| {
            let (u, v) = surf.project(&c.point(*t), hint);
            hint = Some((u, v));
            P3::new(u, v, 0.0)
        })
        .collect();
    let pc = Curve::Nurbs(Arc::new(NurbsCurve::interpolate(&uv, &ts, 3, None, None)));
    b.edges.push(Edge {
        curve: c.clone(),
        t: [ta, tb],
        v: ends,
        degenerate: false,
        tol: super::topo::TOL_MIN,
    });
    let cut = SArc {
        edge: b.edges.len() - 1,
        fwd: true,
        pc,
        t: [ta, tb],
        v: ends,
        cut: true,
        deg: false,
    };
    super::heal_split::trace_pieces(b, fi, vec![cut], area0)
}

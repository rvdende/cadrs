//! Minimum and maximum distances between points, edges, faces and bodies, with the points
//! where they are reached (SMLib's `SmTopologySolver::BrepBrepSolve` with `SM_SO_MINIMIZE` and
//! `SM_SO_MAXIMIZE`).
//!
//! An entity is cut into pieces of each dimension: a face is its surface over its trimmed
//! domain, its edges' curves and its vertices; an edge is its curve and its end vertices; a
//! body all of its faces, edges and vertices. The extremum over two entities is the best over
//! every pair of pieces, and on each pair either inside both pieces (a critical point of the
//! distance) or on a piece's boundary, which is a piece of its own: so each pair is solved
//! for its interior only, a face's (u, v) kept inside its loops.
//!
//! The global search is a branch and bound over pairs of cells (a parameter interval of a
//! curve, a uv rectangle of a face, a point), best bound first: each cell has a box sure to
//! hold it (`Surface::patch_bbox`; a curve's samples grown by h²/8·|C''|), so the distance
//! between two boxes bounds the minimum over the pair from below and the distance between
//! their farthest corners the maximum from above. A pair that can't beat the best found so far
//! is dropped; uv rectangles that miss the face's loops are dropped; the others are split
//! (the larger cell, a face's into four) down to a fiftieth of their piece. From the centres
//! of the pairs (inside their faces) Newton's method on ½|P − Q|² (all second derivatives,
//! Levenberg–Marquardt damped, kept in the pieces' parameter boxes) finds the extremum of the
//! basin exactly.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use nalgebra::{DMatrix, DVector};

use super::curve::Curve;
use super::math::*;
use super::tol;
use super::topo::Body;
use super::uvdomain::FaceDomain;

/// Where on an entity an extremum is reached.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Where {
    /// A free point.
    Point,
    /// A vertex (its index in the body).
    Vertex(usize),
    /// An edge at its parameter t.
    Edge(usize, f64),
    /// A face at (u, v).
    Face(usize, f64, f64),
}

/// An extremum of the distance: its value and where it is reached on each entity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extremum {
    pub distance: f64,
    pub a: P3,
    pub b: P3,
    pub wa: Where,
    pub wb: Where,
}

/// Minimum or maximum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sense {
    Min,
    Max,
}

/// A piece of an entity.
pub enum Piece<'a> {
    Point {
        p: P3,
        at: Where,
    },
    /// An edge's curve over its range, with its end vertices.
    Curve {
        c: &'a Curve,
        t: (f64, f64),
        edge: usize,
        ends: [usize; 2],
    },
    Face {
        dom: FaceDomain<'a>,
        face: usize,
    },
}

/// A free point.
pub fn point_pieces<'a>(p: P3) -> Vec<Piece<'a>> {
    vec![Piece::Point {
        p,
        at: Where::Point,
    }]
}

/// Edge `ei` of `b`: its curve and its vertices.
pub fn edge_pieces(b: &Body, ei: usize) -> Vec<Piece<'_>> {
    let e = &b.edges[ei];
    if e.degenerate {
        return vec![Piece::Point {
            p: b.verts[e.v[0]].p,
            at: Where::Vertex(e.v[0]),
        }];
    }
    let mut out = vec![Piece::Curve {
        c: &e.curve,
        t: (e.t[0], e.t[1]),
        edge: ei,
        ends: e.v,
    }];
    out.push(Piece::Point {
        p: b.verts[e.v[0]].p,
        at: Where::Vertex(e.v[0]),
    });
    if e.v[1] != e.v[0] {
        out.push(Piece::Point {
            p: b.verts[e.v[1]].p,
            at: Where::Vertex(e.v[1]),
        });
    }
    out
}

/// Face `fi` of `b` (`polys`: its uv polygons): its surface, its edges and its vertices.
pub fn face_pieces<'a>(b: &'a Body, fi: usize, polys: &'a [Vec<P2>]) -> Vec<Piece<'a>> {
    let mut edges: Vec<usize> = b.faces[fi]
        .loops
        .iter()
        .flatten()
        .map(|c| c.edge)
        .filter(|&e| !b.edges[e].degenerate)
        .collect();
    edges.sort_unstable();
    edges.dedup();
    let mut verts: Vec<usize> = b.faces[fi]
        .loops
        .iter()
        .flatten()
        .flat_map(|c| b.edges[c.edge].v)
        .collect();
    verts.sort_unstable();
    verts.dedup();
    let mut out = vec![Piece::Face {
        dom: FaceDomain::new(&b.faces[fi].surf, polys),
        face: fi,
    }];
    out.extend(edges.into_iter().map(|e| Piece::Curve {
        c: &b.edges[e].curve,
        t: (b.edges[e].t[0], b.edges[e].t[1]),
        edge: e,
        ends: b.edges[e].v,
    }));
    out.extend(verts.into_iter().map(|v| Piece::Point {
        p: b.verts[v].p,
        at: Where::Vertex(v),
    }));
    out
}

/// Every face, edge and vertex of `b` (`polys`: its faces' uv polygons).
pub fn body_pieces<'a>(b: &'a Body, polys: &'a [Vec<Vec<P2>>]) -> Vec<Piece<'a>> {
    let mut out: Vec<Piece> = (0..b.faces.len())
        .map(|fi| Piece::Face {
            dom: FaceDomain::new(&b.faces[fi].surf, &polys[fi]),
            face: fi,
        })
        .collect();
    out.extend(
        (0..b.edges.len())
            .filter(|&e| !b.edges[e].degenerate)
            .map(|e| Piece::Curve {
                c: &b.edges[e].curve,
                t: (b.edges[e].t[0], b.edges[e].t[1]),
                edge: e,
                ends: b.edges[e].v,
            }),
    );
    out.extend((0..b.verts.len()).map(|v| Piece::Point {
        p: b.verts[v].p,
        at: Where::Vertex(v),
    }));
    out
}

/// A part of a piece: the point, a parameter interval or a uv rectangle.
#[derive(Debug, Clone, Copy)]
enum Cell {
    Point,
    Curve(f64, f64),
    Face(f64, f64, f64, f64),
}

/// Up to two parameters of a piece.
type X = [f64; 2];

impl Cell {
    fn root(p: &Piece) -> Option<Cell> {
        match p {
            Piece::Point { .. } => Some(Cell::Point),
            Piece::Curve { t, .. } => Some(Cell::Curve(t.0, t.1)),
            Piece::Face { dom, .. } => {
                (!dom.is_empty()).then_some(Cell::Face(dom.ub.0, dom.ub.1, dom.vb.0, dom.vb.1))
            }
        }
    }

    fn centre(&self) -> X {
        match *self {
            Cell::Point => [0.0; 2],
            Cell::Curve(a, b) => [0.5 * (a + b), 0.0],
            Cell::Face(u0, u1, v0, v1) => [0.5 * (u0 + u1), 0.5 * (v0 + v1)],
        }
    }

    fn split(&self) -> Vec<Cell> {
        match *self {
            Cell::Point => vec![],
            Cell::Curve(a, b) => {
                let m = 0.5 * (a + b);
                vec![Cell::Curve(a, m), Cell::Curve(m, b)]
            }
            Cell::Face(u0, u1, v0, v1) => {
                let (um, vm) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
                vec![
                    Cell::Face(u0, um, v0, vm),
                    Cell::Face(um, u1, v0, vm),
                    Cell::Face(u0, um, vm, v1),
                    Cell::Face(um, u1, vm, v1),
                ]
            }
        }
    }
}

/// A box sure to hold the cell.
fn cell_box(p: &Piece, c: &Cell) -> Bbox {
    let mut bb = Bbox::empty();
    match (p, c) {
        (Piece::Point { p, .. }, _) => bb.add(p),
        (Piece::Curve { c: crv, .. }, Cell::Curve(a, b)) => {
            let mut k2: f64 = 0.0;
            for i in 0..=4 {
                let (q, _, d2) = crv.d2(a + (b - a) * i as f64 / 4.0);
                bb.add(&q);
                k2 = k2.max(d2.norm());
            }
            let h = (b - a) / 4.0;
            bb = bb.grow(1.5 * h * h / 8.0 * k2 + tol::ROUNDOFF_FINE);
        }
        (Piece::Face { dom, .. }, Cell::Face(u0, u1, v0, v1)) => {
            bb = dom.surf.patch_bbox((*u0, *u1), (*v0, *v1))
        }
        _ => {}
    }
    bb
}

/// Lower (`Min`) or upper (`Max`) bound of the distance between points of two boxes.
fn bound(a: &Bbox, b: &Bbox, sense: Sense) -> f64 {
    let mut s = 0.0;
    for k in 0..3 {
        let g = match sense {
            Sense::Min => (b.min[k] - a.max[k]).max(a.min[k] - b.max[k]).max(0.0),
            Sense::Max => (a.max[k] - b.min[k]).abs().max((b.max[k] - a.min[k]).abs()),
        };
        s += g * g;
    }
    s.sqrt()
}

/// The point of a piece and its first and second derivatives at x.
struct Jet {
    p: P3,
    n: usize,
    d1: [V3; 2],
    d2: [[V3; 2]; 2],
}

fn jet(p: &Piece, x: &X) -> Jet {
    let z = V3::zeros();
    match p {
        Piece::Point { p, .. } => Jet {
            p: *p,
            n: 0,
            d1: [z; 2],
            d2: [[z; 2]; 2],
        },
        Piece::Curve { c, .. } => {
            let (q, d1, d2) = c.d2(x[0]);
            Jet {
                p: q,
                n: 1,
                d1: [d1, z],
                d2: [[d2, z], [z, z]],
            }
        }
        Piece::Face { dom, .. } => {
            let d = dom.surf.d2(x[0], x[1]);
            Jet {
                p: P3::from(d[0]),
                n: 2,
                d1: [d[1], d[2]],
                d2: [[d[3], d[4]], [d[4], d[5]]],
            }
        }
    }
}

/// Whether x is a point of the piece's interior: a face's (u, v) strictly inside its loops (its
/// boundary is its edges' pieces, solved exactly there; the loops' band would let a maximum
/// stray past the edge).
fn valid(p: &Piece, x: &X) -> bool {
    match p {
        Piece::Face { dom, .. } => dom.contains(x[0], x[1], true),
        _ => true,
    }
}

/// x kept in the piece's parameter box.
fn clamp(p: &Piece, x: &X) -> X {
    match p {
        Piece::Point { .. } => *x,
        Piece::Curve { t, .. } => [x[0].clamp(t.0.min(t.1), t.0.max(t.1)), 0.0],
        Piece::Face { dom, .. } => [
            x[0].clamp(dom.ub.0, dom.ub.1),
            x[1].clamp(dom.vb.0, dom.vb.1),
        ],
    }
}

fn where_of(p: &Piece, x: &X) -> Where {
    match p {
        Piece::Point { at, .. } => *at,
        // At an end: the vertex there.
        Piece::Curve { edge, t, ends, .. } => {
            let tol = tol::REL * (t.1 - t.0).abs();
            if (x[0] - t.0).abs() <= tol {
                Where::Vertex(ends[0])
            } else if (x[0] - t.1).abs() <= tol {
                Where::Vertex(ends[1])
            } else {
                Where::Edge(*edge, x[0])
            }
        }
        Piece::Face { face, .. } => Where::Face(*face, x[0], x[1]),
    }
}

/// Newton (Levenberg–Marquardt damped) from (xa, xb) to the extremum of ½|P(xa) − Q(xb)|² in
/// the pieces' parameter boxes; `None` when it ends outside a face.
fn polish(pa: &Piece, xa: X, pb: &Piece, xb: X, sense: Sense) -> Option<(f64, X, X)> {
    let sg = if sense == Sense::Min { 1.0 } else { -1.0 };
    let f = |xa: &X, xb: &X| sg * 0.5 * (jet(pa, xa).p - jet(pb, xb).p).norm_squared();
    let (mut xa, mut xb) = (clamp(pa, &xa), clamp(pb, &xb));
    let mut fx = f(&xa, &xb);
    let mut lambda = 1e-6;
    for _ in 0..60 {
        let (ja, jb) = (jet(pa, &xa), jet(pb, &xb));
        let n = ja.n + jb.n;
        if n == 0 {
            break;
        }
        let r = ja.p - jb.p;
        let col = |k: usize| if k < ja.n { ja.d1[k] } else { -jb.d1[k - ja.n] };
        let sec = |k: usize, l: usize| -> V3 {
            if k < ja.n && l < ja.n {
                ja.d2[k][l]
            } else if k >= ja.n && l >= ja.n {
                -jb.d2[k - ja.n][l - ja.n]
            } else {
                V3::zeros()
            }
        };
        let g = DVector::from_fn(n, |k, _| sg * r.dot(&col(k)));
        let h = DMatrix::from_fn(n, n, |k, l| sg * (col(k).dot(&col(l)) + r.dot(&sec(k, l))));
        let scale = (0..n)
            .map(|k| h[(k, k)].abs())
            .fold(0.0, f64::max)
            .max(tol::TINY);
        let mut moved = false;
        for _ in 0..12 {
            let m = &h + DMatrix::identity(n, n) * (lambda * scale);
            let Some(step) = m.lu().solve(&(-&g)) else {
                lambda *= 10.0;
                continue;
            };
            let mut na = xa;
            let mut nb = xb;
            for k in 0..n {
                if k < ja.n {
                    na[k] += step[k];
                } else {
                    nb[k - ja.n] += step[k];
                }
            }
            let (na, nb) = (clamp(pa, &na), clamp(pb, &nb));
            let fn_ = f(&na, &nb);
            if fn_ <= fx {
                let tiny = (0..2).all(|k| {
                    (na[k] - xa[k]).abs() <= 1e-15 * (1.0 + xa[k].abs())
                        && (nb[k] - xb[k]).abs() <= 1e-15 * (1.0 + xb[k].abs())
                });
                xa = na;
                xb = nb;
                fx = fn_;
                lambda = (lambda / 4.0).max(tol::DEGENERATE);
                moved = !tiny;
                break;
            }
            lambda *= 10.0;
        }
        if !moved {
            break;
        }
    }
    (valid(pa, &xa) && valid(pb, &xb)).then(|| ((jet(pa, &xa).p - jet(pb, &xb).p).norm(), xa, xb))
}

/// A pair of cells waiting in the search, with its bound.
struct Node {
    key: f64,
    bound: f64,
    ia: usize,
    ca: Cell,
    ib: usize,
    cb: Cell,
    ba: Bbox,
    bb: Bbox,
}

impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.key == o.key
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        self.key.total_cmp(&o.key)
    }
}

/// The minimum or maximum distance between two entities given as pieces; `None` when either
/// has none (or only empty faces).
pub fn extremum(a: &[Piece], b: &[Piece], sense: Sense) -> Option<Extremum> {
    let boxes_a: Vec<Option<(Cell, Bbox)>> = a
        .iter()
        .map(|p| Cell::root(p).map(|c| (c, cell_box(p, &c))))
        .collect();
    let boxes_b: Vec<Option<(Cell, Bbox)>> = b
        .iter()
        .map(|p| Cell::root(p).map(|c| (c, cell_box(p, &c))))
        .collect();
    let mut all = Bbox::empty();
    for (_, bb) in boxes_a.iter().chain(&boxes_b).flatten() {
        all.union(bb);
    }
    if all.is_empty() {
        return None;
    }
    let scale = all.diagonal().max(tol::ROUNDOFF);
    // Two extrema within this are one (relative to the entities' size).
    let eps = tol::REL * scale;
    let mut best: Option<Extremum> = None;
    let better = |d: f64, best: &Option<Extremum>| match (best, sense) {
        (None, _) => true,
        (Some(e), Sense::Min) => d < e.distance,
        (Some(e), Sense::Max) => d > e.distance,
    };
    let consider = |ia: usize, xa: X, ib: usize, xb: X, best: &mut Option<Extremum>| {
        if let Some((d, xa, xb)) = polish(&a[ia], xa, &b[ib], xb, sense)
            && better(d, best)
        {
            *best = Some(Extremum {
                distance: d,
                a: jet(&a[ia], &xa).p,
                b: jet(&b[ib], &xb).p,
                wa: where_of(&a[ia], &xa),
                wb: where_of(&b[ib], &xb),
            });
        }
    };
    let key = |bd: f64| if sense == Sense::Min { -bd } else { bd };
    let mut heap = BinaryHeap::new();
    for (ia, ra) in boxes_a.iter().enumerate() {
        let Some((ca, ba)) = ra else { continue };
        for (ib, rb) in boxes_b.iter().enumerate() {
            let Some((cb, bb)) = rb else { continue };
            let bd = bound(ba, bb, sense);
            heap.push(Node {
                key: key(bd),
                bound: bd,
                ia,
                ca: *ca,
                ib,
                cb: *cb,
                ba: *ba,
                bb: *bb,
            });
        }
    }
    // Each piece's cells stop splitting at a fiftieth of the piece.
    let root_a: Vec<f64> = boxes_a
        .iter()
        .map(|r| r.as_ref().map_or(0.0, |(_, bb)| bb.diagonal()))
        .collect();
    let root_b: Vec<f64> = boxes_b
        .iter()
        .map(|r| r.as_ref().map_or(0.0, |(_, bb)| bb.diagonal()))
        .collect();
    let mut pops = 0usize;
    while let Some(nd) = heap.pop() {
        pops += 1;
        if pops > 200_000 {
            break;
        }
        if let Some(e) = &best {
            let hopeless = match sense {
                Sense::Min => nd.bound >= e.distance - eps,
                Sense::Max => nd.bound <= e.distance + eps,
            };
            if hopeless {
                break;
            }
        }
        let (pa, pb) = (&a[nd.ia], &b[nd.ib]);
        let (xa, xb) = (nd.ca.centre(), nd.cb.centre());
        let (sa, sb) = (nd.ba.diagonal(), nd.bb.diagonal());
        let split_a = !matches!(nd.ca, Cell::Point) && sa > root_a[nd.ia] / 50.0 && sa > eps;
        let split_b = !matches!(nd.cb, Cell::Point) && sb > root_b[nd.ib] / 50.0 && sb > eps;
        // From the centres when they lie in their faces: always at the leaves, and wherever the
        // centres alone beat the best.
        if valid(pa, &xa) && valid(pb, &xb) {
            let d = (jet(pa, &xa).p - jet(pb, &xb).p).norm();
            if !(split_a || split_b) || better(d, &best) {
                consider(nd.ia, xa, nd.ib, xb, &mut best);
            }
        }
        if !(split_a || split_b) {
            continue;
        }
        let along_a = split_a && (!split_b || sa >= sb);
        let kids: Vec<(Cell, Cell)> = if along_a {
            nd.ca.split().into_iter().map(|c| (c, nd.cb)).collect()
        } else {
            nd.cb.split().into_iter().map(|c| (nd.ca, c)).collect()
        };
        for (ca, cb) in kids {
            let (cut, piece, cell) = if along_a {
                (true, pa, ca)
            } else {
                (false, pb, cb)
            };
            if let (Piece::Face { dom, .. }, Cell::Face(u0, u1, v0, v1)) = (piece, cell)
                && !dom.meets_box(u0, u1, v0, v1)
            {
                continue;
            }
            let ba = if cut { cell_box(pa, &ca) } else { nd.ba };
            let bb = if cut { nd.bb } else { cell_box(pb, &cb) };
            let bd = bound(&ba, &bb, sense);
            if let Some(e) = &best {
                let hopeless = match sense {
                    Sense::Min => bd >= e.distance - eps,
                    Sense::Max => bd <= e.distance + eps,
                };
                if hopeless {
                    continue;
                }
            }
            heap.push(Node {
                key: key(bd),
                bound: bd,
                ia: nd.ia,
                ca,
                ib: nd.ib,
                cb,
                ba,
                bb,
            });
        }
    }
    best
}

/// [`crate::Kernel::min_distance`] and [`crate::Kernel::max_distance`] for the native kernel.
pub fn kernel_distance(
    k: &super::NativeKernel,
    a: &crate::DistanceTarget,
    b: &crate::DistanceTarget,
    sense: Sense,
) -> crate::Result<crate::DistanceResult> {
    use crate::{DistanceLocation, DistanceTarget, KernelError};
    type Side<'a> = (Vec<Piece<'a>>, Option<&'a Body>);
    // A point on a face (a face picked by a point on it), the nearest of a point and a face:
    // its projection on the surface is the point itself, inside the face's loops.
    if sense == Sense::Min
        && let Some(r) = point_on_face(k, a, b)
    {
        return Ok(r);
    }
    let side = |t: &DistanceTarget| -> crate::Result<Side<'_>> {
        Ok(match *t {
            DistanceTarget::Point(p) => {
                if !p.iter().all(|x| x.is_finite()) {
                    return Err(KernelError::InvalidParameter(
                        "a point that isn't finite".into(),
                    ));
                }
                (point_pieces(p), None)
            }
            DistanceTarget::Edge(id, e) => {
                let b = &k.entry(id)?.body;
                let ei = e.0 as usize;
                if ei >= b.edges.len() {
                    return Err(KernelError::InvalidParameter(format!(
                        "{id:?} has no edge {}",
                        e.0
                    )));
                }
                (edge_pieces(b, ei), Some(b))
            }
            DistanceTarget::Face(id, f) => {
                let en = k.entry(id)?;
                let fi = f.0 as usize;
                if fi >= en.body.faces.len() {
                    return Err(KernelError::InvalidParameter(format!(
                        "{id:?} has no face {}",
                        f.0
                    )));
                }
                (face_pieces(&en.body, fi, &en.polys()[fi]), Some(&*en.body))
            }
            DistanceTarget::Body(id) => {
                let en = k.entry(id)?;
                (body_pieces(&en.body, en.polys()), Some(&*en.body))
            }
        })
    };
    let (pa, ba) = side(a)?;
    let (pb, bb) = side(b)?;
    let Some(x) = extremum(&pa, &pb, sense) else {
        return Err(KernelError::EmptyResult(
            "nothing to measure from: no faces, edges or vertices".into(),
        ));
    };
    let loc = |w: Where, b: Option<&Body>| -> DistanceLocation {
        match w {
            Where::Point => DistanceLocation::Point,
            Where::Vertex(v) => {
                let ids = b.map(super::kernel::vertex_ids).unwrap_or_default();
                DistanceLocation::Vertex(crate::VertexId(
                    ids.get(v).copied().flatten().unwrap_or(v) as u64,
                ))
            }
            Where::Edge(e, t) => DistanceLocation::Edge(crate::EdgeId(e as u64), t),
            Where::Face(f, u, v) => DistanceLocation::Face(crate::FaceId(f as u64), u, v),
        }
    };
    Ok(crate::DistanceResult {
        distance: x.distance,
        a: x.a,
        b: x.b,
        a_at: loc(x.wa, ba),
        b_at: loc(x.wb, bb),
    })
}

/// [`kernel_distance`]'s nearest of a point and a face when the point lies on the face (to a
/// nanometre on its surface, and inside its loops, not on them): distance 0 there. None
/// otherwise (the general measure then).
fn point_on_face(
    k: &super::NativeKernel,
    a: &crate::DistanceTarget,
    b: &crate::DistanceTarget,
) -> Option<crate::DistanceResult> {
    use crate::{DistanceLocation, DistanceTarget};
    let (p, id, f, point_first) = match (*a, *b) {
        (DistanceTarget::Point(p), DistanceTarget::Face(id, f)) => (p, id, f, true),
        (DistanceTarget::Face(id, f), DistanceTarget::Point(p)) => (p, id, f, false),
        _ => return None,
    };
    if !p.iter().all(|x| x.is_finite()) {
        return None;
    }
    let en = k.entry(id).ok()?;
    let fi = f.0 as usize;
    let face = en.body.faces.get(fi)?;
    let (u, v) = face.surf.project(&p, None);
    let q = face.surf.eval(u, v);
    let d = (q - p).norm();
    if !(d <= 1e-9) {
        return None;
    }
    // (Clear of the loops by 1e-5, past the polygons' sampling error, else the general
    // measure: in_face's tolerance is the squared distance.)
    let polys = &en.polys()[fi];
    if super::classify::in_face(polys, &face.surf, P2::new(u, v), 1e-10)
        != super::classify::InFace::Inside
    {
        return None;
    }
    let (on_point, on_face) = (DistanceLocation::Point, DistanceLocation::Face(f, u, v));
    Some(if point_first {
        crate::DistanceResult {
            distance: d,
            a: p,
            b: q,
            a_at: on_point,
            b_at: on_face,
        }
    } else {
        crate::DistanceResult {
            distance: d,
            a: q,
            b: p,
            a_at: on_face,
            b_at: on_point,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::face_build::face_from_surface;
    use crate::native::surface::Surface;

    fn polys(b: &Body) -> Vec<Vec<Vec<P2>>> {
        b.faces
            .iter()
            .map(|f| crate::native::classify::face_polygons(b, f))
            .collect()
    }

    /// A sphere r 2 at (10, 2, 2) and the square x = 5, 0 ≤ y, z ≤ 5: nearest 3 apart at
    /// (8, 2, 2) and (5, 2, 2); farthest from the square's corners to the sphere's far side.
    #[test]
    fn sphere_and_square() {
        let sphere = face_from_surface(
            &Surface::Sphere {
                f: Frame {
                    o: P3::new(10.0, 2.0, 2.0),
                    ..Frame::world()
                },
                r: 2.0,
            },
            None,
        )
        .unwrap();
        let f = Frame {
            o: P3::new(5.0, 0.0, 0.0),
            x: V3::y(),
            y: V3::z(),
            z: V3::x(),
        };
        let square =
            face_from_surface(&Surface::Plane { f }, Some(((0.0, 5.0), (0.0, 5.0)))).unwrap();
        let (ps, pq) = (polys(&sphere), polys(&square));
        let (a, b) = (body_pieces(&sphere, &ps), body_pieces(&square, &pq));
        let m = extremum(&a, &b, Sense::Min).unwrap();
        assert!((m.distance - 3.0).abs() < 1e-9, "{m:?}");
        assert!(
            (m.a - P3::new(8.0, 2.0, 2.0)).norm() < 1e-6
                && (m.b - P3::new(5.0, 2.0, 2.0)).norm() < 1e-6,
            "{m:?}"
        );
        let x = extremum(&a, &b, Sense::Max).unwrap();
        // The corners (5, 5, 5) and (5, 5, 0) or (5, 0, 5)... farthest from the centre: |(5, 3, 3)|.
        let want = (25.0f64 + 9.0 + 9.0).sqrt() + 2.0;
        assert!((x.distance - want).abs() < 1e-9, "{x:?} want {want}");
        // A point inside the sphere: to its surface.
        let p = point_pieces(P3::new(10.5, 2.0, 2.0));
        let m = extremum(&p, &a, Sense::Min).unwrap();
        assert!((m.distance - 1.5).abs() < 1e-9, "{m:?}");
    }

    /// Through the kernel API: an edge (the side x = 3 of a rectangle at z = 5) and a point
    /// against a sphere r 2 at the origin; the rectangle's farthest corner.
    #[test]
    fn kernel_queries() {
        use crate::{DistanceLocation, DistanceTarget, EdgeId, FaceId, Kernel};
        let mut k = crate::native::NativeKernel::new();
        let s = k
            .face_from_bare_surface(
                &Surface::Sphere {
                    f: Frame::world(),
                    r: 2.0,
                },
                None,
            )
            .unwrap();
        let f = Frame {
            o: P3::new(0.0, 0.0, 5.0),
            ..Frame::world()
        };
        let r = k
            .face_from_bare_surface(&Surface::Plane { f }, Some(((3.0, 4.0), (-1.0, 1.0))))
            .unwrap();
        let rb = k.get(r).unwrap().clone();
        let e = (0..rb.edges.len())
            .find(|&e| (rb.edges[e].point(rb.edges[e].mid_t()).x - 3.0).abs() < 1e-12)
            .unwrap();
        let m = k
            .min_distance(
                &DistanceTarget::Edge(r, EdgeId(e as u64)),
                &DistanceTarget::Face(s, FaceId(0)),
            )
            .unwrap();
        assert!((m.distance - (34f64.sqrt() - 2.0)).abs() < 1e-9, "{m:?}");
        // On the edge; on the sphere at (2/√34)(3, 0, 5), which is on its seam (y = 0).
        assert!(
            matches!(m.a_at, DistanceLocation::Edge(..))
                && (m.b - P3::new(3.0, 0.0, 5.0) * (2.0 / 34f64.sqrt())).norm() < 1e-6,
            "{m:?}"
        );
        let m = k
            .min_distance(
                &DistanceTarget::Point(P3::new(0.0, 0.0, -7.0)),
                &DistanceTarget::Body(s),
            )
            .unwrap();
        assert!(
            (m.distance - 5.0).abs() < 1e-9 && (m.b - P3::new(0.0, 0.0, -2.0)).norm() < 1e-6,
            "{m:?}"
        );
        let x = k
            .max_distance(
                &DistanceTarget::Body(r),
                &DistanceTarget::Point(P3::origin()),
            )
            .unwrap();
        assert!(
            (x.distance - 42f64.sqrt()).abs() < 1e-9
                && matches!(x.a_at, DistanceLocation::Vertex(_)),
            "{x:?}"
        );
        assert!(
            k.min_distance(
                &DistanceTarget::Face(s, FaceId(7)),
                &DistanceTarget::Body(r)
            )
            .is_err()
        );
    }

    /// Two faces that cross: distance 0.
    #[test]
    fn crossing_faces() {
        let cyl = face_from_surface(
            &Surface::Cylinder {
                f: Frame::world(),
                r: 1.0,
            },
            Some(((0.0, TAU), (-1.0, 1.0))),
        )
        .unwrap();
        let plane = face_from_surface(
            &Surface::Plane { f: Frame::world() },
            Some(((-3.0, 3.0), (-3.0, 3.0))),
        )
        .unwrap();
        let (pc, pp) = (polys(&cyl), polys(&plane));
        let m = extremum(
            &face_pieces(&cyl, 0, &pc[0]),
            &face_pieces(&plane, 0, &pp[0]),
            Sense::Min,
        )
        .unwrap();
        assert!(m.distance < 1e-9, "{m:?}");
    }

    /// A point on a face measures 0 to it, located on it, by the shortcut and the general
    /// measure alike; a point off it, and one on its edge, by the general measure.
    #[test]
    fn a_point_on_a_face_is_on_it() {
        use crate::{DistanceLocation, DistanceTarget, Kernel, Primitive};
        let mut k = super::super::NativeKernel::new();
        let id = k
            .primitive(&Primitive::block(
                P3::new(0.0, 0.0, 0.0),
                P3::new(10.0, 8.0, 6.0),
            ))
            .unwrap()
            .bodies[0];
        let top = k
            .faces(id)
            .unwrap()
            .into_iter()
            .find(|f| (f.center.z - 6.0).abs() < 1e-9)
            .unwrap()
            .id;
        let face = DistanceTarget::Face(id, top);
        for (p, want) in [
            (P3::new(3.0, 4.0, 6.0), 0.0),
            (P3::new(3.0, 4.0, 7.5), 1.5),
            (P3::new(10.0, 4.0, 6.0), 0.0),
            (P3::new(12.0, 4.0, 6.0), 2.0),
        ] {
            for (a, b) in [
                (DistanceTarget::Point(p), face),
                (face, DistanceTarget::Point(p)),
            ] {
                let m = k.min_distance(&a, &b).unwrap();
                assert!((m.distance - want).abs() < 1e-9, "{p}: {m:?}");
                let at = if matches!(a, DistanceTarget::Face(..)) {
                    m.a_at
                } else {
                    m.b_at
                };
                assert!(
                    matches!(at, DistanceLocation::Face(f, ..) if f == top)
                        || !matches!(at, DistanceLocation::Face(..)),
                    "{p}: {m:?}"
                );
            }
        }
    }
}

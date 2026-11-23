//! Booleans of edges (wires and loose edges) with solids, sheets and other edges, as OCCT's
//! Boolean operations treat arguments of a lower dimension: the result is the edges' pieces,
//! split wherever they cross or touch the other argument, that lie inside or on it (common) or
//! outside it (cut); a fuse of two sets of edges keeps every piece of both (their shared
//! stretches from each, as OCCT's compound holds the shared edge twice).
//!
//! **Splitting.** Each edge is sampled. Where the side it is on (in, on, out) changes between two
//! samples, the change is found by bisection. Points where it only touches the other argument
//! split it too: crossings of a face's surface (a sign change of the distance, its foot inside
//! the face) and the closest approaches to other edges that reach them (crossings, and other
//! edges' ends lying on it).

use super::classify::{InFace, classify_point, face_polygons, in_face, periodic_candidates};
use super::curve::Curve;
use super::math::*;
use super::tol;
use super::topo::Body;
use crate::{BoolOp, PointClass};

/// An edge: its curve on [t0, t1].
#[derive(Debug, Clone)]
pub struct Piece {
    pub curve: Curve,
    pub t0: f64,
    pub t1: f64,
}

impl Piece {
    fn point(&self, t: f64) -> P3 {
        self.curve.point(t)
    }
    fn length(&self) -> f64 {
        self.curve
            .length(self.t0.min(self.t1), self.t0.max(self.t1))
    }
}

/// What the edges are combined with.
pub enum Other<'a> {
    /// A body: its solids (in, on, out), or its faces when it has no solids (on, out).
    Body(&'a Body),
    Edges(&'a [Piece]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    In,
    On,
    Out,
}

struct Ctx<'a> {
    other: &'a Other<'a>,
    /// The body's edges, pieces of one curve joined.
    body_edges: Vec<Piece>,
    polys: Vec<Vec<Vec<P2>>>,
    tol: f64,
    /// How near a face's boundary its side is decided exactly.
    band: f64,
}

impl Ctx<'_> {
    fn side(&self, p: &P3) -> Side {
        match self.other {
            Other::Body(b) if !b.solids.is_empty() => {
                match classify_point(b, &self.polys, p, self.tol) {
                    PointClass::Inside => Side::In,
                    PointClass::OnBoundary => Side::On,
                    PointClass::Outside => Side::Out,
                }
            }
            Other::Body(b) => {
                if (0..b.faces.len()).any(|fi| self.on_face(b, fi, p)) {
                    Side::On
                } else {
                    Side::Out
                }
            }
            Other::Edges(es) => {
                if es.iter().any(|e| dist_to(e, p) <= self.tol) {
                    Side::On
                } else {
                    Side::Out
                }
            }
        }
    }

    fn on_face(&self, b: &Body, fi: usize, p: &P3) -> bool {
        let f = &b.faces[fi];
        let (u, v) = f.surf.project(p, None);
        if (f.surf.eval(u, v) - p).norm() > self.tol {
            return false;
        }
        // Near its boundary, the side of the nearest boundary edge (the polygons are chords of
        // curved edges): material lies left of each coedge about the outward normal.
        let mut best: Option<(f64, usize, bool, f64)> = None;
        for c in f.loops.iter().flatten() {
            let ed = &b.edges[c.edge];
            if ed.degenerate {
                continue;
            }
            let (a, z) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
            let t = ed.curve.closest(p, a, z);
            let d = (ed.point(t) - p).norm();
            if best.is_none_or(|x| d < x.0) {
                best = Some((d, c.edge, c.fwd, t));
            }
        }
        if let Some((d, e, fwd, t)) = best {
            if d <= self.tol {
                return true;
            }
            if d < self.band {
                let ed = &b.edges[e];
                let q = ed.point(t);
                let (_, z) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
                let (a, _) = (ed.t[0].min(ed.t[1]), z);
                // Not at the edge's ends (a corner: the polygon decides).
                if (t - a).abs() > tol::REL * (1.0 + a.abs())
                    && (z - t).abs() > tol::REL * (1.0 + z.abs())
                {
                    let (qu, qv) = f.surf.project(&q, Some((u, v)));
                    let n = f.surf.normal(qu, qv) * if f.sense { 1.0 } else { -1.0 };
                    let tan = ed.curve.tangent(t) * if fwd { 1.0 } else { -1.0 };
                    return (p - q).dot(&n.cross(&tan)) > 0.0;
                }
            }
        }
        periodic_candidates(&f.surf, u, v, &self.polys[fi])
            .into_iter()
            .any(|uv| in_face(&self.polys[fi], &f.surf, uv, self.tol) != InFace::Outside)
    }
}

/// A box holding the edge.
fn piece_box(e: &Piece) -> Bbox {
    super::boolean::curve_bbox(&e.curve, e.t0, e.t1)
}

/// The distance from `p` to an edge.
fn dist_to(e: &Piece, p: &P3) -> f64 {
    let (a, b) = (e.t0.min(e.t1), e.t0.max(e.t1));
    let t = e.curve.closest(p, a, b);
    (e.point(t) - p).norm()
}

/// Parameters along an edge to look between: even steps and the curve's own sampling.
fn samples(e: &Piece, size: f64) -> Vec<f64> {
    let (a, b) = (e.t0, e.t1);
    let mut ts: Vec<f64> = (0..=200).map(|i| a + (b - a) * i as f64 / 200.0).collect();
    let (lo, hi) = (a.min(b), a.max(b));
    ts.extend(e.curve.sample_params(lo, hi, size * 1e-4, 0.05));
    if b < a {
        ts.sort_by(|x, y| y.total_cmp(x));
    } else {
        ts.sort_by(|x, y| x.total_cmp(y));
    }
    ts.dedup_by(|x, y| (*x - *y).abs() < 1e-14 * (1.0 + x.abs()));
    ts
}

/// Bisects for where `pred` changes between `lo` (where it is `at_lo`) and `hi`.
fn bisect(lo: f64, hi: f64, at_lo: bool, pred: impl Fn(f64) -> bool) -> f64 {
    let (lo, hi) = super::math::bisect(lo, hi, |m| pred(m) == at_lo);
    // The point on the "on" side.
    if at_lo { lo } else { hi }
}

/// Golden-section search for the smallest `f` on [a, b].
fn argmin(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    super::math::minimize(f, a, b)
}

/// How much tighter than the tolerance a stretch where two edges run within it is measured
/// again, to tell edges that coincide from edges that only touch tangentially.
pub(crate) const TIGHT: f64 = 1e-2;

/// Whether the stretch [ra, rb] of `e` within `tol` of an edge (distance `df`) is only a
/// tangential touch, and where it touches: two curves that coincide stay together at any
/// tolerance, but two that touch tangentially (an arc ending on a line it is tangent to) part
/// quadratically, so their stretch within the tolerance shrinks with its square root: at
/// [`TIGHT`] times it, to a tenth. A stretch that shrinks to less than half is a touch.
fn tangent_touch(e: &Piece, ra: f64, rb: f64, tol: f64, df: impl Fn(f64) -> f64) -> Option<f64> {
    let tm = argmin(ra.min(rb), ra.max(rb), &df);
    let tight = tol * TIGHT;
    if df(tm) > tight {
        return Some(tm);
    }
    let on = |t: f64| df(t) <= tight;
    let a2 = if on(ra) {
        ra
    } else {
        bisect(ra, tm, false, on)
    };
    let b2 = if on(rb) { rb } else { bisect(tm, rb, true, on) };
    let (l1, l2) = (
        (e.point(rb) - e.point(ra)).norm(),
        (e.point(b2) - e.point(a2)).norm(),
    );
    (l2 < 0.5 * l1).then_some(tm)
}

/// Where `e` meets one edge `o`: where it starts and stops running along it, and the points
/// where it only touches it (crossings, `o`'s ends on it, tangential touches).
fn edge_meets(
    e: &Piece,
    ts: &[f64],
    o: &Piece,
    tol: f64,
    size: f64,
    touches: bool,
    out: &mut Vec<f64>,
) {
    let df = |t: f64| dist_to(o, &e.point(t));
    let d: Vec<f64> = ts.iter().map(|&t| df(t)).collect();
    // Each run of samples within the tolerance: where it starts and stops (bisected), or, for
    // a tangential touch, the one point of touch.
    let mut i = 0;
    while i < ts.len() {
        if d[i] > tol {
            i += 1;
            continue;
        }
        let mut j = i;
        while j + 1 < ts.len() && d[j + 1] <= tol {
            j += 1;
        }
        let lo = (i > 0).then(|| bisect(ts[i - 1], ts[i], false, |t| df(t) <= tol));
        let hi = (j + 1 < ts.len()).then(|| bisect(ts[j], ts[j + 1], true, |t| df(t) <= tol));
        match tangent_touch(e, lo.unwrap_or(ts[i]), hi.unwrap_or(ts[j]), tol, df) {
            Some(t) => out.push(t),
            None => out.extend(lo.into_iter().chain(hi)),
        }
        i = j + 1;
    }
    if !touches {
        return;
    }
    for i in 0..ts.len() {
        let l = if i > 0 { d[i - 1] } else { f64::INFINITY };
        let r = if i + 1 < ts.len() {
            d[i + 1]
        } else {
            f64::INFINITY
        };
        if !(d[i] <= l && d[i] <= r) || d[i] > size * 0.05 {
            continue;
        }
        let t = argmin(ts[i.saturating_sub(1)], ts[(i + 1).min(ts.len() - 1)], df);
        let step = 1e-7 * (ts[ts.len() - 1] - ts[0]).abs();
        if df(t) <= tol && df(t - step) > tol && df(t + step) > tol {
            out.push(t);
        }
    }
}

/// Where `e`, running along some of `edges`, starts or stops running along them, or passes
/// from one to the next (at a vertex between them); not where one of two coincident edges
/// ends while the other goes on.
fn handovers(e: &Piece, ts: &[f64], edges: &[Piece], tol: f64, size: f64) -> Vec<f64> {
    let on = |o: &Piece, t: f64| dist_to(o, &e.point(t)) <= tol;
    // (t, entering, which)
    let mut events: Vec<(f64, bool)> = Vec::new();
    for o in edges {
        let st: Vec<bool> = ts.iter().map(|&t| on(o, t)).collect();
        for i in 1..ts.len() {
            if st[i] != st[i - 1] {
                events.push((bisect(ts[i - 1], ts[i], st[i - 1], |t| on(o, t)), st[i]));
            }
        }
    }
    let fwd = e.t1 >= e.t0;
    events.sort_by(|a, b| {
        if fwd {
            a.0.total_cmp(&b.0)
        } else {
            b.0.total_cmp(&a.0)
        }
    });
    let any = |t: f64| edges.iter().any(|o| on(o, t));
    let gap = size * 1e-5;
    let mut out = Vec::new();
    let mut i = 0;
    while i < events.len() {
        let mut j = i + 1;
        while j < events.len()
            && (e.point(events[j].0) - e.point(events[i].0)).norm() <= gap
            && (events[j].0 - events[i].0).abs() <= 1e-3 * (e.t1 - e.t0).abs()
        {
            j += 1;
        }
        let cluster = &events[i..j];
        let (enters, leaves) = (
            cluster.iter().filter(|x| x.1).count(),
            cluster.iter().filter(|x| !x.1).count(),
        );
        let (a, b) = (cluster[0].0, cluster[cluster.len() - 1].0);
        let d = tol::DIFF_STEP * (e.t1 - e.t0);
        if any(a - d) != any(b + d) || (enters > 0 && leaves > 0) {
            out.push((a + b) / 2.0);
        }
        i = j;
    }
    out
}

/// Where `e` meets face `fi`: where it starts and stops lying on it, and where it crosses it.
fn face_meets(cx: &Ctx, b: &Body, fi: usize, e: &Piece, ts: &[f64], out: &mut Vec<f64>) {
    let f = &b.faces[fi];
    let on = |t: f64| cx.on_face(b, fi, &e.point(t));
    let ons: Vec<bool> = ts.iter().map(|&t| on(t)).collect();
    for i in 1..ts.len() {
        if ons[i] != ons[i - 1] {
            out.push(bisect(ts[i - 1], ts[i], ons[i - 1], on));
        }
    }
    // The distance to the face's surface along its normal, with a sign.
    let mut hint = None;
    let g: Vec<(f64, P3)> = ts
        .iter()
        .map(|&t| {
            let p = e.point(t);
            let (u, v) = f.surf.project(&p, hint);
            hint = Some((u, v));
            let q = f.surf.eval(u, v);
            ((p - q).dot(&f.surf.normal(u, v)), q)
        })
        .collect();
    let gd = |t: f64| {
        let p = e.point(t);
        let (u, v) = f.surf.project(&p, None);
        (p - f.surf.eval(u, v)).dot(&f.surf.normal(u, v))
    };
    for i in 1..ts.len() {
        let (g0, g1) = (g[i - 1].0, g[i].0);
        if ons[i - 1] || ons[i] || g0 * g1 >= 0.0 {
            continue;
        }
        // The feet must move with the curve (not jump across the surface).
        if (g[i - 1].1 - g[i].1).norm()
            > 10.0 * (e.point(ts[i]) - e.point(ts[i - 1])).norm() + cx.tol
        {
            continue;
        }
        let (lo, hi) = super::math::bisect(ts[i - 1], ts[i], |m| gd(m) * g0 > 0.0);
        let t = (lo + hi) / 2.0;
        if on(t) {
            out.push(t);
        }
    }
    // Where it touches the face without crossing it (a smallest distance reaching it).
    for i in 1..ts.len().saturating_sub(1) {
        let (a, m, z) = (g[i - 1].0.abs(), g[i].0.abs(), g[i + 1].0.abs());
        if ons[i] || !(m <= a && m <= z) || g[i - 1].0 * g[i + 1].0 < 0.0 {
            continue;
        }
        let t = argmin(ts[i - 1], ts[i + 1], |t| gd(t).abs());
        if gd(t).abs() <= cx.tol * 10.0
            && on(t)
            && !on(t - tol::DIFF_STEP * (ts[i + 1] - ts[i - 1]))
            && !on(t + tol::DIFF_STEP * (ts[i + 1] - ts[i - 1]))
        {
            out.push(t);
        }
    }
}

/// The parameters where `e` is split against the other argument.
fn splits(cx: &Ctx, e: &Piece, size: f64) -> Vec<f64> {
    let ts = samples(e, size);
    let mut out: Vec<f64> = Vec::new();
    // Only edges whose boxes come within the tolerance of this one's can meet it.
    let near = {
        let bb = piece_box(e).grow(cx.tol);
        move |o: &Piece| piece_box(o).overlaps(&bb)
    };
    match cx.other {
        Other::Body(b) => {
            for fi in 0..b.faces.len() {
                let n = out.len();
                face_meets(cx, b, fi, e, &ts, &mut out);
                if crate::native::env_flag!("CADRS_EBOP_DEBUG") && out.len() > n {
                    eprintln!(
                        "face {fi}: splits {:?}",
                        out[n..].iter().map(|&t| e.point(t)).collect::<Vec<_>>()
                    );
                }
            }
            // Where it starts or stops running along one of the body's edges (pieces of one
            // curve taken together: our seams and closed edges have vertices OCCT's don't).
            let close: Vec<Piece> = cx.body_edges.iter().filter(|o| near(o)).cloned().collect();
            out.extend(handovers(e, &ts, &close, cx.tol, size));
        }
        Other::Edges(es) => {
            for o in es.iter().filter(|o| near(o)) {
                edge_meets(e, &ts, o, cx.tol, size, true, &mut out);
            }
        }
    }
    // In order along the edge, without repeats or the ends.
    let (t0, t1) = (e.t0, e.t1);
    let fwd = t1 >= t0;
    let gap = size * 1e-5;
    out.retain(|&t| {
        (e.point(t) - e.point(t0)).norm() > gap
            && (e.point(t) - e.point(t1)).norm() > gap
            && (t - t0) * (t1 - t) > 0.0
    });
    out.sort_by(|a, b| if fwd { a.total_cmp(b) } else { b.total_cmp(a) });
    let mut kept: Vec<f64> = Vec::new();
    for t in out {
        if kept
            .last()
            .is_none_or(|&l| (e.point(l) - e.point(t)).norm() > gap)
        {
            kept.push(t);
        }
    }
    kept
}

/// A body's edges with the pieces of one curve that meet end to end joined.
fn joined_edges(b: &Body, tol: f64) -> Vec<Piece> {
    let mut out: Vec<Piece> = Vec::new();
    for ed in b.edges.iter().filter(|x| !x.degenerate) {
        let (t0, t1) = (ed.t[0].min(ed.t[1]), ed.t[0].max(ed.t[1]));
        if let Some(p) = out.iter_mut().find(|p| {
            p.curve == ed.curve
                && ((p.t1 - t0).abs() < tol::ROUNDOFF_FINE
                    || (t1 - p.t0).abs() < tol::ROUNDOFF_FINE)
        }) {
            p.t0 = p.t0.min(t0);
            p.t1 = p.t1.max(t1);
            continue;
        }
        let _ = tol;
        out.push(Piece {
            curve: ed.curve.clone(),
            t0,
            t1,
        });
    }
    out
}

/// The pieces of `edges` split against `other`, each with its side.
fn pieces(edges: &[Piece], other: &Other, size: f64, tol: f64) -> Vec<(Piece, Side)> {
    let polys = match other {
        Other::Body(b) => b.faces.iter().map(|f| face_polygons(b, f)).collect(),
        Other::Edges(_) => Vec::new(),
    };
    let body_edges = match other {
        Other::Body(b) => joined_edges(b, tol),
        Other::Edges(_) => Vec::new(),
    };
    let cx = Ctx {
        other,
        body_edges,
        polys,
        tol,
        band: size * 1e-3,
    };
    let mut out = Vec::new();
    for e in edges {
        let mut cuts = vec![e.t0];
        cuts.extend(splits(&cx, e, size));
        cuts.push(e.t1);
        for w in cuts.windows(2) {
            let p = Piece {
                curve: e.curve.clone(),
                t0: w[0],
                t1: w[1],
            };
            if p.length() <= size * 1e-5 {
                continue;
            }
            let side = cx.side(&p.point((w[0] + w[1]) / 2.0));
            out.push((p, side));
        }
    }
    out
}

/// The size of the model (for sampling and tolerances).
fn size_of(edges: &[Piece], other: &Other) -> f64 {
    let mut lo = P3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let mut hi = -lo;
    let mut add = |p: P3| {
        lo = lo.inf(&p);
        hi = hi.sup(&p);
    };
    let mut add_edge = |e: &Piece| {
        for i in 0..=8 {
            add(e.point(e.t0 + (e.t1 - e.t0) * i as f64 / 8.0));
        }
    };
    edges.iter().for_each(&mut add_edge);
    match other {
        Other::Edges(es) => es.iter().for_each(&mut add_edge),
        Other::Body(b) => b.verts.iter().for_each(|v| add(v.p)),
    }
    (hi - lo).norm().max(tol::ROUNDOFF)
}

/// `edges` op `other`: the pieces of the edges (common: inside or on the other argument; cut:
/// outside it; union with edges: both sets' pieces, shared stretches once). None for a union
/// with a solid or sheet (arguments of different dimensions).
pub fn edge_boolean(
    op: BoolOp,
    edges: &[Piece],
    other: &Other,
    keep_shared: bool,
) -> Option<Vec<Piece>> {
    let size = size_of(edges, other);
    let tol = (size * 1e-10).max(1e-7);
    let mine = pieces(edges, other, size, tol);
    Some(match op {
        BoolOp::Intersect => mine
            .into_iter()
            .filter(|(_, s)| *s != Side::Out)
            .map(|(p, _)| p)
            .collect(),
        BoolOp::Subtract => mine
            .into_iter()
            .filter(|(_, s)| *s == Side::Out)
            .map(|(p, _)| p)
            .collect(),
        BoolOp::Union => {
            let Other::Edges(theirs) = other else {
                return None;
            };
            let mut out: Vec<Piece> = mine.into_iter().map(|(p, _)| p).collect();
            let back = Other::Edges(edges);
            // Both sets' pieces; a stretch they share is there twice when `keep_shared` (OCCT's
            // result holds the shared edge in both wires: its length counts twice, its edge once).
            out.extend(
                pieces(theirs, &back, size, tol)
                    .into_iter()
                    .filter(|(_, s)| keep_shared || *s == Side::Out)
                    .map(|(p, _)| p),
            );
            out
        }
    })
}

/// The number of distinct vertices of a set of edges (their ends, merged within `tol`).
pub fn vertex_count(edges: &[(P3, P3)], tol: f64) -> usize {
    let mut pts: Vec<P3> = Vec::new();
    for (a, b) in edges {
        for p in [a, b] {
            if !pts.iter().any(|q| (q - p).norm() <= tol) {
                pts.push(*p);
            }
        }
    }
    pts.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An arc ending on a line it is tangent to stays within the tolerance of the line for a
    /// stretch beyond its end (√(2·r·tol)): a touch at the end, not an overlap, so neither edge
    /// is split there but at the point of touch.
    #[test]
    fn tangent_arc_touches_a_line() {
        let line = Piece {
            curve: Curve::Line {
                o: P3::new(-10.0, 0.0, 0.0),
                d: V3::x(),
            },
            t0: 0.0,
            t1: 20.0,
        };
        let arc = Piece {
            curve: Curve::Circle {
                c: P3::new(0.0, 500.0, 0.0),
                x: V3::x(),
                y: V3::y(),
                r: 500.0,
            },
            t0: -PI / 2.0,
            t1: -PI / 2.0 + 0.17,
        };
        let r = edge_boolean(
            BoolOp::Union,
            std::slice::from_ref(&line),
            &Other::Edges(std::slice::from_ref(&arc)),
            false,
        )
        .unwrap();
        // The line in two at the touch, the arc whole.
        assert_eq!(r.len(), 3, "{r:?}");
        let on_line = r
            .iter()
            .filter(|p| matches!(p.curve, Curve::Line { .. }))
            .count();
        assert_eq!(on_line, 2);
        // Two lines running together still overlap end to end.
        let half = Piece {
            curve: Curve::Line {
                o: P3::new(-10.0, 0.0, 0.0),
                d: V3::x(),
            },
            t0: 5.0,
            t1: 30.0,
        };
        let common = edge_boolean(
            BoolOp::Intersect,
            std::slice::from_ref(&line),
            &Other::Edges(std::slice::from_ref(&half)),
            false,
        )
        .unwrap();
        assert_eq!(common.len(), 1);
        assert!((common[0].length() - 15.0).abs() < 1e-6, "{common:?}");
    }
}

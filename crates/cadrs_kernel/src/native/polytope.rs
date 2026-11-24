//! Convex polytopes: the solids that are where half-spaces meet, kept as their faces' polygons.
//!
//! Cutting one by a plane is exact and takes no search: each face's polygon is clipped by the
//! plane, and the points left on the plane, in order round it, are the new face. So where a
//! solid is known to be the meeting of half-spaces (a box's walls moved in for a shell's
//! cavity, a tool that is planes only), it is made here in closed form, where the general
//! boolean of the body with one half-space after another intersects, splits and classifies
//! every face each time.
//!
//! Two polytopes' common part is one cut by the other's planes, and one less the other is its
//! faces less the common part's, with the common part's own faces turned inside out: each a
//! polygon found from the two polygons' corners ([`boolean`]). A box shelled, chamfered or cut
//! by another box needs none of the general boolean's intersections, splits and rays.

use super::boolean::BoolOut;
use super::curve::Curve;
use super::math::*;
use super::surface::Surface;
use super::topo::{Body, Coedge, Face, Shell, Solid};

/// A face of a polytope: its plane (a point of it and its unit normal, out of the solid) and its
/// corners, counter-clockwise about the normal.
#[derive(Debug, Clone)]
struct Flat {
    o: P3,
    n: V3,
    ring: Vec<P3>,
    /// The face it was made from: the polytope's own (0, its face there) as it was read, or
    /// what a cut says it is of ([`Polytope::clip_from`]).
    from: (usize, usize),
}

/// What a face cut by a plane that is no body's face is from.
const NOBODY: (usize, usize) = (usize::MAX, 0);

/// A convex polytope (see the module docs).
#[derive(Debug, Clone)]
pub(crate) struct Polytope {
    faces: Vec<Flat>,
}

/// The polygon's corners less those within `eps` of the one before (round the ring).
fn distinct(ring: &mut Vec<P3>, eps: f64) {
    ring.dedup_by(|a, b| (*a - *b).norm() <= eps);
    while ring.len() > 1 && (ring[0] - ring[ring.len() - 1]).norm() <= eps {
        ring.pop();
    }
}

/// Twice the polygon's area about `n`.
fn area2(ring: &[P3], n: &V3) -> f64 {
    let o = ring[0];
    (1..ring.len().saturating_sub(1))
        .map(|i| (ring[i] - o).cross(&(ring[i + 1] - o)).dot(n))
        .sum()
}

impl Polytope {
    /// The box `bb`.
    pub(crate) fn of_box(bb: &Bbox) -> Self {
        let (a, b) = (bb.min, bb.max);
        let p = |i: usize| {
            P3::new(
                if i & 1 == 0 { a.x } else { b.x },
                if i & 2 == 0 { a.y } else { b.y },
                if i & 4 == 0 { a.z } else { b.z },
            )
        };
        // (Each face's corners counter-clockwise seen from outside.)
        let sides: [([usize; 4], V3); 6] = [
            ([0, 4, 6, 2], -V3::x()),
            ([1, 3, 7, 5], V3::x()),
            ([0, 1, 5, 4], -V3::y()),
            ([2, 6, 7, 3], V3::y()),
            ([0, 2, 3, 1], -V3::z()),
            ([4, 5, 7, 6], V3::z()),
        ];
        Polytope {
            faces: sides
                .iter()
                .enumerate()
                .map(|(i, (c, n))| Flat {
                    o: p(c[0]),
                    n: *n,
                    ring: c.iter().map(|&i| p(i)).collect(),
                    from: (0, i),
                })
                .collect(),
        }
    }

    /// The body as a polytope, where it is one: a single solid of flat faces with straight edges,
    /// one loop each, every vertex behind every face's plane (within `eps`).
    pub(crate) fn of_body(b: &Body, eps: f64) -> Option<Self> {
        if b.solids.len() != 1 || b.shells.len() != 1 || !b.shells[0].closed || b.faces.len() < 4 {
            return None;
        }
        let mut faces = Vec::with_capacity(b.faces.len());
        for (fi, f) in b.faces.iter().enumerate() {
            let Surface::Plane { f: fr } = &f.surf else {
                return None;
            };
            let [lp] = f.loops.as_slice() else {
                return None;
            };
            let mut ring = Vec::with_capacity(lp.len());
            for c in lp {
                let e = &b.edges[c.edge];
                if e.degenerate || !matches!(e.curve, Curve::Line { .. }) {
                    return None;
                }
                ring.push(b.verts[e.v[usize::from(!c.fwd)]].p);
            }
            let n = fr.z.normalize() * if f.sense { 1.0 } else { -1.0 };
            faces.push(Flat {
                o: ring[0],
                n,
                ring,
                from: (0, fi),
            });
        }
        let convex = faces
            .iter()
            .all(|f| b.verts.iter().all(|v| (v.p - f.o).dot(&f.n) <= eps));
        convex.then_some(Polytope { faces })
    }

    /// Each face's plane: a point of it and its unit normal, out of the solid. A polytope read
    /// from a body ([`Self::of_body`]) has them in the body's face order until it is cut.
    pub(crate) fn planes(&self) -> Vec<(P3, V3)> {
        self.faces.iter().map(|f| (f.o, f.n)).collect()
    }

    /// The polytope's volume: a third of each face's area times its plane's height over the
    /// origin, summed.
    pub(crate) fn volume(&self) -> f64 {
        self.faces
            .iter()
            .map(|f| f.o.coords.dot(&f.n) * area2(&f.ring, &f.n) / 6.0)
            .sum()
    }

    /// Cuts the polytope by the plane through `o` with normal `n`, keeping what lies behind it
    /// (against the normal); points within `eps` of the plane are on it. False when nothing with
    /// a volume is left (the polytope is then not to be used).
    pub(crate) fn clip(&mut self, o: P3, n: V3, eps: f64) -> bool {
        self.clip_from(o, n, eps, NOBODY)
    }

    /// [`Self::clip`], the face the cut makes said to be `from` that face of a body.
    fn clip_from(&mut self, o: P3, n: V3, eps: f64, from: (usize, usize)) -> bool {
        let n = n.normalize();
        let side = |p: &P3| {
            let d = (p - o).dot(&n);
            if d.abs() <= eps { 0.0 } else { d }
        };
        if self
            .faces
            .iter()
            .all(|f| f.ring.iter().all(|p| side(p) <= 0.0))
        {
            return true;
        }
        // The points left on the plane: the new face's corners.
        let mut on: Vec<P3> = Vec::new();
        let mut kept = Vec::with_capacity(self.faces.len() + 1);
        for f in self.faces.drain(..) {
            let m = f.ring.len();
            let mut ring = Vec::with_capacity(m + 2);
            for i in 0..m {
                let (p, q) = (f.ring[i], f.ring[(i + 1) % m]);
                let (dp, dq) = (side(&p), side(&q));
                if dp <= 0.0 {
                    ring.push(p);
                    if dp == 0.0 {
                        on.push(p);
                    }
                }
                if dp * dq < 0.0 {
                    let x = p + (q - p) * (dp / (dp - dq));
                    ring.push(x);
                    on.push(x);
                }
            }
            distinct(&mut ring, eps);
            if ring.len() >= 3 && area2(&ring, &f.n) > eps * eps {
                kept.push(Flat {
                    o: f.o,
                    n: f.n,
                    ring,
                    from: f.from,
                });
            }
        }
        // In order round the plane's normal about their middle (the cut of a convex solid is
        // convex).
        let mut cap: Vec<P3> = Vec::new();
        for p in on {
            if cap.iter().all(|q| (q - p).norm() > eps) {
                cap.push(p);
            }
        }
        if cap.len() >= 3 {
            let mid = P3::from(cap.iter().map(|p| p.coords).sum::<V3>() / cap.len() as f64);
            let x = any_perpendicular(&n).normalize();
            let y = n.cross(&x);
            let angle = |p: &P3| (p - mid).dot(&y).atan2((p - mid).dot(&x));
            cap.sort_by(|a, b| angle(a).total_cmp(&angle(b)));
            if area2(&cap, &n) > eps * eps {
                kept.push(Flat {
                    o,
                    n,
                    ring: cap,
                    from,
                });
            }
        }
        self.faces = kept;
        self.faces.len() >= 4
    }

    /// The polytope as a solid body: a flat face a polygon, their corners and edges shared.
    /// None when its faces don't close (a cut left a sliver thinner than `eps`).
    pub(crate) fn body(&self, eps: f64) -> Option<Body> {
        let pieces: Vec<Piece> = self.faces.iter().map(Piece::of).collect();
        assemble(&pieces, &[], eps).map(|(body, _)| body)
    }
}

/// A flat face of a solid being put together: its plane (a point and the unit normal out of
/// the solid), its loops (the outer first, counter-clockwise about the normal; holes
/// clockwise), and the body's face it is from.
struct Piece {
    o: P3,
    n: V3,
    loops: Vec<Vec<P3>>,
    from: (usize, usize),
    /// The surface it lies on and its sense there, where it keeps a body's face's own (the
    /// same plane in the frame that face had); else a plane made for it.
    on: Option<(Surface, bool)>,
}

impl Piece {
    fn of(f: &Flat) -> Self {
        Piece {
            o: f.o,
            n: f.n,
            loops: vec![f.ring.clone()],
            from: f.from,
            on: None,
        }
    }

    /// Twice its area.
    fn area2(&self) -> f64 {
        self.loops.iter().map(|l| area2(l, &self.n)).sum()
    }
}

/// The solid body the flat faces `pieces` bound, their corners and edges shared, with the
/// volume they enclose: one solid, with its voids if some of the faces close on themselves
/// facing inward, or a solid for each group of faces that closes facing outward. None when the
/// faces don't close (an edge not used once each way), or close into something else. An edge
/// along one of `lines` (the straight edges of the bodies the faces are from, each with its
/// ends' parameters) lies on that line as it is, in its own parameters; any other on a line of
/// its own.
fn assemble(pieces: &[Piece], lines: &[(&Curve, [f64; 2])], eps: f64) -> Option<(Body, f64)> {
    let mut b = Body::default();
    // Each edge by its two corners, with the face that made it and the ways it has been run.
    let mut edges: std::collections::HashMap<(usize, usize), (usize, usize, [bool; 2])> =
        Default::default();
    // (The faces joined along each edge, for the groups that close.)
    let mut group: Vec<usize> = (0..pieces.len()).collect();
    // The line through two points and their parameters on it, the lower first: an edge's own
    // line where the points lie on one (its own ends' parameters where they are its ends), else
    // a line from the first to the second by its length.
    let carrier = |p: P3, q: P3| -> (Curve, [f64; 2], bool) {
        for (line, ends) in lines {
            let Curve::Line { o, d } = line else { continue };
            let dd = d.norm_squared();
            let at = |x: &P3| {
                let t = (x - o).dot(d) / dd;
                let end = ends.iter().find(|e| (*e - t).abs() * dd.sqrt() <= eps);
                end.copied().unwrap_or(t)
            };
            let (tp, tq) = (at(&p), at(&q));
            if (o + d * tp - p).norm() <= eps && (o + d * tq - q).norm() <= eps {
                return if tp < tq {
                    ((*line).clone(), [tp, tq], true)
                } else {
                    ((*line).clone(), [tq, tp], false)
                };
            }
        }
        let len = (q - p).norm();
        let along = Curve::Line {
            o: p,
            d: (q - p) / len,
        };
        (along, [0.0, len], true)
    };
    for (fi, f) in pieces.iter().enumerate() {
        let (surf, sense) = f.on.clone().unwrap_or_else(|| {
            let plane = Surface::Plane {
                f: Frame::from_z(f.o, f.n),
            };
            (plane, true)
        });
        let mut loops = Vec::with_capacity(f.loops.len());
        for ring in &f.loops {
            let vs: Vec<usize> = ring
                .iter()
                .map(
                    |p| match b.verts.iter().position(|v| (v.p - p).norm() <= eps) {
                        Some(i) => i,
                        None => b.add_vertex(*p),
                    },
                )
                .collect();
            let m = vs.len();
            let mut lp = Vec::with_capacity(m);
            for i in 0..m {
                let (v, w) = (vs[i], vs[(i + 1) % m]);
                if v == w {
                    return None;
                }
                let (edge, fwd) = match edges.get_mut(&(v.min(w), v.max(w))) {
                    Some((e, made_by, run)) => {
                        // Run along the edge or against it: each once.
                        let fwd = b.edges[*e].v == [v, w];
                        if std::mem::replace(&mut run[usize::from(fwd)], true) {
                            return None;
                        }
                        let (x, y) = (
                            super::math::set_of(&mut group, *made_by),
                            super::math::set_of(&mut group, fi),
                        );
                        group[x] = y;
                        (*e, fwd)
                    }
                    None => {
                        let (curve, t, along) = carrier(b.verts[v].p, b.verts[w].p);
                        let (from, to) = if along { (v, w) } else { (w, v) };
                        let e = b.add_edge(curve, t[0], t[1], from, to);
                        let mut run = [false; 2];
                        run[usize::from(along)] = true;
                        edges.insert((v.min(w), v.max(w)), (e, fi, run));
                        (e, along)
                    }
                };
                let ed = &b.edges[edge];
                let pc = super::pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None);
                lp.push(Coedge { edge, fwd, pc });
            }
            loops.push(lp);
        }
        b.faces.push(Face { surf, sense, loops });
    }
    // Closed: every edge used once each way.
    let mut uses = vec![0u8; b.edges.len()];
    for c in b.faces.iter().flat_map(|f| f.loops.iter().flatten()) {
        uses[c.edge] += 1;
    }
    if uses.iter().any(|&u| u != 2) {
        return None;
    }
    // Each group of faces joined by edges is a shell, by the volume it bounds a solid's outside
    // or a void: a third of each face's area times its plane's height over the origin, summed.
    let roots: Vec<usize> = (0..pieces.len())
        .map(|f| super::math::set_of(&mut group, f))
        .collect();
    let mut shells: Vec<(usize, Vec<usize>, f64)> = Vec::new();
    for (fi, f) in pieces.iter().enumerate() {
        let v = f.o.coords.dot(&f.n) * f.area2() / 6.0;
        match shells.iter_mut().find(|s| s.0 == roots[fi]) {
            Some(s) => {
                s.1.push(fi);
                s.2 += v;
            }
            None => shells.push((roots[fi], vec![fi], v)),
        }
    }
    let volume: f64 = shells.iter().map(|s| s.2).sum();
    let outer = shells.iter().filter(|s| s.2 > 0.0).count();
    if outer == 0 || (outer > 1 && outer < shells.len()) {
        return None;
    }
    // (A solid's outer shell first, then its voids.)
    shells.sort_by(|x, y| y.2.total_cmp(&x.2));
    for (_, faces, _) in &shells {
        b.shells.push(Shell {
            faces: faces.clone(),
            closed: true,
        });
    }
    if outer == 1 {
        b.solids.push(Solid {
            shells: (0..shells.len()).collect(),
        });
    } else {
        for s in 0..shells.len() {
            b.solids.push(Solid { shells: vec![s] });
        }
    }
    Some((b, volume))
}

/// The polygon `p` less the polygon `q` inside it, both convex, in one plane and
/// counter-clockwise about `n`: the faces left, each its loops (the outer first). `q` may run
/// along sides of `p`: where it does the two sides go, and what is left is polygons of their
/// own; where it touches no side it is a hole in the one face. None where the two only touch
/// in a point (what is left pinches there).
fn less(p: &[P3], q: &[P3], n: &V3, eps: f64) -> Option<Vec<Vec<Vec<P3>>>> {
    // A polygon's sides in order, each cut at the other's corners on it.
    let cut = |ring: &[P3], other: &[P3]| -> Vec<(P3, P3)> {
        let mut sides = Vec::with_capacity(ring.len() + other.len());
        for i in 0..ring.len() {
            let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
            let d = b - a;
            let len = d.norm();
            let mut at: Vec<f64> = other
                .iter()
                .filter_map(|x| {
                    let t = (x - a).dot(&d) / (len * len);
                    let off = (a + d * t - x).norm();
                    (off <= eps && t * len > eps && (1.0 - t) * len > eps).then_some(t)
                })
                .collect();
            at.sort_by(f64::total_cmp);
            let mut from = a;
            for t in at {
                let x = a + d * t;
                if (x - from).norm() > eps {
                    sides.push((from, x));
                    from = x;
                }
            }
            sides.push((from, b));
        }
        sides
    };
    let same = |a: &P3, b: &P3| (a - b).norm() <= eps;
    let mut left = cut(p, q);
    // (q's sides the other way round: what is left lies outside it.)
    let mut inner: Vec<(P3, P3)> = cut(q, p).into_iter().map(|(a, b)| (b, a)).collect();
    // A side of p with q's side along it goes, with that side of q.
    let mut touching = left.len() != p.len() || inner.len() != q.len();
    left.retain(
        |(a, b)| match inner.iter().position(|(c, d)| same(a, d) && same(b, c)) {
            Some(i) => {
                inner.swap_remove(i);
                touching = true;
                false
            }
            None => true,
        },
    );
    touching |= q.iter().any(|x| p.iter().any(|y| same(x, y)));
    left.extend(inner);
    // The sides left, chained end to start into loops.
    let mut loops: Vec<Vec<P3>> = Vec::new();
    while let Some((start, mut end)) = left.pop() {
        let mut ring = vec![start];
        while !same(&end, &start) {
            let mut next = left.iter().enumerate().filter(|(_, (a, _))| same(a, &end));
            let (i, _) = next.next()?;
            if next.next().is_some() {
                return None;
            }
            let (a, b) = left.swap_remove(i);
            ring.push(a);
            end = b;
        }
        // (No corner twice: a loop through one point twice pinches there.)
        if (0..ring.len()).any(|i| ring[i + 1..].iter().any(|x| same(x, &ring[i]))) {
            return None;
        }
        loops.push(ring);
    }
    let area = |l: &Vec<P3>| area2(l, n);
    if touching {
        // Every loop a polygon of its own.
        loops
            .iter()
            .all(|l| area(l) > eps * eps)
            .then(|| loops.into_iter().map(|l| vec![l]).collect())
    } else {
        // p's own loop with q's as its hole.
        let [a, b] = loops.as_slice() else {
            return None;
        };
        let (outer, hole) = if area(a) > 0.0 { (a, b) } else { (b, a) };
        (area(outer) > eps * eps && area(hole) < -eps * eps)
            .then(|| vec![vec![outer.clone(), hole.clone()]])
    }
}

/// The boolean of two bodies that are convex polytopes, in closed form: their common part
/// (`subtract` false) or the first less the second, with the faces of the two bodies each of
/// its faces is from (as the general boolean gives them: 0 the first body, 1 the second).
/// Points within `eps` are one.
///
/// None where the bodies are not both polytopes, or the answer is not plainly this one's to
/// give, and the general boolean's: a corner of one near a plane of the other without being on
/// it (a face a hair off another's), bodies that share no volume, a result that pinches in a
/// point or along an edge, or faces that would not close into the volume the two bodies'
/// volumes say.
pub(crate) fn boolean(a: &Body, b: &Body, subtract: bool, eps: f64) -> Option<BoolOut> {
    let exact =
        |x: &Body| x.verts.iter().all(|v| v.tol <= eps) && x.edges.iter().all(|e| e.tol <= eps);
    if !exact(a) || !exact(b) {
        return None;
    }
    let (pa, pb) = (Polytope::of_body(a, eps)?, Polytope::of_body(b, eps)?);
    // Every corner of each plainly on a plane of the other or plainly off it.
    let size = a.bbox().diagonal().max(b.bbox().diagonal());
    let near = (1e-4 * size).max(100.0 * eps);
    let plain = |x: &Polytope, y: &Polytope| {
        x.faces.iter().flat_map(|f| &f.ring).all(|p| {
            y.faces.iter().all(|g| {
                let d = (p - g.o).dot(&g.n).abs();
                d <= eps || d >= near
            })
        })
    };
    if !plain(&pa, &pb) || !plain(&pb, &pa) {
        return None;
    }
    // The common part: the first cut by each of the second's planes.
    let mut common = pa.clone();
    for (gi, g) in pb.faces.iter().enumerate() {
        if !common.clip_from(g.o, g.n, eps, (1, gi)) {
            return None;
        }
    }
    let shared = common.volume();
    if !(shared > 1e-9 * size * size * size) {
        return None;
    }
    // Each face on its body's face's own surface (the second's turned over where the first is
    // less it).
    let on = |from: (usize, usize)| {
        let (body, turned) = if from.0 == 0 {
            (a, false)
        } else {
            (b, subtract)
        };
        body.faces
            .get(from.1)
            .map(|f| (f.surf.clone(), f.sense != turned))
    };
    let (mut pieces, want): (Vec<Piece>, f64) = if subtract {
        let mut pieces = Vec::with_capacity(pa.faces.len() + common.faces.len());
        for f in &pa.faces {
            // The first's face less the common part's face on it, if it has one there.
            match common.faces.iter().find(|c| c.from == f.from) {
                None => pieces.push(Piece::of(f)),
                Some(c) => {
                    for loops in less(&f.ring, &c.ring, &f.n, eps)? {
                        pieces.push(Piece {
                            o: f.o,
                            n: f.n,
                            loops,
                            from: f.from,
                            on: None,
                        });
                    }
                }
            }
        }
        // The common part's faces on the second's planes, turned to face into it.
        for c in common.faces.iter().filter(|c| c.from.0 == 1) {
            pieces.push(Piece {
                o: c.o,
                n: -c.n,
                loops: vec![c.ring.iter().rev().copied().collect()],
                from: c.from,
                on: None,
            });
        }
        (pieces, pa.volume() - shared)
    } else {
        (common.faces.iter().map(Piece::of).collect(), shared)
    };
    if pieces.is_empty() {
        return None;
    }
    for p in &mut pieces {
        p.on = on(p.from);
    }
    let lines: Vec<(&Curve, [f64; 2])> = [a, b]
        .iter()
        .flat_map(|x| x.edges.iter().map(|e| (&e.curve, e.t)))
        .collect();
    let (body, volume) = assemble(&pieces, &lines, eps)?;
    if (volume - want).abs() > 1e-9 * size * size * size {
        return None;
    }
    Some(BoolOut {
        body,
        from: pieces.iter().map(|p| vec![p.from]).collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit_box() -> Polytope {
        Polytope::of_box(&Bbox {
            min: P3::new(0.0, 0.0, 0.0),
            max: P3::new(10.0, 8.0, 6.0),
        })
    }

    fn block(min: [f64; 3], max: [f64; 3]) -> Body {
        Polytope::of_box(&Bbox {
            min: P3::from(min),
            max: P3::from(max),
        })
        .body(1e-9)
        .unwrap()
    }

    /// The closed-form boolean of two blocks, sound and of volume `want`, with its counts of
    /// solids, shells and faces.
    fn of_blocks(a: &Body, b: &Body, subtract: bool, want: f64) -> (usize, usize, usize) {
        let BoolOut { body: r, from } = boolean(a, b, subtract, 1e-6).expect("a closed form");
        assert!(r.check().is_empty(), "{:?}", r.check());
        assert_eq!(from.len(), r.faces.len());
        let v = crate::native::mass::body_volume(&r);
        assert!((v - want).abs() < 1e-9, "{v} against {want}");
        (r.solids.len(), r.shells.len(), r.faces.len())
    }

    #[test]
    fn two_blocks_common_part_is_the_block_between() {
        let a = block([0.0; 3], [10.0, 8.0, 6.0]);
        let b = block([4.0, 3.0, 2.0], [14.0, 11.0, 9.0]);
        assert_eq!(of_blocks(&a, &b, false, 6.0 * 5.0 * 4.0), (1, 1, 6));
        // (And the same found by the general boolean.)
        let general = crate::native::boolean::boolean_bodies(&a, &b, crate::BoolOp::Intersect);
        let v = crate::native::mass::body_volume(&general.unwrap().body);
        assert!((v - 120.0).abs() < 1e-9);
    }

    #[test]
    fn a_corner_cut_out_leaves_three_l_shaped_faces() {
        // The corner block from (6, 5, 4) on, out of the box: three faces lose a corner, three
        // new ones face into the notch.
        let a = block([0.0; 3], [10.0, 8.0, 6.0]);
        let b = block([6.0, 5.0, 4.0], [12.0, 10.0, 8.0]);
        assert_eq!(of_blocks(&a, &b, true, 480.0 - 4.0 * 3.0 * 2.0), (1, 1, 9));
    }

    #[test]
    fn a_block_inside_leaves_a_void_and_one_flush_with_a_face_a_rim() {
        let a = block([0.0; 3], [10.0, 8.0, 6.0]);
        // Wholly inside: a void, a shell of its own.
        let inside = block([1.0; 3], [9.0, 7.0, 5.0]);
        assert_eq!(
            of_blocks(&a, &inside, true, 480.0 - 8.0 * 6.0 * 4.0),
            (1, 2, 12)
        );
        // Up to the top face: the top is a rim round the opening, one face with a hole.
        let open = block([1.0; 3], [9.0, 7.0, 6.0]);
        let BoolOut { body: r, from } = boolean(&a, &open, true, 1e-6).unwrap();
        assert!(r.check().is_empty(), "{:?}", r.check());
        assert_eq!((r.solids.len(), r.shells.len(), r.faces.len()), (1, 1, 11));
        assert_eq!(r.faces.iter().filter(|f| f.loops.len() == 2).count(), 1);
        // (Five walls of the opening from the second block, six faces from the first.)
        assert_eq!(from.iter().filter(|f| f[0].0 == 1).count(), 5);
        let v = crate::native::mass::body_volume(&r);
        assert!((v - (480.0 - 8.0 * 6.0 * 5.0)).abs() < 1e-9, "{v}");
        // Past the top face, open at the top and the front: the same solid as up to them.
        let through = block([1.0, -3.0, 1.0], [9.0, 7.0, 9.0]);
        assert_eq!(
            of_blocks(&a, &through, true, 480.0 - 8.0 * 7.0 * 5.0),
            (1, 1, 10)
        );
    }

    #[test]
    fn a_slab_cut_through_leaves_two_solids() {
        let a = block([0.0; 3], [10.0, 8.0, 6.0]);
        let slab = block([4.0, -1.0, -1.0], [6.0, 9.0, 7.0]);
        assert_eq!(
            of_blocks(&a, &slab, true, 480.0 - 2.0 * 8.0 * 6.0),
            (2, 2, 12)
        );
    }

    #[test]
    fn what_is_not_plainly_a_closed_form_is_left_to_the_general_boolean() {
        let a = block([0.0; 3], [10.0, 8.0, 6.0]);
        // Apart, and touching face to face: no volume shared.
        assert!(boolean(&a, &block([20.0; 3], [30.0; 3]), true, 1e-6).is_none());
        assert!(boolean(&a, &block([10.0, 0.0, 0.0], [12.0, 8.0, 6.0]), true, 1e-6).is_none());
        // A face a hair off the other's: near, not on.
        let hair = block([1.0, 1.0, 1.0], [9.0, 7.0, 6.0 - 1e-5]);
        assert!(boolean(&a, &hair, true, 1e-6).is_none());
        // Touching the first along an edge only: what is left pinches there.
        let edge = block([-2.0, -2.0, 2.0], [0.0, 0.0, 4.0]);
        assert!(boolean(&a, &edge, true, 1e-6).is_none());
        // The whole of the first: nothing is left.
        assert!(boolean(&a, &block([-1.0; 3], [11.0; 3]), true, 1e-6).is_none());
    }

    #[test]
    fn a_box_is_a_sound_solid() {
        let b = unit_box().body(1e-9).unwrap();
        assert_eq!((b.faces.len(), b.edges.len(), b.verts.len()), (6, 12, 8));
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((crate::native::mass::body_volume(&b) - 480.0).abs() < 1e-9);
    }

    #[test]
    fn a_corner_cut_off_adds_a_triangle() {
        let mut p = unit_box();
        assert!((p.volume() - 480.0).abs() < 1e-9);
        // the corner at (10, 8, 6), cut by the plane through the points 2 from it along its edges
        let n = V3::new(1.0, 1.0, 1.0);
        assert!(p.clip(P3::new(8.0, 8.0, 6.0), n, 1e-9));
        assert!((p.volume() - (480.0 - 8.0 / 6.0)).abs() < 1e-9);
        let b = p.body(1e-9).unwrap();
        assert_eq!((b.faces.len(), b.edges.len(), b.verts.len()), (7, 15, 10));
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((crate::native::mass::body_volume(&b) - (480.0 - 8.0 / 6.0)).abs() < 1e-9);
    }

    #[test]
    fn a_cut_along_a_face_changes_nothing_and_one_past_it_leaves_nothing() {
        let mut p = unit_box();
        assert!(p.clip(P3::new(0.0, 0.0, 6.0), V3::z(), 1e-9));
        assert_eq!(p.faces.len(), 6);
        assert!(!p.clip(P3::new(0.0, 0.0, 0.0), V3::z(), 1e-9));
    }

    #[test]
    fn a_box_body_is_read_back_and_cut() {
        let b = unit_box().body(1e-9).unwrap();
        let mut p = Polytope::of_body(&b, 1e-9).unwrap();
        assert!(p.clip(P3::new(5.0, 0.0, 0.0), V3::x(), 1e-9));
        let half = p.body(1e-9).unwrap();
        assert!((crate::native::mass::body_volume(&half) - 240.0).abs() < 1e-9);
    }
}

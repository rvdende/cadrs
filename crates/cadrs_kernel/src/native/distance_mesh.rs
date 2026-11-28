//! Meshes' pieces for the nearest-points search ([`super::distance`]): triangles, segments and
//! points with their parameters on the exact geometry, a hierarchy of their boxes, the least
//! distance between two such hierarchies and the pairs of pieces within a reach.
//!
//! The geometry of pieces (nearest points of segments and triangles) and the two-tree search
//! follow springrep's `mesh/bvh.rs` and `distance.rs` (MIT OR Apache-2.0,
//! https://github.com/springcraft-ai/springrep).

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use super::math::*;

/// A piece of a mesh with where it lies on its entity: a triangle's corners and their (u, v),
/// a segment's ends and their curve parameters, or a point.
#[derive(Debug, Clone, Copy)]
pub enum Bit {
    Tri([P3; 3], [P2; 3]),
    Seg([P3; 2], [f64; 2]),
    Pt(P3),
}

impl Bit {
    pub fn bbox(&self) -> Bbox {
        let mut bb = Bbox::empty();
        match self {
            Bit::Tri(p, _) => p.iter().for_each(|q| bb.add(q)),
            Bit::Seg(p, _) => p.iter().for_each(|q| bb.add(q)),
            Bit::Pt(p) => bb.add(p),
        }
        bb
    }

    /// The parameters on the entity of a point of the piece (its nearest point found here).
    pub fn param(&self, p: &P3) -> [f64; 2] {
        match self {
            Bit::Tri(t, uv) => {
                let [a, b, c] = *t;
                let (sb, sc, fa) = (b - a, c - a, p - a);
                let (bb, bc, cc) = (sb.dot(&sb), sb.dot(&sc), sc.dot(&sc));
                let det = bb * cc - bc * bc;
                let (u, v) = if det > 0.0 {
                    let (pb, pc) = (fa.dot(&sb), fa.dot(&sc));
                    ((cc * pb - bc * pc) / det, (bb * pc - bc * pb) / det)
                } else {
                    (0.0, 0.0)
                };
                let w = uv[0] + (uv[1] - uv[0]) * u + (uv[2] - uv[0]) * v;
                [w.x, w.y]
            }
            Bit::Seg(s, t) => {
                let d = s[1] - s[0];
                let l = d.norm_squared();
                let f = if l > 0.0 {
                    ((p - s[0]).dot(&d) / l).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                [t[0] + (t[1] - t[0]) * f, 0.0]
            }
            Bit::Pt(_) => [0.0; 2],
        }
    }
}

/// The nearest points of two pieces, and their distance.
pub fn bit_distance(a: &Bit, b: &Bit) -> (f64, P3, P3) {
    let flip = |(d, p, q): (f64, P3, P3)| (d, q, p);
    match (a, b) {
        (Bit::Pt(p), Bit::Pt(q)) => ((p - q).norm(), *p, *q),
        (Bit::Pt(p), Bit::Seg(s, _)) => {
            let q = on_segment(p, &s[0], &s[1]);
            ((p - q).norm(), *p, q)
        }
        (Bit::Pt(p), Bit::Tri(t, _)) => {
            let q = on_triangle(p, t);
            ((p - q).norm(), *p, q)
        }
        (Bit::Seg(..) | Bit::Tri(..), Bit::Pt(..)) => flip(bit_distance(b, a)),
        (Bit::Seg(s, _), Bit::Seg(u, _)) => segments(&s[0], &s[1], &u[0], &u[1]),
        (Bit::Seg(s, _), Bit::Tri(t, _)) => segment_triangle(&s[0], &s[1], t),
        (Bit::Tri(..), Bit::Seg(..)) => flip(bit_distance(b, a)),
        (Bit::Tri(s, _), Bit::Tri(t, _)) => {
            let mut best = (f64::INFINITY, s[0], t[0]);
            for k in 0..3 {
                let x = segment_triangle(&s[k], &s[(k + 1) % 3], t);
                if x.0 < best.0 {
                    best = x;
                }
                let y = flip(segment_triangle(&t[k], &t[(k + 1) % 3], s));
                if y.0 < best.0 {
                    best = y;
                }
                if best.0 == 0.0 {
                    break;
                }
            }
            best
        }
    }
}

fn on_segment(p: &P3, a: &P3, b: &P3) -> P3 {
    let d = b - a;
    let l = d.norm_squared();
    if l <= 0.0 {
        return *a;
    }
    a + d * ((p - a).dot(&d) / l).clamp(0.0, 1.0)
}

/// The nearest points of two segments: where the lines are nearest if that lies on both, else
/// an end of one against the other.
fn segments(p1: &P3, q1: &P3, p2: &P3, q2: &P3) -> (f64, P3, P3) {
    let (d1, d2, apart) = (q1 - p1, q2 - p2, p1 - p2);
    let (l1, l2, lean) = (d1.norm_squared(), d2.norm_squared(), d1.dot(&d2));
    let mut best = (f64::INFINITY, *p1, *p2);
    let mut offer = |c1: P3, c2: P3| {
        let d = (c1 - c2).norm();
        if d < best.0 {
            best = (d, c1, c2);
        }
    };
    offer(*p1, on_segment(p1, p2, q2));
    offer(*q1, on_segment(q1, p2, q2));
    offer(on_segment(p2, p1, q1), *p2);
    offer(on_segment(q2, p1, q1), *q2);
    let det = l1 * l2 - lean * lean;
    if det > 0.0 {
        let (pull1, pull2) = (d1.dot(&apart), d2.dot(&apart));
        let s = (lean * pull2 - pull1 * l2) / det;
        let t = (l1 * pull2 - lean * pull1) / det;
        if (0.0..=1.0).contains(&s) && (0.0..=1.0).contains(&t) {
            offer(p1 + d1 * s, p2 + d2 * t);
        }
    }
    best
}

/// The point of triangle `t` nearest `p`.
fn on_triangle(p: &P3, t: &[P3; 3]) -> P3 {
    let [a, b, c] = *t;
    let (sb, sc, fa) = (b - a, c - a, p - a);
    let (bb, bc, cc) = (sb.dot(&sb), sb.dot(&sc), sc.dot(&sc));
    let det = bb * cc - bc * bc;
    if det > 0.0 {
        let (pb, pc) = (fa.dot(&sb), fa.dot(&sc));
        let u = (cc * pb - bc * pc) / det;
        let v = (bb * pc - bc * pb) / det;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
            return a + sb * u + sc * v;
        }
    }
    let mut best = on_segment(p, &a, &b);
    for (from, to) in [(b, c), (c, a)] {
        let q = on_segment(p, &from, &to);
        if (q - p).norm() < (best - p).norm() {
            best = q;
        }
    }
    best
}

/// The nearest points of a segment and a triangle: where it crosses the triangle, else the
/// nearest of its ends to the triangle and its sides to the segment.
fn segment_triangle(p: &P3, q: &P3, t: &[P3; 3]) -> (f64, P3, P3) {
    if let Some(x) = crossing(p, q, t) {
        return (0.0, x, x);
    }
    let mut best = (f64::INFINITY, *p, t[0]);
    for e in [p, q] {
        let c = on_triangle(e, t);
        let d = (e - c).norm();
        if d < best.0 {
            best = (d, *e, c);
        }
    }
    for k in 0..3 {
        let x = segments(p, q, &t[k], &t[(k + 1) % 3]);
        if x.0 < best.0 {
            best = x;
        }
    }
    best
}

fn crossing(p: &P3, q: &P3, t: &[P3; 3]) -> Option<P3> {
    let d = q - p;
    let (sb, sc) = (t[1] - t[0], t[2] - t[0]);
    let normal = sb.cross(&sc);
    let approach = d.dot(&normal);
    let flat = 1e-12 * d.norm() * sb.norm() * sc.norm();
    if approach.abs() <= flat || approach.abs() < 1e-300 {
        return None;
    }
    let s = (t[0] - p).dot(&normal) / approach;
    if !(0.0..=1.0).contains(&s) {
        return None;
    }
    let fa = p + d * s - t[0];
    let area = normal.norm_squared();
    let u = fa.cross(&sc).dot(&normal) / area;
    let v = sb.cross(&fa).dot(&normal) / area;
    (u >= 0.0 && v >= 0.0 && u + v <= 1.0).then(|| p + d * s)
}

/// The gap between two boxes (0 where they meet).
pub fn box_gap(a: &Bbox, b: &Bbox) -> f64 {
    let mut s = 0.0;
    for k in 0..3 {
        let g = (b.min[k] - a.max[k]).max(a.min[k] - b.max[k]).max(0.0);
        s += g * g;
    }
    s.sqrt()
}

enum Node {
    /// A range of `order`.
    Leaf(usize, usize),
    Inner(usize, usize),
}

/// A hierarchy of boxes: each node's box holds its children's.
pub struct Tree {
    pub boxes: Vec<Bbox>,
    order: Vec<usize>,
    nodes: Vec<(Bbox, Node)>,
}

impl Tree {
    pub fn new(boxes: Vec<Bbox>) -> Tree {
        let mut t = Tree {
            order: (0..boxes.len()).collect(),
            boxes,
            nodes: Vec::new(),
        };
        if !t.boxes.is_empty() {
            t.split(0, t.boxes.len());
        }
        t
    }

    fn split(&mut self, lo: usize, hi: usize) -> usize {
        let mut bb = Bbox::empty();
        for &i in &self.order[lo..hi] {
            bb.union(&self.boxes[i]);
        }
        let at = self.nodes.len();
        self.nodes.push((bb, Node::Leaf(lo, hi)));
        if hi - lo <= 4 {
            return at;
        }
        let ext = bb.max - bb.min;
        let axis = if ext.x >= ext.y && ext.x >= ext.z {
            0
        } else if ext.y >= ext.z {
            1
        } else {
            2
        };
        let mid = (lo + hi) / 2;
        let boxes = &self.boxes;
        let centre = |i: usize| boxes[i].min[axis] + boxes[i].max[axis];
        self.order[lo..hi].select_nth_unstable_by(mid - lo, |&x, &y| {
            centre(x).partial_cmp(&centre(y)).unwrap_or(Ordering::Equal)
        });
        let l = self.split(lo, mid);
        let r = self.split(mid, hi);
        self.nodes[at].1 = Node::Inner(l, r);
        at
    }

    /// The least `dist(i, j)` over box `i` of this tree and box `j` of `other`, `dist` never
    /// less than the boxes' gap: pairs of nodes opened nearest first, those farther than the
    /// least so far skipped.
    pub fn least(&self, other: &Tree, mut dist: impl FnMut(usize, usize) -> f64) -> f64 {
        let mut best = f64::INFINITY;
        if self.nodes.is_empty() || other.nodes.is_empty() {
            return best;
        }
        let gap = |a: usize, b: usize| box_gap(&self.nodes[a].0, &other.nodes[b].0);
        let mut heap = BinaryHeap::new();
        heap.push(Reverse(Gap(gap(0, 0), 0, 0)));
        while let Some(Reverse(Gap(g, a, b))) = heap.pop() {
            if g > best || best == 0.0 {
                break;
            }
            let (ba, na) = &self.nodes[a];
            let (bb, nb) = &other.nodes[b];
            let pairs = match (na, nb) {
                (Node::Leaf(alo, ahi), Node::Leaf(blo, bhi)) => {
                    for &i in &self.order[*alo..*ahi] {
                        for &j in &other.order[*blo..*bhi] {
                            if box_gap(&self.boxes[i], &other.boxes[j]) <= best {
                                best = best.min(dist(i, j));
                            }
                        }
                    }
                    continue;
                }
                (Node::Inner(l, r), Node::Leaf(..)) => [(*l, b), (*r, b)],
                (Node::Leaf(..), Node::Inner(l, r)) => [(a, *l), (a, *r)],
                (Node::Inner(al, ar), Node::Inner(bl, br)) => {
                    let (ea, eb) = (ba.max - ba.min, bb.max - bb.min);
                    if ea.norm_squared() >= eb.norm_squared() {
                        [(*al, b), (*ar, b)]
                    } else {
                        [(a, *bl), (a, *br)]
                    }
                }
            };
            for (x, y) in pairs {
                let g = gap(x, y);
                if g <= best {
                    heap.push(Reverse(Gap(g, x, y)));
                }
            }
        }
        best
    }

    /// The boxes within `reach` of `bb`, into `out` (cleared first).
    pub fn within(&self, bb: &Bbox, reach: f64, out: &mut Vec<usize>) {
        out.clear();
        if self.nodes.is_empty() {
            return;
        }
        let mut stack = vec![0];
        while let Some(n) = stack.pop() {
            let (nb, node) = &self.nodes[n];
            if box_gap(nb, bb) > reach {
                continue;
            }
            match *node {
                Node::Inner(l, r) => {
                    stack.push(r);
                    stack.push(l);
                }
                Node::Leaf(lo, hi) => out.extend(
                    self.order[lo..hi]
                        .iter()
                        .copied()
                        .filter(|&i| box_gap(&self.boxes[i], bb) <= reach),
                ),
            }
        }
    }
}

/// A pair of nodes and the gap between their boxes, ordered by the gap.
struct Gap(f64, usize, usize);

impl PartialEq for Gap {
    fn eq(&self, o: &Self) -> bool {
        self.0.total_cmp(&o.0) == Ordering::Equal
    }
}
impl Eq for Gap {}
impl PartialOrd for Gap {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Gap {
    fn cmp(&self, o: &Self) -> Ordering {
        self.0.total_cmp(&o.0)
    }
}

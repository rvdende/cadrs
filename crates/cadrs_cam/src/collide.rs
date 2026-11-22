//! Whether solid things run into one another as a machine moves: its head into its table, a
//! tool holder into a vise, a spindle into the part.
//!
//! Each thing is a [`Shape`]: its surface's triangles in its own coordinates, in a tree of
//! boxes. Two shapes are tested where each is ([`intersects`], an [`Xf`] each): the trees are
//! walked together, boxes that are apart dropping whole branches, and the triangles of
//! leaves that meet are tested exactly. Nothing is rebuilt as the things move: only the
//! motions change, which is what makes a check at every step of a simulation affordable.
//!
//! A [`Scene`] is the shapes of a machine and what is on it, each carried by a body (a rigid
//! part of the machine, or the work), with the pairs worth testing; [`Scene::check`] tests
//! them all at the bodies' motions. Surfaces are tested, not volumes: a thing wholly inside
//! another is not found, which a simulation stepping from a clear start never gets to.

use crate::geom::{Aabb, P3, add, cross, dot, scale, sub};
use crate::kinematics::Xf;

/// A surface's triangles in a tree of boxes.
pub struct Shape {
    tris: Vec<[P3; 3]>,
    nodes: Vec<Node>,
}

struct Node {
    bounds: Aabb,
    /// A leaf's triangles (a range of `tris`), or an inner node's two children.
    kind: NodeKind,
}

enum NodeKind {
    Leaf(std::ops::Range<usize>),
    Inner(usize, usize),
}

/// The most triangles a leaf holds.
const LEAF: usize = 4;

impl Shape {
    pub fn new(mut tris: Vec<[P3; 3]>) -> Shape {
        let mut nodes = Vec::new();
        if !tris.is_empty() {
            let n = tris.len();
            build(&mut tris, 0..n, &mut nodes);
        }
        Shape { tris, nodes }
    }

    pub fn is_empty(&self) -> bool {
        self.tris.is_empty()
    }

    pub fn triangles(&self) -> &[[P3; 3]] {
        &self.tris
    }

    /// Its box in its own coordinates.
    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map_or(Aabb::EMPTY, |n| n.bounds)
    }
}

/// Builds the tree over `tris[range]`, splitting at the middle triangle along the longest
/// side of the box of their centres; the node's place in `nodes`.
fn build(tris: &mut [[P3; 3]], range: std::ops::Range<usize>, nodes: &mut Vec<Node>) -> usize {
    let bounds = Aabb::of(tris[range.clone()].iter().flatten().copied());
    let at = nodes.len();
    nodes.push(Node {
        bounds,
        kind: NodeKind::Leaf(range.clone()),
    });
    if range.len() > LEAF {
        let centre = |t: &[P3; 3]| scale(add(add(t[0], t[1]), t[2]), 1.0 / 3.0);
        let spread = Aabb::of(tris[range.clone()].iter().map(centre)).size();
        let axis = (0..3)
            .max_by(|a, b| spread[*a].total_cmp(&spread[*b]))
            .unwrap_or(0);
        let mid = range.len() / 2;
        tris[range.clone()]
            .select_nth_unstable_by(mid, |a, b| centre(a)[axis].total_cmp(&centre(b)[axis]));
        let left = build(tris, range.start..range.start + mid, nodes);
        let right = build(tris, range.start + mid..range.end, nodes);
        nodes[at].kind = NodeKind::Inner(left, right);
    }
    at
}

/// A point where the two shapes' surfaces cross, `a` moved by `xa` and `b` by `xb` (the
/// world's coordinates); `None` when they are apart.
pub fn intersects(a: &Shape, xa: &Xf, b: &Shape, xb: &Xf) -> Option<P3> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    // In `a`'s coordinates: `b` moved by the motion between them.
    let rel = xa.inverse().then(xb);
    let hit = walk(a, 0, b, 0, &rel)?;
    Some(xa.point(hit))
}

/// The box of `b` moved by `x`.
fn moved(b: &Aabb, x: &Xf) -> Aabb {
    Aabb::of(b.corners().iter().map(|c| x.point(*c)))
}

fn apart(a: &Aabb, b: &Aabb) -> bool {
    (0..3).any(|k| a.max[k] < b.min[k] || b.max[k] < a.min[k])
}

fn walk(a: &Shape, na: usize, b: &Shape, nb: usize, rel: &Xf) -> Option<P3> {
    let (an, bn) = (&a.nodes[na], &b.nodes[nb]);
    if apart(&an.bounds, &moved(&bn.bounds, rel)) {
        return None;
    }
    match (&an.kind, &bn.kind) {
        (NodeKind::Leaf(ra), NodeKind::Leaf(rb)) => {
            for tb in &b.tris[rb.clone()] {
                let tb = tb.map(|p| rel.point(p));
                for ta in &a.tris[ra.clone()] {
                    if let Some(p) = triangles_cross(ta, &tb) {
                        return Some(p);
                    }
                }
            }
            None
        }
        // Into the bigger node's children first.
        (NodeKind::Inner(l, r), NodeKind::Leaf(_)) => {
            walk(a, *l, b, nb, rel).or_else(|| walk(a, *r, b, nb, rel))
        }
        (NodeKind::Leaf(_), NodeKind::Inner(l, r)) => {
            walk(a, na, b, *l, rel).or_else(|| walk(a, na, b, *r, rel))
        }
        (NodeKind::Inner(al, ar), NodeKind::Inner(bl, br)) => {
            let big = |x: &Aabb| {
                let s = x.size();
                s[0].max(s[1]).max(s[2])
            };
            if big(&an.bounds) >= big(&bn.bounds) {
                walk(a, *al, b, nb, rel).or_else(|| walk(a, *ar, b, nb, rel))
            } else {
                walk(a, na, b, *bl, rel).or_else(|| walk(a, na, b, *br, rel))
            }
        }
    }
}

/// A point where two triangles cross: an edge of one through the other.
fn triangles_cross(a: &[P3; 3], b: &[P3; 3]) -> Option<P3> {
    (0..3)
        .find_map(|i| segment_triangle(a[i], a[(i + 1) % 3], b))
        .or_else(|| (0..3).find_map(|i| segment_triangle(b[i], b[(i + 1) % 3], a)))
}

/// Where the segment `p`–`q` passes through the triangle `t`.
fn segment_triangle(p: P3, q: P3, t: &[P3; 3]) -> Option<P3> {
    let d = sub(q, p);
    let (e1, e2) = (sub(t[1], t[0]), sub(t[2], t[0]));
    let h = cross(d, e2);
    let det = dot(e1, h);
    if det.abs() < 1e-14 {
        return None;
    }
    let inv = 1.0 / det;
    let s = sub(p, t[0]);
    let u = dot(s, h) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let qv = cross(s, e1);
    let v = dot(d, qv) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let along = dot(e2, qv) * inv;
    (0.0..=1.0)
        .contains(&along)
        .then(|| add(p, scale(d, along)))
}

/// A thing in a scene: its shape, and the body that carries it.
pub struct Item {
    pub name: String,
    pub shape: Shape,
    /// Which of the motions given to [`Scene::check`] moves it.
    pub body: usize,
}

/// Two things that have run into one another.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Their places among the scene's items.
    pub a: usize,
    pub b: usize,
    /// A point where their surfaces cross.
    pub at: P3,
}

/// A machine's things and the pairs of them worth testing.
#[derive(Default)]
pub struct Scene {
    pub items: Vec<Item>,
    pub pairs: Vec<(usize, usize)>,
}

impl Scene {
    /// Adds a thing; its place among the items.
    pub fn add(&mut self, name: &str, tris: Vec<[P3; 3]>, body: usize) -> usize {
        self.items.push(Item {
            name: name.to_string(),
            shape: Shape::new(tris),
            body,
        });
        self.items.len() - 1
    }

    /// Every pair of things on different bodies becomes a pair to test, but those `skip`
    /// says to leave (neighbours on a slide touch all the time).
    pub fn pair_all(&mut self, skip: impl Fn(&Item, &Item) -> bool) {
        for a in 0..self.items.len() {
            for b in a + 1..self.items.len() {
                let (ia, ib) = (&self.items[a], &self.items[b]);
                if ia.body != ib.body && !skip(ia, ib) {
                    self.pairs.push((a, b));
                }
            }
        }
    }

    /// The pairs already touching with the bodies at `motions` are taken out of the pairs to
    /// test (parts modelled touching: a slide on its ways), and returned.
    pub fn forgive(&mut self, motions: &[Xf]) -> Vec<(usize, usize)> {
        let touching: Vec<(usize, usize)> = self
            .check(motions)
            .into_iter()
            .map(|h| (h.a, h.b))
            .collect();
        self.pairs.retain(|p| !touching.contains(p));
        touching
    }

    /// The pairs that cross with each body moved by its motion.
    pub fn check(&self, motions: &[Xf]) -> Vec<Hit> {
        self.pairs
            .iter()
            .filter_map(|&(a, b)| {
                let (ia, ib) = (&self.items[a], &self.items[b]);
                let (xa, xb) = (motions.get(ia.body)?, motions.get(ib.body)?);
                intersects(&ia.shape, xa, &ib.shape, xb).map(|at| Hit { a, b, at })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::dist;
    use crate::model::box_triangles;

    fn cube(min: P3, max: P3) -> Shape {
        Shape::new(box_triangles(&Aabb::new(min, max)))
    }

    #[test]
    fn boxes_apart_touching_and_through_one_another() {
        let a = cube([0.0; 3], [10.0; 3]);
        let b = cube([0.0; 3], [10.0; 3]);
        let id = Xf::IDENTITY;
        // Apart along each axis, then pushed in.
        for k in 0..3 {
            let mut d = [0.0; 3];
            d[k] = 10.5;
            assert!(intersects(&a, &id, &b, &Xf::translation(d)).is_none());
            d[k] = 9.5;
            let hit = intersects(&a, &id, &b, &Xf::translation(d)).expect("overlapping");
            // The point is on both: within both boxes.
            assert!(
                (0..3).all(|j| hit[j] >= -1e-9 && hit[j] <= 10.0 + 1e-9),
                "{hit:?}"
            );
            assert!(hit[k] >= 9.5 - 1e-9);
        }
        // Turned 45° about its middle its corners reach 5·√2 from the middle: it comes within
        // reach 2.07 sooner.
        let spin = Xf::translation([12.2, 0.0, 0.0]).then(&Xf::rotation(
            [5.0, 5.0, 5.0],
            [0.0, 0.0, 1.0],
            45.0,
        ));
        assert!(intersects(&a, &id, &b, &spin).is_none());
        let spin = Xf::translation([11.9, 0.0, 0.0]).then(&Xf::rotation(
            [5.0, 5.0, 5.0],
            [0.0, 0.0, 1.0],
            45.0,
        ));
        assert!(intersects(&a, &id, &b, &spin).is_some());
        // One wholly inside the other: surfaces apart, nothing found (see the module docs).
        let small = cube([4.0; 3], [6.0; 3]);
        assert!(intersects(&a, &id, &small, &id).is_none());
    }

    #[test]
    fn it_agrees_with_testing_every_pair_of_triangles() {
        // Two lumpy shapes (rings of boxes) at many motions.
        let ring = |n: usize, r: f64| {
            let mut t = Vec::new();
            for k in 0..n {
                let a = std::f64::consts::TAU * k as f64 / n as f64;
                let c = [r * a.cos(), r * a.sin(), (k % 3) as f64];
                t.extend(box_triangles(&Aabb::new(
                    sub(c, [1.0; 3]),
                    add(c, [1.0; 3]),
                )));
            }
            t
        };
        let (ta, tb) = (ring(12, 8.0), ring(9, 5.0));
        let (a, b) = (Shape::new(ta.clone()), Shape::new(tb.clone()));
        let mut rng = 0x1234_5678_9abc_def1u64;
        let mut next = |lo: f64, hi: f64| {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            lo + (hi - lo) * ((rng >> 11) as f64 / (1u64 << 53) as f64)
        };
        let (mut met, mut clear) = (0, 0);
        for _ in 0..200 {
            let xa = Xf::rotation(
                [0.0; 3],
                crate::geom::normalize([next(-1.0, 1.0), next(-1.0, 1.0), 1.0]),
                next(0.0, 360.0),
            );
            let xb = Xf::translation([next(-14.0, 14.0), next(-14.0, 14.0), next(-3.0, 3.0)])
                .then(&Xf::rotation([0.0; 3], [1.0, 0.0, 0.0], next(0.0, 360.0)));
            let brute = ta.iter().any(|p| {
                let p = p.map(|q| xa.point(q));
                tb.iter()
                    .any(|q| triangles_cross(&p, &q.map(|v| xb.point(v))).is_some())
            });
            let found = intersects(&a, &xa, &b, &xb);
            assert_eq!(found.is_some(), brute);
            if brute {
                met += 1;
            } else {
                clear += 1;
            }
        }
        assert!(met > 20 && clear > 20, "{met} {clear}");
    }

    #[test]
    fn a_scene_tests_its_pairs_at_the_bodies_motions() {
        // A table, a vise on it and a head over them: the head comes down into the vise.
        let mut s = Scene::default();
        let table = s.add(
            "Table",
            box_triangles(&Aabb::new([-50.0, -30.0, -10.0], [50.0, 30.0, 0.0])),
            0,
        );
        let vise = s.add(
            "Vise",
            box_triangles(&Aabb::new([-10.0, -10.0, 0.0], [10.0, 10.0, 20.0])),
            0,
        );
        let head = s.add(
            "Head",
            box_triangles(&Aabb::new([-8.0, -8.0, 40.0], [8.0, 8.0, 90.0])),
            1,
        );
        s.pair_all(|_, _| false);
        // (The table and the vise are one body: no pair.)
        assert_eq!(s.pairs, vec![(table, head), (vise, head)]);
        let down = |z: f64| [Xf::IDENTITY, Xf::translation([0.0, 0.0, -z])];
        assert!(s.check(&down(10.0)).is_empty());
        let hits = s.check(&down(25.0));
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].a, hits[0].b), (vise, head));
        assert!(dist(hits[0].at, [0.0, 0.0, 17.5]) < 15.0);
        // What touches as modelled is forgiven: no longer tested.
        let mut s2 = Scene::default();
        s2.add(
            "Ways",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 10.0])),
            0,
        );
        s2.add(
            "Slide",
            box_triangles(&Aabb::new([5.0, 5.0, 5.0], [15.0, 15.0, 15.0])),
            1,
        );
        s2.pair_all(|_, _| false);
        assert_eq!(s2.forgive(&[Xf::IDENTITY, Xf::IDENTITY]), vec![(0, 1)]);
        assert!(s2.pairs.is_empty());
    }
}

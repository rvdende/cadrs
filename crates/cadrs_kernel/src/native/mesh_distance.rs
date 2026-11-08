//! The signed distance from a point to a closed body through its tessellation: the nearest
//! triangle found through a bounding-volume tree (best first, a node skipped when its box lies
//! farther than the nearest so far), the sign from the angle-weighted pseudonormal of the
//! triangle's part nearest the point (its face, edge or corner: Bærentzen and Aanæs, "Signed
//! distance computation using the angle weighted pseudonormal", 2005), exact for a closed,
//! consistently oriented mesh. Within the tessellation's deflection of the body's own distance.

use super::math::*;
use super::topo::Body;
use std::collections::HashMap;

/// A closed body's mesh, prepared for distance queries.
pub struct MeshDistance {
    pts: Vec<P3>,
    tris: Vec<[u32; 3]>,
    /// Each triangle's unit normal.
    face_n: Vec<V3>,
    /// Each corner's angle-weighted pseudonormal.
    vert_n: Vec<V3>,
    /// Each edge's (lower corner, higher corner) pseudonormal: its two triangles' normals summed.
    edge_n: HashMap<(u32, u32), V3>,
    nodes: Vec<Node>,
    /// The triangles in the tree's order (leaves hold ranges of it).
    order: Vec<u32>,
}

struct Node {
    bb: Bbox,
    /// A leaf's range of `order`, or an inner node's children.
    kind: Kind,
}

enum Kind {
    Leaf(u32, u32),
    Inner(u32, u32),
}

impl MeshDistance {
    /// The body's mesh within `defl` of it, its corners welded where they meet; None when it isn't
    /// closed (an edge without two triangles).
    pub fn new(b: &Body, defl: f64) -> Option<Self> {
        let (meshes, _) = super::tess::mesh_body(b, defl, 0.3);
        let mut pts: Vec<P3> = Vec::new();
        let mut index: HashMap<[u64; 3], u32> = HashMap::new();
        let q = defl.max(1e-12) * 1e-3;
        let mut tris: Vec<[u32; 3]> = Vec::new();
        for m in &meshes {
            let ids: Vec<u32> = m
                .positions
                .iter()
                .map(|p| {
                    let key = [
                        (p.x / q).round() as i64 as u64,
                        (p.y / q).round() as i64 as u64,
                        (p.z / q).round() as i64 as u64,
                    ];
                    *index.entry(key).or_insert_with(|| {
                        pts.push(*p);
                        (pts.len() - 1) as u32
                    })
                })
                .collect();
            for t in &m.indices {
                let t = t.map(|i| ids[i as usize]);
                if t[0] != t[1] && t[1] != t[2] && t[0] != t[2] {
                    tris.push(t);
                }
            }
        }
        if tris.is_empty() {
            return None;
        }
        let face_n: Vec<V3> = tris
            .iter()
            .map(|t| {
                let (a, b, c) = (pts[t[0] as usize], pts[t[1] as usize], pts[t[2] as usize]);
                (b - a)
                    .cross(&(c - a))
                    .try_normalize(1e-300)
                    .unwrap_or(V3::zeros())
            })
            .collect();
        let mut vert_n = vec![V3::zeros(); pts.len()];
        let mut edge_n: HashMap<(u32, u32), V3> = HashMap::new();
        let mut uses: HashMap<(u32, u32), u32> = HashMap::new();
        for (t, n) in tris.iter().zip(&face_n) {
            for k in 0..3 {
                let (i, j, l) = (t[k], t[(k + 1) % 3], t[(k + 2) % 3]);
                let (p, a, c) = (pts[i as usize], pts[j as usize], pts[l as usize]);
                let ang = (a - p)
                    .try_normalize(1e-300)
                    .zip((c - p).try_normalize(1e-300))
                    .map_or(0.0, |(u, v)| u.dot(&v).clamp(-1.0, 1.0).acos());
                vert_n[i as usize] += n * ang;
                let e = (i.min(j), i.max(j));
                *edge_n.entry(e).or_insert(V3::zeros()) += n;
                *uses.entry(e).or_insert(0) += 1;
            }
        }
        if uses.values().any(|&u| u != 2) {
            return None;
        }
        let mut me = MeshDistance {
            pts,
            tris,
            face_n,
            vert_n,
            edge_n,
            nodes: Vec::new(),
            order: Vec::new(),
        };
        let mut order: Vec<u32> = (0..me.tris.len() as u32).collect();
        let boxes: Vec<Bbox> = me.tris.iter().map(|t| me.tri_box(t)).collect();
        me.build(&mut order, &boxes, 0, me.tris.len());
        me.order = order;
        Some(me)
    }

    fn tri_box(&self, t: &[u32; 3]) -> Bbox {
        let mut bb = Bbox::empty();
        for &i in t {
            bb.add(&self.pts[i as usize]);
        }
        bb
    }

    /// The node over `order[lo..hi]` (split at the median of its centres' longest axis): its
    /// index.
    fn build(&mut self, order: &mut [u32], boxes: &[Bbox], lo: usize, hi: usize) -> u32 {
        let mut bb = Bbox::empty();
        for &i in &order[lo..hi] {
            bb.union(&boxes[i as usize]);
        }
        if hi - lo <= 4 {
            self.nodes.push(Node {
                bb,
                kind: Kind::Leaf(lo as u32, hi as u32),
            });
            return (self.nodes.len() - 1) as u32;
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
        order[lo..hi].select_nth_unstable_by(mid - lo, |&a, &b| {
            let (ca, cb) = (boxes[a as usize].center(), boxes[b as usize].center());
            ca[axis].total_cmp(&cb[axis])
        });
        let me = self.nodes.len();
        self.nodes.push(Node {
            bb,
            kind: Kind::Leaf(0, 0),
        });
        let l = self.build(order, boxes, lo, mid);
        let r = self.build(order, boxes, mid, hi);
        self.nodes[me].kind = Kind::Inner(l, r);
        me as u32
    }

    /// The signed distance from `p` (negative inside).
    pub fn distance(&self, p: &P3) -> f64 {
        let mut best = (f64::INFINITY, 0u32, Near::Face);
        let mut stack: Vec<u32> = vec![0];
        while let Some(n) = stack.pop() {
            let node = &self.nodes[n as usize];
            if box_dist2(&node.bb, p) >= best.0 {
                continue;
            }
            match node.kind {
                Kind::Leaf(lo, hi) => {
                    for &t in &self.order[lo as usize..hi as usize] {
                        let tri = &self.tris[t as usize];
                        let (d2, near) = closest(
                            p,
                            &self.pts[tri[0] as usize],
                            &self.pts[tri[1] as usize],
                            &self.pts[tri[2] as usize],
                        );
                        if d2 < best.0 {
                            best = (d2, t, near);
                        }
                    }
                }
                Kind::Inner(l, r) => {
                    // The nearer child last, so it is searched first.
                    let (dl, dr) = (
                        box_dist2(&self.nodes[l as usize].bb, p),
                        box_dist2(&self.nodes[r as usize].bb, p),
                    );
                    if dl < dr {
                        stack.push(r);
                        stack.push(l);
                    } else {
                        stack.push(l);
                        stack.push(r);
                    }
                }
            }
        }
        let (d2, t, near) = best;
        let tri = &self.tris[t as usize];
        let (n, q) = match near {
            Near::Face => (self.face_n[t as usize], self.pts[tri[0] as usize]),
            Near::Vert(k) => (self.vert_n[tri[k] as usize], self.pts[tri[k] as usize]),
            Near::Edge(k) => {
                let (i, j) = (tri[k], tri[(k + 1) % 3]);
                (
                    self.edge_n
                        .get(&(i.min(j), i.max(j)))
                        .copied()
                        .unwrap_or(V3::zeros()),
                    self.pts[i as usize],
                )
            }
        };
        let d = d2.sqrt();
        if (p - q).dot(&n) < 0.0 { -d } else { d }
    }
}

/// Which part of a triangle a point is nearest.
#[derive(Clone, Copy)]
enum Near {
    Face,
    /// Corner k.
    Vert(usize),
    /// The edge from corner k to the next.
    Edge(usize),
}

fn box_dist2(bb: &Bbox, p: &P3) -> f64 {
    let mut d = 0.0;
    for i in 0..3 {
        let x = if p[i] < bb.min[i] {
            bb.min[i] - p[i]
        } else if p[i] > bb.max[i] {
            p[i] - bb.max[i]
        } else {
            0.0
        };
        d += x * x;
    }
    d
}

/// The squared distance from `p` to the triangle abc and the part of it nearest (Ericson,
/// "Real-Time Collision Detection", 5.1.5).
fn closest(p: &P3, a: &P3, b: &P3, c: &P3) -> (f64, Near) {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(&ap), ac.dot(&ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return ((p - a).norm_squared(), Near::Vert(0));
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(&bp), ac.dot(&bp));
    if d3 >= 0.0 && d4 <= d3 {
        return ((p - b).norm_squared(), Near::Vert(1));
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return ((p - (a + ab * v)).norm_squared(), Near::Edge(0));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(&cp), ac.dot(&cp));
    if d6 >= 0.0 && d5 <= d6 {
        return ((p - c).norm_squared(), Near::Vert(2));
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return ((p - (a + ac * w)).norm_squared(), Near::Edge(2));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return ((p - (b + (c - b) * w)).norm_squared(), Near::Edge(1));
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    ((p - (a + ab * v + ac * w)).norm_squared(), Near::Face)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kernel;

    #[test]
    fn box_distance() {
        let mut k = super::super::kernel::NativeKernel::default();
        let id = k
            .primitive(&crate::Primitive::Box {
                min: P3::new(0.0, 0.0, 0.0),
                max: P3::new(2.0, 2.0, 2.0),
            })
            .unwrap()
            .bodies[0];
        let b = k.get(id).unwrap();
        let m = MeshDistance::new(b, 1e-3).unwrap();
        assert!((m.distance(&P3::new(1.0, 1.0, 1.0)) + 1.0).abs() < 1e-9);
        assert!((m.distance(&P3::new(3.0, 1.0, 1.0)) - 1.0).abs() < 1e-9);
        assert!((m.distance(&P3::new(3.0, 3.0, 1.0)) - 2f64.sqrt()).abs() < 1e-9);
        assert!((m.distance(&P3::new(3.0, 3.0, 3.0)) - 3f64.sqrt()).abs() < 1e-9);
        assert!((m.distance(&P3::new(1.0, 1.0, 0.25)) + 0.25).abs() < 1e-9);
    }
}

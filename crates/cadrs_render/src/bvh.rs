//! A bounding volume hierarchy over the scene's triangles: binned surface-area heuristic, at
//! most four triangles a leaf, traversed near child first.

use crate::math::{V3, v3};

#[derive(Debug, Clone, Copy)]
pub struct Aabb {
    pub min: V3,
    pub max: V3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb { min: v3(f32::MAX, f32::MAX, f32::MAX), max: v3(f32::MIN, f32::MIN, f32::MIN) };

    pub fn grow(&mut self, p: V3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    pub fn union(&self, o: &Aabb) -> Aabb {
        Aabb { min: self.min.min(o.min), max: self.max.max(o.max) }
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn area(&self) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let d = self.max - self.min;
        2.0 * (d.x * d.y + d.y * d.z + d.z * d.x)
    }
    pub fn center(&self) -> V3 {
        (self.min + self.max) * 0.5
    }
    /// The entry distance of a ray (origin `o`, inverse direction `inv`) within `0..tmax`.
    #[inline]
    fn hit(&self, o: V3, inv: V3, tmax: f32) -> Option<f32> {
        let t1 = (self.min - o).mul_v(inv);
        let t2 = (self.max - o).mul_v(inv);
        let tmin = t1.min(t2).max_elem().max(0.0);
        let tfar = t1.max(t2);
        let tfar = tfar.x.min(tfar.y).min(tfar.z).min(tmax);
        (tmin <= tfar).then_some(tmin)
    }
}

/// A triangle as the intersection test wants it.
#[derive(Debug, Clone, Copy)]
pub struct Tri {
    pub v0: V3,
    pub e1: V3,
    pub e2: V3,
}

#[derive(Debug, Clone, Copy)]
struct Node {
    bounds: Aabb,
    /// A leaf's first triangle, or an inner node's first child (the second follows it).
    first: u32,
    /// Triangles in a leaf; 0 for an inner node.
    count: u32,
}

/// Where a ray hits: the triangle, the distance and the barycentric coordinates.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    pub tri: u32,
    pub t: f32,
    pub u: f32,
    pub v: f32,
}

pub struct Bvh {
    nodes: Vec<Node>,
    tris: Vec<Tri>,
    /// The scene's index of each of `tris`.
    pub order: Vec<u32>,
}

const BINS: usize = 12;
const LEAF: usize = 4;

impl Bvh {
    pub fn build(tris: &[[V3; 3]]) -> Self {
        let mut order: Vec<u32> = (0..tris.len() as u32).collect();
        let boxes: Vec<Aabb> = tris
            .iter()
            .map(|t| {
                let mut b = Aabb::EMPTY;
                for p in t {
                    b.grow(*p);
                }
                b
            })
            .collect();
        let centers: Vec<V3> = boxes.iter().map(|b| b.center()).collect();
        let mut nodes = Vec::with_capacity(tris.len() * 2 / LEAF + 1);
        nodes.push(Node { bounds: Aabb::EMPTY, first: 0, count: tris.len() as u32 });
        if !tris.is_empty() {
            Self::split(&mut nodes, 0, &mut order, &boxes, &centers);
        }
        let ordered = order
            .iter()
            .map(|&i| {
                let [a, b, c] = tris[i as usize];
                Tri { v0: a, e1: b - a, e2: c - a }
            })
            .collect();
        Bvh { nodes, tris: ordered, order }
    }

    fn split(nodes: &mut Vec<Node>, index: usize, order: &mut [u32], boxes: &[Aabb], centers: &[V3]) {
        let (first, count) = (nodes[index].first as usize, nodes[index].count as usize);
        let items = &mut order[first..first + count];
        let mut bounds = Aabb::EMPTY;
        let mut cb = Aabb::EMPTY;
        for &i in items.iter() {
            bounds = bounds.union(&boxes[i as usize]);
            cb.grow(centers[i as usize]);
        }
        nodes[index].bounds = bounds;
        if count <= LEAF {
            return;
        }
        // The best binned split over the three axes.
        let mut best = (f32::MAX, 0usize, 0usize);
        for axis in 0..3 {
            let lo = cb.min.axis(axis);
            let extent = cb.max.axis(axis) - lo;
            if extent <= 0.0 {
                continue;
            }
            let mut bins = [(Aabb::EMPTY, 0usize); BINS];
            for &i in items.iter() {
                let b = (((centers[i as usize].axis(axis) - lo) / extent * BINS as f32) as usize).min(BINS - 1);
                bins[b].0 = bins[b].0.union(&boxes[i as usize]);
                bins[b].1 += 1;
            }
            let mut right_area = [0.0f32; BINS];
            let mut right_count = [0usize; BINS];
            let mut acc = Aabb::EMPTY;
            let mut n = 0;
            for b in (1..BINS).rev() {
                acc = acc.union(&bins[b].0);
                n += bins[b].1;
                right_area[b] = acc.area();
                right_count[b] = n;
            }
            let mut acc = Aabb::EMPTY;
            let mut n = 0;
            for b in 0..BINS - 1 {
                acc = acc.union(&bins[b].0);
                n += bins[b].1;
                let cost = acc.area() * n as f32 + right_area[b + 1] * right_count[b + 1] as f32;
                if n > 0 && right_count[b + 1] > 0 && cost < best.0 {
                    best = (cost, axis, b + 1);
                }
            }
        }
        let leaf_cost = bounds.area() * count as f32;
        let mid = if best.0 < f32::MAX && best.0 < leaf_cost + bounds.area() * 2.0 {
            let (_, axis, bin) = best;
            let lo = cb.min.axis(axis);
            let extent = cb.max.axis(axis) - lo;
            let mut i = 0;
            let mut j = count;
            while i < j {
                let c = centers[items[i] as usize].axis(axis);
                let b = (((c - lo) / extent * BINS as f32) as usize).min(BINS - 1);
                if b < bin {
                    i += 1;
                } else {
                    j -= 1;
                    items.swap(i, j);
                }
            }
            i
        } else if count > LEAF * 4 {
            // No useful split (identical centres): halve by index.
            count / 2
        } else {
            return;
        };
        if mid == 0 || mid == count {
            return;
        }
        let left = nodes.len();
        nodes.push(Node { bounds: Aabb::EMPTY, first: first as u32, count: mid as u32 });
        nodes.push(Node { bounds: Aabb::EMPTY, first: (first + mid) as u32, count: (count - mid) as u32 });
        nodes[index].first = left as u32;
        nodes[index].count = 0;
        Self::split(nodes, left, order, boxes, centers);
        Self::split(nodes, left + 1, order, boxes, centers);
    }

    pub fn bounds(&self) -> Aabb {
        self.nodes.first().map(|n| n.bounds).unwrap_or(Aabb::EMPTY)
    }

    #[inline]
    fn intersect_tri(t: &Tri, o: V3, d: V3, tmax: f32) -> Option<(f32, f32, f32)> {
        let p = d.cross(t.e2);
        let det = t.e1.dot(p);
        if det.abs() < 1e-12 {
            return None;
        }
        let inv = 1.0 / det;
        let s = o - t.v0;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(t.e1);
        let v = d.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let dist = t.e2.dot(q) * inv;
        (dist > 0.0 && dist < tmax).then_some((dist, u, v))
    }

    /// The nearest hit along the ray within `tmax`. `tri` is the scene's triangle index.
    pub fn intersect(&self, o: V3, d: V3, tmax: f32) -> Option<Hit> {
        self.traverse(o, d, tmax, false)
    }

    /// True if anything lies along the ray within `tmax`.
    pub fn occluded(&self, o: V3, d: V3, tmax: f32) -> bool {
        self.traverse(o, d, tmax, true).is_some()
    }

    fn traverse(&self, o: V3, d: V3, mut tmax: f32, any: bool) -> Option<Hit> {
        if self.tris.is_empty() {
            return None;
        }
        let inv = v3(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut best: Option<Hit> = None;
        let mut stack = [0u32; 64];
        let mut sp = 0;
        self.nodes[0].bounds.hit(o, inv, tmax)?;
        stack[sp] = 0;
        sp += 1;
        while sp > 0 {
            sp -= 1;
            let node = &self.nodes[stack[sp] as usize];
            if node.count > 0 {
                let first = node.first as usize;
                for k in first..first + node.count as usize {
                    if let Some((t, u, v)) = Self::intersect_tri(&self.tris[k], o, d, tmax) {
                        tmax = t;
                        best = Some(Hit { tri: self.order[k], t, u, v });
                        if any {
                            return best;
                        }
                    }
                }
                continue;
            }
            let (l, r) = (node.first as usize, node.first as usize + 1);
            let hl = self.nodes[l].bounds.hit(o, inv, tmax);
            let hr = self.nodes[r].bounds.hit(o, inv, tmax);
            match (hl, hr) {
                (Some(a), Some(b)) => {
                    // The far one first on the stack, so the near one is visited first.
                    let (near, far) = if a <= b { (l, r) } else { (r, l) };
                    if sp + 2 <= stack.len() {
                        stack[sp] = far as u32;
                        stack[sp + 1] = near as u32;
                        sp += 2;
                    }
                }
                (Some(_), None) => {
                    stack[sp] = l as u32;
                    sp += 1;
                }
                (None, Some(_)) => {
                    stack[sp] = r as u32;
                    sp += 1;
                }
                (None, None) => {}
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(n: usize) -> Vec<[V3; 3]> {
        let mut out = Vec::new();
        for i in 0..n {
            for j in 0..n {
                let (x, y) = (i as f32, j as f32);
                out.push([v3(x, y, 0.0), v3(x + 1.0, y, 0.0), v3(x + 1.0, y + 1.0, 0.0)]);
                out.push([v3(x, y, 0.0), v3(x + 1.0, y + 1.0, 0.0), v3(x, y + 1.0, 0.0)]);
            }
        }
        out
    }

    #[test]
    fn finds_the_same_hit_as_brute_force() {
        let tris = grid(20);
        let bvh = Bvh::build(&tris);
        let mut rng = crate::math::Rng::new(1, 2, 3);
        for _ in 0..500 {
            let o = v3(rng.f() * 20.0, rng.f() * 20.0, 5.0);
            let d = v3(rng.f() - 0.5, rng.f() - 0.5, -1.0).normalize();
            let hit = bvh.intersect(o, d, f32::MAX);
            let mut brute = None::<(u32, f32)>;
            for (i, t) in tris.iter().enumerate() {
                let tri = Tri { v0: t[0], e1: t[1] - t[0], e2: t[2] - t[0] };
                if let Some((dist, _, _)) = Bvh::intersect_tri(&tri, o, d, f32::MAX)
                    && brute.is_none_or(|(_, b)| dist < b)
                {
                    brute = Some((i as u32, dist));
                }
            }
            match (hit, brute) {
                (Some(h), Some((_, t))) => assert!((h.t - t).abs() < 1e-4),
                (None, None) => {}
                other => panic!("{other:?}"),
            }
        }
    }
}

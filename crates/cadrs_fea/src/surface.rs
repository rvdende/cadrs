//! A part's boundary as triangles: welding the kernel's per-face tessellation into one closed
//! surface, refining it to the element size, and the spatial queries the mesher needs (inside
//! or outside, distance, closest point).

use std::collections::HashMap;

use crate::geom::{Rng, V3, add, closest_on_triangle, cross, dot, len, orient2, sub};

/// A closed triangulated surface: the kernel's tessellation of one part. `faces[t]` is the CAD
/// face triangle `t` belongs to (any numbering; loads name faces by it). Triangles wind
/// counter-clockwise seen from outside. Vertices need not be shared between faces: they are
/// welded by position.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Surface {
    pub positions: Vec<V3>,
    pub triangles: Vec<[u32; 3]>,
    pub faces: Vec<u32>,
}

impl Surface {
    /// The axis-aligned bounds.
    pub fn bounds(&self) -> Option<(V3, V3)> {
        let mut it = self.positions.iter();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            ([lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])], [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])])
        }))
    }

    /// The enclosed volume (divergence theorem over the triangles).
    pub fn volume(&self) -> f64 {
        self.triangles
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| self.positions[i as usize]);
                dot(a, cross(b, c)) / 6.0
            })
            .sum()
    }

    /// A box `[lo, hi]` with its six faces numbered 0 −x, 1 +x, 2 −y, 3 +y, 4 −z, 5 +z, two
    /// triangles each (as a kernel tessellates a box). For tests and examples.
    pub fn cuboid(lo: V3, hi: V3) -> Surface {
        let c = |i: usize| -> V3 { [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }] };
        // Each face as a quad (corner indices), counter-clockwise from outside.
        let quads: [[usize; 4]; 6] = [[0, 4, 6, 2], [1, 3, 7, 5], [0, 1, 5, 4], [2, 6, 7, 3], [0, 2, 3, 1], [4, 5, 7, 6]];
        let mut s = Surface::default();
        for (f, q) in quads.iter().enumerate() {
            let base = s.positions.len() as u32;
            s.positions.extend(q.iter().map(|&i| c(i)));
            s.triangles.push([base, base + 1, base + 2]);
            s.triangles.push([base, base + 2, base + 3]);
            s.faces.extend([f as u32, f as u32]);
        }
        s
    }
}

/// A welded surface: shared vertices, triangles, their faces.
#[derive(Debug, Clone, Default)]
pub(crate) struct Welded {
    pub pts: Vec<V3>,
    pub tris: Vec<[u32; 3]>,
    pub face: Vec<u32>,
}

/// Welds vertices closer than `tol` and drops triangles that collapse.
pub(crate) fn weld(s: &Surface, tol: f64) -> Welded {
    let cell = tol.max(1e-12) * 4.0;
    let key = |p: V3| [(p[0] / cell).floor() as i64, (p[1] / cell).floor() as i64, (p[2] / cell).floor() as i64];
    let mut grid: HashMap<[i64; 3], Vec<u32>> = HashMap::new();
    let mut pts: Vec<V3> = Vec::new();
    let mut map = vec![0u32; s.positions.len()];
    for (i, &p) in s.positions.iter().enumerate() {
        let k = key(p);
        let mut found = None;
        'search: for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(list) = grid.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &j in list {
                            if len(sub(pts[j as usize], p)) <= tol {
                                found = Some(j);
                                break 'search;
                            }
                        }
                    }
                }
            }
        }
        map[i] = found.unwrap_or_else(|| {
            pts.push(p);
            let j = (pts.len() - 1) as u32;
            grid.entry(k).or_default().push(j);
            j
        });
    }
    let mut w = Welded { pts, ..Default::default() };
    for (t, tri) in s.triangles.iter().enumerate() {
        let v = tri.map(|i| map[i as usize]);
        if v[0] != v[1] && v[1] != v[2] && v[0] != v[2] {
            w.tris.push(v);
            w.face.push(s.faces.get(t).copied().unwrap_or(0));
        }
    }
    w
}

/// The number of edges not shared by exactly two triangles (0 for a closed surface).
pub(crate) fn open_edges(w: &Welded) -> usize {
    let mut count: HashMap<(u32, u32), i32> = HashMap::new();
    for t in &w.tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            *count.entry((a.min(b), a.max(b))).or_default() += 1;
        }
    }
    count.values().filter(|&&c| c != 2).count()
}

/// Points on the surface about `h` apart, for the mesher: the vertices, points along every edge
/// (at most `h` apart), and inside each triangle big enough to hold them, a lattice of spacing
/// `h` kept `h`/2 clear of the triangle's edges. Each point comes with the CAD faces it lies on
/// (an edge's points on both of its triangles' faces). Sampling points (rather than refining the
/// triangles) keeps a curved face's long, thin chord triangles from multiplying.
pub(crate) fn sample(w: &Welded, h: f64) -> (Vec<V3>, Vec<Vec<u32>>) {
    let mut pts = w.pts.clone();
    let mut faces: Vec<Vec<u32>> = vec![Vec::new(); pts.len()];
    let add_face = |list: &mut Vec<u32>, f: u32| {
        if !list.contains(&f) {
            list.push(f);
        }
    };
    let mut edges: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
    for (t, &f) in w.tris.iter().zip(&w.face) {
        for k in 0..3 {
            add_face(&mut faces[t[k] as usize], f);
            let (a, b) = (t[k], t[(k + 1) % 3]);
            add_face(edges.entry((a.min(b), a.max(b))).or_default(), f);
        }
    }
    let mut keys: Vec<(u32, u32)> = edges.keys().copied().collect();
    keys.sort_unstable();
    for (a, b) in keys {
        let (pa, pb) = (w.pts[a as usize], w.pts[b as usize]);
        let n = (len(sub(pb, pa)) / h).ceil() as usize;
        for k in 1..n {
            pts.push(add(pa, crate::geom::scale(sub(pb, pa), k as f64 / n as f64)));
            faces.push(edges[&(a, b)].clone());
        }
    }
    for (t, &f) in w.tris.iter().zip(&w.face) {
        let [a, b, c] = t.map(|i| w.pts[i as usize]);
        let n = cross(sub(b, a), sub(c, a));
        let area2 = len(n);
        if area2 < h * h {
            continue;
        }
        // A lattice in the triangle's plane, along its longest edge.
        let sides = [(a, b), (b, c), (c, a)];
        let (o, e) = sides.iter().copied().max_by(|x, y| len(sub(x.1, x.0)).total_cmp(&len(sub(y.1, y.0)))).unwrap();
        let u = crate::geom::normalize(sub(e, o));
        let nn = crate::geom::normalize(n);
        let v = cross(nn, u);
        let far = [a, b, c].iter().map(|p| dot(sub(*p, o), v)).fold(f64::MIN, f64::max);
        let (lo_v, hi_v) = (far.min(0.0), far.max(0.0));
        let span_u = len(sub(e, o));
        let dist_to_edge = |p: V3, (x, y): (V3, V3)| {
            let d = sub(y, x);
            let t = (dot(sub(p, x), d) / dot(d, d)).clamp(0.0, 1.0);
            len(sub(p, add(x, crate::geom::scale(d, t))))
        };
        let mut j = (lo_v / h).floor() as i64;
        while j as f64 * h <= hi_v {
            let mut i = 0i64;
            while i as f64 * h <= span_u {
                let p = add(o, add(crate::geom::scale(u, i as f64 * h), crate::geom::scale(v, j as f64 * h)));
                let bary_ok = {
                    let c0 = dot(cross(sub(b, a), sub(p, a)), nn);
                    let c1 = dot(cross(sub(c, b), sub(p, b)), nn);
                    let c2 = dot(cross(sub(a, c), sub(p, c)), nn);
                    c0 > 0.0 && c1 > 0.0 && c2 > 0.0
                };
                if bary_ok && sides.iter().all(|s| dist_to_edge(p, *s) >= 0.5 * h) {
                    pts.push(p);
                    faces.push(vec![f]);
                }
                i += 1;
            }
            j += 1;
        }
    }
    (pts, faces)
}

/// Spatial queries over a surface's triangles: point-in-solid by ray parity and closest point.
pub(crate) struct SurfaceIndex {
    tris: Vec<[V3; 3]>,
    pub face: Vec<u32>,
    lo: V3,
    cell: f64,
    dims: [usize; 3],
    bins: Vec<Vec<u32>>,
    /// Bins over (y, z) for rays along +x.
    ray_bins: Vec<Vec<u32>>,
    diag: f64,
}

impl SurfaceIndex {
    pub fn new(pts: &[V3], tris: &[[u32; 3]], face: &[u32], cell: f64) -> Self {
        let tri_pts: Vec<[V3; 3]> = tris.iter().map(|t| t.map(|i| pts[i as usize])).collect();
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for p in pts {
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let diag = len(sub(hi, lo)).max(1e-9);
        let cell = cell.max(diag / 256.0);
        lo = sub(lo, [cell; 3]);
        let dims = [0, 1, 2].map(|k| (((hi[k] + cell - lo[k]) / cell).ceil() as usize).max(1));
        let mut bins = vec![Vec::new(); dims[0] * dims[1] * dims[2]];
        let mut ray_bins = vec![Vec::new(); dims[1] * dims[2]];
        for (t, v) in tri_pts.iter().enumerate() {
            let (mut a, mut b) = ([usize::MAX; 3], [0usize; 3]);
            for p in v {
                for k in 0..3 {
                    let c = (((p[k] - lo[k]) / cell).floor().max(0.0) as usize).min(dims[k] - 1);
                    a[k] = a[k].min(c);
                    b[k] = b[k].max(c);
                }
            }
            for x in a[0]..=b[0] {
                for y in a[1]..=b[1] {
                    for z in a[2]..=b[2] {
                        bins[(x * dims[1] + y) * dims[2] + z].push(t as u32);
                    }
                }
            }
            for y in a[1]..=b[1] {
                for z in a[2]..=b[2] {
                    ray_bins[y * dims[2] + z].push(t as u32);
                }
            }
        }
        Self { tris: tri_pts, face: face.to_vec(), lo, cell, dims, bins, ray_bins, diag }
    }

    fn cell_of(&self, p: V3) -> [i64; 3] {
        [0, 1, 2].map(|k| ((p[k] - self.lo[k]) / self.cell).floor() as i64)
    }

    /// Is `p` inside the closed surface? (Parity of the crossings of a ray along +x; a ray that
    /// grazes an edge is retried from a slightly moved point.)
    pub fn inside(&self, p: V3) -> bool {
        let mut rng = Rng::new(0x1234_5678 ^ p[0].to_bits() ^ p[1].to_bits().rotate_left(21) ^ p[2].to_bits().rotate_left(42));
        let mut q = p;
        for _ in 0..16 {
            if let Some(c) = self.crossings(q) {
                return c % 2 == 1;
            }
            let e = self.diag * 1e-9;
            q = add(p, [(rng.next_f64() - 0.5) * e, (rng.next_f64() - 0.5) * e, (rng.next_f64() - 0.5) * e]);
        }
        false
    }

    fn crossings(&self, p: V3) -> Option<usize> {
        let c = self.cell_of(p);
        if c[1] < 0 || c[2] < 0 || c[1] as usize >= self.dims[1] || c[2] as usize >= self.dims[2] {
            return Some(0);
        }
        let q = [p[1], p[2]];
        let mut n = 0;
        for &t in &self.ray_bins[c[1] as usize * self.dims[2] + c[2] as usize] {
            let [a, b, cc] = self.tris[t as usize];
            let (a2, b2, c2) = ([a[1], a[2]], [b[1], b[2]], [cc[1], cc[2]]);
            let area = orient2(a2, b2, c2);
            if area == 0.0 {
                continue;
            }
            let s0 = orient2(b2, c2, q);
            let s1 = orient2(c2, a2, q);
            let s2 = orient2(a2, b2, q);
            if s0 == 0.0 || s1 == 0.0 || s2 == 0.0 {
                // On a projected edge: only a problem if the triangle is otherwise hit.
                let same = |s: f64| s == 0.0 || (s > 0.0) == (area > 0.0);
                if same(s0) && same(s1) && same(s2) {
                    return None;
                }
                continue;
            }
            if (s0 > 0.0) != (area > 0.0) || (s1 > 0.0) != (area > 0.0) || (s2 > 0.0) != (area > 0.0) {
                continue;
            }
            let x = (s0 * a[0] + s1 * b[0] + s2 * cc[0]) / (s0 + s1 + s2);
            if x == p[0] {
                return None;
            }
            if x > p[0] {
                n += 1;
            }
        }
        Some(n)
    }

    /// The closest point of the surface within `reach` cells of `p`'s cell: (distance, point,
    /// triangle, barycentric weights). `None` if nothing is that near.
    pub fn closest(&self, p: V3, reach: i64) -> Option<(f64, V3, u32, [f64; 3])> {
        let c = self.cell_of(p);
        let mut best: Option<(f64, V3, u32, [f64; 3])> = None;
        for x in c[0] - reach..=c[0] + reach {
            for y in c[1] - reach..=c[1] + reach {
                for z in c[2] - reach..=c[2] + reach {
                    if x < 0 || y < 0 || z < 0 || x as usize >= self.dims[0] || y as usize >= self.dims[1] || z as usize >= self.dims[2] {
                        continue;
                    }
                    for &t in &self.bins[(x as usize * self.dims[1] + y as usize) * self.dims[2] + z as usize] {
                        let [a, b, cc] = self.tris[t as usize];
                        let (q, w) = closest_on_triangle(p, a, b, cc);
                        let d = len(sub(q, p));
                        if best.is_none_or(|b| d < b.0) {
                            best = Some((d, q, t, w));
                        }
                    }
                }
            }
        }
        best
    }

    /// The distance from `p` to the surface, or `cell` if it is farther than one cell.
    pub fn distance_within_cell(&self, p: V3) -> f64 {
        self.closest(p, 1).map_or(self.cell, |c| c.0.min(self.cell))
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cuboid_welds_closed_and_samples() {
        let s = Surface::cuboid([0.0; 3], [10.0, 2.0, 3.0]);
        assert!((s.volume() - 60.0).abs() < 1e-12);
        let w = weld(&s, 1e-9);
        assert_eq!(w.pts.len(), 8);
        assert_eq!(open_edges(&w), 0);
        // Sampled 0.7 apart: every point on the box's surface, on the faces it lies on.
        let (pts, faces) = sample(&w, 0.7);
        assert!(pts.len() > 200);
        for (p, f) in pts.iter().zip(&faces) {
            for &face in f {
                let axis = (face / 2) as usize;
                let at = if face % 2 == 0 { 0.0 } else { [10.0, 2.0, 3.0][axis] };
                assert!((p[axis] - at).abs() < 1e-9, "{p:?} not on face {face}");
            }
        }
        // Distinct points, about as many as the area over the spacing squared.
        for i in 0..pts.len() {
            for j in i + 1..pts.len() {
                assert!(len(sub(pts[i], pts[j])) > 1e-9, "{:?} {:?}", pts[i], pts[j]);
            }
        }
        let area = 2.0 * (10.0 * 2.0 + 10.0 * 3.0 + 2.0 * 3.0);
        assert!(pts.len() < (3.0 * area / (0.7 * 0.7)) as usize, "{} points", pts.len());
        let idx = SurfaceIndex::new(&w.pts, &w.tris, &w.face, 0.7);
        assert!(idx.inside([5.0, 1.0, 1.5]));
        // Grid-aligned points (rays along the triangles' edges) still classify.
        assert!(idx.inside([0.5, 0.5, 0.5]));
        assert!(idx.inside([9.9, 1.9, 2.9]));
        assert!(!idx.inside([10.5, 1.0, 1.0]));
        assert!(!idx.inside([5.0, -0.5, 1.0]));
        assert!((idx.distance_within_cell([5.0, 1.0, 0.25]) - 0.25).abs() < 1e-12);
    }
}

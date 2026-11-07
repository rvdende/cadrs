//! A closed triangle mesh made coarser: edges collapsed (one end into the other), the cheapest
//! first, by quadric error (Garland and Heckbert). Each vertex carries the planes of the
//! triangles merged into it, and a collapse is made only while the vertex kept lies within `tol`
//! of all of them (their squared distances summed), the surface stays a manifold, and no triangle
//! turns over or becomes a sliver. The vertices kept are the mesh's own, so they stay where the
//! mesh had them (on a surface it was fitted to).

use super::math::*;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// The sum of squared distances to planes, as a symmetric 4 × 4 form: (a11 a12 a13 a14 a22 a23
/// a24 a33 a34 a44).
#[derive(Clone, Copy, Default)]
struct Quadric([f64; 10]);

impl Quadric {
    /// w times the squared distance to the plane n · x + d = 0 (n of unit length).
    fn plane(n: V3, d: f64, w: f64) -> Self {
        let (a, b, c) = (n.x, n.y, n.z);
        Quadric(
            [
                a * a,
                a * b,
                a * c,
                a * d,
                b * b,
                b * c,
                b * d,
                c * c,
                c * d,
                d * d,
            ]
            .map(|x| x * w),
        )
    }

    fn add(&mut self, o: &Quadric) {
        for (a, b) in self.0.iter_mut().zip(o.0) {
            *a += b;
        }
    }

    fn sum(&self, o: &Quadric) -> Quadric {
        let mut s = *self;
        s.add(o);
        s
    }

    fn error(&self, p: &P3) -> f64 {
        let q = &self.0;
        let (x, y, z) = (p.x, p.y, p.z);
        q[0] * x * x
            + 2.0 * (q[1] * x * y + q[2] * x * z + q[3] * x)
            + q[4] * y * y
            + 2.0 * (q[5] * y * z + q[6] * y)
            + q[7] * z * z
            + 2.0 * q[8] * z
            + q[9]
    }
}

/// A collapse of `u` into `v`, its cost, and the vertices' versions when it was costed.
struct Collapse {
    cost: f64,
    u: u32,
    v: u32,
    ver: (u32, u32),
}

impl PartialEq for Collapse {
    fn eq(&self, o: &Self) -> bool {
        self.cost.total_cmp(&o.cost) == Ordering::Equal
    }
}
impl Eq for Collapse {}
impl PartialOrd for Collapse {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Collapse {
    // The cheapest on top of the (max-)heap.
    fn cmp(&self, o: &Self) -> Ordering {
        o.cost.total_cmp(&self.cost)
    }
}

/// The vertices about `x` (of its triangles) into `out`.
fn ring(inc: &[Vec<u32>], tris: &[[u32; 3]], x: usize, out: &mut Vec<u32>) {
    out.clear();
    for &t in &inc[x] {
        for &w in &tris[t as usize] {
            if w as usize != x && !out.contains(&w) {
                out.push(w);
            }
        }
    }
}

/// How round a triangle is: 1 equilateral, 0 a sliver.
fn roundness(a: &P3, b: &P3, c: &P3) -> f64 {
    let l = (b - a).norm_squared() + (c - b).norm_squared() + (a - c).norm_squared();
    if l <= 0.0 {
        return 0.0;
    }
    2.0 * 3f64.sqrt() * (b - a).cross(&(c - a)).norm() / l
}

/// The closed, consistently turned mesh (`pts`, `tris`) with edges collapsed while the vertices
/// kept stay within `tol` of the planes of the triangles they replace: its points (the ones
/// still used) and triangles.
pub(crate) fn decimate(pts: &[P3], tris: &[[u32; 3]], tol: f64) -> (Vec<P3>, Vec<[u32; 3]>) {
    let nv = pts.len();
    if nv == 0 || tris.len() <= 4 {
        return (pts.to_vec(), tris.to_vec());
    }
    // About the mesh's middle, so the quadrics' terms stay small.
    let mut bb = Bbox::empty();
    for p in pts {
        bb.add(p);
    }
    let mid = bb.center().coords;
    let p: Vec<P3> = pts.iter().map(|q| P3::from(q.coords - mid)).collect();
    let mut tris = tris.to_vec();
    let mut alive = vec![true; tris.len()];
    let mut inc: Vec<Vec<u32>> = vec![Vec::new(); nv];
    for (t, tri) in tris.iter().enumerate() {
        for &v in tri {
            inc[v as usize].push(t as u32);
        }
    }
    // Each triangle's plane, weighted by its area (up to the mean): a sliver's plane, which
    // its noise turns anyhow, counts for little.
    let area = |t: &[u32; 3]| {
        let [a, b, c] = t.map(|i| p[i as usize]);
        0.5 * (b - a).cross(&(c - a)).norm()
    };
    let mean = tris.iter().map(area).sum::<f64>() / tris.len() as f64;
    let mut q = vec![Quadric::default(); nv];
    for t in &tris {
        let [a, b, c] = t.map(|i| p[i as usize]);
        let n = (b - a).cross(&(c - a));
        let l = n.norm();
        if !(l > 0.0) {
            continue;
        }
        let n = n / l;
        let w = (0.5 * l / mean).min(1.0);
        let pl = Quadric::plane(n, -n.dot(&a.coords), w);
        for &v in t {
            q[v as usize].add(&pl);
        }
    }
    let tol2 = tol * tol;
    let mut ver = vec![0u32; nv];
    let mut dead = vec![false; nv];
    let mut heap: BinaryHeap<Collapse> = BinaryHeap::with_capacity(3 * tris.len());
    let cost =
        |q: &[Quadric], u: u32, v: u32| q[u as usize].sum(&q[v as usize]).error(&p[v as usize]);
    for t in &tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if a < b {
                for (u, v) in [(a, b), (b, a)] {
                    let c = cost(&q, u, v);
                    if c <= tol2 {
                        heap.push(Collapse {
                            cost: c,
                            u,
                            v,
                            ver: (0, 0),
                        });
                    }
                }
            }
        }
    }
    let mut live = tris.len();
    let mut nu: Vec<u32> = Vec::new();
    let mut nvs: Vec<u32> = Vec::new();
    while let Some(c) = heap.pop() {
        let (u, v) = (c.u as usize, c.v as usize);
        if dead[u] || dead[v] || ver[u] != c.ver.0 || ver[v] != c.ver.1 {
            continue;
        }
        if live <= 8 {
            break;
        }
        // The two triangles on the edge, and the link condition: the ends' only common
        // neighbours are those triangles' third corners.
        let shared: Vec<u32> = inc[u]
            .iter()
            .copied()
            .filter(|&t| tris[t as usize].contains(&c.v))
            .collect();
        if shared.len() != 2 {
            continue;
        }
        ring(&inc, &tris, u, &mut nu);
        ring(&inc, &tris, v, &mut nvs);
        let common = nu.iter().filter(|w| nvs.contains(w)).count();
        if common != 2 {
            continue;
        }
        // No triangle about u turned over, swung far or made a sliver.
        let pv = p[v];
        let fine = inc[u].iter().all(|&t| {
            let tri = tris[t as usize];
            if tri.contains(&c.v) {
                return true;
            }
            let [a, b, cc] = tri.map(|i| p[i as usize]);
            let moved = tri.map(|i| if i as usize == u { pv } else { p[i as usize] });
            let n0 = (b - a).cross(&(cc - a));
            let n1 = (moved[1] - moved[0]).cross(&(moved[2] - moved[0]));
            let (l0, l1) = (n0.norm(), n1.norm());
            if !(l1 > 0.0) {
                return false;
            }
            let r0 = roundness(&a, &b, &cc);
            let r1 = roundness(&moved[0], &moved[1], &moved[2]);
            if r1 < 0.1 && r1 < r0 {
                return false;
            }
            // (A sliver's normal says little.)
            r0 < 0.05 || n0.dot(&n1) >= 0.5 * l0 * l1
        });
        if !fine {
            continue;
        }
        // Collapse: u's triangles on the edge go, the others take v for u.
        for &t in &shared {
            alive[t as usize] = false;
            live -= 1;
            for &w in &tris[t as usize] {
                if w as usize != u {
                    inc[w as usize].retain(|&x| x != t);
                }
            }
        }
        let moved: Vec<u32> = inc[u]
            .iter()
            .copied()
            .filter(|t| !shared.contains(t))
            .collect();
        for &t in &moved {
            for w in tris[t as usize].iter_mut() {
                if *w as usize == u {
                    *w = c.v;
                }
            }
            inc[v].push(t);
        }
        inc[u].clear();
        dead[u] = true;
        let qu = q[u];
        q[v].add(&qu);
        ver[v] += 1;
        ring(&inc, &tris, v, &mut nvs);
        for &w in &nvs {
            for (a, b) in [(c.v, w), (w, c.v)] {
                let cst = cost(&q, a, b);
                if cst <= tol2 {
                    heap.push(Collapse {
                        cost: cst,
                        u: a,
                        v: b,
                        ver: (ver[a as usize], ver[b as usize]),
                    });
                }
            }
        }
    }
    // The vertices still used, numbered again.
    let mut new_index = vec![u32::MAX; nv];
    let mut out_pts: Vec<P3> = Vec::new();
    let mut out_tris: Vec<[u32; 3]> = Vec::with_capacity(live);
    for (t, tri) in tris.iter().enumerate() {
        if !alive[t] {
            continue;
        }
        out_tris.push(tri.map(|i| {
            let i = i as usize;
            if new_index[i] == u32::MAX {
                new_index[i] = out_pts.len() as u32;
                out_pts.push(pts[i]);
            }
            new_index[i]
        }));
    }
    (out_pts, out_tris)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// The volume a closed mesh bounds.
    fn volume(pts: &[P3], tris: &[[u32; 3]]) -> f64 {
        tris.iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| pts[i as usize].coords);
                a.dot(&b.cross(&c)) / 6.0
            })
            .sum()
    }

    /// Every edge used twice, once each way: closed, turned alike.
    fn closed(tris: &[[u32; 3]]) -> bool {
        let mut uses: HashMap<(u32, u32), i32> = HashMap::new();
        for t in tris {
            for k in 0..3 {
                *uses.entry((t[k], t[(k + 1) % 3])).or_insert(0) += 1;
            }
        }
        uses.iter()
            .all(|(&(a, b), &n)| n == 1 && uses.get(&(b, a)) == Some(&1))
    }

    /// An octahedron's faces cut `level` times into four, its points pushed out to the unit
    /// sphere: a closed mesh outward.
    fn sphere(level: usize) -> (Vec<P3>, Vec<[u32; 3]>) {
        let mut pts: Vec<P3> = [
            (1.0, 0.0, 0.0),
            (-1.0, 0.0, 0.0),
            (0.0, 1.0, 0.0),
            (0.0, -1.0, 0.0),
            (0.0, 0.0, 1.0),
            (0.0, 0.0, -1.0),
        ]
        .map(|(x, y, z)| P3::new(x, y, z))
        .to_vec();
        let mut tris: Vec<[u32; 3]> = vec![
            [0, 2, 4],
            [2, 1, 4],
            [1, 3, 4],
            [3, 0, 4],
            [2, 0, 5],
            [1, 2, 5],
            [3, 1, 5],
            [0, 3, 5],
        ];
        for _ in 0..level {
            let mut mid: HashMap<(u32, u32), u32> = HashMap::new();
            let mut next = Vec::with_capacity(tris.len() * 4);
            for t in &tris {
                let mut m = |a: u32, b: u32| {
                    *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                        let p = P3::from((pts[a as usize].coords + pts[b as usize].coords) / 2.0);
                        pts.push(P3::from(p.coords.normalize()));
                        pts.len() as u32 - 1
                    })
                };
                let (ab, bc, ca) = (m(t[0], t[1]), m(t[1], t[2]), m(t[2], t[0]));
                next.extend([[t[0], ab, ca], [ab, t[1], bc], [ca, bc, t[2]], [ab, bc, ca]]);
            }
            tris = next;
        }
        (pts, tris)
    }

    /// A sphere's fine mesh made coarser stays closed, its points the sphere's own, its volume
    /// within its new triangles' sag of the fine one's.
    #[test]
    fn a_fine_sphere_made_coarser() {
        let (pts, tris) = sphere(6);
        let (out_pts, out_tris) = decimate(&pts, &tris, 2e-3);
        assert!(closed(&out_tris));
        // (The planes' squared distances summed: a curved surface's vertices take in fewer
        // collapses than a flat one's, about half of a sphere's here.)
        assert!(
            3 * out_tris.len() < 2 * tris.len(),
            "{} of {}",
            out_tris.len(),
            tris.len()
        );
        assert!(
            out_pts
                .iter()
                .all(|p| (p.coords.norm() - 1.0).abs() < 1e-12)
        );
        let (v0, v1) = (volume(&pts, &tris), volume(&out_pts, &out_tris));
        assert!((v1 - v0).abs() < 1e-2 * v0, "{v1} against {v0}");
    }

    /// A cube's faces meshed finely collapse to few triangles, its corners and edges kept: the
    /// volume exact.
    #[test]
    fn a_finely_meshed_cube_collapses_onto_its_planes() {
        let n = 8u32;
        let mut index: HashMap<(i64, i64, i64), u32> = HashMap::new();
        let mut pts: Vec<P3> = Vec::new();
        let mut at = |p: [u32; 3]| -> u32 {
            *index
                .entry((p[0] as i64, p[1] as i64, p[2] as i64))
                .or_insert_with(|| {
                    pts.push(P3::new(p[0] as f64, p[1] as f64, p[2] as f64) / n as f64);
                    pts.len() as u32 - 1
                })
        };
        let mut tris: Vec<[u32; 3]> = Vec::new();
        // Each side: its two axes and where it lies, its quads cut in two, turned outward.
        for axis in 0..3 {
            for side in [0, n] {
                let (u, v) = ((axis + 1) % 3, (axis + 2) % 3);
                for i in 0..n {
                    for j in 0..n {
                        let corner = |di: u32, dj: u32| {
                            let mut p = [0u32; 3];
                            p[axis] = side;
                            p[u] = i + di;
                            p[v] = j + dj;
                            p
                        };
                        let q =
                            [corner(0, 0), corner(1, 0), corner(1, 1), corner(0, 1)].map(&mut at);
                        if side == n {
                            tris.extend([[q[0], q[1], q[2]], [q[0], q[2], q[3]]]);
                        } else {
                            tris.extend([[q[0], q[2], q[1]], [q[0], q[3], q[2]]]);
                        }
                    }
                }
            }
        }
        assert!(closed(&tris));
        let (out_pts, out_tris) = decimate(&pts, &tris, 1e-9);
        assert!(closed(&out_tris));
        assert!(
            out_tris.len() * 4 < tris.len(),
            "{} of {}",
            out_tris.len(),
            tris.len()
        );
        assert!((volume(&out_pts, &out_tris) - 1.0).abs() < 1e-12);
    }
}

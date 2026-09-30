//! 3D Delaunay tetrahedralization by Bowyer–Watson insertion with exact predicates.
//!
//! - **Ghost tetrahedra** close the triangulation: every face of the convex hull carries a
//!   ghost tetrahedron `[a, b, c, GHOST]` (the ghost vertex "at infinity" outside it). A point
//!   conflicts with a ghost if it is strictly outside the hull face, or on the face's plane and
//!   inside its circumcircle (then the real tetrahedron behind the face conflicts too). This is
//!   Shewchuk's rule; with it the triangulation covers exactly the convex hull of the points,
//!   so a convex part is meshed to its faces, not to a guessed super-tetrahedron.
//! - **Exact predicates** ([`crate::geom::orient`], [`crate::geom::insphere`], adaptive
//!   precision from the `robust` crate) make the degenerate inputs a mesher sees all the time
//!   (grid points, four coplanar points on a planar face, cospherical cube corners) safe: the
//!   cavity of a point never has a boundary face in the point's plane, so no flat tetrahedra.
//! - Points are inserted in a **biased randomized order** (rounds of doubling size, each sorted
//!   along a Morton curve, Amenta–Choi–Rote), from a fixed seed, so the mesh is repeatable, and
//!   located by a **visibility walk** from the last tetrahedron made.

use crate::geom::{Rng, V3, insphere, orient};

/// The ghost vertex.
pub const GHOST: u32 = u32::MAX;
const NONE: u32 = u32::MAX;

/// A Delaunay tetrahedralization of `points`: its (real, positively oriented) tetrahedra.
/// Duplicate points are left out (no tetrahedron uses them). Fewer than four affinely
/// independent points give no tetrahedra.
pub fn tetrahedralize(points: &[V3]) -> Vec<[u32; 4]> {
    let mut t = Triangulation::new(points);
    let Some(order) = t.start() else { return Vec::new() };
    for p in order {
        t.insert(p);
    }
    t.real_tets()
}

struct Triangulation<'a> {
    pts: &'a [V3],
    tets: Vec<[u32; 4]>,
    /// `nbr[t][i]`: the tetrahedron across the face opposite vertex `i`.
    nbr: Vec<[u32; 4]>,
    alive: Vec<bool>,
    free: Vec<u32>,
    /// Cavity marks (the insertion counter that marked it).
    mark: Vec<u32>,
    stamp: u32,
    last: u32,
    rng: Rng,
}

impl<'a> Triangulation<'a> {
    fn new(pts: &'a [V3]) -> Self {
        Self {
            pts,
            tets: Vec::with_capacity(pts.len() * 8),
            nbr: Vec::with_capacity(pts.len() * 8),
            alive: Vec::with_capacity(pts.len() * 8),
            free: Vec::new(),
            mark: Vec::with_capacity(pts.len() * 8),
            stamp: 0,
            last: 0,
            rng: Rng::new(0x5eed_cad5),
        }
    }

    #[inline]
    fn p(&self, i: u32) -> V3 {
        self.pts[i as usize]
    }

    /// The first tetrahedron (four well-spread, affinely independent points) and its four
    /// ghosts; returns the other points in insertion order.
    fn start(&mut self) -> Option<Vec<u32>> {
        let n = self.pts.len();
        if n < 4 {
            return None;
        }
        let d2 = |a: V3, b: V3| (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2);
        let i0 = 0u32;
        let p0 = self.p(i0);
        let i1 = (0..n as u32).max_by(|&a, &b| d2(self.p(a), p0).total_cmp(&d2(self.p(b), p0)))?;
        let p1 = self.p(i1);
        if d2(p1, p0) == 0.0 {
            return None;
        }
        let e = crate::geom::sub(p1, p0);
        let area = |i: u32| crate::geom::len(crate::geom::cross(e, crate::geom::sub(self.p(i), p0)));
        let i2 = (0..n as u32).max_by(|&a, &b| area(a).total_cmp(&area(b)))?;
        if area(i2) == 0.0 {
            return None;
        }
        let p2 = self.p(i2);
        let i3 = (0..n as u32).max_by(|&a, &b| orient(p0, p1, p2, self.p(a)).abs().total_cmp(&orient(p0, p1, p2, self.p(b)).abs()))?;
        let o = orient(p0, p1, p2, self.p(i3));
        if o == 0.0 {
            return None;
        }
        let first = if o > 0.0 { [i0, i1, i2, i3] } else { [i0, i2, i1, i3] };
        let t0 = self.alloc(first);
        for i in 0..4 {
            // The ghost on the face opposite vertex i: the face ordered so the ghost is on its
            // outer side, i.e. the tetrahedron's own vertex i is on the negative side.
            let mut f: Vec<u32> = (0..4).filter(|&k| k != i).map(|k| first[k]).collect();
            if orient(self.p(f[0]), self.p(f[1]), self.p(f[2]), self.p(first[i])) > 0.0 {
                f.swap(0, 1);
            }
            let g = self.alloc([f[0], f[1], f[2], GHOST]);
            self.nbr[t0 as usize][i] = g;
            self.nbr[g as usize][3] = t0;
        }
        // Ghost–ghost adjacency: two ghosts share a face (an edge of the tetrahedron and the
        // ghost vertex).
        let ghosts: Vec<u32> = (0..4).map(|i| self.nbr[t0 as usize][i]).collect();
        for &g in &ghosts {
            for k in 0..3 {
                let v = self.tets[g as usize];
                let (a, b) = edge_without(v, k);
                let other = ghosts.iter().copied().find(|&h| h != g && self.tets[h as usize][..3].contains(&a) && self.tets[h as usize][..3].contains(&b)).unwrap();
                self.nbr[g as usize][k] = other;
            }
        }
        self.last = t0;
        let used = [i0, i1, i2, i3];
        let rest: Vec<u32> = (0..n as u32).filter(|i| !used.contains(i)).collect();
        Some(self.brio(rest))
    }

    /// Biased randomized insertion order: shuffled, cut into rounds (the last half, the
    /// quarter before it, …), each round in Morton order.
    fn brio(&mut self, mut idx: Vec<u32>) -> Vec<u32> {
        for i in (1..idx.len()).rev() {
            let j = self.rng.below(i + 1);
            idx.swap(i, j);
        }
        let (mut lo, mut hi) = ([f64::MAX; 3], [f64::MIN; 3]);
        for &i in &idx {
            let p = self.p(i);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let span = (0..3).map(|k| hi[k] - lo[k]).fold(0.0, f64::max).max(1e-300);
        let key = |p: V3| -> u64 {
            let q = |k: usize| (((p[k] - lo[k]) / span) * 1_048_575.0).clamp(0.0, 1_048_575.0) as u64;
            morton(q(0), q(1), q(2))
        };
        let mut out = Vec::with_capacity(idx.len());
        let mut end = idx.len();
        let mut rounds = Vec::new();
        while end > 0 {
            let start = if end <= 64 { 0 } else { end / 2 };
            rounds.push((start, end));
            end = start;
        }
        for &(s, e) in rounds.iter().rev() {
            let mut r: Vec<u32> = idx[s..e].to_vec();
            r.sort_by_key(|&i| key(self.p(i)));
            out.extend(r);
        }
        out
    }

    fn alloc(&mut self, v: [u32; 4]) -> u32 {
        if let Some(t) = self.free.pop() {
            self.tets[t as usize] = v;
            self.nbr[t as usize] = [NONE; 4];
            self.alive[t as usize] = true;
            self.mark[t as usize] = 0;
            t
        } else {
            self.tets.push(v);
            self.nbr.push([NONE; 4]);
            self.alive.push(true);
            self.mark.push(0);
            (self.tets.len() - 1) as u32
        }
    }

    #[inline]
    fn is_ghost(&self, t: u32) -> bool {
        self.tets[t as usize][3] == GHOST
    }

    /// Does `p` conflict with tetrahedron `t` (strictly inside its circumsphere, or for a ghost
    /// strictly outside its hull face or in the face's circumcircle)?
    fn conflict(&self, t: u32, p: V3) -> bool {
        let v = self.tets[t as usize];
        if v[3] == GHOST {
            let o = orient(self.p(v[0]), self.p(v[1]), self.p(v[2]), p);
            o > 0.0 || (o == 0.0 && self.conflict(self.nbr[t as usize][3], p))
        } else {
            insphere(self.p(v[0]), self.p(v[1]), self.p(v[2]), self.p(v[3]), p) > 0.0
        }
    }

    /// A tetrahedron `p` conflicts with (the walk's end), or `None` if `p` is already a vertex.
    fn locate(&mut self, pi: u32) -> Option<u32> {
        let p = self.p(pi);
        let mut t = self.last;
        if !self.alive[t as usize] || self.is_ghost(t) {
            t = (0..self.tets.len() as u32).find(|&k| self.alive[k as usize] && !self.is_ghost(k))?;
        }
        let limit = 4 * self.tets.len() + 64;
        for _ in 0..limit {
            let v = self.tets[t as usize];
            let start = self.rng.below(4);
            let mut next = None;
            for k in 0..4 {
                let i = (start + k) % 4;
                let mut q = v;
                q[i] = pi;
                if orient(self.p(q[0]), self.p(q[1]), self.p(q[2]), self.p(q[3])) < 0.0 {
                    next = Some(self.nbr[t as usize][i]);
                    break;
                }
            }
            match next {
                // Past a hull face: that ghost conflicts.
                Some(n) if self.is_ghost(n) => return Some(n),
                Some(n) => t = n,
                None => {
                    if v.iter().any(|&w| self.p(w) == p) {
                        return None;
                    }
                    return Some(t);
                }
            }
        }
        // The walk should always end; if it cycles, search.
        (0..self.tets.len() as u32).find(|&k| self.alive[k as usize] && self.conflict(k, p))
    }

    fn insert(&mut self, pi: u32) {
        let Some(first) = self.locate(pi) else { return };
        let p = self.p(pi);
        if !self.conflict(first, p) {
            return;
        }
        self.stamp = self.stamp.wrapping_add(1).max(1);
        let stamp = self.stamp;
        let mut cavity = vec![first];
        self.mark[first as usize] = stamp;
        let mut boundary: Vec<(u32, usize, u32)> = Vec::new();
        let mut k = 0;
        while k < cavity.len() {
            let t = cavity[k];
            k += 1;
            for i in 0..4 {
                let n = self.nbr[t as usize][i];
                if self.mark[n as usize] == stamp {
                    continue;
                }
                if self.conflict(n, p) {
                    self.mark[n as usize] = stamp;
                    cavity.push(n);
                } else {
                    boundary.push((t, i, n));
                }
            }
        }
        // One new tetrahedron per boundary face: the old one with vertex i replaced by p.
        let mut made: Vec<u32> = Vec::with_capacity(boundary.len());
        let mut open: Vec<((u32, u32), u32, usize)> = Vec::with_capacity(boundary.len() * 3);
        for &(t, i, n) in &boundary {
            let mut v = self.tets[t as usize];
            v[i] = pi;
            // A ghost's slot 3 replaced by p makes a real tetrahedron (a hull face seen from p).
            let nt = self.alloc(v);
            self.nbr[nt as usize][i] = n;
            let back = self.nbr[n as usize].iter().position(|&x| x == t).unwrap();
            self.nbr[n as usize][back] = nt;
            made.push(nt);
            for j in 0..4 {
                if j == i {
                    continue;
                }
                // The face opposite j holds p and the edge of the two other old vertices.
                let (a, b) = edge_without_two(v, i, j);
                let key = if a < b { (a, b) } else { (b, a) };
                if let Some(pos) = open.iter().position(|(k2, _, _)| *k2 == key) {
                    let (_, ot, oj) = open.swap_remove(pos);
                    self.nbr[nt as usize][j] = ot;
                    self.nbr[ot as usize][oj] = nt;
                } else {
                    open.push((key, nt, j));
                }
            }
        }
        debug_assert!(open.is_empty(), "cavity faces left unmatched");
        for &t in &cavity {
            self.alive[t as usize] = false;
            self.free.push(t);
        }
        if let Some(&r) = made.iter().find(|&&t| !self.is_ghost(t)) {
            self.last = r;
        }
    }

    fn real_tets(&self) -> Vec<[u32; 4]> {
        (0..self.tets.len()).filter(|&t| self.alive[t] && self.tets[t][3] != GHOST).map(|t| self.tets[t]).collect()
    }
}

/// The two vertices of face `k` of a ghost `[a, b, c, G]` other than the ghost: face k is the
/// three vertices without v[k]; without the ghost that leaves an edge.
fn edge_without(v: [u32; 4], k: usize) -> (u32, u32) {
    let e: Vec<u32> = (0..3).filter(|&i| i != k).map(|i| v[i]).collect();
    (e[0], e[1])
}

/// The two entries of `v` at positions other than `i` and `j`.
fn edge_without_two(v: [u32; 4], i: usize, j: usize) -> (u32, u32) {
    let mut e = [0u32; 2];
    let mut n = 0;
    for (k, &x) in v.iter().enumerate() {
        if k != i && k != j {
            e[n] = x;
            n += 1;
        }
    }
    (e[0], e[1])
}

fn morton(x: u64, y: u64, z: u64) -> u64 {
    fn spread(mut v: u64) -> u64 {
        v &= 0x1f_ffff;
        v = (v | v << 32) & 0x1f00000000ffff;
        v = (v | v << 16) & 0x1f0000ff0000ff;
        v = (v | v << 8) & 0x100f00f00f00f00f;
        v = (v | v << 4) & 0x10c30c30c30c30c3;
        v = (v | v << 2) & 0x1249249249249249;
        v
    }
    spread(x) | spread(y) << 1 | spread(z) << 2
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::vol6;

    fn check(points: &[V3], hull_volume: f64) -> Vec<[u32; 4]> {
        let tets = tetrahedralize(points);
        let mut v = 0.0;
        for t in &tets {
            let [a, b, c, d] = t.map(|i| points[i as usize]);
            let o = orient(a, b, c, d);
            assert!(o > 0.0, "non-positive tetrahedron {t:?}");
            v += vol6(a, b, c, d) / 6.0;
        }
        assert!((v - hull_volume).abs() < 1e-9 * hull_volume.max(1.0), "volume {v} vs hull {hull_volume}");
        // Delaunay: no point strictly inside any circumsphere.
        if points.len() <= 400 {
            for t in &tets {
                let [a, b, c, d] = t.map(|i| points[i as usize]);
                for (k, &p) in points.iter().enumerate() {
                    if t.contains(&(k as u32)) {
                        continue;
                    }
                    assert!(insphere(a, b, c, d, p) <= 0.0, "point {k} inside the sphere of {t:?}");
                }
            }
        }
        tets
    }

    #[test]
    fn a_cube_of_grid_points() {
        // A 6×6×6 grid: every cube's corners are cospherical, every face's points coplanar.
        let mut pts = Vec::new();
        for i in 0..6 {
            for j in 0..6 {
                for k in 0..6 {
                    pts.push([i as f64, j as f64, k as f64]);
                }
            }
        }
        check(&pts, 125.0);
    }

    #[test]
    fn random_points_and_a_box_surface() {
        let mut rng = Rng::new(7);
        let mut pts = Vec::new();
        // The box [0,10]×[0,2]×[0,3]'s surface on a fine grid, plus random inside points.
        for i in 0..=20 {
            for j in 0..=4 {
                for k in 0..=6 {
                    let on = i == 0 || i == 20 || j == 0 || j == 4 || k == 0 || k == 6;
                    if on {
                        pts.push([i as f64 * 0.5, j as f64 * 0.5, k as f64 * 0.5]);
                    }
                }
            }
        }
        for _ in 0..150 {
            pts.push([rng.next_f64() * 10.0, rng.next_f64() * 2.0, rng.next_f64() * 3.0]);
        }
        // Duplicates are skipped.
        pts.push([0.0, 0.0, 0.0]);
        check(&pts, 60.0);
    }

    #[test]
    fn collinear_and_coplanar_runs() {
        // Points along the edges of a triangle prism only (many collinear), then its inside.
        let mut pts = Vec::new();
        for i in 0..=10 {
            let t = i as f64 / 10.0;
            for z in [0.0, 1.0] {
                pts.push([t, 0.0, z]);
                pts.push([0.0, t, z]);
                pts.push([t, 1.0 - t, z]);
            }
            pts.push([0.0, 0.0, t]);
            pts.push([1.0, 0.0, t]);
            pts.push([0.0, 1.0, t]);
        }
        check(&pts, 0.5);
    }
}

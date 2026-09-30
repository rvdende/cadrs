//! Tetrahedral meshing of one part (Delaunay refinement of the surface plus an interior lattice),
//! and the quadratic (10-node) elements built on it.
//!
//! 1. The kernel's tessellation is **welded** into one closed surface and **sampled** with
//!    points about the element size `h` apart (along its edges and inside its larger triangles).
//! 2. **Interior points**: a cubic lattice of spacing `h`, keeping points inside the part and at
//!    least `0.4 h` from its surface.
//! 3. The points are **tetrahedralized** ([`crate::delaunay`]) and the tetrahedra whose centroid
//!    is outside the part are **carved** away (ray parity over the surface). For a convex part
//!    the Delaunay mesh of its surface points *is* the part (its hull); for a concave one the
//!    dense surface sampling keeps the carved boundary on the faces.
//! 4. Pieces joined to the rest by less than a face (a tetrahedron hanging by an edge after
//!    carving) are dropped, so the mesh has no hinges.
//! 5. A node is added at the middle of every edge: **10-node tetrahedra**. Boundary triangles
//!    (6 nodes) are tagged with the CAD face they lie on, so loads can find them.

use std::collections::HashMap;

use crate::delaunay;
use crate::geom::{V3, add, area_vector, len, mid, scale, sub, vol6};
use crate::surface::{self, Surface, SurfaceIndex};

/// A tetrahedron's edges, in the order of its mid-edge nodes 4..10.
pub const TET_EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (0, 2), (0, 3), (1, 3), (2, 3)];

/// A boundary triangle of the mesh: corners (counter-clockwise from outside) then the mid-edge
/// nodes of edges 0–1, 1–2, 2–0; the CAD face it lies on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoundaryTri {
    pub nodes: [u32; 6],
    pub face: u32,
}

/// A quadratic tetrahedral mesh of one part.
#[derive(Debug, Clone, Default)]
pub struct TetMesh {
    /// Corner nodes first (`..corners`), then mid-edge nodes.
    pub nodes: Vec<V3>,
    pub corners: usize,
    /// Corners 0–3 (positively oriented), then the mid-edge nodes of [`TET_EDGES`].
    pub tets: Vec<[u32; 10]>,
    pub boundary: Vec<BoundaryTri>,
    /// The element size it was made with.
    pub h: f64,
    /// The volume of the tetrahedra.
    pub volume: f64,
    /// The volume of the part's surface (to compare: how well the mesh fills the part).
    pub surface_volume: f64,
}

/// Why a part couldn't be meshed.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshError {
    Empty,
    NotClosed(usize),
    NoTetrahedra,
}

impl std::fmt::Display for MeshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeshError::Empty => write!(f, "it has no surface"),
            MeshError::NotClosed(n) => write!(f, "its surface isn't closed ({n} open edges)"),
            MeshError::NoTetrahedra => write!(f, "no tetrahedra fit inside it (is it thinner than the element size?)"),
        }
    }
}

/// Meshes a closed surface with elements of size about `h`.
pub fn mesh(s: &Surface, h: f64) -> Result<TetMesh, MeshError> {
    let (lo, hi) = s.bounds().ok_or(MeshError::Empty)?;
    let diag = len(sub(hi, lo));
    if diag <= 0.0 || s.triangles.is_empty() {
        return Err(MeshError::Empty);
    }
    let h = h.max(diag * 1e-3);
    let w = surface::weld(s, diag * 1e-7);
    let open = surface::open_edges(&w);
    // A few open edges (a seam the tessellation didn't stitch) are survivable: ray parity still
    // classifies; a surface full of holes is not.
    if open > 0 && open * 20 > w.tris.len() {
        return Err(MeshError::NotClosed(open));
    }
    let index = SurfaceIndex::new(&w.pts, &w.tris, &w.face, h);
    // Points on the surface about h apart, with the faces they lie on.
    let (mut pts, point_faces) = surface::sample(&w, h);
    let n_surface = pts.len();

    // Interior lattice, centred in the bounds.
    let counts = [0, 1, 2].map(|k| ((hi[k] - lo[k]) / h).floor() as i64);
    let offset = [0, 1, 2].map(|k| lo[k] + ((hi[k] - lo[k]) - counts[k] as f64 * h) * 0.5);
    for i in 0..=counts[0] {
        for j in 0..=counts[1] {
            for k in 0..=counts[2] {
                let p = [offset[0] + i as f64 * h, offset[1] + j as f64 * h, offset[2] + k as f64 * h];
                if index.distance_within_cell(p) >= 0.4 * h && index.inside(p) {
                    pts.push(p);
                }
            }
        }
    }

    let tets = delaunay::tetrahedralize(&pts);
    // Carve: keep tetrahedra whose centroid is inside. A tetrahedron of four surface points
    // lying flat on the surface (its centroid near it) is decided the same way.
    let mut kept: Vec<[u32; 4]> = tets
        .into_iter()
        .filter(|t| {
            let c = scale(t.iter().fold([0.0; 3], |a, &i| add(a, pts[i as usize])), 0.25);
            index.inside(c)
        })
        .collect();
    kept = peel_slivers(&pts, &point_faces, kept);
    // Anything left that is flat to rounding (its gradients would overflow) goes too: a tiny
    // void is harmless, an infinite stiffness isn't.
    kept.retain(|t| quality(&pts, t) > 1e-6);
    if kept.is_empty() {
        return Err(MeshError::NoTetrahedra);
    }
    kept = largest_pieces(&pts, kept);

    // Renumber the used corner nodes.
    let mut map = vec![u32::MAX; pts.len()];
    let mut nodes = Vec::new();
    for t in &kept {
        for &v in t {
            if map[v as usize] == u32::MAX {
                map[v as usize] = nodes.len() as u32;
                nodes.push(pts[v as usize]);
            }
        }
    }
    let corners = nodes.len();
    let mut node_faces: Vec<Vec<u32>> = vec![Vec::new(); corners];
    for (old, &new) in map.iter().enumerate() {
        if new != u32::MAX && old < n_surface {
            node_faces[new as usize] = point_faces[old].clone();
        }
    }

    // Mid-edge nodes.
    let mut edge_node: HashMap<(u32, u32), u32> = HashMap::new();
    let mut tets10 = Vec::with_capacity(kept.len());
    let mut volume = 0.0;
    for t in &kept {
        let c = t.map(|v| map[v as usize]);
        let mut e = [0u32; 10];
        e[..4].copy_from_slice(&c);
        for (k, &(a, b)) in TET_EDGES.iter().enumerate() {
            let key = (c[a].min(c[b]), c[a].max(c[b]));
            let id = *edge_node.entry(key).or_insert_with(|| {
                nodes.push(mid(nodes[c[a] as usize], nodes[c[b] as usize]));
                (nodes.len() - 1) as u32
            });
            e[4 + k] = id;
        }
        let [a, b, cc, d] = c.map(|i| nodes[i as usize]);
        volume += vol6(a, b, cc, d) / 6.0;
        tets10.push(e);
    }

    // Boundary faces: a face of one tetrahedron only.
    let mut faces: HashMap<[u32; 3], (usize, usize)> = HashMap::new();
    for (ti, t) in tets10.iter().enumerate() {
        for i in 0..4 {
            let f = face_without(&t[..4], i);
            if faces.remove(&f).is_none() {
                faces.insert(f, (ti, i));
            }
        }
    }
    let mut boundary: Vec<BoundaryTri> = Vec::with_capacity(faces.len());
    let mut open: Vec<((usize, usize), BoundaryTri)> = faces
        .into_values()
        .map(|(ti, i)| {
            let t = tets10[ti];
            // Outward: the face opposite corner i, ordered so i is behind it.
            let others: Vec<usize> = (0..4).filter(|&k| k != i).collect();
            let (mut a, mut b, c) = (others[0], others[1], others[2]);
            let p = |k: usize| nodes[t[k] as usize];
            if vol6(p(a), p(b), p(c), p(i)) > 0.0 {
                std::mem::swap(&mut a, &mut b);
            }
            let m = |x: usize, y: usize| {
                let (x, y) = (t[x], t[y]);
                edge_node[&(x.min(y), x.max(y))]
            };
            let tri = BoundaryTri { nodes: [t[a], t[b], t[c], m(a, b), m(b, c), m(c, a)], face: u32::MAX };
            ((ti, i), tri)
        })
        .collect();
    open.sort_by_key(|(k, _)| *k);
    for (_, mut tri) in open {
        tri.face = face_of(&tri, &nodes, &node_faces, &index);
        boundary.push(tri);
    }

    Ok(TetMesh { nodes, corners, tets: tets10, boundary, h, volume, surface_volume: s.volume() })
}

/// The CAD face a boundary triangle lies on: the one its three corners share, else the face of
/// the surface triangle nearest its centroid.
fn face_of(tri: &BoundaryTri, nodes: &[V3], node_faces: &[Vec<u32>], index: &SurfaceIndex) -> u32 {
    let c = &tri.nodes[..3];
    let common: Vec<u32> = node_faces[c[0] as usize]
        .iter()
        .copied()
        .filter(|f| node_faces[c[1] as usize].contains(f) && node_faces[c[2] as usize].contains(f))
        .collect();
    if common.len() == 1 {
        return common[0];
    }
    let centroid = scale(add(add(nodes[c[0] as usize], nodes[c[1] as usize]), nodes[c[2] as usize]), 1.0 / 3.0);
    for reach in [1, 3, 8, 64] {
        if let Some((_, _, t, _)) = index.closest(centroid, reach) {
            let f = index.face[t as usize];
            if common.is_empty() || common.contains(&f) {
                return f;
            }
            return common[0];
        }
    }
    common.first().copied().unwrap_or(0)
}

/// A tetrahedron's face opposite corner `i` (of its first four entries), sorted: a key shared by
/// the two tetrahedra on either side.
fn face_without(t: &[u32], i: usize) -> [u32; 3] {
    let mut f = [0u32; 3];
    let mut n = 0;
    for (k, &v) in t.iter().take(4).enumerate() {
        if k != i {
            f[n] = v;
            n += 1;
        }
    }
    f.sort_unstable();
    f
}

/// A tetrahedron's volume over its longest edge cubed, 1 for a regular one.
fn quality(pts: &[V3], t: &[u32; 4]) -> f64 {
    let p = |k: usize| pts[t[k] as usize];
    let v = vol6(p(0), p(1), p(2), p(3)) / 6.0;
    let l = TET_EDGES.iter().map(|&(a, b)| len(sub(p(a), p(b)))).fold(0.0, f64::max);
    v / (l * l * l) * (6.0 * 2f64.sqrt())
}

/// Removes slivers from the boundary: four nearly coplanar points of one CAD face (a curved
/// face's chords) make a flat tetrahedron lying on the surface, which would only stiffen the
/// model badly. Peeled while they have a face on the boundary (a few passes). A flat
/// tetrahedron across a thin wall (its corners on both sides) is part of the wall and stays.
fn peel_slivers(pts: &[V3], point_faces: &[Vec<u32>], mut tets: Vec<[u32; 4]>) -> Vec<[u32; 4]> {
    let on_one_face = |t: &[u32; 4]| {
        let faces = |v: u32| point_faces.get(v as usize).map(Vec::as_slice).unwrap_or(&[]);
        faces(t[0]).iter().any(|f| t[1..].iter().all(|&v| faces(v).contains(f)))
    };
    const FLAT: f64 = 0.02;
    for _ in 0..4 {
        let mut count: HashMap<[u32; 3], u32> = HashMap::new();
        let face = |t: &[u32; 4], i: usize| face_without(t, i);
        for t in &tets {
            for i in 0..4 {
                *count.entry(face(t, i)).or_default() += 1;
            }
        }
        let before = tets.len();
        tets.retain(|t| !(quality(pts, t) < FLAT && on_one_face(t) && (0..4).any(|i| count[&face(t, i)] == 1)));
        if tets.len() == before {
            break;
        }
    }
    tets
}

/// Keeps the face-connected pieces of the mesh that matter (at least 1 % of its volume), so
/// nothing hangs on by an edge or a corner.
fn largest_pieces(pts: &[V3], tets: Vec<[u32; 4]>) -> Vec<[u32; 4]> {
    let n = tets.len();
    let mut faces: HashMap<[u32; 3], usize> = HashMap::new();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], mut x: usize) -> usize {
        while p[x] != x {
            p[x] = p[p[x]];
            x = p[x];
        }
        x
    }
    for (ti, t) in tets.iter().enumerate() {
        for i in 0..4 {
            let f = face_without(t, i);
            if let Some(&o) = faces.get(&f) {
                let (a, b) = (find(&mut parent, ti), find(&mut parent, o));
                parent[a] = b;
            } else {
                faces.insert(f, ti);
            }
        }
    }
    let vol = |t: &[u32; 4]| {
        let [a, b, c, d] = t.map(|i| pts[i as usize]);
        vol6(a, b, c, d) / 6.0
    };
    let mut piece_volume: HashMap<usize, f64> = HashMap::new();
    let mut total = 0.0;
    for (ti, t) in tets.iter().enumerate() {
        let r = find(&mut parent, ti);
        *piece_volume.entry(r).or_default() += vol(t);
        total += vol(t);
    }
    tets.into_iter()
        .enumerate()
        .filter(|(ti, _)| {
            let r = find(&mut parent, *ti);
            piece_volume[&r] >= 0.01 * total
        })
        .map(|(_, t)| t)
        .collect()
}

impl TetMesh {
    /// The boundary triangles on the given CAD faces.
    pub fn on_faces<'a>(&'a self, faces: &'a [u32]) -> impl Iterator<Item = &'a BoundaryTri> + 'a {
        self.boundary.iter().filter(move |t| faces.contains(&t.face))
    }

    /// A boundary triangle's area vector (outward normal × area), from its corners.
    pub fn area_vector(&self, t: &BoundaryTri) -> V3 {
        let [a, b, c] = [0, 1, 2].map(|k| self.nodes[t.nodes[k] as usize]);
        area_vector(a, b, c)
    }

    /// The smallest ratio of a tetrahedron's volume to its longest edge cubed, normalized so
    /// a regular tetrahedron scores 1 (a quality measure: slivers score near 0).
    pub fn min_quality(&self) -> f64 {
        let regular = 1.0 / (6.0 * 2f64.sqrt());
        self.tets
            .iter()
            .map(|t| {
                let p = |k: usize| self.nodes[t[k] as usize];
                let v = vol6(p(0), p(1), p(2), p(3)) / 6.0;
                let l = TET_EDGES.iter().map(|&(a, b)| len(sub(p(a), p(b)))).fold(0.0, f64::max);
                v / (l * l * l) / regular
            })
            .fold(f64::MAX, f64::min)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cuboid_meshes_exactly() {
        let s = Surface::cuboid([0.0; 3], [100.0, 10.0, 10.0]);
        let m = mesh(&s, 2.5).unwrap();
        assert!((m.volume - 10_000.0).abs() < 1e-6, "volume {}", m.volume);
        assert!(m.tets.len() > 1000);
        // Every boundary triangle lies on one of the six faces, and the faces' areas add up.
        let mut area = [0.0f64; 6];
        for t in &m.boundary {
            area[t.face as usize] += len(m.area_vector(t));
        }
        let want = [100.0, 100.0, 1000.0, 1000.0, 1000.0, 1000.0];
        for k in 0..6 {
            assert!((area[k] - want[k]).abs() < 1e-6, "face {k}: {} vs {}", area[k], want[k]);
        }
        // Outward normals.
        for t in &m.boundary {
            let n = m.area_vector(t);
            let axis = (t.face / 2) as usize;
            let sign = if t.face % 2 == 0 { -1.0 } else { 1.0 };
            assert!(n[axis] * sign > 0.0);
        }
        assert!(m.min_quality() > 0.0);
    }

    #[test]
    fn an_l_shaped_part_meshes_close_to_its_volume() {
        // An L: [0,20]×[0,10]×[0,5] ∪ [0,10]×[10,20]×[0,5] (volume 1500), as one surface.
        let mut s = Surface::default();
        let pts: [[f64; 2]; 6] = [[0.0, 0.0], [20.0, 0.0], [20.0, 10.0], [10.0, 10.0], [10.0, 20.0], [0.0, 20.0]];
        let base = |z: f64| pts.iter().map(|p| [p[0], p[1], z]).collect::<Vec<_>>();
        s.positions.extend(base(0.0));
        s.positions.extend(base(5.0));
        // Caps (fan triangulations of the L as two rectangles) and sides.
        let bottom = [[0u32, 3, 1], [1, 3, 2], [0, 5, 3], [3, 5, 4]];
        for t in bottom {
            s.triangles.push(t);
            s.faces.push(0);
            s.triangles.push([t[0] + 6, t[2] + 6, t[1] + 6]);
            s.faces.push(1);
        }
        for i in 0..6u32 {
            let j = (i + 1) % 6;
            s.triangles.push([i, j, j + 6]);
            s.triangles.push([i, j + 6, i + 6]);
            s.faces.extend([2 + i, 2 + i]);
        }
        assert!((s.volume() - 1500.0).abs() < 1e-9);
        let m = mesh(&s, 1.5).unwrap();
        assert!((m.volume - 1500.0).abs() < 15.0, "volume {}", m.volume);
    }
}

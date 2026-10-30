//! A solid from a closed triangle mesh (STL import, [`crate::Kernel::mesh_solid`]).
//!
//! 1. Corners within `tol` are welded into vertices; triangles that collapse are dropped, and
//!    repeated ones (the same corners) kept once, or dropped both when wound against each other
//!    (a wall of no thickness).
//! 2. Triangles wound against their neighbours are turned to agree, when the mesh is otherwise
//!    a closed surface (OCCT sewed such meshes, which orients them).
//! 3. An edge where two parts of the surface touch (four triangles, two each way) is paired up
//!    by the order of its triangles about it, and the vertices there are split per part, so each
//!    part keeps its own edge (OCCT accepted such meshes as wound).
//! 4. The mesh must then be closed and consistently oriented: every directed edge used by
//!    exactly one triangle and its reverse by exactly one other (an open mesh is an error).
//! 5. Coplanar neighbouring triangles are flooded into planar regions (normals within ~1e-5 rad
//!    and corners within `tol` of the region's first triangle's plane, so curved meshes don't
//!    drift into one face).
//! 6. Each region's boundary half-edges are chained into polygon loops; a boundary vertex used by
//!    only two boundary segments in line with each other is dropped, so straight edges are one
//!    edge. Loops run counter-clockwise about the outward normal, holes clockwise.
//! 7. A mesh oriented inside out (negative volume) has every face reversed.

use std::collections::HashMap;

use super::curve::Curve;
use super::math::*;
use super::pcurve;
use super::surface::Surface;
use super::tol;
use super::topo::*;

/// Welds points within `tol` (looking in neighbouring grid cells).
struct Weld {
    tol: f64,
    cells: HashMap<[i64; 3], Vec<usize>>,
    pts: Vec<P3>,
}

impl Weld {
    fn key(&self, p: &P3) -> [i64; 3] {
        [p.x, p.y, p.z].map(|c| (c / self.tol).floor() as i64)
    }
    fn add(&mut self, p: P3) -> usize {
        let k = self.key(&p);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    if let Some(v) = self.cells.get(&[k[0] + dx, k[1] + dy, k[2] + dz]) {
                        for &i in v {
                            if (self.pts[i] - p).norm() <= self.tol {
                                return i;
                            }
                        }
                    }
                }
            }
        }
        self.pts.push(p);
        let i = self.pts.len() - 1;
        self.cells.entry(k).or_default().push(i);
        i
    }
}

pub fn mesh_solid(triangles: &[[P3; 3]], tol: f64) -> Result<Body, String> {
    let tol = if tol.is_finite() && tol > 0.0 {
        tol
    } else {
        tol::POINT
    };
    let mut weld = Weld {
        tol,
        cells: HashMap::new(),
        pts: Vec::new(),
    };
    let mut tris: Vec<[usize; 3]> = Vec::with_capacity(triangles.len());
    for t in triangles {
        let ix = [weld.add(t[0]), weld.add(t[1]), weld.add(t[2])];
        if ix[0] == ix[1] || ix[1] == ix[2] || ix[2] == ix[0] {
            continue;
        }
        let (a, b, c) = (weld.pts[ix[0]], weld.pts[ix[1]], weld.pts[ix[2]]);
        if (b - a).cross(&(c - a)).norm() <= tol::TINY {
            continue;
        }
        tris.push(ix);
    }
    let mut pts = weld.pts;
    let tris = drop_repeats(tris);
    let tris = orient(tris)?;
    let tris = split_touching(tris, &mut pts)?;
    if tris.len() < 4 {
        return Err("fewer than four usable triangles".into());
    }
    let pts = &pts;
    // Directed edges.
    let mut half: HashMap<(usize, usize), usize> = HashMap::with_capacity(tris.len() * 3);
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            if half.insert((t[k], t[(k + 1) % 3]), ti).is_some() {
                return Err("the mesh is not consistently oriented or not manifold (an edge is used twice the same way)".into());
            }
        }
    }
    for &(a, b) in half.keys() {
        if !half.contains_key(&(b, a)) {
            return Err("the mesh is not closed (an edge is used by one triangle only)".into());
        }
    }
    let normal = |t: &[usize; 3]| {
        (pts[t[1]] - pts[t[0]])
            .cross(&(pts[t[2]] - pts[t[0]]))
            .normalize()
    };
    let normals: Vec<V3> = tris.iter().map(normal).collect();
    // Planar regions.
    let mut region = vec![usize::MAX; tris.len()];
    let mut regions: Vec<Vec<usize>> = Vec::new();
    for seed in 0..tris.len() {
        if region[seed] != usize::MAX {
            continue;
        }
        let r = regions.len();
        let (n0, p0) = (normals[seed], pts[tris[seed][0]]);
        let mut members = vec![seed];
        region[seed] = r;
        let mut stack = vec![seed];
        while let Some(t) = stack.pop() {
            for k in 0..3 {
                let (a, b) = (tris[t][k], tris[t][(k + 1) % 3]);
                let o = half[&(b, a)];
                if region[o] != usize::MAX || normals[o].dot(&n0) < 1.0 - 1e-10 {
                    continue;
                }
                if tris[o].iter().any(|&v| (pts[v] - p0).dot(&n0).abs() > tol) {
                    continue;
                }
                region[o] = r;
                members.push(o);
                stack.push(o);
            }
        }
        regions.push(members);
    }
    // Boundary half-edges per region, and each vertex's boundary valence.
    let mut bnd: Vec<Vec<(usize, usize)>> = vec![Vec::new(); regions.len()];
    let mut valence: HashMap<usize, usize> = HashMap::new();
    for (ti, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            if region[half[&(b, a)]] != region[ti] {
                bnd[region[ti]].push((a, b));
                if a < b {
                    *valence.entry(a).or_default() += 1;
                    *valence.entry(b).or_default() += 1;
                }
            }
        }
    }
    let mut body = Body::default();
    let mut vmap: HashMap<usize, usize> = HashMap::new();
    let mut emap: HashMap<(usize, usize), usize> = HashMap::new();
    for (ri, hs) in bnd.iter().enumerate() {
        // Chain the half-edges into loops.
        let mut from: HashMap<usize, Vec<usize>> = HashMap::new();
        for (i, &(a, _)) in hs.iter().enumerate() {
            from.entry(a).or_default().push(i);
        }
        let mut used = vec![false; hs.len()];
        let mut loops: Vec<Vec<usize>> = Vec::new();
        for s in 0..hs.len() {
            if used[s] {
                continue;
            }
            let mut l = Vec::new();
            let mut cur = s;
            loop {
                used[cur] = true;
                l.push(hs[cur].0);
                let end = hs[cur].1;
                if end == hs[s].0 {
                    break;
                }
                match from
                    .get(&end)
                    .and_then(|v| v.iter().copied().find(|&i| !used[i]))
                {
                    Some(n) => cur = n,
                    None => return Err("a face boundary doesn't close".into()),
                }
            }
            // Drop vertices in line with their neighbours and on no other boundary.
            let mut changed = true;
            while changed && l.len() > 3 {
                changed = false;
                for k in 0..l.len() {
                    let n = l.len();
                    let (a, v, b) = (l[(k + n - 1) % n], l[k], l[(k + 1) % n]);
                    if valence.get(&v) != Some(&2) {
                        continue;
                    }
                    let (pa, pv, pb) = (pts[a], pts[v], pts[b]);
                    let ab = pb - pa;
                    if (pv - pa).cross(&ab).norm() <= tol * ab.norm()
                        && (pv - pa).dot(&ab) > 0.0
                        && (pb - pv).dot(&ab) > 0.0
                    {
                        l.remove(k);
                        changed = true;
                        break;
                    }
                }
            }
            loops.push(l);
        }
        // The face: a plane through the region with its outward normal (Newell's, of all loops).
        let mut n = V3::zeros();
        for l in &loops {
            for k in 0..l.len() {
                let (p, q) = (pts[l[k]], pts[l[(k + 1) % l.len()]]);
                n += p.coords.cross(&q.coords);
            }
        }
        let n = if n.norm() > tol::TINY {
            n.normalize()
        } else {
            normals[regions[ri][0]]
        };
        let l0 = &loops[0];
        let o = pts[l0[0]];
        let f = Frame::from_xz(o, pts[l0[1]] - o, n);
        let surf = Surface::Plane { f };
        let area = |l: &[usize]| -> f64 {
            let q: Vec<P2> = l
                .iter()
                .map(|&v| {
                    let lp = f.local(&pts[v]);
                    P2::new(lp.x, lp.y)
                })
                .collect();
            polygon_area(&q)
        };
        let mut ls: Vec<(f64, Vec<usize>)> = loops.into_iter().map(|l| (area(&l), l)).collect();
        ls.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut face_loops = Vec::new();
        for (_, l) in ls {
            let mut co = Vec::with_capacity(l.len());
            for k in 0..l.len() {
                let (a, b) = (l[k], l[(k + 1) % l.len()]);
                let va = *vmap.entry(a).or_insert_with(|| body.add_vertex(pts[a]));
                let vb = *vmap.entry(b).or_insert_with(|| body.add_vertex(pts[b]));
                let key = (a.min(b), a.max(b));
                let e = *emap.entry(key).or_insert_with(|| {
                    let (p, q) = (pts[key.0], pts[key.1]);
                    let (c, len) = Curve::line(p, q);
                    let (v0, v1) = if key.0 == a { (va, vb) } else { (vb, va) };
                    body.add_edge(c, 0.0, len, v0, v1)
                });
                let ed = &body.edges[e];
                let pc = pcurve::pcurve(&surf, &ed.curve, ed.t[0], ed.t[1], None);
                co.push(Coedge {
                    edge: e,
                    fwd: key.0 == a,
                    pc,
                });
            }
            face_loops.push(co);
        }
        body.faces.push(Face {
            surf,
            sense: true,
            loops: face_loops,
        });
    }
    let nf = body.faces.len();
    body.shells.push(Shell {
        faces: (0..nf).collect(),
        closed: true,
    });
    body.solids.push(Solid { shells: vec![0] });
    if super::mass::body_volume(&body) < 0.0 {
        for f in &mut body.faces {
            Body::reverse_face(f);
        }
    }
    Ok(body)
}

/// The triangles with each set of corners once: repeats wound the same way kept once, pairs
/// wound against each other (a wall of no thickness) dropped.
fn drop_repeats(tris: Vec<[usize; 3]>) -> Vec<[usize; 3]> {
    // Corners sorted, and the winding (+1 when the sort is an even permutation).
    let canon = |t: &[usize; 3]| -> ([usize; 3], i32) {
        let mut c = *t;
        c.sort_unstable();
        let r = t.iter().position(|&v| v == c[0]).unwrap_or(0);
        (c, if t[(r + 1) % 3] == c[1] { 1 } else { -1 })
    };
    let mut net: HashMap<[usize; 3], (i32, usize)> = HashMap::with_capacity(tris.len());
    for (i, t) in tris.iter().enumerate() {
        let (c, w) = canon(t);
        let e = net.entry(c).or_insert((0, i));
        e.0 += w;
    }
    if net.len() == tris.len() {
        return tris;
    }
    let mut keep: Vec<usize> = net
        .values()
        .filter(|(w, _)| *w != 0)
        .map(|&(_, i)| i)
        .collect();
    keep.sort_unstable();
    keep.into_iter()
        .map(|i| {
            let (c, w) = canon(&tris[i]);
            let net_w = net[&c].0;
            if (net_w > 0) == (w > 0) {
                tris[i]
            } else {
                [tris[i][0], tris[i][2], tris[i][1]]
            }
        })
        .collect()
}

/// The triangles turned to agree with their neighbours across edges two triangles share, when
/// some edge is used twice the same way; each part that hangs together is oriented from its
/// first triangle (the volume's sign turns the whole body round later). Unchanged when already
/// consistent.
fn orient(mut tris: Vec<[usize; 3]>) -> Result<Vec<[usize; 3]>, String> {
    let mut directed: HashMap<(usize, usize), u32> = HashMap::with_capacity(tris.len() * 3);
    for t in &tris {
        for k in 0..3 {
            *directed.entry((t[k], t[(k + 1) % 3])).or_default() += 1;
        }
    }
    if directed
        .iter()
        .all(|(&(a, b), n)| directed.get(&(b, a)) == Some(n))
    {
        return Ok(tris);
    }
    let mut uses: HashMap<(usize, usize), Vec<usize>> = HashMap::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            uses.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    let mut seen = vec![false; tris.len()];
    for s in 0..tris.len() {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let mut stack = vec![s];
        while let Some(i) = stack.pop() {
            let t = tris[i];
            for k in 0..3 {
                let (a, b) = (t[k], t[(k + 1) % 3]);
                // Only across edges of exactly two triangles (a manifold edge).
                let [x, y] = uses[&(a.min(b), a.max(b))][..] else {
                    continue;
                };
                let j = if x == i { y } else { x };
                if seen[j] {
                    continue;
                }
                seen[j] = true;
                // Agreeing: the neighbour runs the edge b → a.
                let u = tris[j];
                let runs_ab = (0..3).any(|m| u[m] == a && u[(m + 1) % 3] == b);
                if runs_ab {
                    tris[j] = [u[0], u[2], u[1]];
                }
                stack.push(j);
            }
        }
    }
    Ok(tris)
}

/// Pairs the triangles at edges where parts of the surface touch (an edge used by several
/// triangles each way) by their order about the edge, and gives each part its own copy of the
/// vertices where the parts meet, so every directed edge is then used once (with `pts` grown by
/// the copies).
fn split_touching(mut tris: Vec<[usize; 3]>, pts: &mut Vec<P3>) -> Result<Vec<[usize; 3]>, String> {
    // The triangles (and the corner the edge starts at) using each directed edge.
    let mut half: HashMap<(usize, usize), Vec<(usize, usize)>> =
        HashMap::with_capacity(tris.len() * 3);
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            half.entry((t[k], t[(k + 1) % 3])).or_default().push((i, k));
        }
    }
    if half.values().all(|v| v.len() == 1) {
        return Ok(tris);
    }
    // across[i][k]: the triangle across triangle i's edge k (from corner k).
    let mut across: Vec<[usize; 3]> = vec![[usize::MAX; 3]; tris.len()];
    // (The edges in their order, so a mesh wrong at several says the same of it each time.)
    let mut edges: Vec<(usize, usize)> = half.keys().copied().collect();
    edges.sort_unstable();
    for (a, b) in edges {
        let fwd = &half[&(a, b)];
        let Some(back) = half.get(&(b, a)) else {
            continue;
        };
        if fwd.len() != back.len() {
            return Err("the mesh is not consistently oriented or not manifold (an edge is used more often one way than the other)".into());
        }
        if fwd.len() == 1 {
            across[fwd[0].0][fwd[0].1] = back[0].0;
            continue;
        }
        // About the edge a → b: each triangle's angle, from its third corner.
        let (pa, pb) = (pts[a], pts[b]);
        let axis = (pb - pa).normalize();
        let x = {
            let t = tris[fwd[0].0];
            let c = pts[t[(fwd[0].1 + 2) % 3]] - pa;
            (c - axis * c.dot(&axis)).normalize()
        };
        let y = axis.cross(&x);
        let angle = |&(i, k): &(usize, usize)| {
            let c = pts[tris[i][(k + 2) % 3]] - pa;
            c.dot(&y).atan2(c.dot(&x))
        };
        let mut around: Vec<(f64, bool, usize)> = fwd
            .iter()
            .map(|h| (angle(h), true, h.0))
            .chain(back.iter().map(|h| (angle(h), false, h.0)))
            .collect();
        around.sort_by(|p, q| p.0.total_cmp(&q.0));
        // A triangle running a → b has its outside turning positively about the edge, so the
        // material (and its partner) is the next one turning back.
        let n = around.len();
        for m in 0..n {
            let (_, is_fwd, i) = around[m];
            if !is_fwd {
                continue;
            }
            let (_, partner_fwd, j) = around[(m + n - 1) % n];
            if partner_fwd {
                return Err("the mesh is not consistently oriented or not manifold (the triangles about an edge don't alternate)".into());
            }
            let k = fwd.iter().find(|h| h.0 == i).map_or(0, |h| h.1);
            across[i][k] = j;
        }
    }
    // Corners joined across paired edges at the same vertex; each group of corners of a
    // vertex beyond the first gets its own copy of the vertex.
    let mut parent: Vec<usize> = (0..tris.len() * 3).collect();
    fn find(p: &mut [usize], x: usize) -> usize {
        let mut r = x;
        while p[r] != r {
            r = p[r];
        }
        let mut y = x;
        while p[y] != r {
            let n = p[y];
            p[y] = r;
            y = n;
        }
        r
    }
    for i in 0..tris.len() {
        for k in 0..3 {
            let j = across[i][k];
            if j == usize::MAX {
                continue;
            }
            let (a, b) = (tris[i][k], tris[i][(k + 1) % 3]);
            for v in [a, b] {
                let ci = 3 * i + (0..3).find(|&m| tris[i][m] == v).unwrap_or(0);
                let Some(m) = (0..3).find(|&m| tris[j][m] == v) else {
                    continue;
                };
                let (r1, r2) = (find(&mut parent, ci), find(&mut parent, 3 * j + m));
                if r1 != r2 {
                    parent[r1.max(r2)] = r1.min(r2);
                }
            }
        }
    }
    let mut copy: HashMap<(usize, usize), usize> = HashMap::new();
    let mut first: HashMap<usize, usize> = HashMap::new();
    for (i, t) in tris.iter_mut().enumerate() {
        for (k, corner) in t.iter_mut().enumerate() {
            let v = *corner;
            let root = find(&mut parent, 3 * i + k);
            let owner = *first.entry(v).or_insert(root);
            if owner == root {
                continue;
            }
            *corner = *copy.entry((v, root)).or_insert_with(|| {
                pts.push(pts[v]);
                pts.len() - 1
            });
        }
    }
    Ok(tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cube_of_triangles_with_split_sides() {
        let c = |x: f64, y: f64, z: f64| P3::new(x * 10.0, y * 10.0, z * 10.0);
        let quads = [
            [c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
            [c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
            [c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
            [c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.), c(1., 1., 0.)],
            [c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.), c(0., 1., 0.)],
            [c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
        ];
        // Each side as four triangles about its middle (the middles are interior vertices).
        let mut tris = Vec::new();
        for q in &quads {
            let m = P3::from((q[0].coords + q[1].coords + q[2].coords + q[3].coords) / 4.0);
            for k in 0..4 {
                tris.push([q[k], q[(k + 1) % 4], m]);
            }
        }
        let b = mesh_solid(&tris, 1e-6).unwrap();
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert_eq!(b.faces.len(), 6);
        assert_eq!(b.edges.len(), 12);
        assert!((super::super::mass::body_volume(&b) - 1000.0).abs() < 1e-9);
        // Inside out: reversed back.
        let inv: Vec<[P3; 3]> = tris.iter().map(|t| [t[0], t[2], t[1]]).collect();
        assert!(
            (super::super::mass::body_volume(&mesh_solid(&inv, 1e-6).unwrap()) - 1000.0).abs()
                < 1e-9
        );
        assert!(mesh_solid(&tris[..20], 1e-6).is_err());
    }

    /// A cube's twelve outward triangles, of side `s` from corner `o`.
    fn cube(o: P3, s: f64) -> Vec<[P3; 3]> {
        let c = |x: f64, y: f64, z: f64| o + V3::new(x, y, z) * s;
        let quads = [
            [c(0., 0., 0.), c(0., 1., 0.), c(1., 1., 0.), c(1., 0., 0.)],
            [c(0., 0., 1.), c(1., 0., 1.), c(1., 1., 1.), c(0., 1., 1.)],
            [c(0., 0., 0.), c(1., 0., 0.), c(1., 0., 1.), c(0., 0., 1.)],
            [c(0., 1., 0.), c(0., 1., 1.), c(1., 1., 1.), c(1., 1., 0.)],
            [c(0., 0., 0.), c(0., 0., 1.), c(0., 1., 1.), c(0., 1., 0.)],
            [c(1., 0., 0.), c(1., 1., 0.), c(1., 1., 1.), c(1., 0., 1.)],
        ];
        quads
            .iter()
            .flat_map(|q| [[q[0], q[1], q[2]], [q[0], q[2], q[3]]])
            .collect()
    }

    /// mirror_test's STL: two parts of the surface touch along an edge (four triangles there,
    /// two each way). Two cubes sharing an edge are one solid of two shells' worth of volume.
    #[test]
    fn cubes_touching_along_an_edge() {
        let mut tris = cube(P3::origin(), 10.0);
        tris.extend(cube(P3::new(10.0, 10.0, 0.0), 10.0));
        let b = mesh_solid(&tris, tol::POINT).unwrap();
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((super::super::mass::body_volume(&b) - 2000.0).abs() < tol::ROUNDOFF);
        assert_eq!(b.faces.len(), 12);
    }

    /// Triangles wound against their neighbours are turned; a repeated triangle counts once,
    /// and a pair wound against each other (a wall of no thickness) not at all.
    #[test]
    fn badly_wound_and_repeated_triangles() {
        let mut tris = cube(P3::origin(), 10.0);
        tris[3] = [tris[3][0], tris[3][2], tris[3][1]];
        tris[7] = [tris[7][1], tris[7][0], tris[7][2]];
        tris.push(tris[0]);
        let fin = [
            P3::new(0.0, 0.0, 0.0),
            P3::new(10.0, 0.0, 0.0),
            P3::new(5.0, 5.0, 5.0),
        ];
        tris.push(fin);
        tris.push([fin[0], fin[2], fin[1]]);
        let b = mesh_solid(&tris, tol::POINT).unwrap();
        assert!(b.check().is_empty(), "{:?}", b.check());
        assert!((super::super::mass::body_volume(&b) - 1000.0).abs() < tol::ROUNDOFF);
        // Still not a solid: open.
        assert!(mesh_solid(&tris[..11], tol::POINT).is_err());
    }
}

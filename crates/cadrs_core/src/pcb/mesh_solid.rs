//! A component's 3D model meshes as a part's [`Solid`] with no kernel body (a **mesh part**): its triangles grouped into faces that behave like a
//! modelled part's where it matters for fitting a board into an enclosure.
//!
//! - Each mesh (one model shape, one colour) is welded (vertices within 0.1 µm) and split into
//!   **smooth groups**: triangles joined across edges that bend less than [`SHARP_DEG`].
//! - Before that, facets chained across edges along one direction, each bending at most
//!   [`FACET_DEG`] and turning a quarter turn or more in all, are one **cylinder** (below), so
//!   an 8-sided pin of a VRML model has its axis; so is a smooth group whose normals are all
//!   square to one direction.
//! - In other groups, every set of coplanar neighbours is a **planar face** when it is a real face
//!   and not one facet of a tessellated curve: its area is at least [`MIN_FILL`] of the square
//!   of its extent, so a box side counts and a cylinder's thin facet strips don't. A planar face
//!   gets a [`PlaneFrame`] at its centroid (outward normal), so it can be sketched on and has an
//!   implicit mate connector.
//! - What is left of a group, by connectivity, is a **curved face**. When its normals are all
//!   square to one direction it is taken as a **cylinder**: that direction is its axis and a
//!   circle fitted to its points gives the axis' position and the radius (a pin, a hole, a
//!   barrel), so it has an axis connector and Measure gives its radius.
//! - Faces are named `FaceOrigin::Imported { body: mesh index, face: face index }` under `op`,
//!   in the model's own order, so moving the model never renames them.
//! - Edges are drawn where faces meet at a sharp angle and along open boundaries; tangent seams
//!   (a planar face running into a fillet) are kept but marked smooth.
//! - Each face shows its mesh's colour and opacity ([`Solid::looks`]).

use std::collections::{HashMap, VecDeque};

use cadrs_eda::model3d::Mesh;
use cadrs_kernel::SurfaceKind;
use cadrs_kernel::naming::{EdgeName, FaceName, FaceOrigin, OpId};
use cadrs_sketch::PlaneFrame;
use nalgebra::{Matrix3, SymmetricEigen, Vector3};

use crate::appearance::Appearance;
use crate::solid::{Solid, SolidEdge, SolidFace};

/// Faces that meet at more than this angle (degrees) are apart.
pub const SHARP_DEG: f64 = 30.0;
/// Coplanar: normals within this angle (degrees) and points within [`PLANE_TOL`] of the plane.
const PLANAR_DEG: f64 = 0.5;
const PLANE_TOL: f64 = 1e-3;
/// A planar face covers at least this much of the square of its extent.
pub const MIN_FILL: f64 = 0.02;
/// Vertices closer than this (mm) are one.
const WELD: f64 = 1e-4;

type V3 = Vector3<f64>;

fn v(p: [f32; 3]) -> V3 {
    V3::new(p[0] as f64, p[1] as f64, p[2] as f64)
}

fn arr(p: V3) -> [f64; 3] {
    [p.x, p.y, p.z]
}

/// One welded mesh: its points, triangles (welded indices), their area vectors, and the
/// original (smooth) vertex normals per triangle corner.
struct Welded {
    pts: Vec<V3>,
    tris: Vec<[usize; 3]>,
    normals: Vec<V3>,
    areas: Vec<f64>,
    corner_normals: Vec<[V3; 3]>,
}

fn weld(m: &Mesh) -> Welded {
    let mut index: HashMap<[i64; 3], usize> = HashMap::new();
    let mut pts = Vec::new();
    let mut map = Vec::with_capacity(m.positions.len());
    for p in &m.positions {
        let q = v(*p);
        let key = [
            (q.x / WELD).round() as i64,
            (q.y / WELD).round() as i64,
            (q.z / WELD).round() as i64,
        ];
        let i = *index.entry(key).or_insert_with(|| {
            pts.push(q);
            pts.len() - 1
        });
        map.push(i);
    }
    let mut w = Welded {
        pts,
        tris: vec![],
        normals: vec![],
        areas: vec![],
        corner_normals: vec![],
    };
    for t in m.indices.as_chunks::<3>().0 {
        let [a, b, c] = [map[t[0] as usize], map[t[1] as usize], map[t[2] as usize]];
        if a == b || b == c || a == c {
            continue;
        }
        let n = (w.pts[b] - w.pts[a]).cross(&(w.pts[c] - w.pts[a]));
        let area = n.norm() / 2.0;
        if area < 1e-12 {
            continue;
        }
        let cn = |i: u32| {
            m.normals
                .get(i as usize)
                .map(|q| v(*q))
                .filter(|q| q.norm() > 0.5)
                .unwrap_or(n / (2.0 * area))
        };
        w.corner_normals.push([cn(t[0]), cn(t[1]), cn(t[2])]);
        w.tris.push([a, b, c]);
        w.normals.push(n / (2.0 * area));
        w.areas.push(area);
    }
    w
}

/// The triangles on each undirected edge.
fn edge_map(tris: &[[usize; 3]]) -> HashMap<(usize, usize), Vec<usize>> {
    let mut m: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (i, t) in tris.iter().enumerate() {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            m.entry((a.min(b), a.max(b))).or_default().push(i);
        }
    }
    m
}

/// Groups triangles by flood fill over edges that `joins` accepts (`joins(from, to)`), each
/// group grown from its first triangle.
fn flood(
    n: usize,
    edges: &HashMap<(usize, usize), Vec<usize>>,
    tris: &[[usize; 3]],
    allowed: &[bool],
    mut joins: impl FnMut(usize, usize, usize) -> bool,
) -> Vec<Vec<usize>> {
    let mut seen = vec![false; n];
    let mut groups = vec![];
    for start in 0..n {
        if seen[start] || !allowed[start] {
            continue;
        }
        let mut g = vec![];
        let mut q = VecDeque::from([start]);
        seen[start] = true;
        while let Some(t) = q.pop_front() {
            g.push(t);
            for k in 0..3 {
                let (a, b) = (tris[t][k], tris[t][(k + 1) % 3]);
                for &u in &edges[&(a.min(b), a.max(b))] {
                    if !seen[u] && allowed[u] && joins(start, t, u) {
                        seen[u] = true;
                        q.push_back(u);
                    }
                }
            }
        }
        groups.push(g);
    }
    groups
}

/// What a face turned out to be.
enum Shape {
    Plane(V3),
    Cylinder { point: V3, dir: V3, radius: f64 },
    Other,
}

/// A circle through points in a plane (`u`, `v` axes): centre and radius, least squares
/// (Kåsa), and the RMS distance of the points from it.
fn fit_circle(pts: &[(f64, f64)]) -> Option<((f64, f64), f64, f64)> {
    if pts.len() < 3 {
        return None;
    }
    // x² + y² + D x + E y + F = 0.
    let mut a = Matrix3::zeros();
    let mut b = V3::zeros();
    for &(x, y) in pts {
        let row = V3::new(x, y, 1.0);
        a += row * row.transpose();
        b += row * (-(x * x + y * y));
    }
    let s = a.try_inverse()? * b;
    let (cx, cy) = (-s.x / 2.0, -s.y / 2.0);
    let r2 = cx * cx + cy * cy - s.z;
    if r2 <= 0.0 {
        return None;
    }
    let r = r2.sqrt();
    let rms = (pts
        .iter()
        .map(|&(x, y)| ((x - cx).hypot(y - cy) - r).powi(2))
        .sum::<f64>()
        / pts.len() as f64)
        .sqrt();
    Some(((cx, cy), r, rms))
}

/// Whether the triangles `g` of `w` make (part of) a cylinder, and which.
fn cylinder(w: &Welded, g: &[usize]) -> Option<(V3, V3, f64)> {
    let mut m = Matrix3::zeros();
    let mut total = 0.0;
    for &t in g {
        let n = w.normals[t];
        m += n * n.transpose() * w.areas[t];
        total += w.areas[t];
    }
    if total <= 0.0 {
        return None;
    }
    let e = SymmetricEigen::new(m / total);
    let (i, _) = e
        .eigenvalues
        .iter()
        .enumerate()
        .min_by(|a, b| a.1.total_cmp(b.1))?;
    let dir = e.eigenvectors.column(i).normalize();
    // Every normal square to the axis.
    if g.iter().any(|&t| w.normals[t].dot(&dir).abs() > 0.05) {
        return None;
    }
    let u = dir
        .cross(&if dir.x.abs() < 0.9 { V3::x() } else { V3::y() })
        .normalize();
    let vv = dir.cross(&u);
    let mut idx: Vec<usize> = g.iter().flat_map(|&t| w.tris[t]).collect();
    idx.sort_unstable();
    idx.dedup();
    let pts: Vec<(f64, f64)> = idx
        .iter()
        .map(|&i| (w.pts[i].dot(&u), w.pts[i].dot(&vv)))
        .collect();
    let ((cx, cy), r, rms) = fit_circle(&pts)?;
    if rms > (0.01 * r).max(0.002) || r > 1e4 {
        return None;
    }
    // The point on the axis level with the face's middle.
    let along = idx.iter().map(|&i| w.pts[i].dot(&dir)).sum::<f64>() / idx.len() as f64;
    Some((u * cx + vv * cy + dir * along, dir, r))
}

/// The boundary of the triangles `g` (edges not shared with another of them) chained into
/// closed or open polylines.
fn boundary(w: &Welded, g: &[usize]) -> Vec<Vec<usize>> {
    let mut count: HashMap<(usize, usize), (usize, usize, i32)> = HashMap::new();
    for &t in g {
        for k in 0..3 {
            let (a, b) = (w.tris[t][k], w.tris[t][(k + 1) % 3]);
            let e = count.entry((a.min(b), a.max(b))).or_insert((a, b, 0));
            e.2 += 1;
        }
    }
    let segs: Vec<(usize, usize)> = count
        .values()
        .filter(|e| e.2 == 1)
        .map(|e| (e.0, e.1))
        .collect();
    chain(&segs)
}

/// Segments joined end to end into polylines (a closed one repeats no point).
fn chain(segs: &[(usize, usize)]) -> Vec<Vec<usize>> {
    let mut next: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, s) in segs.iter().enumerate() {
        next.entry(s.0).or_default().push(i);
        next.entry(s.1).or_default().push(i);
    }
    let mut used = vec![false; segs.len()];
    let mut out = vec![];
    // Open chains first (from points with one segment), then the closed ones.
    let mut starts: Vec<usize> = next
        .iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(p, _)| *p)
        .collect();
    starts.sort_unstable();
    let mut order: Vec<usize> = starts;
    let mut rest: Vec<usize> = segs.iter().map(|s| s.0).collect();
    rest.sort_unstable();
    order.extend(rest);
    for start in order {
        let Some(&first) = next[&start].iter().find(|&&i| !used[i]) else {
            continue;
        };
        let mut line = vec![start];
        let mut at = start;
        let mut cur = Some(first);
        while let Some(i) = cur {
            used[i] = true;
            let s = segs[i];
            at = if s.0 == at { s.1 } else { s.0 };
            if at == start {
                break;
            }
            line.push(at);
            cur = next[&at].iter().copied().find(|&j| !used[j]);
        }
        out.push(line);
    }
    out
}

/// Facets of a prism-like pin meet at up to this angle (degrees): an 8-sided one is a cylinder.
pub const FACET_DEG: f64 = 46.0;

/// Coarse cylinders (a pin of a few sides): triangles joined across edges that run along one
/// direction, both square to it and bending at most [`FACET_DEG`], turning at least a quarter
/// turn in all, that fit a circle. Each found is a face; the rest are `free`.
fn prisms(
    w: &Welded,
    edges: &HashMap<(usize, usize), Vec<usize>>,
) -> (Vec<(Vec<usize>, Shape)>, Vec<bool>) {
    let n = w.tris.len();
    let bend = FACET_DEG.to_radians().cos();
    let mut free = vec![true; n];
    let mut tried = vec![false; n];
    let mut out = vec![];
    for seed in 0..n {
        if tried[seed] {
            continue;
        }
        let mut axis: Option<V3> = None;
        let mut group = vec![seed];
        let mut q = VecDeque::from([seed]);
        tried[seed] = true;
        while let Some(t) = q.pop_front() {
            for k in 0..3 {
                let (a, b) = (w.tris[t][k], w.tris[t][(k + 1) % 3]);
                let dir = (w.pts[b] - w.pts[a]).normalize();
                for &u in &edges[&(a.min(b), a.max(b))] {
                    if u == t || tried[u] {
                        continue;
                    }
                    let (nt, nu) = (w.normals[t], w.normals[u]);
                    let along = axis.is_none_or(|ax| dir.dot(&ax).abs() > 0.999);
                    // Coplanar halves of one facet join too (their diagonal isn't along the axis).
                    let same = nt.dot(&nu) > 0.9999;
                    if same
                        || (along
                            && nt.dot(&dir).abs() < 0.02
                            && nu.dot(&dir).abs() < 0.02
                            && nt.dot(&nu) >= bend)
                    {
                        if !same && axis.is_none() {
                            axis = Some(dir);
                        }
                        tried[u] = true;
                        group.push(u);
                        q.push_back(u);
                    }
                }
            }
        }
        let Some(ax) = axis else { continue };
        // Every facet square to the axis, and they turn a quarter turn or more.
        if group.iter().any(|&t| w.normals[t].dot(&ax).abs() > 0.02) {
            continue;
        }
        let spread = group
            .iter()
            .map(|&t| w.normals[t].dot(&w.normals[group[0]]))
            .fold(1.0f64, f64::min);
        if spread > 0.0 {
            continue;
        }
        if let Some((point, dir, radius)) = cylinder(w, &group) {
            group.iter().for_each(|&t| free[t] = false);
            out.push((group, Shape::Cylinder { point, dir, radius }));
        }
    }
    (out, free)
}

/// The faces of one welded mesh: each a list of triangles and its shape.
fn faces_of(w: &Welded) -> Vec<(Vec<usize>, Shape)> {
    let n = w.tris.len();
    let edges = edge_map(&w.tris);
    let sharp = SHARP_DEG.to_radians().cos();
    let (mut out, free) = prisms(w, &edges);
    let groups = flood(n, &edges, &w.tris, &free, |_, t, u| {
        w.normals[t].dot(&w.normals[u]) >= sharp
    });
    let flat = PLANAR_DEG.to_radians().cos();
    for g in groups {
        let mut in_group = vec![false; n];
        g.iter().for_each(|&t| in_group[t] = true);
        let group_area: f64 = g.iter().map(|&t| w.areas[t]).sum();
        // A group that is one cylinder (a pin, a hole, a barrel) is one face, however coarse
        // its facets.
        let coplanar = g
            .iter()
            .all(|&t| w.normals[t].dot(&w.normals[g[0]]) >= flat);
        if !coplanar && let Some((point, dir, radius)) = cylinder(w, &g) {
            out.push((g, Shape::Cylinder { point, dir, radius }));
            continue;
        }
        // Coplanar sets.
        let planes = flood(n, &edges, &w.tris, &in_group, |s, _, u| {
            let (ns, p0) = (w.normals[s], w.pts[w.tris[s][0]]);
            w.normals[u].dot(&ns) >= flat
                && w.tris[u]
                    .iter()
                    .all(|&i| (w.pts[i] - p0).dot(&ns).abs() <= PLANE_TOL)
        });
        let mut curved = vec![false; n];
        for p in planes {
            let area: f64 = p.iter().map(|&t| w.areas[t]).sum();
            let (mut lo, mut hi) = (V3::repeat(f64::MAX), V3::repeat(f64::MIN));
            for &t in &p {
                for &i in &w.tris[t] {
                    lo = lo.inf(&w.pts[i]);
                    hi = hi.sup(&w.pts[i]);
                }
            }
            let extent = (hi - lo).norm();
            let whole = (area - group_area).abs() <= 1e-9 * group_area.max(1.0);
            if whole || area >= MIN_FILL * extent * extent {
                let normal = w.normals[p[0]];
                out.push((p, Shape::Plane(normal)));
            } else {
                p.iter().for_each(|&t| curved[t] = true);
            }
        }
        for c in flood(n, &edges, &w.tris, &curved, |_, _, _| true) {
            let shape = match cylinder(w, &c) {
                Some((point, dir, radius)) => Shape::Cylinder { point, dir, radius },
                None => Shape::Other,
            };
            out.push((c, shape));
        }
    }
    out
}

/// The mesh part of `meshes` (in the frame the part is wanted in), its faces named under `op`,
/// with `opacity` (0–1) on every face's colour. See the module docs.
pub fn mesh_solid(meshes: &[Mesh], op: OpId, opacity: f64) -> Solid {
    let mut s = Solid::default();
    let alpha = (opacity.clamp(0.0, 1.0) * 255.0).round() as u8;
    for (mi, m) in meshes.iter().enumerate() {
        let w = weld(m);
        let faces = faces_of(&w);
        let edges = edge_map(&w.tris);
        // Which face each triangle went to.
        let mut face_of = vec![usize::MAX; w.tris.len()];
        let base = s.faces.len();
        for (fi, (tris, shape)) in faces.iter().enumerate() {
            let name = FaceName::new(
                op,
                FaceOrigin::Imported {
                    body: mi as u32,
                    face: fi as u32,
                },
            );
            let first = s.indices.len() / 3;
            let mut area = 0.0;
            let mut centroid = V3::zeros();
            for &t in tris {
                face_of[t] = base + fi;
                let [a, b, c] = w.tris[t];
                let corners = [a, b, c];
                for (k, &i) in corners.iter().enumerate() {
                    let normal = match shape {
                        Shape::Plane(n) => *n,
                        _ => w.corner_normals[t][k],
                    };
                    s.indices.push(s.positions.len() as u32);
                    s.positions.push(arr(w.pts[i]));
                    s.normals.push(arr(normal));
                }
                area += w.areas[t];
                centroid += (w.pts[a] + w.pts[b] + w.pts[c]) / 3.0 * w.areas[t];
            }
            let centroid = if area > 0.0 {
                centroid / area
            } else {
                w.pts[w.tris[tris[0]][0]]
            };
            let loops = boundary(&w, tris)
                .into_iter()
                .map(|l| l.into_iter().map(|i| arr(w.pts[i])).collect())
                .collect();
            let (plane, axis, kind, radius) = match shape {
                Shape::Plane(n) => {
                    let u = n
                        .cross(&if n.x.abs() < 0.9 { V3::x() } else { V3::y() })
                        .normalize();
                    let vv = n.cross(&u);
                    (
                        Some(PlaneFrame {
                            origin: arr(centroid),
                            u: arr(u),
                            v: arr(vv),
                        }),
                        None,
                        Some(SurfaceKind::Plane),
                        None,
                    )
                }
                Shape::Cylinder { point, dir, radius } => (
                    None,
                    Some((arr(*point), arr(*dir))),
                    Some(SurfaceKind::Cylinder),
                    Some(*radius),
                ),
                Shape::Other => (None, None, Some(SurfaceKind::Other), None),
            };
            s.faces.push(SolidFace {
                name,
                plane,
                first_triangle: first,
                triangle_count: tris.len(),
                loops,
                center: Some(arr(centroid)),
                axis,
                area: Some(area),
                kind,
                radius,
                fillet_radius: None,
            });
            s.looks.push((
                name,
                Appearance {
                    rgb: m.color,
                    alpha,
                },
            ));
        }
        // Edges: per pair of faces (or a face and nothing), the segments between them.
        let sharp = SHARP_DEG.to_radians().cos();
        // Per pair of faces: its segments, and whether they are all smooth.
        type Segments = (Vec<(usize, usize)>, bool);
        let mut by_pair: HashMap<(usize, usize), Segments> = HashMap::new();
        for (&(a, b), ts) in &edges {
            let (fa, fb, smooth) = match ts.as_slice() {
                [t] => (face_of[*t], usize::MAX, false),
                [t, u] if face_of[*t] != face_of[*u] => (
                    face_of[*t].min(face_of[*u]),
                    face_of[*t].max(face_of[*u]),
                    w.normals[*t].dot(&w.normals[*u]) >= sharp,
                ),
                _ => continue,
            };
            let e = by_pair.entry((fa, fb)).or_insert((vec![], smooth));
            e.0.push((a, b));
            e.1 &= smooth;
        }
        let mut pairs: Vec<_> = by_pair.into_iter().collect();
        pairs.sort_by_key(|(k, _)| *k);
        for ((fa, fb), (segs, smooth)) in pairs {
            let na = s.faces[fa].name;
            let nb = if fb == usize::MAX {
                na
            } else {
                s.faces[fb].name
            };
            for (k, line) in chain(&segs).into_iter().enumerate() {
                let mut points: Vec<[f64; 3]> = line.iter().map(|&i| arr(w.pts[i])).collect();
                // A closed chain repeats its first point to draw the whole loop.
                if line.len() > 2
                    && segs.iter().any(|&(a, b)| {
                        (a == line[0] && b == *line.last().unwrap())
                            || (b == line[0] && a == *line.last().unwrap())
                    })
                {
                    points.push(points[0]);
                }
                s.edges.push(SolidEdge {
                    name: EdgeName::new(na, nb, k as u32),
                    points,
                    circle: None,
                    ellipse: None,
                    tangent_group: None,
                    smooth,
                });
            }
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mesh_of(body: &cadrs_eda::model3d::Body) -> Vec<Mesh> {
        cadrs_eda::model3d::mesh(body)
    }

    #[test]
    fn a_box_has_six_planar_faces_with_their_planes() {
        let mut b = cadrs_eda::model3d::Body::default();
        b.cuboid([0.0, 0.0, 0.0], [4.0, 2.0, 1.0], [10, 20, 30]);
        let s = mesh_solid(&mesh_of(&b), uuid::Uuid::nil(), 1.0);
        assert_eq!(s.faces.len(), 6);
        assert!(
            s.faces
                .iter()
                .all(|f| f.kind == Some(SurfaceKind::Plane) && f.plane.is_some())
        );
        // The top face: normal +z, through z = 1, area 8.
        let top = s
            .faces
            .iter()
            .find(|f| f.plane.unwrap().normal()[2] > 0.99)
            .unwrap();
        assert!((top.plane.unwrap().origin[2] - 1.0).abs() < 1e-6);
        assert!((top.area.unwrap() - 8.0).abs() < 1e-6);
        // Its outline is one loop of 4 corners.
        assert_eq!(top.loops.len(), 1);
        assert_eq!(top.loops[0].len(), 4);
        // 12 sharp edges, none smooth.
        assert_eq!(s.edges.len(), 12);
        assert!(s.edges.iter().all(|e| !e.smooth));
        assert_eq!(s.looks.len(), 6);
        assert_eq!(s.looks[0].1.rgb, [10, 20, 30]);
    }

    #[test]
    fn a_pin_is_a_cylinder_with_its_axis_and_radius() {
        let mut b = cadrs_eda::model3d::Body::default();
        b.cylinder(
            [3.0, 4.0, -2.0],
            cadrs_eda::model3d::Axis::Z,
            0.5,
            6.0,
            [200, 200, 200],
        );
        let s = mesh_solid(&mesh_of(&b), uuid::Uuid::nil(), 1.0);
        let side = s
            .faces
            .iter()
            .find(|f| f.kind == Some(SurfaceKind::Cylinder))
            .expect("a cylindrical face");
        let (p, d) = side.axis.unwrap();
        assert!(d[2].abs() > 0.999, "{d:?}");
        assert!(
            (p[0] - 3.0).abs() < 1e-3 && (p[1] - 4.0).abs() < 1e-3,
            "{p:?}"
        );
        assert!((side.radius.unwrap() - 0.5).abs() < 5e-3);
        // Two planar ends.
        assert_eq!(
            s.faces
                .iter()
                .filter(|f| f.kind == Some(SurfaceKind::Plane))
                .count(),
            2
        );
    }

    #[test]
    fn an_eight_sided_pin_is_a_cylinder_too() {
        // A VRML-style pin: an octagonal prism, r = 0.32 across its corners, along x.
        let (r, len) = (0.32f32, 6.0f32);
        let ring: Vec<(f32, f32)> = (0..8)
            .map(|i| {
                let t = std::f32::consts::TAU * i as f32 / 8.0;
                (r * t.cos(), r * t.sin())
            })
            .collect();
        let mut m = Mesh {
            color: [200, 200, 200],
            ..Default::default()
        };
        for i in 0..8 {
            let (a, b) = (ring[i], ring[(i + 1) % 8]);
            let base = m.positions.len() as u32;
            m.positions.extend([
                [0.0, a.0, a.1],
                [0.0, b.0, b.1],
                [len, b.0, b.1],
                [len, a.0, a.1],
            ]);
            let mid = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let l = (mid.0 * mid.0 + mid.1 * mid.1).sqrt();
            m.normals.extend([[0.0, mid.0 / l, mid.1 / l]; 4]);
            m.indices
                .extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let s = mesh_solid(&[m], uuid::Uuid::nil(), 1.0);
        let cyl: Vec<_> = s
            .faces
            .iter()
            .filter(|f| f.kind == Some(SurfaceKind::Cylinder))
            .collect();
        assert_eq!(
            cyl.len(),
            1,
            "one cylinder, not eight strips: {:?}",
            s.faces.iter().map(|f| f.kind).collect::<Vec<_>>()
        );
        let (p, d) = cyl[0].axis.unwrap();
        assert!(
            d[0].abs() > 0.999 && p[1].abs() < 1e-3 && p[2].abs() < 1e-3,
            "{p:?} {d:?}"
        );
        assert!((cyl[0].radius.unwrap() - 0.32).abs() < 1e-3);
    }

    #[test]
    fn a_pin_header_has_a_top_face_per_pin() {
        // The generated header's pins are square: each has a top face, centred on its pad, to
        // sketch on or mate to.
        let lib = cadrs_eda::library::LibraryTable::builtin();
        let fp = lib
            .footprint("Connector_PinHeader_2.54mm:PinHeader_1x04_P2.54mm_Vertical")
            .unwrap();
        let (meshes, _) = crate::pcb::model::footprint_meshes(fp).unwrap().unwrap();
        let s = mesh_solid(&meshes, uuid::Uuid::nil(), 1.0);
        let top = s
            .faces
            .iter()
            .filter_map(|f| f.plane.filter(|p| p.normal()[2] > 0.999))
            .map(|p| p.origin[2])
            .fold(f64::MIN, f64::max);
        let tops: Vec<[f64; 3]> = s
            .faces
            .iter()
            .filter_map(|f| f.plane)
            .filter(|p| p.normal()[2] > 0.999 && (p.origin[2] - top).abs() < 1e-6)
            .map(|p| p.origin)
            .collect();
        for pad in &fp.pads {
            let (x, y) = (
                cadrs_eda::units::to_mm(pad.at.x),
                cadrs_eda::units::to_mm(pad.at.y),
            );
            assert!(
                tops.iter()
                    .any(|p| (p[0] - x).abs() < 1e-3 && (p[1] - y).abs() < 1e-3),
                "pin {} at ({x}, {y}): {tops:?}",
                pad.number
            );
        }
    }

    #[test]
    fn face_names_follow_the_model_not_where_it_is() {
        let lib = cadrs_eda::library::LibraryTable::builtin();
        let fp = lib
            .footprint("Package_SO:SOIC-8_3.9x4.9mm_P1.27mm")
            .unwrap();
        let (meshes, _) = crate::pcb::model::footprint_meshes(fp).unwrap().unwrap();
        let a = mesh_solid(&meshes, uuid::Uuid::nil(), 1.0);
        let m = cadrs_kernel::Motion {
            linear: Matrix3::identity(),
            translation: V3::new(50.0, -3.0, 1.6),
        };
        let moved: Vec<Mesh> = meshes
            .iter()
            .map(|g| crate::pcb::model::moved_mesh(g, &m))
            .collect();
        let b = mesh_solid(&moved, uuid::Uuid::nil(), 1.0);
        let names = |s: &Solid| s.faces.iter().map(|f| f.name).collect::<Vec<_>>();
        assert_eq!(names(&a), names(&b));
        assert!(
            a.faces.len() >= 6 + 8,
            "body and 8 leads: {}",
            a.faces.len()
        );
    }

    #[test]
    fn circles_fit_exactly() {
        let pts: Vec<(f64, f64)> = (0..12)
            .map(|i| {
                let t = i as f64 * 0.5;
                (2.0 + 3.0 * t.cos(), -1.0 + 3.0 * t.sin())
            })
            .collect();
        let ((cx, cy), r, rms) = fit_circle(&pts).unwrap();
        assert!(
            (cx - 2.0).abs() < 1e-9
                && (cy + 1.0).abs() < 1e-9
                && (r - 3.0).abs() < 1e-9
                && rms < 1e-9
        );
    }
}

/// The volume properties of a closed mesh part (signed tetrahedra from the origin, Tonon's
/// covariance for the inertia): its volume, area, centroid and inertia at unit density.
/// `None` for a mesh enclosing (next to) nothing, an open shell say.
pub fn mesh_mass(s: &Solid) -> Option<cadrs_kernel::MassProperties> {
    use nalgebra::{Matrix3, Point3, Vector3};
    let v = |i: u32| {
        let p = s.positions[i as usize];
        Vector3::new(p[0], p[1], p[2])
    };
    let canonical = Matrix3::new(2.0, 1.0, 1.0, 1.0, 2.0, 1.0, 1.0, 1.0, 2.0) / 120.0;
    let (mut volume, mut area) = (0.0, 0.0);
    let mut first = Vector3::zeros();
    let mut cov = Matrix3::zeros();
    for t in s.indices.as_chunks::<3>().0 {
        let (a, b, c) = (v(t[0]), v(t[1]), v(t[2]));
        area += (b - a).cross(&(c - a)).norm() / 2.0;
        let m = Matrix3::from_columns(&[a, b, c]);
        let det = m.determinant();
        volume += det / 6.0;
        first += det * (a + b + c) / 24.0;
        cov += det * m * canonical * m.transpose();
    }
    // Wound inward: all signs flip.
    if volume < 0.0 {
        volume = -volume;
        first = -first;
        cov = -cov;
    }
    if volume < 1e-6 {
        return None;
    }
    let com = first / volume;
    let c = cov - volume * com * com.transpose();
    let inertia = Matrix3::identity() * c.trace() - c;
    Some(cadrs_kernel::MassProperties {
        volume,
        surface_area: area,
        center_of_mass: Point3::from(com),
        inertia,
        length: 0.0,
    })
}

#[cfg(test)]
mod mass_tests {
    use super::*;

    #[test]
    fn a_box_mesh_has_its_volume_centroid_and_inertia() {
        let mut b = cadrs_eda::model3d::Body::default();
        b.cuboid([1.0, 2.0, 3.0], [3.0, 5.0, 7.0], [1, 1, 1]);
        let s = mesh_solid(&cadrs_eda::model3d::mesh(&b), uuid::Uuid::nil(), 1.0);
        let m = mesh_mass(&s).unwrap();
        // 2 × 3 × 4.
        assert!((m.volume - 24.0).abs() < 1e-9, "{}", m.volume);
        assert!((m.surface_area - 52.0).abs() < 1e-9);
        assert!((m.center_of_mass - nalgebra::Point3::new(2.0, 3.5, 5.0)).norm() < 1e-9);
        // Ixx = V (b² + c²) / 12 = 24 (9 + 16) / 12 = 50.
        assert!(
            (m.inertia[(0, 0)] - 50.0).abs() < 1e-9 && m.inertia[(0, 1)].abs() < 1e-9,
            "{}",
            m.inertia
        );
    }
}

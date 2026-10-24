//! The parts a job machines, as the strategies see them: in the setup's coordinates (the tool
//! points down −Z), as triangles for the 3D strategies and the simulation, and as exact faces,
//! edges and holes for the 2.5D ones.
//!
//! The studio fills a [`Model`] from the kernel (`cadrs_core::cam::geometry`): a tessellation
//! at the job's tolerance, each face's exact surface (planes with their normal, cylinders with
//! their axis and radius), each circular edge's exact circle, and exact plane sections
//! ([`Sections`]) at the levels a strategy asks for. Without the kernel (tests, a body that
//! won't section) [`Model::slice`] cuts the triangles instead.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::geom::{Aabb, P2, P3, cross, dot, normalize, sub};
use crate::region::{self, Region};

/// The kind of surface a face lies on.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Surface {
    /// A plane: its unit outward normal.
    Plane { normal: P3 },
    /// A circular cylinder: a point on its axis, the unit axis and the radius. `inner` when the
    /// material is outside it (a hole's wall).
    Cylinder {
        point: P3,
        axis: P3,
        radius: f64,
        inner: bool,
    },
    /// A cone, sphere, torus or free-form surface.
    Other,
}

/// A face of a part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Face {
    /// Its persistent name (for selections), as the studio writes it.
    pub name: String,
    /// Which body (index into [`Model::bodies`]).
    pub body: usize,
    pub surface: Surface,
    /// Its boundary loops (outer first), polylines without the closing point.
    pub loops: Vec<Vec<P3>>,
    /// Its triangles (indices into [`Model::triangles`]).
    pub triangles: Vec<u32>,
}

impl Face {
    /// A plane facing up (+Z within 0.01°).
    pub fn faces_up(&self) -> bool {
        matches!(self.surface, Surface::Plane { normal } if normal[2] > 1.0 - 1.5e-8)
    }

    /// Its height when it is a horizontal plane.
    pub fn z(&self) -> Option<f64> {
        match self.surface {
            Surface::Plane { normal } if normal[2].abs() > 1.0 - 1.5e-8 => {
                let n = self.loops.iter().map(|l| l.len()).sum::<usize>();
                (n > 0).then(|| self.loops.iter().flatten().map(|p| p[2]).sum::<f64>() / n as f64)
            }
            _ => None,
        }
    }
}

/// An edge of a part.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edge {
    pub name: String,
    pub body: usize,
    pub points: Vec<P3>,
    /// The exact circle (centre, unit normal, radius) of a circular edge.
    pub circle: Option<(P3, P3, f64)>,
}

/// One part (or workholding) body.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub name: String,
    /// Its triangles' range in [`Model::triangles`].
    pub triangles: std::ops::Range<usize>,
    pub bounds: Aabb,
}

/// What the strategies cut: parts in setup coordinates.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub bodies: Vec<Body>,
    /// Every body's triangles, counter-clockwise seen from outside.
    pub triangles: Vec<[P3; 3]>,
    pub faces: Vec<Face>,
    pub edges: Vec<Edge>,
    /// Exact plane sections the kernel worked out, by level.
    #[serde(default)]
    pub sections: Sections,
}

/// Exact horizontal sections by height (mm, keyed on 1 µm).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Sections(pub Vec<(f64, Region)>);

impl Sections {
    pub fn key(z: f64) -> i64 {
        (z * 1000.0).round() as i64
    }

    pub fn get(&self, z: f64) -> Option<&Region> {
        let k = Sections::key(z);
        self.0
            .iter()
            .find(|(q, _)| Sections::key(*q) == k)
            .map(|(_, r)| r)
    }

    pub fn insert(&mut self, z: f64, r: Region) {
        let k = Sections::key(z);
        self.0.retain(|(q, _)| Sections::key(*q) != k);
        self.0.push((z, r));
    }
}

/// A hole: a vertical cylindrical wall the material surrounds, through or blind.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Hole {
    pub center: P2,
    pub diameter: f64,
    /// The top of its wall.
    pub top: f64,
    /// The bottom of its wall.
    pub bottom: f64,
    /// Open at the bottom (nothing of the part under it).
    pub through: bool,
}

impl Model {
    /// A model of `triangles` as one body (tests and plain meshes): every triangle its own face
    /// with no exact surface.
    pub fn from_triangles(name: &str, triangles: Vec<[P3; 3]>) -> Model {
        let bounds = Aabb::of(triangles.iter().flatten().copied());
        let n = triangles.len();
        Model {
            bodies: vec![Body {
                name: name.into(),
                triangles: 0..n,
                bounds,
            }],
            triangles,
            faces: Vec::new(),
            edges: Vec::new(),
            sections: Sections::default(),
        }
    }

    /// Adds another model's bodies, faces and edges.
    pub fn append(&mut self, other: Model) {
        let t0 = self.triangles.len();
        let b0 = self.bodies.len();
        self.triangles.extend(other.triangles);
        self.bodies.extend(other.bodies.into_iter().map(|mut b| {
            b.triangles = b.triangles.start + t0..b.triangles.end + t0;
            b
        }));
        self.faces.extend(other.faces.into_iter().map(|mut f| {
            f.body += b0;
            for t in &mut f.triangles {
                *t += t0 as u32;
            }
            f
        }));
        self.edges.extend(other.edges.into_iter().map(|mut e| {
            e.body += b0;
            e
        }));
        self.sections = Sections::default();
    }

    pub fn bounds(&self) -> Aabb {
        self.bodies
            .iter()
            .fold(Aabb::EMPTY, |b, x| b.union(&x.bounds))
    }

    /// The part's footprint: everything of it seen from above, as a region (the union of its
    /// upward triangles' shadows).
    pub fn silhouette(&self) -> Region {
        self.shadow_above(f64::NEG_INFINITY)
    }

    /// The shadow of the material at or above `z`: where a tool coming down from above meets
    /// the part before reaching `z`. A vertical line through solid material above `z` leaves
    /// it through an upward-facing triangle above `z`, so the clipped upward triangles cover
    /// exactly that shadow.
    pub fn shadow_above(&self, z: f64) -> Region {
        self.shadows(&[z]).pop().unwrap_or_default()
    }

    /// [`Model::shadow_above`] at many heights at once, in their order. Worked out from the top
    /// down: the triangles wholly above a height are merged into a running union once, and only
    /// those crossing it are clipped, so many levels cost little more than one.
    pub fn shadows(&self, zs: &[f64]) -> Vec<Region> {
        // Upward triangles: (lowest z, highest z, index).
        let mut ups: Vec<(f64, f64, usize)> = self
            .triangles
            .iter()
            .enumerate()
            .filter(|(_, t)| cross(sub(t[1], t[0]), sub(t[2], t[0]))[2] > 1e-12)
            .map(|(i, t)| {
                let lo = t[0][2].min(t[1][2]).min(t[2][2]);
                let hi = t[0][2].max(t[1][2]).max(t[2][2]);
                (lo, hi, i)
            })
            .collect();
        ups.sort_by(|a, b| b.0.total_cmp(&a.0));
        let flat =
            |i: usize| -> Vec<P2> { self.triangles[i].iter().map(|p| [p[0], p[1]]).collect() };
        let mut order: Vec<usize> = (0..zs.len()).collect();
        order.sort_by(|a, b| zs[*b].total_cmp(&zs[*a]));
        let mut out = vec![Vec::new(); zs.len()];
        let mut acc: Region = Vec::new();
        let mut next = 0;
        for k in order {
            let z = zs[k];
            let mut batch = Vec::new();
            while next < ups.len() && ups[next].0 >= z {
                batch.push(flat(ups[next].2));
                next += 1;
            }
            if !batch.is_empty() {
                acc = region::union_regions([acc, region::union_all(batch)]);
            }
            let crossing = ups[next..].iter().filter(|u| u.1 > z).filter_map(|u| {
                let poly = clip_above(&self.triangles[u.2], z);
                (poly.len() >= 3).then(|| poly.iter().map(|p| [p[0], p[1]]).collect::<Vec<P2>>())
            });
            let cross = region::union_all(crossing);
            out[k] = if cross.is_empty() {
                acc.clone()
            } else {
                region::union(&acc, &cross)
            };
        }
        out
    }

    /// The section at `z`: exact when the kernel worked it out, else the triangles cut there.
    pub fn section(&self, z: f64) -> Region {
        if let Some(r) = self.sections.get(z) {
            return r.clone();
        }
        self.slice(z)
    }

    /// The triangles cut by the plane at `z` (a closed mesh gives closed loops), as a region.
    pub fn slice(&self, z: f64) -> Region {
        let key = |p: P2| ((p[0] * 1e6).round() as i64, (p[1] * 1e6).round() as i64);
        // Directed segments: material on the left, so outer loops run counter-clockwise.
        let mut next: HashMap<(i64, i64), Vec<P2>> = HashMap::new();
        let mut segs = Vec::new();
        for t in &self.triangles {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            let Some((a, b)) = cut_triangle(t, z) else {
                continue;
            };
            // Orient the cut so the outward normal is on the right.
            let d = [b[0] - a[0], b[1] - a[1]];
            let right = [d[1], -d[0]];
            let (a, b) = if right[0] * n[0] + right[1] * n[1] >= 0.0 {
                (a, b)
            } else {
                (b, a)
            };
            if key(a) == key(b) {
                continue;
            }
            next.entry(key(a)).or_default().push(b);
            segs.push((a, b));
        }
        type Key = (i64, i64);
        let mut used: HashMap<(Key, Key), bool> = HashMap::new();
        let mut rings = Vec::new();
        for (a, b) in &segs {
            if used.contains_key(&(key(*a), key(*b))) {
                continue;
            }
            let mut ring = vec![*a];
            let mut cur = (*a, *b);
            loop {
                used.insert((key(cur.0), key(cur.1)), true);
                ring.push(cur.1);
                if key(cur.1) == key(*a) {
                    ring.pop();
                    break;
                }
                let Some(cands) = next.get(&key(cur.1)) else {
                    break;
                };
                let Some(n) = cands
                    .iter()
                    .find(|c| !used.contains_key(&(key(cur.1), key(**c))))
                else {
                    break;
                };
                cur = (cur.1, *n);
                if ring.len() > segs.len() + 1 {
                    break;
                }
            }
            if ring.len() >= 3 {
                rings.push(ring);
            }
        }
        region::normalize(&rings)
    }

    /// The horizontal planar faces facing up, with their heights, highest first.
    pub fn floors(&self) -> Vec<(usize, f64)> {
        let mut v: Vec<(usize, f64)> = self
            .faces
            .iter()
            .enumerate()
            .filter(|(_, f)| f.faces_up())
            .filter_map(|(i, f)| f.z().map(|z| (i, z)))
            .collect();
        v.sort_by(|a, b| b.1.total_cmp(&a.1));
        v
    }

    /// The distinct heights of upward flat faces (to 1 µm), highest first.
    pub fn floor_levels(&self) -> Vec<f64> {
        let mut zs: Vec<f64> = self.floors().into_iter().map(|(_, z)| z).collect();
        zs.dedup_by(|a, b| (*a - *b).abs() < 1e-3);
        zs
    }

    /// The holes: vertical cylindrical walls the material surrounds, merged when they line up
    /// (a counterbore's two diameters stay separate).
    pub fn holes(&self) -> Vec<Hole> {
        let mut out: Vec<Hole> = Vec::new();
        let silhouette_bottom = self.bounds().min[2];
        for f in &self.faces {
            let Surface::Cylinder {
                point,
                axis,
                radius,
                inner: true,
            } = f.surface
            else {
                continue;
            };
            if axis[2].abs() < 1.0 - 1e-6 {
                continue;
            }
            let zs: Vec<f64> = f.loops.iter().flatten().map(|p| p[2]).collect();
            if zs.is_empty() {
                continue;
            }
            let top = zs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let bottom = zs.iter().copied().fold(f64::INFINITY, f64::min);
            let center = [point[0], point[1]];
            let d = radius * 2.0;
            if let Some(h) = out.iter_mut().find(|h| {
                (h.center[0] - center[0]).hypot(h.center[1] - center[1]) < 1e-3
                    && (h.diameter - d).abs() < 1e-3
            }) {
                h.top = h.top.max(top);
                h.bottom = h.bottom.min(bottom);
            } else {
                out.push(Hole {
                    center,
                    diameter: d,
                    top,
                    bottom,
                    through: false,
                });
            }
        }
        // Through when nothing of the part is under the hole's bottom at its centre.
        for h in &mut out {
            h.through = h.bottom <= silhouette_bottom + 1e-3
                || !self.material_below([h.center[0], h.center[1]], h.bottom - 1e-3);
        }
        out.sort_by(|a, b| {
            a.center[1]
                .total_cmp(&b.center[1])
                .then(a.center[0].total_cmp(&b.center[0]))
        });
        out
    }

    /// Whether a vertical line at `p` meets the part below `z`.
    pub fn material_below(&self, p: P2, z: f64) -> bool {
        self.triangles.iter().any(|t| {
            let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
            n[2].abs() > 1e-12 && point_in_tri2(p, t) && height_on(t, p).is_some_and(|h| h < z)
        })
    }

    /// The highest point of the part at `p` (`None` off it).
    pub fn top_at(&self, p: P2) -> Option<f64> {
        self.triangles
            .iter()
            .filter(|t| point_in_tri2(p, t))
            .filter_map(|t| height_on(t, p))
            .fold(None, |m: Option<f64>, h| Some(m.map_or(h, |m| m.max(h))))
    }

    /// Faces by name.
    pub fn face(&self, name: &str) -> Option<&Face> {
        self.faces.iter().find(|f| f.name == name)
    }

    pub fn edge(&self, name: &str) -> Option<&Edge> {
        self.edges.iter().find(|e| e.name == name)
    }
}

/// The part of a triangle at or above `z`, as a polygon.
pub fn clip_above(t: &[P3; 3], z: f64) -> Vec<P3> {
    if t.iter().all(|p| p[2] >= z) {
        return t.to_vec();
    }
    let mut out = Vec::new();
    for i in 0..3 {
        let (a, b) = (t[i], t[(i + 1) % 3]);
        let (ia, ib) = (a[2] >= z, b[2] >= z);
        if ia {
            out.push(a);
        }
        if ia != ib {
            let s = (z - a[2]) / (b[2] - a[2]);
            out.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s, z]);
        }
    }
    out
}

/// Where the plane at `z` cuts the triangle (a segment), if it does.
fn cut_triangle(t: &[P3; 3], z: f64) -> Option<(P2, P2)> {
    let mut pts = Vec::with_capacity(2);
    for i in 0..3 {
        let (a, b) = (t[i], t[(i + 1) % 3]);
        let (da, db) = (a[2] - z, b[2] - z);
        if (da < 0.0) != (db < 0.0) {
            let s = da / (da - db);
            pts.push([a[0] + (b[0] - a[0]) * s, a[1] + (b[1] - a[1]) * s]);
        }
    }
    (pts.len() == 2).then(|| (pts[0], pts[1]))
}

pub fn point_in_tri2(p: P2, t: &[P3; 3]) -> bool {
    let s = |a: P3, b: P3| (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]);
    let (d1, d2, d3) = (s(t[0], t[1]), s(t[1], t[2]), s(t[2], t[0]));
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// The triangle's height over `p` (its plane's), `None` for a vertical triangle.
pub fn height_on(t: &[P3; 3], p: P2) -> Option<f64> {
    let n = normalize(cross(sub(t[1], t[0]), sub(t[2], t[0])));
    if n[2].abs() < 1e-12 {
        return None;
    }
    Some(t[0][2] - (n[0] * (p[0] - t[0][0]) + n[1] * (p[1] - t[0][1])) / n[2])
}

/// The triangles of an axis-aligned box (outward, counter-clockwise).
pub fn box_triangles(b: &Aabb) -> Vec<[P3; 3]> {
    let c = b.corners();
    let quads = [
        [0, 3, 2, 1], // bottom (−Z)
        [4, 5, 6, 7], // top
        [0, 1, 5, 4], // −Y
        [1, 2, 6, 5], // +X
        [2, 3, 7, 6], // +Y
        [3, 0, 4, 7], // −X
    ];
    let mut out = Vec::new();
    for q in quads {
        out.push([c[q[0]], c[q[1]], c[q[2]]]);
        out.push([c[q[0]], c[q[2]], c[q[3]]]);
    }
    out
}

/// The triangles of a vertical cylinder (axis along Z), `n` sides, outward.
pub fn cylinder_triangles(center: P2, radius: f64, z0: f64, z1: f64, n: usize) -> Vec<[P3; 3]> {
    let n = n.max(3);
    let p = |i: usize, z: f64| {
        let a = std::f64::consts::TAU * (i % n) as f64 / n as f64;
        [
            center[0] + radius * a.cos(),
            center[1] + radius * a.sin(),
            z,
        ]
    };
    let mut out = Vec::new();
    for i in 0..n {
        let (a0, a1, b0, b1) = (p(i, z0), p(i + 1, z0), p(i, z1), p(i + 1, z1));
        out.push([a0, a1, b1]);
        out.push([a0, b1, b0]);
        out.push([[center[0], center[1], z1], b0, b1]);
        out.push([[center[0], center[1], z0], a1, a0]);
    }
    out
}

/// A unit normal's angle from +Z (degrees).
pub fn tilt_deg(n: P3) -> f64 {
    dot(normalize(n), [0.0, 0.0, 1.0])
        .clamp(-1.0, 1.0)
        .acos()
        .to_degrees()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plate_with_hole() -> Model {
        // A 40 × 30 × 10 box (triangles), with an exact hole face of Ø8 at (20, 15).
        let b = Aabb::new([0.0, 0.0, 0.0], [40.0, 30.0, 10.0]);
        let mut m = Model::from_triangles("plate", box_triangles(&b));
        let ring = |z: f64| -> Vec<P3> {
            (0..32)
                .map(|i| {
                    let a = std::f64::consts::TAU * i as f64 / 32.0;
                    [20.0 + 4.0 * a.cos(), 15.0 + 4.0 * a.sin(), z]
                })
                .collect()
        };
        m.faces.push(Face {
            name: "hole".into(),
            body: 0,
            surface: Surface::Cylinder {
                point: [20.0, 15.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                radius: 4.0,
                inner: true,
            },
            loops: vec![ring(10.0), ring(0.0)],
            triangles: vec![],
        });
        m
    }

    #[test]
    fn silhouette_and_slice_of_a_box() {
        let m = Model::from_triangles(
            "b",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 20.0, 5.0])),
        );
        assert!((region::area(&m.silhouette()) - 200.0).abs() < 1e-6);
        let s = m.slice(2.5);
        assert!((region::area(&s) - 200.0).abs() < 1e-6, "{s:?}");
        assert!(m.slice(6.0).is_empty());
        assert!(m.shadow_above(6.0).is_empty());
        assert_eq!(m.top_at([5.0, 5.0]), Some(5.0));
        assert_eq!(m.top_at([50.0, 5.0]), None);
    }

    #[test]
    fn holes_found_from_exact_faces() {
        let m = plate_with_hole();
        let h = m.holes();
        assert_eq!(h.len(), 1);
        assert!((h[0].diameter - 8.0).abs() < 1e-9);
        assert_eq!(h[0].top, 10.0);
        assert!(h[0].through);
    }

    #[test]
    fn exact_sections_win() {
        let mut m = Model::from_triangles(
            "b",
            box_triangles(&Aabb::new([0.0, 0.0, 0.0], [10.0, 10.0, 5.0])),
        );
        m.sections
            .insert(2.0, vec![region::rect([0.0, 0.0], [1.0, 1.0])]);
        assert!((region::area(&m.section(2.0)) - 1.0).abs() < 1e-9);
        assert!((region::area(&m.section(3.0)) - 100.0).abs() < 1e-6);
    }

    #[test]
    fn stepped_shadow() {
        // Two boxes: a base 20×20×5 and a boss 5×5 up to 15.
        let mut tris = box_triangles(&Aabb::new([0.0, 0.0, 0.0], [20.0, 20.0, 5.0]));
        tris.extend(box_triangles(&Aabb::new(
            [5.0, 5.0, 5.0],
            [10.0, 10.0, 15.0],
        )));
        let m = Model::from_triangles("s", tris);
        assert!((region::area(&m.shadow_above(10.0)) - 25.0).abs() < 1e-6);
        assert!((region::area(&m.shadow_above(2.0)) - 400.0).abs() < 1e-6);
    }
}

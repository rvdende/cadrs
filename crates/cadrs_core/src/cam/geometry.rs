//! The parts as CAM sees them, worked out by the kernel: each part's exact body placed in a
//! setup's (or work plane's) coordinates, tessellated at the job's tolerance, with every face's
//! exact surface (planes with their normals, cylinders with their axes and radii, so holes
//! are recognised exactly), every edge's polyline and exact circle, and exact plane sections
//! (a half-space cut's caps, [`crate::section::section_cap`]) at the heights a strategy needs.
//!
//! [`model`] asks the kernel thread (or the kernel process) for it as a
//! [`crate::rebuild::task::Task`]; [`model_from_parts`] makes the same model from the parts'
//! display meshes (which also come from the kernel, at the display's tolerance) when the
//! kernel can't.
//!
//! Faces and edges are named for drive selections by [`face_key`] and [`edge_key`]: the part
//! and its persistent name, so a selection survives edits that keep the face.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use cadrs_cam::geom::{Frame, P3, cross, dot, normalize, sub};
use cadrs_cam::model::{Edge, Face, Model, Surface};
use cadrs_cam::region;

use crate::document::Feature;
use crate::ids::PartId;
use crate::parts::Part;
use crate::solid::{EdgeName, FaceName};

/// The key a drive selection names a face by.
pub fn face_key(part: PartId, name: &FaceName) -> String {
    format!(
        "{}/{}/{}",
        part.feature,
        part.index,
        ron::to_string(name).unwrap_or_default()
    )
}

/// The key a drive selection names an edge by.
pub fn edge_key(part: PartId, name: &EdgeName) -> String {
    format!(
        "{}/{}/{}",
        part.feature,
        part.index,
        ron::to_string(name).unwrap_or_default()
    )
}

/// The rotation (rows) and translation that take model coordinates into `frame`'s.
pub fn to_frame(frame: &Frame) -> ([[f64; 3]; 3], [f64; 3]) {
    let r = [frame.x, frame.y, frame.z];
    let t = [
        -dot(r[0], frame.origin),
        -dot(r[1], frame.origin),
        -dot(r[2], frame.origin),
    ];
    (r, t)
}

/// What [`model`] works from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRequest {
    /// The studio's Derived features ([`super::CamStudio::part_features`]).
    pub features: Vec<Feature>,
    pub parts: Vec<PartId>,
    /// The coordinates to work in (a setup's or work plane's frame, in model coordinates).
    pub frame: Frame,
    /// Chordal tolerance of the tessellation (mm).
    pub tolerance: f64,
    /// Heights (in the frame) to section the parts at exactly.
    pub levels: Vec<f64>,
}

/// The model the kernel works out (see the module docs), on the kernel thread.
pub fn model(req: ModelRequest) -> crate::rebuild::PendingJob<Result<Model, String>> {
    crate::rebuild::task::run(CamModel(req))
}

/// [`model`] as a task.
#[derive(Serialize, Deserialize)]
pub struct CamModel(pub ModelRequest);

impl crate::rebuild::task::Task for CamModel {
    const NAME: &'static str = "cam-model";
    type Output = Result<Model, String>;
    fn run(self, r: &mut crate::rebuild::Rebuilder) -> Self::Output {
        model_on(r, &self.0)
    }
}

/// Registers the CAM tasks with the kernel process (the app does at start).
pub fn register_tasks() {
    crate::rebuild::task::register::<CamModel>();
}

#[cfg(feature = "brep")]
fn model_on(r: &mut crate::rebuild::Rebuilder, req: &ModelRequest) -> Result<Model, String> {
    use cadrs_kernel::{SurfaceKind, Tessellation};
    let (rot, trans) = to_frame(&req.frame);
    let mut out = Model::default();
    let mut sections: HashMap<i64, (f64, Vec<region::Ring>)> = HashMap::new();
    for &part in &req.parts {
        let (body, names) = r.placed_body_named(&req.features, part, rot, trans)?;
        let result = (|| -> Result<Model, String> {
            let k = r.kernel_mut();
            let mesh = k
                .tessellate(
                    body,
                    Tessellation {
                        deflection: req.tolerance.clamp(0.001, 0.1),
                        angle: 8f64.to_radians(),
                    },
                )
                .map_err(|e| e.to_string())?;
            let infos = k.faces(body).map_err(|e| e.to_string())?;
            let edges = k.edges(body).map_err(|e| e.to_string())?;
            let pos: Vec<P3> = mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect();
            let tris: Vec<[P3; 3]> = mesh
                .indices
                .iter()
                .map(|t| t.map(|i| pos[i as usize]))
                .collect();
            let mut by_face: HashMap<u64, Vec<u32>> = HashMap::new();
            for (i, f) in mesh.triangle_faces.iter().enumerate() {
                by_face.entry(f.0).or_default().push(i as u32);
            }
            let mut m = Model::from_triangles(&format!("{:?}", part.feature), tris);
            for info in &infos {
                let ids = by_face.remove(&info.id.0).unwrap_or_default();
                let face_tris: Vec<[P3; 3]> =
                    ids.iter().map(|&i| m.triangles[i as usize]).collect();
                let surface = match info.kind {
                    SurfaceKind::Plane => Surface::Plane {
                        normal: mean_normal(&face_tris),
                    },
                    SurfaceKind::Cylinder => match (info.axis, info.radius) {
                        (Some(ax), Some(radius)) => {
                            let point = [ax.origin.x, ax.origin.y, ax.origin.z];
                            let axis = [ax.dir.x, ax.dir.y, ax.dir.z];
                            Surface::Cylinder {
                                point,
                                axis,
                                radius,
                                inner: faces_axis(&face_tris, point, axis),
                            }
                        }
                        _ => Surface::Other,
                    },
                    _ => Surface::Other,
                };
                let name = names
                    .face(info.id)
                    .map(|n| face_key(part, &n))
                    .unwrap_or_else(|| format!("{part:?}#f{}", info.id.0));
                m.faces.push(Face {
                    name,
                    body: 0,
                    surface,
                    loops: face_loops(&face_tris),
                    triangles: ids,
                });
            }
            let polylines: HashMap<u64, Vec<P3>> = mesh
                .edges
                .iter()
                .map(|(id, pts)| (id.0, pts.iter().map(|p| [p.x, p.y, p.z]).collect()))
                .collect();
            for e in &edges {
                let Some(points) = polylines.get(&e.id.0).cloned() else {
                    continue;
                };
                let name = names
                    .edge(e.id)
                    .map(|n| edge_key(part, &n))
                    .unwrap_or_else(|| format!("{part:?}#e{}", e.id.0));
                m.edges.push(Edge {
                    name,
                    body: 0,
                    points,
                    circle: e.circle.map(|c| {
                        (
                            [c.center.x, c.center.y, c.center.z],
                            [c.normal.x, c.normal.y, c.normal.z],
                            c.radius,
                        )
                    }),
                });
            }
            Ok(m)
        })();
        // Exact sections: the cap a half-space cut leaves at each height.
        if result.is_ok() {
            for &z in &req.levels {
                let plane = crate::section::SectionPlane {
                    origin: [0.0, 0.0, z],
                    normal: [0.0, 0.0, 1.0],
                };
                if let Ok(cap) = crate::section::section_cap(r.kernel_mut(), body, &plane) {
                    let rings = cap.triangles.iter().map(|t| {
                        t.iter()
                            .map(|p| [p[0] as f64, p[1] as f64])
                            .collect::<Vec<_>>()
                    });
                    let e = sections
                        .entry(cadrs_cam::model::Sections::key(z))
                        .or_insert((z, Vec::new()));
                    e.1.extend(rings);
                }
            }
        }
        r.release_body(body);
        out.append(result?);
    }
    for (_, (z, rings)) in sections {
        out.sections.insert(z, region::union_all(rings));
    }
    Ok(out)
}

#[cfg(not(feature = "brep"))]
fn model_on(_r: &mut crate::rebuild::Rebuilder, _req: &ModelRequest) -> Result<Model, String> {
    Err("the kernel is off in this build".into())
}

/// The area-weighted normal of triangles.
fn mean_normal(tris: &[[P3; 3]]) -> P3 {
    let mut n = [0.0; 3];
    for t in tris {
        let c = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        n = [n[0] + c[0], n[1] + c[1], n[2] + c[2]];
    }
    normalize(n)
}

/// Whether a cylinder's triangles face its axis (the material is outside: a hole's wall).
fn faces_axis(tris: &[[P3; 3]], point: P3, axis: P3) -> bool {
    let mut score = 0.0;
    for t in tris {
        let c = [
            (t[0][0] + t[1][0] + t[2][0]) / 3.0,
            (t[0][1] + t[1][1] + t[2][1]) / 3.0,
            (t[0][2] + t[1][2] + t[2][2]) / 3.0,
        ];
        let d = sub(c, point);
        let radial = sub(d, cadrs_cam::geom::scale(axis, dot(d, axis)));
        let n = cross(sub(t[1], t[0]), sub(t[2], t[0]));
        score += dot(n, radial);
    }
    score < 0.0
}

/// A face's boundary loops from its triangles (the sides no other of its triangles shares),
/// largest first.
pub fn face_loops(tris: &[[P3; 3]]) -> Vec<Vec<P3>> {
    let key = |p: P3| {
        (
            (p[0] * 1e5).round() as i64,
            (p[1] * 1e5).round() as i64,
            (p[2] * 1e5).round() as i64,
        )
    };
    type K = (i64, i64, i64);
    let mut count: HashMap<(K, K), i32> = HashMap::new();
    let mut pos: HashMap<K, P3> = HashMap::new();
    for t in tris {
        for i in 0..3 {
            let (a, b) = (t[i], t[(i + 1) % 3]);
            let (ka, kb) = (key(a), key(b));
            if ka == kb {
                continue;
            }
            pos.insert(ka, a);
            pos.insert(kb, b);
            *count.entry((ka, kb)).or_default() += 1;
        }
    }
    // Boundary sides: used one way and not back.
    let mut next: HashMap<K, Vec<K>> = HashMap::new();
    for (&(a, b), &n) in &count {
        let back = count.get(&(b, a)).copied().unwrap_or(0);
        for _ in 0..(n - back).max(0) {
            next.entry(a).or_default().push(b);
        }
    }
    let mut loops = Vec::new();
    let mut starts: Vec<K> = next.keys().copied().collect();
    starts.sort();
    for s in starts {
        while let Some(first) = next.get_mut(&s).and_then(|v| v.pop()) {
            let mut ring = vec![pos[&s]];
            let mut cur = first;
            let mut guard = 0;
            while cur != s && guard < 1_000_000 {
                ring.push(pos[&cur]);
                let Some(n) = next.get_mut(&cur).and_then(|v| v.pop()) else {
                    break;
                };
                cur = n;
                guard += 1;
            }
            if ring.len() >= 3 {
                loops.push(ring);
            }
        }
    }
    let size = |l: &Vec<P3>| {
        let b = cadrs_cam::Aabb::of(l.iter().copied());
        let s = b.size();
        s[0] * s[0] + s[1] * s[1] + s[2] * s[2]
    };
    loops.sort_by(|a, b| size(b).total_cmp(&size(a)));
    loops
}

/// The same model from the parts' display meshes (see the module docs): moved into `frame`.
pub fn model_from_parts(parts: &[&Part], frame: &Frame) -> Model {
    let mut out = Model::default();
    for p in parts {
        let s = &p.solid;
        let pos: Vec<P3> = s.positions.iter().map(|q| frame.to_local(*q)).collect();
        let tris: Vec<[P3; 3]> = s
            .indices
            .chunks(3)
            .map(|t| [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]])
            .collect();
        let mut m = Model::from_triangles(&p.name, tris);
        for f in &s.faces {
            let ids: Vec<u32> = (f.first_triangle..f.first_triangle + f.triangle_count)
                .map(|i| i as u32)
                .collect();
            let face_tris: Vec<[P3; 3]> = ids.iter().map(|&i| m.triangles[i as usize]).collect();
            let surface = match (f.plane, f.kind, f.axis, f.radius) {
                (Some(_), _, _, _) => Surface::Plane {
                    normal: mean_normal(&face_tris),
                },
                (_, Some(cadrs_kernel::SurfaceKind::Cylinder), Some((pt, dir)), Some(radius)) => {
                    let point = frame.to_local(pt);
                    let axis = frame.dir_to_local(dir);
                    Surface::Cylinder {
                        point,
                        axis,
                        radius,
                        inner: faces_axis(&face_tris, point, axis),
                    }
                }
                _ => Surface::Other,
            };
            m.faces.push(Face {
                name: face_key(p.id, &f.name),
                body: 0,
                surface,
                loops: f
                    .loops
                    .iter()
                    .map(|l| l.iter().map(|q| frame.to_local(*q)).collect())
                    .collect(),
                triangles: ids,
            });
        }
        for e in &s.edges {
            m.edges.push(Edge {
                name: edge_key(p.id, &e.name),
                body: 0,
                points: e.points.iter().map(|q| frame.to_local(*q)).collect(),
                circle: e.circle.map(|c| {
                    (
                        frame.to_local(c.center),
                        frame.dir_to_local(c.normal),
                        c.radius,
                    )
                }),
            });
        }
        if let Some(b) = m.bodies.first_mut() {
            b.name = p.name.clone();
        }
        out.append(m);
    }
    out
}

/// A part's display triangles in `frame` (workholding and stock components).
pub fn part_triangles(p: &Part, frame: &Frame) -> Vec<[P3; 3]> {
    let s = &p.solid;
    s.indices
        .chunks(3)
        .map(|t| {
            [
                frame.to_local(s.positions[t[0] as usize]),
                frame.to_local(s.positions[t[1] as usize]),
                frame.to_local(s.positions[t[2] as usize]),
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loops_of_a_square_with_a_hole() {
        // A 3 × 3 grid of unit squares without the middle one, two triangles each.
        let mut t = Vec::new();
        for j in 0..3 {
            for i in 0..3 {
                if (i, j) == (1, 1) {
                    continue;
                }
                let (x0, y0, x1, y1) = (i as f64, j as f64, i as f64 + 1.0, j as f64 + 1.0);
                t.push([[x0, y0, 0.0], [x1, y0, 0.0], [x1, y1, 0.0]]);
                t.push([[x0, y0, 0.0], [x1, y1, 0.0], [x0, y1, 0.0]]);
            }
        }
        let loops = face_loops(&t);
        assert_eq!(loops.len(), 2);
        // The outline first (12 unit sides), then the hole (4).
        assert_eq!(loops[0].len(), 12);
        assert_eq!(loops[1].len(), 4);
    }

    #[test]
    fn frame_transform_matches_to_local() {
        let f = Frame::from_zx([10.0, 0.0, 5.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
        let (r, t) = to_frame(&f);
        let p = [3.0, 4.0, 7.0];
        let q = [
            dot(r[0], p) + t[0],
            dot(r[1], p) + t[1],
            dot(r[2], p) + t[2],
        ];
        let e = f.to_local(p);
        for i in 0..3 {
            assert!((q[i] - e[i]).abs() < 1e-12);
        }
    }
}

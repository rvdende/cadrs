//! Drawing views of a Part Studio (P3C.2, X6): the projected edges of its parts (hidden-line
//! removal in the kernel, [`cadrs_kernel::Kernel::project`]) with their persistent names, and a
//! shaded rendering from the display meshes and part appearances (D4.9).
//!
//! [`request`] runs on the rebuild worker thread (which owns the kernel session): it rebuilds
//! the features (cached, so usually free), projects the requested parts and returns at once
//! with a [`PendingView`]; the drawing shows a placeholder until it is done.

use std::collections::HashMap;
use std::sync::Arc;

use cadrs_drawing::annotation::{ChamferInfo, HoleInfo, ModelEdge, ThreadInfo, ViewModel};
use cadrs_kernel::naming::EdgeName;
use cadrs_kernel::{ProjectOptions, Projection, ViewFrame};

use crate::appearance::{Appearance, face_appearance};
use crate::document::{Feature, PartProps};
use crate::ids::{FeatureId, PartId};
use crate::parts::Part;

/// What to project.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewRequest {
    /// One part, or every part of the studio.
    pub part: Option<PartId>,
    pub frame: ViewFrame,
    pub options: ProjectOptions,
    /// Also make the shaded triangles.
    pub shaded: bool,
    /// The studio's part settings and feature appearances (for the shaded colours).
    pub props: Vec<PartProps>,
    pub appearances: Vec<(FeatureId, Appearance)>,
    /// Material removed before projecting (section and broken-out views, P3C.8).
    pub cut: Option<cadrs_drawing::view_kinds::ViewCut>,
    /// Assembly views: also draw the curves where parts run into each other (Show part
    /// intersections, X6).
    pub intersections: bool,
    /// A flat pattern view (P3I.7): the part's flat pattern, not its folded solid (see
    /// [`crate::flat_drawing`]).
    pub flat: bool,
}

/// A shaded triangle in the view's 2D frame (model mm) with a colour per corner (linear RGBA)
/// and each corner's depth along the direction of sight (larger is farther), so the drawing can
/// depth-test the faces instead of painting them in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadedTriangle {
    pub points: [[f64; 2]; 3],
    pub colors: [[f32; 4]; 3],
    pub depths: [f64; 3],
}

/// A projected view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewGeometry {
    /// The edges, each with its source edge's or face's persistent name where known.
    pub projection: Projection,
    /// Back to front (paint in order), when asked for.
    pub shaded: Vec<ShadedTriangle>,
    /// The parts shown, in the order of [`cadrs_kernel::ProjSource::body`].
    pub parts: Vec<PartId>,
    /// 2D bounds of the parts' meshes (whether or not edges came back).
    pub bounds: Option<([f64; 2], [f64; 2])>,
    /// The exact geometry of the parts' named edges (P3C.3: what driven dimensions measure).
    pub edges: HashMap<EdgeName, ModelEdge>,
    /// The studio's Hole features' specs, by feature id (hole callouts, PS15.9).
    pub holes: HashMap<uuid::Uuid, HoleInfo>,
    /// The faces a cut left on its plane (P3C.8): loops in the view's 2D frame, outer loops
    /// counter-clockwise, holes clockwise.
    pub hatch: Vec<Vec<[f64; 2]>>,
    /// The tapped holes' threads (Show threads, P3C.8).
    pub threads: Vec<ThreadInfo>,
    /// The studio's Chamfer features' specs, by feature id (chamfer dimensions, P3C.8).
    pub chamfers: HashMap<uuid::Uuid, ChamferInfo>,
    /// A flat pattern view's bends (P3I.7).
    pub flat: Option<cadrs_drawing::flat_view::FlatData>,
}

impl ViewModel for ViewGeometry {
    fn projection(&self) -> &Projection {
        &self.projection
    }
    fn model_edge(&self, name: &EdgeName) -> Option<&ModelEdge> {
        self.edges.get(name)
    }
    fn hole(&self, feature: &uuid::Uuid) -> Option<&HoleInfo> {
        self.holes.get(feature)
    }
    fn hatch(&self) -> &[Vec<[f64; 2]>] {
        &self.hatch
    }
    fn threads(&self) -> &[ThreadInfo] {
        &self.threads
    }
    fn chamfer(&self, feature: &uuid::Uuid) -> Option<&ChamferInfo> {
        self.chamfers.get(feature)
    }
    fn flat(&self) -> Option<&cadrs_drawing::flat_view::FlatData> {
        self.flat.as_ref()
    }
}

/// A solid's edge as drawings measure it: a circle or arc with its exact centre, normal and
/// radius, a straight edge between its end vertices, or a curve.
pub fn model_edge(e: &crate::solid::SolidEdge) -> ModelEdge {
    if let Some(c) = e.circle {
        return ModelEdge::Circle {
            center: c.center,
            normal: c.normal,
            radius: c.radius,
            points: e.points.clone(),
        };
    }
    let (Some(a), Some(b)) = (e.points.first().copied(), e.points.last().copied()) else {
        return ModelEdge::Curve { points: e.points.clone() };
    };
    let d = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    let straight = l > 1e-9
        && e.points.iter().all(|p| {
            let q = [p[0] - a[0], p[1] - a[1], p[2] - a[2]];
            let t = (q[0] * d[0] + q[1] * d[1] + q[2] * d[2]) / (l * l);
            let r = [q[0] - d[0] * t, q[1] - d[1] * t, q[2] - d[2] * t];
            (r[0] * r[0] + r[1] * r[1] + r[2] * r[2]).sqrt() <= 1e-6 * l.max(1.0)
        });
    if straight {
        ModelEdge::Line { a, b }
    } else {
        ModelEdge::Curve { points: e.points.clone() }
    }
}

/// A Hole feature's spec as a hole callout reads it.
pub fn hole_info(spec: &crate::hole::HoleSpec) -> HoleInfo {
    use crate::hole::{HoleEnd as E, HoleStyle, HoleType};
    use cadrs_drawing::annotation::HoleEnd;
    HoleInfo {
        diameter: spec.diameter.value,
        end: match spec.end {
            E::ThroughAll => HoleEnd::Through,
            E::Blind => HoleEnd::Blind,
            E::UpToNext => HoleEnd::UpToNext,
            E::UpToEntity => HoleEnd::UpToEntity,
        },
        depth: spec.depth.value,
        cbore: (spec.style == HoleStyle::Counterbore).then_some((spec.cbore_diameter.value, spec.cbore_depth.value)),
        csink: (spec.style == HoleStyle::Countersink).then_some((spec.csink_diameter.value, spec.csink_angle.value)),
        thread: (spec.hole_type == HoleType::Tapped && !spec.pitch.is_empty())
            .then(|| (crate::hole::thread_label(&spec.pitch), spec.tapped_depth.value)),
    }
}

/// The model data of `parts` and `features` a view's annotations read.
pub fn model_data(parts: &[&Part], features: &[Feature]) -> (HashMap<EdgeName, ModelEdge>, HashMap<uuid::Uuid, HoleInfo>) {
    let edges = parts
        .iter()
        .flat_map(|p| p.solid.edges.iter())
        .map(|e| (e.name, model_edge(e)))
        .collect();
    let holes = features
        .iter()
        .filter_map(|f| f.hole().map(|h| (f.id.0, hole_info(&h.spec))))
        .collect();
    (edges, holes)
}

/// A Chamfer feature's spec as a chamfer dimension reads it.
pub fn chamfer_info(c: &crate::applied::ChamferFeature) -> ChamferInfo {
    use crate::applied::ChamferType;
    match c.kind {
        ChamferType::EqualDistance => ChamferInfo { distance: c.distance, distance2: None, angle: 45.0 },
        ChamferType::TwoDistances => ChamferInfo {
            distance: c.distance,
            distance2: Some(c.distance2),
            angle: (c.distance2 / c.distance.max(1e-12)).atan().to_degrees(),
        },
        ChamferType::DistanceAngle => ChamferInfo { distance: c.distance, distance2: None, angle: c.angle },
    }
}

/// The studio's Chamfer features' specs.
pub fn chamfers(features: &[Feature]) -> HashMap<uuid::Uuid, ChamferInfo> {
    features.iter().filter_map(|f| f.chamfer().map(|c| (f.id.0, chamfer_info(c)))).collect()
}

/// A thread's major diameter (mm) from its designation: "M6x1" → 6, "1/4-20 UNC" → 6.35,
/// "#10-24 UNC" → 4.826.
pub fn thread_major(pitch: &str) -> Option<f64> {
    let p = pitch.trim();
    if let Some(rest) = p.strip_prefix('M') {
        let n: String = rest.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect();
        return n.parse().ok();
    }
    let size = p.split(['-', ' ']).next()?;
    if let Some(n) = size.strip_prefix('#') {
        let n: f64 = n.parse().ok()?;
        return Some((0.060 + 0.013 * n) * 25.4);
    }
    let inches = match size.split_once('/') {
        Some((a, b)) => {
            let (whole, a) = match a.split_once(' ') {
                Some((w, a)) => (w.parse::<f64>().ok()?, a),
                None => (0.0, a),
            };
            whole + a.parse::<f64>().ok()? / b.parse::<f64>().ok()?
        }
        None => size.parse().ok()?,
    };
    Some(inches * 25.4)
}

/// The threads of the tapped Hole features of `features` in `parts`: each hole's cylinder is
/// found by its circular edges (on faces the hole feature made, the tap drill's radius), its
/// entry by the drilling direction (into the back of the sketch plane, or the other way when
/// flipped).
pub fn threads(parts: &[&Part], features: &[Feature]) -> Vec<ThreadInfo> {
    use crate::hole::{HoleEnd, HoleType};
    let mut out = Vec::new();
    for f in features {
        let Some(h) = f.hole() else { continue };
        if h.spec.hole_type != HoleType::Tapped {
            continue;
        }
        let Some(major) = thread_major(&h.spec.pitch) else { continue };
        let minor = h.spec.diameter.value;
        let r = minor / 2.0;
        // The drilling direction from the first point's sketch plane.
        let normal = h
            .points
            .first()
            .and_then(|p| features.iter().find(|s| s.id == p.sketch))
            .and_then(|s| s.sketch())
            .and_then(|s| s.plane)
            .map(|pl| {
                let fr = pl.frame();
                let n = nalgebra::Vector3::new(fr.u[0], fr.u[1], fr.u[2]).cross(&nalgebra::Vector3::new(fr.v[0], fr.v[1], fr.v[2]));
                n.normalize()
            })
            .unwrap_or(nalgebra::Vector3::z());
        let drill = if h.flip { normal } else { -normal };
        // Circles of the hole's radius on its faces: (centre, how many of the edge's faces
        // the hole made).
        let mut circles: Vec<(nalgebra::Point3<f64>, nalgebra::Vector3<f64>, usize)> = Vec::new();
        for p in parts {
            for e in &p.solid.edges {
                let Some(c) = e.circle else { continue };
                if (c.radius - r).abs() > 1e-4 {
                    continue;
                }
                let own = e.name.faces.iter().filter(|n| n.op == f.id.0).count();
                if own == 0 {
                    continue;
                }
                let n = nalgebra::Vector3::new(c.normal[0], c.normal[1], c.normal[2]).normalize();
                circles.push((nalgebra::Point3::new(c.center[0], c.center[1], c.center[2]), n, own));
            }
        }
        // Group the circles by axis line.
        let mut used = vec![false; circles.len()];
        for i in 0..circles.len() {
            if used[i] {
                continue;
            }
            let (ci, ni, _) = circles[i];
            let group: Vec<usize> = (0..circles.len())
                .filter(|&j| {
                    let (cj, nj, _) = circles[j];
                    let d = cj - ci;
                    !used[j] && ni.cross(&nj).norm() < 1e-6 && (d - ni * d.dot(&ni)).norm() < 1e-4
                })
                .collect();
            for &j in &group {
                used[j] = true;
            }
            if group.len() < 2 {
                continue;
            }
            let axis = if ni.dot(&drill) >= 0.0 { ni } else { -ni };
            let along = |k: usize| (circles[k].0 - nalgebra::Point3::origin()).dot(&axis);
            let entry = *group.iter().min_by(|a, b| along(**a).total_cmp(&along(**b))).expect("two");
            let last = *group.iter().max_by(|a, b| along(**a).total_cmp(&along(**b))).expect("two");
            let length = along(last) - along(entry);
            let through = h.spec.end == HoleEnd::ThroughAll && circles[last].2 == 1;
            let tl = if h.spec.tapped_depth.value > 0.0 { h.spec.tapped_depth.value.min(length) } else { length };
            let c = circles[entry].0;
            out.push(ThreadInfo {
                center: [c.x, c.y, c.z],
                axis: [axis.x, axis.y, axis.z],
                major,
                minor,
                length: tl,
                through: through && tl >= length - 1e-6,
            });
        }
    }
    out
}

/// The parts a view of `part` (or all) shows.
pub fn view_parts(parts: &[Part], part: Option<PartId>) -> Vec<&Part> {
    parts
        .iter()
        .filter(|p| part.is_none_or(|id| p.id == id) && p.kind == crate::parts::PartKind::Solid)
        .collect()
}

/// The 2D bounds of `parts`' meshes seen through `frame`.
pub fn mesh_bounds(parts: &[&Part], frame: &ViewFrame) -> Option<([f64; 2], [f64; 2])> {
    let mut out: Option<([f64; 2], [f64; 2])> = None;
    for p in parts {
        for v in &p.solid.positions {
            let q = frame.to_2d(&nalgebra::Point3::new(v[0], v[1], v[2]));
            out = Some(match out {
                None => ([q.x, q.y], [q.x, q.y]),
                Some((lo, hi)) => ([lo[0].min(q.x), lo[1].min(q.y)], [hi[0].max(q.x), hi[1].max(q.y)]),
            });
        }
    }
    out
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// The shaded triangles of `parts` seen through `frame`, back to front (with their corners'
/// depths for a depth test): each face in its
/// appearance's colour, lit from over the viewer's shoulder (a head light a little up and to
/// the left), smooth across the mesh normals.
pub fn shade(
    parts: &[&Part],
    frame: &ViewFrame,
    props: &[PartProps],
    appearances: &[(FeatureId, Appearance)],
) -> Vec<ShadedTriangle> {
    let dir = frame.dir;
    let light = (-dir * 1.0 + frame.up() * 0.35 - frame.x * 0.25).normalize();
    let mut tris: Vec<(f64, ShadedTriangle)> = Vec::new();
    for part in parts {
        let s = &part.solid;
        for face in &s.faces {
            let a = face_appearance(part, &face.name, props, appearances).0;
            let base = [srgb_to_linear(a.rgb[0]), srgb_to_linear(a.rgb[1]), srgb_to_linear(a.rgb[2])];
            let alpha = a.alpha as f32 / 255.0;
            for t in face.first_triangle..face.first_triangle + face.triangle_count {
                let idx = [s.indices[3 * t], s.indices[3 * t + 1], s.indices[3 * t + 2]].map(|i| i as usize);
                if idx.iter().any(|&i| i >= s.positions.len()) {
                    continue;
                }
                let p = idx.map(|i| {
                    let v = s.positions[i];
                    nalgebra::Point3::new(v[0], v[1], v[2])
                });
                let depth = (frame.depth(&p[0]) + frame.depth(&p[1]) + frame.depth(&p[2])) / 3.0;
                let q = p.map(|pt| {
                    let q = frame.to_2d(&pt);
                    [q.x, q.y]
                });
                let colors = idx.map(|i| {
                    let n = s.normals.get(i).copied().unwrap_or([0.0, 0.0, 0.0]);
                    let n = nalgebra::Vector3::new(n[0], n[1], n[2]);
                    // Normals facing away (the back of a thin wall seen edge-on) are turned
                    // round, so no face goes black.
                    let lit = if n.norm() > 0.0 { n.normalize().dot(&light).abs() } else { 0.7 } as f32;
                    let k = 0.38 + 0.62 * lit;
                    [base[0] * k, base[1] * k, base[2] * k, alpha]
                });
                let depths = p.map(|pt| frame.depth(&pt));
                tris.push((depth, ShadedTriangle { points: q, colors, depths }));
            }
        }
    }
    // Farthest first.
    tris.sort_by(|a, b| b.0.total_cmp(&a.0));
    tris.into_iter().map(|(_, t)| t).collect()
}

/// A view being projected on the worker thread.
pub struct PendingView {
    rx: std::sync::Mutex<std::sync::mpsc::Receiver<Result<Arc<ViewGeometry>, String>>>,
    cancelled: Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for PendingView {
    /// Dropping it cancels the projection if it hasn't started yet.
    fn drop(&mut self) {
        self.cancelled.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl PendingView {
    /// The view (or why it failed), once it is done.
    pub fn poll(&self) -> Option<Result<Arc<ViewGeometry>, String>> {
        match self.rx.lock().ok()?.try_recv() {
            Ok(r) => Some(r),
            Err(std::sync::mpsc::TryRecvError::Empty) => None,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Err("The rebuild thread stopped".into())),
        }
    }

    /// Waits for the view.
    pub fn wait(self) -> Result<Arc<ViewGeometry>, String> {
        let rx = self.rx.lock().map_err(|_| "The rebuild thread stopped".to_string())?;
        rx.recv().unwrap_or_else(|_| Err("The rebuild thread stopped".into()))
    }
}

/// Starts projecting `features`' parts on the worker thread.
pub fn request(features: Vec<Feature>, req: ViewRequest) -> PendingView {
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rx = crate::rebuild::project_view(features, req, cancelled.clone());
    PendingView {
        rx: std::sync::Mutex::new(rx),
        cancelled,
    }
}

/// Projects and waits (tests).
pub fn project(features: &[Feature], req: ViewRequest) -> Result<Arc<ViewGeometry>, String> {
    request(features.to_vec(), req).wait()
}

/// Starts projecting an assembly's occurrences for a drawing view on the worker thread (P3C.5,
/// see [`crate::drawing_assembly`]).
pub fn request_assembly(state: crate::drawing_assembly::AssemblyState, req: ViewRequest) -> PendingView {
    let cancelled = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let rx = crate::rebuild::project_assembly_view(state, req, cancelled.clone());
    PendingView {
        rx: std::sync::Mutex::new(rx),
        cancelled,
    }
}

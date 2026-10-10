//! OpenCascade backend, over our fork of `opencascade-rs` (branch `cadrs`).
//!
//! Bodies are stored as OCCT shapes keyed by [`BodyId`]. Every operation builds new shapes and
//! never mutates its inputs.
//!
//! # Exceptions
//! Every OCCT call that can fail goes through the fork's `opencascade::safe` API, which catches
//! OCCT exceptions (`Standard_Failure`) in C++ and returns them as errors, so a failed operation
//! becomes [`KernelError::OperationFailed`] instead of aborting the process.
//!
//! # Ids and history
//! Ids are indices in `TopExp::MapShapes` order (the explorer's order, each sub-shape once):
//! - `FaceId(i)` is the i-th distinct face of the body ([`Kernel::faces`]);
//! - `EdgeId(i)` the i-th distinct edge (the explorer visits shared edges once per face, so
//!   repeats are dropped) ([`Kernel::edges`]);
//! - `VertexId(i)` the i-th distinct vertex ([`Kernel::vertices`]).
//!
//! They are only valid for the body they were read from. Every operation also returns a
//! [`History`] built from OCCT's own (`Generated`, `Modified`, `IsDeleted`, and a sweep's
//! `FirstShape`/`LastShape`, through the fork's `*_h` operations), which
//! [`crate::naming`] turns into names that survive rebuilds.

use std::collections::HashMap;
use std::f64::consts::{PI, TAU};

use glam::{DVec3, dvec3};
use nalgebra::{Point2, Point3, Unit, Vector3};
use opencascade::primitives::{Edge, Face, Shape, Wire};
use opencascade::safe::{
    ChamferKind, CurveType, History as OcctHistory, SubKind, SurfaceType,
};

use crate::{
    Aabb, Axis, BodyId, BoolOp, ChamferMeasure, ChamferOpts, ChamferSpec, Circle3, FilletProfile, FilletSize, FilletSpec, ShellSpec, Curve2, CurveKind, EdgeId, EdgeInfo,
    Extent, ExtrudeSpec, FaceId, FaceInfo, History, InputFace, Kernel,
    KernelError, Loop, MassProperties, Motion, OpResult, Origin, Plane, PointClass, Profile, RayHit, Region, Result,
    SurfaceKind, Tessellation, Transform, TriMesh, VertexId, VertexInfo,
    ProjClass, ProjCurve, ProjEdge, ProjVisibility, ProjectOptions, Projection, ViewFrame,
};

#[path = "occt_extrude.rs"]
mod extrude;
#[path = "occt_revolve.rs"]
mod revolve;
#[path = "occt_sweep.rs"]
mod sweep;
#[path = "occt_draft.rs"]
mod draft;
#[path = "occt_partial.rs"]
mod partial;
#[path = "occt_surfacing.rs"]
mod surfacing;
#[path = "occt_exchange.rs"]
mod exchange;
#[path = "occt_smooth.rs"]
mod smooth;

/// Lengths below this are treated as zero (mm).
const LINEAR_EPS: f64 = 1e-9;

/// The smallest extrude depth the kernel builds (mm): ten times OCCT's `Precision::Confusion`.
/// A thinner solid would have faces closer together than OCCT can tell apart.
pub const MIN_DEPTH: f64 = 1e-6;

fn occt(e: opencascade::Error) -> KernelError {
    KernelError::OperationFailed(e.to_string())
}

/// A kernel session backed by OpenCascade.
#[derive(Default)]
pub struct OcctKernel {
    bodies: HashMap<BodyId, Shape>,
    /// Each body's faces as [`Kernel::faces`] gives them, worked out once: a body never changes
    /// once stored, and a face's area and centroid are exact integrals over its surface (seconds
    /// for a model of B-spline faces), asked for again by every feature that looks at the body.
    face_infos: std::sync::Mutex<HashMap<BodyId, std::sync::Arc<Vec<FaceInfo>>>>,
    next_id: u64,
}

impl OcctKernel {
    pub fn new() -> Self {
        Self::default()
    }

    /// OCCT's thick solid of a body with faces removed (P3.6).
    fn thick_solid(&self, body: BodyId, remove: &[FaceId], offset: f64) -> Result<(Shape, History)> {
        let shape = self.body(body)?;
        let mut faces: Vec<Option<Face>> = faces_of(shape).into_iter().map(Some).collect();
        let removed = remove
            .iter()
            .map(|id| {
                faces
                    .get_mut(id.0 as usize)
                    .and_then(Option::take)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown or repeated {id:?}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let (r, h) = shape.try_hollow_h(offset, &removed).map_err(occt)?;
        Ok((r, single_input_history(body, &h)))
    }

    /// A closed hollow body (P3.6): the offset solid (no face removed) taken from the body
    /// (inward) or the body taken from it (outward). Only the body's faces are named from it.
    fn hollow_by_offset(&self, body: BodyId, offset: f64, before: f64) -> Result<(Shape, History)> {
        let shape = self.body(body)?;
        let fail = || KernelError::OperationFailed("the offset body is not valid".into());
        let (skin, _) = shape.try_hollow_h(offset, std::iter::empty()).map_err(occt)?;
        let skin_volume = skin.mass_properties().volume;
        let fits = if offset > 0.0 { skin_volume > before } else { skin_volume > 0.0 && skin_volume < before };
        if !fits || !skin.is_valid().map_err(occt)? {
            return Err(fail());
        }
        let (r, h) = if offset > 0.0 {
            skin.try_subtract_h(shape).map_err(occt)?
        } else {
            shape.try_subtract_h(&skin).map_err(occt)?
        };
        let n_body = face_count(shape)?;
        let mut out = History::default();
        let body_faces: Vec<&Vec<usize>> = if offset > 0.0 {
            h.faces.iter().skip(face_count(&skin)?).collect()
        } else {
            h.faces.iter().take(n_body).collect()
        };
        for (i, outs) in body_faces.into_iter().enumerate() {
            let input = InputFace { body, face: FaceId(i as u64) };
            for &f in outs {
                out.modified.push((FaceId(f as u64), input));
            }
        }
        Ok((r, out))
    }

    /// True if two kept planar faces facing each other across the body are closer than twice
    /// the wall (their inward walls would cross, PS16.4: Onshape's shell fails there too).
    fn walls_cross(&self, body: BodyId, remove: &[FaceId], t: f64) -> Result<bool> {
        let shape = self.body(body)?;
        let infos = self.faces(body)?;
        let face_shapes = shape.sub_shapes(SubKind::Face).map_err(occt)?;
        let planar: Vec<(&FaceInfo, Plane, (DVec3, DVec3))> = infos
            .iter()
            .filter(|f| !remove.contains(&f.id))
            .filter_map(|f| Some((f, f.plane?, face_shapes.get(f.id.0 as usize)?.bbox().ok()?)))
            .collect();
        let adjacent = |a: FaceId, b: FaceId| -> Result<bool> { Ok(self.adjacent_faces(body, a)?.contains(&b)) };
        for (i, (fa, pa, ba)) in planar.iter().enumerate() {
            let n = pa.normal.into_inner();
            // Only axis-aligned faces are compared (their boxes are their extents).
            let Some(axis) = (0..3).find(|k| n[*k].abs() > 0.999) else { continue };
            for (fb, pb, bb) in &planar[i + 1..] {
                if n.dot(&pb.normal) > -0.999 {
                    continue;
                }
                // B lies behind A (inside the body), within twice the wall.
                let d = (fa.center - fb.center).dot(&n);
                if !(d > 1e-9 && d < 2.0 * t - 1e-9) {
                    continue;
                }
                let overlap = (0..3)
                    .filter(|k| *k != axis)
                    .all(|k| ba.0[k].max(bb.0[k]) < ba.1[k].min(bb.1[k]) - 1e-9);
                if overlap && !adjacent(fa.id, fb.id)? {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// An inward shell built from its definition (P3.6): the walls are the points of the body
    /// within `t` of its faces other than the removed ones. That region is the union of each
    /// kept face swept inward by `t` (a slab), a tube of radius `t` round each concave edge (a
    /// cylinder along a line, a torus round a circle) and a ball at each vertex of a concave
    /// edge (convex edges and vertices need none: the points nearest them lie outside the
    /// body). The cavity is the body less that union, and the shell the body less the cavity.
    /// OCCT's thick solid can't drop an offset face that vanishes (a counterbore's floor
    /// narrower than the wall); this can.
    fn shell_by_regions(&self, body: BodyId, remove: &[FaceId], t: f64) -> Result<(Shape, History)> {
        let shape = self.body(body)?;
        let infos = self.faces(body)?;
        let edges = self.edges(body)?;
        let vertices = self.vertices(body)?;
        let face_shapes = shape.sub_shapes(SubKind::Face).map_err(occt)?;
        let (lo, hi) = shape.bbox().map_err(occt)?;
        let scale = (hi - lo).length().max(1.0);
        let mut tools: Vec<Shape> = Vec::new();
        // The side a face thickens to: inward is against its outward normal.
        let sign = infos
            .iter()
            .find_map(|f| {
                let pl = f.plane?;
                let fs = face_shapes.get(f.id.0 as usize)?;
                let (slab, _) = fs.try_thicken_h(t).ok()?;
                let c = to_na(slab.mass_properties().center_of_mass);
                Some(if (c - f.center.coords).dot(&pl.normal) > 0.0 { -1.0 } else { 1.0 })
            })
            .ok_or_else(|| KernelError::OperationFailed("no planar face to orient the walls by".into()))?;
        for f in &infos {
            if remove.contains(&f.id) {
                continue;
            }
            let fs = face_shapes
                .get(f.id.0 as usize)
                .ok_or_else(|| KernelError::OperationFailed("face list".into()))?;
            let (slab, _) = fs.try_thicken_h(sign * t).map_err(occt)?;
            tools.push(slab);
        }
        // A point inside the body (ray parity along a skewed direction).
        let inside = |p: Point3<f64>| -> Result<bool> {
            let dir = dvec3(0.5773, 0.5774, 0.5775).normalize();
            let hits = shape.ray_hits(to_glam(p.coords), dir).map_err(occt)?;
            Ok(hits.iter().filter(|h| h.t > 1e-9).count() % 2 == 1)
        };
        let delta = 1e-4 * scale;
        let mut concave_vertices: Vec<VertexId> = Vec::new();
        for e in &edges {
            let (Some(a), Some(b)) = (e.faces[0], e.faces[1]) else { continue };
            if a == b || e.curve == CurveKind::Degenerate {
                continue;
            }
            let samples = shape.edge_normals(e.id.0 as usize, 3).map_err(occt)?;
            let mid = samples[1];
            let (n1, n2) = (to_na(mid.normals[0]), to_na(mid.normals[1]));
            if n1.norm() < 0.5 || n2.norm() < 0.5 || (n1 - n2).norm() < 1e-6 {
                continue;
            }
            let q = Point3::from(to_na(mid.point)) + (n1 - n2).normalize() * delta;
            if !inside(q)? {
                continue;
            }
            // A concave edge: a tube round it.
            let tube = match (e.curve, e.circle) {
                (CurveKind::Line, _) => {
                    let d = e.end - e.start;
                    let len = d.norm();
                    let dir = Unit::new_normalize(d);
                    let helper = if dir.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
                    let x_dir = Unit::new_normalize(helper - dir.into_inner() * helper.dot(&dir));
                    let plane = Plane { origin: e.start, x_dir, normal: dir };
                    let disc = profile_faces(&plane, &[disc_region(Point2::origin(), t)])?;
                    disc[0].try_extrude(to_glam(dir.into_inner() * len)).map_err(occt)?
                }
                (_, Some(c)) => {
                    // A disc in the half plane through the axis and the edge's start, revolved
                    // along the edge.
                    let axis = c.normal.into_inner();
                    let radial = e.start - c.center;
                    let radial = radial - axis * radial.dot(&axis);
                    if radial.norm() < LINEAR_EPS {
                        continue;
                    }
                    let x_dir = Unit::new_normalize(radial);
                    let plane = Plane { origin: e.start, x_dir, normal: Unit::new_normalize(axis.cross(&x_dir)) };
                    let disc = profile_faces(&plane, &[disc_region(Point2::origin(), t)])?;
                    let sweep = if e.is_closed() {
                        TAU
                    } else {
                        // The arc's angle, the way the edge runs about the axis.
                        let y = axis.cross(&x_dir);
                        let at = |p: Point3<f64>| {
                            let v = p - c.center;
                            let a = v.dot(&y).atan2(v.dot(&x_dir));
                            if a <= 0.0 { a + TAU } else { a }
                        };
                        let (m, end) = (at(e.mid), at(e.end));
                        if m <= end { end } else { -(TAU - end) }
                    };
                    let (s, _) = disc[0]
                        .try_revolve_h(to_glam(c.center.coords), to_glam(axis), sweep)
                        .map_err(occt)?;
                    s
                }
                _ => {
                    return Err(KernelError::OperationFailed(
                        "a concave edge that is neither a line nor a circle".into(),
                    ));
                }
            };
            tools.push(tube);
            for v in vertices.iter().filter(|v| v.edges.contains(&e.id)) {
                if !concave_vertices.contains(&v.id) {
                    concave_vertices.push(v.id);
                }
            }
        }
        for v in vertices.iter().filter(|v| concave_vertices.contains(&v.id)) {
            // A ball: a half disc turned about its diameter.
            let plane = Plane { origin: v.point, x_dir: Vector3::x_axis(), normal: -Vector3::y_axis() };
            let half = Region {
                outer: Loop {
                    curves: vec![
                        Curve2::Arc { center: Point2::origin(), radius: t, start_angle: -PI / 2.0, sweep: PI, source: None },
                        Curve2::Line { a: Point2::new(0.0, t), b: Point2::new(0.0, -t), source: None },
                    ],
                },
                holes: Vec::new(),
                source: None,
            };
            let face = profile_faces(&plane, &[half])?;
            let (ball, _) = face[0]
                .try_revolve_h(to_glam(v.point.coords), dvec3(0.0, 0.0, 1.0), TAU)
                .map_err(occt)?;
            tools.push(ball);
        }
        // The tools overlap each other, so they are taken away one at a time (a compound of
        // overlapping solids is not a valid boolean argument).
        let mut cavity = clone_shape(shape);
        for tool in &tools {
            cavity = cavity.try_subtract(tool).map_err(occt)?;
        }
        let (result, h) = shape.try_subtract_h(&cavity).map_err(occt)?;
        // The body's faces continue; the cavity's faces are new.
        let n = face_count(shape)?;
        let mut out = History::default();
        for (i, outs) in h.faces.iter().take(n).enumerate() {
            let input = InputFace { body, face: FaceId(i as u64) };
            if outs.is_empty() {
                out.deleted.push(input);
            }
            for &f in outs {
                out.modified.push((FaceId(f as u64), input));
            }
        }
        Ok((result, out))
    }


    /// Cuts (`add` false) or adds each tool to `body` in turn. The body's faces continue; a
    /// tool's faces are new faces from an edge, or continue a face of the body.
    fn apply_tools(&mut self, body: BodyId, tools: &[(Shape, bool, ToolFaces)]) -> Result<OpResult> {
        #[derive(Clone, Copy, PartialEq)]
        enum Src {
            In(FaceId),
            Edge(EdgeId),
        }
        let shape = self.body(body)?;
        let n = face_count(shape)?;
        let mut result = clone_shape(shape);
        let mut sources: Vec<Option<Src>> = (0..n).map(|i| Some(Src::In(FaceId(i as u64)))).collect();
        for (tool, add, faces) in tools {
            let src = match faces {
                ToolFaces::FromEdge(e) => Src::Edge(*e),
                ToolFaces::Continue(f) => Src::In(*f),
            };
            let theirs = vec![Some(src); face_count(tool)?];
            let (next, h) = if *add { result.try_union_clean_h(tool) } else { result.try_subtract_h(tool) }.map_err(occt)?;
            let mut next_sources: Vec<Option<Src>> = vec![None; face_count(&next)?];
            for (outs, src) in h.faces.iter().zip(sources.iter().chain(&theirs)) {
                for &f in outs {
                    if let Some(slot) = next_sources.get_mut(f) {
                        *slot = slot.or(*src);
                    }
                }
            }
            sources = next_sources;
            result = next;
        }
        if !result.is_valid().map_err(occt)? {
            return Err(KernelError::OperationFailed("the result is not a valid solid".into()));
        }
        let mut history = History::default();
        for (i, s) in sources.iter().enumerate() {
            match s {
                Some(Src::In(face)) => history.modified.push((FaceId(i as u64), InputFace { body, face: *face })),
                Some(Src::Edge(edge)) => history.generated.push((FaceId(i as u64), Origin::FromEdge { body, edge: *edge })),
                None => {}
            }
        }
        for i in 0..n {
            let face = FaceId(i as u64);
            if !sources.contains(&Some(Src::In(face))) {
                history.deleted.push(InputFace { body, face });
            }
        }
        self.insert(result, history)
    }

    /// A conic or curvature fillet (P3.6, PS14.4): per straight edge between flat faces, the
    /// section between the corner and the curve, swept along the edge, is cut from a convex
    /// edge or added to a concave one. The contact lines are those of the circular fillet.
    fn section_fillet(&mut self, body: BodyId, edges: &[EdgeId], spec: &FilletSpec) -> Result<OpResult> {
        let not_here = || {
            KernelError::InvalidParameter(
                "Conic, curvature and asymmetric fillets are built on straight edges between flat faces only".into(),
            )
        };
        let shape = self.body(body)?;
        let infos = self.edges(body)?;
        let faces = self.faces(body)?;
        let (lo, hi) = shape.bbox().map_err(occt)?;
        let delta = 1e-4 * (hi - lo).length().max(1.0);
        let inside = |p: Point3<f64>| -> Result<bool> {
            let dir = dvec3(0.5773, 0.5774, 0.5775).normalize();
            let hits = shape.ray_hits(to_glam(p.coords), dir).map_err(occt)?;
            Ok(hits.iter().filter(|h| h.t > 1e-9).count() % 2 == 1)
        };
        let mut tools: Vec<(Shape, bool, ToolFaces)> = Vec::new();
        for &id in edges {
            let e = infos.iter().find(|i| i.id == id).ok_or_else(|| KernelError::OperationFailed(format!("unknown {id:?}")))?;
            let [Some(fa), Some(fb)] = e.faces else { return Err(not_here()) };
            let flat = |f: FaceId| faces.iter().any(|x| x.id == f && x.kind == SurfaceKind::Plane);
            if e.curve != CurveKind::Line || fa == fb || !flat(fa) || !flat(fb) {
                return Err(not_here());
            }
            let mid = shape.edge_normals(id.0 as usize, 3).map_err(occt)?[1];
            let (n1, n2) = (to_na(mid.normals[0]), to_na(mid.normals[1]));
            if (n1 - n2).norm() < 1e-6 || (n1 + n2).norm() < 1e-6 {
                return Err(not_here());
            }
            let m = Point3::from(to_na(mid.point));
            let concave = inside(m + (n1 - n2).normalize() * delta)?;
            // In each face, away from the edge; the section's angle at the corner.
            let s = if concave { 1.0 } else { -1.0 };
            let t1 = (n2 - n1 * n2.dot(&n1)).normalize() * s;
            let t2 = (n1 - n2 * n1.dot(&n2)).normalize() * s;
            let theta = t1.dot(&t2).clamp(-1.0, 1.0).acos();
            let contact = |size: f64| match spec.size {
                FilletSize::Radius(_) => size / (theta / 2.0).tan(),
                FilletSize::Width(_) => size / (2.0 * (theta / 2.0).sin()),
            };
            let first = match spec.size {
                FilletSize::Radius(r) | FilletSize::Width(r) => r,
            };
            let d = contact(first);
            let v = e.start;
            let (mut p1, mut p2) = (v + t1 * d, v + t2 * d);
            if let FilletProfile::Asymmetric { second, flip } = spec.profile {
                if !(second > 0.0 && second.is_finite()) {
                    return Err(KernelError::InvalidParameter("The second radius must be greater than zero".into()));
                }
                let (d1, d2) = if flip { (contact(second), d) } else { (d, contact(second)) };
                p1 = v + t1 * d1;
                p2 = v + t2 * d2;
            }
            let d = d.max(match spec.profile {
                FilletProfile::Asymmetric { second, .. } => contact(second),
                _ => 0.0,
            });
            let (poles, weights): (Vec<Point3<f64>>, Vec<f64>) = match spec.profile {
                FilletProfile::Conic { rho } => {
                    if !(rho > 0.0 && rho < 1.0) {
                        return Err(KernelError::InvalidParameter("Rho must be between 0 and 1".into()));
                    }
                    (vec![p1, v, p2], vec![1.0, rho / (1.0 - rho), 1.0])
                }
                FilletProfile::Curvature { magnitude: k } => {
                    if !(k > 0.0 && k <= 1.0) {
                        return Err(KernelError::InvalidParameter("The magnitude must be between 0 and 1".into()));
                    }
                    let a = |p: Point3<f64>, f: f64| p + (v - p) * f;
                    (vec![p1, a(p1, k / 2.0), a(p1, k), a(p2, k), a(p2, k / 2.0), p2], vec![1.0; 6])
                }
                // The circle's weight at this corner: an affine image of the circular fillet.
                FilletProfile::Asymmetric { .. } => (vec![p1, v, p2], vec![1.0, (theta / 2.0).sin(), 1.0]),
                FilletProfile::Circular => return Err(not_here()),
            };
            // The rest of the section runs out past the faces (into air on a convex edge, into
            // the material on a concave one), so no tool face lies on a face of the body.
            let k = 0.5 * d;
            let out = if concave { -1.0 } else { 1.0 };
            let corners = [p2, p2 + n2 * (k * out), v + (n1 + n2) * (k * out), p1 + n1 * (k * out), p1];
            let g = |p: &Point3<f64>| to_glam(p.coords);
            let mut list = vec![Edge::try_bezier(&poles.iter().map(g).collect::<Vec<_>>(), &weights).map_err(occt)?];
            for w in corners.windows(2) {
                list.push(Edge::try_segment(g(&w[0]), g(&w[1])).map_err(occt)?);
            }
            let wire = Wire::try_from_edges(&list).map_err(occt)?;
            let face = Face::try_from_wires(&wire, &[]).map_err(occt)?;
            let prism = face.try_extrude(to_glam(e.end - e.start)).map_err(occt)?;
            tools.push((prism, concave, ToolFaces::FromEdge(id)));
        }
        self.apply_tools(body, &tools)
    }

    /// How many bodies the session holds.
    pub fn body_count(&self) -> usize {
        self.bodies.len()
    }

    fn body(&self, id: BodyId) -> Result<&Shape> {
        self.bodies.get(&id).ok_or(KernelError::UnknownBody(id))
    }

    /// Stores the result of a modelling operation as a new body, with neighbouring faces on one
    /// surface merged first ([`merge_same_domain`]), as Parasolid (and so Onshape) leaves them.
    fn insert(&mut self, shape: Shape, history: History) -> Result<OpResult> {
        let (shape, history) = merge_same_domain(shape, history)?;
        self.insert_raw(shape, history)
    }

    /// Stores `shape` as a new body as it is (a copy, a transform, a split, a file's body),
    /// rejecting empty results.
    fn insert_raw(&mut self, shape: Shape, history: History) -> Result<OpResult> {
        if shape.faces().next().is_none() {
            return Err(KernelError::OperationFailed("the result is empty".into()));
        }
        let id = BodyId(self.next_id);
        self.next_id += 1;
        self.bodies.insert(id, shape);
        Ok(OpResult {
            bodies: vec![id],
            history,
        })
    }
}

/// Merges neighbouring faces of a solid that lie on one surface (planes, cylinders, ...) into one
/// face, and the edges between them go (`ShapeUpgrade_UnifySameDomain`), as Parasolid does after
/// every operation: two adjacent regions of one sketch extruded together have one cap, and a
/// boss added flush with a face continues that face. Collinear edges are merged too.
///
/// `history` (the operation's, by face of `shape`) is carried through the merge: a merged face
/// lists every origin and input face of the faces it was made of, so it is named after the
/// smallest of their names ([`crate::naming::name_faces`]) and the other names become its aliases
/// ([`crate::naming::BodyNames::aliases`]).
///
/// Surface bodies are left as they are. The merge is dropped (the shape kept as it was, seams and
/// all) when it merges nothing, or when it would change the volume, make an invalid solid out of
/// a valid one, leave a face with no edges, or lose track of a face.
fn merge_same_domain(shape: Shape, history: History) -> Result<(Shape, History)> {
    if shape.sub_count(SubKind::Solid).map_err(occt)? == 0 {
        return Ok((shape, history));
    }
    let before = shape.topology_counts().map_err(occt)?;
    // UnifySameDomain edits the edges of its input in place, and a result shares its untouched
    // faces and edges with the operation's inputs (other bodies): it works on a copy.
    let Ok((merged, h)) = clone_shape(&shape).try_clean_h() else {
        return Ok((shape, history));
    };
    let Ok(after) = merged.topology_counts() else {
        return Ok((shape, history));
    };
    if after[0] == before[0] && after[1] == before[1] {
        return Ok((shape, history));
    }
    // Every face of the result comes from a face of the input, and every input face goes to
    // exactly one face (a merge never splits or deletes a face).
    let mut sourced = vec![false; after[0]];
    let one_to_one = h.faces.len() == before[0]
        && h.faces.iter().all(|outs| {
            outs.len() == 1
                && outs.iter().all(|&f| match sourced.get_mut(f) {
                    Some(s) => {
                        *s = true;
                        true
                    }
                    None => false,
                })
        });
    if !one_to_one || sourced.contains(&false) {
        return Ok((shape, history));
    }
    let volume = shape.mass_properties().volume;
    let merged_volume = merged.mass_properties().volume;
    if !merged_volume.is_finite() || (merged_volume - volume).abs() > 1e-7 * volume.abs().max(1.0) {
        return Ok((shape, history));
    }
    if merged.sub_count(SubKind::Solid).map_err(occt)? != shape.sub_count(SubKind::Solid).map_err(occt)? {
        return Ok((shape, history));
    }
    if !merged.is_valid().unwrap_or(false) && shape.is_valid().unwrap_or(false) {
        return Ok((shape, history));
    }
    // A face that closes on itself (a torus swept as two halves) merges into one face with no
    // edges: valid, but OCCT can't mesh a face without a wire.
    if merged.faces().any(|f| f.edges().next().is_none()) {
        return Ok((shape, history));
    }
    let to =|f: FaceId| FaceId(h.faces[f.0 as usize][0] as u64);
    let in_range = |f: &FaceId| (f.0 as usize) < before[0];
    let mut seen_generated = std::collections::HashSet::new();
    let mut seen_modified = std::collections::HashSet::new();
    let out = History {
        generated: history
            .generated
            .into_iter()
            .filter(|(f, _)| in_range(f))
            .map(|(f, o)| (to(f), o))
            .filter(|e| seen_generated.insert(*e))
            .collect(),
        modified: history
            .modified
            .into_iter()
            .filter(|(f, _)| in_range(f))
            .map(|(f, i)| (to(f), i))
            .filter(|e| seen_modified.insert(*e))
            .collect(),
        deleted: history.deleted,
    };
    Ok((merged, out))
}

/// The number of distinct faces of a shape.
fn face_count(shape: &Shape) -> Result<usize> {
    Ok(shape.topology_counts().map_err(occt)?[0])
}

/// Where the faces of a sweep of one region came from, by face of the swept solid: each input
/// edge's generated faces are the side of the profile curve the edge was built from; the first
/// and last shapes are the caps.
fn sweep_origins(
    face: &Face,
    plane: &Plane,
    region_index: usize,
    region: &Region,
    solid: &Shape,
    h: &OcctHistory,
) -> Result<Vec<Option<Origin>>> {
    let mut origins = vec![None; face_count(solid)?];
    let region_id = region.source.unwrap_or(region_index as u64);
    let loops: Vec<&Loop> = std::iter::once(&region.outer).chain(&region.holes).collect();
    // Every profile curve, sampled in model space, with its id.
    let curves: Vec<(u64, Vec<Point3<f64>>)> = loops
        .iter()
        .enumerate()
        .flat_map(|(li, lp)| {
            lp.curves.iter().enumerate().map(move |(ci, c)| {
                let id = c.source().unwrap_or_else(|| crate::unsourced_curve(li, ci));
                let pts = (0..=32)
                    .map(|k| plane.to_model(c.point_at(k as f64 / 32.0)))
                    .collect();
                (id, pts)
            })
        })
        .collect();
    let edges = face.edges_geometry().map_err(occt)?;
    for (i, generated) in h.edges.iter().enumerate() {
        let Some(e) = edges.get(i) else { continue };
        let mid = Point3::from(to_na(e.mid));
        // The curve the edge lies on: the one passing nearest its middle.
        let nearest = curves
            .iter()
            .map(|(id, pts)| {
                let d = pts
                    .windows(2)
                    .map(|w| segment_distance(mid, w[0], w[1]))
                    .fold(f64::INFINITY, f64::min);
                (*id, d)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let Some((curve, _)) = nearest else { continue };
        for &f in generated {
            if let Some(o) = origins.get_mut(f) {
                o.get_or_insert(Origin::ProfileCurve {
                    region: region_id,
                    curve,
                });
            }
        }
    }
    for (list, origin) in [
        (&h.first, Origin::StartCap { region: region_id }),
        (&h.last, Origin::EndCap { region: region_id }),
    ] {
        for &f in list {
            if let Some(o) = origins.get_mut(f) {
                o.get_or_insert(origin);
            }
        }
    }
    Ok(origins)
}

fn segment_distance(p: Point3<f64>, a: Point3<f64>, b: Point3<f64>) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(&ab) / ab.norm_squared().max(1e-300)).clamp(0.0, 1.0);
    (a + ab * t - p).norm()
}

/// Fuses solids that each carry a per-face tag, carrying the tags through the fusions' histories
/// (a face keeps its tag when trimmed or split; the first tag wins where two faces merge).
///
/// The two halves are fused first, each the same way: fusing one solid at a time into the
/// growing result made a profile of many regions quadratic (825 holes of a perfboard: 28 s).
fn union_tagged<T: Copy>(mut solids: Vec<(Shape, Vec<Option<T>>)>) -> Result<(Shape, Vec<Option<T>>)> {
    match solids.len() {
        0 => Err(KernelError::InvalidProfile("the profile has no regions".into())),
        1 => Ok(solids.pop().expect("one solid")),
        n => {
            let right = solids.split_off(n / 2);
            let (a, a_tags) = union_tagged(solids)?;
            let (b, b_tags) = union_tagged(right)?;
            let (fused, h) = a.try_union_h(&b).map_err(occt)?;
            let tags = carry(&h, &[&a_tags, &b_tags], face_count(&fused)?);
            Ok((fused, tags))
        }
    }
}

/// The tags of a result's faces from its inputs' tags (`inputs` in the operation's order).
fn carry<T: Copy>(h: &OcctHistory, inputs: &[&Vec<Option<T>>], faces: usize) -> Vec<Option<T>> {
    let mut out: Vec<Option<T>> = vec![None; faces];
    let all = inputs.iter().flat_map(|t| t.iter());
    for (outs, tag) in h.faces.iter().zip(all) {
        let Some(tag) = tag else { continue };
        for &f in outs {
            if let Some(slot) = out.get_mut(f) {
                slot.get_or_insert(*tag);
            }
        }
    }
    out
}

/// A history in which every tagged face is generated.
fn generated(origins: Vec<Option<Origin>>) -> History {
    History {
        generated: origins
            .into_iter()
            .enumerate()
            .filter_map(|(i, o)| Some((FaceId(i as u64), o?)))
            .collect(),
        ..History::default()
    }
}

/// The history of an operation on one input body `body` (fillet, chamfer, shell): faces kept
/// or trimmed are modified, faces made from its edges and vertices are generated.
fn single_input_history(body: BodyId, h: &OcctHistory) -> History {
    let mut out = History::default();
    for (i, outs) in h.faces.iter().enumerate() {
        let input = InputFace {
            body,
            face: FaceId(i as u64),
        };
        if outs.is_empty() {
            out.deleted.push(input);
        }
        for &f in outs {
            out.modified.push((FaceId(f as u64), input));
        }
    }
    let mut seen: Vec<usize> = out.modified.iter().map(|(f, _)| f.0 as usize).collect();
    for (i, outs) in h.edges.iter().enumerate() {
        for &f in outs {
            if !seen.contains(&f) {
                seen.push(f);
                out.generated.push((
                    FaceId(f as u64),
                    Origin::FromEdge {
                        body,
                        edge: EdgeId(i as u64),
                    },
                ));
            }
        }
    }
    for (i, outs) in h.vertices.iter().enumerate() {
        for &f in outs {
            if !seen.contains(&f) {
                seen.push(f);
                out.generated.push((
                    FaceId(f as u64),
                    Origin::FromVertex {
                        body,
                        vertex: VertexId(i as u64),
                    },
                ));
            }
        }
    }
    out
}

/// What a tool's faces become in [`OcctKernel::apply_tools`]' history.
#[derive(Debug, Clone, Copy)]
enum ToolFaces {
    /// New faces made from this edge of the body (a fillet section's).
    FromEdge(EdgeId),
    /// They continue this face of the body (a full round replaces its middle face).
    Continue(FaceId),
}

/// A disc of radius `r` about `c`, as a profile region.
fn disc_region(c: Point2<f64>, r: f64) -> Region {
    Region {
        outer: Loop { curves: vec![Curve2::Circle { center: c, radius: r, source: None }] },
        holes: Vec::new(),
        source: None,
    }
}

/// Samples along an edge for a width fillet's radii.
const WIDTH_SAMPLES: usize = 9;

/// The radii that give a fillet of chord width `w` along edge `edge`: where the faces' normals
/// are `φ` apart, the fillet's arc spans `φ` and its chord is `2 r sin(φ/2)`, so
/// `r = w / (2 sin(φ/2))`. One pair when the angle doesn't change along the edge.
fn width_radii(shape: &Shape, edge: EdgeId, w: f64) -> Result<Vec<(f64, f64)>> {
    let samples = shape.edge_normals(edge.0 as usize, WIDTH_SAMPLES).map_err(occt)?;
    let mut out = Vec::with_capacity(samples.len());
    for s in &samples {
        let (a, b) = (to_na(s.normals[0]), to_na(s.normals[1]));
        if a.norm() < 0.5 || b.norm() < 0.5 {
            return Err(KernelError::OperationFailed("an edge to fillet has only one face".into()));
        }
        let phi = a.normalize().dot(&b.normalize()).clamp(-1.0, 1.0).acos();
        if phi < 1e-3 {
            return Err(KernelError::InvalidParameter(
                "A width fillet needs faces that meet at an angle".into(),
            ));
        }
        out.push((s.t, w / (2.0 * (phi / 2.0).sin())));
    }
    let (lo, hi) = out.iter().fold((f64::INFINITY, 0.0f64), |(lo, hi), (_, r)| (lo.min(*r), hi.max(*r)));
    if hi - lo <= 1e-9 * hi {
        return Ok(vec![(0.0, out[0].1)]);
    }
    Ok(out)
}

/// True if a fillet face of `result` touches a face other than the faces that meet at its
/// edge's ends in the input (Onshape's edge overflow).
fn overflows(result: &Shape, h: &OcctHistory, edges: &[EdgeId], infos: &[EdgeInfo], vertices: &[VertexInfo]) -> Result<bool> {
    // The input face each result face continues.
    let mut from: HashMap<usize, usize> = HashMap::new();
    for (i, outs) in h.faces.iter().enumerate() {
        for &f in outs {
            from.entry(f).or_insert(i);
        }
    }
    let around = result.edge_faces().map_err(occt)?;
    let faces_of_edge = |e: EdgeId| -> Vec<usize> {
        infos
            .iter()
            .find(|i| i.id == e)
            .map(|i| i.faces.iter().flatten().map(|f| f.0 as usize).collect())
            .unwrap_or_default()
    };
    for &e in edges {
        let Some(made) = h.edges.get(e.0 as usize) else { continue };
        // The faces meeting at the edge and at its end vertices.
        let mut allowed: Vec<usize> = faces_of_edge(e);
        for v in vertices.iter().filter(|v| v.edges.contains(&e)) {
            for &o in &v.edges {
                allowed.extend(faces_of_edge(o));
            }
        }
        for &fillet in made {
            for list in around.iter().filter(|l| l.contains(&fillet)) {
                for &g in list {
                    if let Some(input) = from.get(&g)
                        && !allowed.contains(input)
                    {
                        return Ok(true);
                    }
                }
            }
        }
    }
    Ok(false)
}

/// The distinct faces of a shape in explorer order (`TopExp::MapShapes` order).
fn faces_of(shape: &Shape) -> Vec<Face> {
    let mut list: Vec<Face> = Vec::new();
    let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
    for face in shape.faces() {
        let h = face.identity_hash();
        let seen = by_hash
            .get(&h)
            .is_some_and(|v| v.iter().any(|&i| list[i].is_same(&face)));
        if !seen {
            by_hash.entry(h).or_default().push(list.len());
            list.push(face);
        }
    }
    list
}

/// The distinct edges of a shape in explorer order, and a lookup from an edge to its index.
struct Edges {
    list: Vec<Edge>,
    by_hash: HashMap<u64, Vec<usize>>,
}

impl Edges {
    fn of(shape: &Shape) -> Self {
        let mut out = Edges {
            list: Vec::new(),
            by_hash: HashMap::new(),
        };
        for edge in shape.edges() {
            if out.index(&edge).is_none() {
                out.by_hash
                    .entry(edge.identity_hash())
                    .or_default()
                    .push(out.list.len());
                out.list.push(edge);
            }
        }
        out
    }

    fn index(&self, edge: &Edge) -> Option<usize> {
        self.by_hash
            .get(&edge.identity_hash())?
            .iter()
            .copied()
            .find(|&i| self.list[i].is_same(edge))
    }
}

impl Kernel for OcctKernel {
    fn name(&self) -> &'static str {
        "occt"
    }

    fn extrude(&mut self, profile: &Profile, extent: Extent) -> Result<OpResult> {
        let (start, depth) = match extent {
            Extent::Blind(d) => (0.0, d),
            Extent::Symmetric(d) => (-d.abs() / 2.0, d.abs()),
            Extent::TwoSided { forward, backward } => (-backward, forward + backward),
        };
        if !depth.is_finite() || depth.abs() < LINEAR_EPS {
            return Err(KernelError::InvalidProfile("extrude depth is zero".into()));
        }
        if depth.abs() < MIN_DEPTH {
            return Err(KernelError::InvalidParameter(format!(
                "the depth ({} mm) is below the kernel's modelling tolerance ({MIN_DEPTH} mm)",
                depth.abs()
            )));
        }
        let normal = to_glam(profile.plane.normal.into_inner());
        let shifted = Plane {
            origin: profile.plane.origin + profile.plane.normal.into_inner() * start,
            ..profile.plane
        };
        let solids = profile_faces(&shifted, &profile.regions)?
            .iter()
            .zip(&profile.regions)
            .enumerate()
            .map(|(i, (face, region))| {
                let (solid, h) = face.try_extrude_h(normal * depth).map_err(occt)?;
                let origins = sweep_origins(face, &shifted, i, region, &solid, &h)?;
                Ok((solid, origins))
            })
            .collect::<Result<Vec<_>>>()?;
        let (shape, origins) = union_tagged(solids)?;
        self.insert(shape, generated(origins))
    }

    fn extrude_with(&mut self, profile: &Profile, spec: &ExtrudeSpec) -> Result<OpResult> {
        self.extrude_full(profile, spec)
    }

    fn compound(&mut self, bodies: &[BodyId]) -> Result<OpResult> {
        use opencascade::primitives::Compound;
        if bodies.is_empty() {
            return Err(KernelError::OperationFailed("nothing to gather".into()));
        }
        let copies: Vec<Shape> = bodies.iter().map(|&b| self.body(b).map(clone_shape)).collect::<Result<_>>()?;
        let compound: Shape = Compound::from_shapes(copies.iter()).into();
        // Each face of the compound is a face of one of the copies (shared, not copied again);
        // a copy's faces are in its original's order.
        let mut by_hash: HashMap<u64, Vec<(usize, usize, Face)>> = HashMap::new();
        for (k, c) in copies.iter().enumerate() {
            for (i, f) in faces_of(c).into_iter().enumerate() {
                by_hash.entry(f.identity_hash()).or_default().push((k, i, f));
            }
        }
        let modified = faces_of(&compound)
            .iter()
            .enumerate()
            .filter_map(|(j, f)| {
                let (k, i, _) = by_hash.get(&f.identity_hash())?.iter().find(|(_, _, g)| g.is_same(f))?;
                Some((FaceId(j as u64), InputFace { body: bodies[*k], face: FaceId(*i as u64) }))
            })
            .collect();
        self.insert(compound, History { modified, ..History::default() })
    }

    fn split_solids(&mut self, body: BodyId) -> Result<Vec<OpResult>> {
        let shape = self.body(body)?;
        let solids = shape.sub_shapes(SubKind::Solid).map_err(occt)?;
        if solids.len() <= 1 {
            // One piece (or a surface body): a copy with the same topology.
            let copy = clone_shape(shape);
            let history = History {
                modified: (0..face_count(&copy)?)
                    .map(|i| (FaceId(i as u64), InputFace { body, face: FaceId(i as u64) }))
                    .collect(),
                ..History::default()
            };
            return Ok(vec![self.insert_raw(copy, history)?]);
        }
        let parent = faces_of(shape);
        let mut by_hash: HashMap<u64, Vec<usize>> = HashMap::new();
        for (i, f) in parent.iter().enumerate() {
            by_hash.entry(f.identity_hash()).or_default().push(i);
        }
        let mut pieces = Vec::new();
        for solid in solids {
            let modified = faces_of(&solid)
                .iter()
                .enumerate()
                .filter_map(|(i, f)| {
                    let j = by_hash
                        .get(&f.identity_hash())?
                        .iter()
                        .copied()
                        .find(|&j| parent[j].is_same(f))?;
                    Some((FaceId(i as u64), InputFace { body, face: FaceId(j as u64) }))
                })
                .collect();
            pieces.push((solid, History { modified, ..History::default() }));
        }
        pieces
            .into_iter()
            .map(|(s, h)| self.insert_raw(s, h))
            .collect()
    }

    fn split_imported(&mut self, body: BodyId) -> Result<Vec<OpResult>> {
        let shape = self.body(body)?;
        let solids = shape.sub_shapes(SubKind::Solid).map_err(occt)?;
        let loose = loose_shells(shape, &solids)?;
        if loose.is_empty() {
            return self.split_solids(body);
        }
        let mut pieces: Vec<Shape> = solids.iter().map(clone_shape).collect();
        pieces.extend(loose);
        pieces.into_iter().map(|s| self.insert_raw(s, History::default())).collect()
    }

    fn revolve(&mut self, profile: &Profile, axis: Axis, angle: f64) -> Result<OpResult> {
        if !angle.is_finite() || angle.abs() < 1e-12 {
            return Err(KernelError::InvalidProfile("revolve angle is zero".into()));
        }
        let angle = angle.clamp(-TAU, TAU);
        let origin = to_glam(axis.origin.coords);
        let dir = to_glam(axis.dir.into_inner());
        let solids = profile_faces(&profile.plane, &profile.regions)?
            .iter()
            .zip(&profile.regions)
            .enumerate()
            .map(|(i, (face, region))| {
                let (solid, h) = face.try_revolve_h(origin, dir, angle).map_err(occt)?;
                let origins = sweep_origins(face, &profile.plane, i, region, &solid, &h)?;
                Ok((solid, origins))
            })
            .collect::<Result<Vec<_>>>()?;
        let (shape, origins) = union_tagged(solids)?;
        self.insert(shape, generated(origins))
    }

    fn revolve_with(&mut self, profile: &Profile, spec: &crate::RevolveSpec) -> Result<OpResult> {
        self.revolve_full(profile, spec)
    }

    fn sweep_with(&mut self, profile: &Profile, spec: &crate::SweepSpec) -> Result<OpResult> {
        self.sweep_full(profile, spec)
    }

    fn loft_with(&mut self, spec: &crate::LoftSpec) -> Result<OpResult> {
        self.loft_full(spec)
    }

    fn split(&mut self, body: BodyId, tool: &crate::SplitTool, source: u64) -> Result<OpResult> {
        self.split_full(body, tool, source)
    }

    fn split_faces(&mut self, body: BodyId, faces: &[FaceId], tool: &crate::SplitTool) -> Result<OpResult> {
        self.split_faces_of(body, faces, tool)
    }

    fn boolean(&mut self, op: BoolOp, target: BodyId, tools: &[BodyId]) -> Result<OpResult> {
        let first = self.body(target)?;
        let mut result = clone_shape(first);
        // The input face each face of the result continues.
        let mut sources: Vec<Option<InputFace>> = (0..face_count(first)?)
            .map(|i| {
                Some(InputFace {
                    body: target,
                    face: FaceId(i as u64),
                })
            })
            .collect();
        let mut inputs: Vec<InputFace> = sources.iter().flatten().copied().collect();
        for &tool in tools {
            let shape = self.body(tool)?;
            let theirs: Vec<Option<InputFace>> = (0..face_count(shape)?)
                .map(|i| {
                    Some(InputFace {
                        body: tool,
                        face: FaceId(i as u64),
                    })
                })
                .collect();
            inputs.extend(theirs.iter().flatten());
            let (next, h) = match op {
                // A face of the target and a face of the tool on one surface, meeting, become
                // one face (no seam where the bodies met); `insert` then merges the rest.
                BoolOp::Union => result.try_union_clean_h(shape),
                BoolOp::Subtract => result.try_subtract_h(shape),
                BoolOp::Intersect => result.try_intersect_h(shape),
            }
            .map_err(occt)?;
            // Every piece of a split face continues it (unlike `carry`, all of them).
            let mut next_sources: Vec<Option<InputFace>> = vec![None; face_count(&next)?];
            for (outs, src) in h.faces.iter().zip(sources.iter().chain(&theirs)) {
                for &f in outs {
                    if let Some(slot) = next_sources.get_mut(f) {
                        *slot = slot.or(*src);
                    }
                }
            }
            sources = next_sources;
            result = next;
        }
        let history = History {
            modified: sources
                .iter()
                .enumerate()
                .filter_map(|(i, s)| Some((FaceId(i as u64), (*s)?)))
                .collect(),
            deleted: inputs
                .into_iter()
                .filter(|i| !sources.contains(&Some(*i)))
                .collect(),
            ..History::default()
        };
        self.insert(result, history)
    }

    fn transform(&mut self, body: BodyId, transform: &Transform) -> Result<OpResult> {
        let shape = self.body(body)?;
        let rotated = match transform.rotation.axis_angle() {
            Some((axis, angle)) => shape.rotated(to_glam(axis.into_inner()), angle),
            None => clone_shape(shape),
        };
        let moved = rotated.translated(to_glam(transform.translation.vector));
        // A copy with the same topology: every face continues its original.
        let history = History {
            modified: (0..face_count(&moved)?)
                .map(|i| {
                    let face = FaceId(i as u64);
                    (face, InputFace { body, face })
                })
                .collect(),
            ..History::default()
        };
        self.insert_raw(moved, history)
    }

    fn transform_motion(&mut self, body: BodyId, motion: &Motion) -> Result<OpResult> {
        let d = motion.linear.determinant();
        let orthonormal = (motion.linear * motion.linear.transpose() - nalgebra::Matrix3::identity()).norm() < 1e-9;
        if !orthonormal || (d.abs() - 1.0).abs() > 1e-9 {
            return Err(KernelError::InvalidParameter("a pattern or mirror transform must keep sizes".into()));
        }
        let shape = self.body(body)?;
        let (moved, h) = shape.try_transform_h(&motion.rows()).map_err(occt)?;
        self.insert_raw(moved, single_input_history(body, &h))
    }

    fn scale(&mut self, body: BodyId, center: Point3<f64>, factor: f64) -> Result<OpResult> {
        if !(factor.is_finite() && factor > 0.0) {
            return Err(KernelError::InvalidParameter("the scale must be greater than zero".into()));
        }
        let shape = self.body(body)?;
        // BRepBuilderAPI_Transform rebuilds the geometry (a scale can't be a location), keeping
        // the topology: face i of the copy is face i of the body.
        let scaled = shape.scaled(to_glam(center.coords), factor);
        let history = History {
            modified: (0..face_count(&scaled)?)
                .map(|i| {
                    let face = FaceId(i as u64);
                    (face, InputFace { body, face })
                })
                .collect(),
            ..History::default()
        };
        self.insert_raw(scaled, history)
    }

    fn face_tool(&mut self, body: BodyId, faces: &[FaceId], source: u64) -> Result<OpResult> {
        let shape = self.body(body)?;
        let idx: Vec<usize> = faces.iter().map(|f| f.0 as usize).collect();
        let (tool, h) = shape.try_face_tool_h(&idx).map_err(occt)?;
        let mut history = single_input_history(body, &h);
        for &f in &h.first {
            let face = FaceId(f as u64);
            if !history.modified.iter().any(|(x, _)| *x == face) {
                history.generated.push((face, Origin::StartCap { region: source }));
            }
        }
        // Faces that lie on their own opening (one flat face) enclose nothing.
        let volume = tool.mass_properties().volume;
        if volume.abs() < 1e-9 {
            return Err(KernelError::InvalidParameter("The faces don't enclose a pocket or a boss".into()));
        }
        self.insert_raw(tool, history)
    }

    fn classify(&self, body: BodyId, point: Point3<f64>, tol: f64) -> Result<PointClass> {
        use opencascade::safe::PointState;
        let shape = self.body(body)?;
        match shape.classify(to_glam(point.coords), tol).map_err(occt)? {
            PointState::Inside => Ok(PointClass::Inside),
            PointState::Outside => Ok(PointClass::Outside),
            PointState::On => Ok(PointClass::OnBoundary),
            PointState::Unknown => Err(KernelError::OperationFailed("the point could not be classified".into())),
        }
    }

    fn fillet(&mut self, body: BodyId, edges: &[EdgeId], radius: f64) -> Result<OpResult> {
        if radius.is_nan() || radius <= 0.0 {
            return Err(KernelError::OperationFailed(
                "fillet radius must be positive".into(),
            ));
        }
        let shape = self.body(body)?;
        let selected = select_edges(shape, edges)?;
        let (result, h) = shape.try_fillet_edges_h(radius, &selected).map_err(occt)?;
        self.insert(result, single_input_history(body, &h))
    }

    fn chamfer(&mut self, body: BodyId, edges: &[EdgeId], spec: ChamferSpec) -> Result<OpResult> {
        let kind = match spec {
            ChamferSpec::EqualDistance(d) => ChamferKind::Equal(d),
            ChamferSpec::TwoDistances(a, b) => ChamferKind::TwoDistances(a, b),
            ChamferSpec::DistanceAngle { distance, angle } => {
                if !(angle > 0.0 && angle < PI / 2.0) {
                    return Err(KernelError::OperationFailed(
                        "chamfer angle must be between 0 and 90°".into(),
                    ));
                }
                ChamferKind::DistanceAngle(distance, angle)
            }
        };
        let (d1, d2) = match kind {
            ChamferKind::Equal(d) => (d, d),
            ChamferKind::TwoDistances(a, b) => (a, b),
            ChamferKind::DistanceAngle(d, _) => (d, d),
        };
        if !(d1 > 0.0 && d2 > 0.0) {
            return Err(KernelError::OperationFailed(
                "chamfer distance must be positive".into(),
            ));
        }
        let shape = self.body(body)?;
        let selected = select_edges(shape, edges)?;
        // The first distance is measured on the edge's first face (in explorer order).
        let faces: Vec<Face> = faces_of(shape);
        let face_of = |edge: &Edge| -> Result<&Face> {
            faces
                .iter()
                .find(|f| f.edges().any(|e| e.is_same(edge)))
                .ok_or_else(|| KernelError::OperationFailed("an edge has no face".into()))
        };
        let pairs = selected
            .iter()
            .map(|e| Ok((e, face_of(e)?)))
            .collect::<Result<Vec<_>>>()?;
        let (result, h) = shape.try_chamfer_edges_h(kind, pairs).map_err(occt)?;
        self.insert(result, single_input_history(body, &h))
    }

    fn shell(&mut self, body: BodyId, remove: &[FaceId], thickness: f64) -> Result<OpResult> {
        if thickness.is_nan() || thickness <= 0.0 {
            return Err(KernelError::OperationFailed(
                "shell thickness must be positive".into(),
            ));
        }
        let shape = self.body(body)?;
        let mut faces: Vec<Option<Face>> = faces_of(shape).into_iter().map(Some).collect();
        let removed = remove
            .iter()
            .map(|id| {
                faces
                    .get_mut(id.0 as usize)
                    .and_then(Option::take)
                    .ok_or_else(|| {
                        KernelError::OperationFailed(format!("unknown or repeated {id:?}"))
                    })
            })
            .collect::<Result<Vec<_>>>()?;
        // Onshape shells inward; OCCT thickens outward for positive offsets.
        let (result, h) = shape.try_hollow_h(-thickness, &removed).map_err(occt)?;
        self.insert(result, single_input_history(body, &h))
    }

    fn delete_faces(&mut self, body: BodyId, remove: &[FaceId]) -> Result<OpResult> {
        if remove.is_empty() {
            return Err(KernelError::InvalidParameter("Select the faces to delete".into()));
        }
        let shape = self.body(body)?;
        let mut faces: Vec<Option<Face>> = faces_of(shape).into_iter().map(Some).collect();
        let removed = remove
            .iter()
            .map(|id| {
                faces
                    .get_mut(id.0 as usize)
                    .and_then(Option::take)
                    .ok_or_else(|| KernelError::OperationFailed(format!("unknown or repeated {id:?}")))
            })
            .collect::<Result<Vec<_>>>()?;
        let (result, h) = shape.try_delete_faces_h(&removed).map_err(occt)?;
        if !result.is_valid().map_err(occt)? || result.sub_count(opencascade::safe::SubKind::Solid).map_err(occt)? != 1 {
            return Err(KernelError::OperationFailed("the faces can't be deleted: the healed body is not a valid solid".into()));
        }
        self.insert(result, single_input_history(body, &h))
    }

    fn simplify(&mut self, body: BodyId) -> Result<OpResult> {
        let shape = self.body(body)?;
        let (result, h) = clone_shape(shape).try_clean_h().map_err(occt)?;
        if !result.is_valid().map_err(occt)? || result.sub_count(SubKind::Solid).map_err(occt)? != 1 {
            return Err(KernelError::OperationFailed("the simplified body is not a valid solid".into()));
        }
        self.insert(result, single_input_history(body, &h))
    }

    fn fillet_with(&mut self, body: BodyId, edges: &[EdgeId], spec: &FilletSpec) -> Result<OpResult> {
        if edges.is_empty() {
            return Err(KernelError::InvalidParameter("Select edges or faces to fillet".into()));
        }
        // OCCT continues a fillet along tangent edges; list them all, so each gets its own
        // radii (a width fillet's vary with the angle between the faces).
        let mut all: Vec<EdgeId> = Vec::new();
        for &e in edges {
            for c in self.tangent_chain(body, e)? {
                if !all.contains(&c) {
                    all.push(c);
                }
            }
        }
        if spec.profile != FilletProfile::Circular {
            let size = match spec.size {
                FilletSize::Radius(x) | FilletSize::Width(x) => x,
            };
            if size.is_nan() || size <= 0.0 {
                return Err(KernelError::InvalidParameter("The fillet size must be positive".into()));
            }
            return self.section_fillet(body, &all, spec);
        }
        let shape = self.body(body)?;
        let radii: Vec<Vec<(f64, f64)>> = match spec.size {
            FilletSize::Radius(r) => {
                if r.is_nan() || r <= 0.0 {
                    return Err(KernelError::InvalidParameter("The fillet radius must be positive".into()));
                }
                all.iter().map(|_| vec![(0.0, r)]).collect()
            }
            FilletSize::Width(w) => {
                if w.is_nan() || w <= 0.0 {
                    return Err(KernelError::InvalidParameter("The fillet width must be positive".into()));
                }
                all.iter()
                    .map(|e| width_radii(shape, *e, w))
                    .collect::<Result<_>>()?
            }
        };
        let selected = select_edges(shape, &all)?;
        // OpenCASCADE fails where the fillet exactly consumes a face (a 1 mm round across a
        // face 1 mm wide), which Parasolid takes: then once more a millionth smaller.
        let (result, h) = match shape.try_fillet_variable_h(selected.iter().zip(radii.iter().map(Vec::as_slice))) {
            Ok(r) => r,
            Err(e) => {
                let shrunk: Vec<Vec<(f64, f64)>> = radii.iter().map(|v| v.iter().map(|(t, r)| (*t, r * (1.0 - 1e-6))).collect()).collect();
                shape.try_fillet_variable_h(selected.iter().zip(shrunk.iter().map(Vec::as_slice))).map_err(|_| occt(e))?
            }
        };
        if !spec.allow_overflow {
            let infos = self.edges(body)?;
            let vertices = self.vertices(body)?;
            if overflows(&result, &h, &all, &infos, &vertices)? {
                return Err(KernelError::InvalidParameter(
                    "The fillet runs over onto a neighbouring face; turn on Allow edge overflow".into(),
                ));
            }
        }
        self.insert(result, single_input_history(body, &h))
    }

    fn draft(&mut self, body: BodyId, spec: &crate::DraftSpec) -> Result<OpResult> {
        self.draft_full(body, spec)
    }

    fn thicken_surfaces(&mut self, spec: &crate::ThickenSpec) -> Result<OpResult> {
        self.thicken_full(spec)
    }

    fn fill(&mut self, spec: &crate::FillSpec) -> Result<OpResult> {
        self.fill_full(spec)
    }

    fn sew_solid(&mut self, bodies: &[BodyId], tol: f64) -> Result<OpResult> {
        self.sew_full(bodies, tol)
    }

    fn offset(&mut self, body: BodyId, spec: &crate::OffsetSpec) -> Result<OpResult> {
        self.offset_full(body, spec)
    }

    fn fillet_variable(&mut self, body: BodyId, laws: &[crate::FilletLaw], allow_overflow: bool) -> Result<OpResult> {
        self.fillet_variable_full(body, laws, allow_overflow)
    }

    fn fillet_partial(&mut self, body: BodyId, edge: EdgeId, spec: &FilletSpec, from: f64, to: f64) -> Result<OpResult> {
        self.fillet_partial_full(body, edge, spec, from, to)
    }

    fn fillet_smooth(&mut self, body: BodyId, edges: &[EdgeId], spec: &FilletSpec, setback: f64) -> Result<OpResult> {
        self.fillet_smooth_full(body, edges, spec, setback)
    }

    fn face_normals_at(&self, body: BodyId, face: FaceId, points: &[Point3<f64>]) -> Result<Vec<Vector3<f64>>> {
        let pts: Vec<DVec3> = points.iter().map(|p| to_glam(p.coords)).collect();
        Ok(self
            .body(body)?
            .face_derivatives(face.0 as usize, &pts)
            .map_err(occt)?
            .into_iter()
            .map(|d| to_na(d.normal).normalize())
            .collect())
    }

    fn full_round(&mut self, body: BodyId, side1: FaceId, center: FaceId, side2: FaceId) -> Result<OpResult> {
        let not_here = |why: &str| {
            KernelError::InvalidParameter(format!(
                "A full round is built between two parallel flat side faces across a flat rectangular face; {why}"
            ))
        };
        let faces = self.faces(body)?;
        let infos = self.edges(body)?;
        let face = |f: FaceId| faces.iter().find(|x| x.id == f).ok_or_else(|| not_here("a face is missing"));
        let (c, a, b) = (face(center)?, face(side1)?, face(side2)?);
        if [c, a, b].iter().any(|f| f.kind != SurfaceKind::Plane) {
            return Err(not_here("these faces are not all flat"));
        }
        // The centre's edges along the sides, and the faces' outward normals there.
        let along = |side: FaceId| {
            infos.iter().find(|e| e.curve == CurveKind::Line && e.faces.contains(&Some(center)) && e.faces.contains(&Some(side)))
        };
        let (Some(ea), Some(eb)) = (along(side1), along(side2)) else {
            return Err(not_here("the side faces don't meet the middle face"));
        };
        let shape = self.body(body)?;
        let normals = |e: &EdgeInfo, of: FaceId| -> Result<Vector3<f64>> {
            let mid = shape.edge_normals(e.id.0 as usize, 3).map_err(occt)?[1];
            Ok(to_na(if e.faces[0] == Some(of) { mid.normals[0] } else { mid.normals[1] }))
        };
        let (nc, na, nb) = (normals(ea, center)?, normals(ea, side1)?, normals(eb, side2)?);
        if na.dot(&nb) > -1.0 + 1e-9 || nc.dot(&na).abs() > 1e-9 {
            return Err(not_here("the side faces are not parallel, or not square to the middle face"));
        }
        let w = (eb.mid - ea.mid).dot(&-na);
        let len = ea.length;
        let dir = ea.end - ea.start;
        if w <= LINEAR_EPS || (eb.length - len).abs() > 1e-7 || (c.area - w * len).abs() > 1e-6 * c.area.max(1.0) {
            return Err(not_here("the middle face is not a rectangle between them"));
        }
        let r = w / 2.0;
        // In the section plane at the edge's start: x across from side 1 to side 2, y out of
        // the middle face. Each corner sliver lies between the corner and the round's arc.
        let plane = Plane {
            origin: ea.start,
            x_dir: Unit::new_normalize(-na),
            normal: Unit::new_normalize(dir),
        };
        let y = plane.normal.cross(&plane.x_dir);
        let flip = if y.dot(&nc) > 0.0 { 1.0 } else { -1.0 };
        let q = |x: f64, yy: f64| Point2::new(x, yy * flip);
        let sliver = |corner: Point2<f64>, along_top: Point2<f64>, down: Point2<f64>, center: Point2<f64>| -> Region {
            let a0 = (along_top - center).y.atan2((along_top - center).x);
            let a1 = (down - center).y.atan2((down - center).x);
            let mut sweep = a1 - a0;
            while sweep > PI {
                sweep -= TAU;
            }
            while sweep < -PI {
                sweep += TAU;
            }
            Region {
                outer: Loop {
                    curves: vec![
                        Curve2::Line { a: down, b: corner, source: None },
                        Curve2::Line { a: corner, b: along_top, source: None },
                        Curve2::Arc { center, radius: r, start_angle: a0, sweep, source: None },
                    ],
                },
                holes: Vec::new(),
                source: None,
            }
        };
        let o = q(r, -r);
        let left = sliver(q(0.0, 0.0), q(r, 0.0), q(0.0, -r), o);
        let right = sliver(q(w, 0.0), q(r, 0.0), q(w, -r), o);
        let mut tools = Vec::new();
        for region in [left, right] {
            let f = profile_faces(&plane, &[region])?;
            tools.push((f[0].try_extrude(to_glam(dir)).map_err(occt)?, false, ToolFaces::Continue(center)));
        }
        let before = self.mass_properties(body)?.volume;
        let result = self.apply_tools(body, &tools)?;
        // Side faces shallower than the radius would let the slivers cut past them: refuse.
        let after = self.mass_properties(result.bodies[0])?.volume;
        let removed = (2.0 - PI / 2.0) * r * r * len;
        if ((before - after) - removed).abs() > 1e-6 * before.max(1.0) {
            self.release(result.bodies[0]);
            return Err(not_here("the side faces must reach at least half the width below it"));
        }
        Ok(result)
    }

    fn chamfer_with(&mut self, body: BodyId, edges: &[EdgeId], opts: &ChamferOpts) -> Result<OpResult> {
        if edges.is_empty() {
            return Err(KernelError::InvalidParameter("Select edges or faces to chamfer".into()));
        }
        let (d1, d2, angle) = match opts.spec {
            ChamferSpec::EqualDistance(d) => (d, d, None),
            ChamferSpec::TwoDistances(a, b) => (a, b, None),
            ChamferSpec::DistanceAngle { distance, angle } => {
                if !(angle > 0.0 && angle < PI / 2.0) {
                    return Err(KernelError::InvalidParameter("The chamfer angle must be between 0 and 90°".into()));
                }
                (distance, distance, Some(angle))
            }
        };
        if !(d1 > 0.0 && d2 > 0.0) {
            return Err(KernelError::InvalidParameter("The chamfer distance must be positive".into()));
        }
        // Tangent propagation is the caller's (it lists the chain); OCCT's chamfer continues
        // along tangent edges as well.
        let infos = self.edges(body)?;
        let faces_info = if opts.measurement == ChamferMeasure::Tangent {
            Some(self.faces(body)?)
        } else {
            None
        };
        let shape = self.body(body)?;
        let selected = select_edges(shape, edges)?;
        let faces = faces_of(shape);
        let mut kinds = Vec::new();
        let mut pairs = Vec::new();
        for (id, edge) in edges.iter().zip(&selected) {
            let info = infos.iter().find(|i| i.id == *id).ok_or_else(|| KernelError::OperationFailed(format!("unknown {id:?}")))?;
            let [Some(fa), fb] = info.faces else {
                return Err(KernelError::OperationFailed("an edge has no face".into()));
            };
            let fb = fb.unwrap_or(fa);
            let flipped = opts.flip != opts.flipped.contains(id);
            let (first, second) = if flipped { (fb, fa) } else { (fa, fb) };
            // A tangent distance on a face that curves across the edge becomes the chord along
            // the face: a sphere, or a cylinder whose axis runs along a straight edge. A face
            // that is straight across the edge (a cylinder's rim, a plane) keeps the distance.
            let dir = info.end - info.start;
            let along_axis = |fi: &FaceInfo| {
                info.curve == CurveKind::Line
                    && dir.norm() > 1e-12
                    && fi.axis.is_some_and(|a| a.dir.dot(&dir.normalize()).abs() > 1.0 - 1e-9)
            };
            let on = |face: FaceId, d: f64| -> f64 {
                let Some(fi) = faces_info.as_ref().and_then(|v| v.iter().find(|f| f.id == face)) else {
                    return d;
                };
                let across = match fi.kind {
                    SurfaceKind::Sphere => true,
                    SurfaceKind::Cylinder => along_axis(fi),
                    _ => false,
                };
                match fi.radius {
                    Some(r) if across && r > 0.0 => 2.0 * r * ((d / r).atan() / 2.0).sin(),
                    _ => d,
                }
            };
            let kind = match angle {
                Some(a) => ChamferKind::DistanceAngle(on(first, d1), a),
                None if d1 == d2 && first == second => ChamferKind::Equal(on(first, d1)),
                None => ChamferKind::TwoDistances(on(first, d1), on(second, d2)),
            };
            let face = faces
                .get(first.0 as usize)
                .ok_or_else(|| KernelError::OperationFailed("an edge has no face".into()))?;
            kinds.push(kind);
            pairs.push((edge, face));
        }
        // One OCCT chamfer per distinct kind (they take one kind per call); an equal-distance
        // chamfer without Tangent is one call.
        let all_same = kinds.windows(2).all(|w| w[0] == w[1]);
        if !all_same {
            return Err(KernelError::InvalidParameter(
                "Chamfers with different distances per edge are not supported yet".into(),
            ));
        }
        let kind = kinds[0];
        let (result, h) = shape.try_chamfer_edges_h(kind, pairs).map_err(occt)?;
        self.insert(result, single_input_history(body, &h))
    }

    fn shell_with(&mut self, body: BodyId, spec: &ShellSpec) -> Result<OpResult> {
        let t = spec.thickness;
        if t.is_nan() || t <= 0.0 {
            return Err(KernelError::InvalidParameter("The shell thickness must be positive".into()));
        }
        if !spec.hollow && spec.remove.is_empty() {
            return Err(KernelError::InvalidParameter("Select the faces to remove, or choose Hollow".into()));
        }
        let fail = || {
            KernelError::InvalidParameter(
                "The shell's walls would intersect themselves; reduce the thickness".into(),
            )
        };
        let before = self.mass_properties(body)?.volume;
        let offset = if spec.outward { t } else { -t };
        let removed_now: Vec<FaceId> = if spec.hollow { Vec::new() } else { spec.remove.clone() };
        if !spec.outward && self.walls_cross(body, &removed_now, t)? {
            return Err(fail());
        }
        let removed_ids: Vec<FaceId> = if spec.hollow { Vec::new() } else { spec.remove.clone() };
        // OCCT's thick solid first; it can't drop an offset face that vanishes (a counterbore's
        // floor narrower than the wall), so an inward shell it gets wrong is built again from
        // the region within t of the faces (see `shell_by_regions`).
        let thick = if spec.hollow {
            self.hollow_by_offset(body, offset, before)
        } else {
            self.thick_solid(body, &removed_ids, offset)
        };
        let checked = thick.and_then(|(r, h)| {
            // OCCT can return a body when the offset walls cross (even the input unchanged):
            // check it is valid and that the volume went the right way.
            let after = r.mass_properties().volume;
            let sane = after > 0.0 && (spec.outward || after < before * (1.0 - 1e-9));
            let ok = sane && r.is_valid().map_err(occt)? && r.sub_count(SubKind::Solid).map_err(occt)? == 1;
            if ok { Ok((r, h)) } else { Err(fail()) }
        });
        let (result, history) = match checked {
            Ok(x) => x,
            Err(_) if !spec.outward => {
                let (r, h) = self.shell_by_regions(body, &removed_ids, t).map_err(|_| fail())?;
                let after = r.mass_properties().volume;
                let ok = after > 0.0
                    && after < before * (1.0 - 1e-9)
                    && r.is_valid().map_err(occt)?
                    && r.sub_count(SubKind::Solid).map_err(occt)? == 1;
                if !ok {
                    return Err(fail());
                }
                (r, h)
            }
            Err(e) => return Err(e),
        };
        self.insert(result, history)
    }

    fn release(&mut self, body: BodyId) {
        self.bodies.remove(&body);
        if let Ok(mut c) = self.face_infos.lock() {
            c.remove(&body);
        }
    }

    fn write_body(&self, body: BodyId) -> Result<Vec<u8>> {
        self.body(body)?.try_to_bin_brep().map_err(occt)
    }

    fn read_body(&mut self, bytes: &[u8]) -> Result<BodyId> {
        let shape = Shape::try_from_bin_brep(bytes).map_err(occt)?;
        Ok(self.insert_raw(shape, History::default())?.bodies[0])
    }

    fn faces(&self, body: BodyId) -> Result<Vec<FaceInfo>> {
        if let Some(f) = self.face_infos.lock().ok().and_then(|c| c.get(&body).cloned()) {
            return Ok(f.as_ref().clone());
        }
        let infos = std::sync::Arc::new(face_infos(self.body(body)?));
        if let Ok(mut c) = self.face_infos.lock() {
            c.insert(body, infos.clone());
        }
        Ok(infos.as_ref().clone())
    }

    fn edges(&self, body: BodyId) -> Result<Vec<EdgeInfo>> {
        let shape = self.body(body)?;
        let geometry = shape.edges_geometry().map_err(occt)?;
        let around = shape.edge_faces().map_err(occt)?;
        let circles = shape.edge_circles().map_err(occt)?;
        if around.len() != geometry.len() || circles.len() != geometry.len() {
            return Err(KernelError::OperationFailed("inconsistent edge lists".into()));
        }
        // An edge with one face is a seam if the face runs along it twice (a cylinder's), else
        // a free edge (the boundary of a surface body).
        let single: Vec<usize> = around
            .iter()
            .enumerate()
            .filter(|(i, f)| f.len() == 1 && geometry[*i].curve != CurveType::Degenerate)
            .map(|(i, _)| i)
            .collect();
        let mut free = vec![false; around.len()];
        if !single.is_empty() {
            let faces = faces_of(shape);
            let edges = Edges::of(shape);
            for i in single {
                let Some(face) = faces.get(around[i][0]) else { continue };
                let Some(edge) = edges.list.get(i) else { continue };
                let uses = face.edges().filter(|e| e.is_same(edge)).count();
                free[i] = uses < 2;
            }
        }
        Ok(geometry
            .iter()
            .zip(around)
            .enumerate()
            .map(|(i, (g, faces))| {
                let face = |k: usize| faces.get(k).map(|f| FaceId(*f as u64));
                // A seam lists its face once: it is on both sides. A free edge has one side.
                let second = if faces.len() == 1 && g.curve != CurveType::Degenerate && !free[i] {
                    face(0)
                } else {
                    face(1)
                };
                let p = |v: DVec3| Point3::from(to_na(v));
                EdgeInfo {
                    id: EdgeId(i as u64),
                    faces: [face(0), second],
                    length: g.length,
                    curve: match g.curve {
                        CurveType::Line => CurveKind::Line,
                        CurveType::Circle => CurveKind::Circle,
                        CurveType::Ellipse => CurveKind::Ellipse,
                        CurveType::Degenerate => CurveKind::Degenerate,
                        CurveType::Other => CurveKind::Other,
                    },
                    start: p(g.start),
                    end: p(g.end),
                    mid: p(g.mid),
                    start_tangent: to_na(g.start_tangent),
                    end_tangent: to_na(g.end_tangent),
                    circle: circles[i].and_then(|c| {
                        let n = to_na(c.dir);
                        (n.norm() > 0.5).then(|| Circle3 {
                            center: Point3::from(to_na(c.origin)),
                            normal: Unit::new_normalize(n),
                            radius: c.radius,
                        })
                    }),
                }
            })
            .collect())
    }

    fn vertices(&self, body: BodyId) -> Result<Vec<VertexInfo>> {
        let shape = self.body(body)?;
        Ok(shape
            .vertices_info()
            .map_err(occt)?
            .into_iter()
            .enumerate()
            .map(|(i, (p, edges))| VertexInfo {
                id: VertexId(i as u64),
                point: Point3::from(to_na(p)),
                edges: edges.into_iter().map(|e| EdgeId(e as u64)).collect(),
            })
            .collect())
    }

    fn mass_properties(&self, body: BodyId) -> Result<MassProperties> {
        let shape = self.body(body)?;
        let props = shape.mass_properties();
        if shape.sub_count(SubKind::Solid).map_err(occt)? == 0 {
            // A surface body: no volume; its centre is the area centroid.
            let faces = self.faces(body)?;
            let area: f64 = faces.iter().map(|f| f.area).sum();
            let center = if area > 0.0 {
                Point3::from(faces.iter().fold(Vector3::zeros(), |acc, f| acc + f.center.coords * f.area) / area)
            } else {
                Point3::origin()
            };
            return Ok(MassProperties {
                volume: 0.0,
                surface_area: props.surface_area,
                center_of_mass: center,
                inertia: nalgebra::Matrix3::zeros(),
            });
        }
        Ok(MassProperties {
            volume: props.volume,
            surface_area: props.surface_area,
            center_of_mass: Point3::from(to_na(props.center_of_mass)),
            inertia: nalgebra::Matrix3::from_fn(|r, c| props.inertia[r][c]),
        })
    }

    fn solid_count(&self, body: BodyId) -> Result<usize> {
        self.body(body)?.sub_count(SubKind::Solid).map_err(occt)
    }

    /// A plain fuse and its solid count: no unifying of faces on one surface, which the real
    /// union does and which can run away on some inputs (the contact test only needs the
    /// count).
    fn joins(&mut self, a: BodyId, b: BodyId) -> Result<bool> {
        let fused = self.body(a)?.try_union(self.body(b)?).map_err(occt)?;
        Ok(fused.sub_count(SubKind::Solid).map_err(occt)? == 1)
    }

    fn bounding_box(&self, body: BodyId) -> Result<Aabb> {
        let (min, max) = self.body(body)?.bbox().map_err(occt)?;
        Ok(Aabb {
            min: Point3::from(to_na(min)),
            max: Point3::from(to_na(max)),
        })
    }

    fn ray_hits(&self, body: BodyId, origin: Point3<f64>, dir: Vector3<f64>) -> Result<Vec<RayHit>> {
        let len = dir.norm();
        if len < LINEAR_EPS {
            return Err(KernelError::InvalidParameter("a ray needs a direction".into()));
        }
        let mut hits: Vec<RayHit> = self
            .body(body)?
            .ray_hits(to_glam(origin.coords), to_glam(dir / len))
            .map_err(occt)?
            .into_iter()
            .map(|h| RayHit {
                face: FaceId(h.face as u64),
                t: h.t,
                point: Point3::from(to_na(h.point)),
            })
            .collect();
        hits.sort_by(|a, b| a.t.total_cmp(&b.t));
        Ok(hits)
    }

    fn tessellate(&self, body: BodyId, quality: Tessellation) -> Result<TriMesh> {
        let shape = self.body(body)?;
        let deflection = quality.deflection.max(1e-4);
        let angle = quality.angle.clamp(1e-3, 1.0);
        // Meshing the whole body gives the faces one discretisation of their shared edges; the
        // faces and edges below read it back.
        shape.try_mesh(deflection, angle).map_err(occt)?;

        let mut out = TriMesh::default();
        for (fi, face) in faces_of(shape).iter().enumerate() {
            // A face neither mesher could triangulate (a degenerate sliver in an imported STEP)
            // is left out of the display mesh rather than failing the whole body.
            let Ok(mesh) = face.triangulation() else { continue };
            let base = out.positions.len() as u32;
            out.positions
                .extend(mesh.positions.iter().map(|v| Point3::from(to_na(*v))));
            out.normals.extend(mesh.normals.iter().map(|n| to_na(*n)));
            // Keep the arrays parallel even if OCCT returned no normals for a face.
            out.normals.resize(out.positions.len(), Vector3::zeros());
            for tri in &mesh.indices {
                out.indices.push(tri.map(|i| base + i));
                out.triangle_faces.push(FaceId(fi as u64));
            }
        }
        out.edges = Edges::of(shape)
            .list
            .iter()
            .enumerate()
            .map(|(i, edge)| {
                let points = edge.polyline(shape, angle, deflection).map_err(occt)?;
                Ok((
                    EdgeId(i as u64),
                    points.into_iter().map(|p| Point3::from(to_na(p))).collect(),
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(out)
    }

    fn project(&self, bodies: &[BodyId], frame: &ViewFrame, opts: &ProjectOptions) -> Result<Projection> {
        use opencascade::hlr::{HlrClass, HlrCurve, hlr_project};
        use opencascade::primitives::Compound;
        let shapes = bodies
            .iter()
            .map(|&id| self.body(id))
            .collect::<Result<Vec<_>>>()?;
        if shapes.is_empty() {
            return Ok(Projection::default());
        }
        // The bodies' edges as polylines (on the display mesh's discretisation) and their
        // faces, to find where each projected edge came from.
        let quality = Tessellation {
            deflection: 0.05,
            angle: PI / 72.0,
        };
        let mut edge_lists = Vec::with_capacity(shapes.len());
        let mut face_lists = Vec::with_capacity(shapes.len());
        for shape in &shapes {
            shape.try_mesh(quality.deflection, quality.angle).map_err(occt)?;
            let edges = Edges::of(shape)
                .list
                .iter()
                .enumerate()
                .map(|(i, edge)| {
                    let points = edge.polyline(shape, quality.angle, quality.deflection).map_err(occt)?;
                    Ok((EdgeId(i as u64), points.into_iter().map(|p| Point3::from(to_na(p))).collect()))
                })
                .collect::<Result<Vec<_>>>()?;
            edge_lists.push(edges);
            face_lists.push(face_infos(shape));
        }
        let seam_lists = bodies
            .iter()
            .map(|&id| {
                Ok(self
                    .edges(id)?
                    .into_iter()
                    .filter_map(|e| match e.faces {
                        [Some(a), Some(b)] if a == b => Some((e.id, a)),
                        _ => None,
                    })
                    .collect::<Vec<_>>())
            })
            .collect::<Result<Vec<_>>>()?;
        let tolerance = opts.tolerance.max(1e-4);
        let origin = to_glam(frame.origin.coords);
        let (dir, x) = (to_glam(frame.dir), to_glam(frame.x));
        let mut hlr = if shapes.len() == 1 {
            hlr_project(shapes[0], origin, dir, x, tolerance)
        } else {
            let compound: Shape = Compound::from_shapes(shapes.iter().copied()).into();
            hlr_project(&compound, origin, dir, x, tolerance)
        }
        .map_err(occt)?;
        // Hidden edges right behind visible ones (a box's back edges behind its front ones)
        // would draw dashes over solid lines.
        hlr.remove_hidden_behind_visible((4.0 * tolerance).max(0.005));
        let p2 = |v: glam::DVec2| Point2::new(v.x, v.y);
        let mut out = Projection::default();
        for (class, visible, e) in hlr.iter() {
            if !visible && !opts.hidden {
                continue;
            }
            let curve = match e.curve {
                HlrCurve::Line { start, end } => ProjCurve::Line {
                    start: p2(start),
                    end: p2(end),
                },
                HlrCurve::Arc {
                    center,
                    radius,
                    start,
                    mid,
                    end,
                    full,
                } => ProjCurve::Arc {
                    center: p2(center),
                    radius,
                    start: p2(start),
                    mid: p2(mid),
                    end: p2(end),
                    full,
                },
                HlrCurve::Other => ProjCurve::Polyline,
            };
            out.edges.push(ProjEdge {
                visibility: if visible {
                    ProjVisibility::Visible
                } else {
                    ProjVisibility::Hidden
                },
                class: match class {
                    HlrClass::Sharp => ProjClass::Sharp,
                    HlrClass::Smooth => ProjClass::Smooth,
                    HlrClass::Outline => ProjClass::Outline,
                },
                curve,
                points: e.polyline.iter().map(|v| p2(*v)).collect(),
                source: None,
            });
        }
        let sources: Vec<crate::projection::SourceBody> = edge_lists
            .iter()
            .zip(&face_lists)
            .zip(&seam_lists)
            .map(|((edges, faces), seams)| crate::projection::SourceBody { edges, faces, seams })
            .collect();
        crate::projection::attach_sources(&mut out, frame, &sources, quality.deflection + 2.0 * tolerance + 1e-3);
        Ok(out)
    }

    fn import_step(&mut self, bytes: &[u8]) -> Result<Vec<BodyId>> {
        let failed = |why: String| KernelError::OperationFailed(format!("STEP import: {why}"));
        let path = std::env::temp_dir().join(format!(
            "cadrs-occt-in-{}-{:?}.step",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::write(&path, bytes).map_err(|e| failed(e.to_string()))?;
        // The STEP reader shares the writer's global state (see `export_step`).
        static STEP_READER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let read = {
            let _guard = STEP_READER.lock().unwrap_or_else(|e| e.into_inner());
            Shape::read_step(&path)
        };
        let _ = std::fs::remove_file(&path);
        let shape = read.map_err(|e| failed(e.to_string()))?;
        let solids = shape.sub_shapes(SubKind::Solid).map_err(occt)?;
        let mut out = Vec::new();
        if solids.is_empty() {
            if shape.faces().next().is_none() {
                return Err(failed("the file has no geometry".into()));
            }
            // Shells or loose faces: a solid if they close up, else one surface body.
            let tol = 1e-4;
            let body = Shape::try_sew_solid(&[&shape], tol)
                .ok()
                .filter(|s| s.mass_properties().volume > 0.0)
                .unwrap_or(shape);
            out.extend(self.insert_raw(body, History::default())?.bodies);
            return Ok(out);
        }
        // The solids, then the surfaces no solid holds.
        let loose = loose_shells(&shape, &solids)?;
        for s in solids.into_iter().chain(loose) {
            match self.insert_raw(s, History::default()) {
                Ok(r) => out.extend(r.bodies),
                Err(e) => {
                    out.iter().for_each(|b| self.release(*b));
                    return Err(e);
                }
            }
        }
        Ok(out)
    }

    fn mesh_solid(&mut self, triangles: &[[Point3<f64>; 3]], tol: f64) -> Result<BodyId> {
        let failed = |why: String| KernelError::OperationFailed(format!("mesh: {why}"));
        let g = |p: &Point3<f64>| dvec3(p.x, p.y, p.z);
        let mut faces: Vec<Shape> = Vec::with_capacity(triangles.len());
        for t in triangles {
            let [a, b, c] = [g(&t[0]), g(&t[1]), g(&t[2])];
            if (b - a).cross(c - a).length() <= LINEAR_EPS {
                continue;
            }
            let edges = [Edge::try_segment(a, b), Edge::try_segment(b, c), Edge::try_segment(c, a)];
            let Ok(edges) = edges.into_iter().collect::<std::result::Result<Vec<Edge>, _>>() else { continue };
            let Ok(wire) = Wire::try_from_edges(edges.iter()) else { continue };
            let Ok(face) = Face::try_from_wires(&wire, &[]) else { continue };
            faces.push(Shape::from(&face));
        }
        if faces.len() < 4 {
            return Err(failed("fewer than four usable triangles".into()));
        }
        // Closed: every edge (between corners welded within `tol`) is used by an even number of
        // triangles. OCCT makes a "solid" of an open shell too.
        let key = |p: &Point3<f64>| [p.x, p.y, p.z].map(|c| (c / tol.max(1e-12)).round() as i64);
        let mut uses: HashMap<([i64; 3], [i64; 3]), u32> = HashMap::new();
        for t in triangles {
            let k = [key(&t[0]), key(&t[1]), key(&t[2])];
            if k[0] == k[1] || k[1] == k[2] || k[2] == k[0] {
                continue;
            }
            for (a, b) in [(k[0], k[1]), (k[1], k[2]), (k[2], k[0])] {
                *uses.entry(if a < b { (a, b) } else { (b, a) }).or_default() += 1;
            }
        }
        let open = uses.values().filter(|n| *n % 2 == 1).count();
        if open > 0 {
            return Err(failed(format!("the mesh is not closed ({open} open edges)")));
        }
        // Consistently wound (every edge crossed once each way): the solid is built from the
        // welded triangles directly, sharing their vertices and edges. Sewing them, which also
        // turns badly wound triangles, took minutes for an STL of 27 000 triangles.
        let mut index: HashMap<[i64; 3], i32> = HashMap::new();
        let mut points: Vec<f64> = Vec::new();
        let mut tris: Vec<i32> = Vec::new();
        let mut directed: HashMap<(i32, i32), u32> = HashMap::new();
        for t in triangles {
            let k = [key(&t[0]), key(&t[1]), key(&t[2])];
            if k[0] == k[1] || k[1] == k[2] || k[2] == k[0] {
                continue;
            }
            let ids = [0, 1, 2].map(|i| {
                *index.entry(k[i]).or_insert_with(|| {
                    points.extend([t[i].x, t[i].y, t[i].z]);
                    (points.len() / 3 - 1) as i32
                })
            });
            for (a, b) in [(ids[0], ids[1]), (ids[1], ids[2]), (ids[2], ids[0])] {
                *directed.entry((a, b)).or_default() += 1;
            }
            tris.extend(ids);
        }
        let wound = directed.iter().all(|(&(a, b), n)| *n == 1 && directed.get(&(b, a)) == Some(&1));
        let solid = match wound.then(|| Shape::try_mesh_solid(&points, &tris, tol)) {
            Some(Ok(s)) => s,
            _ => {
                let refs: Vec<&Shape> = faces.iter().collect();
                Shape::try_sew_solid(&refs, tol).map_err(|e| failed(e.to_string()))?
            }
        };
        let v = solid.mass_properties().volume;
        if !v.is_finite() || v <= 0.0 {
            return Err(failed("the triangles do not enclose a volume".into()));
        }
        // Coplanar neighbours merged into one face (a CAD model's flat faces come back whole;
        // a curved surface's facets stay). Skipped for huge meshes. (It was skipped above 20 000
        // triangles while it was slow; a 27 000-triangle baseplate now merges in about a second
        // and every later feature on it is faster for it.)
        const UNIFY_MAX: usize = 200_000;
        let solid = match (triangles.len() <= UNIFY_MAX).then(|| solid.try_clean()) {
            Some(Ok(u)) if (u.mass_properties().volume - v).abs() <= v * 1e-9 && u.sub_count(SubKind::Solid).map_err(occt)? == 1 => u,
            _ => solid,
        };
        Ok(self.insert_raw(solid, History::default())?.bodies[0])
    }

    fn export_step(&self, bodies: &[BodyId]) -> Result<Vec<u8>> {
        let shapes = bodies
            .iter()
            .map(|&id| self.body(id))
            .collect::<Result<Vec<_>>>()?;
        let path = std::env::temp_dir().join(format!(
            "cadrs-occt-{}-{:?}.step",
            std::process::id(),
            std::thread::current().id()
        ));
        let failed = |why: String| KernelError::OperationFailed(format!("STEP export: {why}"));
        // OCCT's STEP writer keeps global state (the STEP schema and its units) and crashes
        // when two threads write at once, so writes are serialised.
        static STEP_WRITER: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = STEP_WRITER.lock().unwrap_or_else(|e| e.into_inner());
        Shape::write_all_step(shapes, &path).map_err(|e| failed(e.to_string()))?;
        let bytes = std::fs::read(&path).map_err(|e| failed(e.to_string()));
        let _ = std::fs::remove_file(&path);
        bytes
    }

    fn import_model(&mut self, format: crate::exchange::ExchangeFormat, bytes: &[u8]) -> Result<crate::exchange::ImportedModel> {
        exchange::import_model(self, format, bytes)
    }

    fn export_model(
        &self,
        format: crate::exchange::ExchangeFormat,
        name: &str,
        parts: &[(BodyId, String)],
        instances: &[crate::exchange::ExportInstance],
    ) -> Result<Vec<u8>> {
        exchange::export_model(self, format, name, parts, instances)
    }
}

fn to_glam(v: Vector3<f64>) -> DVec3 {
    dvec3(v.x, v.y, v.z)
}

fn to_na(v: DVec3) -> Vector3<f64> {
    Vector3::new(v.x, v.y, v.z)
}

fn clone_shape(shape: &Shape) -> Shape {
    // A zero translation copies the shape (BRepBuilderAPI_Transform with copy = true).
    shape.translated(DVec3::ZERO)
}

/// The shells of `shape` that aren't one of `solids`' (none of their faces is a solid's), each
/// a piece of its own: sewn into a solid when it closes, else the surface as it is (an imported
/// file's surface bodies, such as a board's silkscreen).
fn loose_shells(shape: &Shape, solids: &[Shape]) -> Result<Vec<Shape>> {
    let mut in_solids: HashMap<u64, Vec<Face>> = HashMap::new();
    for s in solids {
        for f in faces_of(s) {
            in_solids.entry(f.identity_hash()).or_default().push(f);
        }
    }
    let owned = |f: &Face| in_solids.get(&f.identity_hash()).is_some_and(|v| v.iter().any(|g| g.is_same(f)));
    Ok(shape
        .sub_shapes(SubKind::Shell)
        .map_err(occt)?
        .into_iter()
        .filter(|sh| !faces_of(sh).iter().any(owned))
        .map(|sh| match Shape::try_sew_solid(&[&sh], 1e-6) {
            Ok(s) if s.sub_count(SubKind::Solid).unwrap_or(0) == 1 && s.is_valid().unwrap_or(false) => s,
            _ => clone_shape(&sh),
        })
        .collect())
}

fn select_edges(shape: &Shape, ids: &[EdgeId]) -> Result<Vec<Edge>> {
    if ids.is_empty() {
        return Err(KernelError::OperationFailed("no edges selected".into()));
    }
    let mut edges: Vec<Option<Edge>> = Edges::of(shape).list.into_iter().map(Some).collect();
    ids.iter()
        .map(|id| {
            edges
                .get_mut(id.0 as usize)
                .and_then(Option::take)
                .ok_or_else(|| KernelError::OperationFailed(format!("unknown or repeated {id:?}")))
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Profiles

/// Builds one planar face per region, with the outer loop counter-clockwise and holes clockwise
/// (seen from the plane normal), as OCCT expects.
fn profile_faces(plane: &Plane, regions: &[Region]) -> Result<Vec<Face>> {
    if regions.is_empty() {
        return Err(KernelError::InvalidProfile(
            "the profile has no regions".into(),
        ));
    }
    regions
        .iter()
        .map(|region| {
            let outer = loop_wire(plane, &oriented(&region.outer, true)?)?;
            let holes = region
                .holes
                .iter()
                .map(|hole| loop_wire(plane, &oriented(hole, false)?))
                .collect::<Result<Vec<_>>>()?;
            Face::try_from_wires(&outer, &holes).map_err(occt)
        })
        .collect()
}

/// Returns the loop's curves ordered counter-clockwise (`ccw`) or clockwise.
fn oriented(lp: &Loop, ccw: bool) -> Result<Vec<Curve2>> {
    if lp.curves.is_empty() {
        return Err(KernelError::InvalidProfile("empty loop".into()));
    }
    let area = signed_area(&lp.curves);
    if !area.is_finite() || area.abs() < LINEAR_EPS {
        return Err(KernelError::InvalidProfile("loop encloses no area".into()));
    }
    if (area > 0.0) == ccw {
        Ok(lp.curves.clone())
    } else {
        Ok(lp.curves.iter().rev().map(reversed).collect())
    }
}

/// The signed area enclosed by the loop (positive = counter-clockwise), via the
/// shoelace integral ½∮(x dy − y dx) over the exact curves.
fn signed_area(curves: &[Curve2]) -> f64 {
    let mut twice = 0.0;
    for c in curves {
        twice += match *c {
            Curve2::Line { a, b, .. } => a.x * b.y - b.x * a.y,
            Curve2::Arc {
                center,
                radius,
                start_angle,
                sweep,
                ..
            } => {
                let (t0, t1) = (start_angle, start_angle + sweep);
                radius * center.x * (t1.sin() - t0.sin())
                    + radius * center.y * (t0.cos() - t1.cos())
                    + radius * radius * sweep
            }
            Curve2::Circle { radius, .. } => 2.0 * PI * radius * radius.abs(),
            Curve2::Ellipse {
                major_radius,
                minor_radius,
                ..
            } => 2.0 * PI * major_radius * minor_radius,
            Curve2::EllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start,
                sweep,
                ..
            } => {
                // p(t) = c + A cos t + B sin t, with A = major·u and B = minor·v:
                // ∮ p × dp = (c×A)(cos t1 − cos t0) + (c×B)(sin t1 − sin t0) + (A×B)(t1 − t0).
                let (s, co) = rotation.sin_cos();
                let a = Vector3::new(major_radius * co, major_radius * s, 0.0);
                let b = Vector3::new(-minor_radius * s, minor_radius * co, 0.0);
                let cross = |p: Vector3<f64>, q: Vector3<f64>| p.x * q.y - p.y * q.x;
                let c = Vector3::new(center.x, center.y, 0.0);
                let (t0, t1) = (start, start + sweep);
                cross(c, a) * (t1.cos() - t0.cos())
                    + cross(c, b) * (t1.sin() - t0.sin())
                    + cross(a, b) * sweep
            }
            Curve2::OffsetEllipseArc { .. } => {
                // ∫ p × p' ds numerically (Simpson's rule on 512 intervals of the parameter).
                let n = 512;
                let h = 1.0 / n as f64;
                let f = |s: f64| {
                    let p = c.point_at(s);
                    let d = (c.point_at((s + 1e-6).min(1.0)) - c.point_at((s - 1e-6).max(0.0)))
                        / ((s + 1e-6).min(1.0) - (s - 1e-6).max(0.0));
                    p.x * d.y - p.y * d.x
                };
                (0..=n)
                    .map(|i| {
                        let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
                        w * f(i as f64 * h)
                    })
                    .sum::<f64>()
                    * h
                    / 3.0
            }
            // ∫ p × p' dt: a quintic, exact by three-point Gauss–Legendre.
            Curve2::Bezier { poles: p, .. } => {
                let d = |t: f64| {
                    let u = 1.0 - t;
                    let (a, b, c) = (3.0 * u * u, 6.0 * u * t, 3.0 * t * t);
                    Vector3::new(
                        a * (p[1].x - p[0].x) + b * (p[2].x - p[1].x) + c * (p[3].x - p[2].x),
                        a * (p[1].y - p[0].y) + b * (p[2].y - p[1].y) + c * (p[3].y - p[2].y),
                        0.0,
                    )
                };
                let x = (0.6_f64).sqrt() / 2.0;
                [(0.5 - x, 5.0 / 18.0), (0.5, 8.0 / 18.0), (0.5 + x, 5.0 / 18.0)]
                    .iter()
                    .map(|(t, w)| {
                        let q = crate::types::bezier_point(&p, *t);
                        let v = d(*t);
                        w * (q.x * v.y - q.y * v.x)
                    })
                    .sum::<f64>()
            }
        };
    }
    twice / 2.0
}

fn reversed(c: &Curve2) -> Curve2 {
    match c.clone() {
        Curve2::Line { a, b, source } => Curve2::Line { a: b, b: a, source },
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            source,
        } => Curve2::Arc {
            center,
            radius,
            start_angle: start_angle + sweep,
            sweep: -sweep,
            source,
        },
        // Internal convention: a negative radius marks a clockwise circle (see `curve_edge`).
        Curve2::Circle {
            center,
            radius,
            source,
        } => Curve2::Circle {
            center,
            radius: -radius,
            source,
        },
        // A whole ellipse runs clockwise with its minor axis flipped.
        Curve2::Ellipse {
            center,
            major_radius,
            minor_radius,
            rotation,
            source,
        } => Curve2::Ellipse {
            center,
            major_radius,
            minor_radius: -minor_radius,
            rotation,
            source,
        },
        Curve2::EllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start,
            sweep,
            source,
        } => Curve2::EllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start: start + sweep,
            sweep: -sweep,
            source,
        },
        c @ (Curve2::OffsetEllipseArc { .. } | Curve2::Bezier { .. }) => c.reversed(),
    }
}

/// A closed wire through the loop's curves. Each curve starts exactly where the previous one
/// ended (the loop's joints are shared points), so OCCT sees a connected wire even when the
/// sketch's curve ends differ in the last bits.
fn loop_wire(plane: &Plane, curves: &[Curve2]) -> Result<Wire> {
    // A whole offset ellipse is two half curves: a closed B-spline edge (it isn't periodic like
    // a circle or an ellipse) makes a seamed face whose edges booleans handle badly (P3.7).
    let split: Vec<Curve2>;
    let curves = match curves {
        [Curve2::OffsetEllipseArc { center, major_radius, minor_radius, rotation, start, sweep, offset, source }]
            if sweep.abs() >= TAU - 1e-9 =>
        {
            let half = |s: f64| Curve2::OffsetEllipseArc {
                center: *center,
                major_radius: *major_radius,
                minor_radius: *minor_radius,
                rotation: *rotation,
                start: s,
                sweep: sweep / 2.0,
                offset: *offset,
                source: *source,
            };
            split = vec![half(*start), half(start + sweep / 2.0)];
            &split[..]
        }
        _ => curves,
    };
    let n = curves.len();
    let starts: Vec<Point2<f64>> = curves.iter().map(Curve2::start).collect();
    let edges = (0..n)
        .map(|i| curve_edge(plane, &curves[i], starts[i], starts[(i + 1) % n], n == 1))
        .collect::<Result<Vec<_>>>()?;
    Wire::try_from_edges(&edges).map_err(occt)
}

/// The edge of one loop curve from `from` to `to` (the loop's joints). `alone`: the curve is
/// the whole loop (a circle or an ellipse).
fn curve_edge(
    plane: &Plane,
    curve: &Curve2,
    from: Point2<f64>,
    to: Point2<f64>,
    alone: bool,
) -> Result<Edge> {
    let at = |p: Point2<f64>| to_glam(plane.to_model(p).coords);
    let normal = to_glam(plane.normal.into_inner());
    let x_dir_at = |angle: f64| {
        let (s, c) = angle.sin_cos();
        to_glam((plane.x_dir.into_inner() * c + plane.y_dir().into_inner() * s).normalize())
    };
    match *curve {
        Curve2::Line { .. } => {
            if (to - from).norm() < LINEAR_EPS {
                return Err(KernelError::InvalidProfile("zero-length line".into()));
            }
            Edge::try_segment(at(from), at(to)).map_err(occt)
        }
        Curve2::Bezier { poles, .. } => {
            if (to - from).norm() < LINEAR_EPS {
                return Err(KernelError::InvalidProfile("a Bézier curve with its ends together".into()));
            }
            Edge::try_bezier(&[at(from), at(poles[1]), at(poles[2]), at(to)], &[1.0; 4]).map_err(occt)
        }
        Curve2::Arc {
            center,
            radius,
            start_angle,
            sweep,
            ..
        } => {
            if radius <= LINEAR_EPS || sweep.abs() < 1e-12 {
                return Err(KernelError::InvalidProfile("degenerate arc".into()));
            }
            if alone || sweep.abs() >= TAU - 1e-9 {
                let n = normal * sweep.signum();
                return Edge::try_circle(at(center), n, x_dir_at(start_angle), radius).map_err(occt);
            }
            let t = start_angle + sweep / 2.0;
            let mid = Point2::new(center.x + radius * t.cos(), center.y + radius * t.sin());
            Edge::try_arc(at(from), at(mid), at(to)).map_err(occt)
        }
        Curve2::Circle { center, radius, .. } => {
            if radius.abs() <= LINEAR_EPS {
                return Err(KernelError::InvalidProfile("degenerate circle".into()));
            }
            // Negative radius = clockwise (see `reversed`).
            let n = normal * radius.signum();
            Edge::try_circle(at(center), n, x_dir_at(0.0), radius.abs()).map_err(occt)
        }
        Curve2::Ellipse {
            center,
            major_radius,
            minor_radius,
            rotation,
            ..
        } => {
            let e = ellipse_axes(major_radius, minor_radius, rotation, 0.0, TAU)?;
            let n = if e.ccw { normal } else { -normal };
            Edge::ellipse(at(center), n, x_dir_at(e.rotation), e.major, e.minor, None).map_err(occt)
        }
        Curve2::EllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start,
            sweep,
            ..
        } => {
            if sweep.abs() < 1e-12 {
                return Err(KernelError::InvalidProfile("degenerate ellipse arc".into()));
            }
            let e = ellipse_axes(major_radius, minor_radius, rotation, start, sweep)?;
            let n = if e.ccw { normal } else { -normal };
            let ends = (!(alone || sweep.abs() >= TAU - 1e-9)).then(|| (at(from), at(to)));
            Edge::ellipse(at(center), n, x_dir_at(e.rotation), e.major, e.minor, ends).map_err(occt)
        }
        Curve2::OffsetEllipseArc {
            center,
            major_radius,
            minor_radius,
            rotation,
            start,
            sweep,
            offset,
            ..
        } => {
            if sweep.abs() < 1e-12 {
                return Err(KernelError::InvalidProfile("degenerate offset ellipse".into()));
            }
            let e = ellipse_axes(major_radius, minor_radius, rotation, start, sweep)?;
            let n = if e.ccw { normal } else { -normal };
            let whole = alone || sweep.abs() >= TAU - 1e-9;
            // The arc runs counter-clockwise about `n` from its start to its end.
            let ends = (!whole).then(|| (at(from), at(to)));
            Edge::try_offset_ellipse(at(center), n, x_dir_at(e.rotation), e.major, e.minor, offset, ends).map_err(occt)
        }
    }
}

/// An ellipse arc in OCCT's terms: major ≥ minor > 0, and the direction it runs.
struct EllipseAxes {
    major: f64,
    minor: f64,
    /// The major axis's angle from the plane's x axis.
    rotation: f64,
    /// Counter-clockwise seen from the plane normal.
    ccw: bool,
}

/// `c + R(rot)·(a cos t, b sin t)` for t from `start` through `sweep`, restated with
/// `major ≥ minor > 0`: a negative `b` mirrors the direction of travel, and `b > a` turns the
/// major axis a quarter turn (the curve itself is unchanged).
fn ellipse_axes(a: f64, b: f64, rotation: f64, _start: f64, sweep: f64) -> Result<EllipseAxes> {
    if a.abs() <= LINEAR_EPS || b.abs() <= LINEAR_EPS {
        return Err(KernelError::InvalidProfile("degenerate ellipse".into()));
    }
    // With b < 0 the point at t is the point at −t of the ellipse with |b|: travel reverses.
    let ccw = (sweep > 0.0) == (b > 0.0);
    let (a, b) = (a.abs(), b.abs());
    Ok(if b > a {
        EllipseAxes {
            major: b,
            minor: a,
            rotation: rotation + PI / 2.0,
            ccw,
        }
    } else {
        EllipseAxes {
            major: a,
            minor: b,
            rotation,
            ccw,
        }
    })
}

// ---------------------------------------------------------------------------------------------
// Queries

/// Every face's [`FaceInfo`], with the axes of curved faces.
fn face_infos(shape: &Shape) -> Vec<FaceInfo> {
    let axes = shape.face_axes().unwrap_or_default();
    faces_of(shape)
        .iter()
        .enumerate()
        .map(|(i, face)| FaceInfo {
            axis: axes.get(i).copied().flatten().and_then(|a| {
                let dir = to_na(a.dir);
                (dir.norm() > 0.5).then(|| Axis {
                    origin: Point3::from(to_na(a.origin)),
                    dir: Unit::new_normalize(dir),
                })
            }),
            radius: axes.get(i).copied().flatten().map(|a| a.radius).filter(|r| *r > 0.0),
            ..face_info(FaceId(i as u64), face)
        })
        .collect()
}

fn face_info(id: FaceId, face: &Face) -> FaceInfo {
    let area = face.surface_area();
    let center = Point3::from(to_na(face.center_of_mass()));
    let kind = match face.surface_type() {
        Ok(SurfaceType::Plane) => SurfaceKind::Plane,
        Ok(SurfaceType::Cylinder) => SurfaceKind::Cylinder,
        Ok(SurfaceType::Cone) => SurfaceKind::Cone,
        Ok(SurfaceType::Sphere) => SurfaceKind::Sphere,
        Ok(SurfaceType::Torus) => SurfaceKind::Torus,
        Ok(SurfaceType::Other) | Err(_) => SurfaceKind::Other,
    };
    let plane = (kind == SurfaceKind::Plane)
        .then(|| {
            // The area centroid lies in the plane, and projecting onto a plane can't fail. (A
            // point of the boundary searched the whole shape for the edge: minutes for a
            // 27 000-face STL.)
            let n = to_na(face.normal_at(to_glam(center.coords)));
            if n.norm() < 0.5 {
                return None;
            }
            let n = n.normalize();
            let helper = if n.x.abs() < 0.9 {
                Vector3::x()
            } else {
                Vector3::y()
            };
            let x_dir = Unit::new_normalize(helper - n * helper.dot(&n));
            // The area centroid of a planar face lies in its plane.
            Some(Plane {
                origin: center,
                x_dir,
                normal: Unit::new_normalize(n),
            })
        })
        .flatten();
    FaceInfo {
        id,
        kind,
        plane,
        area,
        center,
        axis: None,
        radius: None,
    }
}

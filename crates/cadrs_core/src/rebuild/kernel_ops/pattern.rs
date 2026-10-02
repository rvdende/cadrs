//! Rebuilding patterns and mirrors (P3.8, PS22–PS26). A child of `rebuild::kernel_ops`, so it
//! shares its helpers (face lookup, merging, cutting, finishing).
//!
//! - Every copy is a kernel body moved by a [`Motion`] (a reflection for a mirror). Its faces are
//!   renamed `FaceOrigin::Instance { of, face, instance }` under the pattern's id, so the copies'
//!   faces have names of their own that survive rebuilds ("Face of Linear pattern 1").
//! - **Part**: the parts copied; New makes one part per copy (with the seed's palette entry and
//!   `source`, so it shows the seed's appearance and material, PS9.6), the other tabs combine
//!   the copies with the merge scope like an extrude's body.
//! - **Face**: the pocket or boss the faces bound (`Kernel::face_tool`), copied and cut or added.
//! - **Feature** with Reapply: each instance's features are rebuilt with their sketches moved
//!   and their references to the patterned features' faces and edges mapped to the instance's
//!   (references to other geometry stay: an "Up to face" stops at the same face, PS27.5). The
//!   instances' faces are renamed afterwards.
//! - **Feature** without Reapply, and Feature mirror: the material the features removed and
//!   added (the parts before them less the parts after, and the other way) copied, cut and added.

use std::collections::{HashMap, HashSet};

use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{Axis, BoolOp, FaceId, Kernel, Motion, PointClass};
use cadrs_sketch::{FaceName, FaceOrigin, FeaturePlane, PlaneFrame, PlaneRef};
use nalgebra::{Point3, Unit, Vector3};

use crate::applied::EdgeOrFace;
use crate::document::{AxisRef, DirectionRef, EdgeRef, FaceRef, UpTo, VertexRef};
use crate::pattern::{InstanceDot, MirrorFeature, MirrorPlane, PatternFeature, PatternKind, PatternType};

/// One copy: its grid index, its instance number (1, 2, …; the names use it) and its motion.
#[derive(Debug, Clone)]
pub(super) struct Instance {
    pub(super) index: [u32; 2],
    number: u32,
    /// (P3I.5: read by the sheet-metal-aware face patterns.)
    pub(super) motion: Motion,
}

fn v3(p: [f64; 3]) -> Vector3<f64> {
    Vector3::new(p[0], p[1], p[2])
}

fn arr(v: Vector3<f64>) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The id of instance `k` of feature `f` of the pattern `pattern` (Reapply): stable, so the
/// instance's names are.
fn instance_id(pattern: FeatureId, f: FeatureId, k: u32) -> FeatureId {
    let mut bytes = pattern.0.as_bytes().to_vec();
    bytes.extend_from_slice(f.0.as_bytes());
    bytes.extend_from_slice(&k.to_le_bytes());
    let hi = naming::stable_hash(&bytes);
    bytes.push(0x5a);
    let lo = naming::stable_hash(&bytes);
    FeatureId(uuid::Uuid::from_u128((u128::from(hi) << 64) | u128::from(lo)))
}

/// The name a copy gives the face named `n`.
fn instance_face(op: cadrs_kernel::OpId, n: &FaceName, k: u32) -> FaceName {
    FaceName::new(op, FaceOrigin::Instance { of: n.op, face: naming::face_hash(n), instance: k })
}

// ---------------------------------------------------------------------------------------------
// Curve pattern paths

/// A piece of a curve pattern's path, in model space.
#[derive(Debug, Clone, Copy)]
enum Seg {
    Line(Vector3<f64>, Vector3<f64>),
    /// Round `n` from the point `c + r·from`, through `sweep` radians (signed about `n`).
    Arc { c: Vector3<f64>, n: Vector3<f64>, r: f64, from: Vector3<f64>, sweep: f64 },
}

impl Seg {
    fn len(&self) -> f64 {
        match *self {
            Seg::Line(a, b) => (b - a).norm(),
            Seg::Arc { r, sweep, .. } => r * sweep.abs(),
        }
    }

    fn at(&self, s: f64) -> (Vector3<f64>, Vector3<f64>) {
        match *self {
            Seg::Line(a, b) => {
                let d = (b - a).normalize();
                (a + d * s, d)
            }
            Seg::Arc { c, n, r, from, sweep } => {
                let t = if sweep >= 0.0 { s / r } else { -s / r };
                let side = n.cross(&from);
                let radial = from * t.cos() + side * t.sin();
                let tangent = (side * t.cos() - from * t.sin()) * sweep.signum();
                (c + radial * r, tangent)
            }
        }
    }

    fn start(&self) -> Vector3<f64> {
        self.at(0.0).0
    }

    fn end(&self) -> Vector3<f64> {
        self.at(self.len()).0
    }

    fn reversed(&self) -> Seg {
        match *self {
            Seg::Line(a, b) => Seg::Line(b, a),
            Seg::Arc { c, n, r, sweep, .. } => {
                let e = self.end();
                Seg::Arc { c, n, r, from: (e - c) / r, sweep: -sweep }
            }
        }
    }
}

/// The path's pieces, chained end to end, starting at the end nearest `start`.
fn chain_path(mut segs: Vec<Seg>, start: Vector3<f64>) -> Result<Vec<Seg>, String> {
    if segs.is_empty() {
        return Err("The path is empty".into());
    }
    let tol = 1e-4;
    // The free ends (touching no other piece); a closed path has none.
    let ends: Vec<(usize, bool)> = (0..segs.len())
        .flat_map(|i| [(i, false), (i, true)])
        .filter(|&(i, at_end)| {
            let p = if at_end { segs[i].end() } else { segs[i].start() };
            !segs.iter().enumerate().any(|(j, s)| j != i && ((s.start() - p).norm() < tol || (s.end() - p).norm() < tol))
        })
        .collect();
    let pick = if ends.is_empty() {
        (0..segs.len()).map(|i| (i, false)).collect::<Vec<_>>()
    } else {
        ends
    };
    let (first, at_end) = pick
        .into_iter()
        .min_by(|a, b| {
            let d = |(i, e): (usize, bool)| (if e { segs[i].end() } else { segs[i].start() } - start).norm();
            d(*a).total_cmp(&d(*b))
        })
        .expect("a piece");
    let s0 = segs.remove(first);
    let mut out = vec![if at_end { s0.reversed() } else { s0 }];
    while !segs.is_empty() {
        let tail = out.last().expect("one").end();
        let Some(i) = segs.iter().position(|s| (s.start() - tail).norm() < tol || (s.end() - tail).norm() < tol) else {
            return Err("The path's pieces must join end to end".into());
        };
        let s = segs.remove(i);
        out.push(if (s.start() - tail).norm() < tol { s } else { s.reversed() });
    }
    Ok(out)
}

/// A point and unit tangent `s` along a chained path.
fn path_at(path: &[Seg], s: f64) -> (Vector3<f64>, Vector3<f64>) {
    let mut left = s;
    for seg in path {
        let l = seg.len();
        if left <= l + 1e-9 {
            return seg.at(left.clamp(0.0, l));
        }
        left -= l;
    }
    let last = path.last().expect("a piece");
    last.at(last.len())
}

/// The normal of the plane a path lies in, if it lies in one.
fn path_plane(path: &[Seg]) -> Option<Vector3<f64>> {
    let mut pts: Vec<Vector3<f64>> = Vec::new();
    let total: f64 = path.iter().map(Seg::len).sum();
    for i in 0..=32 {
        pts.push(path_at(path, total * f64::from(i) / 32.0).0);
    }
    for seg in path {
        if let Seg::Arc { n, .. } = seg {
            let n = n.normalize();
            if pts.iter().all(|p| (p - pts[0]).dot(&n).abs() < 1e-6) {
                return Some(n);
            }
        }
    }
    let a = pts[0];
    let far = pts.iter().copied().max_by(|p, q| (p - a).norm().total_cmp(&(q - a).norm()))?;
    let d = far - a;
    let off = pts.iter().copied().max_by(|p, q| d.cross(&(p - a)).norm().total_cmp(&d.cross(&(q - a)).norm()))?;
    let n = d.cross(&(off - a));
    if n.norm() < 1e-9 {
        return None;
    }
    let n = n.normalize();
    pts.iter().all(|p| (p - a).dot(&n).abs() < 1e-6).then_some(n)
}

impl Rebuilder {
    /// The pieces of a curve pattern's path (sketch curves exactly; part edges by their exact
    /// circle, else their polyline).
    fn path_segments(&self, before: &[Feature], state: &State, path: &[crate::advanced::PathRef]) -> Result<Vec<Seg>, String> {
        use crate::advanced::PathRef;
        let lost = || "The path no longer exists".to_string();
        let mut out = Vec::new();
        let sketch_curve = |sketch: FeatureId, curve: cadrs_sketch::CurveId, out: &mut Vec<Seg>| -> Result<(), String> {
            let sk = before.iter().find(|f| f.id == sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
            let frame = sk.plane.ok_or_else(lost)?.frame();
            let g = &sk.geometry;
            let w = |p: cadrs_sketch::Vec2| v3(frame.to_world(p));
            match g.curves.get(curve).ok_or_else(lost)?.kind {
                CurveKind::Line { a, b } => out.push(Seg::Line(w(g.pos(a)), w(g.pos(b)))),
                CurveKind::Arc { .. } => {
                    let a = g.arc_geom(curve).ok_or_else(lost)?;
                    let from = v3(frame.u) * a.start_angle.cos() + v3(frame.v) * a.start_angle.sin();
                    out.push(Seg::Arc { c: w(a.center), n: v3(frame.normal()), r: a.radius, from, sweep: a.sweep });
                }
                CurveKind::Circle { center, radius } => {
                    out.push(Seg::Arc { c: w(g.pos(center)), n: v3(frame.normal()), r: radius, from: v3(frame.u), sweep: std::f64::consts::TAU })
                }
                CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
                    return Err("A curve pattern can't follow an ellipse yet".into());
                }
                CurveKind::Spline { .. } | CurveKind::Bezier { .. } => {
                    let pts = cadrs_sketch::hit::curve_polyline(g, curve);
                    for w2 in pts.windows(2) {
                        out.push(Seg::Line(w(w2[0]), w(w2[1])));
                    }
                }
            }
            Ok(())
        };
        for p in path {
            match p {
                PathRef::SketchCurve { sketch, curve } => sketch_curve(*sketch, *curve, &mut out)?,
                PathRef::Curve(f) => {
                    let pts = state.curves.get(f).ok_or_else(lost)?.polyline();
                    for w2 in pts.windows(2) {
                        out.push(Seg::Line(v3(w2[0]), v3(w2[1])));
                    }
                }
                PathRef::Sketch(s) => {
                    let sk = before.iter().find(|f| f.id == *s).and_then(|f| f.sketch()).ok_or_else(lost)?;
                    let ids: Vec<cadrs_sketch::CurveId> =
                        sk.geometry.curves.iter().filter(|(_, c)| !c.construction).map(|(id, _)| id).collect();
                    for c in ids {
                        sketch_curve(*s, c, &mut out)?;
                    }
                }
                PathRef::Edge(e) => {
                    let (part, _) = super::applied::edge_ids(state, e).ok_or_else(lost)?;
                    let edge = part.part.solid.edge(&e.edge).ok_or_else(lost)?;
                    let (a, b) = (v3(edge.points[0]), v3(*edge.points.last().ok_or_else(lost)?));
                    match edge.circle {
                        Some(c) => {
                            let (cc, n) = (v3(c.center), v3(c.normal).normalize());
                            let from = (a - cc).normalize();
                            let closed = (a - b).norm() < 1e-6;
                            let sweep = if closed {
                                std::f64::consts::TAU
                            } else {
                                let to = (b - cc).normalize();
                                // The way the polyline goes round.
                                let mid = v3(edge.midpoint()) - cc;
                                let ang = |v: Vector3<f64>| n.cross(&from).dot(&v).atan2(from.dot(&v)).rem_euclid(std::f64::consts::TAU);
                                let (am, ab) = (ang(mid), ang(to));
                                if am < ab { ab } else { ab - std::f64::consts::TAU }
                            };
                            out.push(Seg::Arc { c: cc, n, r: c.radius, from, sweep });
                        }
                        None => {
                            for w in edge.points.windows(2) {
                                out.push(Seg::Line(v3(w[0]), v3(w[1])));
                            }
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Every instance of a pattern but the seed (skipped ones too), with its motion.
    pub(super) fn pattern_instances(&self, before: &[Feature], x: &PatternFeature, state: &State, reference: Vector3<f64>) -> Result<Vec<Instance>, String> {
        let grid = x.grid();
        let n1 = x.first.count.max(1);
        let number = |i: [u32; 2]| i[1] * n1 + i[0] + 1;
        let mut out = Vec::new();
        match x.kind {
            PatternKind::Linear => {
                let dir = |d: &crate::pattern::LinearDirection| -> Result<Vector3<f64>, String> {
                    let r = d.direction.ok_or("Select a direction")?;
                    let u = self.direction(before, state, &r)?.into_inner();
                    Ok(if d.flip { -u } else { u } * d.distance)
                };
                let d1 = dir(&x.first)?;
                let d2 = if x.second_on { dir(&x.second)? } else { Vector3::zeros() };
                for (i, o) in grid.into_iter().skip(1) {
                    out.push(Instance { index: i, number: number(i), motion: Motion::translation(d1 * o[0] + d2 * o[1]) });
                }
            }
            PatternKind::Circular => {
                let axis = self.axis(before, state, &x.axis.ok_or("Select an axis of pattern")?)?;
                let step = x.angle_step().to_radians();
                for (i, o) in grid.into_iter().skip(1) {
                    out.push(Instance { index: i, number: number(i), motion: Motion::rotation(&axis, step * o[0]) });
                }
            }
            PatternKind::Curve => {
                let path = chain_path(self.path_segments(before, state, &x.path)?, reference)?;
                let total: f64 = path.iter().map(Seg::len).sum();
                let closed = (path[0].start() - path.last().expect("one").end()).norm() < 1e-6;
                let n = f64::from(x.first.count.max(1));
                let spacing = if x.equal_spacing {
                    if closed { total / n } else if n > 1.0 { total / (n - 1.0) } else { 0.0 }
                } else {
                    x.first.distance
                };
                let (p0, t0) = path_at(&path, 0.0);
                let normal = path_plane(&path);
                for (i, o) in grid.into_iter().skip(1) {
                    let s = spacing * o[0];
                    if s < -1e-9 || s > total + 1e-6 {
                        return Err("Instances run past the end of the path; reduce the count or the distance".into());
                    }
                    let (p, t) = path_at(&path, s);
                    let mut motion = Motion::translation(p - p0);
                    if x.tangent_to_curve {
                        let (axis_dir, angle) = match normal {
                            Some(nrm) => (nrm, nrm.dot(&t0.cross(&t)).atan2(t0.dot(&t))),
                            None => {
                                let c = t0.cross(&t);
                                if c.norm() < 1e-12 {
                                    (Vector3::z(), 0.0)
                                } else {
                                    (c.normalize(), c.norm().atan2(t0.dot(&t)))
                                }
                            }
                        };
                        let turn = Motion::rotation(&Axis { origin: Point3::from(p0), dir: Unit::new_normalize(axis_dir) }, angle);
                        motion = turn.then(&motion);
                    }
                    out.push(Instance { index: i, number: number(i), motion });
                }
            }
        }
        Ok(out)
    }

    /// The point a pattern's Skip dots are copies of: the seeds' centre.
    fn seed_reference(&self, state: &State, pattern_type: PatternType, parts: &[PartId], features: &[FeatureId], faces: &[FaceRef]) -> Vector3<f64> {
        let mut pts: Vec<Vector3<f64>> = Vec::new();
        match pattern_type {
            PatternType::Part => {
                for p in parts.iter().filter_map(|p| state.part(*p)) {
                    if let Some(m) = p.part.mass {
                        pts.push(m.center_of_mass.coords);
                    }
                }
            }
            PatternType::Feature => {
                for p in &state.parts {
                    for f in &p.part.solid.faces {
                        if features.iter().any(|x| x.0 == f.name.op)
                            && let Some(c) = f.center
                        {
                            pts.push(v3(c));
                        }
                    }
                }
            }
            PatternType::Face => {
                let parts: Vec<Part> = state.parts.iter().map(|p| p.part.clone()).collect();
                for f in faces {
                    if let Some((p, i)) = crate::mate::find_face(&parts, f)
                        && let Some(c) = p.solid.faces[i].center.or_else(|| p.solid.face_point(i))
                    {
                        pts.push(v3(c));
                    }
                }
            }
        }
        if pts.is_empty() {
            return Vector3::zeros();
        }
        pts.iter().sum::<Vector3<f64>>() / pts.len() as f64
    }

    /// A Linear, Circular or Curve pattern.
    pub(in crate::rebuild) fn pattern(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        x: &PatternFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let reference = self.seed_reference(state, x.pattern_type, &x.parts, &x.features, &x.faces);
        let all = self.pattern_instances(before, x, state, reference)?;
        let dots: Vec<InstanceDot> = all
            .iter()
            .map(|i| InstanceDot {
                index: i.index,
                at: arr(i.motion.point(&Point3::from(reference)).coords),
                skipped: x.is_skipped(i.index[0], i.index[1]),
            })
            .collect();
        let make: Vec<Instance> = all.into_iter().filter(|i| !x.is_skipped(i.index[0], i.index[1])).collect();
        let out = if make.is_empty() {
            // Every instance skipped (or a count of 1): nothing changes.
            Ok(Output {
                state: state.clone(),
                error: None,
                warning: None,
                contacts: None,
                owned: Vec::new(),
                stage: None,
                axis: None,
                arrows: Vec::new(),
                dots: None,
                uses: Vec::new(),
            })
        } else {
            let merge = Merge { op: x.op, merge_all: x.merge_all, scope: &x.merge_scope, surface: false };
            match x.pattern_type {
                PatternType::Part => self.copy_parts(id, &x.parts, &make, &merge, state),
                PatternType::Feature if x.reapply => self.reapply(before, id, &x.features, &make, state),
                PatternType::Feature => self.copy_feature_effect(before, id, &x.features, &make, state),
                PatternType::Face => self.copy_faces(id, &x.faces, &make, state),
            }
        };
        out.map(|mut o| {
            o.dots = Some(dots);
            o
        })
    }

    /// A mirror.
    pub(in crate::rebuild) fn mirror(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        x: &MirrorFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let lost = || "The mirror plane no longer exists".to_string();
        let frame: PlaneFrame = match x.plane.ok_or("Select a mirror plane")? {
            MirrorPlane::Plane(p) => super::advanced::plane_of(state, &p).ok_or_else(lost)?,
            MirrorPlane::Face(f) => {
                let (part, _) = face_ids(state, &f).ok_or_else(lost)?;
                face_frame(part, &f).ok_or("The mirror plane must be a flat face")?
            }
            MirrorPlane::Connector(c) => super::super::connector_frame(before, state, &c)?,
        };
        let motion = Motion::reflection(Point3::from(frame.origin), Unit::new_normalize(v3(frame.normal())));
        let make = vec![Instance { index: [1, 0], number: 1, motion }];
        let merge = Merge { op: x.op, merge_all: x.merge_all, scope: &x.merge_scope, surface: false };
        match x.mirror_type {
            PatternType::Part => self.copy_parts(id, &x.parts, &make, &merge, state),
            PatternType::Feature => self.copy_feature_effect(before, id, &x.features, &make, state),
            PatternType::Face => self.copy_faces(id, &x.faces, &make, state),
        }
    }

    // -----------------------------------------------------------------------------------------
    // Copies

    /// Names for a moved copy of a body named `source` (by the copy's history): every face an
    /// instance of its original.
    pub(super) fn instance_names(&self, body: BodyId, history: &cadrs_kernel::History, source: &BodyNames, op: cadrs_kernel::OpId, k: u32) -> Result<BodyNames, String> {
        let n = self.kernel.faces(body).map_err(|e| e.to_string())?.len();
        let mut faces: Vec<Option<FaceName>> = vec![None; n];
        for (f, input) in &history.modified {
            if let (Some(slot), Some(sn)) = (faces.get_mut(f.0 as usize), source.face(input.face)) {
                *slot = Some(instance_face(op, &sn, k));
            }
        }
        let faces: Vec<FaceName> = faces
            .into_iter()
            .enumerate()
            .map(|(i, f)| f.unwrap_or_else(|| FaceName::new(op, FaceOrigin::Unnamed { index: i as u32 })))
            .collect();
        self.names_from_faces(body, faces)
    }

    /// Edge and vertex names for a body whose faces are named `faces`.
    pub(super) fn names_from_faces(&self, body: BodyId, faces: Vec<FaceName>) -> Result<BodyNames, String> {
        let edges = self.kernel.edges(body).map_err(|e| e.to_string())?;
        let vertices = self.kernel.vertices(body).map_err(|e| e.to_string())?;
        Ok(BodyNames {
            edges: naming::name_edges(&faces, &edges, naming::joint_tolerance(&edges)),
            vertices: naming::name_vertices(&faces, &edges, &vertices),
            faces,
            aliases: Vec::new(),
        })
    }

    /// `body` (named `names`) moved by each instance's motion, the copies renamed.
    fn copies_of(&mut self, id: FeatureId, body: BodyId, names: &BodyNames, make: &[Instance]) -> Result<Vec<(BodyId, BodyNames)>, String> {
        let mut out: Vec<(BodyId, BodyNames)> = Vec::new();
        for inst in make {
            let copy = self.kernel.transform_motion(body, &inst.motion).map(|r| (r.bodies[0], r.history));
            match copy {
                Ok((b, h)) => match self.instance_names(b, &h, names, id.0, inst.number) {
                    Ok(n) => out.push((b, n)),
                    Err(e) => {
                        self.kernel.release(b);
                        self.release_all(&out);
                        return Err(e);
                    }
                },
                Err(e) => {
                    self.release_all(&out);
                    return Err(format!("Copy failed: {e}"));
                }
            }
        }
        Ok(out)
    }

    fn release_all(&mut self, bodies: &[(BodyId, BodyNames)]) {
        for (b, _) in bodies {
            self.kernel.release(*b);
        }
    }

    /// Several copies as one body (their union; they may lie apart). The copies are released.
    fn join(&mut self, id: FeatureId, mut copies: Vec<(BodyId, BodyNames)>) -> Result<(BodyId, BodyNames), String> {
        if copies.len() == 1 {
            return Ok(copies.pop().expect("one"));
        }
        let first = copies[0].0;
        let rest: Vec<BodyId> = copies[1..].iter().map(|(b, _)| *b).collect();
        let joined = self.kernel.boolean(BoolOp::Union, first, &rest).and_then(|r| {
            let body = r.bodies[0];
            let inputs: Vec<(BodyId, &BodyNames)> = copies.iter().map(|(b, n)| (*b, n)).collect();
            Ok((body, naming::name_body(&self.kernel, body, id.0, &r.history, &inputs)?))
        });
        self.release_all(&copies);
        joined.map_err(|e| format!("Joining the copies failed: {e}"))
    }

    /// Part pattern or mirror: copies of the parts, New or combined with the merge scope.
    fn copy_parts(&mut self, id: FeatureId, seeds: &[PartId], make: &[Instance], merge: &Merge, state: &Arc<State>) -> Result<Output, String> {
        let op = id.0;
        let mut copies: Vec<(BodyId, BodyNames, PartId, u32)> = Vec::new();
        for seed in seeds {
            let Some(p) = state.part(*seed) else {
                for (b, ..) in &copies {
                    self.kernel.release(*b);
                }
                return Err("A selected part no longer exists".into());
            };
            let (Some(body), names, palette) = (p.body, p.names.clone(), p.part.palette) else { continue };
            match self.copies_of(id, body, &names, make) {
                Ok(c) => copies.extend(c.into_iter().map(|(b, n)| (b, n, *seed, palette))),
                Err(e) => {
                    for (b, ..) in &copies {
                        self.kernel.release(*b);
                    }
                    return Err(e);
                }
            }
        }
        if copies.is_empty() {
            return Err("There is nothing to copy".into());
        }
        if merge.op == BooleanOp::New {
            let mut next = (**state).clone();
            let mut placed: Vec<Placed> = Vec::new();
            let mut sources: Vec<(PartId, PartId, u32)> = Vec::new();
            for (b, names, seed, palette) in copies {
                let pieces = self.split_named(b, op, &names, &HashMap::new());
                self.kernel.release(b);
                for piece in pieces? {
                    let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                    let pid = Self::new_id(id, &next, &taken);
                    sources.push((pid, seed, palette));
                    placed.push((pid, piece));
                }
            }
            next.geoms = state.geoms.clone();
            let mut out = self.finish(id, placed, next, op, state.geoms.clone(), PartKind::Solid)?;
            // PS9.6: a copy looks like its seed.
            if let Some(st) = Arc::get_mut(&mut out.state) {
                for (pid, seed, palette) in sources {
                    if let Some(p) = st.parts.iter_mut().find(|q| q.part.id == pid) {
                        p.part.source = Some(seed);
                        p.part.palette = palette;
                    }
                }
            }
            return Ok(out);
        }
        let joined = self.join(id, copies.into_iter().map(|(b, n, ..)| (b, n)).collect())?;
        self.combine(id, merge, joined, state, state.geoms.clone())
    }

    /// Face pattern or mirror: the pocket or boss each part's faces bound, copied and cut from
    /// or added to the part.
    fn copy_faces(&mut self, id: FeatureId, faces: &[FaceRef], make: &[Instance], state: &Arc<State>) -> Result<Output, String> {
        let op = id.0;
        // The faces by part.
        let mut groups: Vec<(PartId, BodyId, Arc<BodyNames>, Vec<FaceId>)> = Vec::new();
        for f in faces {
            let (part, ids) = face_ids(state, f).ok_or("A selected face no longer exists")?;
            let body = part.body.ok_or("A part has no body")?;
            match groups.iter_mut().find(|g| g.0 == part.part.id) {
                Some(g) => {
                    for i in ids {
                        if !g.3.contains(&i) {
                            g.3.push(i);
                        }
                    }
                }
                None => groups.push((part.part.id, body, part.names.clone(), ids)),
            }
        }
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        for (pid, body, names, ids) in groups {
            let source = naming::stable_hash(format!("{op} face tool {pid:?}").as_bytes());
            let r = self.kernel.face_tool(body, &ids, source).map_err(|e| format!("The faces can't be copied: {e}"))?;
            let tool = r.bodies[0];
            let named = naming::name_body(&self.kernel, tool, op, &r.history, &[(body, &names)]);
            let class = self
                .kernel
                .mass_properties(tool)
                .and_then(|m| self.kernel.classify(body, m.center_of_mass, 1e-6));
            let (tool_names, class) = match (named, class) {
                (Ok(n), Ok(c)) => (n, c),
                (Err(e), _) | (_, Err(e)) => {
                    self.kernel.release(tool);
                    return Err(format!("The faces can't be copied: {e}"));
                }
            };
            let copies = self.copies_of(id, tool, &tool_names, make);
            self.kernel.release(tool);
            let joined = self.join(id, copies?)?;
            let result = match class {
                PointClass::Outside => self.cut(id, pid, BoolOp::Subtract, &[&joined], &mut next),
                PointClass::Inside => self.add(id, &joined, &[pid], &mut next),
                PointClass::OnBoundary => Err("The faces don't bound a pocket or a boss".into()),
            };
            self.kernel.release(joined.0);
            placed.extend(result?);
        }
        let geoms = state.geoms.clone();
        self.finish(id, placed, next, op, geoms, PartKind::Solid)
    }

    /// Feature pattern without Reapply, or Feature mirror: the material the features removed
    /// and added, copied.
    fn copy_feature_effect(&mut self, before: &[Feature], id: FeatureId, features: &[FeatureId], make: &[Instance], state: &Arc<State>) -> Result<Output, String> {
        let op = id.0;
        let first = before
            .iter()
            .position(|f| features.contains(&f.id))
            .ok_or("The features to copy no longer exist")?;
        let earlier: HashSet<FeatureId> = before[..first].iter().map(|f| f.id).collect();
        let prior: Arc<State> = self
            .trail
            .iter()
            .rev()
            .find(|(f, _)| earlier.contains(f))
            .map(|(_, s)| s.clone())
            .unwrap_or_default();
        let mut next = (**state).clone();
        let mut changed: Vec<PartId> = Vec::new();
        // Bodies made here that a later step replaced.
        let mut spare: Vec<BodyId> = Vec::new();
        let result = (|| -> Result<(), String> {
            let parts: Vec<PartState> = state.parts.iter().filter(|p| p.part.kind == PartKind::Solid).cloned().collect();
            for after in &parts {
                let Some(after_body) = after.body else { continue };
                let prior_part = prior.part(after.part.id);
                if prior_part.is_some_and(|p| p.body == Some(after_body)) {
                    continue;
                }
                let Some(prior_body) = prior_part.and_then(|p| p.body) else {
                    // A part the features made: its copies are new parts.
                    let copies = self.copies_of(id, after_body, &after.names, make)?;
                    for (b, names) in copies {
                        let pieces = self.split_named(b, op, &names, &HashMap::new());
                        self.kernel.release(b);
                        for piece in pieces? {
                            let pid = Self::new_id(id, &next, &[]);
                            next.next_part += 1;
                            let mut part = after.part.clone();
                            part.id = pid;
                            part.feature = id;
                            part.name = format!("Part {}", next.next_part);
                            part.features = vec![id];
                            part.source = Some(after.part.id);
                            next.parts.push(PartState { part, body: Some(piece.body), names: Arc::new(piece.names) });
                            changed.push(pid);
                        }
                    }
                    continue;
                };
                let prior_names = prior_part.map(|p| p.names.clone()).unwrap_or_default();
                let inputs: Vec<(BodyId, &BodyNames)> = vec![(prior_body, &prior_names), (after_body, &after.names)];
                let difference = |this: &mut Self, a: BodyId, b: BodyId| -> Result<Option<(BodyId, BodyNames)>, String> {
                    match this.kernel.boolean(BoolOp::Subtract, a, &[b]) {
                        Ok(r) => {
                            let body = r.bodies[0];
                            let v = this.kernel.mass_properties(body).map(|m| m.volume).unwrap_or(0.0);
                            if v < 1e-9 {
                                this.kernel.release(body);
                                return Ok(None);
                            }
                            let names = naming::name_body(&this.kernel, body, op, &r.history, &inputs).map_err(|e| e.to_string())?;
                            Ok(Some((body, names)))
                        }
                        Err(cadrs_kernel::KernelError::OperationFailed(m)) if m.contains("empty") => Ok(None),
                        Err(e) => Err(format!("Copying the features failed: {e}")),
                    }
                };
                let removed = difference(self, prior_body, after_body)?;
                let added = difference(self, after_body, prior_body)?;
                let pid = after.part.id;
                if let Some((b, names)) = removed {
                    let copies = self.copies_of(id, b, &names, make);
                    self.kernel.release(b);
                    let joined = self.join(id, copies?)?;
                    let placed = self.cut(id, pid, BoolOp::Subtract, &[&joined], &mut next);
                    self.kernel.release(joined.0);
                    self.place_now(&mut next, placed?, &mut changed, &mut spare);
                }
                if let Some((b, names)) = added {
                    let copies = self.copies_of(id, b, &names, make);
                    self.kernel.release(b);
                    let joined = self.join(id, copies?)?;
                    let placed = self.add(id, &joined, &[pid], &mut next);
                    self.kernel.release(joined.0);
                    self.place_now(&mut next, placed?, &mut changed, &mut spare);
                }
            }
            Ok(())
        })();
        for b in spare {
            self.kernel.release(b);
        }
        if let Err(e) = result {
            for pid in &changed {
                if let Some(b) = next.part(*pid).and_then(|p| p.body) {
                    self.kernel.release(b);
                }
            }
            return Err(e);
        }
        // Mesh the parts that changed.
        let placed: Vec<Placed> = changed
            .iter()
            .filter_map(|pid| {
                let p = next.part(*pid)?;
                Some((*pid, Piece { body: p.body?, names: (*p.names).clone(), from: HashSet::new(), volume: 0.0 }))
            })
            .collect();
        let geoms = state.geoms.clone();
        self.finish(id, placed, next, op, geoms, PartKind::Solid)
    }

    /// Puts pieces a cut or join made into `next` right away (the next step works on them):
    /// the body a piece replaces goes to `spare` if this feature made it.
    fn place_now(&mut self, next: &mut State, placed: Vec<Placed>, changed: &mut Vec<PartId>, spare: &mut Vec<BodyId>) {
        for (pid, piece) in placed {
            match next.parts.iter_mut().find(|p| p.part.id == pid) {
                Some(p) => {
                    if let Some(old) = p.body
                        && changed.contains(&pid)
                    {
                        spare.push(old);
                    }
                    p.body = Some(piece.body);
                    p.names = Arc::new(piece.names);
                }
                None => {
                    let Some(template) = next.parts.first().map(|p| p.part.clone()) else {
                        spare.push(piece.body);
                        continue;
                    };
                    next.next_part += 1;
                    let mut part = template;
                    part.id = pid;
                    part.feature = pid.feature;
                    part.name = format!("Part {}", next.next_part);
                    part.features = vec![pid.feature];
                    part.source = None;
                    next.parts.push(PartState { part, body: Some(piece.body), names: Arc::new(piece.names) });
                }
            }
            if !changed.contains(&pid) {
                changed.push(pid);
            }
        }
    }

    // -----------------------------------------------------------------------------------------
    // Reapply

    /// Feature pattern with Reapply features (PS22.3): each instance rebuilds the features with
    /// their sketches moved.
    fn reapply(&mut self, before: &[Feature], id: FeatureId, seeds: &[FeatureId], make: &[Instance], state: &Arc<State>) -> Result<Output, String> {
        let feats: Vec<Feature> = before.iter().filter(|f| seeds.contains(&f.id)).cloned().collect();
        if feats.len() != seeds.len() {
            return Err("A feature to pattern no longer exists".into());
        }
        if make.iter().any(|i| i.motion.is_reflection()) {
            return Err("Reapply features can't mirror".into());
        }
        // The sketches the features use (moved with each instance).
        let mut sketches: HashSet<FeatureId> = HashSet::new();
        for f in &feats {
            sketches.extend(f.input_sketches());
            if let FeatureKind::Revolve(r) = &f.kind
                && let Some(AxisRef::SketchCurve { sketch, .. }) = r.axis
            {
                sketches.insert(sketch);
            }
            if let FeatureKind::Extrude(e) = &f.kind
                && let Some(DirectionRef::SketchLine { sketch, .. }) = e.direction
            {
                sketches.insert(sketch);
            }
        }
        let mut cur = state.clone();
        let mut made: Vec<BodyId> = Vec::new();
        let mut synthetic: HashMap<uuid::Uuid, (uuid::Uuid, u32)> = HashMap::new();
        let mut failure: Option<String> = None;
        'instances: for inst in make {
            let ops: HashMap<uuid::Uuid, uuid::Uuid> =
                feats.iter().map(|f| (f.id.0, instance_id(id, f.id, inst.number).0)).collect();
            let moved: Vec<Feature> = before
                .iter()
                .map(|f| if sketches.contains(&f.id) { moved_sketch(f, &inst.motion) } else { f.clone() })
                .collect();
            for f in &feats {
                let fk = match remap_feature(f, &ops, &inst.motion) {
                    Ok(x) => x,
                    Err(e) => {
                        failure = Some(e);
                        break 'instances;
                    }
                };
                let out = self.compute(&moved, &fk, &cur);
                made.extend(out.owned.iter().copied());
                if let Some(e) = out.error {
                    failure = Some(format!("Instance {} of {}: {e}", crate::pattern::index_label(inst.index), f.name));
                    break 'instances;
                }
                synthetic.insert(fk.id.0, (f.id.0, inst.number));
                cur = out.state;
            }
        }
        let held: HashSet<BodyId> = state.parts.iter().filter_map(|p| p.body).collect();
        if let Some(e) = failure {
            for b in made {
                if !held.contains(&b) {
                    self.kernel.release(b);
                }
            }
            return Err(e);
        }
        // Rename the instances' faces under the pattern and mesh the parts again.
        let mut next = (*cur).clone();
        let op = id.0;
        for p in next.parts.iter_mut() {
            if !p.names.faces.iter().any(|n| synthetic.contains_key(&n.op)) {
                continue;
            }
            let Some(body) = p.body else { continue };
            let faces: Vec<FaceName> = p
                .names
                .faces
                .iter()
                .map(|n| match synthetic.get(&n.op) {
                    Some((seed, k)) => instance_face(op, &FaceName { op: *seed, ..*n }, *k),
                    None => *n,
                })
                .collect();
            let names = self.names_from_faces(body, faces)?;
            let solid = crate::brep::solid_of(&self.kernel, body, &names, &cur.geoms, Some(op))?;
            p.names = Arc::new(names);
            p.part.solid = Arc::new(solid);
            p.part.features.retain(|f| !synthetic.contains_key(&f.0));
            if !p.part.features.contains(&id) {
                p.part.features.push(id);
            }
        }
        let now: HashSet<BodyId> = next.parts.iter().filter_map(|p| p.body).collect();
        let mut owned = Vec::new();
        for b in made {
            if now.contains(&b) {
                owned.push(b);
            } else if !held.contains(&b) {
                self.kernel.release(b);
            }
        }
        Ok(Output {
            state: Arc::new(next),
            error: None,
            warning: None,
            contacts: None,
            owned,
            stage: None,
            axis: None,
            arrows: Vec::new(),
            dots: None,
            uses: Vec::new(),
        })
    }
}

/// A sketch moved by `m` (its plane's frame; its geometry stays in sketch coordinates).
fn moved_sketch(f: &Feature, m: &Motion) -> Feature {
    let mut g = f.clone();
    if let FeatureKind::Sketch(s) = &mut g.kind
        && let Some(plane) = s.plane
    {
        let fr = plane.frame();
        let o = m.point(&Point3::from(fr.origin));
        let (u, v) = (m.vector(&v3(fr.u)), m.vector(&v3(fr.v)));
        s.plane = Some(PlaneRef::Feature(FeaturePlane { feature: f.id.0, origin: arr(o.coords), u: arr(u), v: arr(v) }));
    }
    g
}

/// A feature of a Reapply pattern's instance: its own id, and its references to faces, edges
/// and vertices of the patterned features mapped to the instance's (`ops`: seed → instance)
/// with their stored points moved. References to anything else stay.
fn remap_feature(f: &Feature, ops: &HashMap<uuid::Uuid, uuid::Uuid>, m: &Motion) -> Result<Feature, String> {
    let face_name = |n: FaceName| match ops.get(&n.op) {
        Some(o) => FaceName { op: *o, ..n },
        None => n,
    };
    let moves = |n: &FaceName| ops.contains_key(&n.op);
    let pt = |p: [f64; 3]| arr(m.point(&Point3::from(p)).coords);
    let face = |r: FaceRef| {
        if moves(&r.face) {
            FaceRef { face: face_name(r.face), seed: pt(r.seed), ..r }
        } else {
            r
        }
    };
    let edge = |r: EdgeRef| {
        if r.edge.faces.iter().any(&moves) {
            let e = cadrs_sketch::EdgeName::new(face_name(r.edge.faces[0]), face_name(r.edge.faces[1]), r.edge.index);
            EdgeRef { edge: e, seed: pt(r.seed), ..r }
        } else {
            r
        }
    };
    let vertex = |r: VertexRef| {
        if r.vertex.faces.iter().any(&moves) {
            let mut faces = r.vertex.faces.map(face_name);
            faces.sort();
            VertexRef { vertex: cadrs_sketch::VertexName { faces, index: r.vertex.index }, point: pt(r.point), ..r }
        } else {
            r
        }
    };
    let up_to = |u: UpTo| match u {
        UpTo::Face(r) => UpTo::Face(face(r)),
        UpTo::Vertex(r) => UpTo::Vertex(vertex(r)),
        p => p,
    };
    let direction = |d: DirectionRef| match d {
        DirectionRef::Edge(r) => DirectionRef::Edge(edge(r)),
        DirectionRef::FaceNormal(r) => DirectionRef::FaceNormal(face(r)),
        d => d,
    };
    let entity = |e: EdgeOrFace| match e {
        EdgeOrFace::Edge(r) => EdgeOrFace::Edge(edge(r)),
        EdgeOrFace::Face(r) => EdgeOrFace::Face(face(r)),
    };
    let id = FeatureId(*ops.get(&f.id.0).ok_or("not a patterned feature")?);
    let kind = match &f.kind {
        FeatureKind::Extrude(e) => {
            let mut e = e.clone();
            e.faces = e.faces.into_iter().map(face).collect();
            e.up_to = e.up_to.map(up_to);
            e.direction = e.direction.map(direction);
            if let Some(s) = &mut e.second {
                s.up_to = s.up_to.map(up_to);
            }
            FeatureKind::Extrude(e)
        }
        FeatureKind::Revolve(r) => {
            let mut r = r.clone();
            r.faces = r.faces.into_iter().map(face).collect();
            r.up_to = r.up_to.map(up_to);
            r.axis = r.axis.map(|a| match a {
                AxisRef::Edge(x) => AxisRef::Edge(edge(x)),
                AxisRef::Face(x) => AxisRef::Face(face(x)),
                a => a,
            });
            if let Some(s) = &mut r.second {
                s.up_to = s.up_to.map(up_to);
            }
            FeatureKind::Revolve(r)
        }
        FeatureKind::Fillet(x) => {
            let mut x = x.clone();
            x.entities = x.entities.into_iter().map(entity).collect();
            for list in [&mut x.side1, &mut x.center, &mut x.side2] {
                *list = list.iter().copied().map(face).collect();
            }
            FeatureKind::Fillet(x)
        }
        FeatureKind::Chamfer(x) => {
            let mut x = x.clone();
            x.entities = x.entities.into_iter().map(entity).collect();
            x.overrides = x.overrides.into_iter().map(edge).collect();
            FeatureKind::Chamfer(x)
        }
        FeatureKind::Hole(x) => {
            let mut x = x.clone();
            x.connectors = x
                .connectors
                .into_iter()
                .map(|c| match c {
                    crate::mate::ConnectorRef::Implicit(crate::mate::ConnectorOrigin::Face(r)) => {
                        crate::mate::ConnectorRef::Implicit(crate::mate::ConnectorOrigin::Face(face(r)))
                    }
                    crate::mate::ConnectorRef::Implicit(crate::mate::ConnectorOrigin::Edge(r)) => {
                        crate::mate::ConnectorRef::Implicit(crate::mate::ConnectorOrigin::Edge(edge(r)))
                    }
                    c => c,
                })
                .collect();
            FeatureKind::Hole(x)
        }
        FeatureKind::Sketch(_) => f.kind.clone(),
        _ => return Err(format!("{} can't be reapplied; turn Reapply features off", f.name)),
    };
    Ok(Feature { id, name: f.name.clone(), kind })
}

//! Rebuilding the applied features (P3.6): Fillet, Chamfer, Shell and Hole. A child of
//! `rebuild::kernel_ops`, so it shares its helpers (face lookup, splitting, finishing).

use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{
    Axis, BoolOp, ChamferMeasure, ChamferOpts, ChamferSpec, Curve2, EdgeId, FaceId, FilletProfile, FilletSize, FilletSpec, Kernel, Loop,
    OpResult, Plane, Profile, Region, RevolveSpec, ShellSpec,
};
use nalgebra::{Point2, Point3, Unit, Vector3};

use crate::applied::{
    ChamferFeature, ChamferMeasurement, ChamferType, EdgeOrFace, FilletControl, FilletFeature, FilletMeasurement,
    FilletType, HoleFeature, ShellFeature,
};
use crate::document::EdgeRef;
use crate::hole::{HoleEnd, HoleStart};

/// Tool above a hole's start, so the cut leaves no skin at the part's surface (mm).
const HOLE_ABOVE: f64 = 0.5;

/// Edges picked on one part, as kernel ids of its body.
struct PartEdges {
    part: PartId,
    body: BodyId,
    edges: Vec<EdgeId>,
}

/// The part an edge reference is on and the edge's kernel ids (by its current name, else the
/// edge nearest the stored point).
pub(super) fn edge_ids<'a>(state: &'a State, r: &EdgeRef) -> Option<(&'a PartState, Vec<EdgeId>)> {
    let candidates: Vec<&PartState> = state
        .part(r.part)
        .into_iter()
        .chain(state.parts.iter().filter(|p| p.part.id != r.part))
        .collect();
    // By name on any part first (an edge can move to another part), then where it was.
    for by_name in [true, false] {
        for part in &candidates {
            let solid = &part.part.solid;
            let distance = |e: &crate::solid::SolidEdge| -> Option<f64> { Some(e.distance(r.seed)) };
            let Ok((i, how)) = solid.resolve_edge(&r.edge, distance) else {
                continue;
            };
            if by_name && how == cadrs_kernel::naming::Match::Geometric {
                continue;
            }
            let ids = part.names.edges_named(&solid.edges[i].name);
            if !ids.is_empty() {
                return Some((part, ids));
            }
        }
    }
    None
}

impl Rebuilder {
    /// The picked edges and faces (a face: all its edges) as kernel edges, grouped by part.
    fn entity_edges(&self, state: &State, entities: &[EdgeOrFace]) -> Result<Vec<PartEdges>, String> {
        let mut out: Vec<PartEdges> = Vec::new();
        let push = |part: &PartState, ids: Vec<EdgeId>, out: &mut Vec<PartEdges>| -> Result<(), String> {
            let body = part.body.ok_or("A part has no body")?;
            let i = match out.iter().position(|g| g.part == part.part.id) {
                Some(i) => i,
                None => {
                    out.push(PartEdges { part: part.part.id, body, edges: Vec::new() });
                    out.len() - 1
                }
            };
            for e in ids {
                if !out[i].edges.contains(&e) {
                    out[i].edges.push(e);
                }
            }
            Ok(())
        };
        let mut lost = 0;
        for ent in entities {
            match ent {
                EdgeOrFace::Edge(r) => match edge_ids(state, r) {
                    Some((part, ids)) => push(part, ids, &mut out)?,
                    None => lost += 1,
                },
                EdgeOrFace::Face(f) => match face_ids(state, f) {
                    Some((part, faces)) => {
                        let body = part.body.ok_or("A part has no body")?;
                        let infos = self.kernel.edges(body).map_err(|e| e.to_string())?;
                        let ids: Vec<EdgeId> = infos
                            .iter()
                            .filter(|e| e.faces.iter().flatten().any(|x| faces.contains(x)))
                            // A seam of a curved face is not an edge to round.
                            .filter(|e| e.faces[0] != e.faces[1])
                            .map(|e| e.id)
                            .collect();
                        push(part, ids, &mut out)?;
                    }
                    None => lost += 1,
                },
            }
        }
        if lost > 0 && out.is_empty() {
            return Err("The selected edges no longer exist".into());
        }
        if lost > 0 {
            return Err(if lost == 1 {
                "1 selected edge or face no longer exists".into()
            } else {
                format!("{lost} selected edges or faces no longer exist")
            });
        }
        Ok(out)
    }

    /// With Tangent propagation off, only the picked edges may be rounded; OpenCascade always
    /// continues a fillet or chamfer along the edges tangent to them, so a pick with an unpicked
    /// tangent neighbour is an error rather than a silently larger result.
    fn check_no_propagation(&self, groups: &[PartEdges], what: &str) -> Result<(), String> {
        let mut extra = 0;
        for g in groups {
            let mut seen: Vec<EdgeId> = Vec::new();
            for &e in &g.edges {
                for c in self.kernel.tangent_chain(g.body, e).map_err(|e| e.to_string())? {
                    if !g.edges.contains(&c) && !seen.contains(&c) {
                        seen.push(c);
                    }
                }
            }
            extra += seen.len();
        }
        if extra == 0 {
            return Ok(());
        }
        let edges = if extra == 1 { "1 tangent edge".to_string() } else { format!("{extra} tangent edges") };
        Err(format!(
            "Without tangent propagation the {what} would stop part-way along a smooth edge chain \
             ({edges} not picked), which OpenCascade cannot do; turn on Tangent propagation or pick the whole chain"
        ))
    }

    /// Runs `op` on each part's body and puts the results in place of the parts (the largest
    /// piece keeps the part's id).
    pub(in crate::rebuild) fn per_part(
        &mut self,
        id: FeatureId,
        what: &str,
        state: &Arc<State>,
        groups: Vec<(PartId, BodyId)>,
        op: impl FnMut(&mut Self, usize, BodyId) -> cadrs_kernel::Result<OpResult>,
    ) -> Result<Output, String> {
        self.per_part_keeping(id, what, state, groups, op, |_, _| true)
    }

    /// [`Self::per_part`], keeping only the pieces `keep` accepts (a Split that keeps one side);
    /// the largest kept piece keeps the part.
    pub(in crate::rebuild) fn per_part_keeping(
        &mut self,
        id: FeatureId,
        what: &str,
        state: &Arc<State>,
        groups: Vec<(PartId, BodyId)>,
        mut op: impl FnMut(&mut Self, usize, BodyId) -> cadrs_kernel::Result<OpResult>,
        mut keep: impl FnMut(&mut Self, BodyId) -> bool,
    ) -> Result<Output, String> {
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        let release = |this: &mut Self, placed: Vec<Placed>| {
            for (_, p) in placed {
                this.kernel.release(p.body);
            }
        };
        for (i, (pid, body)) in groups.into_iter().enumerate() {
            let names = state.part(pid).map(|p| p.names.clone()).unwrap_or_default();
            let r = match op(self, i, body) {
                Ok(r) => r,
                Err(e) => {
                    release(self, placed);
                    return Err(format!("{what} failed: {e}"));
                }
            };
            let pieces = match self.split_result(r, id.0, &[(body, &names)]) {
                Ok(p) => p,
                Err(e) => {
                    release(self, placed);
                    return Err(e);
                }
            };
            let mut pieces = pieces;
            let (kept, dropped): (Vec<Piece>, Vec<Piece>) = pieces.into_iter().partition(|p| keep(self, p.body));
            for p in dropped {
                self.kernel.release(p.body);
            }
            if kept.is_empty() {
                release(self, placed);
                return Err(format!("{what} failed: nothing is left on the kept side"));
            }
            pieces = kept;
            pieces.sort_by(|a, b| b.volume.total_cmp(&a.volume));
            for (k, piece) in pieces.into_iter().enumerate() {
                let pid = if k == 0 {
                    pid
                } else {
                    let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                    Self::new_id(id, &next, &taken)
                };
                placed.push((pid, piece));
            }
        }
        let geoms = state.geoms.clone();
        next.geoms = geoms.clone();
        self.finish(id, placed, next, id.0, geoms, PartKind::Solid)
    }

    /// The features of the faces along the edges' tangent chains (P3.11, PS11.2): a fillet
    /// picked on the plate's rim whose chain runs round another fillet's faces depends on that
    /// fillet too, though its references name only the plate's faces.
    fn chain_uses(&self, id: FeatureId, state: &State, groups: &[PartEdges]) -> Vec<FeatureId> {
        let mut out: Vec<FeatureId> = Vec::new();
        for g in groups {
            let Some(part) = state.part(g.part) else { continue };
            for &e in &g.edges {
                let chain = self.kernel.tangent_chain(g.body, e).unwrap_or_else(|_| vec![e]);
                for c in chain {
                    let Some(name) = part.names.edge(c) else { continue };
                    for f in name.faces {
                        let f = FeatureId(f.op);
                        if f != id && !out.contains(&f) {
                            out.push(f);
                        }
                    }
                }
            }
        }
        out
    }

    /// The Fillet feature (PS14.1–14.5).
    pub(in crate::rebuild) fn fillet(&mut self, id: FeatureId, x: &FilletFeature, state: &Arc<State>) -> Result<Output, String> {
        let uses = if x.tangent_propagation && x.kind != FilletType::FullRound {
            self.entity_edges(state, &x.entities).map(|g| self.chain_uses(id, state, &g)).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.fillet_inner(id, x, state).map(|mut o| {
            o.uses = uses;
            o
        })
    }

    fn fillet_inner(&mut self, id: FeatureId, x: &FilletFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        if x.kind == FilletType::FullRound {
            return self.full_round(id, x, state);
        }
        let groups = self.entity_edges(state, &x.entities)?;
        if !x.tangent_propagation && !x.partial {
            self.check_no_propagation(&groups, "fillet")?;
        }
        if x.variable {
            return self.variable_fillet(id, x, groups, state);
        }
        let profile = match x.control {
            FilletControl::Distance if x.asymmetric => FilletProfile::Asymmetric { second: x.second, flip: x.flip_asymmetric },
            FilletControl::Distance => FilletProfile::Circular,
            _ if x.asymmetric => return Err("An asymmetric fillet takes the Distance control".into()),
            FilletControl::Conic => FilletProfile::Conic { rho: x.rho },
            FilletControl::Curvature => FilletProfile::Curvature { magnitude: x.magnitude },
        };
        let spec = FilletSpec {
            size: match x.measurement {
                FilletMeasurement::Radius => FilletSize::Radius(x.size),
                FilletMeasurement::Width => FilletSize::Width(x.size),
            },
            allow_overflow: x.allow_overflow,
            profile,
        };
        if x.partial {
            return self.partial_fillet(id, x, &groups, &spec, state);
        }
        let edges: Vec<Vec<EdgeId>> = groups.iter().map(|g| g.edges.clone()).collect();
        let bodies = groups.iter().map(|g| (g.part, g.body)).collect();
        if x.smooth_corners {
            // Final (PS14.6): corners where three fillets meet set back and blended.
            if spec.profile != FilletProfile::Circular || x.measurement != FilletMeasurement::Radius {
                return Err("Smooth fillet corners take a circular fillet measured by its radius".into());
            }
            let setback = crate::applied::SMOOTH_SETBACK * x.size;
            return self.per_part(id, "Fillet", state, bodies, |this, i, body| this.kernel.fillet_smooth(body, &edges[i], &spec, setback));
        }
        self.per_part(id, "Fillet", state, bodies, |this, i, body| this.kernel.fillet_with(body, &edges[i], &spec))
    }

    /// A partial fillet (P3.11, PS14.6): the one picked edge between its bounds.
    fn partial_fillet(&mut self, id: FeatureId, x: &FilletFeature, groups: &[PartEdges], spec: &FilletSpec, state: &Arc<State>) -> Result<Output, String> {
        let [g] = groups else { return Err("A partial fillet takes one edge".into()) };
        let [edge] = g.edges[..] else { return Err("A partial fillet takes one edge".into()) };
        let infos = self.kernel.edges(g.body).map_err(|e| e.to_string())?;
        let info = infos.iter().find(|i| i.id == edge).ok_or("The edge to fillet no longer exists")?;
        let (from, to) = x.partial_range(info.length)?;
        let spec = *spec;
        self.per_part(id, "Fillet", state, vec![(g.part, g.body)], |this, _, body| this.kernel.fillet_partial(body, edge, &spec, from, to))
    }

    /// A variable fillet (P3.10, PS14.6): per edge of the picked edges' tangent chains, the
    /// radius at its start and end (a picked vertex's radius there, else the fillet's size) and
    /// at its points on edge; without Smooth transition the radius runs linearly between them
    /// (the law sampled finely, since OCCT's law interpolates smoothly through its points).
    fn variable_fillet(&mut self, id: FeatureId, x: &FilletFeature, groups: Vec<PartEdges>, state: &Arc<State>) -> Result<Output, String> {
        if x.asymmetric || x.control != FilletControl::Distance || x.measurement != FilletMeasurement::Radius {
            return Err("A variable fillet takes a radius with the Distance control (not asymmetric)".into());
        }
        let vertex_at = |v: &crate::applied::VertexRadius| -> Option<(Point3<f64>, f64)> {
            let point = state
                .parts
                .iter()
                .find_map(|p| p.part.solid.vertex(&v.vertex.vertex).map(|q| q.point))
                .unwrap_or(v.vertex.point);
            Some((Point3::new(point[0], point[1], point[2]), v.radius))
        };
        let vertices: Vec<(Point3<f64>, f64)> = x.vertices.iter().filter_map(vertex_at).collect();
        let mut laws: Vec<Vec<cadrs_kernel::FilletLaw>> = Vec::new();
        for g in &groups {
            let mut all: Vec<EdgeId> = Vec::new();
            for &e in &g.edges {
                for c in self.kernel.tangent_chain(g.body, e).map_err(|e| e.to_string())? {
                    if !all.contains(&c) {
                        all.push(c);
                    }
                }
            }
            let infos = self.kernel.edges(g.body).map_err(|e| e.to_string())?;
            let mut part_laws = Vec::new();
            for e in all {
                let info = infos.iter().find(|i| i.id == e).ok_or("An edge to fillet no longer exists")?;
                let at = |p: Point3<f64>| vertices.iter().find(|(q, _)| (q - p).norm() < 1e-4).map(|(_, r)| *r).unwrap_or(x.size);
                let mut radii = vec![(0.0, at(info.start)), (1.0, at(info.end))];
                for pt in &x.edge_points {
                    if let Some((part, ids)) = edge_ids(state, &pt.edge)
                        && part.part.id == g.part
                        && ids.contains(&e)
                    {
                        radii.push((pt.location, pt.radius));
                    }
                }
                radii.sort_by(|a, b| a.0.total_cmp(&b.0));
                if !x.smooth_transition {
                    let mut dense = Vec::new();
                    for w in radii.windows(2) {
                        for k in 0..8 {
                            let f = k as f64 / 8.0;
                            dense.push((w[0].0 + (w[1].0 - w[0].0) * f, w[0].1 + (w[1].1 - w[0].1) * f));
                        }
                    }
                    dense.push(*radii.last().expect("two"));
                    radii = dense;
                }
                part_laws.push(cadrs_kernel::FilletLaw { edge: e, radii });
            }
            laws.push(part_laws);
        }
        let bodies = groups.iter().map(|g| (g.part, g.body)).collect();
        let overflow = x.allow_overflow;
        self.per_part(id, "Fillet", state, bodies, |this, i, body| this.kernel.fillet_variable(body, &laws[i], overflow))
    }

    /// The Full round tab (PS14.1): one side face, the center face and the other side face, on
    /// one part.
    fn full_round(&mut self, id: FeatureId, x: &FilletFeature, state: &Arc<State>) -> Result<Output, String> {
        let one = |refs: &[crate::document::FaceRef]| -> Result<(PartId, BodyId, FaceId), String> {
            let r = refs.first().ok_or("Select the side faces and the center face")?;
            let (part, faces) = face_ids(state, r).ok_or("A selected face no longer exists")?;
            let body = part.body.ok_or("A part has no body")?;
            let face = *faces.first().ok_or("A selected face no longer exists")?;
            Ok((part.part.id, body, face))
        };
        let (a, c, b) = (one(&x.side1)?, one(&x.center)?, one(&x.side2)?);
        if a.0 != c.0 || b.0 != c.0 {
            return Err("The full round's faces must be on one part".into());
        }
        self.per_part(id, "Full round", state, vec![(c.0, c.1)], |this, _, body| {
            this.kernel.full_round(body, a.2, c.2, b.2)
        })
    }

    /// The Chamfer feature (PS14.7–14.9).
    pub(in crate::rebuild) fn chamfer(&mut self, id: FeatureId, x: &ChamferFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let mut groups = self.entity_edges(state, &x.entities)?;
        let uses = if x.tangent_propagation { self.chain_uses(id, state, &groups) } else { Vec::new() };
        if x.tangent_propagation {
            for g in &mut groups {
                let mut all: Vec<EdgeId> = Vec::new();
                for &e in &g.edges {
                    for c in self.kernel.tangent_chain(g.body, e).map_err(|e| e.to_string())? {
                        if !all.contains(&c) {
                            all.push(c);
                        }
                    }
                }
                g.edges = all;
            }
        } else {
            self.check_no_propagation(&groups, "chamfer")?;
        }
        let spec = match x.kind {
            ChamferType::EqualDistance => ChamferSpec::EqualDistance(x.distance),
            ChamferType::TwoDistances => ChamferSpec::TwoDistances(x.distance, x.distance2),
            ChamferType::DistanceAngle => ChamferSpec::DistanceAngle {
                distance: x.distance,
                angle: x.angle.to_radians(),
            },
        };
        // The overridden edges, per part.
        let flipped: Vec<Vec<EdgeId>> = groups
            .iter()
            .map(|g| {
                x.overrides
                    .iter()
                    .filter_map(|r| edge_ids(state, r).filter(|(p, _)| p.part.id == g.part).map(|(_, ids)| ids))
                    .flatten()
                    .collect()
            })
            .collect();
        let edges: Vec<Vec<EdgeId>> = groups.iter().map(|g| g.edges.clone()).collect();
        let bodies = groups.iter().map(|g| (g.part, g.body)).collect();
        let measurement = match x.measurement {
            ChamferMeasurement::Offset => ChamferMeasure::Offset,
            ChamferMeasurement::Tangent => ChamferMeasure::Tangent,
        };
        let flip = x.flip;
        self.per_part(id, "Chamfer", state, bodies, |this, i, body| {
            let opts = ChamferOpts {
                spec,
                measurement,
                flip,
                flipped: flipped[i].clone(),
            };
            this.kernel.chamfer_with(body, &edges[i], &opts)
        })
        .map(|mut o| {
            o.uses = uses;
            o
        })
    }

    /// The Shell feature (PS16).
    pub(in crate::rebuild) fn shell(&mut self, id: FeatureId, x: &ShellFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        // The faces to remove, by part (or the parts to hollow).
        let mut groups: Vec<(PartId, BodyId, Vec<cadrs_kernel::FaceId>)> = Vec::new();
        if x.hollow {
            for p in &x.parts {
                let part = state.part(*p).ok_or("A selected part no longer exists")?;
                groups.push((*p, part.body.ok_or("A part has no body")?, Vec::new()));
            }
        } else {
            for f in &x.faces {
                let (part, ids) = face_ids(state, f).ok_or("A face to remove no longer exists")?;
                let body = part.body.ok_or("A part has no body")?;
                match groups.iter_mut().find(|g| g.0 == part.part.id) {
                    Some(g) => g.2.extend(ids.into_iter().filter(|i| !g.2.contains(i)).collect::<Vec<_>>()),
                    None => groups.push((part.part.id, body, ids)),
                }
            }
        }
        let faces: Vec<Vec<cadrs_kernel::FaceId>> = groups.iter().map(|g| g.2.clone()).collect();
        let bodies = groups.iter().map(|g| (g.0, g.1)).collect();
        let (t, outward, hollow) = (x.thickness, x.outward, x.hollow);
        self.per_part(id, "Shell", state, bodies, |this, i, body| {
            let spec = ShellSpec {
                remove: faces[i].clone(),
                thickness: t,
                outward,
                hollow,
            };
            this.kernel.shell_with(body, &spec)
        })
    }

    /// The Hole feature (PS15): a revolved tool per point, subtracted from the parts in scope.
    pub(in crate::rebuild) fn hole(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        h: &HoleFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = h.problem() {
            return Err(p.into());
        }
        let op = id.0;
        let mut points = hole_points(before, h)?;
        // P3.8 (PS15.2): a hole at each mate connector, drilled along its −Z axis.
        for c in &h.connectors {
            let f = super::super::connector_frame(before, state, c)?;
            let key = naming::stable_hash(format!("{c:?}").as_bytes());
            let n = f.normal();
            if !points.iter().any(|(k, _, _)| *k == key) {
                points.push((key, Point3::from(f.origin), Unit::new_normalize(Vector3::new(n[0], n[1], n[2]))));
            }
        }
        if points.is_empty() {
            return Err("The selected sketch points no longer exist".into());
        }
        let solids: Vec<&PartState> = state
            .parts
            .iter()
            .filter(|p| p.part.kind == PartKind::Solid && p.body.is_some())
            .collect();
        let scope: Vec<&PartState> = if h.merge_scope.is_empty() {
            solids.clone()
        } else {
            solids.iter().copied().filter(|p| h.merge_scope.contains(&p.part.id)).collect()
        };
        if scope.is_empty() {
            return Err("There are no parts to cut".into());
        }
        let bodies: Vec<BodyId> = scope.iter().filter_map(|p| p.body).collect();
        let spec = &h.spec;
        // P3.10: the Hole start plane and the Up to entity target, as planes (a curved target
        // face is met where the axis crosses it).
        let plane_frame = |mp: &crate::pattern::MirrorPlane, what: &str| -> Result<cadrs_sketch::PlaneFrame, String> {
            let lost = || format!("The {what} no longer exists");
            Ok(match mp {
                crate::pattern::MirrorPlane::Plane(p) => super::advanced::plane_of(state, p).ok_or_else(lost)?,
                crate::pattern::MirrorPlane::Face(f) => {
                    let (part, _) = face_ids(state, f).ok_or_else(lost)?;
                    face_frame(part, f).ok_or_else(|| format!("The {what} must be a plane or a flat face"))?
                }
                crate::pattern::MirrorPlane::Connector(c) => super::super::connector_frame(before, state, c)?,
            })
        };
        let start_frame = match (spec.start, &h.start_plane) {
            (HoleStart::SelectedPlane, Some(p)) => Some(plane_frame(p, "hole start plane")?),
            (HoleStart::SelectedPlane, None) => return Err("Select a hole start plane".into()),
            _ => None,
        };
        // Up to entity: a curved face of a part (met by ray), else a plane.
        enum Target {
            Plane(cadrs_sketch::PlaneFrame),
            Faces(BodyId, Vec<cadrs_kernel::FaceId>),
        }
        let target = match (spec.end, &h.up_to) {
            (HoleEnd::UpToEntity, Some(crate::pattern::MirrorPlane::Face(f))) => {
                let (part, ids) = face_ids(state, f).ok_or("The face to go up to no longer exists")?;
                match face_frame(part, f) {
                    Some(fr) => Some(Target::Plane(fr)),
                    None => Some(Target::Faces(part.body.ok_or("A part has no body")?, ids)),
                }
            }
            (HoleEnd::UpToEntity, Some(p)) => Some(Target::Plane(plane_frame(p, "entity to go up to")?)),
            (HoleEnd::UpToEntity, None) => return Err("Select a face or plane to go up to".into()),
            _ => None,
        };
        // Where the axis from `o` along `d` crosses a plane (None when parallel to it).
        let cross = |fr: &cadrs_sketch::PlaneFrame, o: &Point3<f64>, d: &Vector3<f64>| -> Option<f64> {
            let n = fr.normal();
            let n = Vector3::new(n[0], n[1], n[2]);
            let along = n.dot(d);
            (along.abs() > 1e-9).then(|| (Point3::new(fr.origin[0], fr.origin[1], fr.origin[2]) - o).dot(&n) / along)
        };
        let end_offset = spec.end_offset.as_ref().map_or(0.0, |l| l.value);
        let mut tools: Vec<(BodyId, BodyNames)> = Vec::new();
        let release_all = |this: &mut Self, tools: &[(BodyId, BodyNames)]| {
            for (b, _) in tools {
                this.kernel.release(*b);
            }
        };
        // PS11.1: holes whose axis meets no part in the scope are left out, with a warning.
        let mut skipped = 0usize;
        for (key, origin, normal) in &points {
            let dir = if h.flip { *normal } else { -*normal };
            // Every crossing of the hole's axis with the parts, ahead of the sketch plane.
            let mut hits: Vec<f64> = Vec::new();
            // Cast from 1 mm behind the plane, so a face lying on it (a sketch on Top under a
            // block standing on Top) is still hit.
            let back = origin - dir.into_inner();
            // Every crossing along the whole line, for whether a point is inside a part.
            let mut all_hits: Vec<f64> = Vec::new();
            for b in &bodies {
                let found = self.kernel.ray_hits(*b, back, dir.into_inner()).map_err(|e| e.to_string())?;
                hits.extend(found.iter().map(|x| x.t - 1.0).filter(|t| *t >= -1e-7));
                all_hits.extend(found.iter().map(|x| x.t - 1.0));
            }
            hits.sort_by(f64::total_cmp);
            let start = match spec.start {
                HoleStart::SketchPlane => 0.0,
                HoleStart::SelectedPlane => {
                    let fr = start_frame.as_ref().expect("checked above");
                    match cross(fr, origin, &dir) {
                        Some(t) => t,
                        None => {
                            release_all(self, &tools);
                            return Err("The hole start plane is parallel to the hole".into());
                        }
                    }
                }
                // A point off the parts in the merge scope gets no hole (PS15.3: a point over
                // a part left out of the scope); none reaching one is the error below.
                HoleStart::Part => match hits.first() {
                    Some(t) => t.max(0.0),
                    None => {
                        skipped += 1;
                        continue;
                    }
                },
            };
            let (depth, through) = match spec.end {
                HoleEnd::Blind => (spec.depth.value, false),
                HoleEnd::ThroughAll => {
                    let far = bodies
                        .iter()
                        .filter_map(|b| self.kernel.bounding_box(*b).ok())
                        .map(|bb| bb.max_along(*origin, &dir))
                        .fold(0.0, f64::max);
                    ((far - start).max(0.0) + 1.0, true)
                }
                HoleEnd::UpToEntity => {
                    let t = match target.as_ref().expect("checked above") {
                        Target::Plane(fr) => cross(fr, origin, &dir),
                        Target::Faces(body, ids) => self
                            .kernel
                            .ray_hits(*body, back, dir.into_inner())
                            .map_err(|e| e.to_string())?
                            .iter()
                            .filter(|x| ids.contains(&x.face))
                            .map(|x| x.t - 1.0)
                            .find(|t| *t > start + 1e-6),
                    };
                    match t {
                        Some(t) if t - end_offset > start + 1e-6 => (t - end_offset - start, false),
                        _ => {
                            release_all(self, &tools);
                            return Err("A hole's entity to go up to isn't ahead of its start".into());
                        }
                    }
                }
                HoleEnd::UpToNext => match hits.iter().find(|t| **t > start + 1e-6) {
                    Some(t) if t - end_offset > start + 1e-6 => (t - end_offset - start, false),
                    Some(_) => {
                        release_all(self, &tools);
                        return Err("The hole's offset goes back past its start".into());
                    }
                    None if hits.is_empty() => {
                        skipped += 1;
                        continue;
                    }
                    None => {
                        release_all(self, &tools);
                        return Err("A hole has nothing to go up to".into());
                    }
                },
            };
            // A hole started from a selected plane inside a part is buried: nothing above its
            // start is cut (P3.10, PS15.6); elsewhere the tool starts a little above the surface.
            let buried = spec.start == HoleStart::SelectedPlane
                && all_hits.iter().filter(|t| **t < start - 1e-7).count() % 2 == 1;
            let section = spec.section(depth, through, if buried { 0.0 } else { HOLE_ABOVE });
            // The section's plane holds the axis: x radial, y along the hole.
            let helper = if dir.x.abs() < 0.9 { Vector3::x() } else { Vector3::y() };
            let u = Unit::new_normalize(helper - dir.into_inner() * helper.dot(&dir));
            let at = origin + dir.into_inner() * start;
            let plane = Plane {
                origin: at,
                x_dir: u,
                normal: Unit::new_normalize(u.cross(&dir)),
            };
            let n = section.len();
            let curves: Vec<Curve2> = (0..n)
                .map(|i| Curve2::Line {
                    a: Point2::new(section[i].0, section[i].1),
                    b: Point2::new(section[(i + 1) % n].0, section[(i + 1) % n].1),
                    source: Some(i as u64 + 1),
                })
                .collect();
            let profile = Profile::new(
                plane,
                vec![Region {
                    outer: Loop { curves },
                    holes: Vec::new(),
                    source: Some(*key),
                }],
            );
            let axis = Axis { origin: at, dir };
            let made = self.kernel.revolve_with(&profile, &RevolveSpec::solid(axis, None)).and_then(|r| {
                let body = r.bodies[0];
                Ok((body, naming::name_body(&self.kernel, body, op, &r.history, &[])?))
            });
            match made {
                Ok(t) => tools.push(t),
                Err(e) => {
                    release_all(self, &tools);
                    return Err(format!("Hole failed: {e}"));
                }
            }
        }
        if tools.is_empty() {
            return Err("The holes don't reach a part".into());
        }
        // The parts the holes reach (by box, then the boolean decides).
        let tool_boxes: Vec<cadrs_kernel::Aabb> = tools.iter().filter_map(|(b, _)| self.kernel.bounding_box(*b).ok()).collect();
        let reached: Vec<PartId> = scope
            .iter()
            .filter(|p| {
                let Some(pb) = p.body.and_then(|b| self.kernel.bounding_box(b).ok()) else {
                    return false;
                };
                tool_boxes.iter().any(|tb| (0..3).all(|i| tb.min[i] <= pb.max[i] && pb.min[i] <= tb.max[i]))
            })
            .map(|p| p.part.id)
            .collect();
        let mut next = (**state).clone();
        let refs: Vec<&(BodyId, BodyNames)> = tools.iter().collect();
        let mut placed = Vec::new();
        for part in &reached {
            match self.cut(id, *part, BoolOp::Subtract, &refs, &mut next) {
                Ok(p) => placed.extend(p),
                Err(e) => {
                    release_all(self, &tools);
                    for (_, p) in placed {
                        let p: Piece = p;
                        self.kernel.release(p.body);
                    }
                    return Err(e.replace("Remove failed", "Hole failed"));
                }
            }
        }
        release_all(self, &tools);
        if reached.is_empty() {
            return Err("The holes don't reach a part".into());
        }
        let geoms = state.geoms.clone();
        next.geoms = geoms.clone();
        // The parts the first hole drills (the dialog's default Merge scope).
        let drilled: Vec<PartId> = match tool_boxes.first() {
            Some(tb) => scope
                .iter()
                .filter(|p| {
                    p.body
                        .and_then(|b| self.kernel.bounding_box(b).ok())
                        .is_some_and(|pb| (0..3).all(|i| tb.min[i] <= pb.max[i] && pb.min[i] <= tb.max[i]))
                })
                .map(|p| p.part.id)
                .collect(),
            None => Vec::new(),
        };
        let automatic = h.merge_scope.is_empty();
        self.finish(id, placed, next, op, geoms, PartKind::Solid).map(|mut o| {
            // P3.8 judge: the parts drilled, for the dialog's default Merge scope.
            if automatic {
                o.contacts = Some(Contacts { touches: drilled.clone(), overlaps: drilled });
            }
            if skipped > 0 {
                o.warning = Some(if skipped == 1 {
                    "1 hole doesn't reach a part in the merge scope".into()
                } else {
                    format!("{skipped} holes don't reach a part in the merge scope")
                });
            }
            o
        })
    }
}

/// The world position and sketch-plane normal of every point a hole is placed at, with a stable
/// key per point (for the names of its faces).
/// A hole's place: its key, where it is, and the sketch plane's normal there.
type HolePlace = (u64, Point3<f64>, Unit<Vector3<f64>>);

fn hole_points(before: &[Feature], h: &HoleFeature) -> Result<Vec<HolePlace>, String> {
    use slotmap::Key;
    let mut out = Vec::new();
    let mut lost = 0;
    let key = |sketch: FeatureId, p: cadrs_sketch::PointId| {
        let mut bytes = sketch.0.as_bytes().to_vec();
        bytes.extend_from_slice(&p.data().as_ffi().to_le_bytes());
        naming::stable_hash(&bytes)
    };
    let add = |sketch: FeatureId, p: cadrs_sketch::PointId, out: &mut Vec<_>| -> bool {
        let Some(sk) = before.iter().find(|f| f.id == sketch).and_then(|f| f.sketch()) else {
            return false;
        };
        let Some(plane) = sk.plane else { return false };
        let Some(pt) = sk.geometry.points.get(p) else { return false };
        let frame = plane.frame();
        let w = frame.to_world(pt.pos);
        let n = frame.normal();
        let k = key(sketch, p);
        if !out.iter().any(|(x, _, _): &(u64, _, _)| *x == k) {
            out.push((k, Point3::new(w[0], w[1], w[2]), Unit::new_normalize(Vector3::new(n[0], n[1], n[2]))));
        }
        true
    };
    for p in &h.points {
        if !add(p.sketch, p.point, &mut out) {
            lost += 1;
        }
    }
    for s in &h.sketches {
        let Some(sk) = before.iter().find(|f| f.id == *s).and_then(|f| f.sketch()) else {
            lost += 1;
            continue;
        };
        for p in crate::hole::hole_vertices(&sk.geometry) {
            add(*s, p, &mut out);
        }
    }
    if lost > 0 && !out.is_empty() {
        return Err(if lost == 1 {
            "1 selected sketch point no longer exists".into()
        } else {
            format!("{lost} selected sketch points no longer exist")
        });
    }
    Ok(out)
}

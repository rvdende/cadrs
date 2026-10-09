//! Rebuilding the Transform feature (Onshape's Transform; the model is in `crate::transform`). A
//! child of `rebuild::kernel_ops`, so it shares its helpers (directions, axes, finishing).
//!
//! - **Moved** (or scaled) parts keep their [`PartId`] and every name: the kernel copies the body
//!   face for face, the faces take their originals' names and, where the copy's edges and
//!   vertices are its original's in the same order (checked by position), their names too. The
//!   swept geometry of the operations whose faces are only on moved parts moves with them (a
//!   scale drops it), so their faces' frames and silhouettes follow; the mate connectors and
//!   pictures the parts own move with them.
//! - **Copies** (Copy part, Copy in place) are new parts named as a pattern's copies
//!   (`FaceOrigin::Instance` under the Transform's id, instance 1), showing their original's
//!   appearance and material.
//! - **Context parts** (P3H.6, PCB7.9; they can only be copied): each Part Studio they come from
//!   is rebuilt once from the feature's snapshot (the rebuild in progress set aside meanwhile),
//!   the part placed where the context had it, then mapped like the others. A copy keeps its
//!   Part Studio's colour (`Solid::looks`); moved parts and copies of copies keep theirs.
//! - **Composite part** (P3H.6): copies of the members gathered unmerged into one body
//!   (`Kernel::compound`), its faces keeping the members' names and colours; a new part
//!   "Composite part N" with the first member's palette entry.

use std::collections::HashMap;

use super::*;
use cadrs_kernel::naming::{self, BodyNames};
use cadrs_kernel::{Kernel, Motion};
use cadrs_sketch::{FaceName, FaceOrigin, PlaneFrame};
use nalgebra::{Matrix3, Point3, Vector3};

use crate::appearance::Appearance;
use crate::transform::{Composite, CompositeFeature, TransformFeature, TransformType};

/// A copy's part, its original (none for a context part's), palette entry, kind and colour.
type CopyLook = (PartId, Option<PartId>, u32, PartKind, Option<Appearance>);

fn motion_of(p: &crate::assembly::Pose) -> Motion {
    Motion { linear: Matrix3::from_row_slice(&p.rotation.concat()), translation: Vector3::from(p.translation) }
}

/// What a Transform does to space.
#[derive(Debug, Clone, Copy)]
enum Map {
    /// A rotation and translation.
    Rigid(Motion),
    /// A uniform scale about a point.
    Scale { center: Point3<f64>, factor: f64 },
}

impl Map {
    fn point(&self, p: &Point3<f64>) -> Point3<f64> {
        match self {
            Map::Rigid(m) => m.point(p),
            Map::Scale { center, factor } => center + (p - center) * *factor,
        }
    }

    /// [`Self::point`] of a point given as an array.
    fn world(&self, p: [f64; 3]) -> [f64; 3] {
        let q = self.point(&Point3::from(p));
        [q.x, q.y, q.z]
    }

    fn frame(&self, f: &PlaneFrame) -> PlaneFrame {
        let v = |a: [f64; 3]| Vector3::new(a[0], a[1], a[2]);
        let arr = |a: Vector3<f64>| [a.x, a.y, a.z];
        let origin = self.point(&Point3::from(v(f.origin))).coords;
        match self {
            Map::Rigid(m) => PlaneFrame { origin: arr(origin), u: arr(m.vector(&v(f.u))), v: arr(m.vector(&v(f.v))) },
            Map::Scale { .. } => PlaneFrame { origin: arr(origin), ..*f },
        }
    }
}

fn v3(p: [f64; 3]) -> Vector3<f64> {
    Vector3::new(p[0], p[1], p[2])
}

/// The rigid motion taking a frame's local coordinates (X = `u`, Y = `v`, Z their normal) to
/// the world, with its axes made orthonormal.
fn frame_motion(f: &PlaneFrame) -> Result<Motion, String> {
    let u = v3(f.u);
    let n = u.cross(&v3(f.v));
    if u.norm() < 1e-12 || n.norm() < 1e-12 {
        return Err("A mate connector has no orientation".into());
    }
    let u = u.normalize();
    let n = n.normalize();
    let v = n.cross(&u);
    Ok(Motion { linear: Matrix3::from_columns(&[u, v, n]), translation: v3(f.origin) })
}

impl Rebuilder {
    /// The Transform feature.
    pub(in crate::rebuild) fn transform(&mut self, before: &[Feature], id: FeatureId, x: &TransformFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let mut parts: Vec<PartId> = Vec::new();
        for p in &x.parts {
            if state.part(*p).is_some() && !parts.contains(p) {
                parts.push(*p);
            }
        }
        if parts.is_empty() && x.context.is_empty() && x.connectors.is_empty() {
            return Err(if x.parts.len() == 1 {
                "The part to transform no longer exists".into()
            } else {
                "The parts to transform no longer exist".into()
            });
        }
        let missing = x.parts.iter().filter(|p| state.part(**p).is_none()).count();
        let map = self.transform_map(before, x, state)?;
        let mut out = if parts.is_empty() && x.context.is_empty() {
            Output { state: state.clone(), error: None, warning: None, contacts: None, owned: Vec::new(), stage: None, axis: None, dots: None, uses: Vec::new(), arrows: Vec::new() }
        } else if x.copies() {
            self.transform_copies(id, &parts, x, &map, state)?
        } else {
            self.transform_moves(id, &parts, &map, state)?
        };
        // Its mate connectors: moved (an explicit one's frame), or copied as its own.
        if !x.connectors.is_empty() {
            let mut next = (*out.state).clone();
            for c in &x.connectors {
                let frame = super::super::connector_frame(before, state, c).map_err(|e| if e.is_empty() { "A mate connector to transform no longer exists".to_string() } else { e })?;
                let moved = map.frame(&frame);
                let owner = match c {
                    crate::mate::ConnectorRef::Feature(f) => state.connector_owners.get(f).copied(),
                    crate::mate::ConnectorRef::Implicit(o) => o.part(),
                };
                let key = match (x.copies(), c) {
                    (false, crate::mate::ConnectorRef::Feature(f)) => *f,
                    (false, _) => return Err("An implicit mate connector can only be copied: check Copy part".into()),
                    (true, _) => id,
                };
                next.connectors.insert(key, moved);
                if let Some(o) = owner {
                    next.connector_owners.insert(key, o);
                }
            }
            out.state = Arc::new(next);
        }
        // PS11.1: the others moved; a warning, as Onshape's yellow.
        if missing > 0 {
            out.warning = Some(if missing == 1 {
                "1 part to transform no longer exists; the others are transformed".into()
            } else {
                format!("{missing} parts to transform no longer exist; the others are transformed")
            });
        }
        Ok(out)
    }

    /// What the Transform's type and fields do to space.
    fn transform_map(&self, before: &[Feature], x: &TransformFeature, state: &State) -> Result<Map, String> {
        let sign = if x.flip { -1.0 } else { 1.0 };
        Ok(match x.transform_type {
            TransformType::TranslateByLine => {
                let line = x.line.ok_or("Select a line to translate along")?;
                Map::Rigid(Motion::translation(self.line_vector(before, state, &line)? * sign))
            }
            TransformType::TranslateByDistance => {
                let d = x.direction.ok_or("Select a direction")?;
                let u = self
                    .direction(before, state, &d)
                    .map_err(|e| e.replace("extrude direction", "direction"))?
                    .into_inner();
                Map::Rigid(Motion::translation(u * x.distance * sign))
            }
            TransformType::TranslateXyz => Map::Rigid(Motion::translation(Vector3::new(x.dx, x.dy, x.dz))),
            TransformType::MateConnectors => {
                let from = x.from.ok_or("Select the mate connector to move from")?;
                let to = x.to.ok_or("Select the mate connector to move to")?;
                let lost = |e: String| if e.is_empty() { "A mate connector no longer exists".to_string() } else { e };
                let a = frame_motion(&super::super::connector_frame(before, state, &from).map_err(lost)?)?;
                let mut b = frame_motion(&super::super::connector_frame(before, state, &to).map_err(lost)?)?;
                // Flip primary axis: half a turn about the destination's X (its Z and Y reversed);
                // then the secondary axis turned about the (flipped) Z.
                if x.flip_primary {
                    b.linear.set_column(1, &(-b.linear.column(1)));
                    b.linear.set_column(2, &(-b.linear.column(2)));
                }
                let turn = x.secondary.degrees().to_radians();
                if turn != 0.0 {
                    let (u, v) = (b.linear.column(0).into_owned(), b.linear.column(1).into_owned());
                    let (s, c) = turn.sin_cos();
                    b.linear.set_column(0, &(u * c + v * s));
                    b.linear.set_column(1, &(v * c - u * s));
                }
                Map::Rigid(a.inverse().then(&b))
            }
            TransformType::Rotate => {
                let a = x.axis.ok_or("Select an axis of rotation")?;
                let axis = self.axis(before, state, &a).map_err(|e| e.replace("revolve axis", "axis of rotation"))?;
                Map::Rigid(Motion::rotation(&axis, x.angle.to_radians() * sign))
            }
            TransformType::CopyInPlace => Map::Rigid(Motion::identity()),
            TransformType::ScaleUniformly => {
                let center = match &x.scale_point {
                    Some(c) => {
                        let f = super::super::connector_frame(before, state, c)
                            .map_err(|e| if e.is_empty() { "The point to scale about no longer exists".to_string() } else { e })?;
                        Point3::from(v3(f.origin))
                    }
                    None => Point3::origin(),
                };
                Map::Scale { center, factor: x.scale }
            }
        })
    }

    /// Translate by line: from the line's first end to its second (a straight part edge or a
    /// sketch line).
    fn line_vector(&self, before: &[Feature], state: &State, line: &DirectionRef) -> Result<Vector3<f64>, String> {
        let lost = || "The line to translate along no longer exists".to_string();
        let (a, b) = match line {
            DirectionRef::Edge(r) => {
                let edge = state
                    .part(r.part)
                    .and_then(|p| p.part.solid.edge(&r.edge))
                    .or_else(|| state.parts.iter().find_map(|p| p.part.solid.edge(&r.edge)))
                    .ok_or_else(lost)?;
                let (a, b) = (v3(edge.points[0]), v3(*edge.points.last().ok_or_else(lost)?));
                let straight = edge.points.iter().all(|p| (b - a).cross(&(v3(*p) - a)).norm() < 1e-6 * (b - a).norm_squared().max(1e-12));
                if !straight || edge.circle.is_some() {
                    return Err("Translate by line needs a straight edge".into());
                }
                (a, b)
            }
            DirectionRef::SketchLine { sketch, curve } => {
                let sk = before.iter().find(|f| f.id == *sketch).and_then(|f| f.sketch()).ok_or_else(lost)?;
                let frame = sk.plane.ok_or_else(lost)?.frame();
                let CurveKind::Line { a, b } = sk.geometry.curves.get(*curve).ok_or_else(lost)?.kind else {
                    return Err("Translate by line needs a sketch line".into());
                };
                (v3(frame.to_world(sk.geometry.pos(a))), v3(frame.to_world(sk.geometry.pos(b))))
            }
            _ => return Err("Translate by line needs a straight edge or a sketch line".into()),
        };
        if (b - a).norm() < 1e-12 {
            return Err("The line has no length".into());
        }
        Ok(b - a)
    }

    /// A copy of `body` moved (or scaled) by `map`.
    fn mapped_copy(&mut self, body: BodyId, map: &Map) -> Result<cadrs_kernel::OpResult, String> {
        match map {
            Map::Rigid(m) => self.kernel.transform_motion(body, m),
            Map::Scale { center, factor } => self.kernel.scale(body, *center, *factor),
        }
        .map_err(|e| format!("Transform failed: {e}"))
    }

    /// The names of `new`, a face-for-face copy of `old` (named `source`) moved by `map`: the
    /// faces its originals', the edges and vertices too where they are the original's in the same
    /// order (else named from the faces).
    fn carried_names(&self, old: BodyId, new: BodyId, history: &cadrs_kernel::History, source: &BodyNames, op: cadrs_kernel::OpId, map: &Map) -> Result<BodyNames, String> {
        let n = self.kernel.faces(new).map_err(|e| e.to_string())?.len();
        let mut faces: Vec<Option<FaceName>> = vec![None; n];
        for (f, input) in &history.modified {
            if let (Some(slot), Some(name)) = (faces.get_mut(f.0 as usize), source.face(input.face)) {
                *slot = Some(name);
            }
        }
        let faces: Vec<FaceName> = faces
            .into_iter()
            .enumerate()
            .map(|(i, f)| f.unwrap_or_else(|| FaceName::new(op, FaceOrigin::Unnamed { index: i as u32 })))
            .collect();
        let (eo, en) = (self.kernel.edges(old).map_err(|e| e.to_string())?, self.kernel.edges(new).map_err(|e| e.to_string())?);
        let (vo, vn) = (self.kernel.vertices(old).map_err(|e| e.to_string())?, self.kernel.vertices(new).map_err(|e| e.to_string())?);
        let size = en.iter().map(|e| e.mid.coords.amax()).fold(1.0f64, f64::max);
        let tol = 1e-6 * size;
        let same = eo.len() == en.len()
            && vo.len() == vn.len()
            && source.edges.len() == eo.len()
            && source.vertices.len() == vo.len()
            && eo.iter().zip(&en).all(|(a, b)| (map.point(&a.mid) - b.mid).norm() < tol)
            && vo.iter().zip(&vn).all(|(a, b)| (map.point(&a.point) - b.point).norm() < tol);
        if same {
            return Ok(BodyNames { faces, edges: source.edges.clone(), vertices: source.vertices.clone(), aliases: source.aliases.clone() });
        }
        Ok(BodyNames {
            edges: naming::name_edges(&faces, &en, naming::joint_tolerance(&en)),
            vertices: naming::name_vertices(&faces, &en, &vn),
            faces,
            aliases: source.aliases.clone(),
        })
    }

    /// The parts moved (or scaled) in place of themselves.
    fn transform_moves(&mut self, id: FeatureId, parts: &[PartId], map: &Map, state: &Arc<State>) -> Result<Output, String> {
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        let release = |this: &mut Self, placed: &[Placed]| {
            for (_, p) in placed {
                this.kernel.release(p.body);
            }
        };
        for pid in parts {
            let p = state.part(*pid).expect("present");
            let Some(body) = p.body else {
                release(self, &placed);
                return Err("A part to transform has no body".into());
            };
            let r = match self.mapped_copy(body, map) {
                Ok(r) => r,
                Err(e) => {
                    release(self, &placed);
                    return Err(e);
                }
            };
            let moved = r.bodies[0];
            let names = match self.carried_names(body, moved, &r.history, &p.names, id.0, map) {
                Ok(n) => n,
                Err(e) => {
                    self.kernel.release(moved);
                    release(self, &placed);
                    return Err(e);
                }
            };
            let volume = self.kernel.mass_properties(moved).map(|m| m.volume).unwrap_or(0.0);
            placed.push((*pid, Piece { body: moved, names, from: HashSet::new(), volume }));
        }
        // The swept geometry of the operations whose faces are all on moved parts follows them.
        let ops_of = |p: &PartState| p.names.faces.iter().map(|f| f.op).collect::<HashSet<_>>();
        let moved_ops: HashSet<cadrs_kernel::OpId> = parts.iter().filter_map(|p| state.part(*p)).flat_map(ops_of).collect();
        let still_ops: HashSet<cadrs_kernel::OpId> = state.parts.iter().filter(|p| !parts.contains(&p.part.id)).flat_map(ops_of).collect();
        let mut geoms = (*state.geoms).clone();
        for op in moved_ops.difference(&still_ops) {
            match map {
                Map::Rigid(m) => {
                    if let Some(g) = geoms.get(op) {
                        let l = &m.linear;
                        let rows = [[l[(0, 0)], l[(0, 1)], l[(0, 2)]], [l[(1, 0)], l[(1, 1)], l[(1, 2)]], [l[(2, 0)], l[(2, 1)], l[(2, 2)]]];
                        let t = [m.translation.x, m.translation.y, m.translation.z];
                        let g = Arc::new(g.moved(rows, t));
                        geoms.insert(*op, g);
                    }
                }
                Map::Scale { .. } => {
                    geoms.remove(op);
                }
            }
        }
        let geoms = Arc::new(geoms);
        next.geoms = geoms.clone();
        // The mate connectors the parts own move with them.
        for (connector, owner) in &state.connector_owners {
            if parts.contains(owner)
                && let Some(f) = next.connectors.get_mut(connector)
            {
                *f = map.frame(f);
            }
        }
        let mut out = self.finish(id, placed, next, id.0, geoms, PartKind::Solid)?;
        // P3H.6: moved parts keep their faces' colours (their faces keep their names), and
        // their pictures move with them.
        if let Some(st) = Arc::get_mut(&mut out.state) {
            for pid in parts {
                let Some(old) = state.part(*pid).map(|p| &p.part.solid) else { continue };
                if old.looks.is_empty() && old.images.is_empty() {
                    continue;
                }
                if let Some(p) = st.parts.iter_mut().find(|q| q.part.id == *pid) {
                    let solid = Arc::make_mut(&mut p.part.solid);
                    solid.looks = old.looks.clone();
                    solid.images = old.images.iter().map(|i| i.mapped(|q| map.world(q))).collect();
                }
            }
        }
        Ok(out)
    }

    /// Copy part (or Copy in place): moved copies as new parts, the originals left.
    fn transform_copies(&mut self, id: FeatureId, parts: &[PartId], x: &TransformFeature, map: &Map, state: &Arc<State>) -> Result<Output, String> {
        let op = id.0;
        let mut next = (**state).clone();
        let mut placed: Vec<Placed> = Vec::new();
        // Each copy's original (none for a context part's), palette entry, kind and colour.
        let mut sources: Vec<CopyLook> = Vec::new();
        // The pictures each copy shows, moved with it.
        let mut pictures: Vec<(PartId, Vec<crate::solid::SolidImage>)> = Vec::new();
        let release = |this: &mut Self, placed: &[Placed]| {
            for (_, p) in placed {
                this.kernel.release(p.body);
            }
        };
        for pid in parts {
            let p = state.part(*pid).expect("present");
            let Some(body) = p.body else {
                release(self, &placed);
                return Err("A part to transform has no body".into());
            };
            let r = match self.mapped_copy(body, map) {
                Ok(r) => r,
                Err(e) => {
                    release(self, &placed);
                    return Err(e);
                }
            };
            let copy = r.bodies[0];
            let pieces = self
                .instance_names(copy, &r.history, &p.names, op, 1)
                .and_then(|names| self.split_named(copy, op, &names, &HashMap::new()));
            self.kernel.release(copy);
            let pieces = match pieces {
                Ok(pieces) => pieces,
                Err(e) => {
                    release(self, &placed);
                    return Err(e);
                }
            };
            for piece in pieces {
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                let new = Self::new_id(id, &next, &taken);
                sources.push((new, Some(*pid), p.part.palette, p.part.kind, p.part.solid.looks.first().map(|(_, a)| *a)));
                if !p.part.solid.images.is_empty() {
                    pictures.push((new, p.part.solid.images.iter().map(|i| i.mapped(|q| map.world(q))).collect()));
                }
                placed.push((new, piece));
            }
        }
        // P3H.6: the context parts, each source Part Studio rebuilt once with the rebuild in
        // progress set aside. Each copy is its own instance (several may copy one part).
        let mut instance = 1u32;
        for (si, src) in x.sources.iter().enumerate() {
            let wanted: Vec<&crate::transform::ContextCopy> = x.context.iter().filter(|c| c.source == si).collect();
            if wanted.is_empty() {
                continue;
            }
            let trail = std::mem::take(&mut self.trail);
            let (last, stage_for) = (self.last.clone(), self.stage_for);
            let built = self.rebuild(&src.features);
            let src_state = self.last.clone();
            self.trail = trail;
            self.last = last;
            self.stage_for = stage_for;
            drop(built);
            for c in wanted {
                let Some(ps) = src_state.part(c.part) else {
                    release(self, &placed);
                    return Err(format!("{} is no longer in its Part Studio", c.name));
                };
                let Some(body) = ps.body else { continue };
                // Where the context had it, then the Transform's map.
                let at = match self.kernel.transform_motion(body, &motion_of(&c.pose)) {
                    Ok(r) => r.bodies[0],
                    Err(e) => {
                        release(self, &placed);
                        return Err(format!("Transform failed: {e}"));
                    }
                };
                let r = self.mapped_copy(at, map);
                self.kernel.release(at);
                let r = match r {
                    Ok(r) => r,
                    Err(e) => {
                        release(self, &placed);
                        return Err(e);
                    }
                };
                instance += 1;
                let copy = r.bodies[0];
                let pieces = self
                    .instance_names(copy, &r.history, &ps.names, op, instance)
                    .and_then(|names| self.split_named(copy, op, &names, &HashMap::new()));
                self.kernel.release(copy);
                let pieces = match pieces {
                    Ok(pieces) => pieces,
                    Err(e) => {
                        release(self, &placed);
                        return Err(e);
                    }
                };
                for piece in pieces {
                    let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                    let new = Self::new_id(id, &next, &taken);
                    sources.push((new, None, ps.part.palette, ps.part.kind, c.appearance));
                    placed.push((new, piece));
                }
            }
        }
        if placed.is_empty() {
            return Err("There is nothing to transform".into());
        }
        next.geoms = state.geoms.clone();
        let mut out = self.finish(id, placed, next, op, state.geoms.clone(), PartKind::Solid)?;
        // PS9.6: a copy looks like its original (its appearance and material).
        if let Some(st) = Arc::get_mut(&mut out.state) {
            for (pid, seed, palette, kind, look) in sources {
                if let Some(p) = st.parts.iter_mut().find(|q| q.part.id == pid) {
                    p.part.source = seed;
                    p.part.palette = palette;
                    p.part.kind = kind;
                    if let Some(a) = look {
                        let solid = Arc::make_mut(&mut p.part.solid);
                        solid.looks = solid.faces.iter().map(|f| (f.name, a)).collect();
                    }
                    if let Some((_, images)) = pictures.iter().find(|(q, _)| *q == pid) {
                        Arc::make_mut(&mut p.part.solid).images = images.clone();
                    }
                }
            }
        }
        Ok(out)
    }

    /// The Composite part feature (see the module docs).
    pub(in crate::rebuild) fn composite(&mut self, id: FeatureId, x: &CompositeFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let op = id.0;
        let mut inputs: Vec<(BodyId, Arc<BodyNames>)> = Vec::new();
        let mut members = Vec::new();
        // The members' rebuild colours (their faces keep their names in the composite).
        let mut member_looks: Vec<(FaceName, crate::appearance::Appearance)> = Vec::new();
        for p in &x.parts {
            let ps = state.part(*p).ok_or("A selected part no longer exists")?;
            let body = ps.body.ok_or("A selected part has no solid model")?;
            inputs.push((body, ps.names.clone()));
            members.push(*p);
            member_looks.extend(ps.part.solid.looks.iter().copied());
        }
        let bodies: Vec<BodyId> = inputs.iter().map(|(b, _)| *b).collect();
        let r = self.kernel.compound(&bodies).map_err(|e| format!("The composite part failed: {e}"))?;
        let body = r.bodies[0];
        let named: Vec<(BodyId, &BodyNames)> = inputs.iter().map(|(b, n)| (*b, &**n)).collect();
        let names = match naming::name_body(&self.kernel, body, op, &r.history, &named) {
            Ok(n) => n,
            Err(e) => {
                self.kernel.release(body);
                return Err(format!("The composite part failed: {e}"));
            }
        };
        let volume = self.kernel.mass_properties(body).map(|m| m.volume).unwrap_or(0.0);
        let mut next = (**state).clone();
        let pid = Self::new_id(id, &next, &[]);
        let palette = state.part(x.parts[0]).map(|p| p.part.palette).unwrap_or(0);
        let n = next.composites.len() + 1;
        next.composites.push(Composite { part: pid, members, closed: x.closed });
        next.geoms = state.geoms.clone();
        let before = next.next_part;
        let mut out = self.finish(id, vec![(pid, Piece { body, names, from: HashSet::new(), volume })], next, op, state.geoms.clone(), PartKind::Solid)?;
        if let Some(st) = Arc::get_mut(&mut out.state) {
            // A composite part isn't counted among "Part N".
            st.next_part = before;
            if let Some(p) = st.parts.iter_mut().find(|q| q.part.id == pid) {
                p.part.name = format!("Composite part {n}");
                p.part.palette = palette;
                if !member_looks.is_empty() {
                    let solid = Arc::make_mut(&mut p.part.solid);
                    solid.looks = solid.faces.iter().filter_map(|f| member_looks.iter().find(|(g, _)| *g == f.name).copied()).collect();
                }
            }
        }
        Ok(out)
    }
}

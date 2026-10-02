//! Rebuilding the sheet metal features that edit an active model (P3I.4: Flange, Hem, Make
//! joint; `crate::sheetmetal_features`), and the **SM1.6 hook** they share,
//! [`Rebuilder::edit_sheet_metal`], for any feature after a Sheet metal model that changes the
//! model's definition (P3I.5's Tab, Bend, … can use it too):
//!
//! 1. find the active model ([`SheetMetalContext`]) that the picked parts belong to;
//! 2. let the feature add to the model's definition (`SheetMetalContext::def`, a
//!    [`cadrs_sheetmetal::sharp_edit::SharpDef`]);
//! 3. build the model again, check it in 3D and flat (as the Sheet metal model does), fold it,
//!    and put its parts back under the same part ids (new flat-pattern parts get new ids, parts
//!    a joint merged away are gone);
//! 4. record the feature as the owner of the walls and joints it added, so their faces are named
//!    after it ("Edge of Flange 1") and keep those names while later features edit the model.

use super::sheetmetal::{Folded, face_index, flat_error, key_of, p3, v3};
use super::*;
use cadrs_sheetmetal::flat::PieceSource;
use cadrs_sheetmetal::model::{P3, V3};
use cadrs_sheetmetal::sharp_edit::{self, EdgeFrame, EdgePick, FlangeEdge, FlangeOpts, HemEdge, HemKind, HemOpts, JointSpec, PickSide};
use cadrs_sheetmetal::{Model, WallId, flatten};

use crate::applied::EdgeOrFace;
use crate::document::DirectionRef;
use crate::sheetmetal::{PieceKey, SheetMetalContext};
use crate::sheetmetal_features::{AngleControl, ChainType, Bound, FlangeEnd, FlangeFeature, HemFeature, MakeJointFeature, MakeJointType, SheetMetalFeature, SmTarget};

/// A picked edge or side face: its part, its points and a stable key.
struct Picked {
    part: PartId,
    pts: Vec<P3>,
    key: u64,
}

fn picked(state: &State, e: &EdgeOrFace) -> Option<Picked> {
    match e {
        EdgeOrFace::Edge(r) => {
            let (part, _) = super::applied::edge_ids(state, r)?;
            let solid = &part.part.solid;
            let (i, _) = solid.resolve_edge(&r.edge, |x| Some(x.distance(r.seed))).ok()?;
            let ed = &solid.edges[i];
            Some(Picked { part: part.part.id, pts: ed.points.iter().map(|p| p3(*p)).collect(), key: key_of(&ed.name) })
        }
        EdgeOrFace::Face(f) => {
            let (part, i) = face_index(state, f)?;
            let face = &part.part.solid.faces[i];
            Some(Picked { part: part.part.id, pts: face.loops.iter().flatten().map(|p| p3(*p)).collect(), key: key_of(&face.name) })
        }
    }
}

/// The walls and joints a definition has (for owners).
fn def_ids(ctx: &SheetMetalContext) -> Vec<PieceKey> {
    let Some(d) = &ctx.def else { return Vec::new() };
    let mut v: Vec<PieceKey> = d.builder.walls.iter().filter_map(|w| w.id).map(PieceKey::Wall).collect();
    v.extend(d.builder.joints.iter().filter_map(|j| j.id).map(PieceKey::Joint));
    for h in &d.builder.hems {
        v.extend(h.wall_id.map(PieceKey::Wall));
        v.extend(h.id.map(PieceKey::Joint));
    }
    v.extend(d.extra_walls.iter().map(|w| PieceKey::Wall(w.id)));
    v.extend(d.extra_joints.iter().map(|j| PieceKey::Joint(j.id)));
    v
}

impl Rebuilder {
    /// SM1.6: a feature after a Sheet metal model edits its definition (see the module docs).
    /// `part` is a part of the model; `edit` changes the context's definition (`ctx.def`,
    /// always `Some` when it runs) given the model as it was, and may return a warning.
    pub(in crate::rebuild) fn edit_sheet_metal(
        &mut self,
        id: FeatureId,
        state: &Arc<State>,
        part: PartId,
        edit: impl FnOnce(&mut SheetMetalContext) -> Result<Option<String>, String>,
    ) -> Result<Output, String> {
        let Some(old) = state.sheet_metal.iter().find(|c| c.parts.iter().any(|(p, _)| *p == part)) else {
            return Err("Select edges of a sheet metal part".into());
        };
        if !old.active {
            return Err("The sheet metal model is finished: it can't be changed".into());
        }
        if old.def.is_none() {
            return Err("Rebuild the Sheet metal model first".into());
        }
        let mut ctx = old.clone();
        let before_ids = def_ids(&ctx);
        let warning = edit(&mut ctx)?;
        for k in def_ids(&ctx) {
            if !before_ids.contains(&k) {
                ctx.owners.push((k, id));
            }
        }
        let def = ctx.def.as_ref().expect("checked");
        let mut model: Model = def.build().map_err(|e| e.message())?;
        model.fixed = old.model.fixed;
        model.corner_overrides = old.model.corner_overrides.clone();
        model.bend_relief_overrides = old.model.bend_relief_overrides.clone();
        if let Some(e) = model.validate().first() {
            return Err(format!("Sheet metal model is inconsistent: {}", e.message()));
        }
        let flat = flatten(&model);
        if let Some(why) = flat_error(&flat) {
            return Err(why);
        }
        let op = id.0;
        let ctx_ref = &ctx;
        let op_of = move |s: PieceSource| match s {
            PieceSource::Wall(w) => ctx_ref.owner(PieceKey::Wall(w)).0,
            PieceSource::Bend(j) => ctx_ref.owner(PieceKey::Joint(j)).0,
        };
        let folded: Vec<Folded> = self.fold(&op_of, op, &model, &flat)?;
        for (_, body, _, sum) in &folded {
            let v = self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(*sum);
            if v < sum - (1e-6 * sum + 1e-3) {
                for (_, b, _, _) in &folded {
                    self.kernel.release(*b);
                }
                return Err("Sheet metal walls intersect".into());
            }
        }
        // The model's parts again, under their ids where their walls carry on.
        let geoms = state.geoms.clone();
        let mut next = State { geoms: geoms.clone(), ..(**state).clone() };
        let mut used: Vec<PartId> = Vec::new();
        let mut placed: Vec<Placed> = Vec::new();
        let mut walls_of: Vec<(PartId, Vec<WallId>)> = Vec::new();
        let mut pieces_all = Vec::new();
        for (walls, body, names, _) in folded {
            let pieces = self.split(body, op, &[(body, &names)]);
            self.kernel.release(body);
            pieces_all.push((walls, pieces));
        }
        for (walls, pieces) in pieces_all {
            let pieces = pieces?;
            let keep = old.parts.iter().find(|(p, ws)| !used.contains(p) && ws.iter().any(|w| walls.contains(w))).map(|(p, _)| *p);
            for (n, pc) in pieces.into_iter().enumerate() {
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).collect();
                let pid = match keep {
                    Some(p) if n == 0 => p,
                    _ => Self::new_id(id, &next, &taken),
                };
                if n == 0 {
                    used.push(pid);
                    walls_of.push((pid, walls.clone()));
                }
                placed.push((pid, pc));
            }
        }
        next.parts.retain(|p| !old.parts.iter().any(|(q, _)| *q == p.part.id) || used.contains(&p.part.id));
        let mut o = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        ctx.parts = walls_of;
        ctx.model = model;
        ctx.flat = flat;
        let mut next = (*o.state).clone();
        let mut all = (*next.sheet_metal).clone();
        all.retain(|c| c.feature != ctx.feature);
        all.push(ctx);
        next.sheet_metal = Arc::new(all);
        o.state = Arc::new(next);
        o.warning = warning;
        Ok(o)
    }

    pub(in crate::rebuild) fn sheet_metal_feature(&mut self, before: &[Feature], id: FeatureId, x: &SheetMetalFeature, state: &Arc<State>) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let mut picks: Vec<Picked> = Vec::new();
        for e in x.entities() {
            picks.push(picked(state, e).ok_or("A selected edge or face no longer exists")?);
        }
        let part = picks[0].part;
        let salt = naming::stable_hash(id.0.as_bytes());
        for p in &mut picks {
            p.key ^= salt;
        }
        let ctx = state
            .sheet_metal
            .iter()
            .find(|c| c.parts.iter().any(|(p, _)| *p == part))
            .ok_or("Select edges of a sheet metal part")?;
        let model = ctx.model.clone();
        let mut eps: Vec<EdgePick> = Vec::new();
        for p in &picks {
            if !ctx.parts.iter().any(|(q, _)| *q == p.part) {
                return Err("The selected edges are on different sheet metal models".into());
            }
            eps.push(sharp_edit::locate(&model, &p.pts).ok_or("Select free edges or side faces of flat sheet metal walls")?);
        }
        enum Job {
            Flange(Vec<FlangeEdge>, FlangeOpts),
            Hem(Vec<HemEdge>, HemOpts),
            Joint(u64, JointSpec),
        }
        let job = match x {
            SheetMetalFeature::Flange(f) => {
                let edges = self.flange_edges(before, state, &model, f, &picks, &eps)?;
                let opts = FlangeOpts {
                    alignment: f.alignment,
                    radius: (!f.use_model_radius).then_some(f.radius),
                    miter: (!f.auto_miter).then_some(f.miter_angle.to_radians()),
                    hold_adjacent: f.hold_adjacent,
                    per_chain: f.partial && f.chain == ChainType::PerChain,
                };
                Job::Flange(edges, opts)
            }
            SheetMetalFeature::Hem(h) => {
                let edges = picks.iter().zip(&eps).map(|(p, e)| HemEdge { pick: *e, key: p.key, toward: default_toward(e.side) != h.flip }).collect();
                Job::Hem(edges, hem_opts(h, &model))
            }
            SheetMetalFeature::MakeJoint(j) => Job::Joint(picks[0].key ^ picks[1].key.rotate_left(17), make_joint_spec(j)),
        };
        self.edit_sheet_metal(id, state, part, move |ctx| {
            let def = ctx.def.as_mut().expect("checked");
            match job {
                Job::Flange(edges, opts) => {
                    sharp_edit::flange(def, &edges, &opts)?;
                }
                Job::Hem(edges, o) => {
                    sharp_edit::hem(def, &edges, &o)?;
                }
                Job::Joint(key, spec) => {
                    sharp_edit::make_joint(def, &eps[0], &eps[1], key, spec)?;
                }
            }
            Ok(None)
        })
    }

    /// Each flange edge's angle, side, distance and partial stretch.
    fn flange_edges(&self, before: &[Feature], state: &State, model: &Model, f: &FlangeFeature, picks: &[Picked], eps: &[EdgePick]) -> Result<Vec<FlangeEdge>, String> {
        let t = model.params.thickness;
        let r = if f.use_model_radius { model.params.bend_radius } else { f.radius };
        let mut out = Vec::new();
        for (p, e) in picks.iter().zip(eps) {
            let fr = EdgeFrame::of(model, e).ok_or("The edge isn't on a flat wall")?;
            let (angle, toward) = match f.angle_control {
                AngleControl::BendAngle => (f.angle.to_radians(), default_toward(e.side) != f.flip),
                AngleControl::AlignToGeometry => {
                    let d = f.parallel_to.as_ref().ok_or("Select what the flange is parallel to")?;
                    let g = v3(self.direction(before, state, d)?.into_inner().into());
                    // A line: along it; a plane: in it, across the edge.
                    let g = if matches!(d, DirectionRef::Edge(_) | DirectionRef::SketchLine { .. }) { g } else { fr.e.cross(&g) };
                    let g = if f.flip { -g } else { g };
                    let pick = |g: V3| fr.angle_of(g);
                    pick(g).or_else(|| pick(-g)).ok_or("The flange can't be parallel to that")?
                }
                AngleControl::AngleFromDirection => {
                    let d = f.direction.as_ref().ok_or("Select a direction")?;
                    let g = v3(self.direction(before, state, d)?.into_inner().into());
                    let g = g - fr.e * g.dot(&fr.e);
                    let g = g.try_normalize(1e-9).ok_or("The direction runs along the edge")?;
                    let a = f.direction_angle.to_radians() * if f.flip { -1.0 } else { 1.0 };
                    let k = fr.e;
                    let rot = g * a.cos() + k.cross(&g) * a.sin() + k * k.dot(&g) * (1.0 - a.cos());
                    fr.angle_of(rot).ok_or("At that angle the flange lies flat")?
                }
            };
            let distance = match f.end {
                FlangeEnd::Blind => f.distance,
                FlangeEnd::UpToEntity | FlangeEnd::UpToEntityOffset => {
                    let target = f.up_to.as_ref().ok_or("Select what the flange goes up to")?;
                    let s = fr.outer_sharp(f.alignment, angle, toward, r);
                    let dir = fr.flange_dir(angle, toward);
                    let d = self.distance_to(state, target, s, dir)?;
                    d + if f.end == FlangeEnd::UpToEntityOffset { f.offset } else { 0.0 }
                }
            };
            if !(distance > t * 1e-3) {
                return Err("The flange doesn't reach out from its edge".into());
            }
            let partial = if f.partial {
                let len = (e.b - e.a).norm();
                let at = |b: &Bound, from_start: bool| -> Result<f64, String> {
                    Ok(match b.kind {
                        FlangeEnd::Blind => b.distance,
                        _ => {
                            let target = b.up_to.as_ref().ok_or("Select what the bound goes up to")?;
                            let pt = self.point_of(state, target)?;
                            let s = (pt - e.a).dot(&fr.e).clamp(0.0, len);
                            let s = if from_start { s } else { len - s };
                            s + if b.kind == FlangeEnd::UpToEntityOffset { b.offset } else { 0.0 }
                        }
                    })
                };
                let first_from_start = !f.flip_sides;
                let d_first = at(&f.bound, first_from_start)?;
                let d_second = match &f.second {
                    Some(b) => at(b, !first_from_start)?,
                    None => 0.0,
                };
                Some(if first_from_start { (d_first, d_second) } else { (d_second, d_first) })
            } else {
                None
            };
            out.push(FlangeEdge { pick: *e, key: p.key, angle, toward, distance, partial });
        }
        Ok(out)
    }

    /// A target's point (a face's centre, an edge's middle, a vertex, a plane's origin).
    fn point_of(&self, state: &State, t: &SmTarget) -> Result<P3, String> {
        let lost = || "The entity to go up to no longer exists".to_string();
        Ok(match t {
            SmTarget::Vertex(v) => p3(v.point),
            SmTarget::Face(f) => {
                let (part, i) = face_index(state, f).ok_or_else(lost)?;
                let face = &part.part.solid.faces[i];
                p3(face.center.or_else(|| face.loops.first()?.first().copied()).ok_or_else(lost)?)
            }
            SmTarget::Edge(r) => {
                let (part, _) = super::applied::edge_ids(state, r).ok_or_else(lost)?;
                let solid = &part.part.solid;
                let (i, _) = solid.resolve_edge(&r.edge, |x| Some(x.distance(r.seed))).map_err(|_| lost())?;
                let pts = &solid.edges[i].points;
                let (a, b) = (p3(pts[0]), p3(*pts.last().ok_or_else(lost)?));
                P3::from((a.coords + b.coords) / 2.0)
            }
            SmTarget::Plane(pl) => p3(plane_of(state, pl).ok_or_else(lost)?.origin),
        })
    }

    /// How far along `dir` from `s` a flange runs to reach a target: to a plane (a planar face or
    /// a plane) where it crosses it, else to the target's point.
    fn distance_to(&self, state: &State, t: &SmTarget, s: P3, dir: V3) -> Result<f64, String> {
        let plane = match t {
            SmTarget::Plane(pl) => plane_of(state, pl),
            SmTarget::Face(f) => face_index(state, f).and_then(|(part, i)| part.part.solid.faces[i].plane),
            _ => None,
        };
        if let Some(pl) = plane {
            let m = v3(pl.normal()).normalize();
            let den = dir.dot(&m);
            if den.abs() < 1e-9 {
                return Err("The flange runs parallel to the entity: it never reaches it".into());
            }
            return Ok((p3(pl.origin) - s).dot(&m) / den);
        }
        Ok((self.point_of(state, t)? - s).dot(&dir))
    }
}

/// Which way a flange or hem turns by default: to the side of the face its edge was picked on
/// (a side face: towards the material).
fn default_toward(side: PickSide) -> bool {
    side != PickSide::Definition
}

fn hem_opts(h: &HemFeature, model: &Model) -> HemOpts {
    let p = &model.params;
    let radius = if h.kind == HemKind::Straight && h.flattened { p.minimal_gap / 2.0 } else { h.radius };
    HemOpts {
        kind: h.kind,
        radius,
        angle: h.angle.to_radians(),
        gap: if h.use_minimal_gap { p.minimal_gap } else { h.gap },
        total: h.total,
        alignment: h.alignment,
        closed: h.closed,
    }
}

fn make_joint_spec(j: &MakeJointFeature) -> JointSpec {
    match j.kind {
        MakeJointType::Rip => JointSpec::Rip(j.style),
        MakeJointType::Bend => JointSpec::Bend((!j.use_model_radius).then_some(j.radius)),
    }
}

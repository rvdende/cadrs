//! Rebuilding the sheet metal features that add walls and joints at the virtual sharps of an
//! active model (P3I.4: Flange, Hem, Make joint; `crate::sheetmetal_features`). Each finds the
//! active model its picks are on, locates the picked free edges on the built model, carries them
//! back onto the walls of the definition's sharp base (a later step such as a Bend may have
//! turned them; [`cadrs_sheetmetal::definition::Definition::pull_back`]), adds its walls and
//! joints there ([`cadrs_sheetmetal::sharp_edit`]) and refolds through the one sheet metal
//! pipeline ([`Rebuilder::edit_sheet_metal`], `sheetmetal/refold.rs`): the steps after the base
//! replay on the changed walls, the parts keep their ids, and the new walls and bends are named
//! after the feature ("Edge of Flange 1").

// NaN-safe checks: `!(x > 0.0)` is true for NaN too, which is what they mean.
#![allow(clippy::neg_cmp_op_on_partial_ord)]

use super::sheetmetal::refold::active_context;
use super::sheetmetal::{face_index, key_of, p3, v3};
use super::*;
use cadrs_sheetmetal::model::{P3, V3};
use cadrs_sheetmetal::sharp_edit::{self, EdgeFrame, EdgePick, FlangeEdge, FlangeOpts, HemEdge, HemKind, HemOpts, JointSpec, PickSide};
use cadrs_sheetmetal::Model;

use crate::applied::EdgeOrFace;
use crate::document::DirectionRef;
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

impl Rebuilder {
    pub(in crate::rebuild) fn sheet_metal_feature(&mut self, before: &[Feature], id: FeatureId, name: &str, x: &SheetMetalFeature, state: &Arc<State>) -> Result<Output, String> {
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
        let ci = active_context(state, part)?;
        let ctx = &state.sheet_metal[ci];
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
        self.edit_sheet_metal(id, name, state, ci, move |ctx| {
            let built = ctx.model.clone();
            let def = ctx.def.as_mut().expect("checked");
            // The picks on the built model, carried back onto the base's walls.
            let back = |e: &EdgePick| -> Result<EdgePick, String> {
                let pts = def.pull_back(&built, e.wall, &[e.a, e.b]).ok_or("Flange, Hem and Make joint work on the model's walls and those of flanges, not on walls a Bend, Jog, Tab, loft or pattern made")?;
                Ok(EdgePick { a: pts[0], b: pts[1], ..*e })
            };
            let not_sharp = "Flange, Hem and Make joint need a model made by Convert, Extrude or Thicken";
            match job {
                Job::Flange(edges, opts) => {
                    let edges: Vec<FlangeEdge> = edges.iter().map(|e| Ok(FlangeEdge { pick: back(&e.pick)?, ..*e })).collect::<Result<_, String>>()?;
                    sharp_edit::flange(def.sharp_mut().ok_or(not_sharp)?, &edges, &opts)?;
                }
                Job::Hem(edges, o) => {
                    let edges: Vec<HemEdge> = edges.iter().map(|e| Ok(HemEdge { pick: back(&e.pick)?, ..*e })).collect::<Result<_, String>>()?;
                    sharp_edit::hem(def.sharp_mut().ok_or(not_sharp)?, &edges, &o)?;
                }
                Job::Joint(key, spec) => {
                    let (a, b) = (back(&eps[0])?, back(&eps[1])?);
                    sharp_edit::make_joint(def.sharp_mut().ok_or(not_sharp)?, &a, &b, key, spec)?;
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
                    // Either way along it is parallel (θ to one side, 180° − θ to the other): the
                    // flange turns to the side of the face its edge was picked on, the arrow
                    // flips it (not the edge's or the line's own direction, which are arbitrary).
                    let side = default_toward(e.side) != f.flip;
                    let both = [fr.angle_of(g), fr.angle_of(-g)];
                    both.iter().flatten().find(|(_, t)| *t == side).or(both.iter().flatten().next()).copied().ok_or("The flange can't be parallel to that")?
                }
                AngleControl::AngleFromDirection => {
                    let d = f.direction.as_ref().ok_or("Select a direction")?;
                    let g = v3(self.direction(before, state, d)?.into_inner().into());
                    let g = g - fr.e * g.dot(&fr.e);
                    let g = g.try_normalize(1e-9).ok_or("The direction runs along the edge")?;
                    let a = f.direction_angle.to_radians() * if f.flip { -1.0 } else { 1.0 };
                    // The angle turns the direction away from the wall (towards the edge's outward
                    // direction), whichever way the edge itself runs.
                    let k = g.cross(&fr.out).try_normalize(1e-9).unwrap_or(fr.e);
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

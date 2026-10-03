//! The **one pipeline** through which a sheet metal model is built and every feature after it
//! changes it (SM1.6). A child of the Sheet metal model's rebuild, so it shares its helpers.
//!
//! The model's [`SheetMetalContext`] carries its [`Definition`]: the walls at their virtual sharps
//! (which Flange, Hem, Make joint and Modify joint edit) and the ordered steps the other features
//! add (Bend, Jog, Tab, cuts, corner breaks, face copies, reliefs, loft walls), plus the forms
//! placed on it. A feature changes the context ([`Rebuilder::edit_sheet_metal`]), then
//! [`Rebuilder::refold`] makes everything again from it:
//!
//! 1. the model: the definition built and its steps replayed (a step that no longer fits fails
//!    the feature, naming the step's feature), checked in 3D;
//! 2. the flat pattern, with the forms' outlines; a collision fails the feature but keeps the
//!    context, so the flat view shows where (SM1.5, X6);
//! 3. the folded bodies, one per flat-pattern part (walls extruded, loft walls as mitred slabs,
//!    bends as shells), faces named by the feature that made each wall or bend; walls that run
//!    into each other fail the feature; parts a loft joined are united;
//! 4. the forms' add and remove parts, placed on their walls (which may have moved since);
//! 5. the parts back **in place**: a folded part takes the id of the old part it shares walls
//!    with (so its name, material and the references of later features stay), parts no folded
//!    part continues are gone, new ones get new ids.
//!
//! So every combination and order of these features goes through the same code, and undo, redo,
//! reordering and rollback simply replay the features.

use super::*;
use cadrs_kernel::{Kernel, Motion};
use cadrs_sheetmetal::definition::Definition;
use cadrs_sheetmetal::flat::PieceSource;
use cadrs_sheetmetal::forms::{FlatForm, on_flat};

use crate::sheetmetal::{FormStep, PieceKey};
use crate::sheetmetal_form::{form_studio, tag_of};

/// Flat faces meeting at less than this (radians) are facets of one round (a polygonised arc's
/// steps: a relief circle's 15°, a corner break's under 7.5°, a mesh-cut bend's 2°): their
/// seams aren't drawn.
const FACET_SEAM_ANGLE: f64 = 0.45;

/// The stable key of a feature (a step's source, a form's flat key).
pub(in crate::rebuild) fn source_of(id: FeatureId) -> u64 {
    naming::stable_hash(id.0.as_bytes())
}

/// `state` with `ctx` as its model's context (and its flat pattern planes, P3I.6).
pub(in crate::rebuild) fn with_context(state: &State, ctx: SheetMetalContext) -> State {
    let mut next = state.clone();
    super::super::sheetmetal_flat::register_flat_planes(&mut next, &ctx);
    let mut all = (*next.sheet_metal).clone();
    match all.iter_mut().find(|c| c.feature == ctx.feature) {
        Some(c) => *c = ctx,
        None => all.push(ctx),
    }
    next.sheet_metal = Arc::new(all);
    next
}

/// The active model holding `part` (by index), or why there is none.
pub(in crate::rebuild) fn active_context(state: &State, part: PartId) -> Result<usize, String> {
    if let Some(i) = state.sheet_metal.iter().position(|c| c.active && c.parts.iter().any(|(p, _)| *p == part)) {
        return Ok(i);
    }
    Err(if state.sheet_metal.iter().any(|c| c.parts.iter().any(|(p, _)| *p == part)) {
        "The sheet metal model is finished: its parts are ordinary solids now".to_string()
    } else {
        "Select a face or edge of an active sheet metal part".to_string()
    })
}

/// An output with nothing but a state (and an error).
pub(in crate::rebuild) fn plain_output(state: State, error: Option<String>) -> Output {
    Output { state: Arc::new(state), error, warning: None, contacts: None, owned: Vec::new(), stage: None, axis: None, arrows: Vec::new(), dots: None, uses: Vec::new() }
}

/// A planar wall's frame (definition surface origin, u, v, normal) as a motion from the
/// world's.
fn wall_frame(model: &Model, wall: WallId) -> Option<Motion> {
    let w = model.wall(wall)?;
    let cadrs_sheetmetal::model::Surface::Planar { origin, u, v } = w.surface else { return None };
    let (u, v) = (u.normalize(), v.normalize());
    let n = u.cross(&v).normalize();
    Some(Motion { linear: nalgebra::Matrix3::from_columns(&[u, v, n]), translation: origin.coords })
}

/// `a` after `b`.
fn compose(a: &Motion, b: &Motion) -> Motion {
    Motion { linear: a.linear * b.linear, translation: a.linear * b.translation + a.translation }
}

/// A form copy's placement relative to its wall (its world placement `m` on `model`).
pub(in crate::rebuild) fn local_to_wall(model: &Model, wall: WallId, m: &Motion) -> Option<Motion> {
    let f = wall_frame(model, wall)?;
    let inv = Motion { linear: f.linear.transpose(), translation: -(f.linear.transpose() * f.translation) };
    Some(compose(&inv, m))
}

impl Rebuilder {
    /// SM1.6: feature `id` (named `label`) changes active model `ci`: `edit` changes its
    /// context (the definition, its forms; it may return a warning), then the model is refolded.
    pub(in crate::rebuild) fn edit_sheet_metal(
        &mut self,
        id: FeatureId,
        label: &str,
        state: &Arc<State>,
        ci: usize,
        edit: impl FnOnce(&mut SheetMetalContext) -> Result<Option<String>, String>,
    ) -> Result<Output, String> {
        let old = state.sheet_metal.get(ci).ok_or("The sheet metal model doesn't exist")?.clone();
        if !old.active {
            return Err("The sheet metal model is finished: it can't be changed".into());
        }
        if old.def.is_none() {
            return Err("The sheet metal model must be rebuilt before it can be changed".into());
        }
        let mut ctx = old.clone();
        let warning = edit(&mut ctx)?;
        if !ctx.editors.contains(&id) {
            ctx.editors.push(id);
        }
        let mut o = self.refold(id, label, state, &old.parts, Some(&old.model), ctx, &[])?;
        if o.warning.is_none() {
            o.warning = warning;
        }
        Ok(o)
    }

    /// Makes model `ctx` again from its definition (see the module docs): its parts take the
    /// place of `old_parts` (and of `consumed`, the parts a Convert used up). `old_model` is the
    /// model before this feature (its new walls and joints are the feature's own).
    #[allow(clippy::too_many_arguments)]
    pub(in crate::rebuild) fn refold(
        &mut self,
        id: FeatureId,
        label: &str,
        state: &Arc<State>,
        old_parts: &[(PartId, Vec<WallId>)],
        old_model: Option<&Model>,
        mut ctx: SheetMetalContext,
        consumed: &[PartId],
    ) -> Result<Output, String> {
        let def: Definition = ctx.def.clone().ok_or("The sheet metal model has no definition")?;
        let model = def.build().map_err(|e| e.message(label))?;
        // A loft's facet joints (planar walls edge to edge, mitred) aren't smooth by design.
        let facet = |e: &cadrs_sheetmetal::model::ModelError| {
            e.kind == cadrs_sheetmetal::model::ModelErrorKind::NotSmooth
                && model.joint(e.joint).is_some_and(|j| def.slabs.contains(&j.a) && def.slabs.contains(&j.b))
        };
        if let Some(e) = model.validate().iter().find(|e| !facet(e)) {
            return Err(format!("Sheet metal model is inconsistent: {}", e.message()));
        }
        let mut flat = flatten(&model);
        form_flats(&mut flat, &model, &ctx.forms);
        // The walls and joints this feature made are its own (their faces are named by it).
        if id != ctx.feature {
            let had = |k: PieceKey| match (old_model, k) {
                (Some(m), PieceKey::Wall(w)) => m.wall(w).is_some(),
                (Some(m), PieceKey::Joint(j)) => m.joint(j).is_some(),
                (None, _) => false,
            };
            let keys = model.walls.iter().map(|w| PieceKey::Wall(w.id)).chain(model.joints.iter().map(|j| PieceKey::Joint(j.id)));
            let new: Vec<PieceKey> = keys.filter(|k| !had(*k) && !ctx.owners.iter().any(|(x, _)| x == k)).collect();
            ctx.owners.extend(new.into_iter().map(|k| (k, id)));
        }
        ctx.model = model.clone();
        ctx.flat = flat.clone();
        if let Some(why) = flat_error(&flat) {
            return Ok(plain_output(with_context(state, ctx), Some(why)));
        }
        let op = id.0;
        let folded = {
            let c = &ctx;
            let op_of = move |s: PieceSource| match s {
                PieceSource::Wall(w) => c.owner(PieceKey::Wall(w)).0,
                PieceSource::Bend(j) => c.owner(PieceKey::Joint(j)).0,
            };
            self.fold(&op_of, op, &model, &flat, &def.slabs)?
        };
        // Walls running into each other in 3D: a fused body smaller than its pieces.
        for (_, body, _, sum) in &folded {
            let v = self.kernel.mass_properties(*body).map(|m| m.volume).unwrap_or(*sum);
            if v < sum - (1e-6 * sum + 1e-3) {
                for (_, b, _, _) in &folded {
                    self.kernel.release(*b);
                }
                return Ok(plain_output(with_context(state, ctx), Some("Sheet metal walls intersect".into())));
            }
        }
        let mut groups = self.merge_groups(op, folded, &def.merges)?;
        if let Err(e) = self.apply_forms(&mut groups, &model, &ctx.forms) {
            for (_, b, _) in &groups {
                self.kernel.release(*b);
            }
            return Err(e);
        }
        // The parts in place.
        let geoms = state.geoms.clone();
        let mut next = State { geoms: geoms.clone(), ..(**state).clone() };
        next.parts.retain(|p| !consumed.contains(&p.part.id));
        let mut used: Vec<PartId> = Vec::new();
        let mut placed: Vec<Placed> = Vec::new();
        let mut walls_of: Vec<(PartId, Vec<WallId>)> = Vec::new();
        let mut groups = groups.into_iter().enumerate();
        while let Some((k, (walls, body, names))) = groups.next() {
            let pieces = self.split(body, op, &[(body, &names)]);
            self.kernel.release(body);
            let mut pieces = match pieces {
                Ok(p) => p,
                Err(e) => {
                    for (_, (_, b, _)) in groups.by_ref() {
                        self.kernel.release(b);
                    }
                    for (_, pc) in placed {
                        self.kernel.release(pc.body);
                    }
                    return Err(e);
                }
            };
            pieces.sort_by(|a, b| b.volume.total_cmp(&a.volume));
            let reuse = old_parts.iter().find(|(p, ws)| !used.contains(p) && ws.iter().any(|w| walls.contains(w))).map(|(p, _)| *p);
            for (n, pc) in pieces.into_iter().enumerate() {
                let taken: Vec<PartId> = placed.iter().map(|(p, _)| *p).chain(used.iter().copied()).collect();
                let want = PartId::new(id, k as u32);
                let pid = match (n, reuse) {
                    (0, Some(p)) => p,
                    (0, None) if next.part(want).is_none() && !taken.contains(&want) => want,
                    _ => Self::new_id(id, &next, &taken),
                };
                if n == 0 {
                    used.push(pid);
                    walls_of.push((pid, walls.clone()));
                }
                placed.push((pid, pc));
            }
        }
        // The model's parts no folded part continues are gone.
        next.parts.retain(|p| !(old_parts.iter().any(|(q, _)| *q == p.part.id) && !used.contains(&p.part.id)));
        let mut o = self.finish(id, placed, next, op, geoms, PartKind::Solid)?;
        // The rounds' facets (reliefs, corner breaks, round holes, cuts across bends) draw no
        // edge lines between them.
        {
            let mut st = (*o.state).clone();
            for p in st.parts.iter_mut().filter(|p| walls_of.iter().any(|(q, _)| *q == p.part.id)) {
                let mut s = (*p.part.solid).clone();
                s.mark_facet_seams(FACET_SEAM_ANGLE);
                p.part.solid = Arc::new(s);
            }
            o.state = Arc::new(st);
        }
        ctx.parts = walls_of;
        o.state = Arc::new(with_context(&o.state, ctx));
        Ok(o)
    }

    /// The folded flat-pattern parts as parts: those a definition's `merges` join become one
    /// body (united), the others stay as they are. Each with its walls, body and names.
    fn merge_groups(&mut self, op: cadrs_kernel::OpId, folded: Vec<Folded>, merges: &[(WallId, WallId)]) -> Result<Vec<(Vec<WallId>, BodyId, BodyNames)>, String> {
        let n = folded.len();
        let mut group: Vec<usize> = (0..n).collect();
        let find = |g: &[usize], mut i: usize| {
            while g[i] != i {
                i = g[i];
            }
            i
        };
        for (a, b) in merges {
            let ia = folded.iter().position(|(ws, ..)| ws.contains(a));
            let ib = folded.iter().position(|(ws, ..)| ws.contains(b));
            if let (Some(ia), Some(ib)) = (ia, ib) {
                let (ra, rb) = (find(&group, ia), find(&group, ib));
                if ra != rb {
                    group[ra.max(rb)] = ra.min(rb);
                }
            }
        }
        let roots: Vec<usize> = (0..n).map(|i| find(&group, i)).collect();
        let mut items: Vec<Option<Folded>> = folded.into_iter().map(Some).collect();
        let mut out = Vec::new();
        for r in 0..n {
            if roots[r] != r {
                continue;
            }
            let members: Vec<Folded> = (0..n).filter(|i| roots[*i] == r).filter_map(|i| items[i].take()).collect();
            if members.len() == 1 {
                let (w, b, nm, _) = members.into_iter().next().expect("one");
                out.push((w, b, nm));
                continue;
            }
            let walls: Vec<WallId> = members.iter().flat_map(|(w, ..)| w.iter().copied()).collect();
            let first = members[0].1;
            let rest: Vec<BodyId> = members[1..].iter().map(|(_, b, _, _)| *b).collect();
            let r = self.kernel.boolean(BoolOp::Union, first, &rest).and_then(|res| {
                let body = res.bodies[0];
                let inputs: Vec<(BodyId, &BodyNames)> = members.iter().map(|(_, b, nm, _)| (*b, nm)).collect();
                Ok((body, naming::name_body(&self.kernel, body, op, &res.history, &inputs)?))
            });
            for (_, b, _, _) in &members {
                self.kernel.release(*b);
            }
            match r {
                Ok((b, nm)) => out.push((walls, b, nm)),
                Err(e) => {
                    for (_, b, _) in &out {
                        self.kernel.release(*b);
                    }
                    return Err(format!("Couldn't join the sheet metal parts: {e}"));
                }
            }
        }
        Ok(out)
    }

    /// The forms' add and remove parts on the folded bodies (each copy on the body with its
    /// wall), in the order the forms were placed.
    fn apply_forms(&mut self, groups: &mut [(Vec<WallId>, BodyId, BodyNames)], model: &Model, forms: &[FormStep]) -> Result<(), String> {
        for step in forms {
            let studio = form_studio(&step.pick, &step.variables, step.thickness)?;
            let tag = tag_of(&studio).cloned().ok_or_else(|| format!("{} has no Tag (Form) feature", step.pick.name))?;
            let (_, sstate) = self.sub_build(&studio)?;
            let op = step.feature.0;
            // By index: each group's body is replaced while `self` is borrowed for the kernel.
            #[allow(clippy::needless_range_loop)]
            for gi in 0..groups.len() {
                let copies: Vec<(usize, Motion)> = step
                    .copies
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| groups[gi].0.contains(&c.wall))
                    .filter_map(|(k, c)| Some((k, compose(&wall_frame(model, c.wall)?, &c.local))))
                    .collect();
                if copies.is_empty() {
                    continue;
                }
                let mut made: Vec<BodyId> = Vec::new();
                let release = |this: &mut Self, v: &[BodyId]| {
                    for b in v {
                        this.kernel.release(*b);
                    }
                };
                let mut adds: Vec<(BodyId, BodyNames)> = Vec::new();
                let mut removes: Vec<(BodyId, BodyNames)> = Vec::new();
                for (ci, m) in &copies {
                    for (list, parts, off) in [(&mut adds, &tag.add, 0), (&mut removes, &tag.remove, 4)] {
                        for (pi, p) in parts.iter().enumerate() {
                            let Some(sp) = sstate.part(*p) else { continue };
                            let Some(sbody) = sp.body else { continue };
                            let r = match self.kernel.transform_motion(sbody, m) {
                                Ok(r) => r,
                                Err(e) => {
                                    release(self, &made);
                                    return Err(format!("Placing {} failed: {e}", step.name));
                                }
                            };
                            let b = r.bodies[0];
                            made.push(b);
                            let names = match self.instance_names(b, &r.history, &sp.names, op, (ci * 8 + pi + off) as u32) {
                                Ok(n) => n,
                                Err(e) => {
                                    release(self, &made);
                                    return Err(e);
                                }
                            };
                            list.push((b, names));
                        }
                    }
                }
                for (kind, tools) in [(BoolOp::Union, &adds), (BoolOp::Subtract, &removes)] {
                    if tools.is_empty() {
                        continue;
                    }
                    let (cur, cur_names) = (groups[gi].1, groups[gi].2.clone());
                    let tb: Vec<BodyId> = tools.iter().map(|(b, _)| *b).collect();
                    let r = self.kernel.boolean(kind, cur, &tb).map_err(|e| format!("{} failed: {e}", step.name)).and_then(|r| {
                        let nb = r.bodies[0];
                        let mut inputs: Vec<(BodyId, &BodyNames)> = vec![(cur, &cur_names)];
                        inputs.extend(tools.iter().map(|(b, n)| (*b, n)));
                        let names = naming::name_body(&self.kernel, nb, op, &r.history, &inputs).map_err(|e| e.to_string())?;
                        Ok((nb, names))
                    });
                    match r {
                        Ok((nb, names)) => {
                            self.kernel.release(cur);
                            groups[gi].1 = nb;
                            groups[gi].2 = names;
                        }
                        Err(e) => {
                            release(self, &made);
                            return Err(e);
                        }
                    }
                }
                release(self, &made);
            }
        }
        Ok(())
    }
}

/// The forms' outlines and centermarks on the flat-pattern parts of their walls.
fn form_flats(flat: &mut cadrs_sheetmetal::FlatPattern, model: &Model, forms: &[FormStep]) {
    for step in forms {
        for c in &step.copies {
            let Some(pi) = flat.parts.iter().position(|p| p.walls.contains(&c.wall)) else { continue };
            let Some((center, lines)) = on_flat(&flat.parts[pi], model, c.wall, c.center, &c.lines) else { continue };
            flat.parts[pi].forms.push(FlatForm { source: source_of(step.feature), name: step.name.clone(), form: step.pick.name.clone(), wall: c.wall, center, lines, up: c.up });
        }
    }
}

//! Assembly structure (P3B.4, `intro-to-assemblies.md` A16.2, A16.3, A17, X9): **subassembly**
//! instances, and the commands that restructure an assembly.
//!
//! **Occurrences.** An instance of another Assembly tab ([`InstanceSource::Assembly`]) stands for
//! every part in that tab (at any depth). Each such part is an [`Occurrence`] of this assembly,
//! with an id of its own: [`derive`]`(subassembly instance, its id in the tab)` (a top-level part
//! instance is its own occurrence). Mate connectors name occurrences, so a mate of this assembly
//! can hold a part inside a subassembly (a Move to new subassembly leaves such mates behind).
//! In the view every occurrence of a subassembly instance `S` is the part `PartId(S, k)`, `k ≥ 1`,
//! so a pick on it is a pick of `S` ([`InstanceId::of_part`]).
//!
//! **Rigid and flexible** (A16.2). A rigid subassembly (the default) moves as one: its parts keep
//! the placements of its tab. A flexible one's mates act in this assembly, so its parts move as
//! they would in its tab; their placements here are kept on the instance
//! ([`Instance::overrides`]). A **Fix** inside the subassembly is not carried into this
//! assembly (A3.7, A16.3): only this assembly's own fixed instances hold ([`solver_model`]). A
//! flexible subassembly that is fixed (or grouped) here is held by its base: the parts fixed in
//! its tab (else its first part); the rest keeps its motion.
//!
//! **Restructuring** (A17.3–A17.5): [`MoveToNewSubassembly`] (also Create empty subassembly),
//! [`MoveIntoSubassembly`] (drop rows onto a subassembly row), [`MoveOutOfSubassembly`] (drag
//! rows out) and [`DissolveSubassembly`]. Each keeps every part where it is in the world, takes
//! along the mates between the instances it moves, re-targets the mates that now cross a level,
//! and is one undo step ([`Scope::Whole`]: it edits two tabs).

use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::commands::assembly_mut;
use super::connector::{ConnectorFrame, MateConnector};
use super::mate::{Mate, MateFeature, MateId, MateKind, MateType, next_name};
use super::{Assembly, Instance, InstanceId, InstanceSource, Pose};
use crate::command::{Command, CommandError, Scope};
use crate::document::{Document, Element};
use crate::ids::{ElementId, FeatureId, PartId};

/// How deep subassemblies may nest (a guard: cycles are refused when inserting).
const MAX_DEPTH: usize = 16;

/// The id, in an assembly, of the occurrence `inner` (an id in the tab of the subassembly
/// instance `outer`) of `outer`. Deterministic, so the same part keeps its id.
pub fn derive(outer: InstanceId, inner: InstanceId) -> InstanceId {
    let a = outer.0.as_u128();
    let b = inner.0.as_u128();
    let mixed = a.rotate_left(29) ^ b.wrapping_mul(0x9e37_79b9_7f4a_7c15_f39c_c060_5ced_c835) ^ 0x5b4e_d3a1_0000_0000_0000_0000_0000_0001;
    InstanceId(Uuid::from_u128(mixed))
}

/// A part placed in an assembly, at any depth.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// Its id in the assembly (see [`derive`]).
    pub id: InstanceId,
    /// The top-level instance it belongs to.
    pub top: InstanceId,
    /// For a part inside a subassembly: the subassembly tab's instance it belongs to.
    pub child: Option<InstanceId>,
    pub element: ElementId,
    pub part: PartId,
    /// Its `<n>` (in the tab it is an instance of).
    pub index: u32,
    /// Its placement in the assembly.
    pub pose: Pose,
    /// Its placement in the coordinates of `top`.
    pub in_top: Pose,
    /// Its placement in the coordinates of `child` (identity for a part instance).
    pub in_child: Pose,
    pub hidden: bool,
    /// `top` is fixed.
    pub fixed: bool,
    /// Its instance in the subassembly's tab (`child`) is fixed there: the subassembly's base.
    pub base: bool,
    /// Its part in the view: `top`'s own part for a part instance, else `PartId(top, k)`.
    pub view_part: PartId,
}

/// Every part of `asm` at any depth, in list order (suppressed instances and missing tabs left
/// out).
pub fn occurrences(doc: &Document, asm: &Assembly) -> Vec<Occurrence> {
    occurrences_at(doc, asm, 0)
}

fn occurrences_at(doc: &Document, asm: &Assembly, depth: usize) -> Vec<Occurrence> {
    let mut out = Vec::new();
    for inst in asm.instances.iter().filter(|i| !i.suppressed) {
        match inst.source {
            InstanceSource::Part { element, part } => out.push(Occurrence {
                id: inst.id,
                top: inst.id,
                child: None,
                element,
                part,
                index: inst.index,
                pose: inst.pose,
                in_top: Pose::IDENTITY,
                in_child: Pose::IDENTITY,
                hidden: inst.hidden,
                fixed: inst.fixed,
                base: false,
                view_part: inst.id.part_id(),
            }),
            InstanceSource::Studio { element } => {
                for (k, part) in inst.parts.iter().enumerate() {
                    out.push(Occurrence {
                        id: derive(inst.id, studio_part_key(*part)),
                        top: inst.id,
                        child: None,
                        element,
                        part: *part,
                        index: inst.index,
                        pose: inst.pose,
                        in_top: Pose::IDENTITY,
                        in_child: Pose::IDENTITY,
                        hidden: inst.hidden,
                        fixed: inst.fixed,
                        base: false,
                        view_part: PartId::new(FeatureId(inst.id.0), k as u32 + 1),
                    });
                }
            }
            InstanceSource::Assembly { element } => {
                if depth >= MAX_DEPTH {
                    continue;
                }
                let Some(child) = doc.element(element).and_then(|e| e.assembly_model()) else { continue };
                // A16.2: a rigid subassembly following a Named position of its tab.
                let followed = inst.follow.filter(|_| !inst.flexible).and_then(|f| child.named_position(f));
                for (k, o) in occurrences_at(doc, child, depth + 1).into_iter().enumerate() {
                    let Some(c) = child.instance(o.top) else { continue };
                    let placed = followed.and_then(|np| np.pose_of(c.id)).unwrap_or_else(|| inst.child_pose(c));
                    let in_top = o.in_top.then(&placed);
                    out.push(Occurrence {
                        id: derive(inst.id, o.id),
                        top: inst.id,
                        child: Some(o.top),
                        element: o.element,
                        part: o.part,
                        index: o.index,
                        pose: in_top.then(&inst.pose),
                        in_top,
                        in_child: o.in_top,
                        hidden: inst.hidden || o.hidden,
                        fixed: inst.fixed,
                        base: c.fixed,
                        view_part: PartId::new(FeatureId(inst.id.0), k as u32 + 1),
                    });
                }
            }
        }
    }
    out
}

/// The id a part of a rigid Part Studio instance has in it (its occurrence is
/// [`derive`]`(instance, key)`).
pub fn studio_part_key(part: PartId) -> InstanceId {
    InstanceId(Uuid::from_u128(part.feature.0.as_u128() ^ (part.index as u128 + 1).wrapping_mul(0x2545_f491_4f6c_dd1d)))
}

/// True if the Assembly tab `container` is `target` or has it inside, at any depth.
pub fn contains_assembly(doc: &Document, container: ElementId, target: ElementId) -> bool {
    fn walk(doc: &Document, el: ElementId, target: ElementId, depth: usize) -> bool {
        if el == target {
            return true;
        }
        if depth > MAX_DEPTH {
            return true;
        }
        let Some(asm) = doc.element(el).and_then(|e| e.assembly_model()) else { return false };
        asm.instances.iter().any(|i| match i.source {
            InstanceSource::Assembly { element } => walk(doc, element, target, depth + 1),
            _ => false,
        })
    }
    walk(doc, container, target, 0)
}

/// Every instance id a mate feature names (connectors, tabs, group members) mapped by `f`.
pub fn map_instances(feature: &mut MateFeature, f: &dyn Fn(InstanceId) -> InstanceId) {
    match &mut feature.kind {
        MateKind::Mate(m) => {
            for c in m.connectors.iter_mut().chain(m.tabs.iter_mut()) {
                c.instance = f(c.instance);
            }
        }
        MateKind::Group { instances } => {
            for i in instances.iter_mut() {
                *i = f(*i);
            }
        }
        MateKind::Relation(_) | MateKind::Variable(_) => {}
        MateKind::Replicate(r) => {
            r.seed = f(r.seed);
            for i in r.instances.iter_mut() {
                *i = f(*i);
            }
            for c in r.targets.iter_mut() {
                c.instance = f(c.instance);
            }
        }
    }
}

/// The assembly the solver sees: one instance per [`Occurrence`] (at its placement; fixed when
/// its top-level instance is), this assembly's mates (a group holding a subassembly holds all of
/// its parts), a group per **rigid** subassembly (its parts move as one), and the mates of each
/// **flexible** subassembly (their connectors re-named to occurrences here). The subassemblies'
/// own Fix is not carried (A3.7). For a document without subassemblies it is the assembly
/// itself (without its suppressed instances).
pub fn solver_model(doc: &Document, asm: &Assembly) -> Assembly {
    solver_model_at(doc, asm, 0)
}

fn solver_model_at(doc: &Document, asm: &Assembly, depth: usize) -> Assembly {
    let occ = occurrences_at(doc, asm, depth);
    let ids: HashSet<InstanceId> = occ.iter().map(|o| o.id).collect();
    let leaves = |top: InstanceId| -> Vec<InstanceId> { occ.iter().filter(|o| o.top == top).map(|o| o.id).collect() };
    // What holds a subassembly (its Fix here, a group here): all of it when rigid; when
    // flexible, its base (the parts fixed in its tab, else its first), so the rest keeps its
    // motion.
    let flexible: HashSet<InstanceId> = asm.instances.iter().filter(|i| i.flexible && i.source.is_assembly()).map(|i| i.id).collect();
    let base = |top: InstanceId| -> Vec<InstanceId> {
        let all: Vec<&Occurrence> = occ.iter().filter(|o| o.top == top).collect();
        if !flexible.contains(&top) {
            return all.iter().map(|o| o.id).collect();
        }
        let fixed: Vec<InstanceId> = all.iter().filter(|o| o.base).map(|o| o.id).collect();
        if fixed.is_empty() { all.first().map(|o| vec![o.id]).unwrap_or_default() } else { fixed }
    };
    let instances: Vec<Instance> = occ
        .iter()
        .map(|o| {
            let mut i = Instance::new(o.id, InstanceSource::Part { element: o.element, part: o.part }, o.pose);
            i.index = o.index;
            i.hidden = o.hidden;
            i.fixed = o.fixed && (o.top == o.id || base(o.top).contains(&o.id));
            i
        })
        .collect();
    let mut mates: Vec<MateFeature> = Vec::new();
    for f in &asm.mates {
        let mut f = f.clone();
        if let MateKind::Group { instances } = &mut f.kind {
            *instances = instances.iter().flat_map(|i| base(*i)).collect();
        }
        mates.push(f);
    }
    for inst in asm.instances.iter().filter(|i| i.source.is_composite() && !i.suppressed) {
        if !inst.flexible || inst.source.is_studio() {
            let l = leaves(inst.id);
            if l.len() >= 2 {
                let id = MateId(derive(inst.id, InstanceId(Uuid::from_u128(0x5ab))).0);
                let name = if inst.source.is_studio() { "Rigid Part Studio" } else { "Rigid subassembly" };
                mates.push(MateFeature::new(id, name, MateKind::Group { instances: l }));
            }
            continue;
        }
        let Some(child) = doc.element(inst.source.element()).and_then(|e| e.assembly_model()) else { continue };
        if depth >= MAX_DEPTH {
            continue;
        }
        for mut f in solver_model_at(doc, child, depth + 1).mates {
            let s = inst.id;
            map_instances(&mut f, &|x| derive(s, x));
            f.id = MateId(derive(s, InstanceId(f.id.0)).0);
            if let MateKind::Relation(r) = &mut f.kind {
                for m in &mut r.mates {
                    *m = MateId(derive(s, InstanceId(m.0)).0);
                }
            }
            mates.push(f);
        }
    }
    // Mates to the Origin hold at this level only (A16.3: not carried into a parent).
    let origin_ok = depth == 0;
    mates.retain_mut(|f| match &mut f.kind {
        MateKind::Mate(m) => m.all_connectors().all(|c| ids.contains(&c.instance) || (origin_ok && c.is_origin())),
        MateKind::Group { instances } => {
            instances.retain(|i| ids.contains(i));
            instances.dedup();
            instances.len() >= 2
        }
        MateKind::Replicate(r) => ids.contains(&r.seed) && r.instances.iter().all(|i| ids.contains(i)) && r.targets.iter().all(|c| ids.contains(&c.instance)),
        MateKind::Relation(_) | MateKind::Variable(_) => true,
    });
    // P3B.9: a relation holds while its mates do.
    let kept: Vec<MateId> = mates.iter().map(|f| f.id).collect();
    mates.retain(|f| f.relation().is_none_or(|r| r.mates.iter().all(|m| kept.contains(m))));
    // The assembly's own explicit connectors, for the mates on them (this level only).
    let connectors = if depth == 0 { asm.connectors.clone() } else { Vec::new() };
    Assembly { instances, mates, connectors, ..Default::default() }
}

/// Places occurrences (the solver's placements, by occurrence id) in the assembly `element`: a
/// top-level instance takes its pose; a part of a rigid subassembly moves the subassembly; a part
/// of a flexible one is placed within it (an override of its tab's placement).
pub(crate) fn place(doc: &mut Document, element: ElementId, poses: &[(InstanceId, Pose)]) -> Result<(), CommandError> {
    if poses.is_empty() {
        return Ok(());
    }
    let mut asm = assembly_of(doc, element)?;
    place_in(doc, &mut asm, poses)?;
    store(doc, element, asm)
}

/// [`place`] on a copy of an assembly of `doc` (a manipulator's preview, drawn without an undo
/// step).
pub fn place_in(doc: &Document, asm: &mut Assembly, poses: &[(InstanceId, Pose)]) -> Result<(), CommandError> {
    let occ: HashMap<InstanceId, Occurrence> = occurrences(doc, asm).into_iter().map(|o| (o.id, o)).collect();
    let mut done: HashSet<InstanceId> = HashSet::new();
    for (id, pose) in poses {
        if let Some(i) = asm.instance_mut(*id) {
            i.pose = *pose;
            continue;
        }
        let o = occ.get(id).ok_or_else(|| CommandError::Invalid(format!("instance {id} not found")))?;
        let Some(s) = asm.instance_mut(o.top) else { continue };
        if !s.flexible {
            if done.insert(s.id) {
                s.pose = o.in_top.inverse().then(pose);
            }
            continue;
        }
        let Some(child) = o.child else { continue };
        // The part's placement in the subassembly, and so its own instance's there.
        let in_top = pose.then(&s.pose.inverse());
        let child_pose = o.in_child.inverse().then(&in_top);
        match s.overrides.iter_mut().find(|(c, _)| *c == child) {
            Some(slot) => slot.1 = child_pose,
            None => s.overrides.push((child, child_pose)),
        }
    }
    Ok(())
}

/// Every id of `ids` is a top-level instance of `asm`.
fn check_top(asm: &Assembly, ids: &[InstanceId]) -> Result<(), CommandError> {
    match ids.iter().find(|i| asm.instance(**i).is_none()) {
        Some(i) => Err(CommandError::Invalid(format!("instance {i} not found"))),
        None => Ok(()),
    }
}

fn assembly_of(doc: &Document, element: ElementId) -> Result<Assembly, CommandError> {
    doc.element(element)
        .ok_or(CommandError::ElementNotFound(element))?
        .assembly_model()
        .cloned()
        .ok_or_else(|| CommandError::Invalid("not an assembly".into()))
}

fn store(doc: &mut Document, element: ElementId, asm: Assembly) -> Result<(), CommandError> {
    *assembly_mut(doc, element)? = asm;
    Ok(())
}

/// A connector at `a`'s origin frame, and one at the same place on `b` (both occurrences of the
/// same assembly, at the placements given): a Fastened mate between them holds them as they are.
fn fastened_between(a: (InstanceId, Pose), b: (InstanceId, Pose)) -> Mate {
    let here = ConnectorFrame::default();
    let on_b = here.moved(&a.1).moved(&b.1.inverse());
    Mate::new(MateType::Fastened, MateConnector::at(a.0, here), MateConnector::at(b.0, on_b))
}

/// Where an instance of `asm` can take a connector: itself (a part), else its first part.
fn anchor(occ: &[Occurrence], asm: &Assembly, id: InstanceId) -> Option<(InstanceId, Pose)> {
    let inst = asm.instance(id)?;
    if !inst.source.is_composite() {
        return Some((id, inst.pose));
    }
    occ.iter().find(|o| o.top == id).map(|o| (o.id, o.pose))
}


/// The next `<n>` of `inst`'s source in `asm`, keeping `inst`'s own when it is free.
fn free_index(asm: &Assembly, inst: &Instance) -> u32 {
    if asm.instances.iter().any(|i| i.source == inst.source && i.index == inst.index) {
        asm.next_index(&inst.source)
    } else {
        inst.index
    }
}

/// Moves the top-level instances `ids` of `parent_el` into its subassembly instance `sub`
/// (Move to new subassembly, a drop on a subassembly row). Everything keeps its world placement.
fn move_into(doc: &mut Document, parent_el: ElementId, sub: InstanceId, ids: &[InstanceId]) -> Result<(), CommandError> {
    let mut parent = assembly_of(doc, parent_el)?;
    check_top(&parent, ids)?;
    if ids.contains(&sub) {
        return Err(CommandError::Invalid("a subassembly can't go into itself".into()));
    }
    let s = parent.instance(sub).cloned().ok_or_else(|| CommandError::Invalid("subassembly not found".into()))?;
    let InstanceSource::Assembly { element: child_el } = s.source else {
        return Err(CommandError::Invalid("not a subassembly".into()));
    };
    let mut child = assembly_of(doc, child_el)?;
    // A17.2: an assembly can't contain itself.
    for id in ids {
        if let Some(InstanceSource::Assembly { element }) = parent.instance(*id).map(|i| i.source)
            && contains_assembly(doc, element, child_el)
        {
            return Err(CommandError::Invalid("an assembly can't contain itself".into()));
        }
    }
    let parent_occ = occurrences(doc, &parent);
    let child_occ = occurrences(doc, &child);
    // The moved instances' occurrences keep their ids in the subassembly.
    let moved_occ: HashSet<InstanceId> = parent_occ.iter().filter(|o| ids.contains(&o.top)).map(|o| o.id).collect();
    let old_inside: HashMap<InstanceId, InstanceId> = child_occ.iter().map(|o| (derive(sub, o.id), o.id)).collect();
    let to_child = |c: InstanceId| -> Option<InstanceId> {
        if moved_occ.contains(&c) { Some(c) } else { old_inside.get(&c).copied() }
    };
    let had_children = !child.instances.is_empty();
    let first_old = child.instances.first().map(|i| i.id);
    // The instances, in list order, placed in the subassembly's coordinates.
    let to_sub = s.pose.inverse();
    let mut moved: Vec<Instance> = Vec::new();
    parent.instances.retain(|i| {
        if ids.contains(&i.id) {
            moved.push(i.clone());
            false
        } else {
            true
        }
    });
    // Numbered afresh in the subassembly, per source in list order (`ex3-step16.png`: the Top
    // Cap subassembly lists O-Ring 0.125 <1>, <2>, not the <3>, <4> they were at the top).
    for mut m in moved.clone() {
        m.pose = m.pose.then(&to_sub);
        m.index = child.next_index(&m.source);
        child.instances.push(m);
    }
    // The mates.
    let mut new_child_mates: Vec<MateFeature> = Vec::new();
    let mut fasten: Vec<(InstanceId, InstanceId)> = Vec::new();
    let mut keep: Vec<MateFeature> = Vec::new();
    for mut f in std::mem::take(&mut parent.mates) {
        match &mut f.kind {
            MateKind::Mate(m) => {
                let all_inside = m.all_connectors().all(|c| to_child(c.instance).is_some());
                let any_moved = m.all_connectors().any(|c| moved_occ.contains(&c.instance));
                if all_inside && any_moved {
                    map_instances(&mut f, &|c| to_child(c).unwrap_or(c));
                    new_child_mates.push(f);
                } else {
                    map_instances(&mut f, &|c| if moved_occ.contains(&c) { derive(sub, c) } else { c });
                    keep.push(f);
                }
            }
            // A Replicate stays at this level.
            MateKind::Replicate(_) => {
                map_instances(&mut f, &|c| if moved_occ.contains(&c) { derive(sub, c) } else { c });
                keep.push(f);
            }
            // P3B.9: a relation goes with its mates (below).
            MateKind::Relation(_) | MateKind::Variable(_) => keep.push(f),
            MateKind::Group { instances } => {
                let inside: Vec<InstanceId> = instances.iter().copied().filter(|i| ids.contains(i)).collect();
                if inside.is_empty() {
                    keep.push(f);
                    continue;
                }
                // The moved members stay together inside (a Fastened replaces the group there,
                // A21.5); the subassembly takes their place in the group here.
                let with_sub = instances.contains(&sub);
                for w in inside.windows(2) {
                    fasten.push((w[0], w[1]));
                }
                if with_sub && had_children && let Some(old) = first_old {
                    fasten.push((old, inside[0]));
                }
                instances.retain(|i| !ids.contains(i));
                if !instances.contains(&sub) {
                    instances.push(sub);
                }
                if instances.len() >= 2 {
                    keep.push(f);
                }
            }
        }
    }
    // P3B.9: a relation whose mates all went into the subassembly goes there too.
    let moved_mates: Vec<MateId> = new_child_mates.iter().map(|f| f.id).collect();
    let (with, keep): (Vec<MateFeature>, Vec<MateFeature>) =
        keep.into_iter().partition(|f| f.relation().is_some_and(|r| r.mates.iter().all(|m| moved_mates.contains(m))));
    parent.mates = keep;
    parent.drop_orphan_relations();
    // In the subassembly's list the moved mates come as Onshape lists them (`ex3-step8.png`:
    // Fastened 3, Fastened 4, Revolute 1): the fastened ones first, then by how much motion
    // they allow, each kind in its old order.
    new_child_mates.sort_by_key(|f| f.mate().and_then(|m| m.mate_type.dof_count()).unwrap_or(0));
    child.mates.extend(new_child_mates);
    child.mates.extend(with);
    if !fasten.is_empty() {
        let child_occ_now = occurrences(doc, &child);
        for (a, b) in fasten {
            let (Some(a), Some(b)) = (anchor(&child_occ_now, &child, a), anchor(&child_occ_now, &child, b)) else { continue };
            let mut all = parent.mates.clone();
            all.extend(child.mates.iter().cloned());
            let name = next_name(&all, "Fastened");
            let id = MateId(derive(sub, InstanceId::from_u128(0xfa57_0000 + child.mates.len() as u128)).0);
            child.mates.push(MateFeature::new(id, name, MateKind::Mate(fastened_between(a, b))));
        }
    }
    super::folders::tidy(&mut parent);
    super::folders::tidy(&mut child);
    store(doc, parent_el, parent)?;
    store(doc, child_el, child)
}

/// Moves the instances `ids` (ids in the subassembly's tab) of the subassembly instance `sub`
/// out to `parent_el`, placed at `at` in its list (by default after `sub`). Everything keeps its
/// world placement; a Fix inside is not carried out (they take `sub`'s).
fn move_out(doc: &mut Document, parent_el: ElementId, sub: InstanceId, ids: &[InstanceId], at: Option<usize>) -> Result<(), CommandError> {
    let mut parent = assembly_of(doc, parent_el)?;
    let s = parent.instance(sub).cloned().ok_or_else(|| CommandError::Invalid("subassembly not found".into()))?;
    let InstanceSource::Assembly { element: child_el } = s.source else {
        return Err(CommandError::Invalid("not a subassembly".into()));
    };
    let mut child = assembly_of(doc, child_el)?;
    check_top(&child, ids)?;
    if ids.iter().any(|i| parent.instance(*i).is_some()) {
        return Err(CommandError::Invalid("instance id already in use".into()));
    }
    let child_occ = occurrences(doc, &child);
    let moved_occ: HashSet<InstanceId> = child_occ.iter().filter(|o| ids.contains(&o.top)).map(|o| o.id).collect();
    let back: HashMap<InstanceId, InstanceId> = moved_occ.iter().map(|x| (derive(sub, *x), *x)).collect();
    // The instances.
    let mut out: Vec<Instance> = Vec::new();
    for id in ids {
        let c = child.instance(*id).cloned().ok_or_else(|| CommandError::Invalid("instance not found".into()))?;
        let mut i = c.clone();
        i.pose = s.child_pose(&c).then(&s.pose);
        i.fixed = s.fixed;
        i.index = free_index(&parent, &i);
        out.push(i);
    }
    child.instances.retain(|i| !ids.contains(&i.id));
    let at = at.unwrap_or_else(|| parent.instances.iter().position(|i| i.id == sub).map(|p| p + 1).unwrap_or(parent.instances.len()));
    let at = at.min(parent.instances.len());
    for (k, i) in out.into_iter().enumerate() {
        parent.instances.insert(at + k, i);
    }
    if let Some(si) = parent.instance_mut(sub) {
        si.overrides.retain(|(c, _)| !ids.contains(c));
    }
    // This assembly's mates on the moved parts name them directly now.
    for f in &mut parent.mates {
        map_instances(f, &|c| back.get(&c).copied().unwrap_or(c));
    }
    // The subassembly's mates that hold a moved instance come out with it.
    let mut stay: Vec<MateFeature> = Vec::new();
    for mut f in std::mem::take(&mut child.mates) {
        match &mut f.kind {
            MateKind::Mate(m) => {
                if m.all_connectors().any(|c| moved_occ.contains(&c.instance)) {
                    map_instances(&mut f, &|c| if moved_occ.contains(&c) { c } else { derive(sub, c) });
                    parent.mates.push(f);
                } else {
                    stay.push(f);
                }
            }
            MateKind::Replicate(_) => {
                map_instances(&mut f, &|c| if moved_occ.contains(&c) { c } else { derive(sub, c) });
                stay.push(f);
            }
            MateKind::Relation(_) | MateKind::Variable(_) => stay.push(f),
            MateKind::Group { instances } => {
                let leaving: Vec<InstanceId> = instances.iter().copied().filter(|i| ids.contains(i)).collect();
                if leaving.is_empty() {
                    stay.push(f);
                    continue;
                }
                instances.retain(|i| !ids.contains(i));
                let mut outside = leaving;
                if !instances.is_empty() {
                    outside.push(sub);
                }
                if outside.len() >= 2 {
                    let id = MateId(derive(sub, InstanceId(f.id.0)).0);
                    parent.mates.push(MateFeature::new(id, f.name.clone(), MateKind::Group { instances: outside }));
                }
                if instances.len() >= 2 {
                    stay.push(f);
                }
            }
        }
    }
    child.mates = stay;
    // P3B.9: a relation whose mates came out comes out too.
    let out_ids: Vec<MateId> = parent.mates.iter().map(|f| f.id).collect();
    let (out, stay): (Vec<MateFeature>, Vec<MateFeature>) =
        std::mem::take(&mut child.mates).into_iter().partition(|f| f.relation().is_some_and(|r| r.mates.iter().all(|m| out_ids.contains(m))));
    child.mates = stay;
    parent.mates.extend(out);
    child.drop_orphan_relations();
    super::folders::tidy(&mut parent);
    super::folders::tidy(&mut child);
    store(doc, parent_el, parent)?;
    store(doc, child_el, child)
}

/// **Move to new subassembly** (A17.3, A21.4) and **Create empty subassembly** (A21.6): a new
/// Assembly tab (right of `element`'s) holding `instances` and the mates between them, and an
/// instance of it in `element` at the first moved instance's place (or after `after`). Every
/// part keeps its world placement. One undo step.
#[derive(Debug, Clone)]
pub struct MoveToNewSubassembly {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    /// The new tab.
    pub new_element: ElementId,
    /// Its instance in `element`.
    pub instance: InstanceId,
    /// The tab's name (`None`: the next "Assembly n").
    pub name: Option<String>,
    /// Where an empty subassembly goes: after this instance (else at the end).
    pub after: Option<InstanceId>,
}

impl Command for MoveToNewSubassembly {
    fn label(&self) -> String {
        if self.instances.is_empty() { "Create empty subassembly".into() } else { "Move to new subassembly".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if doc.element(self.new_element).is_some() {
            return Err(CommandError::Invalid("element id already in use".into()));
        }
        let parent = assembly_of(doc, self.element)?;
        check_top(&parent, &self.instances)?;
        if parent.instance(self.instance).is_some() {
            return Err(CommandError::Invalid("instance id already in use".into()));
        }
        let name = match &self.name {
            Some(n) if !n.trim().is_empty() => n.trim().to_string(),
            _ => doc.next_element_name("Assembly"),
        };
        let mut tab = Element::assembly(name);
        tab.id = self.new_element;
        let at = doc.element_index(self.element).map(|i| i + 1).unwrap_or(doc.elements.len());
        doc.elements.insert(at, tab);
        // The subassembly instance, where the first moved instance is.
        let source = InstanceSource::Assembly { element: self.new_element };
        let mut inst = Instance::new(self.instance, source, Pose::IDENTITY);
        inst.index = parent.next_index(&source);
        let pos = parent
            .instances
            .iter()
            .position(|i| self.instances.contains(&i.id))
            .or_else(|| self.after.and_then(|a| parent.instances.iter().position(|i| i.id == a)).map(|p| p + 1))
            .unwrap_or(parent.instances.len());
        assembly_mut(doc, self.element)?.instances.insert(pos, inst);
        if !self.instances.is_empty() {
            move_into(doc, self.element, self.instance, &self.instances)?;
        }
        let parent = assembly_mut(doc, self.element)?;
        super::folders::tidy(parent);
        Ok(())
    }
}

/// Drops top-level instances onto a subassembly row (A17.4, A21.7): they move into it, with the
/// mates between them and its parts. One undo step.
#[derive(Debug, Clone)]
pub struct MoveIntoSubassembly {
    pub element: ElementId,
    pub sub: InstanceId,
    pub instances: Vec<InstanceId>,
}

impl Command for MoveIntoSubassembly {
    fn label(&self) -> String {
        "Move into subassembly".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.instances.is_empty() {
            return Err(CommandError::Invalid("no instances".into()));
        }
        move_into(doc, self.element, self.sub, &self.instances)
    }
}

/// Drags instances of an expanded subassembly back to the top level (A17.4, A21.11). `at`: the
/// place in the list (by default after the subassembly). One undo step.
#[derive(Debug, Clone)]
pub struct MoveOutOfSubassembly {
    pub element: ElementId,
    pub sub: InstanceId,
    /// Their ids in the subassembly's tab.
    pub instances: Vec<InstanceId>,
    pub at: Option<usize>,
}

impl Command for MoveOutOfSubassembly {
    fn label(&self) -> String {
        "Move out of subassembly".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        if self.instances.is_empty() {
            return Err(CommandError::Invalid("no instances".into()));
        }
        move_out(doc, self.element, self.sub, &self.instances, self.at)
    }
}

/// **Dissolve subassembly** (A17.5, A21.12): its instances and mates come back to `element`
/// where it was, and it goes; its (now empty) tab stays. One undo step.
#[derive(Debug, Clone)]
pub struct DissolveSubassembly {
    pub element: ElementId,
    pub sub: InstanceId,
}

impl Command for DissolveSubassembly {
    fn label(&self) -> String {
        "Dissolve subassembly".into()
    }
    fn scope(&self) -> Scope {
        Scope::Whole
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let parent = assembly_of(doc, self.element)?;
        let s = parent.instance(self.sub).cloned().ok_or_else(|| CommandError::Invalid("subassembly not found".into()))?;
        let child_el = match s.source {
            InstanceSource::Assembly { element } => element,
            _ => return Err(CommandError::Invalid("not a subassembly".into())),
        };
        let kids: Vec<InstanceId> = assembly_of(doc, child_el)?.instances.iter().map(|i| i.id).collect();
        let at = parent.instances.iter().position(|i| i.id == self.sub);
        if !kids.is_empty() {
            move_out(doc, self.element, self.sub, &kids, at)?;
        }
        let parent = assembly_mut(doc, self.element)?;
        parent.instances.retain(|i| i.id != self.sub);
        // A group that held the subassembly holds its instances now.
        let sub = self.sub;
        parent.mates.retain_mut(|f| match &mut f.kind {
            MateKind::Group { instances } => {
                if let Some(p) = instances.iter().position(|i| *i == sub) {
                    instances.remove(p);
                    for k in &kids {
                        if !instances.contains(k) {
                            instances.push(*k);
                        }
                    }
                }
                instances.len() >= 2
            }
            MateKind::Mate(_) | MateKind::Replicate(_) | MateKind::Relation(_) | MateKind::Variable(_) => true,
        });
        super::folders::tidy(parent);
        Ok(())
    }
}

/// Makes subassembly instances flexible or rigid (the lock icon, A16.2). Rigid again, they take
/// their tab's placements.
#[derive(Debug, Clone)]
pub struct SetSubassemblyFlexible {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    pub flexible: bool,
}

impl Command for SetSubassemblyFlexible {
    fn label(&self) -> String {
        if self.flexible { "Make flexible".into() } else { "Make rigid".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        check_top(asm, &self.instances)?;
        for i in asm.instances.iter_mut().filter(|i| self.instances.contains(&i.id)) {
            if !i.source.is_assembly() {
                return Err(CommandError::Invalid("only a subassembly can be flexible".into()));
            }
            i.flexible = self.flexible;
            if !self.flexible {
                i.overrides.clear();
            } else {
                // Flexible: its mates act here; it follows no Named position (A16.2).
                i.follow = None;
            }
        }
        Ok(())
    }
}

/// Suppresses or unsuppresses instances (X2, the instance menu's Suppress): a suppressed instance
/// stays in the list, greyed, and is not drawn, measured or solved.
#[derive(Debug, Clone)]
pub struct SetInstancesSuppressed {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    pub suppressed: bool,
}

impl Command for SetInstancesSuppressed {
    fn label(&self) -> String {
        if self.suppressed { "Suppress instances".into() } else { "Unsuppress instances".into() }
    }
    fn scope(&self) -> Scope {
        Scope::Element(self.element)
    }
    fn apply(&self, doc: &mut Document) -> Result<(), CommandError> {
        let asm = assembly_mut(doc, self.element)?;
        check_top(asm, &self.instances)?;
        for i in asm.instances.iter_mut().filter(|i| self.instances.contains(&i.id)) {
            i.suppressed = self.suppressed;
        }
        Ok(())
    }
}

/// How the top-level assembly is held (A16.3, the root row's icon): some instance fixed, or none.
pub fn root_fixed(asm: &Assembly) -> bool {
    asm.instances.iter().any(|i| i.fixed && !i.suppressed)
}

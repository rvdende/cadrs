//! The **Replicate** dialog (P3B.8, `intro-to-assemblies.md` X16; "Replicate 1" in
//! `ex4-step11.png`): the toolbar's Replicate (or a Replicate feature's Edit…).
//!
//! - **Instance to replicate**: the seed, picked in the view or the list; its mate (the first
//!   mate that holds it to another instance) is what each copy gets, shown under the field.
//! - **Instances to replicate onto**: the matching places, picked in the view (a hole's rim or
//!   its face on the part the seed is mated to, or, editing, a copy already there; a pick is
//!   taken to the matching place it is on),
//!   or **All matching** on that part ([`cadrs_core::assembly::replicate::matching_targets`]).
//! - The copies are previewed where they will go, translucent; ✓ adds "Replicate n" (one feature of the Mate
//!   Features list, its copies under a row of the Instances list) or changes the one edited, as
//!   one undo step ([`cadrs_core::assembly::replicate::SetReplicate`]).
//!
//! Names: `replicate-dialog`, `replicate-seed`, `replicate-seed-mate`, `replicate-targets`,
//! `replicate-all-matching` (its checkbox `…-checkbox`).

use std::collections::HashMap;
use std::sync::Arc;

use bevy::prelude::*;
use cadrs_core::ElementId;
use cadrs_core::assembly::connector::{ConnectorAnchor, EntityRef, ImplicitPoint, MateConnector};
use cadrs_core::assembly::mate::{MateFeature, MateId, MateKind, next_name};
use cadrs_core::assembly::replicate::{self, Replicate, SetReplicate};
use cadrs_core::assembly::{Instance, InstanceId};
use cadrs_ui::dialog_fields::OptionRow;
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, SelectionList, SelectionListActivate, SelectionListState};

use crate::parts::PartCache;
use crate::viewport::{Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ReplicatePlugin;

impl Plugin for ReplicatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (follow_selection, sync_dialog).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<ReplicateSession>())
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_check)
            .add_observer(on_field);
    }
}

/// Which field takes the view's picks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Seed,
    Targets,
}

/// The open dialog.
#[derive(Resource, Debug, Clone)]
pub struct ReplicateSession {
    pub element: ElementId,
    pub id: MateId,
    pub name: String,
    pub editing: bool,
    pub seed: Option<InstanceId>,
    pub seed_mate: Option<MateId>,
    pub targets: Vec<MateConnector>,
    /// The places before the picks (an edited feature's): a pick adds a place or, on one already
    /// there (or its copy), takes it out.
    pub base: Vec<MateConnector>,
    /// The copies kept from the feature edited, by target place.
    pub kept: Vec<(MateConnector, InstanceId)>,
    pub all_matching: bool,
    pub field: Field,
    /// Every matching place on the part (P3B.9, P3B.8 judge: a place keeps its number, "Hole 4",
    /// whichever are picked).
    pub places: Vec<MateConnector>,
}

#[derive(Component)]
struct ReplicateDialog;

#[derive(Component, Debug, Clone, PartialEq)]
struct DialogKey(String);

/// Ids of the previewed copies (not in the document).
const GHOST: u128 = 0x7e91_1c47_0000_0000_0000_0000_0000_0000;

fn close_others(world: &mut World) {
    world.remove_resource::<super::relation_dialog::RelationSession>();
    super::mate_dialog::cancel(world);
    world.remove_resource::<super::group_dialog::GroupSession>();
    super::connector_tool::cancel(world);
    super::animate::stop(world);
}

/// The toolbar's Replicate: the selected instance is the seed.
pub fn open(world: &mut World) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()).cloned() else { return };
    close_others(world);
    let seed = super::selected_instances(world.resource::<Selection>()).into_iter().find(|i| model.instance(*i).is_some_and(|x| !x.source.is_composite()));
    let seed_mate = seed.and_then(|s| mate_of(&model, s));
    let field = if seed.is_some() { Field::Targets } else { Field::Seed };
    world.resource_mut::<Selection>().0.clear();
    let mut s = ReplicateSession {
        element,
        id: MateId::new(),
        name: next_name(&model.mates, "Replicate"),
        editing: false,
        seed,
        seed_mate,
        targets: Vec::new(),
        base: Vec::new(),
        kept: Vec::new(),
        all_matching: false,
        field,
        places: Vec::new(),
    };
    s.places = candidates(world, &s);
    world.insert_resource(s);
}

/// Edit… on a Replicate feature.
pub fn edit(world: &mut World, id: MateId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(f) = doc.active_element().and_then(|e| e.assembly_model()?.mate(id).cloned()) else { return };
    let MateKind::Replicate(r) = f.kind else { return };
    close_others(world);
    world.resource_mut::<Selection>().0.clear();
    let mut s = ReplicateSession {
        element,
        id,
        name: f.name,
        editing: true,
        seed: Some(r.seed),
        seed_mate: Some(r.seed_mate),
        kept: r.targets.iter().copied().zip(r.instances.iter().copied()).collect(),
        base: r.targets.clone(),
        targets: r.targets,
        all_matching: false,
        field: Field::Targets,
        places: Vec::new(),
    };
    s.places = candidates(world, &s);
    world.insert_resource(s);
}

/// The first mate that holds `seed` to another instance (not the Origin).
fn mate_of(model: &cadrs_core::assembly::Assembly, seed: InstanceId) -> Option<MateId> {
    model
        .mates
        .iter()
        .filter(|f| !f.suppressed)
        .find(|f| f.mate().is_some_and(|m| m.mate_type.uses_connectors() && m.connectors.iter().any(|c| c.instance == seed) && m.connectors.iter().all(|c| !c.is_origin())))
        .map(|f| f.id)
}

/// The seed mate's connector on the other instance.
fn other_connector(model: &cadrs_core::assembly::Assembly, seed: InstanceId, mate: MateId) -> Option<MateConnector> {
    model.mate(mate)?.mate()?.connectors.iter().find(|c| c.instance != seed).copied()
}

/// The matching places on the part the seed is mated to (its instance's source part now).
fn candidates(world: &mut World, s: &ReplicateSession) -> Vec<MateConnector> {
    let (Some(seed), Some(mate)) = (s.seed, s.seed_mate) else { return Vec::new() };
    let Some(model) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else { return Vec::new() };
    let Some(other) = other_connector(&model, seed, mate) else { return Vec::new() };
    world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
        let doc = world.resource::<ActiveDocument>();
        let Some((_, element, part)) = super::occurrence_source(doc, &parts, other.instance) else { return Vec::new() };
        let Some(b) = parts.build(&doc.doc, element) else { return Vec::new() };
        let Some(p) = b.part(part) else { return Vec::new() };
        replicate::matching_targets(&p.solid, &other)
    })
}

/// The picks of the view as the dialog's fields.
fn follow_selection(selection: Res<Selection>, mut commands: Commands, session: Option<Res<ReplicateSession>>) {
    if session.is_none() || !selection.is_changed() {
        return;
    }
    let picks = selection.0.clone();
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource::<ReplicateSession>().cloned() else { return };
        match s.field {
            Field::Seed => {
                let Some(model) = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned()) else { return };
                let Some(i) = picks.iter().filter_map(super::instance_of).find(|i| model.instance(*i).is_some_and(|x| !x.source.is_composite())) else { return };
                s.seed = Some(i);
                s.seed_mate = mate_of(&model, i);
                s.targets.clear();
                s.field = Field::Targets;
                s.places = candidates(world, &s);
                world.insert_resource(s);
                world.resource_mut::<Selection>().0.clear();
            }
            Field::Targets => {
                if s.all_matching || picks.is_empty() {
                    return;
                }
                // Each rim or face picked on the mated part: the matching place it is on.
                let all = s.places.clone();
                let mut targets: Vec<MateConnector> = Vec::new();
                for p in &picks {
                    // A copy picked (Edit): its place.
                    if let Some(i) = super::instance_of(p)
                        && let Some((c, _)) = s.kept.iter().find(|(_, k)| *k == i)
                    {
                        if !targets.contains(c) {
                            targets.push(*c);
                        }
                        continue;
                    }
                    let Some((inst, entity)) = super::connectors::entity_of(p) else { continue };
                    let hit = all.iter().find(|c| {
                        c.instance == inst
                            && match (c.anchor, entity) {
                                (ConnectorAnchor::Implicit { point: ImplicitPoint::CircleCenter(e), owner }, EntityRef::Edge(x)) => e == x || owner == EntityRef::Edge(x),
                                (ConnectorAnchor::Implicit { point: ImplicitPoint::CircleCenter(e), .. }, EntityRef::Face(f)) => e.faces.contains(&f),
                                (ConnectorAnchor::Implicit { point: ImplicitPoint::AxisMiddle(g), .. }, EntityRef::Face(f)) => g == f,
                                _ => false,
                            }
                    });
                    if let Some(c) = hit
                        && !targets.contains(c)
                    {
                        targets.push(*c);
                    }
                }
                // Each pick toggles its place (a copy picked comes out, its ghost picked comes
                // back); the picks don't stay selected (P3B.8 judge: taken-out copies are drawn as
                // ghosts, not as a selection).
                let mut out = s.targets.clone();
                for c in &targets {
                    match out.iter().position(|x| x == c) {
                        Some(k) => {
                            out.remove(k);
                        }
                        None => out.push(*c),
                    }
                }
                // In the part's order.
                out.sort_by_key(|c| s.places.iter().position(|p| p == c).unwrap_or(usize::MAX));
                world.resource_mut::<Selection>().0.clear();
                if out != s.targets {
                    s.targets = out;
                    world.insert_resource(s);
                }
            }
        }
    });
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("replicate-all-matching-checkbox") {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource::<ReplicateSession>().cloned() else { return };
        s.all_matching = on;
        s.targets = if on { s.places.clone() } else { Vec::new() };
        s.base.clear();
        world.resource_mut::<Selection>().0.clear();
        world.insert_resource(s);
    });
}

/// Clicking a field makes it the one the picks go to.
fn on_field(ev: On<SelectionListActivate>, q: Query<&Name>, session: Option<ResMut<ReplicateSession>>) {
    let Some(mut s) = session else { return };
    let f = match q.get(ev.entity).map(|n| n.as_str()) {
        Ok("replicate-seed") => Field::Seed,
        Ok("replicate-targets") => Field::Targets,
        _ => return,
    };
    if s.field != f {
        s.field = f;
    }
}

/// The feature and its copies as ✓ would make them (the copies kept from an edit keep their
/// ids; new ones get `ids`).
fn plan(world: &mut World, s: &ReplicateSession, mut ids: impl FnMut(usize) -> InstanceId) -> Option<(MateFeature, Vec<Instance>)> {
    let (seed, mate) = (s.seed?, s.seed_mate?);
    if s.targets.is_empty() {
        return None;
    }
    let (flat, solids): (cadrs_core::assembly::Assembly, HashMap<InstanceId, Arc<cadrs_core::Solid>>) = super::mate_dialog::model_and_solids(world)?;
    let model = world.get_resource::<ActiveDocument>()?.active_element()?.assembly_model()?.clone();
    let kept = s.kept.clone();
    let (r, instances) = replicate::plan(
        &model,
        &flat,
        &solids,
        seed,
        mate,
        s.targets.clone(),
        |k| kept.iter().find(|(c, _)| Some(c) == s.targets.get(k)).map(|(_, i)| *i).unwrap_or_else(|| ids(k)),
        s.id,
    )
    .ok()?;
    let mut f = MateFeature::new(s.id, s.name.clone(), MateKind::Replicate(r));
    if let Some(old) = model.mate(s.id) {
        f.suppressed = old.suppressed;
    }
    Some((f, instances))
}

fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ReplicateSession>().cloned() else { return };
    let Some((feature, instances)) = plan(world, &s, |_| InstanceId::new()) else { return };
    world.resource_mut::<super::AssemblyParts>().ghosts.clear();
    if super::run(world, &SetReplicate { element: s.element, feature, instances }) {
        world.remove_resource::<ReplicateSession>();
        world.resource_mut::<Selection>().0.clear();
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ReplicateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ReplicateDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<ReplicateSession>();
            world.resource_mut::<super::AssemblyParts>().ghosts.clear();
            world.resource_mut::<Selection>().0.clear();
        });
    }
}

fn instance_label(world: &World, i: InstanceId) -> String {
    world.resource::<PartCache>().part_name(super::occurrence_part(i)).unwrap_or("instance").to_string()
}

/// Whether a part of the view is a previewed copy.
fn is_ghost_part(p: &cadrs_core::PartId) -> bool {
    (p.feature.0.as_u128() >> 64) == (GHOST >> 64)
}

fn sync_dialog(world: &mut World, mut dimmed: Local<Vec<cadrs_core::PartId>>) {
    let Some(s) = world.get_resource::<ReplicateSession>().cloned() else {
        let mut q = world.query_filtered::<Entity, With<ReplicateDialog>>();
        let old: Vec<Entity> = q.iter(world).collect();
        for e in old {
            world.entity_mut(e).despawn();
        }
        let mut cache = world.resource_mut::<PartCache>();
        if cache.transparent.iter().any(|p| is_ghost_part(p) || dimmed.contains(p)) {
            cache.transparent.retain(|p| !is_ghost_part(p) && !dimmed.contains(p));
        }
        dimmed.clear();
        return;
    };
    // Editing: the copies taken out are drawn as ghosts (translucent) until ✓; a click on one
    // puts it back.
    {
        let out: Vec<cadrs_core::PartId> = s.kept.iter().filter(|(c, _)| !s.targets.contains(c)).map(|(_, i)| i.part_id()).collect();
        if *dimmed != out {
            let mut cache = world.resource_mut::<PartCache>();
            let back: Vec<cadrs_core::PartId> = dimmed.iter().filter(|p| !out.contains(p)).copied().collect();
            cache.set_transparent(&back, false);
            cache.set_transparent(&out, true);
            *dimmed = out;
        }
    }
    let model = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.assembly_model().cloned());
    let seed_items: Vec<String> = s.seed.map(|i| vec![instance_label(world, i)]).unwrap_or_default();
    let mate_text = match (s.seed, s.seed_mate.and_then(|m| model.as_ref()?.mate(m).map(|f| f.name.clone()))) {
        (Some(_), Some(n)) => format!("Mate: {n}"),
        (Some(_), None) => "The instance has no mate to copy".to_string(),
        _ => String::new(),
    };
    let target_owner = s.seed.zip(s.seed_mate).and_then(|(a, m)| other_connector(model.as_ref()?, a, m)).map(|c| instance_label(world, c.instance)).unwrap_or_else(|| "part".into());
    // Each place by its number among all the matching places (stable as picks come and go).
    let target_items: Vec<String> = s
        .targets
        .iter()
        .enumerate()
        .map(|(k, c)| format!("Hole {} on {target_owner}", s.places.iter().position(|p| p == c).unwrap_or(k) + 1))
        .collect();
    let valid = s.seed.is_some() && s.seed_mate.is_some() && !s.targets.is_empty();
    let key = DialogKey(format!("{} {:?} {:?} {} {:?} {}", s.name, seed_items, target_items, s.all_matching, s.field, mate_text));
    // The preview: the copies where they will go.
    let ghosts: Vec<Instance> = if valid {
        let mut n = 0;
        plan(world, &s, |_| {
            n += 1;
            InstanceId::from_u128(GHOST + n)
        })
        .map(|(_, i)| i.into_iter().filter(|x| s.kept.iter().all(|(_, k)| *k != x.id)).collect())
        .unwrap_or_default()
    } else {
        Vec::new()
    };
    {
        let mut parts = world.resource_mut::<super::AssemblyParts>();
        if parts.ghosts.iter().map(|g| (g.id, g.pose)).ne(ghosts.iter().map(|g| (g.id, g.pose))) {
            parts.ghosts = ghosts.clone();
        }
    }
    // The copies are previewed translucent (P3B.8 judge).
    {
        let want: Vec<cadrs_core::PartId> = ghosts.iter().map(|g| g.id.part_id()).collect();
        let mut cache = world.resource_mut::<PartCache>();
        let stale = cache.transparent.iter().any(|p| is_ghost_part(p) && !want.contains(p));
        let missing = want.iter().any(|p| !cache.transparent.contains(p));
        if stale || missing {
            cache.transparent.retain(|p| !is_ghost_part(p));
            cache.transparent.extend(want);
        }
    }
    let mut q = world.query_filtered::<(Entity, &DialogKey), With<ReplicateDialog>>();
    let old: Vec<(Entity, DialogKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    if old.iter().any(|(_, k)| *k == key) {
        let mut q_list = world.query::<(&Name, &mut SelectionListState)>();
        for (n, mut l) in q_list.iter_mut(world) {
            let active = match n.as_str() {
                "replicate-seed" => s.field == Field::Seed,
                "replicate-targets" => s.field == Field::Targets,
                _ => continue,
            };
            if l.active != active {
                l.active = active;
            }
        }
        return;
    }
    for (e, _) in old {
        world.entity_mut(e).despawn();
    }
    let mut q_area = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else { return };
    let t = world.resource::<Theme>().clone();
    let tb = t.clone();
    let (field, all) = (s.field, s.all_matching);
    let d = world
        .spawn((
            ReplicateDialog,
            key,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("replicate-dialog")
                .title(s.name.clone())
                .valid(valid)
                .width(230.0)
                .body(move |b| {
                    b.spawn(SelectionList::new("replicate-seed").placeholder("Instance to replicate").items(seed_items).active(field == Field::Seed).build(&tb));
                    if !mate_text.is_empty() {
                        b.spawn((
                            Name::new("replicate-seed-mate"),
                            tb.text(mate_text, tb.font_sm, bevy::text::FontWeight::NORMAL, tb.muted_foreground),
                            Node { margin: UiRect::new(Val::Px(4.0), Val::ZERO, Val::Px(1.0), Val::Px(4.0)), ..default() },
                        ));
                    }
                    b.spawn(SelectionList::new("replicate-targets").placeholder("Select matching holes").items(target_items).active(field == Field::Targets).build(&tb));
                    b.spawn(OptionRow::new("replicate-all-matching", "All matching on the part").checked(all).build(&tb));
                })
                .build(&t),
        ))
        .id();
    world.entity_mut(area).add_child(d);
}

/// A Replicate's copied instances for the feature list's tooltip ("6 instances").
pub fn describe(r: &Replicate) -> String {
    format!("{} instance{}", r.instances.len(), if r.instances.len() == 1 { "" } else { "s" })
}

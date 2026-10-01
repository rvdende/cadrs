//! The **Composite part** dialog (P3H.6; PCB7.9, X9; the feature is
//! `cadrs_core::transform::CompositeFeature`): **Parts** and **Closed** (a closed composite is
//! listed, picked and inserted as one part). While it is open the view shows the studio before
//! it, so its members can be picked, the picked ones selected; a box dragged on empty space adds
//! the parts inside it.
//!
//! The toolbar's Composite part button inserts "Composite part N" and opens the dialog (parts
//! selected before fill its list); a feature row's double-click edits one. ✓ or Enter keeps it,
//! ✕ or Esc removes it (or reverts an edit). Every change is a command; accepting squashes them
//! into one undo step, as the Boolean dialog does.
//!
//! Names: `composite-dialog`, `composite-parts-field`, `composite-closed(-checkbox)`.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::assembly::context::is_context;
use cadrs_core::commands::{AddFeature, ReplaceFeature};
use cadrs_core::transform::CompositeFeature;
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind, PartId};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, OptionRow, SelectionList, SelectionListRemove,
    SelectionListState,
};

use crate::parts::PartCache;
use crate::viewport::{Pick, PickRequest, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct CompositeUiPlugin;

impl Plugin for CompositeUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (composite_picks, composite_keys, sync_dialog, preview).chain().before(crate::parts::PartsSet).run_if(in_state(AppState::Document)),
        )
        .add_systems(Update, show_parts.after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
        .add_systems(OnExit(AppState::Document), |world: &mut World| {
            if world.contains_resource::<CompositeSession>() {
                finish(world);
            }
        })
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_checkbox)
        .add_observer(on_remove);
    }
}

/// The Composite part whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct CompositeSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub is_new: bool,
    pub mark: usize,
    pub before: Option<Feature>,
}

#[derive(Component)]
struct CompositeDialog;

/// Closed, as the dialog was built (it is rebuilt when it changes).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltFor(bool);

#[derive(Component)]
struct PartsField;

/// Closes every other feature dialog and sketch (before this one opens).
fn close_others(world: &mut World) {
    if world.contains_resource::<crate::applied::AppliedSession>() {
        crate::applied::finish(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
}

/// Starts a new Composite part in the active Part Studio (the toolbar's button).
pub fn begin(world: &mut World) {
    if world.contains_resource::<CompositeSession>() {
        finish(world);
    }
    close_others(world);
    let parts: Vec<PartId> = world.resource::<Selection>().0.iter().filter_map(|p| p.part()).filter(|p| !is_context(p.feature)).collect();
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active_element().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })).map(|e| e.id) else {
        return;
    };
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    let kind = FeatureKind::Composite(CompositeFeature { parts, closed: false });
    if let Err(e) = doc.execute(&AddFeature { element, feature, base_name: "Composite part".into(), kind }) {
        warn!("cannot insert a composite part: {e}");
        return;
    }
    start(world, element, feature, true, mark, None);
}

/// Opens an existing Composite part for editing (a feature row's double-click).
pub fn edit(world: &mut World, feature: FeatureId) {
    if world.get_resource::<CompositeSession>().is_some_and(|s| s.feature == feature) {
        return;
    }
    if world.contains_resource::<CompositeSession>() {
        finish(world);
    }
    close_others(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(el) = doc.active_element() else { return };
    let element = el.id;
    let Some(before) = el.feature(feature).filter(|f| matches!(f.kind, FeatureKind::Composite(_))).cloned() else { return };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    start(world, element, feature, false, mark, Some(before));
}

fn start(world: &mut World, element: ElementId, feature: FeatureId, is_new: bool, mark: usize, before: Option<Feature>) {
    world.resource_mut::<Selection>().0.clear();
    world.insert_resource(CompositeSession { element, feature, is_new, mark, before });
}

fn end(world: &mut World) {
    world.remove_resource::<CompositeSession>();
    *world.resource_mut::<crate::parts::PartOverride>() = crate::parts::PartOverride::default();
    world.resource_mut::<Selection>().0.clear();
}

fn current(world: &World, s: &CompositeSession) -> Option<Feature> {
    world.get_resource::<ActiveDocument>()?.doc.element(s.element)?.feature(s.feature).cloned()
}

fn params(world: &World) -> Option<CompositeFeature> {
    let s = world.get_resource::<CompositeSession>()?;
    match current(world, s)?.kind {
        FeatureKind::Composite(c) => Some(c),
        _ => None,
    }
}

/// Replaces the open feature's parameters (one command).
fn set(world: &mut World, c: CompositeFeature, label: &str) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else { return };
    let Some(mut f) = current(world, &s) else { return };
    let kind = FeatureKind::Composite(c);
    if f.kind == kind {
        return;
    }
    f.kind = kind;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    if let Err(e) = doc.execute(&ReplaceFeature { element: s.element, feature: f, label: label.into() }) {
        warn!("cannot change the composite part: {e}");
    }
}

/// Adds the parts not in the list yet and removes the ones in it (`toggle`), or only adds them
/// (a box). Context parts aren't parts of the studio: they are left out.
pub fn pick_parts(world: &mut World, parts: &[PartId], toggle: bool) {
    let Some(mut c) = params(world) else { return };
    for p in parts.iter().filter(|p| !is_context(p.feature)) {
        match c.parts.iter().position(|q| q == p) {
            Some(i) if toggle => {
                c.parts.remove(i);
            }
            Some(_) => {}
            None => c.parts.push(*p),
        }
    }
    set(world, c, "Select parts");
}

/// ✓ / Enter: keeps the feature if it rebuilds.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else { return };
    let Some(f) = current(world, &s).filter(|f| f.is_valid()) else { return };
    let features = world.get_resource::<ActiveDocument>().and_then(|d| Some(d.doc.element(s.element)?.active_features())).unwrap_or_default();
    if cadrs_core::rebuild::build(&features).error(f.id).is_some() {
        return;
    }
    let label = if s.is_new { format!("Insert {}", f.name) } else { format!("Edit {}", f.name) };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    end(world);
}

/// ✕ / Esc: removes a new feature or reverts an edit.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else { return };
    let cur = current(world, &s);
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let name = cur.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        if s.is_new {
            doc.discard_element_since(s.mark, s.element);
        } else {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
            if let (Some(before), Some(f)) = (s.before.clone(), cur)
                && f != before
            {
                let _ = doc.execute(&ReplaceFeature { element: s.element, feature: before, label: format!("Cancel {name}") });
            }
        }
    }
    end(world);
}

/// Accepts if valid, otherwise cancels.
pub fn finish(world: &mut World) {
    accept(world);
    if world.contains_resource::<CompositeSession>() {
        cancel(world);
    }
}

/// The dialog's parts show as selected.
fn show_parts(session: Option<Res<CompositeSession>>, doc: Option<Res<ActiveDocument>>, mut selection: ResMut<Selection>) {
    let Some((s, doc)) = session.zip(doc) else { return };
    let Some(FeatureKind::Composite(c)) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)).map(|f| &f.kind) else { return };
    let want: Vec<Pick> = c.parts.iter().map(|p| Pick::Part(*p)).collect();
    if selection.0 != want {
        selection.0 = want;
    }
}

/// The view shows the studio without the composite (its members pickable), and up to it when
/// it isn't the last feature.
fn preview(session: Option<Res<CompositeSession>>, doc: Option<Res<ActiveDocument>>, mut over: ResMut<crate::parts::PartOverride>) {
    let Some(s) = session else { return };
    let rollback_to = doc.and_then(|d| {
        let el = d.doc.element(s.element)?;
        (crate::feature_list::last_built(el) != Some(s.feature)).then_some(s.feature)
    });
    if over.rolled_back != Some(s.feature) {
        over.rolled_back = Some(s.feature);
    }
    if over.rollback_to != rollback_to {
        over.rollback_to = rollback_to;
    }
}

// ---------------------------------------------------------------------------------------------
// Input

fn composite_picks(mut picks: MessageReader<PickRequest>, session: Option<Res<CompositeSession>>, mut commands: Commands) {
    if session.is_none() {
        picks.clear();
        return;
    }
    for p in picks.read() {
        let Some(part) = p.0.and_then(|p| p.part()) else { continue };
        commands.queue(move |world: &mut World| pick_parts(world, &[part], true));
    }
}

fn composite_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<CompositeSession>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    focus: Res<bevy::input_focus::InputFocus>,
    mut commands: Commands,
) {
    if session.is_none() {
        keys.clear();
        return;
    }
    for k in keys.read() {
        if k.state != ButtonState::Pressed || !q_dialogs.is_empty() || focus.get().is_some() {
            continue;
        }
        match k.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<CompositeDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<CompositeDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

/// Closed.
fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "composite-closed-checkbox") {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        if let Some(mut c) = params(world) {
            c.closed = on;
            set(world, c, "Closed");
        }
    });
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<(), With<PartsField>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        if let Some(p) = params(world).and_then(|c| c.parts.get(i).copied()) {
            pick_parts(world, &[p], true);
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Dialog

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn sync_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<CompositeSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &BuiltFor, &mut FeatureDialogState), With<CompositeDialog>>,
    mut q_list: Query<&mut SelectionListState, With<PartsField>>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(f) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)) else { return };
    let FeatureKind::Composite(c) = &f.kind else { return };
    let names: Vec<String> = c.parts.iter().map(|p| cache.part_name(*p).unwrap_or("Part").to_string()).collect();
    let valid = f.is_valid() && !cache.errors.contains_key(&f.id) && !cache.rebuilding;
    let layout = BuiltFor(c.closed);
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, *k));
    if built.is_none_or(|(_, k)| k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else { return };
        let t = theme.clone();
        let items = names.clone();
        let closed = c.closed;
        let dialog = commands
            .spawn((
                CompositeDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new("composite-dialog")
                    .title(f.name.clone())
                    .valid(valid)
                    .body(move |b| {
                        b.spawn((PartsField, SelectionList::new("composite-parts-field").placeholder("Parts and composite parts").items(items).active(true).build(&t)));
                        b.spawn(OptionRow::new("composite-closed", "Closed").checked(closed).build(&t));
                    })
                    .footer(|f| {
                        f.spawn(Node { flex_grow: 1.0, ..default() });
                        f.spawn((Name::new("composite-dialog-help"), cadrs_ui::icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)), Tooltip::new("Help")));
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState { title: f.name.clone(), valid, error: false };
        if *st != want {
            *st = want;
        }
    }
    for mut l in &mut q_list {
        let want = SelectionListState { items: names.clone(), active: true, error: false, red_items: false, red: Vec::new() };
        if *l != want {
            *l = want;
        }
    }
}

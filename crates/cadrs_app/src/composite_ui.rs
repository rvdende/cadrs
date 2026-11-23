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
//! The same dialog, with other fields, edits the two other features that are lists of parts:
//! **Frame trim** (`cadrs_core::frame::FrameTrimFeature`: a list for each group, the one
//! clicked taking the picks, and an empty one at the end that starts the next group; a member
//! picked into a group leaves the one it was in) and **Cut list**
//! (`cadrs_core::frame::CutListFeature`: **All frame members** and the parts picked).
//!
//! Names: `composite-dialog`, `composite-parts-field`, `composite-closed(-checkbox)`;
//! `frame-trim-dialog`, `frame-trim-group-<i>`; `cut-list-dialog`, `cut-list-parts-field`,
//! `cut-list-all(-checkbox)`.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::assembly::context::is_context;
use cadrs_core::commands::{AddFeature, ReplaceFeature};
use cadrs_core::transform::CompositeFeature;
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind, PartId};
use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::tr;
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, OptionRow,
    SelectionList, SelectionListActivate, SelectionListRemove, SelectionListState,
};

use crate::parts::PartCache;
use crate::viewport::{Pick, PickRequest, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct CompositeUiPlugin;

impl Plugin for CompositeUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (composite_picks, composite_keys, sync_dialog, preview)
                .chain()
                .before(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(
            Update,
            show_parts
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |world: &mut World| {
            if world.contains_resource::<CompositeSession>() {
                finish(world);
            }
        })
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_checkbox)
        .add_observer(on_remove)
        .add_observer(on_activate);
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
    /// A Frame trim's group that takes the picks (one past the last: a new group).
    pub group: usize,
}

#[derive(Component)]
struct CompositeDialog;

/// What the dialog was built for (it is rebuilt when it changes): the feature's kind, its
/// checkbox, and how many lists it has.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuiltFor(u8, bool, usize);

/// A list of parts: the Frame trim group it is (0 for the other features' one list).
#[derive(Component)]
struct PartsField(usize);

/// Whether this dialog edits the feature.
pub fn edits(kind: &FeatureKind) -> bool {
    matches!(
        kind,
        FeatureKind::Composite(_) | FeatureKind::FrameTrim(_) | FeatureKind::CutList(_)
    )
}

/// The dialog's lists of parts.
fn lists(kind: &FeatureKind) -> Vec<Vec<PartId>> {
    match kind {
        FeatureKind::Composite(c) => vec![c.parts.clone()],
        FeatureKind::CutList(c) => vec![c.parts.clone()],
        FeatureKind::FrameTrim(x) => x.groups.clone(),
        _ => Vec::new(),
    }
}

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
    begin_with(world, "Composite part", |parts| {
        FeatureKind::Composite(CompositeFeature {
            parts,
            closed: false,
        })
    });
}

/// Starts a new Frame trim (the toolbar's button): the parts selected are its first group.
pub fn begin_frame_trim(world: &mut World) {
    begin_with(world, "Frame trim", |parts| {
        FeatureKind::FrameTrim(cadrs_core::frame::FrameTrimFeature {
            groups: if parts.is_empty() {
                Vec::new()
            } else {
                vec![parts]
            },
        })
    });
}

/// Starts a new Cut list (the toolbar's button): of every frame member, or of the parts
/// selected.
pub fn begin_cut_list(world: &mut World) {
    begin_with(world, "Cut list", |parts| {
        FeatureKind::CutList(cadrs_core::frame::CutListFeature {
            all: parts.is_empty(),
            parts,
            overrides: Vec::new(),
        })
    });
}

fn begin_with(world: &mut World, base_name: &str, make: fn(Vec<PartId>) -> FeatureKind) {
    if world.contains_resource::<CompositeSession>() {
        finish(world);
    }
    close_others(world);
    let parts: Vec<PartId> = world
        .resource::<Selection>()
        .0
        .iter()
        .filter_map(|p| p.part())
        .filter(|p| !is_context(p.feature))
        .collect();
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(element) = doc
        .active_element()
        .filter(|e| matches!(e.kind, ElementKind::PartStudio { .. }))
        .map(|e| e.id)
    else {
        return;
    };
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    let kind = make(parts);
    let group = lists(&kind).len().min(1);
    if let Err(e) = doc.execute(&AddFeature {
        element,
        feature,
        base_name: base_name.into(),
        kind,
    }) {
        warn!("cannot insert a {base_name}: {e}");
        return;
    }
    start(world, element, feature, true, mark, None);
    if let Some(mut s) = world.get_resource_mut::<CompositeSession>() {
        s.group = group;
    }
}

/// Opens an existing Composite part for editing (a feature row's double-click).
pub fn edit(world: &mut World, feature: FeatureId) {
    if world
        .get_resource::<CompositeSession>()
        .is_some_and(|s| s.feature == feature)
    {
        return;
    }
    if world.contains_resource::<CompositeSession>() {
        finish(world);
    }
    close_others(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else {
        return;
    };
    let element = el.id;
    let Some(before) = el.feature(feature).filter(|f| edits(&f.kind)).cloned() else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    start(world, element, feature, false, mark, Some(before));
}

fn start(
    world: &mut World,
    element: ElementId,
    feature: FeatureId,
    is_new: bool,
    mark: usize,
    before: Option<Feature>,
) {
    world.resource_mut::<Selection>().0.clear();
    world.insert_resource(CompositeSession {
        element,
        feature,
        is_new,
        mark,
        before,
        group: 0,
    });
}

fn end(world: &mut World) {
    world.remove_resource::<CompositeSession>();
    *world.resource_mut::<crate::parts::PartOverride>() = crate::parts::PartOverride::default();
    world.resource_mut::<Selection>().0.clear();
}

fn current(world: &World, s: &CompositeSession) -> Option<Feature> {
    world
        .get_resource::<ActiveDocument>()?
        .doc
        .element(s.element)?
        .feature(s.feature)
        .cloned()
}

fn params(world: &World) -> Option<FeatureKind> {
    let s = world.get_resource::<CompositeSession>()?;
    Some(current(world, s)?.kind).filter(edits)
}

/// Replaces the open feature's parameters (one command).
fn set(world: &mut World, kind: FeatureKind, label: &str) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else {
        return;
    };
    let Some(mut f) = current(world, &s) else {
        return;
    };
    if f.kind == kind {
        return;
    }
    f.kind = kind;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    if let Err(e) = doc.execute(&ReplaceFeature {
        element: s.element,
        feature: f,
        label: label.into(),
    }) {
        warn!("cannot change the composite part: {e}");
    }
}

/// Adds the parts not in the list yet and removes the ones in it (`toggle`), or only adds them
/// (a box). Context parts aren't parts of the studio: they are left out.
pub fn pick_parts(world: &mut World, parts: &[PartId], toggle: bool) {
    let Some(mut kind) = params(world) else {
        return;
    };
    let mut group = world
        .get_resource::<CompositeSession>()
        .map_or(0, |s| s.group);
    let picked = parts.iter().filter(|p| !is_context(p.feature));
    let pick = |list: &mut Vec<PartId>, p: &PartId| match list.iter().position(|q| q == p) {
        Some(i) if toggle => {
            list.remove(i);
        }
        Some(_) => {}
        None => list.push(*p),
    };
    match &mut kind {
        FeatureKind::Composite(c) => picked.for_each(|p| pick(&mut c.parts, p)),
        FeatureKind::CutList(c) => picked.for_each(|p| pick(&mut c.parts, p)),
        FeatureKind::FrameTrim(x) => {
            group = group.min(x.groups.len());
            if group == x.groups.len() {
                x.groups.push(Vec::new());
            }
            for p in picked {
                // A member is in one group: picked into this one, it leaves the other.
                for (g, list) in x.groups.iter_mut().enumerate() {
                    if g != group {
                        list.retain(|q| q != p);
                    }
                }
                pick(&mut x.groups[group], p);
            }
            // The groups left empty go; the picks stay with the list they were going to (a
            // new one, if that one went).
            let before = x.groups[..group].iter().filter(|g| g.is_empty()).count();
            let gone = x.groups[group].is_empty();
            x.groups.retain(|g| !g.is_empty());
            group = if gone { x.groups.len() } else { group - before };
        }
        _ => return,
    }
    if let Some(mut s) = world.get_resource_mut::<CompositeSession>()
        && s.group != group
    {
        s.group = group;
    }
    set(world, kind, "Select parts");
}

/// ✓ / Enter: keeps the feature if it rebuilds.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else {
        return;
    };
    let Some(f) = current(world, &s).filter(|f| f.is_valid()) else {
        return;
    };
    let features = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| Some(d.doc.element(s.element)?.active_features()))
        .unwrap_or_default();
    if crate::accept::builds(world, "composite", features, f.id, accept) != Some(true) {
        return;
    }
    let label = if s.is_new {
        format!("Insert {}", f.name)
    } else {
        format!("Edit {}", f.name)
    };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    end(world);
}

/// ✕ / Esc: removes a new feature or reverts an edit.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<CompositeSession>().cloned() else {
        return;
    };
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
                let _ = doc.execute(&ReplaceFeature {
                    element: s.element,
                    feature: before,
                    label: format!("Cancel {name}"),
                });
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
fn show_parts(
    session: Option<Res<CompositeSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut selection: ResMut<Selection>,
) {
    let Some((s, doc)) = session.zip(doc) else {
        return;
    };
    let Some(kind) = doc
        .doc
        .element(s.element)
        .and_then(|e| e.feature(s.feature))
        .map(|f| &f.kind)
        .filter(|k| edits(k))
    else {
        return;
    };
    let want: Vec<Pick> = lists(kind).into_iter().flatten().map(Pick::Part).collect();
    if selection.0 != want {
        selection.0 = want;
    }
}

/// The view shows the studio without the composite (its members pickable), and up to it when
/// it isn't the last feature.
fn preview(
    session: Option<Res<CompositeSession>>,
    doc: Option<Res<ActiveDocument>>,
    mut over: ResMut<crate::parts::PartOverride>,
) {
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

fn composite_picks(
    mut picks: MessageReader<PickRequest>,
    session: Option<Res<CompositeSession>>,
    mut commands: Commands,
) {
    if session.is_none() {
        picks.clear();
        return;
    }
    for p in picks.read() {
        let Some(part) = p.0.and_then(|p| p.part()) else {
            continue;
        };
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
        match k.shortcut_code() {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn on_accept(
    ev: On<FeatureDialogAccept>,
    q: Query<(), With<CompositeDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(
    ev: On<FeatureDialogCancel>,
    q: Query<(), With<CompositeDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

/// Closed; a Cut list's All frame members.
fn on_checkbox(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity).map(|n| n.as_str().to_string()) else {
        return;
    };
    if name != "composite-closed-checkbox" && name != "cut-list-all-checkbox" {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| match params(world) {
        Some(FeatureKind::Composite(mut c)) if name == "composite-closed-checkbox" => {
            c.closed = on;
            set(world, FeatureKind::Composite(c), "Closed");
        }
        Some(FeatureKind::CutList(mut c)) if name == "cut-list-all-checkbox" => {
            c.all = on;
            set(world, FeatureKind::CutList(c), "All frame members");
        }
        _ => {}
    });
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<&PartsField>, mut commands: Commands) {
    let Ok(&PartsField(group)) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        let part = params(world).and_then(|k| lists(&k).get(group)?.get(i).copied());
        let Some(p) = part else { return };
        // (Out of the list it is in, wherever the picks are going.)
        let was = world
            .get_resource::<CompositeSession>()
            .map_or(0, |s| s.group);
        let lists_before = params(world).map_or(0, |k| lists(&k).len());
        if let Some(mut s) = world.get_resource_mut::<CompositeSession>() {
            s.group = group;
        }
        pick_parts(world, &[p], true);
        let lists_now = params(world).map_or(0, |k| lists(&k).len());
        if let Some(mut s) = world.get_resource_mut::<CompositeSession>() {
            // Back to the list the picks were going to (one up, if a list before it went).
            s.group = if lists_now < lists_before && was > group {
                was - 1
            } else if lists_now < lists_before && was == group {
                lists_now
            } else {
                was
            };
        }
    });
}

/// A Frame trim's group clicked: it takes the next picks.
fn on_activate(
    ev: On<SelectionListActivate>,
    q: Query<&PartsField>,
    session: Option<ResMut<CompositeSession>>,
) {
    let (Ok(&PartsField(group)), Some(mut s)) = (q.get(ev.entity), session) else {
        return;
    };
    if s.group != group {
        s.group = group;
    }
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
    mut q_list: Query<(&PartsField, &mut SelectionListState)>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(f) = doc
        .doc
        .element(s.element)
        .and_then(|e| e.feature(s.feature))
    else {
        return;
    };
    if !edits(&f.kind) {
        return;
    }
    let name_of = |p: &PartId| {
        cache
            .part_name(*p)
            .map_or_else(|| tr("common-part"), str::to_string)
    };
    let mut names: Vec<Vec<String>> = lists(&f.kind)
        .iter()
        .map(|l| l.iter().map(name_of).collect())
        .collect();
    // (A Frame trim's empty list at the end: the next group.)
    let trim = matches!(f.kind, FeatureKind::FrameTrim(_));
    if trim {
        names.push(Vec::new());
    }
    let active = |g: usize| !trim || g == s.group.min(names.len() - 1);
    let valid = f.is_valid() && !cache.errors.contains_key(&f.id) && !cache.rebuilding;
    let layout = match &f.kind {
        FeatureKind::Composite(c) => BuiltFor(0, c.closed, 1),
        FeatureKind::CutList(c) => BuiltFor(1, c.all, 1),
        _ => BuiltFor(2, false, names.len()),
    };
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, *k));
    if built.is_none_or(|(_, k)| k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else {
            return;
        };
        let t = theme.clone();
        let items = names.clone();
        let actives: Vec<bool> = (0..names.len()).map(active).collect();
        let BuiltFor(which, checked, _) = layout;
        let dialog_name =
            ["composite-dialog", "cut-list-dialog", "frame-trim-dialog"][which as usize];
        let dialog = commands
            .spawn((
                CompositeDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new(dialog_name)
                    .title(f.name.clone())
                    .valid(valid)
                    .body(move |b| match which {
                        0 => {
                            b.spawn((
                                PartsField(0),
                                SelectionList::new("composite-parts-field")
                                    .placeholder(tr("model-composite-parts"))
                                    .items(items[0].clone())
                                    .active(true)
                                    .build(&t),
                            ));
                            b.spawn(
                                OptionRow::new("composite-closed", tr("model-composite-closed"))
                                    .checked(checked)
                                    .build(&t),
                            );
                        }
                        1 => {
                            b.spawn(
                                OptionRow::new("cut-list-all", tr("model-cut-list-all"))
                                    .checked(checked)
                                    .build(&t),
                            );
                            b.spawn((
                                PartsField(0),
                                SelectionList::new("cut-list-parts-field")
                                    .placeholder(if checked {
                                        tr("model-cut-list-more-parts")
                                    } else {
                                        tr("model-cut-list-parts")
                                    })
                                    .items(items[0].clone())
                                    .active(true)
                                    .build(&t),
                            ));
                        }
                        _ => {
                            for (g, list) in items.iter().enumerate() {
                                b.spawn((
                                    PartsField(g),
                                    SelectionList::new(format!("frame-trim-group-{g}"))
                                        .placeholder(if g == 0 {
                                            tr("model-frame-trim-first-group")
                                        } else {
                                            tr("model-frame-trim-group")
                                        })
                                        .items(list.clone())
                                        .active(actives[g])
                                        .build(&t),
                                ));
                            }
                        }
                    })
                    .footer(|f| {
                        f.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        f.spawn((
                            Name::new("composite-dialog-help"),
                            cadrs_ui::icon("help-filled", 14.0, Color::srgb_u8(0xa8, 0xa8, 0xa8)),
                            Tooltip::new(tr("common-help")),
                        ));
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState {
            title: f.name.clone(),
            valid,
            error: false,
        };
        if *st != want {
            *st = want;
        }
    }
    for (&PartsField(g), mut l) in &mut q_list {
        let Some(items) = names.get(g) else { continue };
        let want = SelectionListState {
            items: items.clone(),
            active: active(g),
            error: false,
            red_items: false,
            red: Vec::new(),
        };
        if *l != want {
            *l = want;
        }
    }
}

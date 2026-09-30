//! P3D.4: Replace reference (IR4, X5; `inspection-and-repair/ex1-step13.png`–`ex1-step16.png`).
//!
//! Only while the Repair panel is open, hovering an item of a feature dialog's selection list
//! (an extrude's or revolve's inputs, a fillet's or chamfer's entities) shows a small "Replace
//! reference" icon at its right (IR4.2). It opens a second dialog beside the feature's,
//! **Replace reference**, with its own ✓ and ✕:
//!
//! - **Selection to replace**: the item, in red when it is a missing reference ("Missing Face
//!   of Sketch 3"); hovering it outlines where it was in the Repair panel (IR3.6).
//! - **Selection to replace with** (active, pale blue): the next pick in the viewport (a sketch
//!   region, a face, an edge, or a sketch from the feature list) replaces the item at once, so
//!   the feature previews with it. One reference at a time (IR4.3).
//! - **Propagate changes** (on by default, IR4.4): every feature after this one that used the
//!   same reference (the same sketch region, or the same persistent face or edge name) gets
//!   the replacement too.
//! - For a fillet or chamfer that propagates along tangents, a replacement that stands for
//!   fewer edges than the missing reference's tangent chain had is flagged under the fields
//!   (IR4.7: pick the face when the new edges aren't tangent-connected).
//!
//! The replacement is a [`ReplaceReference`] step inside the feature's dialog, so the feature's
//! ✓ accepts both dialogs (IR4.5) and its ✕ drops both. The Replace dialog's ✓ keeps the
//! replacement and closes; its ✕ takes it back.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::repair::{Reference, ReplaceReference, chain_check, entity_edges, references};
use cadrs_core::{ElementId, FeatureId, RegionRef};
use cadrs_core::document::{EdgeRef, FaceRef};
use cadrs_ui::{
    CheckboxChange, FeatureDialog, FeatureDialogAccept, FeatureDialogCancel, OptionRow, SelectionList, SelectionListReplace,
    SelectionListState, Theme,
};

use crate::parts::PartCache;
use crate::viewport::{Pick, PickRequest, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ReplaceReferencePlugin;

impl Plugin for ReplaceReferencePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_replace_icon)
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_propagate)
            .add_systems(
                Update,
                (end_with_feature, replace_picks, sync_dialog)
                    .chain()
                    .after(crate::parts::PartsSet)
                    .before(crate::repair::RepairSet)
                    .run_if(in_state(AppState::Document)),
            );
    }
}

/// The Replace reference dialog: the feature and the item it replaces.
#[derive(Resource, Debug, Clone)]
pub struct ReplaceSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub index: usize,
    /// The item as the feature's list showed it ("Missing Face of Sketch 3"), and whether it
    /// was missing.
    pub old_label: String,
    pub old_missing: bool,
    pub old: Reference,
    /// The replacement picked, and its label.
    pub with: Option<Reference>,
    pub with_label: String,
    pub propagate: bool,
    /// The IR4.7 note, if the replacement stands for fewer edges.
    pub note: Option<String>,
    /// The edges the old reference stood for in the Repair panel's state.
    old_edges: usize,
    /// A replacement step is on the undo stack (the next pick or ✕ takes it back first).
    applied: bool,
}

/// The item under the pointer in the "Selection to replace" field, for the Repair highlight.
pub fn hovered_old(world: &World, list: &str) -> Option<(FeatureId, Reference)> {
    if list != "replace-from-field" {
        return None;
    }
    let s = world.get_resource::<ReplaceSession>()?;
    Some((s.feature, s.old.clone()))
}

fn feature_dialog_open(world: &World) -> bool {
    world.contains_resource::<crate::extrude::ExtrudeSession>() || world.contains_resource::<crate::applied::AppliedSession>()
}

fn on_replace_icon(ev: On<SelectionListReplace>, q: Query<(&Name, &SelectionListState)>, mut commands: Commands) {
    let Ok((name, state)) = q.get(ev.entity) else { return };
    let list = name.as_str().to_string();
    let index = ev.index;
    let label = state.items.get(index).cloned().unwrap_or_default();
    let missing = state.red.get(index).copied().unwrap_or(false);
    commands.queue(move |world: &mut World| start(world, &list, index, label, missing));
}

/// Opens the dialog on item `index` of the dialog list `list`.
pub fn start(world: &mut World, list: &str, index: usize, label: String, missing: bool) {
    if !world.resource::<crate::repair::Repair>().open {
        return;
    }
    let Some(feature) = crate::repair::list_feature(world, list) else { return };
    let Some((element, f)) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().and_then(|e| Some((e.id, e.feature(feature)?.clone()))))
    else {
        return;
    };
    let Some(old) = references(&f).get(index).cloned() else { return };
    // What the old reference stood for, in the Repair panel's state (IR4.7).
    let old_edges = {
        let mut r = world.resource_mut::<crate::repair::Repair>();
        let i = r.features.iter().position(|x| x.id == feature).unwrap_or(r.features.len());
        let before = r.parts_before(i);
        entity_edges(&before.parts, &old)
    };
    world.insert_resource(ReplaceSession {
        element,
        feature,
        index,
        old_label: label,
        old_missing: missing,
        old,
        with: None,
        with_label: String::new(),
        propagate: true,
        note: None,
        old_edges,
        applied: false,
    });
}

/// The feature's dialog closed (✓ or ✕): the Replace dialog goes with it.
fn end_with_feature(world: &mut World) {
    if world.contains_resource::<ReplaceSession>() && !feature_dialog_open(world) {
        world.remove_resource::<ReplaceSession>();
    }
}

/// A pick is the replacement.
fn replace_picks(mut picks: MessageReader<PickRequest>, session: Option<Res<ReplaceSession>>, mut commands: Commands) {
    if session.is_none() {
        return;
    }
    for p in picks.read() {
        let Some(pick) = p.0 else { continue };
        commands.queue(move |world: &mut World| take_pick(world, pick));
    }
}

/// The reference a pick stands for.
fn reference_of(cache: &PartCache, pick: Pick) -> Option<Reference> {
    match pick {
        Pick::Region(sketch, i) => {
            let region = cache.sketch_regions(sketch)?.regions.get(i as usize)?.clone();
            Some(Reference::Region(RegionRef::new(sketch, &region)))
        }
        Pick::Feature(sketch) => Some(Reference::Sketch(sketch)),
        Pick::Face(part, face) => {
            let solid = &cache.part(part)?.solid;
            let i = solid.faces.iter().position(|f| f.name == face)?;
            Some(Reference::Face(FaceRef { part, face, seed: solid.face_point(i)? }))
        }
        Pick::Edge(part, edge) => {
            let solid = &cache.part(part)?.solid;
            Some(Reference::Edge(EdgeRef { part, edge, seed: solid.edge(&edge)?.midpoint() }))
        }
        _ => None,
    }
}

fn take_pick(world: &mut World, pick: Pick) {
    let Some(with) = reference_of(world.resource::<PartCache>(), pick) else { return };
    let Some(s) = world.get_resource::<ReplaceSession>().cloned() else { return };
    // A list takes what it can hold: an extrude regions, sketches and faces; a fillet edges
    // and faces.
    let fits = match world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()?.feature(s.feature).cloned()) {
        Some(f) if f.fillet().is_some() || f.chamfer().is_some() => matches!(with, Reference::Edge(_) | Reference::Face(_)),
        Some(_) => !matches!(with, Reference::Edge(_)),
        None => false,
    };
    if !fits {
        return;
    }
    apply(world, Some(with));
}

/// Takes back the last replacement, then replaces with `with` (if any) as the session says.
fn apply(world: &mut World, with: Option<Reference>) {
    let Some(mut s) = world.get_resource::<ReplaceSession>().cloned() else { return };
    let mut doc = world.resource_mut::<ActiveDocument>();
    if s.applied {
        doc.undo();
        s.applied = false;
    }
    if let Some(w) = &with {
        match doc.execute(&ReplaceReference { element: s.element, feature: s.feature, index: s.index, with: w.clone(), propagate: s.propagate }) {
            Ok(()) => s.applied = true,
            Err(e) => warn!("replace reference: {e}"),
        }
    }
    let features: Vec<cadrs_core::Feature> = doc.doc.element(s.element).map(|e| e.features().to_vec()).unwrap_or_default();
    s.with_label = with.as_ref().map(|w| w.label(&features)).unwrap_or_default();
    // IR4.7: an edge or face replacement of a fillet or chamfer that propagates along tangents.
    s.note = None;
    if let (Some(w), Some(i)) = (&with, features.iter().position(|f| f.id == s.feature))
        && features[i].fillet().is_some_and(|x| x.tangent_propagation)
    {
        let before = cadrs_core::rebuild::build(&features[..i]);
        s.note = chain_check(s.old_edges, entity_edges(&before.parts, w), matches!(w, Reference::Edge(_)));
    }
    s.with = with;
    world.insert_resource(s);
}

fn on_propagate(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "replace-propagate-checkbox") {
        return;
    }
    let checked = ev.checked;
    commands.queue(move |world: &mut World| {
        let Some(mut s) = world.get_resource_mut::<ReplaceSession>() else { return };
        s.propagate = checked;
        let with = s.with.clone();
        apply(world, with);
    });
}

#[derive(Component)]
struct ReplaceDialog;

/// What the dialog shows (its rows are rebuilt when it changes).
#[derive(Component, PartialEq)]
struct Shown(String, bool, String, bool, Option<String>, f32);

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ReplaceDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<ReplaceSession>();
        });
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ReplaceDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            apply(world, None);
            world.remove_resource::<ReplaceSession>();
        });
    }
}

/// The dialog, beside the feature's (`ex1-step14.png`).
#[allow(clippy::type_complexity)]
fn sync_dialog(
    session: Option<Res<ReplaceSession>>,
    theme: Res<Theme>,
    q_dialog: Query<(Entity, &Shown), With<ReplaceDialog>>,
    q_feature: Query<(&Name, &ComputedNode), (With<cadrs_ui::FeatureDialogState>, Without<ReplaceDialog>)>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut commands: Commands,
) {
    let Some(s) = session else {
        for (e, _) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    // Right of the feature dialog.
    let left = q_feature
        .iter()
        .filter(|(n, _)| n.as_str() != "replace-reference-dialog")
        .map(|(_, c)| c.size().x * c.inverse_scale_factor())
        .fold(0.0f32, f32::max)
        + 4.0;
    let want = Shown(s.old_label.clone(), s.old_missing, s.with_label.clone(), s.propagate, s.note.clone(), left.round());
    if q_dialog.iter().any(|(_, sh)| *sh == want) {
        return;
    }
    for (e, _) in &q_dialog {
        commands.entity(e).try_despawn();
    }
    let Some(area) = q_area.iter().next() else { return };
    let t = theme.clone();
    let (old, missing, with, propagate, note) = (s.old_label.clone(), s.old_missing, s.with_label.clone(), s.propagate, s.note.clone());
    let d = commands
        .spawn((
            ReplaceDialog,
            want,
            DespawnOnExit(AppState::Document),
            FeatureDialog::new("replace-reference-dialog")
                .title("Replace reference")
                .width(200.0)
                .body(move |b| {
                    b.spawn(
                        SelectionList::new("replace-from-field")
                            .placeholder("Selection to replace")
                            .items(vec![old])
                            .error(missing)
                            .red(vec![missing])
                            .build(&t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::vertical(Val::Px(2.0));
                    });
                    b.spawn(
                        SelectionList::new("replace-with-field")
                            .placeholder("Selection to replace with")
                            .items(if with.is_empty() { vec![] } else { vec![with] })
                            .active(true)
                            .tint_filled()
                            .build(&t),
                    )
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::vertical(Val::Px(2.0));
                    });
                    b.spawn(OptionRow::new("replace-propagate", "Propagate changes").checked(propagate).build(&t));
                    if let Some(note) = note {
                        // The dialog is sized before the text wraps: room for its lines (about
                        // 34 characters each at this size).
                        let lines = note.chars().count().div_ceil(34).max(1) as f32;
                        b.spawn((
                            Name::new("replace-note"),
                            Node {
                                width: Val::Px(188.0),
                                min_height: Val::Px(lines * 13.5),
                                flex_shrink: 0.0,
                                margin: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::ZERO, Val::Px(4.0)),
                                ..default()
                            },
                        ))
                        .with_children(|n| {
                            n.spawn(t.text(note, 10.5, FontWeight::NORMAL, Color::srgb_u8(0x9a, 0x5b, 0x00)))
                                .insert((TextLayout::default(), Node { max_width: Val::Px(186.0), ..default() }));
                        });
                    }
                })
                .build(&theme),
        ))
        .id();
    commands.entity(d).entry::<Node>().and_modify(move |mut n| n.left = Val::Px(left));
    commands.entity(area).add_child(d);
}

//! The **Replace instances** dialog (P3B.9, `intro-to-assemblies.md` X15;
//! [`cadrs_core::assembly::replace`]), from the instance menu's **Replace instances…**:
//!
//! - **Instances to replace**: the right-clicked instance, or the selected ones; it follows the
//!   selection (click more instances in the view or the list); each row has its ✕.
//! - **Replace with**: every part of the document's Part Studios, each with its thumbnail; click
//!   one to choose it.
//! - Below, what happens to the mates: "Mates kept: 1" when the new part has matching faces
//!   and edges (the same Part Studio's names, else the same geometry where the old ones were),
//!   "… removed: 1" for those it hasn't.
//! - ✓ replaces them (one undo step): same placements, the new part's name and numbers.
//!
//! Names: `replace-dialog`, `replace-instances`, `replace-with`, rows `replace-part-<slug>`,
//! `replace-summary`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::assembly::replace::{self, ReplaceInstances};
use cadrs_core::assembly::{InstanceId, InstanceSource};
use cadrs_core::{ElementId, PartId};
use cadrs_ui::prelude::*;
use cadrs_ui::{FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, SelectionList, SelectionListRemove, SelectionListState};

use crate::parts::PartCache;
use crate::viewport::{Pick, Selection, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ReplaceDialogPlugin;

impl Plugin for ReplaceDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (follow_selection, sync_dialog).chain().after(crate::parts::PartsSet).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut commands: Commands| commands.remove_resource::<ReplaceSession>())
            .add_observer(on_accept)
            .add_observer(on_cancel)
            .add_observer(on_remove);
    }
}

/// The open Replace instances dialog.
#[derive(Resource, Debug, Clone)]
pub struct ReplaceSession {
    pub element: ElementId,
    pub instances: Vec<InstanceId>,
    /// The part chosen to replace them with.
    pub with: Option<(ElementId, PartId)>,
}

#[derive(Component)]
struct ReplaceDialog;

/// What the dialog was built from.
#[derive(Component, Debug, Clone, PartialEq)]
struct DialogKey(Vec<String>, Option<(ElementId, PartId)>, String);

/// A row of the Replace with list.
#[derive(Component, Debug, Clone, Copy)]
struct PartRow(ElementId, PartId);

/// The part instances among `ids` (subassemblies and rigid studios are not replaced).
fn part_instances(doc: &ActiveDocument, ids: &[InstanceId]) -> Vec<InstanceId> {
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return Vec::new() };
    ids.iter().copied().filter(|i| model.instance(*i).is_some_and(|x| x.source.part().is_some())).collect()
}

/// Opens the dialog on `instances` (the menu's targets).
pub fn open(world: &mut World, instances: Vec<InstanceId>) {
    let Some(element) = world.get_resource::<ActiveDocument>().and_then(super::active_assembly) else { return };
    let instances = part_instances(world.resource::<ActiveDocument>(), &instances);
    if instances.is_empty() {
        return;
    }
    world.remove_resource::<super::mate_dialog::MateSession>();
    world.remove_resource::<super::group_dialog::GroupSession>();
    world.resource_mut::<Selection>().0 = instances.iter().map(|i| Pick::Part(i.part_id())).collect();
    world.insert_resource(ReplaceSession { element, instances, with: None });
}

fn follow_selection(selection: Res<Selection>, doc: Option<Res<ActiveDocument>>, session: Option<ResMut<ReplaceSession>>) {
    let (Some(mut s), Some(doc)) = (session, doc) else { return };
    if !selection.is_changed() {
        return;
    }
    let now = part_instances(&doc, &super::selected_instances(&selection));
    if s.instances != now {
        s.instances = now;
    }
}

fn on_remove(ev: On<SelectionListRemove>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(|n| n.as_str()) != Ok("replace-instances") {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        let Some(id) = world.get_resource::<ReplaceSession>().and_then(|s| s.instances.get(i).copied()) else { return };
        world.resource_mut::<Selection>().0.retain(|p| super::instance_of(p) != Some(id));
    });
}

/// The mates of `s`'s plan: (kept, removed) names, and the plan itself.
#[allow(clippy::type_complexity)]
fn plan(world: &mut World, s: &ReplaceSession) -> Option<(Vec<cadrs_core::assembly::mate::MateFeature>, Vec<cadrs_core::assembly::mate::MateId>)> {
    let (el, part) = s.with?;
    world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let model = doc.doc.element(s.element)?.assembly_model()?;
        let old = cadrs_core::assembly::source_solids(model, |e| parts.build(&doc.doc, e));
        let new = parts.build(&doc.doc, el)?.part(part)?.solid.clone();
        Some(replace::plan(model, &s.instances, &old, &new))
    })
}

/// ✓: the instances take the chosen part, with the mates that match (one undo step).
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ReplaceSession>().cloned() else { return };
    let Some((el, part)) = s.with else { return };
    if s.instances.is_empty() {
        return;
    }
    let Some((mates, dropped)) = plan(world, &s) else { return };
    let n = dropped.len();
    let source = InstanceSource::Part { element: el, part };
    if super::run(world, &ReplaceInstances { element: s.element, instances: s.instances.clone(), source, mates, dropped }) {
        world.remove_resource::<ReplaceSession>();
        world.resource_mut::<Selection>().0 = s.instances.iter().map(|i| Pick::Part(i.part_id())).collect();
        if n > 0 {
            let theme = world.resource::<Theme>().clone();
            let text = if n == 1 { "1 mate removed (no matching geometry)".to_string() } else { format!("{n} mates removed (no matching geometry)") };
            let mut c = world.commands();
            cadrs_ui::toast::show_notification(&mut c, &theme, cadrs_ui::toast::Notification::info(text).max_width(420.0).name("replace-toast"));
            world.flush();
        }
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ReplaceDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ReplaceDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.remove_resource::<ReplaceSession>();
    }
}

/// Every part of the document's Part Studios: (studio, part, "Part", "Studio").
fn candidates(world: &mut World) -> Vec<(ElementId, PartId, String, String)> {
    world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
        let Some(doc) = world.get_resource::<ActiveDocument>() else { return Vec::new() };
        let mut out = Vec::new();
        for e in doc.doc.elements.iter().filter(|e| e.assembly_model().is_none()) {
            let Some(b) = parts.build(&doc.doc, e.id) else { continue };
            for p in b.parts.iter().filter(|p| p.kind == cadrs_core::PartKind::Solid) {
                let name = cadrs_core::assembly::source_part_name(&doc.doc, &InstanceSource::Part { element: e.id, part: p.id }, Some(&b));
                out.push((e.id, p.id, name, e.name.clone()));
            }
        }
        out
    })
}

fn sync_dialog(world: &mut World) {
    let session = world.get_resource::<ReplaceSession>().cloned();
    let mut q = world.query_filtered::<(Entity, &DialogKey), With<ReplaceDialog>>();
    let existing: Vec<(Entity, DialogKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    let Some(s) = session else {
        for (e, _) in existing {
            world.entity_mut(e).despawn();
        }
        return;
    };
    let names: Vec<String> = {
        let cache = world.resource::<PartCache>();
        s.instances.iter().map(|i| cache.part_name(i.part_id()).unwrap_or("instance").to_string()).collect()
    };
    // Nothing changed: nothing to do (the plan is worked out only when something did).
    if existing.first().is_some_and(|(_, k)| k.0 == names && k.1 == s.with) {
        return;
    }
    let summary = match plan(world, &s) {
        None if s.with.is_none() => "Choose the part to replace them with".to_string(),
        None => String::new(),
        Some((kept, dropped)) => {
            let mut t = format!("Mates kept: {}", kept.len());
            if !dropped.is_empty() {
                let model = world.resource::<ActiveDocument>().doc.element(s.element).and_then(|e| e.assembly_model().cloned());
                let names: Vec<String> = dropped.iter().filter_map(|id| model.as_ref()?.mate(*id).map(|f| f.name.clone())).collect();
                t.push_str(&format!(" · removed (no matching geometry): {}", names.join(", ")));
            }
            t
        }
    };
    let key = DialogKey(names.clone(), s.with, summary.clone());
    if let Some((e, k)) = existing.first() {
        if *k == key {
            return;
        }
        // Only the list changed: update it in place.
        if k.1 == key.1 && k.2 == key.2 {
            let mut ql = world.query::<(&Name, &mut SelectionListState)>();
            for (n, mut st) in ql.iter_mut(world) {
                if n.as_str() == "replace-instances" && st.items != names {
                    st.items = names.clone();
                }
            }
            let mut qd = world.query::<&mut FeatureDialogState>();
            if let Ok(mut st) = qd.get_mut(world, *e) {
                st.valid = !names.is_empty() && s.with.is_some();
            }
            world.entity_mut(*e).insert(key);
            return;
        }
        world.entity_mut(*e).despawn();
    }
    let cands = candidates(world);
    // The rows' thumbnails.
    let mut thumbs: Vec<Option<Handle<Image>>> = Vec::new();
    world.resource_scope(|world, mut store: Mut<super::insert::Thumbnails>| {
        world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
            world.resource_scope(|world, mut parts: Mut<super::AssemblyParts>| {
                let doc = world.resource::<ActiveDocument>();
                for (el, p, ..) in &cands {
                    thumbs.push(super::insert::thumbnail(&mut store, &mut images, doc, &mut parts, (*el, Some(*p))));
                }
            });
        });
    });
    let t = world.resource::<Theme>().clone();
    let tb = t.clone();
    let valid = !names.is_empty() && s.with.is_some();
    let chosen = s.with;
    let bundle = (
        ReplaceDialog,
        key,
        DespawnOnExit(AppState::Document),
        FeatureDialog::new("replace-dialog")
            .title("Replace instances")
            .valid(valid)
            .width(280.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(SelectionList::new("replace-instances").placeholder("Instances to replace").tint_filled().items(names).active(true).build(t))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.flex_grow = 0.0;
                        n.margin = UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(2.0), Val::Px(6.0));
                    });
                b.spawn(t.text("Replace with", 12.0, FontWeight::SEMIBOLD, t.foreground));
                b.spawn((
                    Name::new("replace-with"),
                    Node {
                        flex_direction: FlexDirection::Column,
                        max_height: Val::Px(260.0),
                        overflow: Overflow::scroll_y(),
                        margin: UiRect::vertical(Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BorderColor::all(t.border),
                ))
                .with_children(|l| {
                    for ((el, p, name, studio), img) in cands.into_iter().zip(thumbs) {
                        let on = chosen == Some((el, p));
                        let mut row = l.spawn((
                            TreeItem::new(format!("replace-part-{}", super::insert::slug(&name)), name.clone()).height(36.0).left(4.0).selected(on).build(t),
                            PartRow(el, p),
                            Tooltip::new(format!("{name} (Part Studio {studio})")),
                        ));
                        row.observe(|click: On<Pointer<Click>>, q: Query<&PartRow>, mut commands: Commands| {
                            if let Ok(r) = q.get(click.entity).copied() {
                                commands.queue(move |world: &mut World| {
                                    if let Some(mut s) = world.get_resource_mut::<ReplaceSession>() {
                                        s.with = Some((r.0, r.1));
                                    }
                                });
                            }
                        });
                        if let Some(img) = img {
                            let th = row.world_scope(|w| w.spawn((Node { width: Val::Px(30.0), height: Val::Px(30.0), flex_shrink: 0.0, ..default() }, ImageNode::new(img), Pickable::IGNORE)).id());
                            row.insert_children(0, &[th]);
                        }
                        // Its Part Studio, when named otherwise.
                        if studio != name {
                            row.with_child((t.text(studio, 10.5, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE, Node { margin: UiRect::right(Val::Px(6.0)), ..default() }));
                        }
                    }
                });
                b.spawn((Name::new("replace-summary"), t.text(summary, 11.5, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::top(Val::Px(4.0)), ..default() }));
            })
            .build(&t),
    );
    let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = qa.iter(world).next() else { return };
    let d = world.spawn(bundle).id();
    world.entity_mut(area).add_child(d);
}

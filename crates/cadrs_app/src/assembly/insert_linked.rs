//! P3G.1: the Insert dialog's **Other documents** tab and the **version graph** of its Current
//! document tab (`external-references.md` ER1.1–ER1.8, ER3.1, ER3.2; `ex1-step4.png` A–C,
//! `lesson-referencing-versions-within-a-document.png`; the browser itself is
//! [`crate::linked`]).
//!
//! - **Other documents**: the search field and the locations, a location's documents, then the
//!   document picked, at its newest version ("↳ V1"), with a back arrow, Create version and the
//!   version graph; below it the same Part Studios | Assemblies lists as the Current document,
//!   read from that version. A click picks an item up and a click in the graphics area inserts
//!   it, as for this document (ER1.8); each insert is one step that also stores the copies the
//!   link needs ([`cadrs_core::external::InsertLinked`]). A circular reference is refused with
//!   a toast (DV1.5).
//! - **Current document**: the header's version graph button swaps the list for the graph: Main
//!   (the workspace) and the versions down to Start. Picking a version lists this document as it
//!   was then ("↳ V1" under the name) and inserts frozen references to it (ER3.1); Main goes
//!   back to the workspace. Create version beside it makes one on the fly (DV1.8) and selects it.

use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::assembly::InstanceSource;
use cadrs_core::parts::PartKind;
use cadrs_core::{Document, ElementId};
use cadrs_ui::{OpenedHeader, Theme, VersionGraphSelect};

use super::insert::{InsertSession, Item, ItemLink, TreeRow};
use crate::linked::{self, GraphPick, Opened, Owner, VersionTarget};
use crate::{ActiveDocument, AppState};

pub struct InsertLinkedPlugin;

impl Plugin for InsertLinkedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_other_panel, sync_graph_panel).chain().run_if(in_state(AppState::Document)))
            .add_observer(on_button)
            .add_observer(on_graph_pick);
    }
}

/// The Other documents tab's browser (hidden on the other tabs).
#[derive(Component)]
pub struct InsertOtherPanel;

/// The Current document's header (its name and branch or version, with the buttons).
#[derive(Component)]
pub struct InsertCurrentHeader;

/// The version graph, in place of the lists while it is open.
#[derive(Component)]
pub struct InsertGraphPanel;

/// The rows of the tree for a linked source: its Part Studios and their parts, or its
/// assemblies, as they are at the version, filtered by the search (names, and part numbers and
/// descriptions, ER1.4).
pub fn linked_tree_items(doc: &Document, parts_of: &mut super::AssemblyParts, s: &InsertSession, src: &Opened) -> Vec<TreeRow> {
    let q = s.query.to_lowercase();
    let mut out = Vec::new();
    let item_link = |el: ElementId| -> Option<ItemLink> { Some(ItemLink { reference: src.reference(el)?, snapshot: src.snapshot(el)?.clone() }) };
    if s.assemblies {
        for el in src.doc.elements.iter().filter(|e| e.assembly_model().is_some()) {
            // This document at a version: not the assembly itself, nor one holding it.
            if src.this && cadrs_core::assembly::structure::contains_assembly(&src.doc, el.id, s.element) {
                continue;
            }
            if !el.name.to_lowercase().contains(&q) {
                continue;
            }
            let (Some(link), Some(snap)) = (item_link(el.id), src.snapshot(el.id)) else { continue };
            let root = snap.root;
            out.push((Item { sources: vec![InstanceSource::Assembly { element: root }], label: el.name.clone(), rigid: Vec::new(), link: Some(link) }, Some((root, None)), false));
        }
        return out;
    }
    for el in src.doc.elements.iter().filter(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. })) {
        let (Some(link), Some(snap)) = (item_link(el.id), src.snapshot(el.id)) else { continue };
        let root = snap.root;
        let Some(build) = parts_of.build(doc, root) else { continue };
        let parts: Vec<&cadrs_core::Part> = build
            .parts
            .iter()
            .filter(|p| match p.kind {
                PartKind::Solid => s.parts,
                PartKind::Surface => s.surfaces,
            })
            .collect();
        let studio_hit = el.name.to_lowercase().contains(&q);
        let names: Vec<String> = parts.iter().map(|p| cadrs_core::parts::display_name(p, el.part_props()).to_string()).collect();
        let hit = |i: usize| -> bool {
            if studio_hit || names[i].to_lowercase().contains(&q) {
                return true;
            }
            el.part_prop(parts[i].id).is_some_and(|pp| {
                [&pp.properties.part_number, &pp.properties.description].iter().any(|v| v.as_ref().is_some_and(|v| v.to_lowercase().contains(&q)))
            })
        };
        let shown: Vec<usize> = (0..parts.len()).filter(|i| hit(*i)).collect();
        if shown.is_empty() {
            continue;
        }
        let studio_item = if s.rigid {
            Item { sources: vec![InstanceSource::Studio { element: root }], label: el.name.clone(), rigid: parts.iter().map(|p| p.id).collect(), link: Some(link.clone()) }
        } else {
            Item { sources: parts.iter().map(|p| InstanceSource::Part { element: root, part: p.id }).collect(), label: el.name.clone(), rigid: Vec::new(), link: Some(link.clone()) }
        };
        out.push((studio_item, Some((root, None)), true));
        for i in shown {
            out.push((
                Item { sources: vec![InstanceSource::Part { element: root, part: parts[i].id }], label: names[i].clone(), rigid: Vec::new(), link: Some(link.clone()) },
                Some((root, Some(parts[i].id))),
                false,
            ));
        }
    }
    out
}

/// The copies of the source shown are there for building and ghosts.
pub fn sync_linked_extra(session: Option<Res<InsertSession>>, mut parts_of: ResMut<super::AssemblyParts>) {
    let want: &[cadrs_core::external::LinkedElement] = match session.as_ref().and_then(|s| s.linked_source()) {
        Some(o) => &o.links,
        None => &[],
    };
    let same = parts_of.linked_extra.len() == want.len() && parts_of.linked_extra.iter().zip(want).all(|(a, b)| a.id() == b.id());
    if !same {
        parts_of.linked_extra = want.to_vec();
    }
}

/// The Other documents search field.
pub fn read_other_search(q: Query<(&Name, &bevy::text::EditableText)>, session: Option<ResMut<InsertSession>>) {
    let Some(mut s) = session else { return };
    if let Some((_, t)) = q.iter().find(|(n, _)| n.as_str() == "insert-other-search-field") {
        let v = t.value().to_string();
        if s.browse.search != v {
            s.browse.search = v;
        }
    }
}

/// Rebuilds the Other documents panel when its state changes; a new search only rebuilds the
/// list under the field, so the field keeps its focus and caret.
fn sync_other_panel(world: &mut World, mut last: Local<Option<(linked::Browse, linked::BrowserList)>>, mut seen_generation: Local<u64>) {
    let Some(s) = world.get_resource::<InsertSession>() else {
        *last = None;
        return;
    };
    let browse = s.browse.clone();
    // The library is read in the background (`linked::LibrarySnapshot`): a read that lands
    // rebuilds the list too (P3E.2 merge: the folders came after the first frame).
    let generation = world.resource::<linked::LibrarySnapshot>().generation;
    let landed = generation != *seen_generation;
    *seen_generation = generation;
    let mut q = world.query_filtered::<(Entity, Ref<InsertOtherPanel>), ()>();
    let Some((panel, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else { return };
    if !(added || landed && browse.opened.is_none()) && last.as_ref().is_some_and(|(b, _)| *b == browse) {
        return;
    }
    if added {
        world.resource_mut::<linked::LibrarySnapshot>().invalidate();
    }
    let list = if browse.opened.is_none() { linked::browser_list(world, &browse) } else { linked::BrowserList::default() };
    let thumbs = linked::thumbs_for(world, &list.rows);
    let theme = world.resource::<Theme>().clone();
    let only_search = !added && browse.opened.is_none() && last.as_ref().is_some_and(|(b, _)| b.opened.is_none() && b.location == browse.location && (b.search != browse.search || landed));
    let mut qn = world.query::<(Entity, &Name)>();
    let list_node = qn.iter(world).find(|(_, n)| n.as_str() == "insert-other-list").map(|(e, _)| e);
    match (only_search, list_node) {
        (true, Some(node)) => {
            world.entity_mut(node).despawn();
            let mut commands = world.commands();
            commands.entity(panel).with_children(|p| linked::spawn_browser_list(p, &theme, Owner::Insert, &browse, &list, &thumbs));
        }
        _ => {
            world.entity_mut(panel).despawn_children();
            let mut commands = world.commands();
            commands.entity(panel).with_children(|p| match &browse.opened {
                None => linked::spawn_browser(p, &theme, Owner::Insert, &browse, &list, &thumbs),
                Some(o) => {
                    p.spawn(OpenedHeader::new("insert-other", o.name.clone(), o.version_label()).graph_open(browse.graph).build(&theme));
                    if o.version.is_none() {
                        linked::spawn_no_version(p, &theme, Owner::Insert);
                    }
                }
            });
        }
    }
    world.flush();
    *last = Some((browse, list));
}

/// The version graph while it is open: this document's (Current document) or the opened
/// document's.
/// What the graph panel was built from.
type GraphKey = (bool, Option<cadrs_core::DocumentId>, Option<cadrs_core::history_log::VersionId>, u64);

fn sync_graph_panel(world: &mut World, mut last: Local<Option<GraphKey>>) {
    let Some(s) = world.get_resource::<InsertSession>() else {
        *last = None;
        return;
    };
    if !s.graph_open() {
        *last = None;
        return;
    }
    let (document, selected) = if s.other {
        let Some(o) = s.browse.opened.as_ref() else { return };
        (o.document, o.version.as_ref().map(|v| v.0))
    } else {
        (world.resource::<ActiveDocument>().doc.id, s.current.as_ref().and_then(|o| o.version.as_ref().map(|v| v.0)))
    };
    let generation = world.resource::<crate::history_panel::DocLog>().generation;
    let key = (s.other, Some(document), selected, generation);
    let mut q = world.query_filtered::<(Entity, Ref<InsertGraphPanel>), ()>();
    let Some((panel, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else { return };
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    let (graph, _) = linked::version_graph(world, "insert-graph", document, selected);
    let theme = world.resource::<Theme>().clone();
    world.entity_mut(panel).despawn_children();
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        p.spawn(graph.build(&theme));
    });
    world.flush();
    *last = Some(key);
}

fn on_button(a: On<Activate>, q: Query<&Name>, session: Option<Res<InsertSession>>, mut commands: Commands) {
    let Some(s) = session else { return };
    let Ok(n) = q.get(a.entity) else { return };
    let opened = s.browse.opened.as_ref().map(|o| o.document);
    match n.as_str() {
        "insert-other-back" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::Insert, |b| {
                b.opened = None;
                b.graph = false;
            });
            if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                s.armed = None;
                s.pending = false;
            }
        }),
        "insert-other-graph" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::Insert, |b| b.graph = !b.graph);
        }),
        "insert-other-create-version" | "insert-other-no-version-create" => {
            if let Some(d) = opened {
                commands.queue(move |world: &mut World| linked::open_create_version_dialog(world, VersionTarget::Other(d, Owner::Insert)));
            }
        }
        _ => {}
    }
}

/// A node of the version graph: the Other documents tab opens that version; the Current document
/// tab reads this document at it (or its workspace, Main).
fn on_graph_pick(ev: On<VersionGraphSelect>, session: Option<Res<InsertSession>>, mut commands: Commands) {
    if ev.graph != "insert-graph" || session.is_none() {
        return;
    }
    let index = ev.index;
    commands.queue(move |world: &mut World| {
        let Some(s) = world.get_resource::<InsertSession>() else { return };
        let other = s.other;
        let document = if other { s.browse.opened.as_ref().map(|o| o.document) } else { Some(world.resource::<ActiveDocument>().doc.id) };
        let Some(document) = document else { return };
        let (_, picks) = linked::version_graph(world, "insert-graph", document, None);
        let Some(pick) = picks.get(index).copied() else { return };
        if other {
            if let GraphPick::Version(v) = pick {
                linked::open_in(world, Owner::Insert, document, Some(v));
            }
            return;
        }
        let current = match pick {
            GraphPick::Workspace => None,
            GraphPick::Version(v) => match linked::open(world, document, Some(v)) {
                Ok(o) => Some(o),
                Err(m) => {
                    linked::toast(world, m);
                    return;
                }
            },
        };
        if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
            s.current = current;
            s.current_graph = false;
            s.armed = None;
            s.pending = false;
        }
    });
}

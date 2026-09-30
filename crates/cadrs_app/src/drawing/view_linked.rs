//! P3G.1: the drawing **Insert view** browser's Other documents tab and version graph (D13.1,
//! X14, ER1.1, ER3.5; `external-references.md` ER1.7: "a drawing that must show an older
//! released revision"). The browser is [`crate::linked`]'s; this module keeps the Insert view
//! state's lists in step with the source picked:
//!
//! - **Current document**: the header's version graph button swaps the list for the graph (Main,
//!   the versions, Start); picking a version lists this document's studios and assemblies as
//!   they were then ("↳ V1" under the name). A view of it references that version: its first
//!   placement stores the frozen copy with the view (one undo step), so edits in the workspace
//!   never change it and never make it out of date; a workspace view still follows the studio
//!   and goes out of date as before (P3C.6).
//! - **Other documents**: the locations, a document at its newest version (or picked in the
//!   graph), its studios and assemblies; views of them work the same way.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::{Document, ElementId, ElementKind, PartId};
use cadrs_drawing::ObjectRef;
use cadrs_ui::{OpenedHeader, TabStripSelect, Theme, ToolButton, VersionGraphSelect};

use super::view_tools::{BrowserStudio, InsertViewState, ViewTool};
use super::DrawingUi;
use crate::linked::{self, GraphPick, Opened, Owner, VersionTarget};
use crate::{ActiveDocument, AppState};

pub struct ViewLinkedPlugin;

impl Plugin for ViewLinkedPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_sources.run_if(in_state(AppState::Document)))
            .add_observer(on_source_tab)
            .add_observer(on_button)
            .add_observer(on_graph_pick);
    }
}

/// The source the browser lists: the opened other document, or this one at a version; `None`
/// for this document's workspace.
pub fn linked_source(st: &InsertViewState) -> Option<&Opened> {
    if st.source_tab == 1 { st.browse.opened.as_ref().filter(|o| o.version.is_some()) } else { st.current.as_ref() }
}

/// The version graph is showing.
pub fn graph_open(st: &InsertViewState) -> bool {
    if st.source_tab == 1 { st.browse.opened.is_some() && st.browse.graph } else { st.current_graph }
}

/// The studios (with their solid parts) and assemblies of `doc`, under the ids `id` gives them.
fn lists(doc: &Document, id: impl Fn(ElementId) -> Option<ElementId>) -> (Vec<BrowserStudio>, Vec<(ElementId, String)>) {
    let mut studios = Vec::new();
    let mut assemblies = Vec::new();
    for el in &doc.elements {
        let Some(to) = id(el.id) else { continue };
        match &el.kind {
            ElementKind::PartStudio { .. } => {
                let build = cadrs_core::rebuild::build(el.features());
                let props = el.part_props();
                let parts: Vec<(PartId, String)> = build
                    .parts
                    .iter()
                    .filter(|p| p.kind == cadrs_core::PartKind::Solid)
                    .map(|p| (p.id, cadrs_core::parts::display_name(p, props).to_string()))
                    .collect();
                studios.push(BrowserStudio { id: to, name: el.name.clone(), parts });
            }
            ElementKind::Assembly => assemblies.push((to, el.name.clone())),
            _ => {}
        }
    }
    (studios, assemblies)
}

type SourcesKey = (usize, linked::Browse, Option<Opened>, bool, u64);

/// Keeps the state's lists, the Other documents list and the version graph in step with the
/// source picked.
fn sync_sources(world: &mut World, mut last: Local<Option<SourcesKey>>) {
    if world.resource::<DrawingUi>().tool != ViewTool::Insert {
        *last = None;
        return;
    }
    let st = world.resource::<InsertViewState>().clone();
    // With the background library reads' generation (P3E.2 merge: a read that lands rebuilds).
    let generation = world.resource::<crate::history_panel::DocLog>().generation + (world.resource::<linked::LibrarySnapshot>().generation << 32);
    let key: SourcesKey = (st.source_tab, st.browse.clone(), st.current.clone(), st.current_graph, generation);
    if last.as_ref() == Some(&key) {
        return;
    }
    let first = last.is_none();
    if first {
        world.resource_mut::<linked::LibrarySnapshot>().invalidate();
    }
    let source_changed = last.as_ref().is_none_or(|k| k.0 != key.0 || k.1.opened != key.1.opened || k.2 != key.2);
    *last = Some(key);
    let mut next = st.clone();
    // The documents list (Other documents, nothing opened).
    if st.source_tab == 1 && st.browse.opened.is_none() {
        let list = linked::browser_list(world, &st.browse);
        next.other_thumbs = linked::thumbs_for(world, &list.rows);
        next.other_list = list;
    }
    // The graph.
    next.graph = if graph_open(&st) {
        let (document, selected) = if st.source_tab == 1 {
            let o = st.browse.opened.as_ref().expect("graph_open");
            (o.document, o.version.as_ref().map(|v| v.0))
        } else {
            (world.resource::<ActiveDocument>().doc.id, st.current.as_ref().and_then(|o| o.version.as_ref().map(|v| v.0)))
        };
        Some(linked::version_graph(world, "insert-view-graph", document, selected).0)
    } else {
        None
    };
    // The lists, from the source (the first time they come from `open_insert_view`).
    if source_changed && !first {
        let (studios, assemblies) = match linked_source(&st) {
            Some(src) => lists(&src.doc, |e| src.snapshot(e).map(|s| s.root)),
            None if st.source_tab == 1 => (Vec::new(), Vec::new()),
            None => lists(&world.resource::<ActiveDocument>().doc, Some),
        };
        next.expanded = studios.first().map(|s| s.id).into_iter().collect();
        next.studios = studios;
        next.assemblies = assemblies;
        next.reference = None;
        next.search.clear();
    }
    let mut s = world.resource_mut::<InsertViewState>();
    if *s != next {
        *s = next;
    }
}

/// The browser's part between its source tabs and its lists: this document's header (name,
/// "Main" or "↳ V1", Create version, Version graph), or the Other documents browser (the
/// locations and documents, or the opened document's header); then the graph when it is open.
/// Returns whether the Part Studios | Assemblies lists follow.
pub fn source_section(c: &mut ChildSpawnerCommands, t: &Theme, st: &InsertViewState) -> bool {
    if st.source_tab == 1 {
        match &st.browse.opened {
            None => {
                linked::spawn_browser(c, t, Owner::InsertView, &st.browse, &st.other_list, &st.other_thumbs);
                return false;
            }
            Some(o) => {
                c.spawn(OpenedHeader::new("insert-view-other", o.name.clone(), o.version_label()).graph_open(st.browse.graph).build(t));
                if o.version.is_none() {
                    linked::spawn_no_version(c, t, Owner::InsertView);
                    return false;
                }
            }
        }
    } else {
        let version = st.current.as_ref().map(|o| o.version_label()).unwrap_or_else(|| "Main".into());
        c.spawn(Node {
            padding: UiRect::new(Val::Px(8.0), Val::Px(6.0), Val::Px(6.0), Val::Px(4.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|r| {
            r.spawn(Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: Val::Px(0.0), overflow: Overflow::clip(), ..default() }).with_children(|d| {
                d.spawn((t.text(st.document.clone(), t.font_base, FontWeight::BOLD, t.foreground), Pickable::IGNORE)).insert(TextLayout::no_wrap());
                d.spawn(Node { column_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() }).with_children(|m| {
                    m.spawn((cadrs_ui::icon("branches", 12.0, t.muted_foreground), Pickable::IGNORE));
                    m.spawn((Name::new("insert-view-branch-label"), t.text(version, t.font_sm, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
                });
            });
            for (name, icon, tip, on) in [
                ("insert-view-create-version", "versions", "Create version", false),
                ("insert-view-version", "branches", "Version graph: views of the workspace or of a version", st.current_graph),
            ] {
                r.spawn(ToolButton::new(name, icon).icon_size(16.0).tooltip(tip).selected(on).build(t))
                    .insert(BorderColor::all(Color::srgb_u8(0xd4, 0xd4, 0xd4)))
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(26.0);
                        n.height = Val::Px(26.0);
                        n.border = UiRect::all(Val::Px(1.0));
                        n.flex_shrink = 0.0;
                    });
            }
        });
    }
    if let Some(g) = &st.graph {
        c.spawn((Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },)).with_children(|p| {
            p.spawn(g.clone().build(t));
        });
        return false;
    }
    true
}

/// The copies a view of `element` needs when they are not in the document yet (a view of a
/// version or of another document, placed for the first time).
pub fn pending_links(world: &World, element: uuid::Uuid) -> Vec<cadrs_core::external::LinkedElement> {
    let element = ElementId(element);
    let doc = &world.resource::<ActiveDocument>().doc;
    if doc.element(element).is_some() {
        return Vec::new();
    }
    let st = world.resource::<InsertViewState>();
    let Some(src) = linked_source(st) else { return Vec::new() };
    src.roots.iter().find(|(_, s)| s.root == element).map(|(_, s)| s.links.clone()).unwrap_or_default()
}

/// [`super::view_tools::insert_view_op`] for a document `doc` (with the copies a linked view
/// needs) and its drawing `d`: the view with its source's hash, and the source recorded the
/// first time.
pub fn insert_view_op_in(doc: &Document, d: &cadrs_drawing::Drawing, sheet: cadrs_drawing::SheetId, mut v: cadrs_drawing::View) -> cadrs_drawing::DrawingOp {
    use cadrs_drawing::DrawingOp;
    if let Some(src) = d.source(v.reference.element) {
        v.source_hash = src.hash_of(v.reference.part);
        return DrawingOp::InsertView { sheet, view: v };
    }
    match cadrs_core::drawing_source::live_source(doc, ElementId(v.reference.element)) {
        Some(src) => {
            v.source_hash = src.hash_of(v.reference.part);
            let insert = DrawingOp::InsertView { sheet, view: v };
            let label = insert.label();
            DrawingOp::Batch { ops: vec![DrawingOp::SetSource(src), insert], label }
        }
        None => DrawingOp::InsertView { sheet, view: v },
    }
}

/// The document a ghost of the Insert view is projected from: with the copies of a linked
/// source not inserted yet.
pub fn ghost_doc(doc: &Document, st: &InsertViewState) -> Option<Document> {
    let r = st.reference?;
    if doc.element(ElementId(r.element)).is_some() {
        return None;
    }
    let src = linked_source(st)?;
    Some(linked::with_links(doc, &src.links))
}

fn on_source_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut state: ResMut<InsertViewState>) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "insert-view-source-tabs") && state.source_tab != ev.index {
        state.source_tab = ev.index;
    }
}

fn on_button(a: On<Activate>, q: Query<&Name>, ui: Res<DrawingUi>, state: Res<InsertViewState>, mut commands: Commands) {
    if ui.tool != ViewTool::Insert {
        return;
    }
    let Ok(n) = q.get(a.entity) else { return };
    let opened = state.browse.opened.as_ref().map(|o| o.document);
    match n.as_str() {
        "insert-view-other-back" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::InsertView, |b| {
                b.opened = None;
                b.graph = false;
            });
        }),
        "insert-view-other-graph" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::InsertView, |b| b.graph = !b.graph);
        }),
        "insert-view-other-create-version" | "insert-view-other-no-version-create" => {
            if let Some(d) = opened {
                commands.queue(move |world: &mut World| linked::open_create_version_dialog(world, VersionTarget::Other(d, Owner::InsertView)));
            }
        }
        "insert-view-version" => commands.queue(|world: &mut World| {
            let mut s = world.resource_mut::<InsertViewState>();
            s.current_graph = !s.current_graph;
        }),
        "insert-view-create-version" => commands.queue(|world: &mut World| linked::open_create_version_dialog(world, VersionTarget::Current)),
        _ => {}
    }
}

fn on_graph_pick(ev: On<VersionGraphSelect>, mut commands: Commands) {
    if ev.graph != "insert-view-graph" {
        return;
    }
    let index = ev.index;
    commands.queue(move |world: &mut World| {
        let st = world.resource::<InsertViewState>();
        let other = st.source_tab == 1;
        let document = if other { st.browse.opened.as_ref().map(|o| o.document) } else { Some(world.resource::<ActiveDocument>().doc.id) };
        let Some(document) = document else { return };
        let (_, picks) = linked::version_graph(world, "insert-view-graph", document, None);
        let Some(pick) = picks.get(index).copied() else { return };
        if other {
            if let GraphPick::Version(v) = pick {
                linked::open_in(world, Owner::InsertView, document, Some(v));
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
        let mut s = world.resource_mut::<InsertViewState>();
        s.current = current;
        s.current_graph = false;
    });
}

/// The object a reference names in the browser's lists, for the card ("Block (V1)").
pub fn version_suffix(st: &InsertViewState, r: &ObjectRef) -> Option<String> {
    let src = linked_source(st)?;
    src.roots.iter().any(|(_, s)| s.root.0 == r.element).then(|| src.version_label())
}

/// Thumbnails are not shown in the drawing browser (as the Current document tab).
pub type Thumbs = HashMap<cadrs_core::DocumentId, Handle<Image>>;

//! P3E.2: the full **Tab manager** (`test-drive.md` TD5.4, X6; `essential-tips.md` T1.2, X1;
//! `external-references.md` ER7.7; `reference/onshape/tab_menu.md`, `tab_manager_open-01.png`).
//! It grew out of P3G.3's minimal manager (in `move_document.rs`, made scrollable by P3F.3) and
//! keeps its multi-select and Move to document.
//!
//! The tab bar's leftmost icon opens it: a **docked left panel** (300 px, full height between
//! the icon rail and the toolbar and feature list, which it pushes right) with
//! - the header "Tabs" with **Sort ▾** (Document order, Name A–Z, Name Z–A, Type) and ✕;
//! - a **Search tabs** field, the **type filters** (Part Studios, Assemblies, Drawings: toggles)
//!   and **Clear**;
//! - **New folder** (the selection goes into a new folder, renamed in place in its row);
//! - the **vertical list**: two-line rows with a thumbnail (the Part Studio's or Assembly's parts
//!   drawn small, else the type icon), the name and the type in grey italic; the active tab's
//!   row is light blue with a dark-blue bar at its left and its name bold. Folders are
//!   expandable rows ("Plates", "Folder · 3 tabs") with their tabs indented under them. While
//!   searching, filtering or sorted, the matching tabs are listed flat with their folder's name;
//! - a click opens a tab (and selects it), Ctrl+click adds or removes a row, Shift+click selects
//!   a range; a folder row's click opens or closes it;
//! - **dragging** rows (a selected row drags the whole selection; the dragged rows dim) shows a
//!   blue line where they land, or a box round a folder row they drop into: one undo step
//!   ([`cadrs_core::tab_tree::MoveTabItems`]);
//! - a row's right-click: Move to document…, New folder from selection, Move to top level, and
//!   for a folder Open in tab bar, Rename…, Delete folder…;
//! - **Move to document…** (P3G.3) for the selected tabs (a folder's tabs with it; the new
//!   document is named after the folder);
//! - at the bottom, a large **preview** of the selected (or active) tab.
//!
//! Names: `tab-manager-dock`, `tab-manager-panel`, `tab-manager-close`, `tab-manager-sort`
//! (`tab-manager-sort-menu`: `tab-manager-sort-document`, `-name-asc`, `-name-desc`, `-type`),
//! `tab-manager-search` (field `tab-manager-search-field`), `tab-manager-filter-part-studios`,
//! `-assemblies`, `-drawings`, `tab-manager-clear`, `tab-manager-new-folder`,
//! `tab-manager-count`, `tab-manager-rows`, `tab-manager-row-<k>` (the k-th tab row),
//! `tab-manager-folder-<name>`, `tab-manager-empty`, `tab-manager-move`, `tab-manager-preview`
//! (`tab-manager-preview-name`), `tab-manager-menu` (`tab-manager-menu-*`),
//! `tab-manager-drop-line`, `tab-manager-drag-ghost`, `tab-manager-folder-rename-field`.

use std::collections::HashMap;

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::tab_tree::{self, MoveTabItems, RenameTabFolder, TabItem, TabNode};
use cadrs_core::{ElementId, ElementKind};
use cadrs_ui::menu::{ContextMenuAnchor, ContextMenuRequested, ContextMenuTarget, Menu, MenuAction, MenuItem, open_context_menu, open_menu};
use cadrs_ui::prelude::*;
use cadrs_ui::{InlineEditCommit, InlineEditOptions, begin_inline_edit};

use crate::{ActiveDocument, AppState};

pub struct TabManagerPlugin;

impl Plugin for TabManagerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TabManager>()
            .init_resource::<ManagerDrag>()
            .init_resource::<TabThumbs>()
            .init_resource::<PendingRowRename>()
            .add_systems(Update, (read_search, sync_panel, fill_thumbs, sync_filters, sync_rows, sync_preview, start_row_rename, place_drop).chain().run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut tm: ResMut<TabManager>, mut d: ResMut<ManagerDrag>, mut th: ResMut<TabThumbs>| {
                *tm = TabManager::default();
                *d = ManagerDrag::default();
                *th = TabThumbs::default();
            })
            .add_observer(on_button)
            .add_observer(on_sort_action)
            .add_observer(on_row_menu)
            .add_observer(on_row_menu_action)
            .add_observer(on_row_rename_commit)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end);
    }
}

/// Which tabs the type filter shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TypeFilter {
    #[default]
    All,
    PartStudios,
    Assemblies,
    Drawings,
}

impl TypeFilter {
    fn keeps(self, kind: &ElementKind) -> bool {
        match self {
            TypeFilter::All => true,
            TypeFilter::PartStudios => matches!(kind, ElementKind::PartStudio { .. }),
            TypeFilter::Assemblies => matches!(kind, ElementKind::Assembly),
            TypeFilter::Drawings => matches!(kind, ElementKind::Drawing(_)),
        }
    }
}

/// The list's order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortBy {
    /// The tab bar's order, with folders.
    #[default]
    Document,
    NameAsc,
    NameDesc,
    Type,
}

/// The Tab manager's state.
#[derive(Resource, Debug, Clone, Default, PartialEq)]
pub struct TabManager {
    pub open: bool,
    /// The selected rows: tabs and folders (a folder's id is a tab-tree id).
    pub selected: Vec<ElementId>,
    /// Where a Shift+click range starts.
    pub anchor: Option<ElementId>,
    pub search: String,
    pub filter: TypeFilter,
    pub sort: SortBy,
    /// Folders closed in the list (open by default).
    pub collapsed: Vec<ElementId>,
}

impl TabManager {
    /// A flat list (searching, filtering or sorted) rather than the folder tree.
    fn flat(&self) -> bool {
        !self.search.trim().is_empty() || self.filter != TypeFilter::All || self.sort != SortBy::Document
    }
}

/// A tab's icon.
pub fn tab_icon(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "part-studio",
        ElementKind::Assembly => "assembly",
        ElementKind::Drawing(_) => "details",
        ElementKind::Render(_) => "render-studio",
        ElementKind::PcbStudio(_) => "pcb-studio",
    }
}

/// A tab's type as the list's second line says it.
pub fn tab_type(el: &cadrs_core::Element) -> &'static str {
    match el.kind {
        ElementKind::PartStudio { .. } => "Part Studio",
        ElementKind::Assembly => "Assembly",
        ElementKind::Drawing(_) => "Drawing",
        ElementKind::Render(_) => "Render Studio",
        ElementKind::PcbStudio(_) => "PCB Studio",
    }
}

/// The dock between the icon rail and the rest of the window (`document.rs` spawns it).
#[derive(Component)]
pub struct TabManagerDock;

#[derive(Component)]
struct TabManagerPanel;

/// The rows' container (rebuilt when the list changes).
#[derive(Component)]
struct RowsBox;

/// The filter toggles and Clear (rebuilt when the filters change).
#[derive(Component)]
struct FilterBox;

/// The preview at the bottom (rebuilt when the previewed tab changes).
#[derive(Component)]
struct PreviewBox;

#[derive(Component)]
struct CountText;

/// A row: its item, depth and the folder it is in.
#[derive(Component, Debug, Clone, Copy)]
struct ManagerRow {
    item: TabItem,
    depth: usize,
    parent: Option<ElementId>,
}

/// One row of the list as shown.
#[derive(Debug, Clone, PartialEq)]
struct RowSpec {
    item: TabItem,
    depth: usize,
    parent: Option<ElementId>,
    name: String,
    icon: &'static str,
    /// The second line: the type ("Part Studio"), or a folder's "Folder · 3 tabs".
    kind: String,
    /// A folder: open, and its number of tabs.
    folder: Option<(bool, usize)>,
    /// While the list is flat: the folder a tab is in.
    location: Option<String>,
}

/// The rows the list shows now.
fn rows_of(doc: &cadrs_core::Document, tm: &TabManager) -> Vec<RowSpec> {
    let layout = tab_tree::layout(doc);
    let mut out = Vec::new();
    let needle = tm.search.trim().to_lowercase();
    let folder_kind = |n: usize| if n == 1 { "Folder · 1 tab".to_string() } else { format!("Folder · {n} tabs") };
    if !tm.flat() {
        fn walk(doc: &cadrs_core::Document, tm: &TabManager, nodes: &[TabNode], depth: usize, parent: Option<ElementId>, out: &mut Vec<RowSpec>, folder_kind: &dyn Fn(usize) -> String) {
            for n in nodes {
                match n {
                    TabNode::Tab(e) => {
                        let Some(el) = doc.element(*e) else { continue };
                        out.push(RowSpec { item: TabItem::Tab(*e), depth, parent, name: el.name.clone(), icon: tab_icon(el), kind: tab_type(el).into(), folder: None, location: None });
                    }
                    TabNode::Folder { id, name, children } => {
                        let open = !tm.collapsed.contains(id);
                        let count = n.tabs().len();
                        out.push(RowSpec { item: TabItem::Folder(*id), depth, parent, name: name.clone(), icon: "folder", kind: folder_kind(count), folder: Some((open, count)), location: None });
                        if open {
                            walk(doc, tm, children, depth + 1, Some(*id), out, folder_kind);
                        }
                    }
                }
            }
        }
        walk(doc, tm, &layout, 0, None, &mut out, &folder_kind);
    } else {
        for e in tab_tree::flatten(&layout) {
            let Some(el) = doc.element(e) else { continue };
            if !tm.filter.keeps(&el.kind) || !el.name.to_lowercase().contains(&needle) {
                continue;
            }
            let parent = tab_tree::parent_of(&layout, TabItem::Tab(e));
            let location = parent.and_then(|p| doc.tab_tree.folder(p)).map(|f| f.name.clone());
            out.push(RowSpec { item: TabItem::Tab(e), depth: 0, parent, name: el.name.clone(), icon: tab_icon(el), kind: tab_type(el).into(), folder: None, location });
        }
        let lower = |r: &RowSpec| r.name.to_lowercase();
        match tm.sort {
            SortBy::Document => {}
            SortBy::NameAsc => out.sort_by(|a, b| cadrs_core::library::natural_cmp(&lower(a), &lower(b))),
            SortBy::NameDesc => out.sort_by(|a, b| cadrs_core::library::natural_cmp(&lower(b), &lower(a))),
            SortBy::Type => out.sort_by(|a, b| a.kind.cmp(&b.kind)),
        }
    }
    out
}

fn on_button(a: On<Activate>, q: Query<&Name>, mut tm: ResMut<TabManager>, theme: Res<Theme>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else { return };
    let toggle = |tm: &mut TabManager, f: TypeFilter| tm.filter = if tm.filter == f { TypeFilter::All } else { f };
    match name.as_str() {
        "tab-manager" => {
            tm.open = !tm.open;
            if tm.open && tm.selected.is_empty() {
                tm.anchor = None;
            }
        }
        "tab-manager-close" => tm.open = false,
        "tab-manager-new-folder" => {
            let sel = tm.selected.clone();
            commands.queue(move |w: &mut World| new_folder_from(w, sel));
        }
        "tab-manager-move" => {
            let sel = tm.selected.clone();
            commands.queue(move |w: &mut World| move_to_document(w, sel));
        }
        "tab-manager-filter-part-studios" => toggle(&mut tm, TypeFilter::PartStudios),
        "tab-manager-filter-assemblies" => toggle(&mut tm, TypeFilter::Assemblies),
        "tab-manager-filter-drawings" => toggle(&mut tm, TypeFilter::Drawings),
        "tab-manager-clear" => {
            tm.filter = TypeFilter::All;
            tm.sort = SortBy::Document;
            if !tm.search.is_empty() {
                tm.search.clear();
                commands.queue(|w: &mut World| set_search_field(w, ""));
            }
        }
        "tab-manager-sort" => {
            let sort = tm.sort;
            let item = |id: &'static str, label: &str, s: SortBy| MenuItem::new(id, label).checked(sort == s);
            let menu = Menu::new("tab-manager-sort-menu")
                .min_width(170.0)
                .item_height(22.0)
                .text_only()
                .item(item("tab-manager-sort-document", "Document order", SortBy::Document))
                .item(item("tab-manager-sort-name-asc", "Name A–Z", SortBy::NameAsc))
                .item(item("tab-manager-sort-name-desc", "Name Z–A", SortBy::NameDesc))
                .item(item("tab-manager-sort-type", "Type", SortBy::Type));
            open_menu(&mut commands, a.entity, menu.build(&theme));
        }
        _ => {}
    }
}

fn on_sort_action(ev: On<MenuAction>, mut tm: ResMut<TabManager>) {
    let s = match ev.item.as_str() {
        "tab-manager-sort-document" => SortBy::Document,
        "tab-manager-sort-name-asc" => SortBy::NameAsc,
        "tab-manager-sort-name-desc" => SortBy::NameDesc,
        "tab-manager-sort-type" => SortBy::Type,
        _ => return,
    };
    if tm.sort != s {
        tm.sort = s;
    }
}

fn set_search_field(world: &mut World, value: &str) {
    let mut q = world.query::<(&Name, &mut bevy::text::EditableText)>();
    for (n, mut t) in q.iter_mut(world) {
        if n.as_str() == "tab-manager-search-field" {
            t.queue_edit(bevy::text::TextEdit::SelectAll);
            if value.is_empty() {
                t.queue_edit(bevy::text::TextEdit::Backspace);
            } else {
                t.queue_edit(bevy::text::TextEdit::Insert(value.to_string().into()));
            }
        }
    }
}

fn read_search(q: Query<(&Name, &bevy::text::EditableText)>, mut tm: ResMut<TabManager>) {
    if let Some(v) = q.iter().find(|(n, _)| n.as_str() == "tab-manager-search-field").map(|(_, t)| t.value().to_string())
        && tm.search != v
    {
        tm.search = v;
    }
}

/// The selected tabs, a selected folder's tabs included, in the document's order.
fn selected_tabs(doc: &cadrs_core::Document, sel: &[ElementId]) -> Vec<ElementId> {
    let layout = tab_tree::layout(doc);
    let mut picked: Vec<ElementId> = Vec::new();
    for s in sel {
        if doc.tab_tree.folder(*s).is_some() {
            picked.extend(tab_tree::tabs_in(&layout, *s));
        } else {
            picked.push(*s);
        }
    }
    tab_tree::flatten(&layout).into_iter().filter(|e| picked.contains(e)).collect()
}

/// The selection as tree items, in list order, without those inside a selected folder.
fn selected_items(doc: &cadrs_core::Document, sel: &[ElementId]) -> Vec<TabItem> {
    let layout = tab_tree::layout(doc);
    let mut order = Vec::new();
    fn walk(nodes: &[TabNode], out: &mut Vec<TabItem>) {
        for n in nodes {
            out.push(n.item());
            if let TabNode::Folder { children, .. } = n {
                walk(children, out);
            }
        }
    }
    walk(&layout, &mut order);
    let inside_selected = |i: &TabItem| {
        let mut p = tab_tree::parent_of(&layout, *i);
        while let Some(f) = p {
            if sel.contains(&f) {
                return true;
            }
            p = tab_tree::parent_of(&layout, TabItem::Folder(f));
        }
        false
    };
    order.into_iter().filter(|i| sel.contains(&i.id()) && !inside_selected(i)).collect()
}

fn move_to_document(world: &mut World, sel: Vec<ElementId>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let tabs = selected_tabs(&doc.doc, &sel);
    if tabs.is_empty() {
        return;
    }
    // A folder moves under its own name (P3E.2 judge).
    let name = match sel.as_slice() {
        [one] => doc.doc.tab_tree.folder(*one).map(|f| f.name.clone()),
        _ => None,
    };
    // P3E.3a (P3E.2 carried delta): the docked manager stays open under the dialog.
    crate::move_document::open_move_dialog_named(world, tabs, name);
}

fn new_folder_from(world: &mut World, sel: Vec<ElementId>) {
    let items = match world.get_resource::<ActiveDocument>() {
        Some(d) => selected_items(&d.doc, &sel),
        None => return,
    };
    if let Some(f) = crate::tab_folders::create_folder_with(world, items, false) {
        let mut tm = world.resource_mut::<TabManager>();
        tm.selected = vec![f];
        tm.anchor = Some(f);
        // Renamed in place in its row (P3E.2 judge).
        world.resource_mut::<PendingRowRename>().0 = Some(f);
    }
}

// ---------------------------------------------------------------------------------------------
// Thumbnails

/// Tab thumbnails: a Part Studio's or an Assembly's parts drawn small (the insert dialog's
/// renderer), kept while the tab is unchanged. A few are drawn per frame.
#[derive(Resource, Default)]
pub struct TabThumbs {
    map: HashMap<ElementId, (cadrs_core::Element, Option<Handle<Image>>)>,
    pub generation: u64,
}

/// Studios bigger than this aren't rebuilt for their thumbnail: it is drawn from the parts on
/// screen once the studio has been open (P3E.3a), and shows the icon until then.
const THUMB_FEATURE_LIMIT: usize = 60;
const THUMB_SIZE: u32 = 160;

fn fill_thumbs(world: &mut World) {
    if !world.resource::<TabManager>().open || !world.resource::<ManagerDrag>().items.is_empty() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>().map(|d| d.doc.clone()) else { return };
    let mut budget = 2;
    for el in &doc.elements {
        if budget == 0 {
            break;
        }
        let fresh = world.resource::<TabThumbs>().map.get(&el.id).is_some_and(|(e, _)| e == el);
        if fresh {
            continue;
        }
        // P3E.3a (P3E.2 carried delta: a big studio's preview was its icon): a studio too big
        // to rebuild again for its thumbnail is drawn from the parts on screen, once they are
        // built from its current features.
        let big = matches!(el.kind, ElementKind::PartStudio { .. }) && el.features().len() > THUMB_FEATURE_LIMIT;
        if big {
            let cache = world.resource::<crate::parts::PartCache>();
            let on_screen = cache.settled().is_some_and(|(id, features, over)| id == el.id && features == el.features() && *over == crate::parts::PartOverride::default());
            if !on_screen {
                continue;
            }
        }
        budget -= 1;
        let img = world.resource_scope(|world, mut parts: Mut<crate::assembly::AssemblyParts>| -> Option<image::RgbaImage> {
            match &el.kind {
                ElementKind::PartStudio { .. } if big => {
                    let cache = world.resource::<crate::parts::PartCache>();
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> =
                        cache.parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, el.part_props()).rgb)).collect();
                    (!list.is_empty()).then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                ElementKind::PartStudio { .. } if el.features().len() <= THUMB_FEATURE_LIMIT => {
                    let build = parts.build(&doc, el.id)?;
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> =
                        build.parts.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, el.part_props()).rgb)).collect();
                    (!list.is_empty()).then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                ElementKind::Assembly => {
                    let asm = el.assembly_model()?;
                    let (ps, props) = cadrs_core::assembly::instance_parts(&doc, asm, |e| parts.build(&doc, e));
                    let list: Vec<(&cadrs_core::Solid, [u8; 3])> = ps.iter().map(|p| (&*p.solid, cadrs_core::appearance::part_appearance(p, &props).rgb)).collect();
                    (!list.is_empty()).then(|| cadrs_core::assembly::thumb::render(&list, THUMB_SIZE))
                }
                _ => None,
            }
        });
        let handle = img.map(|img| {
            let (w, h) = img.dimensions();
            world.resource_mut::<Assets<Image>>().add(Image::new(
                Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                TextureDimension::D2,
                img.into_raw(),
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::RENDER_WORLD,
            ))
        });
        let mut th = world.resource_mut::<TabThumbs>();
        th.map.insert(el.id, (el.clone(), handle));
        th.generation += 1;
    }
}

fn thumb_of(world: &World, id: ElementId) -> Option<Handle<Image>> {
    world.resource::<TabThumbs>().map.get(&id).and_then(|(_, h)| h.clone())
}

// ---------------------------------------------------------------------------------------------
// The panel

/// Shows or hides the dock and spawns the panel's frame once (header, search, filters, the
/// list's frame, footer and preview).
fn sync_panel(world: &mut World) {
    let open = world.resource::<TabManager>().open;
    let mut q_dock = world.query_filtered::<(Entity, &mut Node), With<TabManagerDock>>();
    let Some((dock, mut node)) = q_dock.iter_mut(world).next() else { return };
    let want = if open { Display::Flex } else { Display::None };
    if node.display != want {
        node.display = want;
    }
    let mut q = world.query_filtered::<Entity, With<TabManagerPanel>>();
    let existing: Vec<Entity> = q.iter(world).collect();
    if !open {
        for e in existing {
            world.entity_mut(e).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }
    let tm = world.resource::<TabManager>().clone();
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    let panel = world
        .spawn((
            Name::new("tab-manager-panel"),
            TabManagerPanel,
            ChildOf(dock),
            Node { flex_grow: 1.0, min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() },
            Pickable::default(),
        ))
        .id();
    world.entity_mut(panel).with_children(|p| {
        p.spawn(Node { align_items: AlignItems::Center, height: Val::Px(36.0), flex_shrink: 0.0, padding: UiRect::new(Val::Px(10.0), Val::Px(6.0), Val::ZERO, Val::ZERO), column_gap: Val::Px(4.0), ..default() }).with_children(|h| {
            h.spawn((t.text("Tabs", t.font_md, FontWeight::BOLD, t.foreground), Node { flex_grow: 1.0, ..default() }));
            h.spawn(IconButton::new("tab-manager-sort", "sort-descending").icon_size(16.0).tooltip("Sort").build(t));
            h.spawn(IconButton::new("tab-manager-close", "close").icon_size(14.0).tooltip("Close").build(t));
        });
        p.spawn(Node { padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::ZERO, Val::Px(4.0)), flex_shrink: 0.0, ..default() }).with_children(|r| {
            r.spawn(TextInput::new("tab-manager-search").placeholder("Search tabs").value(tm.search.clone()).height(28.0).cleanable().width(Val::Percent(100.0)).build(t));
        });
        p.spawn((FilterBox, Node { align_items: AlignItems::Center, flex_shrink: 0.0, height: Val::Px(32.0), padding: UiRect::horizontal(Val::Px(8.0)), column_gap: Val::Px(2.0), ..default() }));
        p.spawn(Node { align_items: AlignItems::Center, flex_shrink: 0.0, padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::Px(2.0), Val::Px(4.0)), column_gap: Val::Px(6.0), border: UiRect::bottom(Val::Px(1.0)), ..default() })
            .insert(BorderColor::all(t.border))
            .with_children(|r| {
                r.spawn((CountText, Name::new("tab-manager-count"), t.text(String::new(), 10.5, FontWeight::NORMAL, t.muted_foreground), Node { flex_grow: 1.0, ..default() }));
                r.spawn(cadrs_ui::Button::new("tab-manager-new-folder").label("New folder").icon("folder-new").small().ghost().build(t)).insert(Tooltip::new("Put the selected tabs in a new folder"));
            });
        // The list takes the height left, whatever the filter shows (a stable panel).
        p.spawn(Node { flex_grow: 1.0, flex_basis: Val::Px(0.0), min_height: Val::Px(0.0), flex_direction: FlexDirection::Column, ..default() }).with_children(|frame| {
            let list = frame
                .spawn((
                    Name::new("tab-manager-rows"),
                    RowsBox,
                    bevy::ui_widgets::ScrollArea,
                    cadrs_ui::scrollbar::ScrollGutter(6.0),
                    Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_height: Val::Px(0.0), overflow: Overflow::scroll_y(), ..default() },
                ))
                .id();
            frame.spawn(cadrs_ui::vertical_scrollbar(t, "tab-manager-scrollbar", list));
        });
        p.spawn(Node { flex_shrink: 0.0, padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)), border: UiRect::top(Val::Px(1.0)), ..default() })
            .insert(BorderColor::all(t.border))
            .with_children(|f| {
                f.spawn(cadrs_ui::Button::new("tab-manager-move").label("Move to document…").small().disabled(true).build(t));
            });
        p.spawn((
            Name::new("tab-manager-preview"),
            PreviewBox,
            Node { flex_shrink: 0.0, height: Val::Px(230.0), flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(10.0)), border: UiRect::top(Val::Px(1.0)), ..default() },
            BorderColor::all(t.border),
            BackgroundColor(t.sidebar),
        ));
    });
    world.flush();
}

type FilterKey = (TypeFilter, bool);

/// The type toggles and Clear.
fn sync_filters(world: &mut World, mut last: Local<Option<FilterKey>>) {
    let tm = world.resource::<TabManager>().clone();
    let mut q = world.query_filtered::<(Entity, Ref<FilterBox>), ()>();
    let Some((bx, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else {
        *last = None;
        return;
    };
    let key: FilterKey = (tm.filter, tm.flat());
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let theme = world.resource::<Theme>().clone();
    world.entity_mut(bx).despawn_children();
    world.entity_mut(bx).with_children(|r| {
        for (name, icon_name, tip, f) in [
            ("tab-manager-filter-part-studios", "part-studio", "Part Studios", TypeFilter::PartStudios),
            ("tab-manager-filter-assemblies", "assembly", "Assemblies", TypeFilter::Assemblies),
            ("tab-manager-filter-drawings", "details", "Drawings", TypeFilter::Drawings),
        ] {
            r.spawn(IconButton::new(name, icon_name).icon_size(18.0).tooltip(format!("Show only {tip}")).selected(tm.filter == f).build(&theme));
        }
        r.spawn(Node { flex_grow: 1.0, ..default() });
        r.spawn(cadrs_ui::Button::new("tab-manager-clear").label("Clear").small().disabled(!tm.flat()).build(&theme));
    });
    world.flush();
}

type RowsKey = (Vec<RowSpec>, Vec<ElementId>, Option<ElementId>, usize, u64);

/// Rebuilds the rows (and the count and the Move button) when they change.
fn sync_rows(world: &mut World, mut last: Local<Option<RowsKey>>) {
    let tm = world.resource::<TabManager>().clone();
    if !tm.open {
        *last = None;
        return;
    }
    // Not while rows are dragged (the drag's entity must stay).
    if !world.resource::<ManagerDrag>().items.is_empty() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    // Rows gone (moved, deleted) leave the selection.
    let known = |e: &ElementId| doc.doc.element(*e).is_some() || doc.doc.tab_tree.folder(*e).is_some();
    if tm.selected.iter().any(|e| !known(e)) {
        let keep: Vec<ElementId> = tm.selected.iter().copied().filter(known).collect();
        world.resource_mut::<TabManager>().selected = keep;
        return;
    }
    let rows = rows_of(&doc.doc, &tm);
    let active = doc.active;
    let n_tabs = doc.doc.elements.len();
    let move_count = selected_tabs(&doc.doc, &tm.selected).len();
    let generation = world.resource::<TabThumbs>().generation;
    let mut q_box = world.query_filtered::<(Entity, Ref<RowsBox>), ()>();
    let Some((rows_box, added)) = q_box.iter(world).next().map(|(e, r)| (e, r.is_added())) else { return };
    let key: RowsKey = (rows.clone(), tm.selected.clone(), active, n_tabs, generation);
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    world.entity_mut(rows_box).despawn_children();
    let shown_tabs = rows.iter().filter(|r| r.folder.is_none()).count();
    let count = if tm.flat() && tm.sort == SortBy::Document || (!tm.search.trim().is_empty() || tm.filter != TypeFilter::All) {
        format!("{shown_tabs} of {n_tabs} tabs")
    } else if n_tabs == 1 {
        "1 tab".into()
    } else {
        format!("{n_tabs} tabs")
    };
    let mut q_count = world.query_filtered::<&mut Text, With<CountText>>();
    for mut c in q_count.iter_mut(world) {
        c.0 = count.clone();
    }
    let thumbs: HashMap<ElementId, Handle<Image>> = rows.iter().filter_map(|r| Some((r.item.id(), thumb_of(world, r.item.id())?))).collect();
    let active_fill = Color::srgb_u8(0xb3, 0xdc, 0xf2);
    let active_bar = Color::srgb_u8(0x1f, 0x5f, 0xa8);
    world.entity_mut(rows_box).with_children(|list| {
        if rows.is_empty() {
            list.spawn((Name::new("tab-manager-empty"), t.text("No tabs match.", t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::new(Val::Px(12.0), Val::Px(8.0), Val::Px(8.0), Val::Px(8.0)), ..default() }));
        }
        let mut k = 0;
        for r in &rows {
            let selected = tm.selected.contains(&r.item.id());
            let is_active = r.folder.is_none() && active == Some(r.item.id());
            let name = match r.folder {
                Some(_) => crate::document::tab_node_name(&r.name).replacen("tab-", "tab-manager-folder-", 1),
                None => {
                    k += 1;
                    format!("tab-manager-row-{k}")
                }
            };
            let normal = if is_active { active_fill } else { Color::NONE };
            let mut row = list.spawn((
                Name::new(name),
                ManagerRow { item: r.item, depth: r.depth, parent: r.parent },
                ContextMenuTarget,
                Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    height: Val::Px(54.0),
                    flex_shrink: 0.0,
                    padding: UiRect::new(Val::Px(10.0 + 16.0 * r.depth as f32), Val::Px(10.0), Val::ZERO, Val::ZERO),
                    ..default()
                },
                BackgroundColor(normal),
                Pickable::default(),
                bevy::picking::hover::Hovered::default(),
                cadrs_ui::style::Visuals {
                    background: cadrs_ui::style::StateColors::new(normal, if is_active { active_fill } else { t.menu_hover }, t.list_active, normal).with_selected(if is_active { active_fill } else { t.list_selected }),
                    border: cadrs_ui::style::StateColors::all(Color::NONE),
                    foreground: cadrs_ui::style::StateColors::all(t.foreground),
                    focus_ring: Color::NONE,
                },
            ));
            if selected {
                row.insert(cadrs_ui::style::Selected);
            }
            let thumb = thumbs.get(&r.item.id()).cloned();
            row.with_children(|c| {
                if is_active {
                    c.spawn((Node { position_type: PositionType::Absolute, left: Val::Px(0.0), top: Val::Px(0.0), bottom: Val::Px(0.0), width: Val::Px(5.0), ..default() }, BackgroundColor(active_bar), Pickable::IGNORE));
                }
                // The chevron's column (every row has it, so the indent is the same filtered).
                let chevron = Node { width: Val::Px(12.0), flex_shrink: 0.0, ..default() };
                match r.folder {
                    Some((open, _)) => {
                        c.spawn((chevron, Pickable::IGNORE)).with_child((cadrs_ui::icon::icon(if open { "chevron-down" } else { "chevron-right" }, 12.0, t.tool_foreground), Pickable::IGNORE));
                    }
                    None => {
                        c.spawn((chevron, Pickable::IGNORE));
                    }
                }
                let bx = Node { width: Val::Px(48.0), height: Val::Px(42.0), flex_shrink: 0.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() };
                c.spawn((bx, Pickable::IGNORE)).with_children(|b| match thumb {
                    Some(h) => {
                        b.spawn((ImageNode::new(h), Node { width: Val::Px(42.0), height: Val::Px(42.0), ..default() }, Pickable::IGNORE));
                    }
                    None => {
                        b.spawn((cadrs_ui::icon::icon(r.icon, 28.0, t.tool_foreground), Pickable::IGNORE));
                    }
                });
                let mut name_col = c.spawn((RowName(r.item), Node { flex_direction: FlexDirection::Column, flex_grow: 1.0, min_width: Val::Px(0.0), row_gap: Val::Px(2.0), ..default() }, Pickable::IGNORE));
                if r.folder.is_some() {
                    name_col.insert(cadrs_ui::InlineEdit::default());
                }
                name_col.with_children(|col| {
                    let mut label = col.spawn((t.text(r.name.clone(), t.font_base, if is_active { FontWeight::BOLD } else { FontWeight::NORMAL }, t.foreground), Pickable::IGNORE));
                    if r.folder.is_some() {
                        label.insert(cadrs_ui::InlineEditLabel);
                    }
                    let mut font = t.font(t.font_sm, FontWeight::NORMAL);
                    font.style = bevy::text::FontStyle::Italic;
                    let second = match &r.location {
                        Some(loc) => format!("{} · in {loc}", r.kind),
                        None => r.kind.clone(),
                    };
                    col.spawn((Text::new(second), font, TextColor(t.muted_foreground), Pickable::IGNORE));
                });
            });
            row.observe(on_row_click);
        }
    });
    // The Move button's label and state.
    let label = if move_count > 1 { format!("Move {move_count} tabs to document…") } else { "Move to document…".to_string() };
    let mut q_btn = world.query::<(Entity, &Name)>();
    let btn = q_btn.iter(world).find(|(_, n)| n.as_str() == "tab-manager-move").map(|(e, _)| e);
    if let Some(b) = btn {
        let mut q_kids = world.query::<(&Children,)>();
        let texts: Vec<Entity> = q_kids.get(world, b).map(|(c,)| c.iter().collect()).unwrap_or_default();
        for e in texts {
            if let Some(mut tx) = world.get_mut::<Text>(e) {
                tx.0 = label.clone();
            }
        }
        if move_count == 0 {
            world.entity_mut(b).insert(bevy::ui::InteractionDisabled);
        } else {
            world.entity_mut(b).remove::<bevy::ui::InteractionDisabled>();
        }
    }
    world.flush();
}

/// The large preview of the selected tab (else the active one).
fn sync_preview(world: &mut World, mut last: Local<Option<(Option<ElementId>, String, u64)>>) {
    let mut q = world.query_filtered::<(Entity, Ref<PreviewBox>), ()>();
    let Some((bx, added)) = q.iter(world).next().map(|(e, r)| (e, r.is_added())) else {
        *last = None;
        return;
    };
    let tm = world.resource::<TabManager>().clone();
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let pick = tm.selected.iter().rev().copied().find(|e| doc.doc.elements.iter().any(|x| x.id == *e)).or(doc.active);
    let el = pick.and_then(|e| doc.doc.element(e)).cloned();
    let generation = world.resource::<TabThumbs>().generation;
    let key = (pick, el.as_ref().map(|e| e.name.clone()).unwrap_or_default(), generation);
    if !added && last.as_ref() == Some(&key) {
        return;
    }
    *last = Some(key);
    let thumb = pick.and_then(|e| thumb_of(world, e));
    let theme = world.resource::<Theme>().clone();
    let t = &theme;
    world.entity_mut(bx).despawn_children();
    world.entity_mut(bx).with_children(|p| {
        let Some(el) = el else { return };
        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|h| {
            h.spawn((cadrs_ui::icon::icon(tab_icon(&el), 18.0, t.tool_foreground), Pickable::IGNORE));
            h.spawn((Name::new("tab-manager-preview-name"), t.text(el.name.clone(), t.font_md, FontWeight::SEMIBOLD, t.foreground)));
        });
        p.spawn(Node { flex_grow: 1.0, justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..default() }).with_children(|b| match thumb {
            Some(h) => {
                b.spawn((ImageNode::new(h), Node { width: Val::Px(170.0), height: Val::Px(170.0), ..default() }, Pickable::IGNORE));
            }
            None => {
                b.spawn((cadrs_ui::icon::icon(tab_icon(&el), 72.0, t.subtle_foreground), Pickable::IGNORE));
            }
        });
    });
    world.flush();
}

// ---------------------------------------------------------------------------------------------
// Renaming a folder in its row

/// A row's name column (a folder's is renamed in place there).
#[derive(Component, Debug, Clone, Copy)]
struct RowName(TabItem);

/// A folder made here: renamed in place in its row as soon as the row is there.
#[derive(Resource, Debug, Default)]
struct PendingRowRename(Option<ElementId>);

/// Starts renaming `folder` in its Tab manager row (the Rename… item, and a new folder).
pub fn rename_in_row(world: &mut World, folder: ElementId) {
    world.resource_mut::<PendingRowRename>().0 = Some(folder);
}

fn start_row_rename(mut pending: ResMut<PendingRowRename>, q: Query<(Entity, &RowName)>, doc: Option<Res<ActiveDocument>>, theme: Res<Theme>, mut commands: Commands) {
    let Some(folder) = pending.0 else { return };
    let Some((e, _)) = q.iter().find(|(_, r)| r.0 == TabItem::Folder(folder)) else { return };
    pending.0 = None;
    let name = doc.and_then(|d| d.doc.tab_tree.folder(folder).map(|f| f.name.clone())).unwrap_or_default();
    let mut opts = InlineEditOptions::new("tab-manager-folder-rename-field");
    opts.width = Val::Px(160.0);
    opts.height = 22.0;
    begin_inline_edit(&mut commands, &theme, e, name, opts);
}

fn on_row_rename_commit(ev: On<InlineEditCommit>, q: Query<&RowName>, doc: Option<ResMut<ActiveDocument>>) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let (Ok(row), Some(mut doc)) = (q.get(ev.entity), doc) else { return };
    let TabItem::Folder(f) = row.0 else { return };
    let name = ev.value.trim().to_string();
    if name.is_empty() || doc.doc.tab_tree.folder(f).is_some_and(|x| x.name == name) {
        return;
    }
    let _ = doc.execute(&RenameTabFolder { id: f, name });
}

fn on_row_click(mut click: On<Pointer<Click>>, q: Query<&ManagerRow>, keys: Res<ButtonInput<KeyCode>>, drag: Res<ManagerDrag>, mut tm: ResMut<TabManager>, doc: Option<ResMut<ActiveDocument>>) {
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    if drag.moved {
        return;
    }
    let (Ok(row), Some(mut doc)) = (q.get(click.entity).copied(), doc) else { return };
    let id = row.item.id();
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight, KeyCode::SuperLeft, KeyCode::SuperRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    // The rows in list order (a range runs over what is shown).
    let order: Vec<ElementId> = rows_of(&doc.doc, &tm).iter().map(|r| r.item.id()).collect();
    if shift && let Some(a) = tm.anchor {
        if let Some(range) = range_selection(&order, a, id) {
            tm.selected = range;
        }
    } else if ctrl {
        if let Some(i) = tm.selected.iter().position(|e| *e == id) {
            tm.selected.remove(i);
        } else {
            tm.selected.push(id);
        }
        tm.anchor = Some(id);
    } else {
        tm.selected = vec![id];
        tm.anchor = Some(id);
        match row.item {
            TabItem::Tab(e) => doc.set_active(e),
            TabItem::Folder(f) => {
                if let Some(i) = tm.collapsed.iter().position(|x| *x == f) {
                    tm.collapsed.remove(i);
                } else {
                    tm.collapsed.push(f);
                }
            }
        }
    }
}

/// The rows a Shift+click selects: every row of `order` (the list as shown, scrolled out of view
/// or not) from the anchor to the clicked row, in list order. `None` if either isn't listed.
pub fn range_selection(order: &[ElementId], anchor: ElementId, clicked: ElementId) -> Option<Vec<ElementId>> {
    let i = order.iter().position(|e| *e == anchor)?;
    let j = order.iter().position(|e| *e == clicked)?;
    let (lo, hi) = (i.min(j), i.max(j));
    Some(order[lo..=hi].to_vec())
}

// ---------------------------------------------------------------------------------------------
// The row menu

#[derive(Component)]
struct RowMenu;

fn on_row_menu(ev: On<ContextMenuRequested>, q: Query<&ManagerRow>, mut tm: ResMut<TabManager>, doc: Option<Res<ActiveDocument>>, theme: Res<Theme>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity).copied() else { return };
    let Some(doc) = doc else { return };
    let id = row.item.id();
    if !tm.selected.contains(&id) {
        tm.selected = vec![id];
        tm.anchor = Some(id);
    }
    let n = selected_tabs(&doc.doc, &tm.selected).len();
    let items = selected_items(&doc.doc, &tm.selected);
    let layout = tab_tree::layout(&doc.doc);
    let any_in_folder = items.iter().any(|i| tab_tree::parent_of(&layout, *i).is_some());
    let mut menu = Menu::new("tab-manager-menu")
        .min_width(210.0)
        .item_height(22.0)
        .item(MenuItem::new("tab-manager-menu-move", if n > 1 { format!("Move {n} tabs to document…") } else { "Move to document…".to_string() }).disabled(n == 0))
        .item(MenuItem::new("tab-manager-menu-folder", "New folder from selection").icon("folder-new"));
    if any_in_folder {
        menu = menu.item(MenuItem::new("tab-manager-menu-top", "Move to top level").icon("home"));
    }
    if let [TabItem::Folder(_)] = items.as_slice() {
        menu = menu
            .separator()
            .item(MenuItem::new("tab-manager-menu-open", "Open in tab bar").icon("folder"))
            .item(MenuItem::new("tab-manager-menu-rename", "Rename…").icon("edit"))
            .item(MenuItem::new("tab-manager-menu-delete", "Delete folder…").icon("remove-circle"));
    }
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands.entity(anchor).insert((RowMenu, DespawnOnExit(AppState::Document)));
}

fn on_row_menu_action(ev: On<MenuAction>, q: Query<(), (With<RowMenu>, With<ContextMenuAnchor>)>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let item = ev.item.clone();
    commands.queue(move |w: &mut World| {
        let sel = w.resource::<TabManager>().selected.clone();
        let folder = sel.first().copied();
        match item.as_str() {
            "tab-manager-menu-move" => move_to_document(w, sel),
            "tab-manager-menu-folder" => new_folder_from(w, sel),
            "tab-manager-menu-top" => {
                let Some(mut doc) = w.get_resource_mut::<ActiveDocument>() else { return };
                let items = selected_items(&doc.doc, &sel);
                let _ = doc.execute(&MoveTabItems { items, parent: None, before: None, label: "Move to top level".into() });
            }
            "tab-manager-menu-open" => {
                if let Some(f) = folder {
                    crate::tab_folders::open_folder(w, Some(f));
                }
            }
            "tab-manager-menu-rename" => {
                if let Some(f) = folder {
                    rename_in_row(w, f);
                }
            }
            "tab-manager-menu-delete" => {
                if let Some(f) = folder {
                    crate::tab_folders::ask_delete_folder(w, f);
                }
            }
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Dragging rows

#[derive(Resource, Debug, Default)]
struct ManagerDrag {
    items: Vec<TabItem>,
    start: Vec2,
    pointer: Vec2,
    moved: bool,
    target: Option<(Option<ElementId>, Option<TabItem>)>,
}

#[derive(Component)]
struct DropLine;

#[derive(Component)]
struct DragGhost;

fn on_drag_start(ev: On<Pointer<DragStart>>, q: Query<&ManagerRow>, tm: Res<TabManager>, doc: Option<Res<ActiveDocument>>, mut drag: ResMut<ManagerDrag>) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let (Ok(row), Some(doc)) = (q.get(ev.entity), doc) else { return };
    // Reordering works on the tree, not a filtered list.
    if !tm.search.trim().is_empty() || tm.filter != TypeFilter::All {
        return;
    }
    let items = if tm.selected.contains(&row.item.id()) { selected_items(&doc.doc, &tm.selected) } else { vec![row.item] };
    *drag = ManagerDrag { items, start: ev.pointer_location.position, pointer: ev.pointer_location.position, moved: false, target: None };
}

fn on_drag(ev: On<Pointer<Drag>>, q: Query<(), With<ManagerRow>>, mut drag: ResMut<ManagerDrag>) {
    if q.contains(ev.entity) && !drag.items.is_empty() {
        drag.pointer = ev.pointer_location.position;
        if drag.pointer.distance(drag.start) > 5.0 {
            drag.moved = true;
        }
    }
}

fn on_drag_end(ev: On<Pointer<DragEnd>>, q: Query<(), With<ManagerRow>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let drag = std::mem::take(&mut *world.resource_mut::<ManagerDrag>());
        world.resource_mut::<ManagerDrag>().moved = drag.moved;
        let (true, Some((parent, before))) = (drag.moved, drag.target) else { return };
        let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
        let layout = tab_tree::layout(&doc.doc);
        // Nothing moves: one item dropped just before or after itself in its own level.
        if let [one] = drag.items.as_slice()
            && tab_tree::parent_of(&layout, *one) == parent
            && let Some(level) = tab_tree::level_of(&layout, parent)
        {
            let k = level.iter().position(|n| n.item() == *one);
            let next = k.and_then(|k| level.get(k + 1)).map(|n| n.item());
            if before == Some(*one) || next == before {
                return;
            }
        }
        let name = match drag.items.as_slice() {
            [TabItem::Tab(e)] => doc.doc.element(*e).map(|e| e.name.clone()).unwrap_or_default(),
            [TabItem::Folder(f)] => doc.doc.tab_tree.folder(*f).map(|f| f.name.clone()).unwrap_or_default(),
            many => format!("{} tabs", many.len()),
        };
        let label = match (parent, before) {
            (Some(f), None) => format!("Move {name} into {}", doc.doc.tab_tree.folder(f).map(|f| f.name.as_str()).unwrap_or("folder")),
            _ => format!("Reorder {name}"),
        };
        let _ = doc.execute(&MoveTabItems { items: drag.items, parent, before, label });
    });
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_drop(
    mut drag: ResMut<ManagerDrag>,
    q_rows: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &ManagerRow)>,
    mut q_line: Query<(Entity, &mut Node, &mut BackgroundColor, &mut BorderColor), (With<DropLine>, Without<DragGhost>)>,
    mut q_ghost: Query<(Entity, &mut Node), (With<DragGhost>, Without<DropLine>)>,
    q_dim: Query<(Entity, &ChildOf), With<crate::tab_folders::DragDim>>,
    q_row_entities: Query<(Entity, &ManagerRow)>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if drag.items.is_empty() || !drag.moved {
        if drag.items.is_empty() && drag.moved {
            drag.moved = false;
        }
        if drag.items.is_empty() {
            for (e, _) in &q_dim {
                commands.entity(e).try_despawn();
            }
        }
        for (e, ..) in &q_line {
            commands.entity(e).try_despawn();
        }
        for (e, _) in &q_ghost {
            commands.entity(e).try_despawn();
        }
        return;
    }
    // The dragged rows dim.
    for (e, r) in &q_row_entities {
        if drag.items.contains(&r.item) && !q_dim.iter().any(|(_, p)| p.parent() == e) {
            crate::tab_folders::dim(&mut commands, e);
        }
    }
    let mut rows: Vec<(f32, f32, f32, f32, ManagerRow)> = q_rows
        .iter()
        .map(|(n, t, r)| {
            let s = n.inverse_scale_factor();
            let (c, size) = (t.translation * s, n.size() * s);
            (c.y - size.y / 2.0, c.y + size.y / 2.0, c.x - size.x / 2.0, size.x, *r)
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    let Some(first) = rows.first().copied() else { return };
    let p = drag.pointer;
    let inside = p.x > first.2 - 30.0 && p.x < first.2 + first.3 + 30.0 && p.y > first.0 - 30.0 && p.y < rows.last().map_or(0.0, |r| r.1) + 30.0;
    let blue = Color::srgb_u8(0x2b, 0x64, 0xc0);
    drag.target = None;
    let mut mark: Option<(f32, f32, f32, f32, bool)> = None;
    if inside {
        // Onto a folder row's middle: into it.
        if let Some((top, bottom, left, width, r)) = rows.iter().find(|(top, bottom, ..)| p.y > top + 6.0 && p.y < bottom - 6.0).filter(|r| matches!(r.4.item, TabItem::Folder(_)))
            && !drag.items.contains(&r.item)
        {
            drag.target = Some((Some(r.item.id()), None));
            mark = Some((*top, *left, *width, bottom - top, true));
        } else {
            let k = rows.iter().position(|(top, bottom, ..)| p.y < (top + bottom) / 2.0);
            let (parent, before, y, depth) = match k {
                Some(k) => (rows[k].4.parent, Some(rows[k].4.item), rows[k].0, rows[k].4.depth),
                None => (None, None, rows[rows.len() - 1].1, 0),
            };
            // Not into a dragged folder.
            let bad = parent.is_some_and(|f| drag.items.contains(&TabItem::Folder(f)));
            if !bad {
                drag.target = Some((parent, before));
                let indent = 10.0 + 16.0 * depth as f32;
                mark = Some((y - 1.5, first.2 + indent, first.3 - indent - 8.0, 3.0, false));
            }
        }
    }
    match mark {
        None => {
            for (e, ..) in &q_line {
                commands.entity(e).try_despawn();
            }
        }
        Some((top, left, width, height, boxed)) => {
            // A line's border is blue too: a 2 px node's fill sits inside its 1 px borders.
            let (bg, border) = if boxed { (blue.with_alpha(0.18), blue) } else { (blue, blue) };
            let (tv, lv, wv, hv) = (Val::Px(top), Val::Px(left), Val::Px(width), Val::Px(height));
            match q_line.iter_mut().next() {
                Some((_, mut n, mut b, mut bc)) => {
                    if n.top != tv || n.left != lv || n.width != wv || n.height != hv {
                        n.top = tv;
                        n.left = lv;
                        n.width = wv;
                        n.height = hv;
                    }
                    b.set_if_neq(BackgroundColor(bg));
                    bc.set_if_neq(BorderColor::all(border));
                }
                None => {
                    commands.spawn((
                        Name::new("tab-manager-drop-line"),
                        DropLine,
                        Node { position_type: PositionType::Absolute, top: tv, left: lv, width: wv, height: hv, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(3.0)), ..default() },
                        BackgroundColor(bg),
                        BorderColor::all(border),
                        GlobalZIndex(40),
                        Pickable::IGNORE,
                        DespawnOnExit(AppState::Document),
                    ));
                }
            }
        }
    }
    let (gx, gy) = (Val::Px(p.x + 16.0), Val::Px(p.y + 10.0));
    match q_ghost.iter_mut().next() {
        Some((_, mut n)) => {
            if n.left != gx || n.top != gy {
                n.left = gx;
                n.top = gy;
            }
        }
        None => {
            let Some(doc) = doc else { return };
            let (label, icon) = match drag.items.as_slice() {
                [TabItem::Tab(e)] => {
                    let el = doc.doc.element(*e);
                    (el.map(|e| e.name.clone()).unwrap_or_default(), el.map(tab_icon).unwrap_or("part-studio"))
                }
                [TabItem::Folder(f)] => (doc.doc.tab_tree.folder(*f).map(|f| f.name.clone()).unwrap_or_default(), "folder"),
                many => (format!("{} items", many.len()), "tab-manager"),
            };
            let t = &*theme;
            commands.spawn((
                Name::new("tab-manager-drag-ghost"),
                DragGhost,
                Node {
                    position_type: PositionType::Absolute,
                    left: gx,
                    top: gy,
                    height: Val::Px(22.0),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
                BorderColor::all(t.border),
                GlobalZIndex(41),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
                children![
                    (cadrs_ui::icon::icon(icon, 14.0, t.tool_foreground), Pickable::IGNORE),
                    (t.text(label, t.font_sm, FontWeight::MEDIUM, t.foreground.with_alpha(0.85)), Pickable::IGNORE),
                ],
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::tab_tree::CreateTabFolder;
    use cadrs_core::{Command, Document, Element};

    fn doc() -> Document {
        let mut d = Document::empty("T");
        d.elements.push(Element::part_studio("Base plate"));
        d.elements.push(Element::assembly("Hexapod"));
        d.elements.push(Element::part_studio("Top plate"));
        d.elements.push(Element::part_studio("Piston"));
        d
    }

    /// P3E.3a (P3E.2 carried delta): a Shift+click range covers every row between the anchor
    /// and the clicked row, whether the rows are in view or not: rows 1 and 12 of 60 tabs give
    /// the 12 tabs in list order, either way round.
    #[test]
    fn a_shift_click_range_covers_rows_out_of_view() {
        let mut d = Document::empty("T");
        for i in 0..60 {
            d.elements.push(Element::part_studio(format!("Studio {}", i + 2)));
        }
        let tm = TabManager::default();
        let order: Vec<ElementId> = rows_of(&d, &tm).iter().map(|r| r.item.id()).collect();
        assert_eq!(order.len(), 60);
        let range = range_selection(&order, order[0], order[11]).unwrap();
        assert_eq!(range, order[..12].to_vec());
        assert_eq!(range_selection(&order, order[11], order[0]).unwrap(), order[..12].to_vec());
        assert_eq!(range_selection(&order, order[5], order[5]).unwrap(), vec![order[5]]);
        assert!(range_selection(&order, ElementId::new(), order[3]).is_none());
    }

    #[test]
    fn search_and_type_filters() {
        let d = doc();
        let mut tm = TabManager { search: "PLATE".into(), ..default() };
        let names: Vec<String> = rows_of(&d, &tm).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Base plate", "Top plate"]);
        tm.search.clear();
        tm.filter = TypeFilter::Assemblies;
        let names: Vec<String> = rows_of(&d, &tm).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["Hexapod"]);
        tm.filter = TypeFilter::Drawings;
        assert!(rows_of(&d, &tm).is_empty());
    }

    #[test]
    fn folders_are_expandable_rows() {
        let mut d = doc();
        let ids: Vec<ElementId> = d.elements.iter().map(|e| e.id).collect();
        let f = ElementId::new();
        CreateTabFolder { id: f, name: Some("Plates".into()), parent: None, before: None, items: vec![TabItem::Tab(ids[0]), TabItem::Tab(ids[2])] }.apply(&mut d).unwrap();
        let mut tm = TabManager::default();
        let rows = rows_of(&d, &tm);
        let names: Vec<(&str, usize)> = rows.iter().map(|r| (r.name.as_str(), r.depth)).collect();
        assert_eq!(names, [("Plates", 0), ("Base plate", 1), ("Top plate", 1), ("Hexapod", 0), ("Piston", 0)]);
        assert_eq!(rows[0].folder, Some((true, 2)));
        tm.collapsed.push(f);
        assert_eq!(rows_of(&d, &tm).len(), 3);
        // A selected folder moves (and moves to another document) with its tabs.
        assert_eq!(selected_tabs(&d, &[f]), vec![ids[0], ids[2]]);
        assert_eq!(selected_items(&d, &[f, ids[0], ids[3]]), vec![TabItem::Folder(f), TabItem::Tab(ids[3])]);
    }
}

//! P3E.2: tab folders in the tab bar (`test-drive.md` TD5.3, X6; the model is
//! [`cadrs_core::tab_tree`]).
//!
//! - **"+" → Create folder** makes "Folder N" right of the active tab (or at the end), and starts
//!   renaming it in place (Enter commits; one undo step with the create).
//! - The bar always shows the top level. A **folder** is a tab with a folder icon and a chevron:
//!   ▲, or ▼ while its list is open. A click opens its **list** above it: its tabs (a click opens
//!   one, the active one ticked) and its folders, each opening its own list to the side, as
//!   deep as they go. A folder inside another has no tab of its own: its list ends with
//!   Rename… (a name popup) and Delete folder…. The folder tab holding the active tab is marked.
//! - A folder's right-click menu: Show tabs, Rename…, Delete folder… (the dialog asks about its
//!   tabs: **Delete folder only** moves them up a level, **Delete folder and tabs** deletes them
//!   too).
//! - **Dragging** a tab or a folder along the bar shows a blue line where it will land; onto a
//!   folder tab (a blue box), it goes into that folder. One undo step
//!   ([`cadrs_core::tab_tree::MoveTabItems`]).
//! - A tab's menu has **Move to folder ▸** (the folders, "Top level", "New folder").
//! - **▾** (right of the tab strip, while the tabs overflow) lists the tabs scrolled out of
//!   sight; picking one opens it (T1.2).
//!
//! Names: `tab-folder-<name>` (a folder tab), `tab-folder-list` (`tab-folder-pick-<tab id>`,
//! `tab-folder-sub-<folder id>`, `tab-folder-rename-<folder id>`, `tab-folder-delete-<folder
//! id>`), `tab-folder-rename-popup`, `tab-folder-menu` (`tab-folder-open`, `tab-folder-rename`,
//! `tab-folder-delete`),
//! `delete-tab-folder-dialog` (`delete-tab-folder-keep`, `delete-tab-folder-all`,
//! `delete-tab-folder-cancel`), `tab-drop-line`, `tab-drag-ghost`, `tab-overflow`,
//! `tab-overflow-menu` (`tab-overflow-<k>`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use bevy::ui_widgets::popover::PopoverSide;
use cadrs_core::ElementId;
use cadrs_core::commands::{AddElement, NewElementKind};
use cadrs_core::tab_tree::{
    self, CreateTabFolder, DeleteTabFolder, MoveTabItems, RenameTabFolder, TabItem, TabNode,
};
use cadrs_ui::menu::{
    ContextMenuAnchor, ContextMenuRequested, MenuAction, MenuEntry, open_context_menu,
};
use cadrs_ui::prelude::*;
use cadrs_ui::style::StateColors;
use cadrs_ui::{Button, DialogClose, InlineEditCommit, InlineEditOptions, begin_inline_edit};

use crate::document::TabButton;
use crate::{ActiveDocument, AppState};

pub struct TabFoldersPlugin;

impl Plugin for TabFoldersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PendingFolderRename>()
            .init_resource::<TabDrag>()
            .add_systems(
                Update,
                (
                    start_pending_rename,
                    sync_folder_chevrons,
                    mark_listed_tabs,
                    quiet_hover_while_dragging,
                    hide_covered_viewport_tools,
                    place_tab_drop,
                    close_overflow_on_escape,
                )
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut d: ResMut<TabDrag>| {
                *d = TabDrag::default();
            })
            .add_observer(on_folder_tab_activate)
            .add_observer(on_folder_list_action)
            .add_observer(on_rename_popup_commit)
            .add_observer(on_folder_menu)
            .add_observer(on_folder_menu_action)
            .add_observer(on_folder_rename_commit)
            .add_observer(on_delete_button)
            .add_observer(on_delete_close)
            .add_observer(on_overflow)
            .add_observer(on_overflow_row)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end);
    }
}

/// A folder's tab in the tab bar.
#[derive(Component, Debug, Clone, Copy)]
pub struct FolderTab(pub ElementId);

/// What a folder's list opened from: its picks come here, and what each of its items does.
#[derive(Component, Debug, Clone, Default)]
struct FolderListAnchor(Vec<(String, ListPick)>);

/// What an item of a folder's list does.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ListPick {
    /// Opens the tab (and its right-click opens the tab's menu).
    Tab(ElementId),
    /// Makes a studio of this kind in the folder.
    New(ElementId, NewKind),
    Rename(ElementId),
    Delete(ElementId),
}

/// The studios a folder's list can make in it (`New ▸`).
#[derive(Debug, Clone, Copy, PartialEq)]
enum NewKind {
    PartStudio,
    Assembly,
    VariableStudio,
    PcbStudio,
    CamStudio,
    RenderStudio,
}

impl NewKind {
    /// In the "+" menu's order (its studios, then the rest).
    const ALL: [NewKind; 6] = [
        NewKind::CamStudio,
        NewKind::PcbStudio,
        NewKind::RenderStudio,
        NewKind::PartStudio,
        NewKind::Assembly,
        NewKind::VariableStudio,
    ];

    /// The item's label, icon and name part.
    fn look(self) -> (&'static str, &'static str, &'static str) {
        match self {
            NewKind::PartStudio => ("Part Studio", "part-studio", "part-studio"),
            NewKind::Assembly => ("Assembly", "assembly", "assembly"),
            NewKind::VariableStudio => ("Variable Studio", "variable-studio", "variable-studio"),
            NewKind::PcbStudio => ("PCB Studio", "pcb-studio", "pcb-studio"),
            NewKind::CamStudio => ("CAM Studio", "cam-studio", "cam-studio"),
            NewKind::RenderStudio => ("Render Studio", "render-studio", "render-studio"),
        }
    }
}

/// A tab in an open folder's list: its right-click opens the tab's menu
/// (`document::on_tab_context_menu`), as a tab in the bar does.
#[derive(Component, Debug, Clone, Copy)]
pub struct ListedTab(pub ElementId);

#[derive(Resource, Debug, Default)]
struct PendingFolderRename(Option<ElementId>);

#[derive(Component)]
struct FolderMenuFor(ElementId);

#[derive(Component)]
struct DeleteFolderDialog(ElementId);

/// The name popup renaming a folder inside another.
#[derive(Component)]
struct RenamePopupFor(ElementId);

/// The space between the tab bar and what opens over it (menus, lists, popups, the card).
pub const ABOVE_BAR: f32 = 4.0;

/// The tab bar's rectangle (logical px).
pub fn tab_bar_rect(world: &mut World) -> Option<Rect> {
    let mut q = world.query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>();
    q.iter(world)
        .find(|(n, ..)| n.as_str() == "tab-bar")
        .map(|(_, n, t)| {
            let s = n.inverse_scale_factor();
            Rect::from_center_size(t.translation * s, n.size() * s)
        })
}

/// A primary click opens the folder's list above it (a right-click only opens its menu; a
/// click on the tab while the list is open lands on the menus' dismiss layer and closes it).
fn on_folder_tab_activate(
    a: On<PointerClick>,
    q: Query<&FolderTab>,
    drag: Res<TabDrag>,
    mut commands: Commands,
) {
    if drag.moved || a.button != PointerButton::Primary {
        return;
    }
    if let Ok(f) = q.get(a.entity).copied() {
        let anchor = a.entity;
        commands.queue(move |world: &mut World| open_folder_list(world, anchor, f.0));
    }
}

/// Opens `folder`'s list above `anchor` (its tab, or ▾'s button).
pub fn open_folder_list(world: &mut World, anchor: Entity, folder: ElementId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let layout = tab_tree::layout(&doc.doc);
    let Some(children) = tab_tree::level_of(&layout, Some(folder)) else {
        return;
    };
    let accent = world.resource::<Theme>().primary;
    let mut list = FolderList {
        doc: &doc.doc,
        active: doc.active,
        accent,
        picks: Vec::new(),
        names: std::collections::HashSet::new(),
    };
    let entries = list.entries(children, folder, false);
    let picks = std::mem::take(&mut list.picks);
    let theme = world.resource::<Theme>().clone();
    // (Its anchor, a tab, is inset 4 px in the bar: 8 px leaves the bar's 4 clear.)
    let menu = Menu::new("tab-folder-list")
        .side(PopoverSide::Top)
        .gap(ABOVE_BAR + 4.0)
        .min_width(210.0)
        .item_height(25.0)
        .entries(entries);
    if let Ok(mut e) = world.get_entity_mut(anchor) {
        e.insert(FolderListAnchor(picks));
    }
    open_menu(&mut world.commands(), anchor, menu.build(&theme));
    world.flush();
}

/// Builds a folder's list, noting what each item does.
struct FolderList<'a> {
    doc: &'a cadrs_core::Document,
    active: Option<ElementId>,
    /// The accent colour, for the folders on the active tab's path.
    accent: Color,
    picks: Vec<(String, ListPick)>,
    names: std::collections::HashSet<String>,
}

impl FolderList<'_> {
    /// `tab-folder-<what>-<slug of label>`, unique in the list (the scenarios name items so).
    fn name(&mut self, what: &str, label: &str) -> String {
        let slug = crate::document::tab_node_name(label);
        let slug = slug.strip_prefix("tab-").unwrap_or(&slug);
        let base = format!("tab-folder-{what}-{slug}");
        let mut name = base.clone();
        let mut n = 2;
        while !self.names.insert(name.clone()) {
            name = format!("{base}-{n}");
            n += 1;
        }
        name
    }

    fn item(&mut self, what: &str, label: &str, pick: ListPick) -> MenuItem {
        let name = self.name(what, label);
        self.picks.push((name.clone(), pick));
        MenuItem::new(name, label.to_string())
    }

    /// A folder's tabs and folders in order, a folder's own list as its submenu (opening to the
    /// side); then New ▸ (a studio made in the folder); a folder inside another (`nested`) ends
    /// with Rename… and Delete folder…, as it has no tab to right-click.
    fn entries(&mut self, nodes: &[TabNode], folder: ElementId, nested: bool) -> Vec<MenuEntry> {
        let mut out: Vec<MenuEntry> = Vec::new();
        for n in nodes {
            match n {
                TabNode::Tab(e) => {
                    let Some(el) = self.doc.element(*e) else {
                        continue;
                    };
                    let checked = self.active == Some(*e);
                    let item = self
                        .item("pick", &el.name.clone(), ListPick::Tab(*e))
                        .icon(crate::tab_preview::tab_icon(el))
                        .checked(checked);
                    out.push(item.into());
                }
                TabNode::Folder { id, name, children } => {
                    let sub = self.entries(children, *id, true);
                    let item_name = self.name("sub", name);
                    // A folder holding the active tab: its icon in the accent, as its tab's is.
                    let holds_active = self.active.is_some_and(|a| n.tabs().contains(&a));
                    let mut item = MenuItem::new(item_name, name.clone())
                        .icon("folder")
                        .submenu(sub);
                    if holds_active {
                        item = item.icon_color(self.accent);
                    }
                    out.push(item.into());
                }
            }
        }
        let folder_name = self
            .doc
            .tab_tree
            .folder(folder)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        if nodes.is_empty() {
            let name = self.name("empty", &folder_name);
            out.push(MenuItem::new(name, "Empty folder").disabled(true).into());
        }
        out.push(MenuEntry::Separator);
        let mut studios: Vec<MenuEntry> = Vec::new();
        for k in NewKind::ALL {
            // The "+" menu's separator, before Part Studio.
            if k == NewKind::PartStudio {
                studios.push(MenuEntry::Separator);
            }
            let (label, icon, slug) = k.look();
            studios.push(
                self.item(
                    &format!("new-{slug}"),
                    &folder_name,
                    ListPick::New(folder, k),
                )
                .label(label)
                .icon(icon)
                .into(),
            );
        }
        let new = self.name("new", &folder_name);
        out.push(
            MenuItem::new(new, "New")
                .icon("plus")
                .submenu(studios)
                .into(),
        );
        if nested {
            out.push(MenuEntry::Separator);
            let rename = self
                .item("rename", &folder_name, ListPick::Rename(folder))
                .label("Rename…")
                .icon("edit");
            let delete = self
                .item("delete", &folder_name, ListPick::Delete(folder))
                .label("Delete folder…")
                .icon("remove-circle")
                .danger();
            out.push(rename.into());
            out.push(delete.into());
        }
        out
    }
}

fn on_folder_list_action(ev: On<MenuAction>, q: Query<&FolderListAnchor>, mut commands: Commands) {
    let Some(pick) = q.get(ev.entity).ok().and_then(|a| {
        a.0.iter()
            .find(|(name, _)| *name == ev.item)
            .map(|(_, p)| *p)
    }) else {
        return;
    };
    commands.queue(move |world: &mut World| match pick {
        ListPick::Tab(tab) => {
            if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
                && d.active != Some(tab)
            {
                d.set_active(tab);
            }
        }
        ListPick::New(folder, kind) => new_studio_in(world, folder, kind),
        ListPick::Rename(f) => rename_folder(world, f),
        ListPick::Delete(f) => ask_delete_folder(world, f),
    });
}

/// Makes a studio in `folder`, at its end, and opens it: one undo step.
fn new_studio_in(world: &mut World, folder: ElementId, kind: NewKind) {
    if crate::linked_session::refuse(world) {
        return;
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let kind = match kind {
        NewKind::PartStudio => NewElementKind::PartStudio,
        NewKind::Assembly => NewElementKind::Assembly,
        NewKind::VariableStudio => NewElementKind::VariableStudio,
        NewKind::PcbStudio => NewElementKind::PcbStudio,
        NewKind::CamStudio => NewElementKind::CamStudio,
        NewKind::RenderStudio => {
            NewElementKind::RenderStudio(crate::render_ui::default_source(&doc))
        }
    };
    let id = ElementId::new();
    let mark = doc.history.undo_len();
    let after = doc.active;
    if doc
        .execute(&AddElement {
            id,
            kind,
            name: None,
            after,
        })
        .is_err()
    {
        return;
    }
    let name = doc
        .doc
        .element(id)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let folder_name = doc
        .doc
        .tab_tree
        .folder(folder)
        .map(|f| f.name.clone())
        .unwrap_or_default();
    let _ = doc.execute(&MoveTabItems {
        items: vec![TabItem::Tab(id)],
        parent: Some(folder),
        before: None,
        label: String::new(),
    });
    doc.squash_since(mark, format!("Create {name} in {folder_name}"));
    doc.set_active(id);
}

/// The open folder list's tab items take right-clicks (the tab's menu).
#[allow(clippy::type_complexity)]
fn mark_listed_tabs(
    q_anchor: Query<&FolderListAnchor, With<cadrs_ui::menu::MenuOpenAnchor>>,
    q_items: Query<(Entity, &cadrs_ui::menu::MenuItemState), Without<ListedTab>>,
    mut commands: Commands,
) {
    let Some(anchor) = q_anchor.iter().next() else {
        return;
    };
    for (e, item) in &q_items {
        if let Some((_, ListPick::Tab(tab))) = anchor.0.iter().find(|(n, _)| *n == item.item) {
            commands
                .entity(e)
                .try_insert((ListedTab(*tab), ContextMenuTarget));
        }
    }
}

/// A folder tab whose list is open: pressed, not the active tab's fill (two tabs read as
/// selected).
#[derive(Component)]
struct ListOpen;

/// A folder tab's chevron: ▲ (its list opens upward), ▼ while the list is open; and the tab
/// pressed then.
#[allow(clippy::type_complexity)]
fn sync_folder_chevrons(
    q: Query<
        (
            Entity,
            &Children,
            Has<cadrs_ui::menu::MenuOpenAnchor>,
            Has<ListOpen>,
        ),
        With<FolderTab>,
    >,
    mut q_icon: Query<&mut cadrs_ui::Icon, With<cadrs_ui::tabs::TabTrailingIcon>>,
    mut commands: Commands,
) {
    for (tab, children, open, marked_open) in &q {
        if open && !marked_open {
            commands.entity(tab).try_insert((
                ListOpen,
                cadrs_ui::ForceState(cadrs_ui::VisualState::Pressed),
            ));
        } else if !open && marked_open {
            commands
                .entity(tab)
                .try_remove::<(ListOpen, cadrs_ui::ForceState)>();
        }
        let want = if open { "chevron-down" } else { "chevron-up" };
        for c in children.iter() {
            if let Ok(mut icon) = q_icon.get_mut(c)
                && icon.name != want
            {
                icon.name = want.into();
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Create, rename, delete

/// "+" → Create folder: "Folder N" at the top level, right of the active tab when it is there
/// (else at the end), renamed in place right away.
pub fn create_folder_here(world: &mut World) {
    if crate::linked_session::refuse(world) {
        return;
    }
    let parent = None;
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let layout = tab_tree::layout(&doc.doc);
    let level: Vec<TabItem> = tab_tree::level_of(&layout, parent)
        .unwrap_or(&[])
        .iter()
        .map(|n| n.item())
        .collect();
    let before = doc
        .active
        .and_then(|a| level.iter().position(|i| *i == TabItem::Tab(a)))
        .and_then(|k| level.get(k + 1).copied());
    let id = ElementId::new();
    if doc
        .execute(&CreateTabFolder {
            id,
            name: None,
            parent,
            before,
            items: Vec::new(),
        })
        .is_ok()
    {
        world.resource_mut::<PendingFolderRename>().0 = Some(id);
    }
}

/// Creates a folder holding `items` (the tab menu's "New folder")
/// in the level of the first of them; `rename_in_bar`: its tab is renamed in place.
pub fn create_folder_with(
    world: &mut World,
    items: Vec<TabItem>,
    rename_in_bar: bool,
) -> Option<ElementId> {
    if crate::linked_session::refuse(world) {
        return None;
    }
    let mut doc = world.get_resource_mut::<ActiveDocument>()?;
    let layout = tab_tree::layout(&doc.doc);
    let parent = items.first().and_then(|i| tab_tree::parent_of(&layout, *i));
    let id = ElementId::new();
    doc.execute(&CreateTabFolder {
        id,
        name: None,
        parent,
        before: None,
        items,
    })
    .ok()?;
    if rename_in_bar {
        world.resource_mut::<PendingFolderRename>().0 = Some(id);
    }
    Some(id)
}

fn start_pending_rename(
    mut pending: ResMut<PendingFolderRename>,
    q: Query<(Entity, &FolderTab)>,
    theme: Res<Theme>,
    doc: Option<Res<ActiveDocument>>,
    mut commands: Commands,
) {
    let Some(folder) = pending.0 else { return };
    let Some((e, _)) = q.iter().find(|(_, f)| f.0 == folder) else {
        return;
    };
    pending.0 = None;
    let name = doc
        .and_then(|d| d.doc.tab_tree.folder(folder).map(|f| f.name.clone()))
        .unwrap_or_default();
    start_rename(&mut commands, &theme, e, name);
}

fn start_rename(commands: &mut Commands, theme: &Theme, entity: Entity, name: String) {
    let mut opts = InlineEditOptions::new("tab-folder-rename-field");
    opts.width = Val::Px(120.0);
    opts.height = 21.0;
    begin_inline_edit(commands, theme, entity, name, opts);
}

fn on_folder_rename_commit(
    ev: On<InlineEditCommit>,
    q: Query<&FolderTab>,
    doc: Option<ResMut<ActiveDocument>>,
) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let (Ok(f), Some(mut doc)) = (q.get(ev.entity), doc) else {
        return;
    };
    let name = ev.value.trim().to_string();
    if name.is_empty() || doc.doc.tab_tree.folder(f.0).is_some_and(|x| x.name == name) {
        return;
    }
    let _ = doc.execute(&RenameTabFolder { id: f.0, name });
}

fn on_folder_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&FolderTab>,
    q_bar: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform, &Name)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(f) = q.get(ev.entity).copied() else {
        return;
    };
    let tab = ev.entity;
    let menu = Menu::new("tab-folder-menu")
        .side(PopoverSide::Top)
        .min_width(170.0)
        .item_height(25.0)
        .item(MenuItem::new("tab-folder-open", "Show tabs").icon("folder"))
        .item(MenuItem::new("tab-folder-rename", "Rename…").icon("edit"))
        .separator()
        .item(
            MenuItem::new("tab-folder-delete", "Delete folder…")
                .icon("remove-circle")
                .danger(),
        );
    // Above the tab bar, at the cursor's x (like a tab's menu).
    let bar_top = q_bar
        .iter()
        .find(|(.., n)| n.as_str() == "tab-bar")
        .map(|(n, t, _)| {
            let s = n.inverse_scale_factor();
            t.translation.y * s - n.size().y * s / 2.0
        })
        .unwrap_or(ev.position.y);
    let anchor = open_context_menu(
        &mut commands,
        Vec2::new(ev.position.x, bar_top - ABOVE_BAR),
        menu.build(&theme),
    );
    commands.entity(anchor).insert((
        FolderMenuFor(f.0),
        FolderTabOf(tab),
        DespawnOnExit(AppState::Document),
    ));
}

/// The folder tab a folder's right-click menu is for.
#[derive(Component)]
struct FolderTabOf(Entity);

fn on_folder_menu_action(
    ev: On<MenuAction>,
    q: Query<(&FolderMenuFor, &FolderTabOf), With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok((target, tab)) = q.get(ev.entity) else {
        return;
    };
    let (folder, tab) = (target.0, tab.0);
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match item.as_str() {
        // (After the context menu has closed, so the list stays.)
        "tab-folder-open" => {
            world.commands().queue(move |world: &mut World| {
                open_folder_list(world, tab, folder);
            });
        }
        "tab-folder-rename" => rename_folder(world, folder),
        "tab-folder-delete" => ask_delete_folder(world, folder),
        _ => {}
    });
}

/// Renames a folder: a top-level one in place on its tab; one inside another (no tab of its
/// own) in a name popup above the tab of the folder it is in.
pub fn rename_folder(world: &mut World, folder: ElementId) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let layout = tab_tree::layout(&doc.doc);
    if tab_tree::parent_of(&layout, TabItem::Folder(folder)).is_none() {
        world.resource_mut::<PendingFolderRename>().0 = Some(folder);
        return;
    }
    let name = doc
        .doc
        .tab_tree
        .folder(folder)
        .map(|f| f.name.clone())
        .unwrap_or_default();
    // The top-level folder it is in, whose tab the popup sits over.
    let mut top = folder;
    while let Some(p) = tab_tree::parent_of(&layout, TabItem::Folder(top)) {
        top = p;
    }
    let at = popup_spot(world, top);
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let popup = cadrs_ui::NamePopup::new("tab-folder-rename-popup", "Folder name", at)
        .value(name)
        .above()
        .spawn(&mut commands, &theme);
    commands.entity(popup).insert(RenamePopupFor(folder));
    world.flush();
}

/// Where a name popup for something in `folder` (a top-level folder) goes: its bottom-left
/// corner over the folder's tab, just above the bar.
pub fn popup_spot(world: &mut World, folder: ElementId) -> Vec2 {
    let bar = tab_bar_rect(world);
    let mut q = world.query::<(&FolderTab, &ComputedNode, &bevy::ui::UiGlobalTransform)>();
    let left = q
        .iter(world)
        .find(|(f, ..)| f.0 == folder)
        .map(|(_, n, t)| {
            let s = n.inverse_scale_factor();
            t.translation.x * s - n.size().x * s / 2.0
        });
    match (bar, left) {
        (Some(bar), Some(left)) => Vec2::new(left, bar.min.y - ABOVE_BAR),
        (Some(bar), None) => Vec2::new(bar.min.x + 40.0, bar.min.y - ABOVE_BAR),
        _ => Vec2::new(120.0, 400.0),
    }
}

/// The top-level folder holding `item` (itself when it is one).
pub fn top_folder_of(doc: &cadrs_core::Document, item: TabItem) -> Option<ElementId> {
    let layout = tab_tree::layout(doc);
    let mut top = match item {
        TabItem::Folder(f) => Some(f),
        TabItem::Tab(_) => None,
    };
    let mut cur = item;
    while let Some(p) = tab_tree::parent_of(&layout, cur) {
        top = Some(p);
        cur = TabItem::Folder(p);
    }
    top
}

fn on_rename_popup_commit(
    ev: On<cadrs_ui::NamePopupCommit>,
    q: Query<&RenamePopupFor>,
    doc: Option<ResMut<ActiveDocument>>,
) {
    let (Ok(f), Some(mut doc)) = (q.get(ev.entity), doc) else {
        return;
    };
    let name = ev.value.trim().to_string();
    if name.is_empty() || doc.doc.tab_tree.folder(f.0).is_some_and(|x| x.name == name) {
        return;
    }
    let _ = doc.execute(&RenameTabFolder { id: f.0, name });
}

/// Delete folder…: an empty folder goes at once; otherwise a dialog asks about its tabs.
pub fn ask_delete_folder(world: &mut World, folder: ElementId) {
    if crate::linked_session::refuse(world) {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let layout = tab_tree::layout(&doc.doc);
    let n = tab_tree::tabs_in(&layout, folder).len();
    let Some(name) = doc.doc.tab_tree.folder(folder).map(|f| f.name.clone()) else {
        return;
    };
    // What it holds itself (tabs and folders), and the tabs in all.
    let (own_tabs, own_folders) = tab_tree::level_of(&layout, Some(folder))
        .unwrap_or(&[])
        .iter()
        .fold((0, 0), |(t, f), n| match n {
            TabNode::Tab(_) => (t + 1, f),
            TabNode::Folder { .. } => (t, f + 1),
        });
    let all = doc.doc.elements.len();
    if n == 0 {
        let _ = world
            .resource_mut::<ActiveDocument>()
            .execute(&DeleteTabFolder {
                id: folder,
                delete_tabs: false,
            });
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("delete-tab-folder-dialog")
            .title("Delete folder")
            .width(460.0)
            .body(move |b| {
                b.spawn((Text::new(format!("Delete the folder \u{201c}{name}\u{201d}?")), tb.font(tb.font_base, FontWeight::NORMAL), TextColor(tb.foreground)));
                let count = |k: usize, one: &str, many: &str| format!("{k} {}", if k == 1 { one } else { many });
                let mut holds = Vec::new();
                if own_tabs > 0 {
                    holds.push(count(own_tabs, "tab", "tabs"));
                }
                if own_folders > 0 {
                    holds.push(count(own_folders, "folder", "folders"));
                }
                let mut what = holds.join(" and ");
                if own_folders > 0 {
                    what.push_str(&format!(" ({} in all)", count(n, "tab", "tabs")));
                }
                let them = if own_tabs + own_folders == 1 { "it" } else { "them" };
                b.spawn((
                    tb.text(format!("It holds {what}. Delete folder only moves {them} up a level; Delete folder and tabs deletes every tab in it too."), tb.font_base, FontWeight::NORMAL, tb.muted_foreground),
                    Node { max_width: Val::Px(420.0), margin: UiRect::top(Val::Px(6.0)), ..default() },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                f.spawn(Button::new("delete-tab-folder-keep").label("Delete folder only").primary().build(&tf));
                f.spawn(Button::new("delete-tab-folder-all").label("Delete folder and tabs").danger().disabled(n >= all).build(&tf));
                f.spawn(Button::new("delete-tab-folder-cancel").label("Cancel").build(&tf));
            })
            .build(&theme),
        DeleteFolderDialog(folder),
        DespawnOnExit(AppState::Document),
    ));
}

fn close_delete_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<DeleteFolderDialog>>();
    let v: Vec<Entity> = q.iter(world).collect();
    for e in v {
        world.entity_mut(e).despawn();
    }
}

fn on_delete_button(
    a: On<Activate>,
    q: Query<&Name>,
    q_dialog: Query<&DeleteFolderDialog>,
    mut commands: Commands,
) {
    let Ok(name) = q.get(a.entity) else { return };
    let delete_tabs = match name.as_str() {
        "delete-tab-folder-keep" => false,
        "delete-tab-folder-all" => true,
        "delete-tab-folder-cancel" => {
            commands.queue(close_delete_dialog);
            return;
        }
        _ => return,
    };
    let Some(folder) = q_dialog.iter().next().map(|d| d.0) else {
        return;
    };
    commands.queue(move |world: &mut World| {
        close_delete_dialog(world);
        if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
            let _ = doc.execute(&DeleteTabFolder {
                id: folder,
                delete_tabs,
            });
        }
    });
}

fn on_delete_close(
    ev: On<DialogClose>,
    q: Query<(), With<DeleteFolderDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(close_delete_dialog);
    }
}

// ---------------------------------------------------------------------------------------------
// The tab menu's Move to folder ▸

/// The tab menu's "Move to folder" submenu for tab `id`: every folder but its own, "Top level"
/// when it is in a folder, and "New folder".
pub fn move_to_folder_entries(doc: &cadrs_core::Document, id: ElementId) -> Vec<MenuEntry> {
    let layout = tab_tree::layout(doc);
    let parent = tab_tree::parent_of(&layout, TabItem::Tab(id));
    let mut out: Vec<MenuEntry> = Vec::new();
    if parent.is_some() {
        out.push(MenuItem::new("tab-to-top", "Top level").icon("home").into());
    }
    for (k, (f, _, _)) in tab_tree::folders(&layout).into_iter().enumerate() {
        if Some(f) == parent {
            continue;
        }
        // A folder inside another by its path: "Brackets › Fasteners".
        let path = tab_tree::folder_path(&layout, f)
            .into_iter()
            .map(|(_, n)| n)
            .collect::<Vec<_>>()
            .join(" › ");
        out.push(
            MenuItem::new(format!("tab-to-folder-{}", k + 1), path)
                .icon("folder")
                .into(),
        );
    }
    out.push(MenuEntry::Separator);
    out.push(
        MenuItem::new("tab-to-new-folder", "New folder")
            .icon("folder-new")
            .into(),
    );
    out
}

/// Handles the submenu's items (from `document::on_tab_menu_action`). True if it was one.
pub fn tab_menu_action(world: &mut World, id: ElementId, item: &str) -> bool {
    let target = if item == "tab-to-top" {
        None
    } else if item == "tab-to-new-folder" {
        create_folder_with(world, vec![TabItem::Tab(id)], true);
        return true;
    } else if let Some(k) = item
        .strip_prefix("tab-to-folder-")
        .and_then(|k| k.parse::<usize>().ok())
    {
        let Some(doc) = world.get_resource::<ActiveDocument>() else {
            return true;
        };
        let Some(f) = tab_tree::folders(&tab_tree::layout(&doc.doc))
            .get(k - 1)
            .map(|f| f.0)
        else {
            return true;
        };
        Some(f)
    } else {
        return false;
    };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return true;
    };
    let name = doc
        .doc
        .element(id)
        .map(|e| e.name.clone())
        .unwrap_or_default();
    let _ = doc.execute(&MoveTabItems {
        items: vec![TabItem::Tab(id)],
        parent: target,
        before: None,
        label: format!("Move {name} to folder"),
    });
    true
}

// ---------------------------------------------------------------------------------------------
// ▾: the tabs out of sight

#[allow(clippy::too_many_arguments)]
fn on_overflow(
    a: On<Activate>,
    q: Query<&Name>,
    q_open: Query<(), With<OverflowPanel>>,
    q_strip: Query<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    q_tabs: Query<(
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Option<&TabButton>,
        Option<&FolderTab>,
    )>,
    doc: Option<Res<ActiveDocument>>,
    windows: Query<&Window>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if !q.get(a.entity).is_ok_and(|n| n.as_str() == "tab-overflow") {
        return;
    }
    if !q_open.is_empty() {
        commands.queue(close_overflow);
        return;
    }
    let Some(doc) = doc else { return };
    let Some((_, sn, st)) = q_strip.iter().find(|(n, ..)| n.as_str() == "tab-strip") else {
        return;
    };
    // Above the bar, its right edge on the bar's (and the view toolbar's over it).
    let (Some((_, bn, bt)), Some(window)) = (
        q_strip.iter().find(|(n, ..)| n.as_str() == "tab-bar"),
        windows.iter().next(),
    ) else {
        return;
    };
    let bar = {
        let s = bn.inverse_scale_factor();
        Rect::from_center_size(bt.translation * s, bn.size() * s)
    };
    let (panel_right, panel_bottom) = (
        window.width() - bar.max.x,
        window.height() - bar.min.y + ABOVE_BAR,
    );
    let s = sn.inverse_scale_factor();
    let (l, r) = (
        st.translation.x * s - sn.size().x * s / 2.0,
        st.translation.x * s + sn.size().x * s / 2.0,
    );
    // The tabs (and folders) mostly out of the strip's view, in bar order: (x, item, before).
    let mut hidden: Vec<(f32, TabItem, bool)> = q_tabs
        .iter()
        .filter_map(|(n, t, tab, folder)| {
            let item = match (tab, folder) {
                (Some(b), _) => TabItem::Tab(b.0),
                (_, Some(f)) => TabItem::Folder(f.0),
                _ => return None,
            };
            let (x, w) = (t.translation.x * s, n.size().x * s);
            let visible = (x + w / 2.0).min(r) - (x - w / 2.0).max(l);
            (visible < w * 0.5).then_some((x, item, x < l))
        })
        .collect();
    hidden.sort_by(|a, b| a.0.total_cmp(&b.0));
    let t = &*theme;
    let active = doc.active;
    // A popover list over the tab bar's right end: it scrolls with a visible scrollbar, marks
    // the active tab and separates the tabs before the view from those after it.
    commands.spawn((
        Name::new("tab-overflow-dismiss"),
        OverflowPanel,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        GlobalZIndex(cadrs_ui::z::MENU - 1),
        Pickable::default(),
        DespawnOnExit(AppState::Document),
        observe(|_: On<PointerClick>, mut commands: Commands| commands.queue(close_overflow)),
    ));
    let panel = commands
        .spawn((
            Name::new("tab-overflow-menu"),
            OverflowPanel,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(panel_right),
                bottom: Val::Px(panel_bottom),
                width: Val::Px(240.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::vertical(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
            BoxShadow::new(
                t.shadow,
                Val::Px(0.0),
                Val::Px(2.0),
                Val::Px(0.0),
                Val::Px(8.0),
            ),
            GlobalZIndex(cadrs_ui::z::MENU),
            Pickable::default(),
            DespawnOnExit(AppState::Document),
        ))
        .id();
    let rows: Vec<(usize, TabItem, bool, String, &'static str)> = hidden
        .iter()
        .enumerate()
        .map(|(k, (_, item, before))| {
            let (label, icon) = match item {
                TabItem::Tab(e) => {
                    let el = doc.doc.element(*e);
                    (
                        el.map(|e| e.name.clone()).unwrap_or_default(),
                        el.map(crate::tab_preview::tab_icon)
                            .unwrap_or("part-studio"),
                    )
                }
                TabItem::Folder(f) => (
                    doc.doc
                        .tab_tree
                        .folder(*f)
                        .map(|f| f.name.clone())
                        .unwrap_or_default(),
                    "folder",
                ),
            };
            (k + 1, *item, *before, label, icon)
        })
        .collect();
    commands.entity(panel).with_children(|p| {
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            min_height: Val::Px(0.0),
            ..default()
        })
        .with_children(|frame| {
            let list = frame
                .spawn((
                    Name::new("tab-overflow-list"),
                    bevy::ui_widgets::ScrollArea,
                    cadrs_ui::scrollbar::ScrollGutter(6.0),
                    Node {
                        flex_direction: FlexDirection::Column,
                        max_height: Val::Px(400.0),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                ))
                .with_children(|l| {
                    if rows.is_empty() {
                        l.spawn((
                            Name::new("tab-overflow-none"),
                            t.text(
                                "Every tab is in view",
                                t.font_base,
                                FontWeight::NORMAL,
                                t.muted_foreground,
                            ),
                            Node {
                                margin: UiRect::all(Val::Px(8.0)),
                                ..default()
                            },
                        ));
                    }
                    let mut was_before = None;
                    for (k, item, before, label, icon) in &rows {
                        if was_before == Some(true) && !before {
                            l.spawn((
                                Name::new("tab-overflow-separator"),
                                Node {
                                    height: Val::Px(1.0),
                                    flex_shrink: 0.0,
                                    margin: UiRect::vertical(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(t.separator),
                                Pickable::IGNORE,
                            ));
                        }
                        was_before = Some(*before);
                        let is_active = matches!(item, TabItem::Tab(e) if Some(*e) == active);
                        l.spawn((
                            Name::new(format!("tab-overflow-{k}")),
                            OverflowRow(*item),
                            bevy::ui_widgets::Button,
                            bevy::picking::hover::Hovered::default(),
                            cadrs_ui::style::Visuals {
                                background: StateColors::new(
                                    Color::NONE,
                                    t.menu_hover,
                                    t.list_active,
                                    Color::NONE,
                                )
                                .with_selected(t.list_selected),
                                border: StateColors::all(Color::NONE),
                                foreground: StateColors::all(t.foreground),
                                focus_ring: t.focus_ring,
                            },
                            cadrs_ui::style::InitState {
                                disabled: false,
                                selected: is_active,
                                force: None,
                            },
                            Node {
                                height: Val::Px(25.0),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(8.0),
                                padding: UiRect::new(
                                    Val::Px(12.0),
                                    Val::Px(14.0),
                                    Val::ZERO,
                                    Val::ZERO,
                                ),
                                ..default()
                            },
                        ))
                        .with_children(|r| {
                            r.spawn((
                                cadrs_ui::icon::icon(*icon, 15.0, t.tool_foreground),
                                Pickable::IGNORE,
                            ));
                            r.spawn((
                                t.text(
                                    label.clone(),
                                    t.font_base,
                                    if is_active {
                                        FontWeight::SEMIBOLD
                                    } else {
                                        FontWeight::NORMAL
                                    },
                                    t.foreground,
                                ),
                                Node {
                                    flex_grow: 1.0,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ));
                            if is_active {
                                r.spawn((
                                    Name::new(format!("tab-overflow-{k}-check")),
                                    cadrs_ui::icon::icon("check", 14.0, t.primary),
                                    Pickable::IGNORE,
                                ));
                            }
                        });
                    }
                })
                .id();
            frame.spawn(cadrs_ui::vertical_scrollbar(
                t,
                "tab-overflow-scrollbar",
                list,
            ));
        });
    });
}

/// The ▾ list and its dismiss layer.
#[derive(Component)]
pub(crate) struct OverflowPanel;

/// A tab (or folder) in the ▾ list.
#[derive(Component, Clone, Copy)]
struct OverflowRow(TabItem);

fn close_overflow(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<OverflowPanel>>();
    let v: Vec<Entity> = q.iter(world).collect();
    for e in v {
        if let Ok(e) = world.get_entity_mut(e) {
            e.despawn();
        }
    }
}

fn close_overflow_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    q: Query<(), With<OverflowPanel>>,
    mut commands: Commands,
) {
    if !q.is_empty() && keys.just_pressed(KeyCode::Escape) {
        commands.queue(close_overflow);
    }
}

fn on_overflow_row(a: On<Activate>, q: Query<&OverflowRow>, mut commands: Commands) {
    let Ok(row) = q.get(a.entity).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| {
        close_overflow(world);
        match row.0 {
            TabItem::Tab(e) => {
                if let Some(mut d) = world.get_resource_mut::<ActiveDocument>() {
                    d.set_active(e);
                }
            }
            // Its list, over ▾.
            TabItem::Folder(f) => {
                let mut q = world.query::<(Entity, &Name)>();
                let anchor = q
                    .iter(world)
                    .find(|(_, n)| n.as_str() == "tab-overflow")
                    .map(|(e, _)| e);
                if let Some(anchor) = anchor {
                    open_folder_list(world, anchor, f);
                }
            }
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Dragging tabs along the bar

/// A tab or folder being dragged in the tab bar.
#[derive(Resource, Debug, Default)]
pub struct TabDrag {
    items: Vec<TabItem>,
    /// The dragged tab's entity (dimmed while it moves).
    source: Option<Entity>,
    start: Vec2,
    pointer: Vec2,
    /// Past the threshold (a click that wobbles isn't a drag).
    pub moved: bool,
    target: Option<Drop>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Drop {
    /// Into a folder (or Home: the top level), at its end.
    Into(Option<ElementId>),
    /// In the bar's level, before an item (`None`: at the end).
    Before(Option<ElementId>, Option<TabItem>),
}

#[derive(Component)]
struct DropMark;

/// A veil over a dragged tab (P3E.2 judge r1: the dragged item dims).
#[derive(Component)]
pub struct DragDim;

/// Dims `e` (once) while it is dragged.
pub fn dim(commands: &mut Commands, e: Entity) {
    dim_with(commands, e, Color::srgba(1.0, 1.0, 1.0, 0.6));
}

/// [`dim`] with a veil of this colour (P3E.3a: a dragged tab in the strip is veiled grey, so
/// the white active tab dims too; P3E.2 carried delta).
pub fn dim_with(commands: &mut Commands, e: Entity, veil: Color) {
    commands.spawn((
        Name::new("drag-dim"),
        DragDim,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            ..default()
        },
        BackgroundColor(veil),
        Pickable::IGNORE,
        ChildOf(e),
    ));
}

#[derive(Component)]
struct DragGhost;

/// The view's bottom-right tools step aside while something the tab bar opened covers part of
/// them (a folder's list or the tab's card at the window's right edge left half-cut icons
/// showing beside it).
#[allow(clippy::type_complexity)]
fn hide_covered_viewport_tools(
    mut q_tools: Query<
        (&ComputedNode, &bevy::ui::UiGlobalTransform, &mut Visibility),
        With<crate::document::ViewportTools>,
    >,
    q_over: Query<
        (&ComputedNode, &bevy::ui::UiGlobalTransform),
        Or<(
            With<cadrs_ui::menu::MenuPopup>,
            With<OverflowPanel>,
            With<crate::tab_preview::PreviewCard>,
            With<cadrs_ui::name_popup::NamePopupRoot>,
        )>,
    >,
) {
    let rect = |n: &ComputedNode, t: &bevy::ui::UiGlobalTransform| {
        let s = n.inverse_scale_factor();
        Rect::from_center_size(t.translation * s, n.size() * s)
    };
    for (n, t, mut v) in &mut q_tools {
        let tools = rect(n, t);
        let covered = q_over.iter().any(|(on, ot)| {
            let r = rect(on, ot);
            r.width() > 0.0 && !r.intersect(tools).is_empty()
        });
        v.set_if_neq(if covered {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        });
    }
}

/// A tab whose hover fill is held off while another tab is dragged over it.
#[derive(Component)]
struct DragQuiet;

/// While a tab is dragged, the tab under the pointer shows no hover fill: with the blue line
/// beside it, a fill read as "drops onto it" (only a folder's blue box means that).
#[allow(clippy::type_complexity)]
fn quiet_hover_while_dragging(
    drag: Res<TabDrag>,
    q: Query<
        (
            Entity,
            &bevy::picking::hover::Hovered,
            Has<cadrs_ui::Selected>,
            Has<DragQuiet>,
        ),
        Or<(With<TabButton>, With<FolderTab>)>,
    >,
    mut commands: Commands,
) {
    let dragging = drag.moved && !drag.items.is_empty();
    for (e, hovered, selected, quiet) in &q {
        let want = dragging && hovered.get() && !selected;
        if want && !quiet {
            commands.entity(e).try_insert((
                DragQuiet,
                cadrs_ui::ForceState(cadrs_ui::VisualState::Normal),
            ));
        } else if !want && quiet {
            commands
                .entity(e)
                .try_remove::<(DragQuiet, cadrs_ui::ForceState)>();
        }
    }
}

fn item_of(e: Entity, q_tab: &Query<&TabButton>, q_folder: &Query<&FolderTab>) -> Option<TabItem> {
    q_tab
        .get(e)
        .map(|t| TabItem::Tab(t.0))
        .ok()
        .or_else(|| q_folder.get(e).map(|f| TabItem::Folder(f.0)).ok())
}

fn on_drag_start(
    ev: On<PointerDragStart>,
    q_tab: Query<&TabButton>,
    q_folder: Query<&FolderTab>,
    mut drag: ResMut<TabDrag>,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let Some(item) = item_of(ev.entity, &q_tab, &q_folder) else {
        return;
    };
    *drag = TabDrag {
        items: vec![item],
        source: Some(ev.entity),
        start: ev.pointer.position,
        pointer: ev.pointer.position,
        moved: false,
        target: None,
    };
}

fn on_drag(
    ev: On<PointerDrag>,
    q_tab: Query<&TabButton>,
    q_folder: Query<&FolderTab>,
    mut drag: ResMut<TabDrag>,
) {
    if item_of(ev.entity, &q_tab, &q_folder).is_none() || drag.items.is_empty() {
        return;
    }
    drag.pointer = ev.pointer.position;
    if drag.pointer.distance(drag.start) > 6.0 {
        drag.moved = true;
    }
}

fn on_drag_end(
    ev: On<PointerDragEnd>,
    q_tab: Query<&TabButton>,
    q_folder: Query<&FolderTab>,
    mut commands: Commands,
) {
    if item_of(ev.entity, &q_tab, &q_folder).is_none() {
        return;
    }
    commands.queue(|world: &mut World| {
        let drag = std::mem::take(&mut *world.resource_mut::<TabDrag>());
        // The click that ends a drag must not open a folder: `moved` stays set for a frame.
        world.resource_mut::<TabDrag>().moved = drag.moved;
        let (true, Some(target)) = (drag.moved, drag.target) else {
            return;
        };
        drop_items(world, drag.items, target);
    });
}

fn drop_items(world: &mut World, items: Vec<TabItem>, target: Drop) {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let layout = tab_tree::layout(&doc.doc);
    let (parent, before) = match target {
        Drop::Into(f) => (f, None),
        Drop::Before(p, b) => (p, b),
    };
    // Nothing moves: the same level, and the item already right before `before`.
    if let [one] = items.as_slice() {
        let cur_parent = tab_tree::parent_of(&layout, *one);
        if cur_parent == parent
            && let Some(level) = tab_tree::level_of(&layout, parent)
        {
            let k = level.iter().position(|n| n.item() == *one);
            let next = k.and_then(|k| level.get(k + 1)).map(|n| n.item());
            if before == Some(*one) || next == before {
                return;
            }
        }
        if Drop::Into(Some(one.id())) == target {
            return;
        }
    }
    let name = match items.as_slice() {
        [TabItem::Tab(e)] => doc
            .doc
            .element(*e)
            .map(|e| e.name.clone())
            .unwrap_or_default(),
        [TabItem::Folder(f)] => doc
            .doc
            .tab_tree
            .folder(*f)
            .map(|f| f.name.clone())
            .unwrap_or_default(),
        many => format!("{} tabs", many.len()),
    };
    let label = match target {
        Drop::Into(Some(f)) => format!(
            "Move {name} into {}",
            doc.doc
                .tab_tree
                .folder(f)
                .map(|f| f.name.as_str())
                .unwrap_or("folder")
        ),
        Drop::Into(None) => format!("Move {name} to the top level"),
        Drop::Before(..) => format!("Reorder {name}"),
    };
    let _ = doc.execute(&MoveTabItems {
        items,
        parent,
        before,
        label,
    });
}

/// Where a dragged tab would land, and the blue mark there.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_tab_drop(
    mut drag: ResMut<TabDrag>,
    q_tabs: Query<(
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Option<&TabButton>,
        Option<&FolderTab>,
    )>,
    q_bar: Query<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    mut q_mark: Query<
        (Entity, &mut Node, &mut BackgroundColor, &mut BorderColor),
        (With<DropMark>, Without<DragGhost>),
    >,
    mut q_ghost: Query<(Entity, &mut Node), (With<DragGhost>, Without<DropMark>)>,
    q_dim: Query<Entity, With<DragDim>>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if drag.items.is_empty() || !drag.moved {
        if drag.items.is_empty() && drag.moved {
            // The frame after a drop: clicks may open folders again.
            drag.moved = false;
        }
        if drag.items.is_empty() {
            for e in &q_dim {
                commands.entity(e).try_despawn();
            }
        }
        for (e, ..) in &q_mark {
            commands.entity(e).try_despawn();
        }
        for (e, _) in &q_ghost {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if q_dim.is_empty()
        && let Some(src) = drag.source
        && commands.get_entity(src).is_ok()
    {
        dim_with(&mut commands, src, Color::srgba(0.80, 0.80, 0.80, 0.72));
    }
    let Some((_, bn, bt)) = q_bar.iter().find(|(n, ..)| n.as_str() == "tab-bar") else {
        return;
    };
    let s = bn.inverse_scale_factor();
    let (bar_top, bar_h) = (
        bt.translation.y * s - bn.size().y * s / 2.0,
        bn.size().y * s,
    );
    let p = drag.pointer;
    let blue = Color::srgb_u8(0x2b, 0x64, 0xc0);
    // (left, right, what) of the bar's tabs and folders.
    let mut hits: Vec<(f32, f32, TabItem)> = Vec::new();
    for (n, t, tab, folder) in &q_tabs {
        let what = match (tab, folder) {
            (Some(b), _) => TabItem::Tab(b.0),
            (_, Some(f)) => TabItem::Folder(f.0),
            _ => continue,
        };
        let (x, w) = (t.translation.x * s, n.size().x * s);
        if w <= 0.0 {
            continue;
        }
        hits.push((x - w / 2.0, x + w / 2.0, what));
    }
    hits.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Out of the bar (with some slack): no drop.
    let near = p.y > bar_top - 40.0 && p.y < bar_top + bar_h + 20.0;
    let mut mark: Option<(f32, f32, bool)> = None; // (left, width, box)
    drag.target = None;
    if near {
        // Onto a folder's middle.
        for (l, r, what) in &hits {
            if p.x < *l || p.x > *r {
                continue;
            }
            if let TabItem::Folder(f) = what
                && !drag.items.contains(&TabItem::Folder(*f))
                && p.x > l + (r - l) * 0.25
                && p.x < r - (r - l) * 0.25
            {
                drag.target = Some(Drop::Into(Some(*f)));
                mark = Some((*l, r - l, true));
            }
        }
        if drag.target.is_none()
            && let Some(first) = hits.first()
        {
            let k = hits.iter().position(|(l, r, _)| p.x < (l + r) / 2.0);
            let before = k.map(|k| hits[k].2);
            let x = match k {
                Some(k) => hits[k].0 - 1.0,
                None => hits.last().map_or(first.1, |t| t.1 + 1.0),
            };
            drag.target = Some(Drop::Before(None, before));
            mark = Some((x - 1.5, 3.0, false));
        }
    }
    match mark {
        None => {
            for (e, ..) in &q_mark {
                commands.entity(e).try_despawn();
            }
        }
        Some((left, width, boxed)) => {
            let (bg, border) = if boxed {
                (blue.with_alpha(0.22), blue)
            } else {
                (blue, blue)
            };
            // The tabs' height: inside the bar's border and padding.
            let (lv, wv, tv, hv) = (
                Val::Px(left),
                Val::Px(width),
                Val::Px(bar_top + 4.0),
                Val::Px(bar_h - 8.0),
            );
            match q_mark.iter_mut().next() {
                Some((_, mut n, mut b, mut bc)) => {
                    if n.left != lv || n.width != wv || n.top != tv {
                        n.left = lv;
                        n.width = wv;
                        n.top = tv;
                        n.height = hv;
                    }
                    b.set_if_neq(BackgroundColor(bg));
                    bc.set_if_neq(BorderColor::all(border));
                }
                None => {
                    commands.spawn((
                        Name::new("tab-drop-line"),
                        DropMark,
                        Node {
                            position_type: PositionType::Absolute,
                            left: lv,
                            top: tv,
                            width: wv,
                            height: hv,
                            border: UiRect::all(Val::Px(2.0)),
                            border_radius: BorderRadius::all(Val::Px(3.0)),
                            ..default()
                        },
                        BackgroundColor(bg),
                        BorderColor::all(border),
                        GlobalZIndex(cadrs_ui::z::DIALOG - 20),
                        Pickable::IGNORE,
                        DespawnOnExit(AppState::Document),
                    ));
                }
            }
        }
    }
    // A ghost of the dragged tab follows the pointer, over the bar (clear of the drop mark).
    let (gx, gy) = (Val::Px(p.x + 14.0), Val::Px(bar_top - ABOVE_BAR - 24.0));
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
                    (
                        el.map(|e| e.name.clone()).unwrap_or_default(),
                        el.map(crate::tab_preview::tab_icon)
                            .unwrap_or("part-studio"),
                    )
                }
                [TabItem::Folder(f)] => (
                    doc.doc
                        .tab_tree
                        .folder(*f)
                        .map(|f| f.name.clone())
                        .unwrap_or_default(),
                    "folder",
                ),
                many => (format!("{} tabs", many.len()), "tab-manager"),
            };
            let t = &*theme;
            commands.spawn((
                Name::new("tab-drag-ghost"),
                DragGhost,
                Node {
                    position_type: PositionType::Absolute,
                    left: gx,
                    top: gy,
                    height: Val::Px(24.0),
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(6.0),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(t.radius)),
                    ..default()
                },
                BackgroundColor(t.popover.with_alpha(0.7)),
                BorderColor::all(t.border),
                GlobalZIndex(cadrs_ui::z::DIALOG - 19),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
                children![
                    (
                        cadrs_ui::icon::icon(icon, 15.0, t.tool_foreground),
                        Pickable::IGNORE
                    ),
                    (
                        t.text(
                            label,
                            t.font_sm,
                            FontWeight::MEDIUM,
                            t.foreground.with_alpha(0.85)
                        ),
                        Pickable::IGNORE
                    ),
                ],
            ));
        }
    }
}

/// The tab tree's nodes at a level (the top level: `None`).
pub fn level_items(doc: &cadrs_core::Document, folder: Option<ElementId>) -> Vec<TabNode> {
    let layout = tab_tree::layout(doc);
    tab_tree::level_of(&layout, folder)
        .map(|l| l.to_vec())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------------------------
// Scenario set-up

/// `tab-fixture [N]` (scenarios): stores and opens "Many tabs", a document of N tabs (60 by
/// default): small Part Studios "Studio 2", "Studio 3", … (a block each), every fifth an
/// Assembly, every twelfth a Drawing (an empty ANSI A sheet). Built from
/// [`cadrs_core::samples::scale::document`] without its 250-feature studio.
pub fn fixture(world: &mut World, arg: &str) {
    let n = arg.trim().parse::<usize>().unwrap_or(60).max(1);
    let mut doc = cadrs_core::samples::scale::document(n + 1);
    doc.elements.remove(0);
    doc.name = format!("Many tabs ({n})");
    doc.id = cadrs_core::DocumentId::from_u128(0x3e20_0000_0000_0000_0000_0000_0000_0600);
    if let Some(t) = cadrs_drawing::template::builtin("ANSI_A_MM.dwt") {
        let mut k = 0;
        for (i, e) in doc.elements.iter_mut().enumerate() {
            if (i + 1) % 12 == 0 {
                k += 1;
                let id = e.id;
                *e = cadrs_core::Element::drawing(
                    format!("Drawing {k}"),
                    cadrs_drawing::Drawing::from_template(&t, None),
                );
                e.id = id;
            }
        }
    }
    let store = world.resource::<crate::DocumentStore>().0.clone();
    let now = world.resource::<crate::AppClock>().now();
    let user = world.resource::<crate::UserProfile>().id.clone();
    let meta = cadrs_core::DocumentMeta::new(&user, now - 600);
    if let Err(e) = store.create(&doc, &meta) {
        warn!("tab-fixture: {e}");
    }
    world.insert_resource(ActiveDocument::stored(doc, meta));
}

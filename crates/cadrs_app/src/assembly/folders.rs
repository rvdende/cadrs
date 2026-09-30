//! Folders, dragging rows and the filter of the assembly lists (P3B.4, `intro-to-assemblies.md`
//! A1.5, A17.4, A18; `lesson-assembly-folders.png`, `ex3-step3.png`, `ex3-step7.png`,
//! `ex3-step11.png`). The folder model is P3.9's ([`cadrs_core::assembly::folders`]).
//!
//! - **New folder** (the Instances header's icon, A18.2) and **Add selection to folder…** (the
//!   instance and mate menus, A18.3, A18.5) open the **Folder name** popup (✓ / ✗, Enter / Esc)
//!   beside the first selected row; the folder gathers the selection at its first item (with
//!   nothing selected it is empty, at the end of the list).
//! - A folder row's ▸ opens and closes it, its eye hides or shows what it holds, a double-click
//!   renames it, and its menu (A18.4) has **Rename**, **Hide** / **Show**, **Suppress** /
//!   **Unsuppress**, **Delete** (the folder and its contents) and **Unpack folder**.
//! - **Dragging rows** (instances, mates, a folder with its contents, a subassembly's
//!   instances): a blue line shows where they land, between rows (reordering; between two rows
//!   of a folder they join it) or onto a folder's row (they join it at its end). Instances
//!   dropped onto a **subassembly row** move into it (a badge counts them, "4 items", A17.4); a
//!   subassembly's instance dragged anywhere else in the list moves out to the top level. Each
//!   drop is one undo step.
//! - The **filter** field (A1.5) over both lists: the P3.9 filter language
//!   ([`cadrs_core::feature_list::Filter`]).

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use cadrs_core::FeatureId;
use cadrs_core::assembly::InstanceId;
use cadrs_core::assembly::commands::{SetInstancesHidden, SetMatesSuppressed};
use cadrs_core::assembly::folders::{
    CreateAssemblyFolder, DeleteAssemblyFolder, FolderList, MoveListItems, SetAssemblyFolder, UnpackAssemblyFolder, folders, item, mate_item, order,
};
use cadrs_core::assembly::mate::MateId;
use cadrs_core::assembly::structure::{MoveIntoSubassembly, MoveOutOfSubassembly, SetInstancesSuppressed};
use cadrs_core::feature_list::Filter;
use cadrs_ui::menu::{ContextMenuAnchor, ContextMenuRequested, Menu, MenuAction, MenuItem, open_context_menu};
use cadrs_ui::prelude::*;
use cadrs_ui::{NamePopup, NamePopupCommit};

use super::list::{ChildRow, InstanceRow};
use super::mates_list::MateRow;
use crate::viewport::Selection;
use crate::{ActiveDocument, AppState};

pub struct AssemblyFoldersPlugin;

impl Plugin for AssemblyFoldersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ListUi>()
            .init_resource::<AsmDrag>()
            .add_systems(Update, (read_filter, place_drop_line).run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut ui: ResMut<ListUi>| *ui = ListUi::default())
            .add_observer(on_new_folder)
            .add_observer(on_popup_commit)
            .add_observer(on_folder_toggle)
            .add_observer(on_folder_menu)
            .add_observer(on_folder_menu_action)
            .add_observer(on_folder_double_click)
            .add_observer(on_folder_rename)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end);
    }
}

/// The lists' view state: open subassemblies and the filter.
#[derive(Resource, Debug, Clone, Default)]
pub struct ListUi {
    pub expanded: HashSet<InstanceId>,
    pub filter_text: String,
    pub filter: Option<Filter>,
}

/// A folder's row in the Instances or Mate Features list.
#[derive(Component, Debug, Clone, Copy)]
pub struct AsmFolderRow {
    pub list: FolderList,
    pub id: FeatureId,
}

/// A row in a folder.
#[derive(Component, Debug, Clone, Copy)]
pub struct InAsmFolder(pub FeatureId);

/// The Folder name popup open: what it will put in a new folder.
#[derive(Component, Debug, Clone)]
struct FolderPopup {
    element: cadrs_core::ElementId,
    list: FolderList,
    items: Vec<FeatureId>,
}

fn read_filter(q: Query<(&Name, &EditableText)>, mut ui: ResMut<ListUi>) {
    let Some(text) = q.iter().find(|(n, _)| n.as_str() == "instance-filter-field").map(|(_, t)| t.value().to_string()) else { return };
    if ui.filter_text != text {
        ui.filter = Filter::parse(&text);
        ui.filter_text = text;
    }
}

// ---------------------------------------------------------------------------------------------
// New folder, Add selection to folder…

fn on_new_folder(a: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity).is_ok_and(|n| n.as_str() == "instance-new-folder") {
        commands.queue(|world: &mut World| {
            let selected = super::selected_instances(world.resource::<Selection>());
            add_to_folder(world, FolderList::Instances, selected.iter().map(|i| item(*i)).collect(), None);
        });
    }
}

/// Opens the Folder name popup for a new folder of `items` in `list` (A18.2, A18.3), beside the
/// first of them (or under the list's header).
/// The Folder name popup opens at `menu_at` (where the context menu was opened, P3B.4 judge),
/// else beside the first item's row.
pub fn add_to_folder(world: &mut World, list: FolderList, items: Vec<FeatureId>, menu_at: Option<Vec2>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(element) = super::active_assembly(doc) else { return };
    let Some(model) = doc.active_element().and_then(|e| e.assembly_model()) else { return };
    // In list order.
    let items: Vec<FeatureId> = order(model, list).into_iter().filter(|i| items.contains(i)).collect();
    let all = [folders(model, FolderList::Instances), folders(model, FolderList::Mates)].concat();
    let default = (1..).map(|n| format!("Folder {n}")).find(|n| !all.iter().any(|f| &f.name == n)).unwrap_or_default();
    // Beside the first item's row (else the list header).
    let first = items.first().copied();
    let mut at = Vec2::new(250.0, 120.0);
    let mut q = world.query::<(&ComputedNode, &bevy::ui::UiGlobalTransform, Option<&InstanceRow>, Option<&MateRow>, Option<&Name>)>();
    for (node, t, ir, mr, name) in q.iter(world) {
        let hit = match (ir, mr) {
            (Some(r), _) => first == Some(item(r.0)),
            (_, Some(r)) => first == Some(mate_item(r.0)),
            _ => first.is_none() && name.is_some_and(|n| n.as_str() == "instances-header"),
        };
        if hit {
            let s = node.inverse_scale_factor();
            let size = node.size() * s;
            let c = t.translation * s;
            at = Vec2::new(c.x + size.x / 2.0 + 10.0, c.y - size.y / 2.0);
        }
    }
    if let Some(p) = menu_at {
        at = p + Vec2::new(12.0, 0.0);
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let popup = NamePopup::new("folder-name-popup", "Folder name", at).value(default).spawn(&mut commands, &theme);
    commands.entity(popup).insert((FolderPopup { element, list, items }, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_popup_commit(ev: On<NamePopupCommit>, q: Query<&FolderPopup>, mut commands: Commands) {
    let Ok(p) = q.get(ev.entity).cloned() else { return };
    let name = ev.value.trim().to_string();
    if name.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        super::run(world, &CreateAssemblyFolder { element: p.element, list: p.list, folder: FeatureId::new(), name: Some(name), items: p.items });
    });
}

// ---------------------------------------------------------------------------------------------
// Folder rows

fn folder_of(world: &World, list: FolderList, id: FeatureId) -> Option<(cadrs_core::ElementId, cadrs_core::document::FeatureFolder)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let el = super::active_assembly(doc)?;
    let model = doc.active_element()?.assembly_model()?;
    Some((el, folders(model, list).iter().find(|f| f.id == id)?.clone()))
}

fn on_folder_toggle(ev: On<cadrs_ui::TreeToggle>, q: Query<&AsmFolderRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity).copied() else { return };
    commands.queue(move |world: &mut World| {
        let Some((element, f)) = folder_of(world, row.list, row.id) else { return };
        super::run(world, &SetAssemblyFolder { element, list: row.list, folder: row.id, open: Some(!f.open), name: None });
    });
}

#[derive(Component, Debug, Clone, Copy)]
struct FolderMenu(AsmFolderRow);

fn on_folder_menu(ev: On<ContextMenuRequested>, q: Query<&AsmFolderRow>, mut commands: Commands) {
    let Ok(row) = q.get(ev.entity).copied() else { return };
    let at = ev.position;
    commands.queue(move |world: &mut World| {
        let Some((_, f)) = folder_of(world, row.list, row.id) else { return };
        let model = world.resource::<ActiveDocument>().active_element().and_then(|e| e.assembly_model().cloned()).unwrap_or_default();
        let (hidden, suppressed) = match row.list {
            FolderList::Instances => {
                let inside: Vec<&cadrs_core::assembly::Instance> = f.features.iter().filter_map(|x| model.instance(InstanceId(x.0))).collect();
                (!inside.is_empty() && inside.iter().all(|i| i.hidden), !inside.is_empty() && inside.iter().all(|i| i.suppressed))
            }
            FolderList::Mates => {
                let shown = &world.resource::<super::mate_display::MateDisplay>().shown;
                let inside: Vec<&cadrs_core::assembly::mate::MateFeature> = f.features.iter().filter_map(|x| model.mate(MateId(x.0))).collect();
                (!inside.iter().any(|m| shown.contains(&m.id)), !inside.is_empty() && inside.iter().all(|m| m.suppressed))
            }
        };
        let menu = Menu::new("asm-folder-menu")
            .min_width(170.0)
            .item_height(22.0)
            .item(MenuItem::new("asm-folder-rename", "Rename"))
            .item(if hidden { MenuItem::new("asm-folder-show", "Show").icon("visible") } else { MenuItem::new("asm-folder-hide", "Hide").icon("hidden") })
            .item(if suppressed { MenuItem::new("asm-folder-unsuppress", "Unsuppress") } else { MenuItem::new("asm-folder-suppress", "Suppress") })
            .separator()
            .item(MenuItem::new("asm-folder-unpack", "Unpack folder"))
            .item(MenuItem::new("asm-folder-delete", "Delete").icon("remove-circle"));
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
        commands.entity(anchor).insert((FolderMenu(row), DespawnOnExit(AppState::Document)));
        world.flush();
    });
}

fn on_folder_menu_action(ev: On<MenuAction>, q: Query<&FolderMenu, With<ContextMenuAnchor>>, mut commands: Commands) {
    let Ok(FolderMenu(row)) = q.get(ev.entity).copied() else { return };
    let action = ev.item.clone();
    commands.queue(move |world: &mut World| folder_action(world, row, &action));
}

fn folder_action(world: &mut World, row: AsmFolderRow, action: &str) {
    let Some((element, f)) = folder_of(world, row.list, row.id) else { return };
    let (list, folder) = (row.list, row.id);
    let instances: Vec<InstanceId> = f.features.iter().map(|x| InstanceId(x.0)).collect();
    let mates: Vec<MateId> = f.features.iter().map(|x| MateId(x.0)).collect();
    match (action, list) {
        ("asm-folder-rename", _) => rename_folder(world, row),
        ("asm-folder-unpack", _) => {
            super::run(world, &UnpackAssemblyFolder { element, list, folder });
        }
        ("asm-folder-delete", _) => {
            super::run(world, &DeleteAssemblyFolder { element, list, folder });
        }
        ("asm-folder-hide" | "asm-folder-show", FolderList::Instances) if !instances.is_empty() => {
            super::run(world, &SetInstancesHidden { element, instances, hidden: action == "asm-folder-hide" });
        }
        ("asm-folder-hide" | "asm-folder-show", FolderList::Mates) => {
            let mut d = world.resource_mut::<super::mate_display::MateDisplay>();
            for m in mates {
                if action == "asm-folder-hide" {
                    d.shown.remove(&m);
                } else {
                    d.shown.insert(m);
                }
            }
        }
        ("asm-folder-suppress" | "asm-folder-unsuppress", FolderList::Instances) if !instances.is_empty() => {
            super::run(world, &SetInstancesSuppressed { element, instances, suppressed: action == "asm-folder-suppress" });
        }
        ("asm-folder-suppress" | "asm-folder-unsuppress", FolderList::Mates) if !mates.is_empty() => {
            super::run(world, &SetMatesSuppressed { element, mates, suppressed: action == "asm-folder-suppress" });
        }
        _ => {}
    }
}

/// Starts renaming a folder in place (Enter commits, one undo step).
fn rename_folder(world: &mut World, row: AsmFolderRow) {
    let Some((_, f)) = folder_of(world, row.list, row.id) else { return };
    let mut q = world.query::<(Entity, &AsmFolderRow)>();
    let Some(e) = q.iter(world).find(|(_, r)| r.id == row.id).map(|(e, _)| e) else { return };
    let theme = world.resource::<Theme>().clone();
    let mut opts = cadrs_ui::InlineEditOptions::new("asm-folder-rename");
    opts.width = Val::Px(140.0);
    opts.height = 20.0;
    opts.font_size = Some(theme.font_sm);
    opts.weight = FontWeight::MEDIUM;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    cadrs_ui::begin_inline_edit(&mut commands, &theme, e, f.name, opts);
    world.flush();
}

fn on_folder_double_click(ev: On<cadrs_ui::DoubleClick>, q: Query<&AsmFolderRow>, mut commands: Commands) {
    if let Ok(row) = q.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| rename_folder(world, row));
    }
}

fn on_folder_rename(ev: On<cadrs_ui::InlineEditCommit>, q: Query<&AsmFolderRow>, mut commands: Commands) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Ok(row) = q.get(ev.entity).copied() else { return };
    let name = ev.value.trim().to_string();
    commands.queue(move |world: &mut World| {
        let Some((element, f)) = folder_of(world, row.list, row.id) else { return };
        if !name.is_empty() && f.name != name {
            super::run(world, &SetAssemblyFolder { element, list: row.list, folder: row.id, open: None, name: Some(name) });
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Dragging rows

/// What is being dragged, where the pointer is, and where it would land.
#[derive(Resource, Debug, Default)]
pub struct AsmDrag {
    pub list: Option<FolderList>,
    /// Top-level items (instances or mates, a folder's contents).
    pub items: Vec<FeatureId>,
    /// Or instances of an open subassembly (its instance id, theirs in its tab).
    pub child: Option<(InstanceId, Vec<InstanceId>)>,
    pub pointer: Vec2,
    pub target: Option<Drop>,
}

/// Where a drag lands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Drop {
    /// At this index of the list without the dragged items, in this folder.
    At { to: usize, folder: Option<FeatureId> },
    /// Into this subassembly (A17.4).
    Into(InstanceId),
}

#[derive(Component)]
struct DropLine;

#[derive(Component)]
struct DropBadge;

#[allow(clippy::too_many_arguments)]
fn on_drag_start(
    ev: On<Pointer<DragStart>>,
    q_inst: Query<&InstanceRow>,
    q_child: Query<&ChildRow>,
    q_mate: Query<&MateRow>,
    q_folder: Query<&AsmFolderRow>,
    doc: Option<Res<ActiveDocument>>,
    selection: Res<Selection>,
    mut drag: ResMut<AsmDrag>,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let Some(model) = doc.as_ref().and_then(|d| d.active_element()?.assembly_model()) else { return };
    let pointer = ev.pointer_location.position;
    let (list, items, child) = if let Ok(r) = q_inst.get(ev.entity) {
        // The selection when the row is in it (in list order), else the row.
        let sel = super::selected_instances(&selection);
        let ids: Vec<InstanceId> = if sel.contains(&r.0) { model.instances.iter().map(|i| i.id).filter(|i| sel.contains(i)).collect() } else { vec![r.0] };
        (FolderList::Instances, ids.into_iter().map(item).collect(), None)
    } else if let Ok(c) = q_child.get(ev.entity) {
        (FolderList::Instances, Vec::new(), Some((c.sub, vec![c.id])))
    } else if let Ok(m) = q_mate.get(ev.entity) {
        (FolderList::Mates, vec![mate_item(m.0)], None)
    } else if let Ok(f) = q_folder.get(ev.entity) {
        let items = folders(model, f.list).iter().find(|x| x.id == f.id).map(|x| x.features.clone()).unwrap_or_default();
        if items.is_empty() {
            return;
        }
        (f.list, items, None)
    } else {
        return;
    };
    *drag = AsmDrag { list: Some(list), items, child, pointer, target: None };
}

type AnyRow = Or<(With<InstanceRow>, With<ChildRow>, With<MateRow>, With<AsmFolderRow>)>;

fn on_drag(ev: On<Pointer<Drag>>, q: Query<(), AnyRow>, mut drag: ResMut<AsmDrag>) {
    if q.contains(ev.entity) && drag.list.is_some() {
        drag.pointer = ev.pointer_location.position;
    }
}

fn on_drag_end(ev: On<Pointer<DragEnd>>, q: Query<(), AnyRow>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let drag = std::mem::take(&mut *world.resource_mut::<AsmDrag>());
        let (Some(list), Some(target)) = (drag.list, drag.target) else { return };
        let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
        let Some(element) = super::active_assembly(doc) else { return };
        let Some(model) = doc.active_element().and_then(|e| e.assembly_model()).cloned() else { return };
        let instances: Vec<InstanceId> = drag.items.iter().map(|x| InstanceId(x.0)).collect();
        match (target, drag.child) {
            (Drop::Into(sub), None) => {
                super::run(world, &MoveIntoSubassembly { element, sub, instances });
            }
            (Drop::At { to, .. }, Some((sub, kids))) => {
                // A17.4: out of the subassembly, to the top level (`to` counts top-level rows).
                super::run(world, &MoveOutOfSubassembly { element, sub, instances: kids, at: Some(to) });
            }
            (Drop::At { to, folder }, None) => {
                let o = order(&model, list);
                let first = o.iter().position(|x| Some(x) == drag.items.first());
                let without: Vec<&FeatureId> = o.iter().filter(|x| !drag.items.contains(x)).collect();
                let at_before = first.map(|f| o[..f].iter().filter(|x| !drag.items.contains(x)).count());
                let same_folder = drag.items.first().and_then(|x| cadrs_core::assembly::folders::folder_of(&model, list, *x)).map(|f| f.id) == folder;
                if at_before == Some(to) && same_folder || to > without.len() {
                    return;
                }
                super::run(world, &MoveListItems { element, list, items: drag.items, to, folder, label: "Reorder".into() });
            }
            _ => {}
        }
    });
}

/// One row of a list on screen.
struct ScreenRow {
    top: f32,
    bottom: f32,
    left: f32,
    width: f32,
    what: RowWhat,
}

/// The row a drop goes into (a subassembly or a folder), highlighted while dragging.
#[derive(Component)]
struct DropHighlight;

#[derive(Clone, Copy, PartialEq)]
enum RowWhat {
    Item { id: FeatureId, sub: bool, folder: Option<FeatureId> },
    Child,
    Folder(FeatureId),
}

/// Where a drop would land, the blue line there, and the "n items" badge over a subassembly.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_drop_line(
    mut drag: ResMut<AsmDrag>,
    doc: Option<Res<ActiveDocument>>,
    q_rows: Query<(
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Option<&InstanceRow>,
        Option<&MateRow>,
        Option<&AsmFolderRow>,
        Option<&InAsmFolder>,
        Option<&ChildRow>,
    )>,
    mut q_line: Query<(Entity, &mut Node), (With<DropLine>, Without<DropBadge>)>,
    mut q_badge: Query<(Entity, &mut Node, &Children), (With<DropBadge>, Without<DropLine>)>,
    mut q_text: Query<&mut Text>,
    mut q_hl: Query<(Entity, &mut Node), (With<DropHighlight>, Without<DropLine>, Without<DropBadge>)>,
    theme: Res<Theme>,
    cache: Res<crate::parts::PartCache>,
    mut commands: Commands,
) {
    let clear = |commands: &mut Commands, q_line: &Query<(Entity, &mut Node), (With<DropLine>, Without<DropBadge>)>| {
        for (e, _) in q_line.iter() {
            commands.entity(e).try_despawn();
        }
    };
    let Some(list) = drag.list else {
        clear(&mut commands, &q_line);
        for (e, ..) in &q_badge {
            commands.entity(e).try_despawn();
        }
        for (e, _) in &q_hl {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(model) = doc.as_ref().and_then(|d| d.active_element()?.assembly_model()) else { return };
    let mut rows: Vec<ScreenRow> = Vec::new();
    for (node, t, ir, mr, fr, inf, cr) in &q_rows {
        let what = match (list, ir, mr, fr, cr) {
            (FolderList::Instances, Some(r), ..) => {
                RowWhat::Item { id: item(r.0), sub: model.instance(r.0).is_some_and(|i| i.source.is_assembly()), folder: inf.map(|f| f.0) }
            }
            (FolderList::Mates, _, Some(r), ..) => RowWhat::Item { id: mate_item(r.0), sub: false, folder: inf.map(|f| f.0) },
            (_, _, _, Some(f), _) if f.list == list => RowWhat::Folder(f.id),
            (FolderList::Instances, _, _, _, Some(_)) => RowWhat::Child,
            _ => continue,
        };
        let s = node.inverse_scale_factor();
        let size = node.size() * s;
        if size.y < 1.0 {
            continue;
        }
        let c = t.translation * s;
        rows.push(ScreenRow { top: c.y - size.y / 2.0, bottom: c.y + size.y / 2.0, left: c.x - size.x / 2.0, width: size.x, what });
    }
    rows.sort_by(|a, b| a.top.total_cmp(&b.top));
    if rows.is_empty() {
        return;
    }
    let y = drag.pointer.y;
    let dragged = drag.items.clone();
    let o: Vec<FeatureId> = order(model, list).into_iter().filter(|x| !dragged.contains(x)).collect();
    // Onto a subassembly row's middle: into it (not for its own instances or itself).
    let over_sub = rows.iter().find_map(|r| match r.what {
        RowWhat::Item { id, sub: true, .. } if y > r.top + 3.0 && y < r.bottom - 3.0 && !dragged.contains(&id) && drag.child.is_none() && !dragged.is_empty() => {
            Some((InstanceId(id.0), r))
        }
        _ => None,
    });
    let mut badge: Option<(Vec2, String)> = None;
    let mut highlight: Option<Rect> = None;
    let count = |n: usize| if n == 1 { "1 item".to_string() } else { format!("{n} items") };
    let (target, line) = if let Some((sub, r)) = over_sub {
        badge = Some((Vec2::new(r.left + 150.0, r.top + 2.0), count(dragged.len())));
        highlight = Some(Rect::new(r.left, r.top, r.left + r.width, r.bottom));
        (Drop::Into(sub), None)
    } else if let Some(r) = rows.iter().find(|r| matches!(r.what, RowWhat::Folder(_)) && y > r.top + 5.0 && y < r.bottom - 5.0) {
        // Onto a folder's row: at its end.
        let RowWhat::Folder(f) = r.what else { unreachable!() };
        let inside = folders(model, list).iter().find(|x| x.id == f).map(|x| x.features.clone()).unwrap_or_default();
        let to = inside.iter().rev().find_map(|x| o.iter().position(|y| y == x)).map_or(o.len(), |i| i + 1);
        // The folder row lit, with the count (`ex3-step15.png`: "8 items").
        highlight = Some(Rect::new(r.left, r.top, r.left + r.width, r.bottom));
        badge = Some((Vec2::new(r.left + 150.0, r.top + 2.0), count(dragged.len())));
        (Drop::At { to, folder: Some(f) }, None)
    } else {
        // Between rows: before the first top-level row below the pointer.
        let k = rows.iter().position(|r| y < (r.top + r.bottom) / 2.0).unwrap_or(rows.len());
        let before = rows[k..].iter().find_map(|r| match r.what {
            RowWhat::Item { id, .. } if !dragged.contains(&id) => Some(id),
            RowWhat::Folder(f) => folders(model, list).iter().find(|x| x.id == f).and_then(|x| x.features.iter().find(|y| o.contains(y)).copied()),
            _ => None,
        });
        let to = before.and_then(|b| o.iter().position(|x| *x == b)).unwrap_or(o.len());
        // Inside a folder when the rows above and below are of it (or above is its header).
        let folder = match (k.checked_sub(1).map(|i| rows[i].what), rows.get(k).map(|r| r.what)) {
            (Some(RowWhat::Folder(f)), Some(RowWhat::Item { folder: Some(g), .. })) if f == g => Some(f),
            (Some(RowWhat::Item { folder: Some(f), .. }), Some(RowWhat::Item { folder: Some(g), .. })) if f == g => Some(f),
            _ => None,
        };
        let (ly, lx) = match rows.get(k) {
            Some(r) => (r.top, r.left),
            None => (rows[rows.len() - 1].bottom, rows[rows.len() - 1].left),
        };
        (Drop::At { to, folder }, Some((ly, lx + if folder.is_some() { 14.0 } else { 0.0 })))
    };
    drag.target = Some(target);
    // One row dragged: its name in a pill beside the pointer (`ex3-step11.png`: "Rear Cap mount
    // <1>").
    if badge.is_none() {
        let name = match (&drag.child, dragged.as_slice(), list) {
            (Some((sub, kids)), _, _) if kids.len() == 1 => {
                let occ = cadrs_core::assembly::structure::derive(*sub, kids[0]);
                cache.part_name(super::occurrence_part(occ)).map(str::to_string)
            }
            (None, [one], FolderList::Instances) => {
                let i = InstanceId(one.0);
                model.instance(i).map(|inst| match cache.part_name(i.part_id()) {
                    Some(n) if !inst.source.is_assembly() => n.to_string(),
                    _ => inst.name(&cadrs_core::assembly::source_part_name(&doc.as_ref().expect("checked").doc, &inst.source, None)),
                })
            }
            (None, [one], FolderList::Mates) => model.mates.iter().find(|m| m.id.0 == one.0).map(|m| m.name.clone()),
            _ => None,
        };
        if let Some(n) = name {
            badge = Some((drag.pointer + Vec2::new(14.0, -10.0), n));
        }
    }
    // The drop target's row (P3B.4 judge).
    match highlight {
        None => {
            for (e, _) in &q_hl {
                commands.entity(e).try_despawn();
            }
        }
        Some(r) => {
            let (top, left, w, h) = (Val::Px(r.min.y), Val::Px(r.min.x), Val::Px(r.width()), Val::Px(r.height()));
            match q_hl.iter_mut().next() {
                Some((_, mut n)) => {
                    if n.top != top || n.left != left || n.width != w || n.height != h {
                        n.top = top;
                        n.left = left;
                        n.width = w;
                        n.height = h;
                    }
                }
                None => {
                    commands.spawn((
                        Name::new("asm-drop-target"),
                        DropHighlight,
                        Node { position_type: PositionType::Absolute, top, left, width: w, height: h, border: UiRect::all(Val::Px(1.0)), border_radius: BorderRadius::all(Val::Px(2.0)), ..default() },
                        BackgroundColor(Color::srgba_u8(0x2b, 0x64, 0xc0, 0x22)),
                        BorderColor::all(Color::srgb_u8(0x2b, 0x64, 0xc0)),
                        GlobalZIndex(cadrs_ui::z::DIALOG - 21),
                        Pickable::IGNORE,
                        DespawnOnExit(AppState::Document),
                    ));
                }
            }
        }
    }
    // The line.
    match line {
        None => clear(&mut commands, &q_line),
        Some((ly, lx)) => {
            let top = Val::Px(ly - 1.0);
            let left = Val::Px(lx);
            match q_line.iter_mut().next() {
                Some((_, mut n)) => {
                    if n.top != top || n.left != left {
                        n.top = top;
                        n.left = left;
                    }
                }
                None => {
                    commands.spawn((
                        Name::new("asm-drop-line"),
                        DropLine,
                        Node { position_type: PositionType::Absolute, top, left, width: Val::Px(170.0), height: Val::Px(2.0), ..default() },
                        BackgroundColor(Color::srgb_u8(0x2b, 0x64, 0xc0)),
                        GlobalZIndex(cadrs_ui::z::DIALOG - 20),
                        Pickable::IGNORE,
                        DespawnOnExit(AppState::Document),
                    ));
                }
            }
        }
    }
    // The badge ("4 items", `ex3-step7.png`).
    match badge {
        None => {
            for (e, ..) in &q_badge {
                commands.entity(e).try_despawn();
            }
        }
        Some((at, label)) => {
            if let Some((_, mut node, children)) = q_badge.iter_mut().next() {
                node.left = Val::Px(at.x);
                node.top = Val::Px(at.y);
                for c in children.iter() {
                    if let Ok(mut t) = q_text.get_mut(c)
                        && t.0 != label
                    {
                        t.0 = label.clone();
                    }
                }
            } else {
                commands.spawn((
                    Name::new("asm-drop-badge"),
                    DropBadge,
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(at.x),
                        top: Val::Px(at.y),
                        padding: UiRect::axes(Val::Px(7.0), Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(9.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0x1f, 0x4e, 0x9e)),
                    GlobalZIndex(cadrs_ui::z::DIALOG - 19),
                    Pickable::IGNORE,
                    DespawnOnExit(AppState::Document),
                    children![(theme.text(label, 11.0, FontWeight::MEDIUM, Color::WHITE), Pickable::IGNORE)],
                ));
            }
        }
    }
}

//! Folders in the Parts list, as the feature list has them (`crate::feature_folders`):
//!
//! - A folder is a row with a chevron and a folder icon, "Fasteners (4)", its parts under it,
//!   indented; closed, they are hidden from the list (not from the view). Its menu renames,
//!   opens or closes and unpacks it; a double click renames it.
//! - A part's menu has **Add to folder ▸** (a new folder, named in place, or one there is) and
//!   **Remove from folder**, for the selected parts when the part is one of them.
//! - Dragging a part row (every selected part, when it is one of them) onto a folder's row or
//!   one of its parts puts the parts in that folder; onto a part out of folders, or a group's
//!   header, takes them out. The row it would drop on is outlined in blue.
//!
//! Each change is one undo step ([`cadrs_core::commands::CreatePartFolder`] and the others).

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::commands::{CreatePartFolder, MovePartsToFolder, SetPartFolder, UnpackPartFolder};
use cadrs_core::{FeatureId, PartId};
use cadrs_ui::menu::{ContextMenuAnchor, ContextMenuRequested, MenuAction, open_context_menu};
use cadrs_ui::prelude::*;

use crate::document::PartRows;
use crate::parts_list::{GroupHeader, PartRow};
use crate::viewport::Selection;
use crate::{ActiveDocument, AppState};

pub struct PartFoldersPlugin;

impl Plugin for PartFoldersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PartDrag>()
            .init_resource::<PendingRename>()
            .add_systems(
                Update,
                (place_drop_box, start_pending_rename).run_if(in_state(AppState::Document)),
            )
            .add_observer(on_folder_toggle)
            .add_observer(on_folder_menu)
            .add_observer(on_folder_menu_action)
            .add_observer(on_folder_double_click)
            .add_observer(on_folder_rename_commit)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end);
    }
}

/// A folder's row in the Parts list.
#[derive(Component, Debug, Clone, Copy)]
pub struct PartFolderRow(pub FeatureId);

/// A part row's folder (for where a drop lands).
#[derive(Component, Debug, Clone, Copy)]
pub struct InPartFolder(pub FeatureId);

#[derive(Component)]
struct FolderMenuFor(FeatureId);

/// A folder just made from a part's menu: its name is edited in place once its row is there.
#[derive(Resource, Debug, Default)]
struct PendingRename(Option<FeatureId>);

/// The folder row bundle: "Fasteners (4)" with a chevron and a folder icon.
pub fn folder_row(t: &Theme, id: FeatureId, name: &str, count: usize, open: bool) -> impl Bundle {
    let slug = crate::document::tab_node_name(name).replacen("tab-", "part-folder-", 1);
    (
        TreeItem::new(slug, format!("{name} ({count})"))
            .disclosure(Some(open))
            .icon("folder", 16.0)
            .left(8.0)
            .editable()
            .build(t),
        PartFolderRow(id),
        ContextMenuTarget,
        cadrs_ui::DoubleClickable,
    )
}

/// Runs `command` on the active Part Studio, an undo step.
fn execute(
    world: &mut World,
    command: impl FnOnce(cadrs_core::ElementId) -> Box<dyn cadrs_core::Command>,
) -> bool {
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return false;
    };
    let Some(element) = doc.active_element().map(|e| e.id) else {
        return false;
    };
    doc.execute(command(element).as_ref()).is_ok()
}

fn folder_open(world: &World, folder: FeatureId) -> bool {
    world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element())
        .and_then(|el| el.part_folders().iter().find(|f| f.id == folder))
        .is_some_and(|f| f.open)
}

fn on_folder_toggle(
    ev: On<cadrs_ui::TreeToggle>,
    q: Query<&PartFolderRow>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| toggle(world, row.0));
}

fn toggle(world: &mut World, folder: FeatureId) {
    let open = !folder_open(world, folder);
    execute(world, |element| {
        Box::new(SetPartFolder {
            element,
            folder,
            open: Some(open),
            name: None,
        })
    });
}

fn on_folder_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&PartFolderRow>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity).copied() else {
        return;
    };
    let menu = Menu::new("part-folder-context-menu")
        .min_width(160.0)
        .item_height(22.0)
        .text_only()
        .item(MenuItem::new("part-folder-menu-rename", "Rename"))
        .item(MenuItem::new(
            "part-folder-menu-toggle",
            "Open / close folder",
        ))
        .item(MenuItem::new("part-folder-menu-unpack", "Unpack folder"));
    let anchor = open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((FolderMenuFor(row.0), DespawnOnExit(AppState::Document)));
}

fn on_folder_menu_action(
    ev: On<MenuAction>,
    q: Query<&FolderMenuFor, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(target) = q.get(ev.entity) else { return };
    let folder = target.0;
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match item.as_str() {
        "part-folder-menu-rename" => rename_folder(world, folder),
        "part-folder-menu-toggle" => toggle(world, folder),
        "part-folder-menu-unpack" => {
            execute(world, |element| {
                Box::new(UnpackPartFolder { element, folder })
            });
        }
        _ => {}
    });
}

/// The part menu's Add to folder ▸ New folder: `parts` go into a new folder, whose name is then
/// edited in place.
pub fn new_folder(world: &mut World, parts: Vec<PartId>) {
    let folder = FeatureId::new();
    let made = execute(world, |element| {
        Box::new(CreatePartFolder {
            element,
            folder,
            name: None,
            parts,
        })
    });
    if made {
        world.resource_mut::<PendingRename>().0 = Some(folder);
    }
}

/// The part menu's Add to folder ▸ a folder, or Remove from folder (`None`).
pub fn move_to_folder(world: &mut World, parts: Vec<PartId>, folder: Option<FeatureId>) {
    execute(world, |element| {
        Box::new(MovePartsToFolder {
            element,
            parts,
            folder,
        })
    });
}

/// Starts renaming a folder in place (Enter commits, one undo step; Esc cancels).
fn rename_folder(world: &mut World, folder: FeatureId) {
    let name = world.get_resource::<ActiveDocument>().and_then(|d| {
        d.active_element()?
            .part_folders()
            .iter()
            .find(|f| f.id == folder)
            .map(|f| f.name.clone())
    });
    let Some(name) = name else { return };
    let mut q = world.query::<(Entity, &PartFolderRow)>();
    let Some(row) = q.iter(world).find(|(_, r)| r.0 == folder).map(|(e, _)| e) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut opts = cadrs_ui::InlineEditOptions::new("part-folder-rename");
    opts.width = Val::Px(150.0);
    opts.height = 20.0;
    opts.font_size = Some(theme.font_sm);
    opts.weight = FontWeight::MEDIUM;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    cadrs_ui::begin_inline_edit(&mut commands, &theme, row, name, opts);
    world.flush();
}

fn start_pending_rename(
    mut pending: ResMut<PendingRename>,
    q: Query<&PartFolderRow>,
    mut commands: Commands,
) {
    let Some(folder) = pending.0 else { return };
    if q.iter().any(|r| r.0 == folder) {
        pending.0 = None;
        commands.queue(move |world: &mut World| rename_folder(world, folder));
    }
}

fn on_folder_double_click(
    ev: On<cadrs_ui::DoubleClick>,
    q: Query<&PartFolderRow>,
    mut commands: Commands,
) {
    if let Ok(row) = q.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| rename_folder(world, row.0));
    }
}

fn on_folder_rename_commit(
    ev: On<cadrs_ui::InlineEditCommit>,
    q: Query<&PartFolderRow>,
    mut commands: Commands,
) {
    if ev.entity != ev.original_event_target() {
        return;
    }
    let Ok(row) = q.get(ev.entity).copied() else {
        return;
    };
    let name = ev.value.trim().to_string();
    if name.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        execute(world, |element| {
            Box::new(SetPartFolder {
                element,
                folder: row.0,
                open: None,
                name: Some(name),
            })
        });
    });
}

// ---------------------------------------------------------------------------------------------
// Drag and drop

/// Part rows being dragged: the parts, where the pointer is, and the folder they would go to
/// (`Some(None)`: out of every folder).
#[derive(Resource, Debug, Default)]
struct PartDrag {
    parts: Vec<PartId>,
    pointer: Vec2,
    target: Option<Option<FeatureId>>,
}

/// The outline round the row a drop lands on.
#[derive(Component)]
struct DropBox;

/// The dragged rows' ghost at the pointer.
#[derive(Component)]
struct DragGhost;

fn on_drag_start(
    ev: On<PointerDragStart>,
    q: Query<&PartRow>,
    selection: Res<Selection>,
    mut drag: ResMut<PartDrag>,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let Ok(row) = q.get(ev.entity) else {
        return;
    };
    let selected: Vec<PartId> = selection.0.iter().filter_map(|p| p.part()).collect();
    let parts = if selected.contains(&row.0) {
        selected
    } else {
        vec![row.0]
    };
    *drag = PartDrag {
        parts,
        pointer: ev.pointer.position,
        target: None,
    };
}

fn on_drag(ev: On<PointerDrag>, q: Query<(), With<PartRow>>, mut drag: ResMut<PartDrag>) {
    if q.contains(ev.entity) && !drag.parts.is_empty() {
        drag.pointer = ev.pointer.position;
    }
}

fn on_drag_end(ev: On<PointerDragEnd>, q: Query<(), With<PartRow>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let drag = std::mem::take(&mut *world.resource_mut::<PartDrag>());
        if let Some(folder) = drag.target {
            move_to_folder(world, drag.parts, folder);
        }
    });
}

/// Where a drop would land (the folder of the row under the pointer, or none), the blue outline
/// round that row, and the ghost of the dragged rows at the pointer.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn place_drop_box(
    mut drag: ResMut<PartDrag>,
    doc: Option<Res<ActiveDocument>>,
    cache: Res<crate::parts::PartCache>,
    q_list: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<PartRows>>,
    q_rows: Query<(
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Option<&PartRow>,
        Option<&InPartFolder>,
        Option<&PartFolderRow>,
        Has<GroupHeader>,
    )>,
    mut q_box: Query<(Entity, &mut Node), (With<DropBox>, Without<DragGhost>)>,
    mut q_ghost: Query<(Entity, &mut Node), (With<DragGhost>, Without<DropBox>)>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if drag.parts.is_empty() {
        for (e, _) in q_box.iter().chain(q_ghost.iter()) {
            commands.entity(e).try_despawn();
        }
        return;
    }
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else {
        return;
    };
    let rect = |n: &ComputedNode, at: &bevy::ui::UiGlobalTransform| {
        let s = n.inverse_scale_factor();
        Rect::from_center_size(at.translation * s, n.size() * s)
    };
    let p = drag.pointer;
    // The row under the pointer, in the list's shown part, and the folder a drop there joins.
    let over = q_list
        .iter()
        .next()
        .map(|(n, at)| rect(n, at))
        .filter(|list| list.contains(p))
        .and_then(|_| {
            q_rows
                .iter()
                .find_map(|(n, at, part, inside, folder, header)| {
                    let r = rect(n, at);
                    if r.height() <= 0.0 || !r.contains(p) {
                        return None;
                    }
                    match (part, inside, folder, header) {
                        (_, _, Some(f), _) => Some((r, Some(f.0))),
                        (Some(_), Some(f), ..) => Some((r, Some(f.0))),
                        (Some(_), None, ..) | (.., true) => Some((r, None)),
                        _ => None,
                    }
                })
        });
    // Nothing to do there: every part already in that folder (or out of folders).
    let over = over.filter(|(_, folder)| {
        !drag
            .parts
            .iter()
            .all(|x| el.part_folder_of(*x).map(|f| f.id) == *folder)
    });
    drag.target = over.map(|(_, folder)| folder);
    match (over, q_box.iter_mut().next()) {
        (None, Some((e, _))) => commands.entity(e).try_despawn(),
        (Some((r, _)), Some((_, mut n))) => {
            let (left, top, width, height) = (
                Val::Px(r.min.x),
                Val::Px(r.min.y),
                Val::Px(r.width()),
                Val::Px(r.height()),
            );
            if n.left != left || n.top != top || n.width != width || n.height != height {
                n.left = left;
                n.top = top;
                n.width = width;
                n.height = height;
            }
        }
        (Some((r, _)), None) => {
            let blue = Color::srgb_u8(0x2b, 0x64, 0xc0);
            commands.spawn((
                Name::new("part-drop-box"),
                DropBox,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(r.min.x),
                    top: Val::Px(r.min.y),
                    width: Val::Px(r.width()),
                    height: Val::Px(r.height()),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(blue.with_alpha(0.14)),
                BorderColor::all(blue),
                GlobalZIndex(cadrs_ui::z::DIALOG - 20),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
            ));
        }
        (None, None) => {}
    }
    let (gx, gy) = (Val::Px(p.x + 18.0), Val::Px(p.y + 12.0));
    match q_ghost.iter_mut().next() {
        Some((_, mut n)) => {
            if n.left != gx || n.top != gy {
                n.left = gx;
                n.top = gy;
            }
        }
        None => {
            let label = match drag.parts.as_slice() {
                [one] => cache.part_name(*one).unwrap_or("Part").to_string(),
                many => format!("{} parts", many.len()),
            };
            let t = &*theme;
            commands.spawn((
                Name::new("part-drag-ghost"),
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
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.85)),
                BorderColor::all(t.border),
                GlobalZIndex(cadrs_ui::z::DIALOG - 19),
                Pickable::IGNORE,
                DespawnOnExit(AppState::Document),
                children![
                    (
                        cadrs_ui::icon::icon("part", 14.0, t.muted_foreground),
                        Pickable::IGNORE
                    ),
                    (
                        t.text(
                            label,
                            t.font_sm,
                            FontWeight::MEDIUM,
                            t.foreground.with_alpha(0.8)
                        ),
                        Pickable::IGNORE
                    ),
                ],
            ));
        }
    }
}

//! #112: organising the documents page as a file manager does.
//!
//! - **Selecting many.** A click selects one item; Ctrl+click adds or removes one; Shift+click
//!   selects everything listed between the last click and this one
//!   ([`super::details::click_select`]).
//! - **Dragging.** Documents and folders dragged (a selected one drags the whole selection) onto
//!   a folder's row or card, or onto a breadcrumb's folder or its root, move there in one undo
//!   step ([`MoveItems`]). The target is outlined while the pointer is over it, and a label
//!   follows the pointer (`landing-drag-ghost`).
//! - **Folders' menu** (right-click a folder's row or card, `folder-context-menu`): Open,
//!   Rename… (`rename-folder-dialog`), Copy… (`copy-folder-dialog`: the folder with its folders
//!   and documents), Move to ▸ (folders nest), Properties…, Move to trash (what it holds goes with
//!   it); in the Trash, Restore and Delete permanently… (`purge-folder-dialog`).
//! - **A selection's menu** (right-click one of several selected, `multi-context-menu`): Copy,
//!   Move to ▸, Move to trash, or in the Trash, Restore and Delete permanently…
//!   (`purge-items-dialog`).
//! - **Delete** moves the selection to the trash; in the Trash it deletes it permanently, asking
//!   first. **Empty trash…** (the Trash's button and the app menu, `empty-trash-dialog`) deletes
//!   everything there permanently. Both are one undo step ([`PurgeItems`]).

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::library::{
    AddCopies, Library, MoveItems, PurgeFolder, PurgeItems, RenameFolder, RestoreEntry,
    TrashFolder, TrashItems,
};
use cadrs_core::{DocumentEntry, DocumentId, Filter, FolderEntry, FolderId};
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;

use super::details::{self, LandingDetails, Pick};
use super::{
    DocLibrary, LandingDialog, LandingState, PickRow, close_dialogs, dialog_buttons, field_label,
    field_value, undo_toast,
};
use crate::{AppClock, AppState, DocumentStore, UserProfile};

/// A folder's row or card (its right-click menu).
#[derive(Component, Clone, Copy)]
pub struct FolderRow(pub FolderId);

/// Takes documents and folders dropped on it: into a folder, or the top level (`None`).
#[derive(Component, Clone, Copy)]
pub struct DropFolder(pub Option<FolderId>);

/// The items being dragged.
#[derive(Resource, Debug, Default)]
pub struct LandingDrag {
    pub items: Vec<Pick>,
    start: Vec2,
    pointer: Vec2,
    /// Moved far enough to be a drag (kept a frame past the drop, so the release's click
    /// doesn't select).
    pub moved: bool,
}

/// The label following the pointer while dragging.
#[derive(Component)]
struct DragGhost;

/// The box drawn inside the drop target under the pointer (a child of it: the target's own
/// outline is its focus ring).
#[derive(Component)]
struct DropHover;

/// Which items a selection's menu is for.
#[derive(Component, Clone)]
struct ContextItems(Vec<Pick>);

/// Which folder a folder's menu is for.
#[derive(Component, Clone, Copy)]
struct ContextFolder(FolderId);

/// The items a Delete permanently… of several deletes (on its dialog).
#[derive(Component, Clone)]
struct PurgePicks(Vec<Pick>);

pub fn plugin(app: &mut App) {
    app.init_resource::<LandingDrag>()
        .init_resource::<details::ListOrder>()
        .add_systems(
            Update,
            (place_ghost, settle_drag).run_if(in_state(AppState::Landing)),
        )
        .add_observer(on_drag_start)
        .add_observer(on_drag)
        .add_observer(on_drag_end)
        .add_observer(on_drag_enter)
        .add_observer(on_drag_leave)
        .add_observer(on_drop)
        .add_observer(on_folder_context_menu)
        .add_observer(on_folder_action)
        .add_observer(on_multi_action);
}

// ---------------------------------------------------------------------------------------------
// Moving, copying, trashing

/// The documents and folders among `items`.
fn split(items: &[Pick]) -> (Vec<DocumentId>, Vec<FolderId>) {
    let docs = items
        .iter()
        .filter_map(|p| match p {
            Pick::Doc(d) => Some(*d),
            _ => None,
        })
        .collect();
    let folders = items
        .iter()
        .filter_map(|p| match p {
            Pick::Folder(f) => Some(*f),
            _ => None,
        })
        .collect();
    (docs, folders)
}

/// A folder's path for menus: "Projects › Brackets".
pub fn folder_label(lib: &Library, f: FolderId) -> String {
    lib.folder_path(f)
        .into_iter()
        .map(|f| f.name)
        .collect::<Vec<_>>()
        .join(" \u{203a} ")
}

/// Move to ▸: the top level, then every folder (by path) the items can go into: not in the
/// trash, not one of `moving` or inside one. `current` checks where they are now (when they are
/// all in one place).
pub fn move_entries(
    lib: &Library,
    current: Option<Option<FolderId>>,
    moving: &[FolderId],
) -> Vec<MenuEntry> {
    let mut entries: Vec<MenuEntry> = vec![
        MenuItem::new("ctx-move-top", "Documents (no folder)")
            .checked(current == Some(None))
            .into(),
    ];
    let mut folders: Vec<(String, usize, FolderId)> = lib
        .folders
        .iter()
        .enumerate()
        .filter(|(_, f)| {
            !lib.folder_hidden(f.id) && moving.iter().all(|m| !lib.is_within(f.id, *m))
        })
        .map(|(i, f)| (folder_label(lib, f.id), i, f.id))
        .collect();
    folders.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.0, &b.0));
    for (label, i, id) in folders {
        entries.push(
            MenuItem::new(format!("ctx-move-folder-{i}"), label)
                .icon("folder")
                .checked(current == Some(Some(id)))
                .into(),
        );
    }
    entries
}

/// The folder a Move to ▸ item names (`Some(None)`: the top level).
pub fn move_target(lib: &Library, item: &str) -> Option<Option<FolderId>> {
    if item == "ctx-move-top" {
        return Some(None);
    }
    let i: usize = item.strip_prefix("ctx-move-folder-")?.parse().ok()?;
    Some(Some(lib.folders.get(i)?.id))
}

/// Where the items all are now, if they are in one place.
fn common_place(lib: &Library, items: &[Pick]) -> Option<Option<FolderId>> {
    let mut places = items.iter().filter_map(|p| match p {
        Pick::Doc(d) => lib.get(*d).map(|e| e.meta.folder),
        Pick::Folder(f) => lib.folder(*f).map(|f| f.parent),
        Pick::Sample(_) => None,
    });
    let first = places.next()?;
    places.all(|p| p == first).then_some(first)
}

/// What a toast calls the items: "“Bracket”" or "3 items".
fn items_name(lib: &Library, items: &[Pick]) -> String {
    match items {
        [Pick::Doc(d)] => format!(
            "\u{201c}{}\u{201d}",
            lib.get(*d).map(|e| e.name.as_str()).unwrap_or("document")
        ),
        [Pick::Folder(f)] => format!(
            "\u{201c}{}\u{201d}",
            lib.folder(*f).map(|e| e.name.as_str()).unwrap_or("folder")
        ),
        many => format!("{} items", many.len()),
    }
}

/// Moves `items` into `to` (the top level with `None`) in one undo step. A folder dropped on
/// itself is left alone; one dropped into a folder inside it is refused with a toast.
pub fn move_items(world: &mut World, items: &[Pick], to: Option<FolderId>) {
    let lib = world.resource::<DocLibrary>().lib.clone();
    // Already there, or a folder onto itself: nothing to do for it.
    let items: Vec<Pick> = items
        .iter()
        .copied()
        .filter(|p| match p {
            Pick::Doc(d) => lib.get(*d).is_some_and(|e| e.meta.folder != to),
            Pick::Folder(f) => Some(*f) != to && lib.folder(*f).is_some_and(|x| x.parent != to),
            Pick::Sample(_) => false,
        })
        .collect();
    let (docs, folders) = split(&items);
    if docs.is_empty() && folders.is_empty() {
        return;
    }
    if let Some(t) = to
        && folders.iter().any(|f| lib.is_within(t, *f))
    {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        cadrs_ui::show_toast(
            &mut commands,
            &theme,
            "A folder can't go into a folder inside it",
        );
        world.flush();
        return;
    }
    let text = format!(
        "Moved {} to {}",
        items_name(&lib, &items),
        match to {
            Some(f) => format!("\u{201c}{}\u{201d}", folder_label(&lib, f)),
            None => "Documents".into(),
        }
    );
    let store = world.resource::<DocumentStore>().0.clone();
    world
        .resource_mut::<DocLibrary>()
        .execute(&store, &MoveItems { docs, folders, to });
    undo_toast(world, text);
}

/// Moves `items` to the trash in one undo step (a folder with what it holds).
pub fn trash_items(world: &mut World, items: &[Pick]) {
    let lib = world.resource::<DocLibrary>().lib.clone();
    let (docs, folders) = split(items);
    if docs.is_empty() && folders.is_empty() {
        return;
    }
    let text = format!("Moved {} to trash", items_name(&lib, items));
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    world
        .resource_mut::<DocLibrary>()
        .execute(&store, &TrashItems { docs, folders, now });
    undo_toast(world, text);
}

/// Deletes `items` (in the trash) permanently in one undo step.
fn purge_items(world: &mut World, items: &[Pick]) {
    let lib = world.resource::<DocLibrary>().lib.clone();
    let (docs, folders) = split(items);
    if docs.is_empty() && folders.is_empty() {
        return;
    }
    let text = format!("Deleted {} permanently", items_name(&lib, items));
    let store = world.resource::<DocumentStore>().0.clone();
    world
        .resource_mut::<DocLibrary>()
        .execute(&store, &PurgeItems { docs, folders });
    undo_toast(world, text);
}

/// Brings `items` back from the trash.
fn restore_items(world: &mut World, items: &[Pick]) {
    let store = world.resource::<DocumentStore>().0.clone();
    for p in items {
        let mut lib = world.resource_mut::<DocLibrary>();
        match *p {
            Pick::Doc(id) => lib.execute(&store, &RestoreEntry { id }),
            Pick::Folder(id) => lib.execute(&store, &TrashFolder { id, now: None }),
            Pick::Sample(_) => {}
        }
    }
    let lib = world.resource::<DocLibrary>().lib.clone();
    undo_toast(world, format!("Restored {}", items_name(&lib, items)));
}

/// Copies of `docs` (each "Copy of <name>", in `into`, or where it is with `None`) and of
/// `folders` (each with its folders and live documents, the top one named `top_name` when
/// given, else "Copy of <name>", beside the original) made on disk, added in one undo step.
fn copy_items(
    world: &mut World,
    docs: &[DocumentId],
    folders: &[FolderId],
    top_name: Option<String>,
) {
    let lib = world.resource::<DocLibrary>().lib.clone();
    let store = world.resource::<DocumentStore>().0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let mut new_folders: Vec<FolderEntry> = Vec::new();
    let mut new_docs: Vec<DocumentEntry> = Vec::new();
    let mut copy_doc = |e: &DocumentEntry, name: &str, folder: Option<FolderId>| {
        let id = DocumentId::new();
        match store.copy_document(e.id, id, name, &user, now) {
            Ok(mut entry) => {
                entry.meta.folder = folder;
                new_docs.push(entry);
            }
            Err(err) => error!("cannot copy \u{201c}{}\u{201d}: {err}", e.name),
        }
    };
    for d in docs {
        if let Some(e) = lib.get(*d) {
            copy_doc(e, &format!("Copy of {}", e.name), e.meta.folder);
        }
    }
    for f in folders {
        let Some(top) = lib.folder(*f) else { continue };
        // Each folder of the tree gets a new id; the copies keep the shape.
        let tree = lib.folder_tree(*f);
        let ids: Vec<(FolderId, FolderId)> = tree.iter().map(|x| (*x, FolderId::new())).collect();
        let new_id = |old: FolderId| ids.iter().find(|(o, _)| *o == old).map(|(_, n)| *n);
        for (old, new) in &ids {
            let Some(src) = lib.folder(*old) else {
                continue;
            };
            let name = if *old == top.id {
                top_name
                    .clone()
                    .unwrap_or_else(|| format!("Copy of {}", src.name))
            } else {
                src.name.clone()
            };
            new_folders.push(FolderEntry {
                parent: if *old == top.id {
                    top.parent
                } else {
                    src.parent.and_then(new_id)
                },
                ..FolderEntry::new(*new, &name, &user, now)
            });
            for e in lib.in_folder(*old) {
                copy_doc(&e, &e.name, Some(*new));
            }
        }
    }
    if new_folders.is_empty() && new_docs.is_empty() {
        return;
    }
    let text = match (new_folders.first(), new_docs.as_slice()) {
        (Some(f), _) => format!("Copied to \u{201c}{}\u{201d}", f.name),
        (None, [e]) => format!("Copied to \u{201c}{}\u{201d}", e.name),
        (None, many) => format!("Copied {} documents", many.len()),
    };
    world.resource_mut::<DocLibrary>().execute(
        &store,
        &AddCopies {
            folders: new_folders,
            entries: new_docs,
        },
    );
    undo_toast(world, text);
}

// ---------------------------------------------------------------------------------------------
// Menus

/// A selection's menu, for a right-click on one of several selected items.
pub fn multi_menu(lib: &Library, items: &[Pick], trash: bool) -> Menu {
    let menu = Menu::new("multi-context-menu").min_width(190.0);
    if trash {
        return menu
            .item(MenuItem::new("multi-restore", "Restore").icon("restore"))
            .item(MenuItem::new("multi-delete-forever", "Delete permanently…").icon("delete"));
    }
    let (_, folders) = split(items);
    menu.item(MenuItem::new("multi-copy", "Copy").icon("copy"))
        .item(
            MenuItem::new("multi-move", "Move to…")
                .icon("move-to-folder")
                .submenu(move_entries(lib, common_place(lib, items), &folders)),
        )
        .separator()
        .item(MenuItem::new("multi-trash", "Move to trash").icon("delete"))
}

/// Opens a selection's menu at `position` for `items`.
pub fn open_multi_menu(
    commands: &mut Commands,
    theme: &Theme,
    lib: &Library,
    items: Vec<Pick>,
    trash: bool,
    position: Vec2,
) {
    let menu = multi_menu(lib, &items, trash);
    let anchor = cadrs_ui::open_context_menu(commands, position, menu.build(theme));
    commands
        .entity(anchor)
        .insert((ContextItems(items), DespawnOnExit(AppState::Landing)));
}

fn on_multi_action(
    ev: On<MenuAction>,
    q: Query<&ContextItems, With<ContextMenuAnchor>>,
    lib: Res<DocLibrary>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(items) = q.get(ev.entity) else { return };
    let items = items.0.clone();
    match ev.item.as_str() {
        "multi-copy" => {
            let (docs, folders) = split(&items);
            commands.queue(move |world: &mut World| copy_items(world, &docs, &folders, None));
        }
        "multi-trash" => commands.queue(move |world: &mut World| trash_items(world, &items)),
        "multi-restore" => commands.queue(move |world: &mut World| restore_items(world, &items)),
        "multi-delete-forever" => open_purge_items_dialog(&mut commands, &theme, &lib.lib, items),
        m => {
            if let Some(to) = move_target(&lib.lib, m) {
                commands.queue(move |world: &mut World| move_items(world, &items, to));
            }
        }
    }
}

fn on_folder_context_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&FolderRow>,
    state: Res<LandingState>,
    details_state: Res<LandingDetails>,
    theme: Res<Theme>,
    lib: Res<DocLibrary>,
    mut commands: Commands,
) {
    let Ok(row) = q.get(ev.entity) else { return };
    let id = row.0;
    let trash = state.filter == Filter::Trash;
    let selection = details_state.selection.clone();
    if selection.len() > 1 && selection.contains(&Pick::Folder(id)) {
        open_multi_menu(
            &mut commands,
            &theme,
            &lib.lib,
            selection,
            trash,
            ev.position,
        );
        return;
    }
    commands.queue(move |world: &mut World| details::select(world, Pick::Folder(id), false));
    let menu = if trash {
        Menu::new("folder-context-menu")
            .item(MenuItem::new("folder-restore", "Restore").icon("restore"))
            .item(MenuItem::new("folder-delete-forever", "Delete permanently…").icon("delete"))
            .min_width(190.0)
    } else {
        let parent = lib.lib.folder(id).map(|f| f.parent);
        Menu::new("folder-context-menu")
            .item(MenuItem::new("folder-open", "Open").icon("folder"))
            .item(MenuItem::new("folder-rename", "Rename…").icon("edit"))
            .separator()
            .item(MenuItem::new("folder-copy", "Copy…").icon("copy"))
            .item(
                MenuItem::new("folder-move", "Move to…")
                    .icon("move-to-folder")
                    .submenu(move_entries(&lib.lib, parent, &[id])),
            )
            .item(MenuItem::new("folder-properties", "Properties…").icon("info"))
            .separator()
            .item(MenuItem::new("folder-trash", "Move to trash").icon("delete"))
            .min_width(190.0)
    };
    let anchor = cadrs_ui::open_context_menu(&mut commands, ev.position, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((ContextFolder(id), DespawnOnExit(AppState::Landing)));
}

fn on_folder_action(
    ev: On<MenuAction>,
    q: Query<&ContextFolder, With<ContextMenuAnchor>>,
    lib: Res<DocLibrary>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(f) = q.get(ev.entity) else { return };
    let id = f.0;
    let Some(folder) = lib.lib.folder(id).cloned() else {
        return;
    };
    let items = vec![Pick::Folder(id)];
    match ev.item.as_str() {
        "folder-open" => {
            commands.queue(move |world: &mut World| details::open_pick(world, items[0]))
        }
        "folder-rename" => open_rename_folder_dialog(&mut commands, &theme, &folder),
        "folder-copy" => open_copy_folder_dialog(&mut commands, &theme, &folder),
        "folder-properties" => commands.queue(move |world: &mut World| {
            details::open_details(world, Pick::Folder(id), details::DetailsTab::Info)
        }),
        "folder-trash" => commands.queue(move |world: &mut World| trash_items(world, &items)),
        "folder-restore" => commands.queue(move |world: &mut World| restore_items(world, &items)),
        "folder-delete-forever" => open_purge_folder_dialog(&mut commands, &theme, &folder),
        m => {
            if let Some(to) = move_target(&lib.lib, m) {
                commands.queue(move |world: &mut World| move_items(world, &items, to));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Folder dialogs

fn open_rename_folder_dialog(commands: &mut Commands, theme: &Theme, folder: &FolderEntry) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let name = folder.name.clone();
    commands.spawn((
        Dialog::new("rename-folder-dialog")
            .title("Rename folder")
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "Folder name");
                b.spawn(
                    TextInput::new("rename-folder-name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
            })
            .footer(move |f| dialog_buttons(f, &tf, "rename-folder-dialog", "Rename"))
            .build(&t),
        LandingDialog::RenameFolder(folder.id),
        DespawnOnExit(AppState::Landing),
    ));
}

/// Copy…: the copy's name, prefilled "Copy of <name>" and selected.
fn open_copy_folder_dialog(commands: &mut Commands, theme: &Theme, folder: &FolderEntry) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let name = format!("Copy of {}", folder.name);
    commands.spawn((
        Dialog::new("copy-folder-dialog")
            .title("Copy folder")
            .width(420.0)
            .body(move |b| {
                field_label(b, &tb, "New folder name");
                b.spawn(
                    TextInput::new("copy-folder-name")
                        .value(name)
                        .select_all_on_focus()
                        .autofocus()
                        .build(&tb),
                );
                b.spawn(tb.text(
                    "Its folders and documents are copied with it.",
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| dialog_buttons(f, &tf, "copy-folder-dialog", "Copy"))
            .build(&t),
        LandingDialog::CopyFolder(folder.id),
        DespawnOnExit(AppState::Landing),
    ));
}

fn open_purge_folder_dialog(commands: &mut Commands, theme: &Theme, folder: &FolderEntry) {
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let name = folder.name.clone();
    commands.spawn((
        Dialog::new("purge-folder-dialog")
            .title("Delete permanently")
            .width(420.0)
            .body(move |b| {
                b.spawn((
                    Text::new(format!(
                        "Permanently delete \u{201c}{name}\u{201d} and everything in it?"
                    )),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                ));
                b.spawn(tb.text(
                    "You cannot restore it from Trash.",
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| dialog_buttons(f, &tf, "purge-folder-dialog", "Delete permanently"))
            .build(&t),
        LandingDialog::PurgeFolder(folder.id),
        DespawnOnExit(AppState::Landing),
    ));
}

/// Delete permanently… for `items` in the Trash: a document's or a folder's own dialog for one,
/// else one for them all.
pub fn open_purge_items_dialog(
    commands: &mut Commands,
    theme: &Theme,
    lib: &Library,
    items: Vec<Pick>,
) {
    match items.as_slice() {
        [] => {}
        [Pick::Doc(d)] => {
            if let Some(e) = lib.get(*d) {
                super::open_purge_dialog(commands, theme, e);
            }
        }
        [Pick::Folder(f)] => {
            if let Some(f) = lib.folder(*f) {
                open_purge_folder_dialog(commands, theme, f);
            }
        }
        _ => {
            let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
            let n = items.len();
            commands.spawn((
                Dialog::new("purge-items-dialog")
                    .title("Delete permanently")
                    .width(420.0)
                    .body(move |b| {
                        b.spawn((
                            Text::new(format!("Permanently delete {n} items?")),
                            tb.font(tb.font_base, FontWeight::NORMAL),
                            TextColor(tb.foreground),
                        ));
                        b.spawn(tb.text(
                            "You cannot restore them from Trash.",
                            tb.font_base,
                            FontWeight::NORMAL,
                            tb.muted_foreground,
                        ));
                    })
                    .footer(move |f| {
                        dialog_buttons(f, &tf, "purge-items-dialog", "Delete permanently")
                    })
                    .build(&t),
                LandingDialog::PurgeItems,
                PurgePicks(items),
                DespawnOnExit(AppState::Landing),
            ));
        }
    }
}

/// Empty trash…: asks before deleting everything in the trash permanently (nothing when it is
/// empty).
pub fn open_empty_trash_dialog(commands: &mut Commands, theme: &Theme, lib: &Library) {
    let (docs, folders) = lib.trash();
    let what = match docs.len() + folders.len() {
        0 => return,
        1 => "the item".to_string(),
        n => format!("all {n} items"),
    };
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    commands.spawn((
        Dialog::new("empty-trash-dialog")
            .title("Empty trash")
            .width(420.0)
            .body(move |b| {
                b.spawn((
                    Text::new(format!("Permanently delete {what} in the trash?")),
                    tb.font(tb.font_base, FontWeight::NORMAL),
                    TextColor(tb.foreground),
                ));
                b.spawn(tb.text(
                    "You cannot restore them.",
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
            })
            .footer(move |f| dialog_buttons(f, &tf, "empty-trash-dialog", "Empty trash"))
            .build(&t),
        LandingDialog::EmptyTrash,
        DespawnOnExit(AppState::Landing),
    ));
}

/// OK in a folder dialog, or one deleting several items permanently.
pub fn confirm(world: &mut World, dialog: LandingDialog) {
    let store = world.resource::<DocumentStore>().0.clone();
    match dialog {
        LandingDialog::RenameFolder(id) => {
            let name = field_value(world, "rename-folder-name-field");
            if name.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            let text = format!("Renamed to \u{201c}{}\u{201d}", name.trim());
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &RenameFolder { id, name });
            undo_toast(world, text);
        }
        LandingDialog::CopyFolder(id) => {
            let name = field_value(world, "copy-folder-name-field");
            if name.trim().is_empty() {
                return;
            }
            close_dialogs(world);
            copy_items(world, &[], &[id], Some(name.trim().to_string()));
        }
        LandingDialog::PurgeFolder(id) => {
            close_dialogs(world);
            let lib = world.resource::<DocLibrary>().lib.clone();
            let name = lib.folder(id).map(|f| f.name.clone()).unwrap_or_default();
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &PurgeFolder { id });
            undo_toast(world, format!("Deleted \u{201c}{name}\u{201d} permanently"));
        }
        LandingDialog::PurgeItems => {
            let mut q = world.query::<&PurgePicks>();
            let Some(items) = q.iter(world).next().map(|p| p.0.clone()) else {
                return;
            };
            close_dialogs(world);
            purge_items(world, &items);
        }
        LandingDialog::EmptyTrash => {
            close_dialogs(world);
            let (docs, folders) = world.resource::<DocLibrary>().lib.trash();
            if docs.is_empty() && folders.is_empty() {
                return;
            }
            world
                .resource_mut::<DocLibrary>()
                .execute(&store, &PurgeItems { docs, folders });
            undo_toast(world, "Emptied the trash".into());
        }
        _ => {}
    }
}

// ---------------------------------------------------------------------------------------------
// Dragging

fn on_drag_start(
    ev: On<PointerDragStart>,
    q: Query<&PickRow>,
    details_state: Res<LandingDetails>,
    state: Res<LandingState>,
    mut drag: ResMut<LandingDrag>,
) {
    if ev.button != PointerButton::Primary || state.filter == Filter::Trash {
        return;
    }
    let Ok(row) = q.get(ev.entity) else { return };
    if matches!(row.0, Pick::Sample(_)) {
        return;
    }
    let items = if details_state.selection.contains(&row.0) {
        details_state
            .selection
            .iter()
            .copied()
            .filter(|p| !matches!(p, Pick::Sample(_)))
            .collect()
    } else {
        vec![row.0]
    };
    *drag = LandingDrag {
        items,
        start: ev.pointer.position,
        pointer: ev.pointer.position,
        moved: false,
    };
}

fn on_drag(ev: On<PointerDrag>, q: Query<(), With<PickRow>>, mut drag: ResMut<LandingDrag>) {
    if q.contains(ev.entity) && !drag.items.is_empty() {
        drag.pointer = ev.pointer.position;
        if drag.pointer.distance(drag.start) > 5.0 {
            drag.moved = true;
        }
    }
}

fn on_drag_end(
    ev: On<PointerDragEnd>,
    q: Query<(), With<PickRow>>,
    q_hover: Query<Entity, With<DropHover>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    for e in &q_hover {
        commands.entity(e).try_despawn();
    }
    commands.queue(|world: &mut World| {
        let mut drag = world.resource_mut::<LandingDrag>();
        drag.items.clear();
    });
}

/// Whether `target` can take the dragged items (not a folder being dragged).
fn takes(drag: &LandingDrag, target: Option<FolderId>) -> bool {
    drag.moved
        && !drag.items.is_empty()
        && target.is_none_or(|f| !drag.items.contains(&Pick::Folder(f)))
}

fn on_drag_enter(
    mut ev: On<PointerDragEnter>,
    q: Query<&DropFolder>,
    q_hover: Query<(Entity, &ChildOf), With<DropHover>>,
    drag: Res<LandingDrag>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(target) = q.get(ev.entity) else { return };
    ev.propagate(false);
    if !takes(&drag, target.0) || q_hover.iter().any(|(_, p)| p.parent() == ev.entity) {
        return;
    }
    for (e, _) in &q_hover {
        commands.entity(e).try_despawn();
    }
    commands.entity(ev.entity).with_child((
        Name::new("landing-drop-target"),
        DropHover,
        Pickable::IGNORE,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            border: UiRect::all(Val::Px(2.0)),
            border_radius: BorderRadius::all(Val::Px(theme.radius)),
            ..default()
        },
        BorderColor::all(theme.primary),
    ));
}

fn on_drag_leave(
    mut ev: On<PointerDragLeave>,
    q: Query<(), With<DropFolder>>,
    q_hover: Query<(Entity, &ChildOf), With<DropHover>>,
    mut commands: Commands,
) {
    if !q.contains(ev.entity) {
        return;
    }
    ev.propagate(false);
    for (e, p) in &q_hover {
        if p.parent() == ev.entity {
            commands.entity(e).try_despawn();
        }
    }
}

fn on_drop(
    mut ev: On<PointerDragDrop>,
    q: Query<&DropFolder>,
    drag: Res<LandingDrag>,
    mut commands: Commands,
) {
    let Ok(target) = q.get(ev.entity) else { return };
    ev.propagate(false);
    if !takes(&drag, target.0) {
        return;
    }
    let (items, to) = (drag.items.clone(), target.0);
    commands.queue(move |world: &mut World| move_items(world, &items, to));
}

/// The label following the pointer while dragging: the item's name, or how many.
fn place_ghost(
    drag: Res<LandingDrag>,
    lib: Res<DocLibrary>,
    theme: Res<Theme>,
    mut q: Query<(Entity, &mut Node), With<DragGhost>>,
    mut commands: Commands,
) {
    if !drag.moved || drag.items.is_empty() {
        for (e, _) in &q {
            commands.entity(e).despawn();
        }
        return;
    }
    let (x, y) = (drag.pointer.x + 14.0, drag.pointer.y + 10.0);
    if let Ok((_, mut node)) = q.single_mut() {
        node.left = Val::Px(x);
        node.top = Val::Px(y);
        return;
    }
    let t = &*theme;
    let text = match drag.items.as_slice() {
        [one] => items_name(&lib.lib, &[*one])
            .trim_matches(['\u{201c}', '\u{201d}'])
            .to_string(),
        many => format!("{} items", many.len()),
    };
    commands
        .spawn((
            Name::new("landing-drag-ghost"),
            DragGhost,
            Pickable::IGNORE,
            GlobalZIndex(1000),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(x),
                top: Val::Px(y),
                padding: UiRect::axes(Val::Px(8.0), Val::Px(4.0)),
                column_gap: Val::Px(6.0),
                align_items: AlignItems::Center,
                border_radius: BorderRadius::all(Val::Px(t.radius_lg)),
                ..default()
            },
            BackgroundColor(t.primary),
            DespawnOnExit(AppState::Landing),
        ))
        .with_children(|g| {
            let icon_name = match drag.items.as_slice() {
                [Pick::Folder(_)] => "folder",
                _ => "file",
            };
            g.spawn((
                cadrs_ui::icon(icon_name, 14.0, t.primary_foreground),
                Pickable::IGNORE,
            ));
            g.spawn((
                t.text(text, t.font_base, FontWeight::MEDIUM, t.primary_foreground),
                Pickable::IGNORE,
            ));
        });
}

/// A frame after a drop, the release's click selects again.
fn settle_drag(mut drag: ResMut<LandingDrag>) {
    if drag.items.is_empty() && drag.moved {
        drag.moved = false;
    }
}

//! The PCB Studio's dialogs:
//!
//! - **Import ECAD files** (PCB4.3, `ex2-step4-import-ecad-dialog.png`): Choose Files (a
//!   multi-select file picker for `.emn` and `.emp`, [`cadrs_ui::file_picker::open_files_picker`])
//!   with "No file chosen" / "2 files chosen", then Import / Cancel. Import pairs each `.emn` with
//!   its `.emp` ([`cadrs_core::pcb::import`]), adds one board per pair (one undo step each) and
//!   shows the last; parse errors and warnings go to a toast.
//! - **PCB Studio settings** (PCB2.2, X6, `v2-settings-select-folder-poster.png`): "Component
//!   library document" and "Create new component documents in this folder", each a read-only
//!   field with a browse button at its right end ([`cadrs_ui::PathField`]): the field shows the
//!   document's or folder's name, its tooltip where it is. The library's button opens **Select a
//!   library document** (the Default library and the stored documents, with New library document:
//!   a blank document with one PCB Studio tab, PCB2.3); the folder's opens **Select a folder**
//!   (the documents page's folders, with New folder). Then a hint, the build string, Update (one
//!   undo step; the settings are workspace-level, see [`super::sync`]) and Close.
//! - **Select custom part** (PCB11.6, X10): a Document select (the stored documents and this
//!   one), a Version select (Current, then the document's versions), the parts of its Part
//!   Studios, then ✓ OK (one undo step: the package's representation in the library) / Cancel.
//! - **Help** (?): a short local help page (there is no web help).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight, TextEdit};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::pcb::import::{chosen_label, import_files};
use cadrs_core::ElementId;
use cadrs_core::history_log::VersionId;
use cadrs_core::pcb::{
    BUILD_STRING, CustomPart, DEFAULT_LIBRARY_FILE, FolderRef, ImportBoard, LibraryRef, PartSource, PartTransform, PcbSettings, Representation, SetPcbSettings, SetRepresentation,
};
use cadrs_ui::file_picker::{FilePicked, FilesPicked, open_files_picker};
use cadrs_ui::input::TextInputField;
use cadrs_ui::{Button, Dialog, DialogClose, ListItem, Notification, PathField, PathFieldBrowse, Select, SelectChange, Theme, set_path_field_display, show_notification};

use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

/// The dialogs' state.
#[derive(Resource, Default, Debug)]
pub struct PcbDialogs {
    /// The files chosen in Import ECAD files.
    pub chosen: Vec<PathBuf>,
    /// Where the file picker opens (the folder of the last files picked).
    pub import_dir: Option<PathBuf>,
    /// The settings dialog's values, until Update.
    pub settings: Option<PcbSettings>,
    /// The open library document or folder picker.
    pub picker: Option<Picker>,
    /// The open Select custom part dialog.
    pub custom: Option<CustomPick>,
}

/// Marks the Import ECAD files dialog.
#[derive(Component)]
struct ImportDialog;

/// Marks the settings dialog.
#[derive(Component)]
struct SettingsDialog;

/// Marks the help dialog.
#[derive(Component)]
struct HelpDialog;

/// Replaces the text of the text field whose editable text is named `name`.
pub fn set_text(world: &mut World, name: &str, value: &str) {
    let mut q = world.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name) {
        t.queue_edit(TextEdit::SelectAll);
        t.queue_edit(TextEdit::Insert(value.to_string().into()));
    }
}

fn set_label(world: &mut World, name: &str, value: &str) {
    let mut q = world.query::<(&Name, &mut Text)>();
    if let Some((_, mut t)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name) {
        t.0 = value.to_string();
    }
}

fn close<T: Component>(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<T>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

fn wrapped(t: &Theme, name: &str, text: &str, italic: bool, color: Color) -> impl Bundle {
    let mut font = t.font(t.font_sm, FontWeight::NORMAL);
    if italic {
        font.style = bevy::text::FontStyle::Italic;
    }
    (Name::new(name.to_string()), Text::new(text), font, TextColor(color), Node { max_width: Val::Percent(100.0), ..default() })
}

// ---------------------------------------------------------------------------------------------
// Import ECAD files

/// Opens Import ECAD files (the toolbar's upload button).
pub fn open_import_dialog(world: &mut World) {
    let mut q = world.query_filtered::<(), With<ImportDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    world.resource_mut::<PcbDialogs>().chosen.clear();
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-import-dialog")
            .title("Import ECAD files")
            .width(430.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Center, padding: UiRect::vertical(Val::Px(4.0)), ..default() })
                    .with_children(|row| {
                        row.spawn((
                            Button::new("pcb-import-choose").label("Choose Files").build(t),
                            observe(|_: On<Activate>, mut commands: Commands| {
                                commands.queue(open_import_picker);
                            }),
                        ));
                        row.spawn((Name::new("pcb-import-chosen"), t.text("No file chosen", t.font_base, FontWeight::NORMAL, t.foreground)));
                    });
                let hint = if cfg!(feature = "kicad") {
                    "Choose an IDF board file (.emn) and its library file (.emp), or a KiCad project (.kicad_pro)."
                } else {
                    "Choose an IDF board file (.emn) and its library file (.emp)."
                };
                b.spawn(wrapped(t, "pcb-import-hint", hint, false, t.muted_foreground));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-import-ok").label("Import").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept_import);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-import-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<ImportDialog>);
                    }),
                ));
            })
            .build(&theme),
        ImportDialog,
        DespawnOnExit(AppState::Document),
    ));
}

/// Where the Choose Files picker opens: the last folder, else the working folder.
fn import_dir(world: &World) -> PathBuf {
    world.resource::<PcbDialogs>().import_dir.clone().or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."))
}

fn open_import_picker(world: &mut World) {
    let theme = world.resource::<Theme>().clone();
    let dir = import_dir(world);
    let mut commands = world.commands();
    let mut exts = vec!["emn", "emp"];
    #[cfg(feature = "kicad")]
    exts.extend(super::kicad::EXTENSIONS);
    open_files_picker(&mut commands, &theme, "pcb-import-picker", "Choose ECAD files", "pcb-import", dir, &exts);
    world.flush();
}

/// Files picked for Import ECAD files (the picker, or `pcb-choose` in a scenario).
pub fn choose_files(world: &mut World, paths: Vec<PathBuf>) {
    let label = chosen_label(&paths);
    {
        let mut d = world.resource_mut::<PcbDialogs>();
        if let Some(dir) = paths.first().and_then(|p| p.parent()) {
            d.import_dir = Some(dir.to_path_buf());
        }
        d.chosen = paths;
    }
    set_label(world, "pcb-import-chosen", &label);
}

pub fn on_files_picked(mut msgs: MessageReader<FilesPicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag == "pcb-import" {
            let paths = m.paths.clone();
            commands.queue(move |w: &mut World| choose_files(w, paths));
        }
    }
}

fn accept_import(world: &mut World) {
    let paths = world.resource::<PcbDialogs>().chosen.clone();
    if paths.is_empty() {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_notification(&mut commands, &theme, Notification::warning("Choose the board (.emn) and library (.emp) files first.").name("pcb-import-toast"));
        world.flush();
        return;
    }
    close::<ImportDialog>(world);
    import_paths(world, &paths);
}

/// Imports the files into the active PCB Studio: one board (and one undo step) per `.emn`,
/// the last one shown; a toast says what happened.
pub fn import_paths(world: &mut World, paths: &[PathBuf]) {
    // KiCad projects become native boards; the rest goes the IDF way.
    #[cfg(feature = "kicad")]
    let (kicad_names, kicad_warnings, kicad_errors) = super::kicad::import(world, paths);
    #[cfg(not(feature = "kicad"))]
    let (kicad_names, kicad_warnings, kicad_errors) = (Vec::<String>::new(), Vec::<String>::new(), Vec::<String>::new());
    let idf: Vec<PathBuf> = paths.iter().filter(|p| p.extension().is_some_and(|x| x == "emn" || x == "emp")).cloned().collect();
    // Only KiCad files picked: nothing for the IDF pairing to complain about.
    let only_kicad = idf.is_empty() && (!kicad_names.is_empty() || !kicad_errors.is_empty());
    let mut report = if only_kicad { Default::default() } else { import_files(&idf) };
    report.warnings.extend(kicad_warnings);
    let theme = world.resource::<Theme>().clone();
    let mut names = kicad_names;
    let mut errors = report.errors.clone();
    errors.extend(kicad_errors);
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        match doc.active_element().filter(|e| e.pcb().is_some()).map(|e| e.id) {
            Some(element) => {
                for b in report.boards {
                    let n = b.board.board.placements.len();
                    let cmd = ImportBoard { element, board: Box::new(b.board), source: b.source };
                    match doc.execute(&cmd) {
                        Ok(()) => {
                            let name = doc.active_element().and_then(|e| e.pcb()).and_then(|s| s.active_board()).map(|b| b.name().to_string()).unwrap_or_default();
                            names.push(format!("{name} ({n} components)"));
                        }
                        Err(e) => errors.push(e.to_string()),
                    }
                }
            }
            None => errors.push("Open a PCB Studio tab to import into".into()),
        }
    }
    let mut commands = world.commands();
    let n = if !errors.is_empty() {
        let mut msg = format!("Couldn't import: {}", errors.join("; "));
        if !names.is_empty() {
            msg = format!("Imported {}. {msg}", names.join(", "));
        }
        Notification::warning(msg).max_width(640.0)
    } else if !report.warnings.is_empty() {
        let more = if report.warnings.len() > 1 { format!(" (and {} more)", report.warnings.len() - 1) } else { String::new() };
        Notification::warning(format!("Imported {}. {}{more}", names.join(", "), report.warnings[0])).max_width(640.0)
    } else {
        Notification::info(format!("Imported {}", names.join(", "))).seconds(3.0)
    };
    show_notification(&mut commands, &theme, n.name("pcb-import-toast"));
    world.flush();
}

// ---------------------------------------------------------------------------------------------
// Settings

/// The default component library file: in the documents folder.
pub fn default_library(world: &World) -> PathBuf {
    clean_path(&world.get_resource::<DocumentStore>().map(|s| s.0.root().join(DEFAULT_LIBRARY_FILE)).unwrap_or_else(|| PathBuf::from(DEFAULT_LIBRARY_FILE)))
}

/// `p` made absolute, with `.` and `..` resolved without touching the disk (the documents
/// folder may not exist yet).
pub fn clean_path(p: &Path) -> PathBuf {
    let abs = std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf());
    let mut out = PathBuf::new();
    for c in abs.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// The library field's text and tooltip: the document's name, and where it is.
fn library_display(world: &World, s: &PcbSettings) -> (String, String) {
    match &s.library {
        Some(l) => {
            let path = world.get_resource::<DocumentStore>().map(|st| clean_path(&st.0.document_path(l.document)).display().to_string()).unwrap_or_default();
            (l.name.clone(), path)
        }
        None => (s.library_name(), default_library(world).display().to_string()),
    }
}

/// The folder field's text and tooltip ("" shows the placeholder).
fn folder_display(s: &PcbSettings) -> (String, String) {
    match &s.component_folder {
        Some(f) => (f.name.clone(), format!("Documents › {}", f.name)),
        None => (String::new(), "Select a folder".into()),
    }
}

/// Opens the settings dialog (the toolbar's gear) with the workspace settings, as the active
/// PCB Studio has them.
pub fn open_settings_dialog(world: &mut World) {
    let settings = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element().and_then(|e| e.pcb()).map(|s| s.settings.clone())).unwrap_or_default();
    let (library, library_tip) = library_display(world, &settings);
    let (folder, folder_tip) = folder_display(&settings);
    world.resource_mut::<PcbDialogs>().settings = Some(settings);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-settings-dialog")
            .title("PCB Studio settings")
            .width(470.0)
            .body(move |b| {
                let t = &tb;
                let label = |s: &'static str| t.text(s, t.font_base, FontWeight::MEDIUM, t.foreground);
                b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), width: Val::Percent(100.0), ..default() }).with_children(|c| {
                    c.spawn((Name::new("pcb-settings-library-label"), label("Component library document")));
                    c.spawn(PathField::new("pcb-settings-library").value(library).icon("file").read_only(true).value_tooltip(library_tip).tooltip("Choose the library document").build(t));
                    c.spawn(wrapped(t, "pcb-settings-library-hint", "Holds the mappings from ECAD packages to their 3D representations. Every PCB Studio in this workspace uses it.", false, t.muted_foreground));
                    c.spawn((Name::new("pcb-settings-folder-label"), label("Create new component documents in this folder"), Node { margin: UiRect::top(Val::Px(10.0)), ..default() }));
                    c.spawn(PathField::new("pcb-settings-folder").value(folder).placeholder("Select a folder…").read_only(true).value_tooltip(folder_tip).tooltip("Select a folder").build(t));
                    c.spawn(wrapped(
                        t,
                        "pcb-settings-folder-hint",
                        "PCB Studio creates a document in this folder for each ECAD component it models. Everyone who uses this PCB Studio needs write access to it.",
                        true,
                        t.muted_foreground,
                    ));
                    c.spawn((
                        Name::new("pcb-settings-build"),
                        t.text(BUILD_STRING, t.font_sm, FontWeight::NORMAL, t.subtle_foreground),
                        Node { align_self: AlignSelf::Center, margin: UiRect::top(Val::Px(14.0)), ..default() },
                    ));
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-settings-update").label("Update").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(update_settings);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-settings-close").label("Close").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<SettingsDialog>);
                    }),
                ));
            })
            .build(&theme),
        SettingsDialog,
        DespawnOnExit(AppState::Document),
    ));
}

/// Update: the chosen settings into the studio (one undo step), then close. [`super::sync`]
/// writes them to the workspace, so every PCB Studio sees them.
fn update_settings(world: &mut World) {
    let settings = world.resource_mut::<PcbDialogs>().settings.take();
    if let Some(settings) = settings
        && let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Some((element, current)) = doc.active_element().and_then(|e| Some((e.id, e.pcb()?.settings.clone())))
        && current != settings
        && let Err(e) = doc.execute(&SetPcbSettings { element, settings })
    {
        warn!("PCB settings: {e}");
    }
    close::<SettingsDialog>(world);
}

/// A path field's button: the library document picker or the folder picker.
pub fn on_path_browse(ev: On<PathFieldBrowse>, mut commands: Commands) {
    let kind = match ev.name.as_str() {
        "pcb-settings-library" => PickerKind::Library,
        "pcb-settings-folder" => PickerKind::Folder,
        _ => return,
    };
    commands.queue(move |w: &mut World| open_picker(w, kind, None));
}

/// Kept for P3H.3 callers: folders picked on disk are no longer used by the settings.
pub fn on_folder_picked(mut msgs: MessageReader<FilePicked>) {
    msgs.clear();
}

// ---------------------------------------------------------------------------------------------
// Library document and folder pickers (PCB2.2, `v2-settings-select-folder-poster.png`)

/// Which picker is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickerKind {
    /// Select a library document: "Default library" and the stored documents.
    Library,
    /// Select a folder: the documents page's folders.
    Folder,
}

impl PickerKind {
    fn prefix(self) -> &'static str {
        match self {
            PickerKind::Library => "pcb-library-picker",
            PickerKind::Folder => "pcb-folder-picker",
        }
    }
}

/// An open picker: its rows (key, label) and the chosen one.
#[derive(Clone, Debug)]
pub struct Picker {
    pub kind: PickerKind,
    pub rows: Vec<(String, String)>,
    pub selected: Option<String>,
}

#[derive(Component)]
struct PickerDialog;

/// A picker row's key.
#[derive(Component, Clone)]
struct PickerRow(String);

/// The rows of a picker from the store.
fn picker_rows(world: &World, kind: PickerKind) -> Vec<(String, String)> {
    let Some(store) = world.get_resource::<DocumentStore>() else { return vec![] };
    let (lib, _) = store.0.list();
    match kind {
        PickerKind::Library => {
            let mut docs: Vec<(String, String)> = lib.entries.iter().filter(|e| e.meta.trashed.is_none()).map(|e| (e.id.to_string(), e.name.clone())).collect();
            docs.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.1, &b.1));
            let mut rows = vec![("default".to_string(), "Default library".to_string())];
            rows.extend(docs);
            rows
        }
        PickerKind::Folder => {
            let mut f: Vec<(String, String)> = lib.folders.iter().map(|f| (f.id.to_string(), f.name.clone())).collect();
            f.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.1, &b.1));
            f
        }
    }
}

/// The key of the current setting, to preselect it.
fn current_key(world: &World, kind: PickerKind) -> Option<String> {
    let s = world.resource::<PcbDialogs>().settings.clone()?;
    match kind {
        PickerKind::Library => Some(s.library.map(|l| l.document.to_string()).unwrap_or_else(|| "default".into())),
        PickerKind::Folder => s.component_folder.map(|f| f.id.to_string()),
    }
}

/// Opens (or reopens, after New) a picker, with `select` chosen (else the current setting).
pub fn open_picker(world: &mut World, kind: PickerKind, select: Option<String>) {
    close::<PickerDialog>(world);
    let rows = picker_rows(world, kind);
    let selected = select.or_else(|| current_key(world, kind)).filter(|k| rows.iter().any(|r| r.0 == *k));
    world.resource_mut::<PcbDialogs>().picker = Some(Picker { kind, rows: rows.clone(), selected: selected.clone() });
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let prefix = kind.prefix();
    let (title, root, icon, new_label, empty) = match kind {
        PickerKind::Library => ("Select a library document", "My documents", "file", "New library document", "No documents yet."),
        PickerKind::Folder => ("Select a folder", "My documents", "folder", "New folder", "No folders yet: New folder makes one."),
    };
    world.spawn((
        Dialog::new(prefix)
            .title(title)
            .width(420.0)
            .body(move |b| {
                let t = &tb;
                b.spawn((
                    Name::new(format!("{prefix}-list")),
                    Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), max_height: Val::Px(300.0), overflow: Overflow::scroll_y(), ..default() },
                ))
                .with_children(|l| {
                    l.spawn(ListItem::new(format!("{prefix}-root")).icon("user").label(root).height(28.0).build(t)).insert(Pickable::IGNORE);
                    if rows.is_empty() {
                        l.spawn(wrapped(t, &format!("{prefix}-empty"), empty, true, t.muted_foreground));
                    }
                    for (key, label) in &rows {
                        let row_icon = if key == "default" { "books" } else { icon };
                        l.spawn((
                            ListItem::new(format!("{prefix}-row-{}", super::slug(label))).icon(row_icon).label(label.clone()).padding_left(28.0).height(28.0).selected(selected.as_deref() == Some(key.as_str())).build(t),
                            PickerRow(key.clone()),
                            observe(|a: On<Activate>, q: Query<&PickerRow>, mut d: ResMut<PcbDialogs>, q_rows: Query<(Entity, &PickerRow)>, mut commands: Commands| {
                                let Ok(r) = q.get(a.entity) else { return };
                                if let Some(p) = d.picker.as_mut() {
                                    p.selected = Some(r.0.clone());
                                }
                                for (e, x) in &q_rows {
                                    if x.0 == r.0 {
                                        commands.entity(e).try_insert(cadrs_ui::Selected);
                                    } else {
                                        commands.entity(e).try_remove::<cadrs_ui::Selected>();
                                    }
                                }
                            }),
                        ));
                    }
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new(format!("{prefix}-new")).icon(if kind == PickerKind::Folder { "folder-new" } else { "file-new" }).label(new_label).ghost().build(t),
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        commands.queue(move |w: &mut World| picker_new(w, kind));
                    }),
                ));
                f.spawn(Node { flex_grow: 1.0, ..default() });
                f.spawn((
                    Button::new(format!("{prefix}-select")).label("Select").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(picker_select);
                    }),
                ));
                f.spawn((
                    Button::new(format!("{prefix}-cancel")).label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            w.resource_mut::<PcbDialogs>().picker = None;
                            close::<PickerDialog>(w);
                        });
                    }),
                ));
            })
            .build(&theme),
        PickerDialog,
        DespawnOnExit(AppState::Document),
    ));
}

/// A name not used yet among `taken`: `base`, else "base (2)", "base (3)", ...
fn free_name(base: &str, taken: &[String]) -> String {
    if !taken.iter().any(|t| t == base) {
        return base.to_string();
    }
    (2..).map(|n| format!("{base} ({n})")).find(|c| !taken.contains(c)).unwrap()
}

/// Adds a folder to the documents page (as its New folder does) and returns it.
pub fn create_folder(world: &mut World, name: &str) -> Option<cadrs_core::FolderEntry> {
    let store = world.get_resource::<DocumentStore>()?.0.clone();
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let (before, _) = store.list();
    let taken: Vec<String> = before.folders.iter().map(|f| f.name.clone()).collect();
    let entry = cadrs_core::FolderEntry { id: cadrs_core::FolderId::new(), name: free_name(name, &taken), created: now, owned_by: user };
    let mut after = before.clone();
    after.folders.push(entry.clone());
    match store.sync(&before, &after) {
        Ok(()) => Some(entry),
        Err(e) => {
            warn!("new folder: {e}");
            None
        }
    }
}

/// New library document / New folder: made in the store, then chosen.
fn picker_new(world: &mut World, kind: PickerKind) {
    let key = match kind {
        PickerKind::Folder => create_folder(world, "PCB Components").map(|f| f.id.to_string()),
        PickerKind::Library => {
            let Some(store) = world.get_resource::<DocumentStore>().map(|s| s.0.clone()) else { return };
            let now = world.resource::<AppClock>().now();
            let user = world.resource::<UserProfile>().id.clone();
            let taken: Vec<String> = store.list().0.entries.iter().map(|e| e.name.clone()).collect();
            let name = free_name(cadrs_core::pcb::library::LIBRARY_DOCUMENT_NAME, &taken);
            match cadrs_core::pcb::library::create_library_document(&store, &name, &user, now) {
                Ok(e) => {
                    let _ = store.write_thumbnail(e.id, &cadrs_core::thumbnail::placeholder(cadrs_core::store::thumbnail_seed(&e.name, now)));
                    Some(e.id.to_string())
                }
                Err(e) => {
                    warn!("new library document: {e}");
                    None
                }
            }
        }
    };
    open_picker(world, kind, key);
}

/// Select: the chosen row into the settings dialog's field.
fn picker_select(world: &mut World) {
    let Some(p) = world.resource_mut::<PcbDialogs>().picker.take() else { return };
    close::<PickerDialog>(world);
    let Some(key) = p.selected.clone() else { return };
    let label = p.rows.iter().find(|r| r.0 == key).map(|r| r.1.clone()).unwrap_or_default();
    let Some(mut s) = world.resource::<PcbDialogs>().settings.clone() else { return };
    let uuid = |k: &str| uuid::Uuid::parse_str(k).ok();
    match p.kind {
        PickerKind::Library => {
            s.library = uuid(&key).map(|u| LibraryRef { document: cadrs_core::DocumentId(u), name: label });
            let (text, tip) = library_display(world, &s);
            set_path_field_display(world, "pcb-settings-library", &text, &tip);
        }
        PickerKind::Folder => {
            s.component_folder = uuid(&key).map(|u| FolderRef { id: cadrs_core::FolderId(u), name: label });
            let (text, tip) = folder_display(&s);
            set_path_field_display(world, "pcb-settings-folder", &text, &tip);
        }
    }
    world.resource_mut::<PcbDialogs>().settings = Some(s);
}

// ---------------------------------------------------------------------------------------------
// Select custom part (PCB11.6, X10)

/// The Select custom part dialog's state.
#[derive(Clone, Debug)]
pub struct CustomPick {
    pub element: ElementId,
    pub package: String,
    /// The documents to pick from: (id, name).
    pub documents: Vec<(cadrs_core::DocumentId, String)>,
    pub document: usize,
    /// The chosen document's versions: (None = as it is now, name).
    pub versions: Vec<(Option<VersionId>, String)>,
    pub version: usize,
    /// Its Part Studios' parts: (tab, tab name, part, part name).
    pub parts: Vec<(ElementId, String, cadrs_core::PartId, String)>,
    pub part: Option<usize>,
}

#[derive(Component)]
struct CustomPartDialog;

#[derive(Component, Clone, Copy)]
struct CustomPartRow(usize);

/// The versions and parts of the chosen document.
fn load_pick(world: &World, pick: &mut CustomPick) {
    let Some((id, _)) = pick.documents.get(pick.document).cloned() else { return };
    let store = world.get_resource::<DocumentStore>();
    let log = store.and_then(|s| cadrs_core::history_log::HistoryLog::load(&s.0, id).ok().flatten());
    let mut versions = vec![(None, "Current (latest changes)".to_string())];
    if let Some(l) = &log {
        versions.extend(l.versions().iter().rev().map(|v| (Some(v.id()), v.name().to_string())));
    }
    if pick.versions.len() != versions.len() || pick.versions.iter().zip(&versions).any(|(a, b)| a.0 != b.0) {
        pick.version = 0;
    }
    pick.versions = versions;
    let version = pick.versions.get(pick.version).and_then(|v| v.0);
    pick.parts.clear();
    pick.part = None;
    if let Some(doc) = super::view::source_document(world, id, version) {
        for el in doc.elements.iter().filter(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. })) {
            for (part, name, _, _) in super::view::studio_parts(&doc, el.id) {
                pick.parts.push((el.id, el.name.clone(), part, name));
            }
        }
    }
    if pick.parts.len() == 1 {
        pick.part = Some(0);
    }
}

/// Opens Select custom part for `package` (the Component pane's Custom part).
pub fn open_custom_part_dialog(world: &mut World, element: ElementId, package: &str) {
    let open = world.get_resource::<ActiveDocument>().map(|d| (d.doc.id, d.doc.name.clone()));
    let mut documents: Vec<(cadrs_core::DocumentId, String)> = world
        .get_resource::<DocumentStore>()
        .map(|s| s.0.list().0.entries.iter().filter(|e| e.meta.trashed.is_none() && Some(e.id) != open.as_ref().map(|o| o.0)).map(|e| (e.id, e.name.clone())).collect())
        .unwrap_or_default();
    documents.sort_by(|a, b| cadrs_core::library::natural_cmp(&a.1, &b.1));
    if let Some((id, name)) = open {
        documents.push((id, format!("{name} (this document)")));
    }
    // The document of the current custom part, if there is one.
    let current = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.doc.element(element))
        .and_then(|e| e.pcb())
        .and_then(|s| s.library.get(package).custom().map(|c| c.source.document));
    let document = current.and_then(|c| documents.iter().position(|d| d.0 == c)).unwrap_or(0);
    let mut pick = CustomPick { element, package: package.to_string(), documents, document, versions: vec![], version: 0, parts: vec![], part: None };
    load_pick(world, &mut pick);
    world.resource_mut::<PcbDialogs>().custom = Some(pick);
    spawn_custom_dialog(world);
}

fn spawn_custom_dialog(world: &mut World) {
    close::<CustomPartDialog>(world);
    let Some(pick) = world.resource::<PcbDialogs>().custom.clone() else { return };
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-custom-part-dialog")
            .title("Select custom part")
            .width(460.0)
            .body(move |b| {
                let t = &tb;
                let label = |s: &str| t.text(s.to_string(), t.font_sm, FontWeight::MEDIUM, t.muted_foreground);
                b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(6.0), width: Val::Percent(100.0), ..default() }).with_children(|c| {
                    c.spawn(wrapped(t, "pcb-custom-part-for", &format!("The 3D representation of {} in every PCB Studio that uses this library.", pick.package), false, t.foreground));
                    c.spawn(label("Document"));
                    let mut sel = Select::new("pcb-custom-document").bordered().width(Val::Percent(100.0));
                    for (_, name) in &pick.documents {
                        sel = sel.option(name.clone(), true);
                    }
                    c.spawn(sel.selected(pick.document).build(t));
                    c.spawn(label("Version"));
                    let mut sel = Select::new("pcb-custom-version").bordered().width(Val::Percent(100.0));
                    for (_, name) in &pick.versions {
                        sel = sel.option(name.clone(), true);
                    }
                    c.spawn(sel.selected(pick.version).build(t));
                    c.spawn(label("Part"));
                    c.spawn((
                        Name::new("pcb-custom-parts"),
                        Node {
                            flex_direction: FlexDirection::Column,
                            width: Val::Percent(100.0),
                            max_height: Val::Px(220.0),
                            min_height: Val::Px(60.0),
                            overflow: Overflow::scroll_y(),
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(t.radius)),
                            ..default()
                        },
                        BorderColor::all(t.input_border),
                    ))
                    .with_children(|l| {
                        if pick.parts.is_empty() {
                            l.spawn(wrapped(t, "pcb-custom-no-parts", "This document has no parts.", true, t.muted_foreground));
                        }
                        for (i, (_, studio, _, part)) in pick.parts.iter().enumerate() {
                            l.spawn((
                                ListItem::new(format!("pcb-custom-part-row-{i}")).icon("part").label(part.clone()).detail(studio.clone()).height(28.0).selected(pick.part == Some(i)).build(t),
                                CustomPartRow(i),
                                observe(|a: On<Activate>, q: Query<&CustomPartRow>, q_rows: Query<(Entity, &CustomPartRow)>, mut d: ResMut<PcbDialogs>, mut commands: Commands| {
                                    let Ok(r) = q.get(a.entity) else { return };
                                    if let Some(p) = d.custom.as_mut() {
                                        p.part = Some(r.0);
                                    }
                                    for (e, x) in &q_rows {
                                        if x.0 == r.0 {
                                            commands.entity(e).try_insert(cadrs_ui::Selected);
                                        } else {
                                            commands.entity(e).try_remove::<cadrs_ui::Selected>();
                                        }
                                    }
                                }),
                            ));
                        }
                    });
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-custom-ok").icon("check").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept_custom_part);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-custom-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(cancel_custom_part);
                    }),
                ));
            })
            .build(&theme),
        CustomPartDialog,
        DespawnOnExit(AppState::Document),
    ));
}

/// Another document or version: its parts.
fn on_custom_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let (doc, index) = match name.as_str() {
        "pcb-custom-document" => (true, ev.index),
        "pcb-custom-version" => (false, ev.index),
        _ => return,
    };
    commands.queue(move |w: &mut World| {
        let Some(mut pick) = w.resource::<PcbDialogs>().custom.clone() else { return };
        if doc {
            pick.document = index;
            pick.version = 0;
            pick.versions.clear();
        } else {
            pick.version = index;
        }
        load_pick(w, &mut pick);
        w.resource_mut::<PcbDialogs>().custom = Some(pick);
        spawn_custom_dialog(w);
    });
}

/// ✓: the part becomes the package's representation (one undo step), placed as modelled; the
/// Component pane then offers Translate, Rotate and Center.
fn accept_custom_part(world: &mut World) {
    let Some(pick) = world.resource::<PcbDialogs>().custom.clone() else { return };
    let (Some(i), Some((doc_id, doc_name))) = (pick.part, pick.documents.get(pick.document).cloned()) else {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        show_notification(&mut commands, &theme, Notification::warning("Choose a part first.").name("pcb-custom-toast"));
        world.flush();
        return;
    };
    let (el, el_name, part, part_name) = pick.parts[i].clone();
    let (version, version_name) = pick.versions.get(pick.version).cloned().unwrap_or((None, String::new()));
    let doc_name = doc_name.trim_end_matches(" (this document)").to_string();
    let source = PartSource { document: doc_id, document_name: doc_name, element: el, element_name: el_name, part, part_name, version, version_name: version.map(|_| version_name) };
    let rep = Representation::Custom(Box::new(CustomPart { source, transform: PartTransform::default() }));
    let cmd = SetRepresentation { element: pick.element, package: pick.package.clone(), representation: rep, label: format!("Custom part for {}", pick.package) };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = doc.execute(&cmd)
    {
        warn!("custom part: {e}");
    }
    world.resource_mut::<PcbDialogs>().custom = None;
    world.resource_mut::<super::PcbUi>().edit = None;
    close::<CustomPartDialog>(world);
}

/// Cancel: nothing changes (the pane's Representation goes back).
fn cancel_custom_part(world: &mut World) {
    world.resource_mut::<PcbDialogs>().custom = None;
    close::<CustomPartDialog>(world);
    super::panes::refresh(world);
}

/// Registers the dialogs' observers.
pub fn register(app: &mut App) {
    app.add_observer(on_custom_select);
}

/// `pcb-sample-part-document` (scenarios): stores [`cadrs_pcb::sample::custom_part_document`]
/// with a version "V1".
pub fn store_sample_part_document(world: &mut World) {
    let Some(store) = world.get_resource::<DocumentStore>().map(|s| s.0.clone()) else { return };
    let now = world.resource::<AppClock>().now();
    let user = world.resource::<UserProfile>().id.clone();
    let (doc, _, _) = match cadrs_pcb::sample::custom_part_document() {
        Ok(d) => d,
        Err(e) => {
            warn!("sample part document: {e}");
            return;
        }
    };
    if let Err(e) = store.create(&doc, &cadrs_core::DocumentMeta::new(&user, now)) {
        warn!("sample part document: {e}");
        return;
    }
    let _ = store.write_thumbnail(doc.id, &cadrs_core::thumbnail::placeholder(cadrs_core::store::thumbnail_seed(&doc.name, now)));
    let mut log = cadrs_core::history_log::HistoryLog::start(&doc, now, &user);
    log.create_version("V1", "Heatsink model for QFP100", now, &user);
    if let Err(e) = log.save(&store) {
        warn!("sample part document history: {e}");
    }
}
// ---------------------------------------------------------------------------------------------
// Help

const HELP: &[(&str, &str)] = &[
    ("Boards", "A PCB Studio holds any number of boards. + creates a new board (a 100 × 80 mm outline) and lets you type its name; double-click a board to rename it. Click a board under Boards to show it; the board shown is bold and blue. Right-click a board to delete it (Undo brings it back)."),
    ("Import ECAD files", "Click the upload button, choose a board's .emn file and its .emp library (Ctrl+click picks both), then Import. Each import adds a board."),
    ("Viewport", "The board is green and components are coloured by package. Orbit with the right mouse button, pan with the middle button, zoom with the wheel; F fits the board. Click a component to select it."),
    ("Components", "+ creates a component and lets you type its name; double-click it to rename it. Click a board under Components to list its packages; click a package for its component view (the part alone on a grid). Click the board again to go back."),
    ("Component properties", "The right edge's first button shows the selected component's part name, part number and representation: None, From ECAD data (a box from the outline and height) or Custom part (a part from a Part Studio, moved with Translate, Center and Rotate, then Accept). The choice is kept in the component library, so every PCB Studio follows."),
    ("Bill of materials", "The right edge's second button lists the board's components grouped by part number. Click a row to select its components; double-click a designator to rename it."),
    ("Search", "Type in Search and press Enter (or the magnifier): matching designators, packages, part numbers and boards light up. The arrows step through them; the cross clears."),
    ("Settings", "The gear sets the component library document and the folder for component documents, for every PCB Studio in this workspace."),
];

/// Opens the help dialog (the toolbar's ?).
pub fn open_help_dialog(world: &mut World) {
    let mut q = world.query_filtered::<(), With<HelpDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-help-dialog")
            .title("PCB Studio help")
            .width(480.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(8.0), width: Val::Percent(100.0), ..default() }).with_children(|c| {
                    for (i, (head, text)) in HELP.iter().enumerate() {
                        c.spawn((Name::new(format!("pcb-help-{i}-title")), t.text(*head, t.font_base, FontWeight::BOLD, t.foreground)));
                        c.spawn(wrapped(t, &format!("pcb-help-{i}"), text, false, t.foreground));
                    }
                });
            })
            .footer(move |f| {
                f.spawn((
                    Button::new("pcb-help-close").label("Close").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<HelpDialog>);
                    }),
                ));
            })
            .build(&theme),
        HelpDialog,
        DespawnOnExit(AppState::Document),
    ));
}

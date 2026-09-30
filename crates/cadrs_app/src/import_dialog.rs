//! The **Import** feature's dialog (Onshape import, [`cadrs_core::import`]): parts from a STEP,
//! IGES or STL file stored with the document. (Importing a file as new tabs or a new document,
//! with the choice of its assembly structure, is [`crate::import_file`].)
//!
//! - The toolbar's Import button opens the file picker (STEP, IGES and STL files); the file picked
//!   is stored with the document and inserted as "Import N" ([`cadrs_core::import::AddImport`]),
//!   and its dialog opens. Double-clicking the feature in the list edits it.
//! - The dialog shows the file, **Choose file** to replace it, **Y axis is up**, and for STL
//!   files **Specify units** with the unit (unitless STL numbers are read as mm otherwise).
//! - ✓ or Enter accepts (one undo step, "Insert Import 1"), ✕ or Esc removes a new Import or
//!   reverts an edit. Every change is a command.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::commands::{ReplaceFeature, SetFeature};
use cadrs_core::import::{AddImport, ImportFeature, ImportFormat, ImportUnit};
use cadrs_core::{ElementId, ElementKind, Feature, FeatureId, FeatureKind};
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, FeatureDialogAccept, FeatureDialogCancel, FeatureDialogState, FilePicked, OptionRow, Select, SelectChange};

use crate::parts::PartCache;
use crate::viewport::ViewportArea;
use crate::{ActiveDocument, AppState};

/// The file picker's tag.
const PICK_TAG: &str = "import-feature";

pub struct ImportDialogPlugin;

impl Plugin for ImportDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (on_file_picked, import_keys, sync_import_dialog)
                .chain()
                .before(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_systems(OnExit(AppState::Document), |world: &mut World| {
            if world.contains_resource::<ImportSession>() {
                finish(world);
            }
        })
        .add_observer(on_accept)
        .add_observer(on_cancel)
        .add_observer(on_option)
        .add_observer(on_unit)
        .add_observer(on_choose);
    }
}

/// The Import feature whose dialog is open.
#[derive(Resource, Debug, Clone)]
pub struct ImportSession {
    pub element: ElementId,
    pub feature: FeatureId,
    pub is_new: bool,
    pub mark: usize,
    pub before: Option<Feature>,
}

#[derive(Component)]
struct ImportDialog;

/// What the dialog was built for (it is rebuilt when it changes).
#[derive(Component, Debug, Clone, PartialEq)]
struct BuiltFor(String, ImportFormat, bool, Option<ImportUnit>, usize);

#[derive(Component)]
struct ChooseFile;

/// Closes the other feature dialogs and the sketch before a linked feature's dialog opens.
pub fn close_other_sessions(world: &mut World) {
    if world.contains_resource::<crate::applied::AppliedSession>() {
        crate::applied::finish(world);
    }
    if world.contains_resource::<crate::boolean::BooleanSession>() {
        crate::boolean::finish(world);
    }
    if world.contains_resource::<crate::extrude::ExtrudeSession>() {
        crate::extrude::finish_session(world);
    }
    if world.contains_resource::<crate::sketch::SketchSession>() {
        crate::sketch::finish_session(world);
    }
}

/// Where the picker starts: `$CADRS_IMPORT_DIR`, else the working directory.
pub(crate) fn start_dir() -> std::path::PathBuf {
    std::env::var_os("CADRS_IMPORT_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// The toolbar's Import button: the file picker; the Import is inserted when a file is picked.
pub fn begin_import(world: &mut World) {
    if world.contains_resource::<ImportSession>() {
        finish(world);
    }
    if world.contains_resource::<crate::derived_ui::DerivedSession>() {
        crate::derived_ui::finish(world);
    }
    close_other_sessions(world);
    open_picker(world);
}

fn open_picker(world: &mut World) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::open_file_picker(&mut commands, &theme, "import-picker", "Import", PICK_TAG, start_dir(), &["step", "stp", "iges", "igs", "stl"]);
    world.flush();
}

/// Opens an existing Import for editing.
pub fn edit_import(world: &mut World, feature: FeatureId) {
    if world.get_resource::<ImportSession>().is_some_and(|s| s.feature == feature) {
        return;
    }
    if world.contains_resource::<ImportSession>() {
        finish(world);
    }
    if world.contains_resource::<crate::derived_ui::DerivedSession>() {
        crate::derived_ui::finish(world);
    }
    close_other_sessions(world);
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else {
        return;
    };
    let Some(el) = doc.active_element() else { return };
    let element = el.id;
    let Some(before) = el.feature(feature).filter(|f| matches!(f.kind, FeatureKind::Import(_))).cloned() else {
        return;
    };
    let mark = doc.history.undo_len();
    doc.discard_since(mark);
    world.insert_resource(ImportSession { element, feature, is_new: false, mark, before: Some(before) });
}

fn end(world: &mut World) {
    world.remove_resource::<ImportSession>();
}

fn params(world: &World) -> Option<ImportFeature> {
    let s = world.get_resource::<ImportSession>()?;
    match &world.get_resource::<ActiveDocument>()?.doc.element(s.element)?.feature(s.feature)?.kind {
        FeatureKind::Import(x) => Some(x.clone()),
        _ => None,
    }
}

fn set(world: &mut World, x: ImportFeature, label: &str) {
    let Some(s) = world.get_resource::<ImportSession>().cloned() else { return };
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    if let Err(e) = doc.execute(&SetFeature { element: s.element, feature: s.feature, kind: FeatureKind::Import(x), label: label.into() }) {
        warn!("cannot change the import: {e}");
    }
}

/// A file picked: a new Import (or, while the dialog is open, its new file).
fn on_file_picked(mut msgs: MessageReader<FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != PICK_TAG {
            continue;
        }
        let path = m.path.clone();
        commands.queue(move |world: &mut World| pick_file(world, &path));
    }
}

/// Stores the file at `path` and inserts an Import of it (or gives the open Import that file).
pub fn pick_file(world: &mut World, path: &std::path::Path) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("file").to_string();
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            warn!("cannot import {}: {e}", path.display());
            return;
        }
    };
    if let Some(old) = params(world) {
        match ImportFeature::from_file(&name, bytes) {
            Ok(x) => set(world, ImportFeature { y_axis_up: old.y_axis_up, units: old.units, flatten: old.flatten, structure: old.structure, ..x }, "Choose file"),
            Err(e) => warn!("{e}"),
        }
        return;
    }
    let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() else { return };
    let Some(element) = doc.active_element().filter(|e| matches!(e.kind, ElementKind::PartStudio { .. })).map(|e| e.id) else {
        return;
    };
    let feature = FeatureId::new();
    let mark = doc.history.undo_len();
    let cmd = AddImport { element, feature, file_name: name, bytes: std::sync::Arc::new(bytes), y_axis_up: false, units: None };
    if let Err(e) = doc.execute(&cmd) {
        warn!("cannot import: {e}");
        return;
    }
    world.insert_resource(ImportSession { element, feature, is_new: true, mark, before: None });
}

/// ✓ / Enter: keeps the Import if it rebuilds.
pub fn accept(world: &mut World) {
    let Some(s) = world.get_resource::<ImportSession>().cloned() else { return };
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some(el) = doc.doc.element(s.element) else { return };
    let Some(f) = el.feature(s.feature).filter(|f| f.is_valid()).cloned() else { return };
    if cadrs_core::rebuild::build(&el.active_features()).error(f.id).is_some() {
        return;
    }
    let label = if s.is_new { format!("Insert {}", f.name) } else { format!("Edit {}", f.name) };
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        doc.squash_element_since(s.mark, s.element, label);
    }
    end(world);
}

/// ✕ / Esc: removes a new Import or reverts an edit.
pub fn cancel(world: &mut World) {
    let Some(s) = world.get_resource::<ImportSession>().cloned() else { return };
    let cur = world.get_resource::<ActiveDocument>().and_then(|d| d.doc.element(s.element)?.feature(s.feature).cloned());
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let name = cur.as_ref().map(|f| f.name.clone()).unwrap_or_default();
        if s.is_new {
            doc.discard_element_since(s.mark, s.element);
        } else {
            doc.squash_element_since(s.mark, s.element, format!("Edit {name}"));
            if let (Some(before), Some(f)) = (s.before.clone(), cur)
                && f != before
            {
                let _ = doc.execute(&ReplaceFeature { element: s.element, feature: before, label: format!("Cancel {name}") });
            }
        }
    }
    end(world);
}

/// Accepts if valid, otherwise cancels.
pub fn finish(world: &mut World) {
    accept(world);
    if world.contains_resource::<ImportSession>() {
        cancel(world);
    }
}

fn import_keys(
    mut keys: MessageReader<KeyboardInput>,
    session: Option<Res<ImportSession>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut commands: Commands,
) {
    if session.is_none() {
        keys.clear();
        return;
    }
    for k in keys.read() {
        if k.state != ButtonState::Pressed || !q_dialogs.is_empty() {
            continue;
        }
        match k.key_code {
            KeyCode::Enter | KeyCode::NumpadEnter => commands.queue(accept),
            KeyCode::Escape => commands.queue(cancel),
            _ => {}
        }
    }
}

fn on_accept(ev: On<FeatureDialogAccept>, q: Query<(), With<ImportDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(accept);
    }
}

fn on_cancel(ev: On<FeatureDialogCancel>, q: Query<(), With<ImportDialog>>, mut commands: Commands) {
    if q.contains(ev.entity) {
        commands.queue(cancel);
    }
}

fn on_option(ev: On<CheckboxChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let which = name.as_str().to_string();
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        let Some(mut x) = params(world) else { return };
        let label = match which.as_str() {
            "import-y-up-checkbox" => {
                x.y_axis_up = on;
                "Y axis is up"
            }
            "import-units-checkbox" => {
                x.units = on.then_some(ImportUnit::Millimeter);
                "Specify units"
            }
            _ => return,
        };
        set(world, x, label);
    });
}

fn on_unit(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    if !q.get(ev.entity).is_ok_and(|n| n.as_str() == "import-units-select") {
        return;
    }
    let unit = ImportUnit::ALL[ev.index.min(ImportUnit::ALL.len() - 1)];
    commands.queue(move |world: &mut World| {
        if let Some(mut x) = params(world) {
            x.units = Some(unit);
            set(world, x, "Units");
        }
    });
}

fn on_choose(a: On<bevy::ui_widgets::Activate>, q: Query<(), With<ChooseFile>>, mut commands: Commands) {
    if q.contains(a.entity) {
        commands.queue(open_picker);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_import_dialog(
    doc: Option<Res<ActiveDocument>>,
    session: Option<Res<ImportSession>>,
    cache: Res<PartCache>,
    theme: Res<Theme>,
    q_area: Query<Entity, With<ViewportArea>>,
    mut q_dialog: Query<(Entity, &BuiltFor, &mut FeatureDialogState), With<ImportDialog>>,
    mut commands: Commands,
) {
    let (Some(doc), Some(s)) = (doc, session) else {
        for (e, ..) in &q_dialog {
            commands.entity(e).try_despawn();
        }
        return;
    };
    let Some(f) = doc.doc.element(s.element).and_then(|e| e.feature(s.feature)) else { return };
    let FeatureKind::Import(x) = &f.kind else { return };
    let error = cache.errors.get(&f.id).cloned();
    let valid = f.is_valid() && error.is_none() && !cache.rebuilding;
    let parts = cache.parts.iter().filter(|p| p.feature == f.id).count();
    let layout = BuiltFor(x.file_name.clone(), x.format, x.y_axis_up, x.units, parts);
    let built = q_dialog.iter().next().map(|(e, k, _)| (e, k.clone()));
    if built.as_ref().is_none_or(|(_, k)| *k != layout) {
        if let Some((e, _)) = built {
            commands.entity(e).try_despawn();
        }
        let Some(area) = q_area.iter().next() else { return };
        let t = theme.clone();
        let x = x.clone();
        let dialog = commands
            .spawn((
                ImportDialog,
                layout,
                DespawnOnExit(AppState::Document),
                FeatureDialog::new("import-dialog")
                    .title(f.name.clone())
                    .valid(valid)
                    .body(move |b| {
                        b.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(4.0), ..default() }).with_children(|b| {
                            b.spawn((Name::new("import-file-row"), Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), min_height: Val::Px(28.0), ..default() }))
                                .with_children(|r| {
                                    r.spawn((cadrs_ui::icon("file-import", 16.0, t.muted_foreground), Pickable::IGNORE));
                                    r.spawn((
                                        Name::new("import-file-name"),
                                        t.text(format!("{} ({})", x.file_name, x.format.label()), t.font_base, FontWeight::MEDIUM, t.foreground),
                                        Node { flex_grow: 1.0, ..default() },
                                        Pickable::IGNORE,
                                    ));
                                    r.spawn((ChooseFile, cadrs_ui::Button::new("import-choose-file").label("Choose file").small().outline().tooltip("Import another file").build(&t)));
                                });
                            b.spawn(OptionRow::new("import-y-up", "Y axis is up").checked(x.y_axis_up).build(&t));
                            if x.format == ImportFormat::Stl {
                                b.spawn(OptionRow::new("import-units", "Specify units").checked(x.units.is_some()).build(&t));
                                if let Some(u) = x.units {
                                    let mut sel = Select::new("import-units-select").bordered().width(Val::Px(150.0));
                                    for k in ImportUnit::ALL {
                                        sel = sel.option(k.label(), true);
                                    }
                                    let i = ImportUnit::ALL.iter().position(|k| *k == u).unwrap_or(0);
                                    b.spawn(cadrs_ui::form_row(&t, "import-units-row", "Units", 60.0)).with_children(|r| {
                                        r.spawn(sel.selected(i).build(&t));
                                    });
                                }
                            }
                            let what = match parts {
                                0 => "No parts yet".to_string(),
                                1 => "1 part".to_string(),
                                n => format!("{n} parts"),
                            };
                            b.spawn((Name::new("import-summary"), t.text(what, t.font_base, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
                        });
                    })
                    .build(&theme),
            ))
            .id();
        commands.entity(area).add_child(dialog);
        return;
    }
    for (_, _, mut st) in &mut q_dialog {
        let want = FeatureDialogState { title: f.name.clone(), valid, error: error.is_some() };
        if *st != want {
            *st = want;
        }
    }
}

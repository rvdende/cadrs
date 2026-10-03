//! **Export DXF/DWG of flat pattern** (P3I.6, SM15; `reference/onshape/sheetmetal/help/`
//! `feature-tools/sheetmetal-export-01-03.png`, lesson `15-exporting-a-flat-pattern`): the
//! **Export as DXF/DWG** dialog, laid out as Onshape's:
//!
//! - **File name** ("<document> - Flat pattern of <part>") with *View export rules* (cadrs has
//!   no export rules: greyed, saying so);
//! - **Format** DXF / DWG (DWG through the external converter, disabled without one);
//!   **Version** 2018, 2013, 2010, 2007, 2004 or 2000 (the default);
//! - **Scope**: Single flat pattern part only / All flat pattern parts in the current model /
//!   All flat pattern parts in the Part Studio; several parts go side by side in one file, or
//!   one file each with *Export each part as its own file*;
//! - **Options**: Download (into the **Folder**, with Browse…, as the Export dialog's);
//! - the eight checkboxes with Onshape's defaults ([`cadrs_core::flat_export::FlatExportOptions`]).
//!
//! Opened with [`open`] from the Sheet metal model's context menu in the feature list (its first
//! part) and from a sheet metal part's context menu in the view (that part); the flat view's
//! menu (P3I.3) calls [`open`] with the part under the pointer. Exporting doesn't change the
//! document, so it is not an undo step.
//!
//! Names: `flat-export-dialog`, `flat-export-help`, `flat-export-name-field`, `flat-export-rules`,
//! `flat-export-format`, `flat-export-version`, `flat-export-scope`, `flat-export-separate`,
//! `flat-export-options`, `flat-export-folder-field`, `flat-export-browse`, the checkboxes
//! `flat-export-splines`, `-z-zero`, `-centerlines`, `-tangents`, `-cbore`, `-form-outlines`,
//! `-form-centermarks`, `-sketches`, and `flat-export-ok` / `flat-export-cancel`.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight, TextEdit};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::flat_export::{FlatExportOptions, FlatScope, default_file_name, flat_parts_named, part_page, scope_parts, side_by_side};
use cadrs_core::{FeatureId, PartId};
use cadrs_drawing::dxf::DxfVersion;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, CheckboxState, DialogClose, Notification, Select, SelectState, TextInputField, show_notification};

use crate::parts::PartCache;
use crate::{ActiveDocument, AppState};

pub struct FlatExportDialogPlugin;

impl Plugin for FlatExportDialogPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (on_folder_picked, sync_separate).run_if(in_state(AppState::Document)));
    }
}

/// The dialog's root: the part it was opened on.
#[derive(Component, Debug, Clone, Copy)]
pub struct FlatExportDialog {
    pub part: PartId,
}

const WIDTH: f32 = 540.0;
const FIELD_H: f32 = 30.0;

/// The versions offered: every one the DXF writer writes (2000 to 2018; R12's older format
/// isn't written), newest first, with 2000 the default as Onshape's.
fn versions() -> Vec<DxfVersion> {
    DxfVersion::WRITTEN.into_iter().rev().collect()
}

/// The checkboxes: name, label, default.
const CHECKS: [(&str, &str, bool); 8] = [
    ("flat-export-splines", "Export splines as polylines", false),
    ("flat-export-z-zero", "Set z-height to zero and normals to positive", true),
    ("flat-export-centerlines", "Include bend centerlines", true),
    ("flat-export-tangents", "Include bend tangent lines", false),
    ("flat-export-cbore", "Include counterbore and countersink lines", false),
    ("flat-export-form-outlines", "Include form feature outlines", false),
    ("flat-export-form-centermarks", "Include form feature centermarks", false),
    ("flat-export-sketches", "Include visible sketches", true),
];

fn label(p: &mut ChildSpawner, t: &Theme, text: &str) {
    p.spawn(t.text(text, t.font_base, FontWeight::BOLD, t.foreground));
}

fn row(name: &str) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Node { flex_direction: FlexDirection::Column, width: Val::Percent(100.0), row_gap: Val::Px(6.0), margin: UiRect::bottom(Val::Px(12.0)), ..default() },
    )
}

/// The active Part Studio's part renames.
fn props(world: &World) -> Vec<cadrs_core::PartProps> {
    world.get_resource::<ActiveDocument>().and_then(|d| d.active_element()).map(|e| e.part_props().to_vec()).unwrap_or_default()
}

/// The flat-pattern parts of the active Part Studio, as the last rebuild has them.
fn studio_build(world: &World) -> Option<(Vec<cadrs_core::Feature>, std::sync::Arc<cadrs_core::rebuild::Build>)> {
    let el = world.get_resource::<ActiveDocument>()?.active_element()?;
    let features = el.active_features();
    let build = cadrs_core::rebuild::build(&features);
    Some((features, build))
}

/// The first part of a Sheet metal model (for its feature list menu).
pub fn first_part(world: &World, model: FeatureId) -> Option<PartId> {
    let (_, build) = studio_build(world)?;
    flat_parts_named(&build, &props(world)).into_iter().find(|r| r.model == model).map(|r| r.part)
}

/// The flat-pattern part `part` is, if it is one (its view menu offers the flat's items).
pub fn flat_ref(world: &World, part: PartId) -> Option<cadrs_core::flat_export::FlatPartRef> {
    let (_, build) = studio_build(world)?;
    flat_parts_named(&build, &props(world)).into_iter().find(|r| r.part == part)
}

fn toast(world: &mut World, note: Notification) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    show_notification(&mut commands, &theme, note.name("flat-export-toast"));
    world.flush();
}

/// Opens the dialog on a flat-pattern part.
pub fn open(world: &mut World, part: PartId) {
    let Some((_, build)) = studio_build(world) else { return };
    // Named as the Parts list names them (a renamed part by its new name).
    let all = flat_parts_named(&build, &props(world));
    let Some(me) = all.iter().find(|r| r.part == part).cloned() else {
        return toast(world, Notification::warning("That part has no flat pattern"));
    };
    let document = world.get_resource::<ActiveDocument>().map(|d| d.doc.name.clone()).unwrap_or_default();
    let base = default_file_name(&document, &me.name);
    let dir = crate::export_dir(world).map(|d| d.display().to_string()).unwrap_or_default();
    let converter = cadrs_drawing::dwg::find_converter().filter(|c| c.can_write());
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("flat-export-dialog")
            .title("Export as DXF/DWG")
            .width(WIDTH)
            // The header's "?" before the ✕, as Onshape's.
            .header({
                let th = theme.clone();
                move |h| {
                    h.spawn(cadrs_ui::IconButton::new("flat-export-help", "help").icon_size(14.0).tooltip("Exports the flat pattern as DXF or DWG: outline, cut-outs, bend lines by direction and the options below, each on its own layer").build(&th));
                }
            })
            .body(move |b| {
                let t = &tb;
                let full = || Val::Percent(100.0);
                b.spawn(row("flat-export-name-row")).with_children(|g| {
                    g.spawn(Node { align_items: AlignItems::Baseline, column_gap: Val::Px(6.0), ..default() }).with_children(|l| {
                        label(l, t, "File name");
                        l.spawn((
                            Name::new("flat-export-rules"),
                            t.text("View export rules", t.font_sm, FontWeight::NORMAL, Color::srgb_u8(0x8a, 0xa4, 0xc8)),
                            Tooltip::new("cadrs has no export rules: the file name is yours to set"),
                        ));
                    });
                    g.spawn(TextInput::new("flat-export-name").value(base).select_all_on_focus().autofocus().width(full()).height(FIELD_H).build(t));
                });
                b.spawn(row("flat-export-format-row")).with_children(|g| {
                    label(g, t, "Format");
                    let dwg = match &converter {
                        Some(_) => ("DWG".to_string(), true),
                        None => ("DWG (needs LibreDWG or the ODA File Converter on PATH)".to_string(), false),
                    };
                    g.spawn(Select::new("flat-export-format").bordered().width(full()).option("DXF", true).option(dwg.0, dwg.1).build(t));
                });
                b.spawn(row("flat-export-version-row")).with_children(|g| {
                    label(g, t, "Version");
                    let mut s = Select::new("flat-export-version").bordered().width(full());
                    let all = versions();
                    for v in &all {
                        s = s.option(v.year(), true);
                    }
                    let default = all.iter().position(|v| *v == DxfVersion::R2000).unwrap_or(0);
                    g.spawn(s.selected(default).build(t));
                });
                b.spawn(row("flat-export-scope-row")).with_children(|g| {
                    label(g, t, "Scope");
                    let mut s = Select::new("flat-export-scope").bordered().width(full());
                    for sc in FlatScope::ALL {
                        s = s.option(sc.label(), true);
                    }
                    g.spawn(s.build(t));
                    g.spawn(Checkbox::new("flat-export-separate").label("Export each part as its own file").height(22.0).build(t))
                        .entry::<Node>()
                        .and_modify(|mut n| n.display = Display::None);
                });
                b.spawn(row("flat-export-options-row")).with_children(|g| {
                    label(g, t, "Options");
                    g.spawn(Select::new("flat-export-options").bordered().width(full()).option("Download", true).build(t));
                    g.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, width: full(), ..default() }).with_children(|r| {
                        r.spawn((Name::new("flat-export-folder-label"), t.text("Folder", t.font_sm, FontWeight::NORMAL, t.muted_foreground)));
                        r.spawn(TextInput::new("flat-export-folder").value(dir).width(full()).height(26.0).build(t));
                        r.spawn((
                            Button::new("flat-export-browse").label("Browse…").outline().build(t),
                            observe(|_: On<Activate>, mut commands: Commands| {
                                commands.queue(browse_folder);
                            }),
                        ));
                    });
                });
                b.spawn((Name::new("flat-export-checks"), Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() })).with_children(|g| {
                    for (name, text, on) in CHECKS {
                        g.spawn(Checkbox::new(name).label(text).checked(on).height(24.0).build(t));
                    }
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("flat-export-ok").label("Export").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(start_export);
                    }),
                ));
                f.spawn((
                    Button::new("flat-export-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<FlatExportDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        FlatExportDialog { part },
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// *Export each part as its own file* shows for the scopes of several parts.
fn sync_separate(q_dialog: Query<(), With<FlatExportDialog>>, q_sel: Query<(&Name, &SelectState)>, mut q: Query<(&Name, &mut Node)>) {
    if q_dialog.is_empty() {
        return;
    }
    let many = q_sel.iter().any(|(n, s)| n.as_str() == "flat-export-scope" && s.selected > 0);
    for (n, mut node) in &mut q {
        if n.as_str() == "flat-export-separate" {
            let want = if many { Display::Flex } else { Display::None };
            if node.display != want {
                node.display = want;
            }
        }
    }
}

fn browse_folder(world: &mut World) {
    let typed = field(world, "flat-export-folder-field");
    let dir = PathBuf::from(typed.trim());
    let dir = if dir.is_dir() { dir } else { std::env::current_dir().unwrap_or_default() };
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::file_picker::open_folder_picker(&mut commands, &theme, "folder-picker", "Choose a folder", "flat-export-folder", dir);
    world.flush();
}

fn on_folder_picked(mut msgs: MessageReader<cadrs_ui::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != "flat-export-folder" {
            continue;
        }
        let text = m.path.display().to_string();
        commands.queue(move |w: &mut World| {
            let mut q = w.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
            if let Some((_, mut t)) = q.iter_mut(w).find(|(n, _)| n.as_str() == "flat-export-folder-field") {
                t.queue_edit(TextEdit::SelectAll);
                t.queue_edit(TextEdit::Insert(text.clone().into()));
            }
        });
    }
}

fn field(w: &mut World, name: &str) -> String {
    let mut q = w.query_filtered::<(&Name, &EditableText), With<TextInputField>>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, t)| t.value().to_string()).unwrap_or_default()
}

fn select(w: &mut World, name: &str) -> usize {
    let mut q = w.query::<(&Name, &SelectState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected).unwrap_or(0)
}

fn checked(w: &mut World, name: &str) -> bool {
    let mut q = w.query::<(&Name, &CheckboxState)>();
    q.iter(w).any(|(n, s)| n.as_str() == name && s.checked)
}

/// Export: writes the file(s) and closes the dialog.
fn start_export(world: &mut World) {
    let mut q = world.query::<(Entity, &FlatExportDialog)>();
    let Some((dialog, spec)) = q.iter(world).next().map(|(e, d)| (e, *d)) else { return };
    let base = field(world, "flat-export-name-field").trim().to_string();
    let folder = field(world, "flat-export-folder-field").trim().to_string();
    let dwg = select(world, "flat-export-format") == 1;
    let all = versions();
    let version = all[select(world, "flat-export-version").min(all.len() - 1)];
    let scope = FlatScope::ALL[select(world, "flat-export-scope").min(2)];
    let separate = checked(world, "flat-export-separate") && scope != FlatScope::Single;
    let o = FlatExportOptions {
        splines_as_polylines: checked(world, "flat-export-splines"),
        z_zero: checked(world, "flat-export-z-zero"),
        centerlines: checked(world, "flat-export-centerlines"),
        tangent_lines: checked(world, "flat-export-tangents"),
        cbore_lines: checked(world, "flat-export-cbore"),
        form_outlines: checked(world, "flat-export-form-outlines"),
        form_centermarks: checked(world, "flat-export-form-centermarks"),
        sketches: checked(world, "flat-export-sketches"),
    };
    world.trigger(DialogClose { entity: dialog });
    if folder.is_empty() {
        return toast(world, Notification::warning("Nowhere to save the export"));
    }
    let Some((features, build)) = studio_build(world) else { return };
    let document = world.get_resource::<ActiveDocument>().map(|d| d.doc.name.clone()).unwrap_or_default();
    let hidden = world.resource::<PartCache>().hidden_sketches.clone();
    let el = world.get_resource::<ActiveDocument>().and_then(|d| d.active_element().cloned());
    let shown = |id: FeatureId| !hidden.contains(&id) && el.as_ref().is_none_or(|e| e.sketch_visibility(id) != Some(false));
    let refs = scope_parts(&build, &props(world), spec.part, scope);
    let pages: Vec<(String, cadrs_drawing::export::Page)> = refs.iter().filter_map(|r| Some((r.name.clone(), part_page(&build, &features, r, &o, &shown)?))).collect();
    if pages.is_empty() {
        return toast(world, Notification::warning("Export failed: no flat pattern to export"));
    }
    let dir = PathBuf::from(&folder);
    let name = if base.is_empty() { "Flat pattern".to_string() } else { base };
    let files: Vec<(String, cadrs_drawing::export::Page)> = if separate {
        pages.into_iter().map(|(part, p)| (default_file_name(&document, &part), p)).collect()
    } else if pages.len() == 1 {
        vec![(name, pages.into_iter().next().expect("one").1)]
    } else {
        let all: Vec<cadrs_drawing::export::Page> = pages.into_iter().map(|(_, p)| p).collect();
        vec![(name.clone(), side_by_side(&all, &name))]
    };
    let mut written = Vec::new();
    for (stem, page) in &files {
        match cadrs_core::dxf_export::write_file(page, version, dwg, &dir, &cadrs_core::export::sanitize(stem)) {
            Ok(p) => {
                info!("exported {}", p.display());
                written.push(p);
            }
            Err(why) => return toast(world, Notification::warning(format!("Export failed: {why}"))),
        }
    }
    let note = match written.as_slice() {
        [one] => format!("Exported {}", one.display()),
        many => format!("Exported {} files to {}", many.len(), dir.display()),
    };
    toast(world, Notification::info(note).seconds(8.0));
}

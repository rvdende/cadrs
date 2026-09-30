//! Export… for drawings (P3C.7, D2.10, X13): the drawing tab's menu → Export… opens a dialog
//! like the Parts list's STEP export (`crate::export_dialog`), with:
//!
//! - **File name**: the drawing's name to start with.
//! - **Format**: PDF, DXF, DWG, DWT, PNG or JPEG. DWG and DWT (a DWG-format template) are disabled, with a tooltip saying what to
//!   install, when no converter is on `PATH` (`cadrs_drawing::dwg`).
//! - **Sheets**: all, or the current one. PDF puts every sheet on its own page at its paper
//!   size; DXF, DWG and images write a file per sheet.
//! - **Colors** (PDF, PNG, JPEG): colour, or black and white. **Resolution** (PNG, JPEG): 150,
//!   300 or 600 dpi.
//! - **Folder**: where the files go (`$CADRS_EXPORT_DIR` or the Downloads folder to start
//!   with; scenarios type a folder under `target/`).
//!
//! Export (or Enter in the name) writes the files and a toast says where they went. Exporting
//! doesn't change the document, so it is not an undo step.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_drawing::export::{ExportOptions, Format};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, DialogClose, Notification, Select, SelectState, TextInputField, show_notification};

use crate::{ActiveDocument, AppClock, AppState};

pub struct DrawingExportPlugin;

impl Plugin for DrawingExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (sync_format_rows, on_folder_picked).run_if(in_state(AppState::Document)))
            .add_observer(on_submit);
    }
}

/// The dialog's root: the drawing it exports and the chosen format.
#[derive(Component, Debug, Clone)]
pub struct DrawingExportDialog {
    pub element: ElementId,
    pub format: Format,
    /// The sheet shown when the dialog opened.
    pub current: usize,
}

/// A format button.
#[derive(Component, Debug, Clone, Copy)]
struct FormatButton(Format);

const WIDTH: f32 = 560.0;
/// Text fields as tall as the dropdowns.
const FIELD_H: f32 = 26.0;

/// Browse…: the folder picker, starting at the typed folder.
fn browse_folder(world: &mut World) {
    let typed = field(world, "drawing-export-folder-field");
    let dir = std::path::PathBuf::from(typed.trim());
    let dir = if dir.is_dir() { dir } else { std::env::current_dir().unwrap_or_default() };
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    cadrs_ui::file_picker::open_folder_picker(&mut commands, &theme, "folder-picker", "Choose a folder", "export-folder", dir);
    world.flush();
}

/// A folder chosen with Browse… goes into the Folder field.
fn on_folder_picked(mut msgs: MessageReader<cadrs_ui::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag != "export-folder" {
            continue;
        }
        let path = m.path.display().to_string();
        commands.queue(move |w: &mut World| {
            let mut q = w.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
            if let Some((_, mut t)) = q.iter_mut(w).find(|(n, _)| n.as_str() == "drawing-export-folder-field") {
                t.queue_edit(bevy::text::TextEdit::SelectAll);
                t.queue_edit(bevy::text::TextEdit::Insert(path.into()));
            }
        });
    }
}

fn label(p: &mut ChildSpawner, t: &Theme, text: &str) {
    p.spawn(t.text(text, t.font_base, FontWeight::BOLD, t.foreground));
}

/// A labelled field: the label, then the field 4 px under it; every group the same 10 px apart
/// (P3C.8: the gaps were uneven).
fn row(name: &str) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Node {
            flex_direction: FlexDirection::Column,
            width: Val::Percent(100.0),
            row_gap: Val::Px(4.0),
            margin: UiRect::bottom(Val::Px(10.0)),
            ..default()
        },
    )
}

/// Opens Export… for drawing `element`.
pub fn open_drawing_export(world: &mut World, element: ElementId) {
    let Some((name, sheets, current)) = world.get_resource::<ActiveDocument>().and_then(|doc| {
        let el = doc.doc.element(element)?;
        let cadrs_core::ElementKind::Drawing(d) = &el.kind else { return None };
        let current = world.resource::<super::DrawingUi>().sheet_index(element, d);
        let names: Vec<String> = d.sheets.iter().map(|s| s.name.clone()).collect();
        Some((el.name.clone(), names, current))
    }) else {
        return;
    };
    let dir = crate::export_dir(world).map(|d| d.display().to_string()).unwrap_or_default();
    let converter = cadrs_drawing::dwg::find_converter().filter(|c| c.can_write());
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let format = Format::Pdf;
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("drawing-export-dialog")
            .title("Export")
            .width(WIDTH)
            .body(move |b| {
                let t = &tb;
                let full = || Val::Percent(100.0);
                b.spawn(row("drawing-export-name-row")).with_children(|g| {
                    label(g, t, "File name");
                    g.spawn(TextInput::new("drawing-export-name").value(name).select_all_on_focus().autofocus().width(full()).height(FIELD_H).build(t));
                });
                b.spawn(row("drawing-export-format-row")).with_children(|g| {
                // The DWG note sits beside the label, where the buttons' tooltips (shown under
                // them) don't cover it.
                g.spawn(Node { align_items: AlignItems::Baseline, column_gap: Val::Px(10.0), ..default() }).with_children(|l| {
                    label(l, t, "Format");
                    if converter.is_none() {
                        l.spawn((
                            Name::new("drawing-export-dwg-note"),
                            t.text("DWG and DWT need LibreDWG or the ODA File Converter on PATH.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                        ));
                    }
                });
                g.spawn((Name::new("drawing-export-formats"), Node { column_gap: Val::Px(6.0), ..default() }))
                    .with_children(|r| {
                        for f in Format::ALL {
                            let disabled = f.needs_converter() && converter.is_none();
                            let tip = match (f, &converter) {
                                (Format::Pdf, _) => "Vector PDF, a page per sheet at its paper size".to_string(),
                                (Format::Dxf, _) => "AutoCAD R2013 DXF in millimetres, a file per sheet".to_string(),
                                (Format::Dwg, Some(c)) => format!("DWG through the {} (a file per sheet)", c.name()),
                                // Short, so it stays clear of the Sheets label below (P3C.8's delta); the
                                // row's hint says what to install.
                                (Format::Dwg, None) => "DWG: no converter on PATH".to_string(),
                                (Format::Dwt, Some(c)) => format!("AutoCAD template (DWG format) through the {}", c.name()),
                                (Format::Dwt, None) => "DWT: no converter on PATH".to_string(),
                                (Format::Png, _) => "PNG image, a file per sheet".to_string(),
                                (Format::Jpeg, _) => "JPEG image, a file per sheet".to_string(),
                            };
                            r.spawn((
                                Button::new(format!("drawing-export-format-{}", f.extension()))
                                    .label(f.label())
                                    .outline()
                                    .selected(f == format)
                                    .disabled(disabled)
                                    .tooltip(tip)
                                    .width(Val::Px(70.0))
                                    .build(t),
                                FormatButton(f),
                                observe(move |_: On<Activate>, mut q: Query<&mut DrawingExportDialog>| {
                                    for mut d in &mut q {
                                        d.format = f;
                                    }
                                }),
                            ))
                            // The chosen format: a blue border on a pale blue (a toggle, like the
                            // sketch toolbar's active tool), clearly unlike the grey hover.
                            .insert(cadrs_ui::Visuals {
                                background: cadrs_ui::StateColors::new(t.background, t.list_hover, t.list_active, t.background)
                                    .with_selected(Color::srgb_u8(0xdd, 0xea, 0xfb)),
                                border: cadrs_ui::StateColors::all(Color::srgb_u8(0xc8, 0xc8, 0xc8)).with_selected(Color::srgb_u8(0x1f, 0x7a, 0xe0)),
                                foreground: cadrs_ui::StateColors::new(t.foreground, t.foreground, t.foreground, t.disabled_foreground)
                                    .with_selected(Color::srgb_u8(0x14, 0x5c, 0xb8)),
                                focus_ring: t.focus_ring,
                            });
                        }
                    });
                });
                b.spawn(row("drawing-export-sheets-row")).with_children(|g| {
                    label(g, t, "Sheets");
                    let mut sel = Select::new("drawing-export-sheets").bordered().width(full()).option(format!("All sheets ({})", sheets.len()), true);
                    sel = sel.option(format!("Current sheet ({})", sheets.get(current).cloned().unwrap_or_default()), true);
                    g.spawn(sel.build(t));
                });
                b.spawn(row("drawing-export-color-row")).with_children(|r| {
                    label(r, t, "Colors");
                    r.spawn(Select::new("drawing-export-color").bordered().width(full()).option("Color", true).option("Black and white", true).build(t));
                });
                b.spawn(row("drawing-export-version-row")).with_children(|r| {
                    label(r, t, "Version");
                    let mut v = Select::new("drawing-export-version").bordered().width(full());
                    for d in cadrs_drawing::dxf::DxfVersion::ALL {
                        v = v.option(d.label(), true);
                    }
                    r.spawn(v.build(t));
                });
                b.spawn(row("drawing-export-dpi-row")).with_children(|r| {
                    label(r, t, "Resolution");
                    r.spawn(
                        Select::new("drawing-export-dpi")
                            .bordered()
                            .width(full())
                            .option("150 dpi", true)
                            .option("300 dpi", true)
                            .option("600 dpi", true)
                            .selected(1)
                            .build(t),
                    );
                });
                b.spawn(row("drawing-export-folder-row")).with_children(|g| {
                    label(g, t, "Folder");
                    g.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, width: full(), ..default() }).with_children(|r| {
                        r.spawn(TextInput::new("drawing-export-folder").value(dir).width(full()).height(FIELD_H).build(t));
                        r.spawn((
                            Button::new("drawing-export-browse").label("Browse…").outline().build(t),
                            observe(|_: On<Activate>, mut commands: Commands| {
                                commands.queue(browse_folder);
                            }),
                        ));
                    });
                });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("drawing-export-ok").label("Export").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(run_export);
                    }),
                ));
                f.spawn((
                    Button::new("drawing-export-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<DrawingExportDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        DrawingExportDialog { element, format, current },
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// The format buttons show the chosen one; Colors shows for PDF and images, Resolution for
/// images.
fn sync_format_rows(
    q: Query<&DrawingExportDialog, Changed<DrawingExportDialog>>,
    q_buttons: Query<(Entity, &FormatButton, Has<cadrs_ui::Selected>)>,
    mut q_rows: Query<(&Name, &mut Node)>,
    mut commands: Commands,
) {
    let Some(d) = q.iter().next() else { return };
    for (e, b, sel) in &q_buttons {
        if (b.0 == d.format) != sel {
            if sel {
                commands.entity(e).try_remove::<cadrs_ui::Selected>();
            } else {
                commands.entity(e).try_insert(cadrs_ui::Selected);
            }
        }
    }
    for (n, mut node) in &mut q_rows {
        let show = match n.as_str() {
            "drawing-export-color-row" => matches!(d.format, Format::Pdf | Format::Png | Format::Jpeg),
            "drawing-export-dpi-row" => matches!(d.format, Format::Png | Format::Jpeg),
            "drawing-export-version-row" => matches!(d.format, Format::Dxf | Format::Dwg | Format::Dwt),
            _ => continue,
        };
        let want = if show { Display::Flex } else { Display::None };
        if node.display != want {
            node.display = want;
        }
    }
}

fn on_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "drawing-export-name-field") {
        commands.queue(run_export);
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

/// Export: writes the files and says where they went.
fn run_export(world: &mut World) {
    let mut q = world.query::<(Entity, &DrawingExportDialog)>();
    let Some((dialog, spec)) = q.iter(world).next().map(|(e, d)| (e, d.clone())) else {
        return;
    };
    let base = field(world, "drawing-export-name-field");
    let folder = field(world, "drawing-export-folder-field");
    let all = select(world, "drawing-export-sheets") == 0;
    let color = select(world, "drawing-export-color") == 0;
    let dpi = [150.0, 300.0, 600.0][select(world, "drawing-export-dpi").min(2)];
    let dxf_version = cadrs_drawing::dxf::DxfVersion::ALL[select(world, "drawing-export-version").min(1)];
    world.trigger(DialogClose { entity: dialog });
    let result = export_now(world, &spec, &base, &folder, all, &ExportOptions { format: spec.format, color, dpi, dxf_version });
    let theme = world.resource::<Theme>().clone();
    let note = match result {
        Ok(files) => {
            for f in &files {
                info!("exported {}", f.display());
            }
            match files.as_slice() {
                [one] => Notification::info(format!("Exported {}", one.display())).seconds(8.0),
                many => Notification::info(format!("Exported {} files to {}", many.len(), folder.trim())).seconds(8.0),
            }
        }
        Err(why) => Notification::warning(format!("Export failed: {why}")),
    };
    let mut commands = world.commands();
    show_notification(&mut commands, &theme, note.name("export-toast"));
    world.flush();
}

/// Writes drawing `spec.element`'s sheets (all, or the current one) into `folder`.
pub fn export_now(world: &mut World, spec: &DrawingExportDialog, base: &str, folder: &str, all: bool, opts: &ExportOptions) -> Result<Vec<PathBuf>, String> {
    let folder = folder.trim();
    if folder.is_empty() {
        return Err("no folder to save to".into());
    }
    let date = super::notes::today(world.get_resource::<AppClock>());
    let doc = world.get_resource::<ActiveDocument>().ok_or("no document")?;
    let el = doc.doc.element(spec.element).ok_or("the drawing is gone")?;
    let cadrs_core::ElementKind::Drawing(d) = &el.kind else {
        return Err("not a drawing".into());
    };
    let sheets: Vec<usize> = if all { (0..d.sheets.len()).collect() } else { vec![spec.current.min(d.sheets.len().saturating_sub(1))] };
    let cache = world.resource::<super::views::ViewCache>();
    let pages = cadrs_core::drawing_export::pages(&doc.doc, &el.name, d, &sheets, date, &|v| cache.geometry(v));
    let base = if base.trim().is_empty() { el.name.clone() } else { base.trim().to_string() };
    cadrs_drawing::export::write_files(&pages, opts, std::path::Path::new(folder), &base)
}

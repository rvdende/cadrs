//! Post: a setup's toolpaths (Post child operations) or the selected ones (Post selected
//! operations) as G-code for the job's machine. The Export dialog shows the program number,
//! the file name, the machine and its post processor, the output units, the operations in it
//! and the start of the program; Post writes it to the folder (Browse… picks another), with the
//! setup sheet beside it when that's ticked ([`cadrs_core::cam::sheet`]), and says where.

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use cadrs_core::cam::cadrs_cam::post;
use cadrs_core::cam::cadrs_cam::tool::Unit as LenUnit;
use cadrs_core::cam::{CamId, NodeRef, OpKind, program};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    Button, Checkbox, CheckboxChange, Dialog, DialogClose, Notification, Select, show_notification,
};

use super::{CamUi, Computed, active_cam_owned, toast};
use crate::{ActiveDocument, AppState};

pub struct ExportPlugin;

impl Plugin for ExportPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_activate)
            .add_observer(on_select)
            .add_observer(on_check)
            .add_systems(
                Update,
                on_folder_picked.run_if(in_state(AppState::Document)),
            );
    }
}

/// What the open Export dialog posts.
#[derive(Resource, Clone)]
pub struct ExportSession {
    pub job: CamId,
    pub setup: CamId,
    /// The operations (`None`: all of the setup's).
    pub ops: Option<Vec<CamId>>,
    pub inch: bool,
    /// Write the setup sheet beside the program.
    pub sheet: bool,
    /// Post although it runs beyond the machine's travel (the operator ticked it).
    pub beyond_travel: bool,
}

#[derive(Component)]
struct ExportDialog;

/// Post for the selection: the selected toolpaths of one setup, else the selected (or active)
/// setup's.
pub fn open_for_selection(world: &mut World) {
    let Some((_, cam)) = active_cam_owned(world) else {
        return;
    };
    let sel = world.resource::<CamUi>().selected.clone();
    let mut setup: Option<(usize, usize)> = None;
    let mut ops: Vec<CamId> = Vec::new();
    for id in &sel {
        match cam.find(*id) {
            Some(NodeRef::Operation(j, s, o)) => {
                if matches!(
                    cam.jobs[j].setups[s].operations[o].kind,
                    OpKind::Toolpath(_)
                ) && setup.is_none_or(|x| x == (j, s))
                {
                    setup = Some((j, s));
                    ops.push(*id);
                }
            }
            Some(NodeRef::Setup(j, s)) if setup.is_none() => setup = Some((j, s)),
            Some(NodeRef::Job(j)) if setup.is_none() && !cam.jobs[j].setups.is_empty() => {
                setup = Some((j, 0))
            }
            _ => {}
        }
    }
    let setup = setup.or_else(|| {
        let j = cam
            .jobs
            .iter()
            .position(|j| Some(j.id) == cam.active().map(|a| a.id))?;
        (!cam.jobs[j].setups.is_empty()).then_some((j, 0))
    });
    let Some((j, s)) = setup else {
        toast(world, "Select a setup or toolpaths to post");
        return;
    };
    let job = &cam.jobs[j];
    let st = &job.setups[s];
    // Every toolpath posted must be calculated.
    let wanted: Vec<CamId> = st
        .operations
        .iter()
        .filter(|o| !o.suppressed && matches!(o.kind, OpKind::Toolpath(_)))
        .filter(|o| ops.is_empty() || ops.contains(&o.id))
        .map(|o| o.id)
        .collect();
    let missing: Vec<CamId> = wanted
        .iter()
        .copied()
        .filter(|o| world.resource::<Computed>().current(*o).is_none())
        .collect();
    if wanted.is_empty() {
        toast(world, "The setup has no toolpaths to post");
        return;
    }
    if !missing.is_empty() {
        world.resource_mut::<Computed>().request(missing);
        toast(
            world,
            "Calculating the toolpaths first: post again when they're done",
        );
        return;
    }
    let inch = job
        .machine
        .as_ref()
        .is_some_and(|m| m.post.output_unit == LenUnit::Inch);
    super::dialogs::close(world);
    close(world);
    world.insert_resource(ExportSession {
        job: job.id,
        setup: st.id,
        ops: (!ops.is_empty()).then_some(ops),
        inch,
        sheet: true,
        beyond_travel: false,
    });
    open_dialog(world);
}

/// What the session posts (with the dialog's units).
struct Program {
    nc: String,
    /// The default file name.
    file: String,
    /// "Generic 3-axis mill — Fanuc (post: Fanuc)".
    machine: String,
    /// "T1 Profile 1", …
    names: Vec<String>,
    /// The setup sheet (HTML).
    sheet: String,
    /// What the operator should know before running it.
    notes: Vec<String>,
    /// Where it runs beyond the machine's travel: posted only when the operator says so.
    travel: Vec<String>,
}

fn program_text(world: &World, s: &ExportSession) -> Result<Program, String> {
    let doc = world.resource::<ActiveDocument>();
    let (_, cam) = super::active_cam(doc).ok_or("No CAM Studio")?;
    let job = cam.job(s.job).ok_or("The job is gone")?;
    let setup = job
        .setups
        .iter()
        .find(|x| x.id == s.setup)
        .ok_or("The setup is gone")?;
    let parts =
        super::compute::studio_parts(world, super::active_cam(doc).ok_or("No CAM Studio")?.0)
            .ok_or("The parts are still being built")?;
    let paths = world.resource::<Computed>().paths();
    let run = program::setup_run(cam, job, setup, &parts, &paths, s.ops.as_deref())?;
    let mut job2 = job.clone();
    if let Some(m) = &mut job2.machine {
        m.post.output_unit = if s.inch { LenUnit::Inch } else { LenUnit::Mm };
    }
    let nc = program::post_setup(setup, &job2, &run)?;
    let m = job2.machine.as_ref().ok_or("The job has no machine")?;
    let names = run
        .ops
        .iter()
        .map(|o| format!("T{} {}", o.tool_number, o.name))
        .collect();
    let file = post::file_name(&format!("{} {}", job.name, setup.name), m.dialect);
    let sheet = cadrs_core::cam::sheet::setup_sheet(cam, job, setup, &run, &parts, &file);
    // The work offsets to set beyond the setup's own.
    let mut notes: Vec<String> = program::offset_lines(setup, job, &run)
        .into_iter()
        .map(|l| format!("Set {l}"))
        .collect();
    // Where the machine mating puts it on the table: inside the travel?
    let travel = program::stock_shape(cam, job, &parts)
        .map(|shape| {
            let stock = program::stock_in(&shape, &run.frame);
            program::travel_warnings(job, setup, &run, &stock)
        })
        .unwrap_or_default();
    if m.kind == cadrs_core::cam::cadrs_cam::MachineKind::Plasma
        && m.dialect == post::Dialect::LinuxCnc
        && run.ops.iter().any(|o| o.tool.beam.material == 0)
    {
        notes.push(
            "No material number on the torch: QtPlasmaC cuts with the material it has loaded \
             (set one on the tool)"
                .to_string(),
        );
    }
    Ok(Program {
        nc,
        file,
        machine: format!("{} (post: {})", m.title(), m.dialect.label()),
        names,
        sheet,
        notes,
        travel,
    })
}

fn open_dialog(world: &mut World) {
    let Some(s) = world.get_resource::<ExportSession>().cloned() else {
        return;
    };
    let Program {
        nc,
        file,
        machine,
        names,
        notes,
        travel,
        ..
    } = match program_text(world, &s) {
        Ok(x) => x,
        Err(e) => {
            toast(world, &e);
            world.remove_resource::<ExportSession>();
            return;
        }
    };
    let program_number = {
        let doc = world.resource::<ActiveDocument>();
        super::active_cam(doc)
            .and_then(|(_, c)| c.setup(s.setup).map(|(_, st)| st.program_number))
            .unwrap_or(1)
    };
    let dir = crate::export_dir(world)
        .map(|d| d.display().to_string())
        .unwrap_or_default();
    let summary = format!(
        "{} lines, {}",
        nc.lines().count(),
        if s.inch { "inches" } else { "millimetres" }
    );
    let head: String = nc.lines().take(80).collect::<Vec<_>>().join("\n");
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let mut commands = world.commands();
    commands.spawn((
        ExportDialog,
        DespawnOnExit(AppState::Document),
        Dialog::new("cam-export-dialog")
            .title("Post")
            .width(560.0)
            .body(move |b| {
                let t = &tb;
                let full = || Val::Percent(100.0);
                let row =
                    |b: &mut ChildSpawner, label: &str, f: &mut dyn FnMut(&mut ChildSpawner)| {
                        b.spawn((
                            Name::new(format!("cam-export-row {label}")),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(8.0),
                                width: full(),
                                margin: UiRect::bottom(Val::Px(6.0)),
                                ..default()
                            },
                        ))
                        .with_children(|r| {
                            r.spawn((
                                t.text(label, t.font_base, FontWeight::BOLD, t.foreground),
                                Node {
                                    width: Val::Px(120.0),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                            ));
                            f(r);
                        });
                    };
                row(b, "Program number", &mut |r| {
                    r.spawn(
                        TextInput::new("cam-export-number")
                            .value(program_number.to_string())
                            .width(Val::Px(120.0))
                            .height(26.0)
                            .build(t),
                    );
                });
                row(b, "File name", &mut |r| {
                    r.spawn(
                        TextInput::new("cam-export-name")
                            .value(file.clone())
                            .select_all_on_focus()
                            .autofocus()
                            .width(full())
                            .height(26.0)
                            .build(t),
                    );
                });
                row(b, "Machine", &mut |r| {
                    r.spawn(t.text(&machine, t.font_base, FontWeight::NORMAL, t.foreground));
                });
                row(b, "Output units", &mut |r| {
                    r.spawn(
                        Select::new("cam-export-units")
                            .bordered()
                            .width(Val::Px(160.0))
                            .option("Millimetres", true)
                            .option("Inches", true)
                            .selected(usize::from(s.inch))
                            .build(t),
                    );
                });
                row(b, "Operations", &mut |r| {
                    r.spawn(t.text(
                        &names.join(", "),
                        t.font_base,
                        FontWeight::NORMAL,
                        t.foreground,
                    ));
                });
                row(b, "Setup sheet", &mut |r| {
                    r.spawn(
                        Checkbox::new("cam-export-sheet")
                            .label("Write the setup sheet beside it (HTML)")
                            .checked(s.sheet)
                            .build(t),
                    );
                });
                row(b, "Folder", &mut |r| {
                    r.spawn(
                        TextInput::new("cam-export-folder")
                            .value(dir.clone())
                            .width(full())
                            .height(26.0)
                            .build(t),
                    );
                    r.spawn(
                        Button::new("cam-export-browse")
                            .label("Browse…")
                            .outline()
                            .build(t),
                    );
                });
                b.spawn((
                    Name::new("cam-export-preview"),
                    Node {
                        width: full(),
                        height: Val::Px(220.0),
                        overflow: Overflow::scroll_y(),
                        padding: UiRect::all(Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(t.input_background),
                    BorderColor::all(t.border),
                    children![t.text(&head, 11.0, FontWeight::NORMAL, t.foreground)],
                ));
                b.spawn((
                    Name::new("cam-export-summary"),
                    t.text(&summary, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                ));
                for n in travel.iter().chain(&notes) {
                    b.spawn((
                        Name::new("cam-export-note"),
                        t.text(n, t.font_sm, FontWeight::NORMAL, t.danger),
                    ));
                }
                if !travel.is_empty() {
                    b.spawn(
                        Checkbox::new("cam-export-beyond-travel")
                            .label("Post anyway: I've checked where it runs on the machine")
                            .checked(s.beyond_travel)
                            .build(t),
                    );
                }
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn(
                    Button::new("cam-export-ok")
                        .label("Post")
                        .icon("cam-post")
                        .primary()
                        .build(t),
                );
                f.spawn(Button::new("cam-export-cancel").label("Cancel").build(t));
            })
            .build(&theme),
    ));
    world.flush();
}

/// Closes the Export dialog.
pub fn close(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<ExportDialog>>();
    for e in q.iter(world).collect::<Vec<_>>() {
        world.trigger(DialogClose { entity: e });
    }
    world.remove_resource::<ExportSession>();
}

fn field(w: &mut World, name: &str) -> String {
    let mut q = w.query_filtered::<(&Name, &EditableText), With<cadrs_ui::TextInputField>>();
    q.iter(w)
        .find(|(n, _)| n.as_str() == name)
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default()
}

fn on_select(ev: On<cadrs_ui::SelectChange>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity)
        .is_ok_and(|n| n.as_str() == "cam-export-units")
    {
        let inch = ev.index == 1;
        commands.queue(move |w: &mut World| {
            if let Some(mut s) = w.get_resource_mut::<ExportSession>() {
                s.inch = inch;
            }
            // The preview in the new units.
            let mut q = w.query_filtered::<Entity, With<ExportDialog>>();
            for e in q.iter(w).collect::<Vec<_>>() {
                w.entity_mut(e).despawn();
            }
            open_dialog(w);
        });
    }
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, session: Option<ResMut<ExportSession>>) {
    if let (Ok(n), Some(mut s)) = (q.get(ev.entity), session) {
        match n.as_str() {
            "cam-export-sheet" => s.sheet = ev.checked,
            "cam-export-beyond-travel" => s.beyond_travel = ev.checked,
            _ => {}
        }
    }
}

fn on_activate(ev: On<bevy::ui_widgets::Activate>, q: Query<&Name>, mut commands: Commands) {
    let Ok(n) = q.get(ev.entity) else { return };
    match n.as_str() {
        "cam-export-ok" => commands.queue(post_now),
        "cam-export-cancel" => commands.queue(close),
        "cam-export-browse" => commands.queue(|w: &mut World| {
            let dir = PathBuf::from(field(w, "cam-export-folder-field"));
            let theme = w.resource::<Theme>().clone();
            let mut commands = w.commands();
            cadrs_ui::file_picker::open_folder_picker(
                &mut commands,
                &theme,
                "cam-export-folder-picker",
                "Post to folder",
                "cam-export-folder",
                dir,
            );
            w.flush();
        }),
        _ => {}
    }
}

fn on_folder_picked(mut msgs: MessageReader<cadrs_ui::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag == "cam-export-folder" {
            let text = m.path.display().to_string();
            commands.queue(move |w: &mut World| {
                let mut q = w
                    .query_filtered::<(&Name, &mut EditableText), With<cadrs_ui::TextInputField>>();
                if let Some((_, mut t)) = q
                    .iter_mut(w)
                    .find(|(n, _)| n.as_str() == "cam-export-folder-field")
                {
                    t.queue_edit(bevy::text::TextEdit::SelectAll);
                    t.queue_edit(bevy::text::TextEdit::Insert(text.clone().into()));
                }
            });
        }
    }
}

/// Post: writes the program.
pub fn post_now(world: &mut World) {
    let Some(s) = world.get_resource::<ExportSession>().cloned() else {
        return;
    };
    let name = field(world, "cam-export-name-field").trim().to_string();
    let folder = field(world, "cam-export-folder-field").trim().to_string();
    let number: Option<u32> = field(world, "cam-export-number-field").trim().parse().ok();
    // A changed program number goes into the setup.
    if let Some(n) = number
        && let Some((_, cam)) = active_cam_owned(world)
        && let Some((job, st)) = cam.setup(s.setup)
        && st.program_number != n
    {
        let mut st2 = st.clone();
        st2.program_number = n.clamp(1, 9999);
        super::edit(
            world,
            cadrs_core::cam::Edit::SetSetup {
                job: job.id,
                setup: Box::new(st2),
            },
        );
    }
    let Program {
        nc,
        file: default_name,
        sheet,
        travel,
        ..
    } = match program_text(world, &s) {
        Ok(x) => x,
        Err(e) => {
            toast(world, &e);
            return;
        }
    };
    // Beyond the travel the machine stops on its limits mid-cut, or crashes on a soft limit
    // set wrong: only when the operator has said so.
    if !travel.is_empty() && !s.beyond_travel {
        toast(
            world,
            &format!(
                "{} (tick \"Post anyway\" once you've checked it on the machine)",
                travel[0]
            ),
        );
        return;
    }
    if folder.is_empty() {
        toast(world, "Choose a folder to post to");
        return;
    }
    let file = if name.is_empty() { default_name } else { name };
    let path = PathBuf::from(&folder).join(&file);
    let sheet_path = s.sheet.then(|| {
        let stem = path
            .file_stem()
            .map_or("program".into(), |x| x.to_string_lossy());
        PathBuf::from(&folder).join(format!("{stem}-setup-sheet.html"))
    });
    let r = std::fs::create_dir_all(&folder)
        .and_then(|_| std::fs::write(&path, nc.as_bytes()))
        .and_then(|_| match &sheet_path {
            Some(p) => std::fs::write(p, sheet.as_bytes()),
            None => Ok(()),
        });
    match r {
        Ok(()) => {
            close(world);
            let theme = world.resource::<Theme>().clone();
            let mut commands = world.commands();
            let what = match &sheet_path {
                Some(p) => format!(
                    "Posted {} and its setup sheet {}",
                    path.display(),
                    p.file_name()
                        .map_or(String::new(), |n| n.to_string_lossy().into())
                ),
                None => format!("Posted {}", path.display()),
            };
            show_notification(
                &mut commands,
                &theme,
                Notification::info(what).name("cam-toast"),
            );
            world.flush();
        }
        Err(e) => toast(world, &format!("Couldn't write {}: {e}", path.display())),
    }
}

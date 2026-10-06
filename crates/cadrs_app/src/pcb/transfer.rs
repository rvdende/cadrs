//! The PCB Studio's two MCAD ↔ ECAD dialogs (P3H.5):
//!
//! - **Sync a Part Studio or assembly with PCB Studio** (PCB5.2–5.6, PCB9.6, X8;
//!   `ex1-step10-sync-dialog.png`): "Select part studio or assembly to use:" (the document's
//!   Part Studio and Assembly tabs, "Cell phone (Assembly)"), "Top face of board part is parallel
//!   to:" (Top, Front, Right Plane), OK / Cancel. OK gathers the tab's parts
//!   ([`cadrs_pcb::sync::plan`]) and makes the board on the kernel thread; when it is done the
//!   board goes into the studio (one undo step): a new board named after the tab, or the board
//!   synced from that tab before, updated in place. A toast says which, and lists the parts that
//!   weren't translated ("Not translated: Enclosure, …", PCB5.4). For a board synced before, the
//!   dialog opens on its tab and plane.
//! - **Export ECAD files** (PCB9.7–9.8; `ex1-step17-export-ecad-dialog.png`): "Write to IDF
//!   Version:" IDF 2.0 / IDF 3.0 (3.0 chosen), Export / Cancel. Export writes the shown board as
//!   `<board>.zip` (`<board>/<board>.emn` and `.emp`, [`cadrs_idf::write_zip`]; IDF 2.0 without
//!   keep areas) where other exports go (the Downloads folder; a scenario's `exports` folder),
//!   " (2)" when the name is taken, and a toast says where, with **Open .emn** to read the board
//!   file (PCB6 step 18: the file in a text editor).
//!
//! Names: `pcb-sync-dialog` (`pcb-sync-source`, `pcb-sync-plane`, `pcb-sync-ok`,
//! `pcb-sync-cancel`), `pcb-sync-toast`; `pcb-export-dialog` (`pcb-export-version` with
//! `pcb-export-idf2`, `pcb-export-idf3`; `pcb-export-ok`, `pcb-export-cancel`),
//! `pcb-export-toast` (`pcb-export-open-emn`), `pcb-emn-viewer` (`pcb-emn-find`, `pcb-emn-scroll`,
//! `pcb-emn-line`, `pcb-emn-text`).

use std::path::{Path, PathBuf};

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::pcb::{BoardSource, SyncPlaneChoice};
use cadrs_core::rebuild::PendingJob;
use cadrs_idf::IdfVersion;
use cadrs_pcb::sync::{SyncOutcome, SyncSourceTab};
use cadrs_ui::{Button, Dialog, DialogClose, Notification, RadioGroup, RadioGroupState, Select, SelectState, Theme, show_notification};

use crate::{ActiveDocument, AppClock, AppState};

pub fn register(app: &mut App) {
    app.add_systems(Update, finish_syncs.run_if(in_state(AppState::Document)))
        .add_systems(PostUpdate, flag_syncing)
        .add_observer(on_emn_find);
}

fn close<T: Component>(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<T>>();
    let roots: Vec<Entity> = q.iter(world).collect();
    for e in roots {
        world.trigger(DialogClose { entity: e });
    }
}

fn label(t: &Theme, text: &str) -> impl Bundle {
    (t.text(text, t.font_sm, FontWeight::NORMAL, t.foreground), Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(6.0), Val::Px(4.0)), ..default() })
}

fn toast(world: &mut World, n: Notification) -> Entity {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let e = show_notification(&mut commands, &theme, n);
    world.flush();
    e
}

// ---------------------------------------------------------------------------------------------
// Sync

/// Marks the Sync dialog; holds the tabs it offers.
#[derive(Component)]
struct SyncDialog(Vec<SyncSourceTab>);

/// Opens the Sync dialog (the toolbar's sync button).
pub fn open_sync_dialog(world: &mut World) {
    let mut q = world.query_filtered::<(), With<SyncDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
    let Some((el, s)) = super::active_studio(doc) else { return };
    let tabs = cadrs_pcb::sync::sources(&doc.doc);
    // A board synced before: its tab and plane; else the first assembly.
    let shown = world.resource::<super::PcbUi>().shown_board(el, s);
    let last = shown.and_then(|b| s.board(b)).and_then(|b| match &b.source {
        BoardSource::Mcad(m) => Some((m.element, m.plane)),
        // P3H.6: a board Create assembly was run on: its assembly, Top plane.
        _ => s.generated.iter().rev().find(|g| g.board == b.id).map(|g| (g.assembly, SyncPlaneChoice::Top)),
    });
    let source = last.and_then(|(e, _)| tabs.iter().position(|t| t.element == e)).or_else(|| tabs.iter().position(|t| t.label.ends_with("(Assembly)"))).unwrap_or(0);
    let plane = last.and_then(|(_, p)| SyncPlaneChoice::ALL.iter().position(|q| *q == p)).unwrap_or(0);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let labels: Vec<String> = tabs.iter().map(|t| t.label.clone()).collect();
    world.spawn((
        Dialog::new("pcb-sync-dialog")
            .title("Sync a Part Studio or assembly with PCB Studio")
            .width(440.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(label(t, "Select part studio or assembly to use:"));
                let mut sel = Select::new("pcb-sync-source").bordered().width(Val::Percent(100.0)).selected(source);
                if labels.is_empty() {
                    sel = sel.option("No Part Studio or assembly in this document", false);
                }
                for l in &labels {
                    sel = sel.option(l.clone(), true);
                }
                b.spawn(sel.build(t));
                b.spawn(label(t, "Top face of board part is parallel to:"));
                let mut planes = Select::new("pcb-sync-plane").bordered().width(Val::Percent(100.0)).selected(plane);
                for p in SyncPlaneChoice::ALL {
                    planes = planes.option(p.label(), true);
                }
                b.spawn(planes.build(t));
                b.spawn(Node { height: Val::Px(6.0), ..default() });
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-sync-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept_sync);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-sync-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<SyncDialog>);
                    }),
                ));
            })
            .build(&theme),
        SyncDialog(tabs),
        DespawnOnExit(AppState::Document),
    ));
}

fn select_index(world: &mut World, name: &str) -> usize {
    let mut q = world.query::<(&Name, &SelectState)>();
    q.iter(world).find(|(n, _)| n.as_str() == name).map_or(0, |(_, s)| s.selected)
}

/// Scripted steps wait while a sync runs.
fn flag_syncing(q: Query<(), With<RunningSync>>, mut pending: ResMut<cadrs_ui::PendingWork>, mut why: ResMut<cadrs_ui::PendingWhy>) {
    if !q.is_empty() {
        pending.0 = true;
        why.add("PCB sync");
    }
}

/// A sync on the kernel thread.
#[derive(Component)]
struct RunningSync {
    pending: PendingJob<Result<SyncOutcome, String>>,
}

fn accept_sync(world: &mut World) {
    let tabs = {
        let mut q = world.query::<&SyncDialog>();
        let Some(d) = q.iter(world).next() else { return };
        d.0.clone()
    };
    let (si, pi) = (select_index(world, "pcb-sync-source"), select_index(world, "pcb-sync-plane"));
    close::<SyncDialog>(world);
    let Some(tab) = tabs.get(si) else {
        toast(world, Notification::warning("There is no Part Studio or assembly to sync").name("pcb-sync-toast"));
        return;
    };
    let plane = SyncPlaneChoice::ALL[pi.min(2)];
    start_sync(world, tab.element, plane);
}

/// Starts a sync of the tab `source` into the active PCB Studio (the dialog's OK, or a
/// scenario's `pcb-sync`).
pub fn start_sync(world: &mut World, source: ElementId, plane: SyncPlaneChoice) {
    let plan = {
        let Some(doc) = world.get_resource::<ActiveDocument>() else { return };
        let Some((el, s)) = super::active_studio(doc) else { return };
        let shown = world.resource::<super::PcbUi>().shown_board(el, s);
        cadrs_pcb::sync::plan(&doc.doc, el, source, plane, shown)
    };
    match plan {
        Ok(p) => {
            world.spawn((Name::new("pcb-sync-running"), RunningSync { pending: cadrs_pcb::sync::run(p) }, DespawnOnExit(AppState::Document)));
        }
        Err(e) => {
            toast(world, Notification::warning(format!("Couldn't sync: {e}")).name("pcb-sync-toast").max_width(640.0));
        }
    }
}

/// Puts finished syncs into the studio (one undo step) and says what happened.
fn finish_syncs(q: Query<(Entity, &RunningSync)>, mut commands: Commands) {
    for (e, run) in &q {
        let Some(result) = run.pending.poll() else { continue };
        let result = result.unwrap_or_else(|| Err("The kernel thread stopped".into()));
        commands.entity(e).despawn();
        commands.queue(move |world: &mut World| finish_sync(world, result));
    }
}

fn finish_sync(world: &mut World, result: Result<SyncOutcome, String>) {
    let o = match result {
        Ok(o) => o,
        Err(e) => {
            toast(world, Notification::warning(format!("Couldn't sync: {e}")).name("pcb-sync-toast").max_width(640.0));
            return;
        }
    };
    let el = o.command.element;
    let run = world.resource_mut::<ActiveDocument>().execute(&o.command);
    if let Err(e) = run {
        toast(world, Notification::warning(format!("Couldn't sync: {e}")).name("pcb-sync-toast").max_width(640.0));
        return;
    }
    let name = {
        let doc = world.resource::<ActiveDocument>();
        doc.doc.element(el).and_then(|e| e.pcb()).and_then(|s| s.active_board()).map(|b| (b.id, b.name().to_string()))
    };
    if let Some((id, n)) = name {
        super::show_board(world, el, id);
        let msg = cadrs_pcb::sync::message(&o, &n);
        let note = if o.unrecognised.is_empty() && o.warnings.is_empty() { Notification::info(msg).seconds(4.0) } else { Notification::info(msg).seconds(8.0) };
        toast(world, note.name("pcb-sync-toast").max_width(640.0));
    }
}

// ---------------------------------------------------------------------------------------------
// Export

#[derive(Component)]
struct ExportDialog;

/// Opens Export ECAD files (the toolbar's download button).
pub fn open_export_dialog(world: &mut World) {
    let mut q = world.query_filtered::<(), With<ExportDialog>>();
    if q.iter(world).next().is_some() || super::shown(world).is_none() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    world.spawn((
        Dialog::new("pcb-export-dialog")
            .title("Export ECAD files")
            .width(360.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(label(t, "Write to IDF Version:"));
                b.spawn(RadioGroup::new("pcb-export-version").option("pcb-export-idf2", "IDF 2.0").option("pcb-export-idf3", "IDF 3.0").selected(Some(1)).build(t));
                b.spawn((
                    Name::new("pcb-export-note"),
                    t.text("IDF 2.0 files are written without keep-in and keep-out areas.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node { margin: UiRect::top(Val::Px(6.0)), max_width: Val::Percent(100.0), ..default() },
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("pcb-export-ok").label("Export").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept_export);
                    }),
                ));
                f.spawn((
                    Button::new("pcb-export-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<ExportDialog>);
                    }),
                ));
            })
            .build(&theme),
        ExportDialog,
        DespawnOnExit(AppState::Document),
    ));
}

/// `<stem>.zip` in `dir`, else `<stem> (2).zip`, …
pub fn unique_zip(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.zip"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| dir.join(format!("{stem} ({n}).zip"))).find(|p| !p.exists()).expect("an unused name")
}

/// The IDF header date (`yyyy/mm/dd.hh:mm:ss`) of the app's clock.
fn idf_date(world: &World) -> String {
    let clock = world.get_resource::<AppClock>().copied().unwrap_or_else(AppClock::system);
    let t = clock.now() + clock.utc_offset;
    chrono::DateTime::from_timestamp(t, 0).map(|d| d.format("%Y/%m/%d.%H:%M:%S").to_string()).unwrap_or_else(|| cadrs_idf::DEFAULT_DATE.to_string())
}

fn accept_export(world: &mut World) {
    let version = {
        let mut q = world.query::<(&Name, &RadioGroupState)>();
        let i = q.iter(world).find(|(n, _)| n.as_str() == "pcb-export-version").and_then(|(_, s)| s.selected).unwrap_or(1);
        if i == 0 { IdfVersion::V2 } else { IdfVersion::V3 }
    };
    close::<ExportDialog>(world);
    export_shown(world, version);
}

/// Writes the shown board as `<board>.zip` (see the module docs) and says where.
pub fn export_shown(world: &mut World, version: IdfVersion) {
    let Some((_, _, b)) = super::shown(world) else { return };
    let mut board = b.board.clone();
    board.header.date = idf_date(world);
    let dir = world.get_resource::<crate::ExportDirOverride>().and_then(|d| d.0.clone()).or_else(cadrs_core::export::default_dir);
    let Some(dir) = dir else {
        toast(world, Notification::warning("Nowhere to save the export").name("pcb-export-toast"));
        return;
    };
    let bytes = cadrs_idf::write_zip(&board, &b.library, version);
    let path = unique_zip(&dir, &cadrs_idf::safe_file_name(&board.name));
    let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, &bytes));
    if let Err(e) = written {
        toast(world, Notification::warning(format!("Export failed: {}: {e}", path.display())).name("pcb-export-toast").max_width(640.0));
        return;
    }
    info!("exported {}", path.display());
    let emn = cadrs_idf::zip_entries(&bytes).ok().and_then(|e| e.into_iter().find(|(n, _)| n.ends_with(".emn"))).map(|(n, d)| (n, String::from_utf8_lossy(&d).into_owned()));
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
    let note = Notification::info(format!("Exported {file} (IDF {}) to {}", version.as_str(), dir.display())).seconds(10.0).name("pcb-export-toast").max_width(640.0);
    let t = toast(world, note);
    if let Some((name, text)) = emn {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        let open = cadrs_ui::toast_action(&mut commands, &theme, t, "pcb-export-open-emn", "Open .emn");
        let file = name.rsplit('/').next().unwrap_or(&name).to_string();
        commands.entity(open).insert(observe(move |_: On<Activate>, mut commands: Commands| {
            let (file, text) = (file.clone(), text.clone());
            commands.queue(move |world: &mut World| {
                cadrs_ui::close_toasts(world);
                open_emn_viewer(world, &file, &text);
            });
        }));
        world.flush();
    }
}

#[derive(Component)]
struct EmnViewer;

/// The viewer's scrolled list of lines.
#[derive(Component)]
struct EmnScroll;

/// A line of the viewer (its index).
#[derive(Component)]
struct EmnLine(usize);

/// Height of a line in the viewer (px).
const EMN_LINE: f32 = 16.0;

/// A read-only view of an exported board file (PCB6 step 18's text editor) with a **Find**
/// field (PCB10 step 14: search for the moved component): Enter highlights the first line that
/// contains the text (ignoring case) and scrolls it into view.
pub fn open_emn_viewer(world: &mut World, file: &str, text: &str) {
    close::<EmnViewer>(world);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let lines: Vec<String> = text.lines().map(str::to_string).collect();
    world.spawn((
        Dialog::new("pcb-emn-viewer")
            .title(file.to_string())
            .width(560.0)
            .body(move |b| {
                let t = &tb;
                b.spawn(cadrs_ui::TextInput::new("pcb-emn-find").placeholder("Find").width(Val::Px(220.0)).height(26.0).build(t))
                    .entry::<Node>()
                    .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(6.0)));
                b.spawn((
                    Name::new("pcb-emn-scroll"),
                    EmnScroll,
                    Node {
                        max_height: Val::Px(460.0),
                        overflow: Overflow::scroll_y(),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(8.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    ScrollPosition::default(),
                    BackgroundColor(Color::WHITE),
                    BorderColor::all(t.panel_border),
                ))
                .with_children(|c| {
                    for (i, l) in lines.iter().enumerate() {
                        c.spawn((
                            Name::new("pcb-emn-line"),
                            EmnLine(i),
                            Node { height: Val::Px(EMN_LINE), flex_shrink: 0.0, padding: UiRect::horizontal(Val::Px(2.0)), ..default() },
                            BackgroundColor(Color::NONE),
                        ))
                        .with_child((Name::new("pcb-emn-text"), t.text(if l.is_empty() { " ".to_string() } else { l.clone() }, 12.0, FontWeight::NORMAL, t.foreground)));
                    }
                });
            })
            .footer(move |f| {
                f.spawn((
                    Button::new("pcb-emn-close").label("Close").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close::<EmnViewer>);
                    }),
                ));
            })
            .build(&theme),
        EmnViewer,
        DespawnOnExit(AppState::Document),
    ));
}

/// Enter in the viewer's Find field.
fn on_emn_find(ev: On<cadrs_ui::TextSubmit>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == "pcb-emn-find-field") {
        let term = ev.value.clone();
        commands.queue(move |w: &mut World| find_in_emn(w, &term));
    }
}

/// Highlights the first line containing `term` (ignoring case) and scrolls it into view (a few
/// lines below the top).
pub fn find_in_emn(world: &mut World, term: &str) {
    let term = term.trim().to_lowercase();
    let hit = {
        let mut q = world.query::<(&EmnLine, &Children)>();
        let mut texts = world.query::<&Text>();
        let mut found = None;
        let mut rows: Vec<(usize, Entity)> = q.iter(world).map(|(l, c)| (l.0, c[0])).collect();
        rows.sort();
        for (i, child) in rows {
            if !term.is_empty() && texts.get(world, child).is_ok_and(|t| t.0.to_lowercase().contains(&term)) {
                found = Some(i);
                break;
            }
        }
        found
    };
    let highlight = Color::srgb_u8(0x9c, 0xc8, 0xf5);
    let mut q = world.query::<(&EmnLine, &mut BackgroundColor)>();
    for (l, mut bg) in q.iter_mut(world) {
        bg.set_if_neq(BackgroundColor(if Some(l.0) == hit { highlight } else { Color::NONE }));
    }
    if let Some(i) = hit {
        let mut q = world.query_filtered::<&mut ScrollPosition, With<EmnScroll>>();
        for mut sp in q.iter_mut(world) {
            sp.0.y = (i as f32 - 4.0).max(0.0) * EMN_LINE;
        }
    }
}

//! Updates from GitHub releases: the documents page's top bar shows the version, and when a newer
//! release is out, an "Update available" button beside it. The button opens a dialog with the
//! release notes and an Upgrade button; upgrading shows the download's progress (MB/s, the time
//! left), then "Installing…" and "Launching…" while the new version starts.
//!
//! - The check asks GitHub for the latest release once, on a thread of its own, when the app
//!   starts. Only windowed runs check ([`CheckForUpdates`], inserted by `cadrs`'s `main`):
//!   scenarios, headless runs and tests never touch the network. `CADRS_NO_UPDATE_CHECK=1` turns
//!   it off.
//! - The upgrade replaces this install in place: the release archive for this platform
//!   (`cadrs-v<version>-x86_64-linux.tar.gz` or `…-windows.zip`, see `tools/release/package.sh`)
//!   is downloaded and unpacked beside the executable, then the executable and `libraries/` are
//!   swapped for the new ones and the app restarts. The running executable is renamed rather
//!   than overwritten (Windows can't delete it while it runs); the next start removes it.
//! - A build that isn't an install (run from a checkout's `target/`, with no `libraries/` beside
//!   it), or a platform with no release archive, gets a link to the release page instead. macOS
//!   is one for now: its release is a disk image of a signed app bundle (#103), and swapping files
//!   inside the bundle would break its signature, so the new disk image is downloaded from the
//!   page and dragged over the old app.
//! - The browser build (`tools/web`) is deployed with the site, so it is always the latest: it
//!   never checks.

// The check and the install are unused in a page.
#![cfg_attr(target_arch = "wasm32", allow(dead_code, unused_imports))]

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;
use web_time::Instant;

use bevy::prelude::*;
use bevy::text::{FontWeight, Justify, LineBreak};
use bevy::ui_widgets::ScrollArea;
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Dialog, Progress, ProgressBar};

const LATEST_URL: &str = "https://api.github.com/repos/rvdende/cadrs/releases/latest";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// The release archives are about 80 MB; anything past this isn't one.
const MAX_ARCHIVE: u64 = 1 << 30;
/// How long "Launching…" shows before this instance exits, the new one already started.
const LAUNCH_DELAY: Duration = Duration::from_millis(1200);

pub struct UpdatePlugin;

impl Plugin for UpdatePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UpdateStatus>()
            .add_systems(Startup, (remove_old_executable, start_check))
            .add_systems(
                Update,
                (
                    poll_job,
                    sync_update_slot,
                    sync_dialog_footer,
                    sync_progress,
                    exit_after_launch,
                )
                    .chain(),
            );
    }
}

/// Inserted for runs that look for a newer release.
#[derive(Resource)]
pub struct CheckForUpdates;

/// A newer release than this build.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    /// "0.1.8", without the tag's "v".
    pub version: String,
    /// The release's page on GitHub.
    pub page: String,
    /// The release notes (Markdown).
    pub notes: String,
    /// This platform's archive, when there is one and this build can be replaced in place.
    pub archive: Option<Archive>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Archive {
    pub url: String,
    /// Bytes, as GitHub lists it (0 when it doesn't).
    pub size: u64,
}

#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub enum UpdateStatus {
    /// Not checked yet, or no newer release.
    #[default]
    UpToDate,
    Available(Release),
    Downloading {
        release: Release,
        done: u64,
        total: u64,
        started: Instant,
    },
    /// Unpacking the release and swapping it in.
    Installing(Release),
    /// The new version is starting; this one exits at the instant.
    Launching(Instant),
}

impl UpdateStatus {
    pub(crate) fn release(&self) -> Option<&Release> {
        match self {
            Self::Available(r) | Self::Installing(r) | Self::Downloading { release: r, .. } => {
                Some(r)
            }
            Self::UpToDate | Self::Launching(_) => None,
        }
    }

    /// Which footer the dialog shows; it is rebuilt when this changes.
    fn phase(&self) -> u8 {
        match self {
            Self::UpToDate | Self::Available(_) => 0,
            Self::Downloading { .. } => 1,
            Self::Installing(_) => 2,
            Self::Launching(_) => 3,
        }
    }
}

enum Msg {
    Checked(Result<Option<Release>, String>),
    /// Bytes downloaded so far, of the total (0: unknown).
    Progress(u64, u64),
    /// Downloaded; unpacking and swapping.
    Installing,
    Installed(Result<PathBuf, String>),
}

/// The thread checking or upgrading.
#[derive(Resource)]
struct UpdateJob(Mutex<Receiver<Msg>>);

/// The top bar's slot for the update button, filled by [`sync_update_slot`].
#[derive(Component)]
struct UpdateSlot;

/// The top bar's update button once a newer release is out, then the version: the same in the
/// landing page's and the document's bars. Both fold into the account menu on a narrow window
/// (`crate::top_bar`).
pub fn spawn_version(r: &mut ChildSpawnerCommands, t: &Theme) {
    let fold = cadrs_ui::HideBelow(crate::top_bar::Fold::Version.below());
    r.spawn((
        Name::new("update-slot"),
        UpdateSlot,
        Node {
            align_items: AlignItems::Center,
            margin: UiRect::right(Val::Px(8.0)),
            ..default()
        },
        fold,
    ));
    r.spawn((
        Name::new("version"),
        t.text(
            concat!("v", env!("CARGO_PKG_VERSION")),
            t.font_xs,
            FontWeight::NORMAL,
            t.muted_foreground,
        ),
        fold,
    ))
    .entry::<Node>()
    .and_modify(|mut n| n.margin = UiRect::right(Val::Px(10.0)));
}

/// The release notes dialog.
#[derive(Component)]
struct UpdateDialog;

/// The dialog's footer, filled by [`sync_dialog_footer`] for the status's phase.
#[derive(Component)]
struct UpdateFooter {
    phase: Option<u8>,
}

/// The download's "12.3 of 75.0 MB · 4.1 MB/s · 15 s left".
#[derive(Component)]
struct ProgressLabel;

#[cfg(not(target_arch = "wasm32"))]
fn agent(timeout: u64) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(timeout)))
        .user_agent(concat!(
            "cadrs/",
            env!("CARGO_PKG_VERSION"),
            " (+https://github.com/rvdende/cadrs)"
        ))
        .build()
        .into()
}

fn spawn_job(commands: &mut Commands, work: impl FnOnce(&Sender<Msg>) -> Msg + Send + 'static) {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let last = work(&tx);
        let _ = tx.send(last);
    });
    commands.insert_resource(UpdateJob(Mutex::new(rx)));
}

fn start_check(mut commands: Commands, check: Option<Res<CheckForUpdates>>) {
    if check.is_none() || std::env::var_os("CADRS_NO_UPDATE_CHECK").is_some_and(|v| v != "0") {
        return;
    }
    spawn_job(&mut commands, |_| Msg::Checked(latest_release()));
}

fn poll_job(
    mut commands: Commands,
    job: Option<Res<UpdateJob>>,
    mut status: ResMut<UpdateStatus>,
    theme: Res<Theme>,
) {
    let Some(job) = job else {
        return;
    };
    let Ok(rx) = job.0.lock() else {
        return;
    };
    loop {
        let msg = match rx.try_recv() {
            Ok(msg) => msg,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                commands.remove_resource::<UpdateJob>();
                return;
            }
        };
        match msg {
            Msg::Checked(Ok(Some(release))) => {
                info!("cadrs v{} is out (this is v{VERSION})", release.version);
                *status = UpdateStatus::Available(release);
            }
            Msg::Checked(Ok(None)) => {}
            Msg::Checked(Err(e)) => info!("cannot check for updates: {e}"),
            Msg::Progress(d, t) => {
                if let UpdateStatus::Downloading { done, total, .. } = &mut *status {
                    *done = d;
                    *total = t;
                }
            }
            Msg::Installing => {
                if let Some(r) = status.release().cloned() {
                    *status = UpdateStatus::Installing(r);
                }
            }
            Msg::Installed(Ok(exe)) => {
                let args: Vec<_> = std::env::args_os().skip(1).collect();
                match std::process::Command::new(&exe).args(args).spawn() {
                    Ok(_) => *status = UpdateStatus::Launching(Instant::now() + LAUNCH_DELAY),
                    Err(e) => {
                        warn!("cannot start {}: {e}", exe.display());
                        *status = UpdateStatus::UpToDate;
                        cadrs_ui::show_toast(
                            &mut commands,
                            &theme,
                            "Updated. Restart cadrs to use the new version.",
                        );
                    }
                }
            }
            Msg::Installed(Err(e)) => {
                warn!("update failed: {e}");
                if let Some(r) = status.release().cloned() {
                    *status = UpdateStatus::Available(r);
                }
                cadrs_ui::show_notification(
                    &mut commands,
                    &theme,
                    cadrs_ui::Notification::error(format!("Update failed: {e}")),
                );
            }
        }
    }
}

fn exit_after_launch(status: Res<UpdateStatus>, mut exit: MessageWriter<AppExit>) {
    if let UpdateStatus::Launching(at) = *status
        && Instant::now() >= at
    {
        exit.write(AppExit::Success);
    }
}

/// Fills the top bar's [`UpdateSlot`] when it is spawned or a newer release turns up.
fn sync_update_slot(
    mut commands: Commands,
    status: Res<UpdateStatus>,
    theme: Res<Theme>,
    slots: Query<(Entity, Option<&Children>), With<UpdateSlot>>,
) {
    let t = &*theme;
    let want = !matches!(*status, UpdateStatus::UpToDate);
    for (slot, children) in &slots {
        if children.is_some_and(|c| !c.is_empty()) == want {
            continue;
        }
        commands.entity(slot).despawn_children();
        if want {
            commands.entity(slot).with_child((
                Button::new("update-button")
                    .label("Update available")
                    .icon("download")
                    .link()
                    .small()
                    .tooltip("Click to view release notes")
                    .build(t),
                observe(
                    |_: On<Activate>,
                     mut commands: Commands,
                     status: Res<UpdateStatus>,
                     theme: Res<Theme>,
                     open: Query<(), With<UpdateDialog>>| {
                        if open.is_empty()
                            && let Some(r) = status.release()
                        {
                            open_dialog(&mut commands, &theme, r);
                        }
                    },
                ),
            ));
        }
    }
}

/// Opens the newer release's notes (the bell's notice), unless they are open.
pub(crate) fn show_release_notes(world: &mut World) {
    let open = world
        .query_filtered::<(), With<UpdateDialog>>()
        .iter(world)
        .next()
        .is_some();
    let Some(release) = world.resource::<UpdateStatus>().release().cloned() else {
        return;
    };
    if open {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    open_dialog(&mut commands, &theme, &release);
    world.flush();
}

fn open_dialog(commands: &mut Commands, theme: &Theme, release: &Release) {
    let t = theme.clone();
    let tb = theme.clone();
    let notes = release.notes.clone();
    let version = release.version.clone();
    commands.spawn((
        Dialog::new("update-dialog")
            .title(format!("cadrs v{version} is available"))
            .width(640.0)
            .body(move |b| {
                b.spawn(tb.text(
                    format!("You have v{VERSION}."),
                    tb.font_base,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ));
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|pane| {
                    let list = pane
                        .spawn((
                            Name::new("update-notes"),
                            ScrollArea,
                            cadrs_ui::scrollbar::ScrollGutter(16.0),
                            Node {
                                flex_direction: FlexDirection::Column,
                                max_height: Val::Px(420.0),
                                row_gap: Val::Px(6.0),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|n| release_notes(n, &tb, &notes))
                        .id();
                    pane.spawn(cadrs_ui::vertical_scrollbar(
                        &tb,
                        "update-notes-scrollbar",
                        list,
                    ));
                });
            })
            .footer(|f| {
                f.spawn((
                    Name::new("update-footer"),
                    UpdateFooter { phase: None },
                    Node {
                        flex_grow: 1.0,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    },
                ));
            })
            .build(&t),
        UpdateDialog,
    ));
}

/// Fills the dialog's footer for the status: the Upgrade button, the progress, or what is
/// happening.
fn sync_dialog_footer(
    mut commands: Commands,
    status: Res<UpdateStatus>,
    theme: Res<Theme>,
    mut footers: Query<(Entity, &mut UpdateFooter)>,
) {
    let t = &*theme;
    let phase = status.phase();
    for (e, mut footer) in &mut footers {
        if footer.phase == Some(phase) {
            continue;
        }
        footer.phase = Some(phase);
        commands.entity(e).despawn_children();
        let note = |text: &'static str| {
            (
                t.text(text, t.font_base, FontWeight::NORMAL, t.muted_foreground),
                Node {
                    flex_grow: 1.0,
                    ..default()
                },
            )
        };
        commands.entity(e).with_children(|f| match &*status {
            UpdateStatus::UpToDate => {}
            UpdateStatus::Available(r) if r.archive.is_some() => {
                f.spawn(note("Downloads and installs the new version."));
                close_button(f, t);
                f.spawn((
                    Button::new("update-upgrade")
                        .label("Upgrade")
                        .primary()
                        .build(t),
                    observe(
                        |_: On<Activate>,
                         mut commands: Commands,
                         mut status: ResMut<UpdateStatus>| {
                            let UpdateStatus::Available(r) = status.clone() else {
                                return;
                            };
                            let Some(archive) = r.archive.clone() else {
                                return;
                            };
                            *status = UpdateStatus::Downloading {
                                release: r,
                                done: 0,
                                total: archive.size,
                                started: Instant::now(),
                            };
                            spawn_job(&mut commands, move |tx| {
                                Msg::Installed(install(&archive, tx))
                            });
                        },
                    ),
                ));
            }
            UpdateStatus::Available(r) => {
                // Why there is no Upgrade: no archive for this platform, or a build run from
                // a checkout (no `libraries/` beside it), which an upgrade mustn't overwrite.
                f.spawn(note(if archive_name(&r.version).is_none() {
                    "There is no download for this platform; build it from source."
                } else {
                    "Built from source: update with git pull and cargo install-local."
                }));
                close_button(f, t);
                let page = r.page.clone();
                f.spawn((
                    Button::new("update-page")
                        .label("Open release page")
                        .primary()
                        .build(t),
                    observe(move |_: On<Activate>| open_url(&page)),
                ));
            }
            UpdateStatus::Downloading { .. } => {
                f.spawn(Progress::new("update-progress").build(t));
                f.spawn((
                    ProgressLabel,
                    t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                    Node {
                        min_width: Val::Px(250.0),
                        ..default()
                    },
                ))
                .insert(TextLayout::new(Justify::Right, LineBreak::NoWrap));
            }
            UpdateStatus::Installing(_) => {
                f.spawn(Progress::new("update-progress").value(1.0).build(t));
                f.spawn(t.text(
                    "Installing\u{2026}",
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.foreground,
                ));
            }
            UpdateStatus::Launching(_) => {
                f.spawn(Progress::new("update-progress").value(1.0).build(t));
                f.spawn(t.text(
                    "Launching\u{2026}",
                    t.font_sm,
                    FontWeight::NORMAL,
                    t.foreground,
                ));
            }
        });
    }
}

fn close_button(f: &mut ChildSpawnerCommands, t: &Theme) {
    f.spawn((
        Button::new("update-close").label("Close").build(t),
        observe(
            |_: On<Activate>,
             dialogs: Query<Entity, With<UpdateDialog>>,
             mut commands: Commands| {
                for d in &dialogs {
                    commands.trigger(cadrs_ui::DialogClose { entity: d });
                }
            },
        ),
    ));
}

/// Moves the download's bar and label along.
fn sync_progress(
    status: Res<UpdateStatus>,
    mut bars: Query<&mut ProgressBar>,
    mut labels: Query<&mut Text, With<ProgressLabel>>,
) {
    let UpdateStatus::Downloading {
        done,
        total,
        started,
        ..
    } = *status
    else {
        return;
    };
    if !status.is_changed() {
        return;
    }
    let value = if total > 0 {
        done as f32 / total as f32
    } else {
        0.0
    };
    for mut b in &mut bars {
        b.value = value;
    }
    let text = progress_text(done, total, started.elapsed().as_secs_f64());
    for mut l in &mut labels {
        l.0.clone_from(&text);
    }
}

/// "12.3 of 75.0 MB · 4.1 MB/s · 15 s left".
fn progress_text(done: u64, total: u64, secs: f64) -> String {
    const MB: f64 = 1_000_000.0;
    let rate = if secs > 0.0 { done as f64 / secs } else { 0.0 };
    let mut s = if total > 0 {
        format!("{:.1} of {:.1} MB", done as f64 / MB, total as f64 / MB)
    } else {
        format!("{:.1} MB", done as f64 / MB)
    };
    if rate > 0.0 && secs >= 0.5 {
        s += &format!(" \u{b7} {:.1} MB/s", rate / MB);
        if total > done {
            s += &format!(
                " \u{b7} {} left",
                duration_text((total - done) as f64 / rate)
            );
        }
    }
    s
}

fn duration_text(secs: f64) -> String {
    let s = secs.ceil() as u64;
    if s < 60 {
        format!("{s} s")
    } else {
        format!("{} min {} s", s / 60, s % 60)
    }
}

// ---------------------------------------------------------------------------------------------
// Release notes

/// Lays out GitHub's Markdown release notes: headings, bullets (nested by indent), tables and
/// paragraphs, with **bold** runs; links show their text, `code` its contents.
fn release_notes(p: &mut ChildSpawner, t: &Theme, md: &str) {
    let lines: Vec<&str> = md.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        i += 1;
        if trimmed.is_empty() {
            continue;
        }
        if let Some(h) = trimmed.strip_prefix('#') {
            let h = h.trim_start_matches('#').trim();
            let mut e = p.spawn(Node {
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            });
            e.with_children(|r| rich_text(r, t, h, t.font_lg, FontWeight::SEMIBOLD));
            continue;
        }
        if trimmed.starts_with('|') {
            let mut rows = vec![table_cells(trimmed)];
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(table_cells(lines[i].trim()));
                i += 1;
            }
            table(p, t, rows);
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            p.spawn(Node {
                padding: UiRect::left(Val::Px(4.0 + 8.0 * indent as f32)),
                column_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|r| {
                r.spawn(t.text(
                    "\u{2022}",
                    t.font_base,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ));
                rich_text(r, t, item, t.font_base, FontWeight::NORMAL);
            });
            continue;
        }
        p.spawn(Node::default())
            .with_children(|r| rich_text(r, t, trimmed, t.font_base, FontWeight::NORMAL));
    }
}

/// "| a | b |" → ["a", "b"].
fn table_cells(line: &str) -> Vec<String> {
    line.trim_matches('|')
        .split('|')
        .map(|c| c.trim().to_string())
        .collect()
}

/// A Markdown table: the first row is the header, the "|---|" row under it goes. Each column
/// gets a share of the width by its longest cell, the same in every row, so the cells line up.
fn table(p: &mut ChildSpawner, t: &Theme, rows: Vec<Vec<String>>) {
    let separator = |r: &Vec<String>| {
        r.iter()
            .all(|c| c.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
    };
    let header = rows.len() > 1 && separator(&rows[1]);
    let rows: Vec<_> = rows.into_iter().filter(|r| !separator(r)).collect();
    let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
    let shares: Vec<f32> = (0..columns)
        .map(|c| {
            let longest = rows
                .iter()
                .filter_map(|r| r.get(c))
                .map(|s| s.chars().count())
                .max()
                .unwrap_or(0);
            longest.clamp(4, 60) as f32
        })
        .collect();
    p.spawn((
        Name::new("notes-table"),
        Node {
            flex_direction: FlexDirection::Column,
            margin: UiRect::vertical(Val::Px(4.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius)),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|tbl| {
        for (ri, row) in rows.iter().enumerate() {
            let head = header && ri == 0;
            let mut r = tbl.spawn(Node {
                border: UiRect::top(Val::Px(if ri == 0 { 0.0 } else { 1.0 })),
                ..default()
            });
            r.insert(BorderColor::all(t.separator));
            if head {
                r.insert(BackgroundColor(t.secondary));
            }
            r.with_children(|cells| {
                for (c, share) in shares.iter().enumerate() {
                    cells
                        .spawn((
                            Node {
                                flex_grow: *share,
                                flex_basis: Val::Px(0.0),
                                min_width: Val::Px(0.0),
                                // A column, so the text wraps to the cell's width and the
                                // row grows to its lines.
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::axes(Val::Px(8.0), Val::Px(5.0)),
                                border: UiRect::left(Val::Px(if c == 0 { 0.0 } else { 1.0 })),
                                ..default()
                            },
                            BorderColor::all(t.separator),
                        ))
                        .with_children(|cell| {
                            let text = row.get(c).map(String::as_str).unwrap_or_default();
                            let weight = if head {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::NORMAL
                            };
                            rich_text_in(cell, t, text, t.font_base, weight, Node::default());
                        });
                }
            });
        }
    });
}

/// A wrapping text whose **bold** runs are spans of their own, taking the rest of its row.
fn rich_text(p: &mut ChildSpawner, t: &Theme, md: &str, size: f32, weight: FontWeight) {
    rich_text_in(
        p,
        t,
        md,
        size,
        weight,
        Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_width: Val::Px(0.0),
            ..default()
        },
    );
}

/// A wrapping text whose **bold** runs are spans of their own, laid out by `node`.
fn rich_text_in(
    p: &mut ChildSpawner,
    t: &Theme,
    md: &str,
    size: f32,
    weight: FontWeight,
    node: Node,
) {
    let mut runs = inline_runs(md);
    // The root holds the first run, the spans the rest.
    let (first, first_bold) = if runs.is_empty() {
        (String::new(), false)
    } else {
        runs.remove(0)
    };
    let mut text = p.spawn((
        t.text(
            first,
            size,
            if first_bold {
                FontWeight::SEMIBOLD
            } else {
                weight
            },
            t.foreground,
        ),
        node,
    ));
    text.insert(TextLayout::new(Justify::Left, LineBreak::WordBoundary));
    text.with_children(|s| {
        for (run, bold) in runs {
            let w = if bold { FontWeight::SEMIBOLD } else { weight };
            s.spawn((TextSpan(run), t.font(size, w), TextColor(t.foreground)));
        }
    });
}

/// Splits Markdown inline text into (text, bold) runs: `**` toggles bold, backticks go, and a
/// `[text](url)` link keeps its text.
fn inline_runs(md: &str) -> Vec<(String, bool)> {
    let mut plain = String::new();
    let mut rest = md;
    while let Some(i) = rest.find('[') {
        plain.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        if let Some(j) = after.find("](")
            && let Some(k) = after[j + 2..].find(')')
        {
            plain.push_str(&after[..j]);
            rest = &after[j + 2 + k + 1..];
        } else {
            plain.push('[');
            rest = after;
        }
    }
    plain.push_str(rest);
    let plain = plain.replace('`', "");
    plain
        .split("**")
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .map(|(i, s)| (s.to_string(), i % 2 == 1))
        .collect()
}

fn open_url(url: &str) {
    let mut cmd = if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else {
        std::process::Command::new("xdg-open")
    };
    if let Err(e) = cmd.arg(url).spawn() {
        warn!("cannot open {url}: {e}");
    }
}

// ---------------------------------------------------------------------------------------------
// The check

/// "0.1.10" → [0, 1, 10]; whatever follows a "-" or "+" (a pre-release) is left out.
fn version_key(v: &str) -> Vec<u64> {
    let v = v.trim().trim_start_matches('v');
    let v = v.split(['-', '+']).next().unwrap_or_default();
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

fn newer(latest: &str, current: &str) -> bool {
    version_key(latest) > version_key(current)
}

/// This platform's release archive name, as `tools/release/package.sh` names it.
fn archive_name(version: &str) -> Option<String> {
    let ext = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux.tar.gz",
        ("windows", "x86_64") => "windows.zip",
        // macOS (`cadrs-v<version>-universal-macos.dmg`): the release page (see the module docs).
        _ => return None,
    };
    Some(format!("cadrs-v{version}-x86_64-{ext}"))
}

/// The folder this build can be updated in: an install has `libraries/` beside the executable
/// (`cadrs --install`, or an unpacked release archive); a checkout's `target/release/` has not.
fn install_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.to_path_buf();
    cadrs_fs::is_dir(dir.join("libraries")).then_some(dir)
}

/// Reads GitHub's answer for the latest release: the newer release, if it is newer.
fn parse_release(json: &serde_json::Value, current: &str, in_place: bool) -> Option<Release> {
    let version = json["tag_name"]
        .as_str()?
        .trim_start_matches('v')
        .to_string();
    if !newer(&version, current) {
        return None;
    }
    let name = archive_name(&version);
    let archive = json["assets"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|a| name.is_some() && a["name"].as_str() == name.as_deref())
        .and_then(|a| {
            Some(Archive {
                url: a["browser_download_url"].as_str()?.to_string(),
                size: a["size"].as_u64().unwrap_or(0),
            })
        })
        .filter(|_| in_place);
    Some(Release {
        page: json["html_url"]
            .as_str()
            .unwrap_or("https://github.com/rvdende/cadrs/releases")
            .to_string(),
        notes: json["body"].as_str().unwrap_or_default().to_string(),
        version,
        archive,
    })
}

#[cfg(target_arch = "wasm32")]
fn latest_release() -> Result<Option<Release>, String> {
    Ok(None)
}

#[cfg(not(target_arch = "wasm32"))]
fn latest_release() -> Result<Option<Release>, String> {
    let mut r = agent(20)
        .get(LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value =
        serde_json::from_str(&r.body_mut().read_to_string().map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    Ok(parse_release(&json, VERSION, install_dir().is_some()))
}

// ---------------------------------------------------------------------------------------------
// The install

fn err(what: &str, p: &Path, e: impl std::fmt::Display) -> String {
    format!("cannot {what} {}: {e}", p.display())
}

/// The running executable renamed out of the way by an update.
fn old_exe(exe: &Path) -> PathBuf {
    exe.with_extension("old")
}

/// Removes what the last update left: the executable it replaced (still running then).
fn remove_old_executable() {
    if let Ok(exe) = std::env::current_exe() {
        let old = old_exe(&exe);
        if cadrs_fs::is_file(&old) {
            let _ = cadrs_fs::remove_file(old);
        }
    }
}

/// Downloads the archive, unpacks it beside the executable and swaps it in, telling `tx` how far
/// it got. Returns the new executable.
fn install(archive: &Archive, tx: &Sender<Msg>) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("cannot find this executable: {e}"))?;
    let dir = install_dir().ok_or("this build is not an install")?;
    let staging = dir.join("update.new");
    if cadrs_fs::exists(&staging) {
        cadrs_fs::remove_dir_all(&staging).map_err(|e| err("remove", &staging, e))?;
    }
    cadrs_fs::create_dir_all(&staging).map_err(|e| err("create", &staging, e))?;
    let result = download_and_swap(archive, tx, &exe, &dir, &staging);
    let _ = cadrs_fs::remove_dir_all(&staging);
    result.map(|()| exe)
}

fn download_and_swap(
    from: &Archive,
    tx: &Sender<Msg>,
    exe: &Path,
    dir: &Path,
    staging: &Path,
) -> Result<(), String> {
    let archive = staging.join("archive");
    download(from, tx, &archive)?;
    let _ = tx.send(Msg::Installing);
    let unpacked = staging.join("unpacked");
    unpack(&archive, &unpacked)?;
    // The archive holds one folder, `cadrs-v<version>-x86_64-<platform>/`.
    let root = cadrs_fs::read_dir(&unpacked)
        .map_err(|e| err("read", &unpacked, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| cadrs_fs::is_dir(p))
        .ok_or("the archive holds no folder")?;
    let exe_name = exe.file_name().ok_or("no executable name")?;
    let new_exe = root.join(format!("cadrs{}", std::env::consts::EXE_SUFFIX));
    let new_libraries = root.join("libraries");
    if !cadrs_fs::is_file(&new_exe) || !cadrs_fs::is_dir(&new_libraries) {
        return Err("the archive has no cadrs executable or libraries".into());
    }
    swap(
        exe,
        &dir.join(exe_name),
        &new_exe,
        &dir.join("libraries"),
        &new_libraries,
    )?;
    // The rest (README.md, the licenses) is nice to have.
    for f in ["README.md", "LICENSE-APACHE", "LICENSE-MIT"] {
        if cadrs_fs::is_file(root.join(f)) {
            let _ = cadrs_fs::copy(root.join(f), dir.join(f));
        }
    }
    Ok(())
}

#[cfg(target_arch = "wasm32")]
fn download(_: &Archive, _: &Sender<Msg>, _: &Path) -> Result<(), String> {
    Err("a page is updated with the site".into())
}

/// Downloads the archive to `to`, telling `tx` the bytes so far about ten times a second.
#[cfg(not(target_arch = "wasm32"))]
fn download(from: &Archive, tx: &Sender<Msg>, to: &Path) -> Result<(), String> {
    let mut r = agent(1800)
        .get(&from.url)
        .call()
        .map_err(|e| e.to_string())?;
    let total = r
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok()?.parse().ok())
        .unwrap_or(from.size);
    let mut body = r.body_mut().with_config().limit(MAX_ARCHIVE).reader();
    let mut f = cadrs_fs::File::create(to).map_err(|e| err("create", to, e))?;
    let mut buf = vec![0u8; 1 << 16];
    let mut done = 0u64;
    let mut told = Instant::now();
    loop {
        let n = body
            .read(&mut buf)
            .map_err(|e| format!("download failed: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| err("write", to, e))?;
        done += n as u64;
        if told.elapsed() >= Duration::from_millis(100) {
            told = Instant::now();
            let _ = tx.send(Msg::Progress(done, total));
        }
    }
    let _ = tx.send(Msg::Progress(done, total));
    if total > 0 && done != total {
        return Err(format!("download stopped at {done} of {total} bytes"));
    }
    Ok(())
}

fn unpack(archive: &Path, to: &Path) -> Result<(), String> {
    let f = cadrs_fs::File::open(archive).map_err(|e| err("open", archive, e))?;
    #[cfg(not(any(windows, target_arch = "wasm32")))]
    return tar::Archive::new(flate2::read::GzDecoder::new(f))
        .unpack(to)
        .map_err(|e| err("unpack", archive, e));
    // Windows' archive is a zip (and a page never gets this far).
    #[cfg(any(windows, target_arch = "wasm32"))]
    zip::ZipArchive::new(f)
        .and_then(|mut z| z.extract(to))
        .map_err(|e| err("unpack", archive, e))
}

/// Moves the new executable and libraries in, keeping the old ones until both are in place.
fn swap(
    running: &Path,
    exe: &Path,
    new_exe: &Path,
    libraries: &Path,
    new_libraries: &Path,
) -> Result<(), String> {
    let old = old_exe(running);
    let old_libraries = libraries.with_extension("old");
    for p in [&old, &old_libraries] {
        if cadrs_fs::is_dir(p) {
            cadrs_fs::remove_dir_all(p).map_err(|e| err("remove", p, e))?;
        } else if cadrs_fs::exists(p) {
            cadrs_fs::remove_file(p).map_err(|e| err("remove", p, e))?;
        }
    }
    cadrs_fs::rename(running, &old).map_err(|e| err("move", running, e))?;
    if let Err(e) = cadrs_fs::rename(new_exe, exe) {
        let _ = cadrs_fs::rename(&old, running);
        return Err(err("move", new_exe, e));
    }
    if let Err(e) = cadrs_fs::rename(libraries, &old_libraries)
        .map_err(|e| err("move", libraries, e))
        .and_then(|()| {
            cadrs_fs::rename(new_libraries, libraries).map_err(|e| err("move", new_libraries, e))
        })
    {
        // Put everything back.
        let _ = cadrs_fs::rename(&old_libraries, libraries);
        let _ = cadrs_fs::remove_file(exe);
        let _ = cadrs_fs::rename(&old, running);
        return Err(e);
    }
    let _ = cadrs_fs::remove_dir_all(&old_libraries);
    if !cfg!(windows) {
        // Not running from it any more; Windows keeps it until the next start.
        let _ = cadrs_fs::remove_file(&old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_by_number() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v0.2.0", "0.1.7"));
        assert!(newer("1.0", "0.9.9"));
        assert!(!newer("0.1.7", "0.1.7"));
        assert!(!newer("0.1.6", "0.1.7"));
        assert!(!newer("0.1.7-rc1", "0.1.7"));
    }

    fn release_json(tag: &str) -> serde_json::Value {
        let names = [
            format!("cadrs-{tag}-x86_64-linux.tar.gz"),
            format!("cadrs-{tag}-x86_64-windows.zip"),
            "SHA256SUMS.txt".to_string(),
        ];
        serde_json::json!({
            "tag_name": tag,
            "html_url": format!("https://github.com/rvdende/cadrs/releases/tag/{tag}"),
            "body": "## What's new",
            "assets": names.iter().map(|n| serde_json::json!({
                "name": n,
                "browser_download_url": format!("https://example.com/{n}"),
            })).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn a_newer_release_names_this_platforms_archive() {
        let r = parse_release(&release_json("v0.2.0"), "0.1.7", true).unwrap();
        assert_eq!(r.version, "0.2.0");
        assert_eq!(
            r.page,
            "https://github.com/rvdende/cadrs/releases/tag/v0.2.0"
        );
        let want = archive_name("0.2.0").map(|n| format!("https://example.com/{n}"));
        assert_eq!(r.archive.map(|a| a.url), want);
        assert_eq!(r.notes, "## What's new");
    }

    #[test]
    fn a_build_that_is_not_an_install_gets_only_the_page() {
        let r = parse_release(&release_json("v0.2.0"), "0.1.7", false).unwrap();
        assert_eq!(r.archive, None);
    }

    #[test]
    fn the_same_or_an_older_release_is_no_update() {
        assert_eq!(parse_release(&release_json("v0.1.7"), "0.1.7", true), None);
        assert_eq!(parse_release(&release_json("v0.1.6"), "0.1.7", true), None);
    }

    #[test]
    fn inline_markdown_keeps_bold_runs_and_link_text() {
        assert_eq!(
            inline_runs("**Fillets.** See [the docs](https://x.y) and `cadrs --open`."),
            vec![
                ("Fillets.".to_string(), true),
                (" See the docs and cadrs --open.".to_string(), false),
            ]
        );
    }

    #[test]
    fn table_rows_split_into_cells() {
        assert_eq!(table_cells("| Platform | File |"), vec!["Platform", "File"]);
        assert_eq!(table_cells("|---|:--:|"), vec!["---", ":--:"]);
    }

    #[test]
    fn progress_reads_in_megabytes_with_the_time_left() {
        assert_eq!(
            progress_text(10_000_000, 80_000_000, 2.0),
            "10.0 of 80.0 MB \u{b7} 5.0 MB/s \u{b7} 14 s left"
        );
        assert_eq!(progress_text(0, 80_000_000, 0.0), "0.0 of 80.0 MB");
        assert_eq!(duration_text(75.2), "1 min 16 s");
    }

    #[test]
    fn the_swap_replaces_the_executable_and_libraries() {
        let dir = std::env::temp_dir().join(format!("cadrs-update-test-{}", std::process::id()));
        let _ = cadrs_fs::remove_dir_all(&dir);
        let new = dir.join("new");
        cadrs_fs::create_dir_all(dir.join("libraries")).unwrap();
        cadrs_fs::create_dir_all(new.join("libraries")).unwrap();
        cadrs_fs::write(dir.join("cadrs"), "old").unwrap();
        cadrs_fs::write(dir.join("libraries/a"), "old").unwrap();
        cadrs_fs::write(new.join("cadrs"), "new").unwrap();
        cadrs_fs::write(new.join("libraries/b"), "new").unwrap();
        let exe = dir.join("cadrs");
        swap(
            &exe,
            &exe,
            &new.join("cadrs"),
            &dir.join("libraries"),
            &new.join("libraries"),
        )
        .unwrap();
        assert_eq!(cadrs_fs::read_to_string(&exe).unwrap(), "new");
        assert!(dir.join("libraries/b").is_file());
        assert!(!dir.join("libraries/a").exists());
        assert!(!dir.join("libraries.old").exists());
        let _ = cadrs_fs::remove_dir_all(&dir);
    }
}

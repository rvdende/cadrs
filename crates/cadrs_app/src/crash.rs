//! Crash reports (issue #319): what lets a person tell us about a crash, and us reproduce it.
//!
//! A person's runs (not scenarios or headless runs) keep, in the data dir:
//! - `logs/cadrs-<session>.log`: the session's log (what the terminal shows), for every session,
//!   so one started from a file manager or a launcher has one too. The ten newest are kept.
//! - `logs/session-<session>.lock`: the running session, locked while it runs and removed when
//!   it closes normally. It holds what was going on (version, GPU, the open document and tab),
//!   rewritten as that changes. The system unlocks it when the process ends however it ends, so
//!   one found unlocked at the next start is a session that ended without closing: a panic, or a
//!   crash outside Rust (a GPU driver's, say) that no panic hook sees.
//! - `crashes/<session>/panic.txt`: every panic of the session (message, place, thread,
//!   backtrace), written by the panic hook. (A caught panic, a kernel task's say, is written too:
//!   it helps explain a crash that follows.)
//!
//! At the next start, the session found unlocked gets `crashes/<session>/report.txt` (what was
//! going on, its panics and the end of its log) and a copy of its log, and the app opens the
//! *cadrs closed unexpectedly* dialog: report it on GitHub (a new issue, filled in), show the
//! files to attach, copy the report, or add a copy of the document that was open. Nothing is
//! sent anywhere without the person. The app menu's *Report a problem…* opens the same dialog
//! for the running session.
//!
//! In a page (the browser build) there are no files: *Report a problem…* opens a new issue.

use bevy::prelude::*;
use bevy::text::{FontWeight, Justify, LineBreak};
use bevy::ui_widgets::Activate;
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, Dialog};

/// The page where people report problems.
const NEW_ISSUE: &str = "https://github.com/rvdende/cadrs/issues/new";
const VERSION: &str = env!("CARGO_PKG_VERSION");

pub struct CrashPlugin;

impl Plugin for CrashPlugin {
    fn build(&self, app: &mut App) {
        #[cfg(not(target_arch = "wasm32"))]
        app.add_systems(Update, (native::track_context, native::open_pending));
        let _ = app;
    }
}

/// A report the dialog shows: the files it is in and what to say about it.
#[derive(Clone, Debug)]
pub struct Report {
    /// The folder holding `report.txt`, the log and (if added) the document.
    pub dir: std::path::PathBuf,
    /// The whole report (`report.txt`).
    pub text: String,
    /// The first lines of the panic, or why there is none.
    pub summary: String,
    /// Whether the session crashed (else: the person asked to report a problem).
    pub crashed: bool,
    /// The document that was open, if any: its id and name.
    pub document: Option<(cadrs_core::ids::DocumentId, String)>,
}

#[derive(Component)]
struct ReportDialog;

/// Opens *Report a problem…* for the running session (the app menu's item).
pub fn report_problem(world: &mut World) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let Some(session) = native::SESSION.get() else {
            crate::open_url(&issue_url(&issue_title(None), &issue_body(None)));
            return;
        };
        match session.report_now() {
            Ok(r) => open_dialog(world, r),
            Err(e) => {
                warn!("cannot write the report: {e}");
                crate::open_url(&issue_url(&issue_title(None), &issue_body(None)));
            }
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = world;
        crate::open_url(&issue_url(&issue_title(None), &issue_body(None)));
    }
}

/// Opens the dialog on a sample report (the scenarios' `crash-dialog` step).
pub fn open_sample(world: &mut World) {
    let summary = "index out of bounds: the len is 3 but the index is 7\n\
                   at crates/cadrs_app/src/drawing/views.rs:371:17\n\
                   thread: Compute Task Pool (2)\n\
                   12: cadrs_app::drawing::views::drive_view_cache"
        .to_string();
    let dir = std::env::temp_dir().join("cadrs-sample-crash");
    open_dialog(
        world,
        Report {
            dir,
            text: format!("cadrs: {VERSION}\nGPU: Sample GPU (Vulkan)\n\n{summary}\n"),
            summary,
            crashed: true,
            document: Some((
                cadrs_core::ids::DocumentId::from_u128(1),
                "Untitled document".into(),
            )),
        },
    );
}

/// The new issue's title.
fn issue_title(r: Option<&Report>) -> String {
    match r {
        Some(r) if r.crashed => {
            let first = r.summary.lines().next().unwrap_or("").trim();
            let first: String = first.chars().take(80).collect();
            if first.is_empty() {
                "Crash".into()
            } else {
                format!("Crash: {first}")
            }
        }
        _ => String::new(),
    }
}

/// The new issue's text: what happened (for the person to fill in), the version and system,
/// and the panic. Short enough for a link; the files carry the rest.
fn issue_body(r: Option<&Report>) -> String {
    let mut b = String::from("**What happened, and what were you doing?**\n\n\n\n");
    b.push_str(&format!(
        "**cadrs** {VERSION} on {} {}\n",
        std::env::consts::OS,
        std::env::consts::ARCH
    ));
    if let Some(r) = r {
        if let Some(gpu) = r.text.lines().find_map(|l| l.strip_prefix("GPU: ")) {
            b.push_str(&format!("**GPU** {gpu}\n"));
        }
        if r.crashed {
            let summary: String = r.summary.chars().take(1500).collect();
            b.push_str(&format!("\n```\n{}\n```\n", summary.trim_end()));
        }
        b.push_str(
            "\nPlease attach `report.txt` and the log from the folder cadrs showed \
             (drag them into this box), and the document if you added it.\n",
        );
    }
    b
}

/// A link to a new issue with `title` and `body` filled in.
fn issue_url(title: &str, body: &str) -> String {
    format!(
        "{NEW_ISSUE}?title={}&body={}",
        percent_encode(title),
        percent_encode(body)
    )
}

/// `s` with everything but RFC 3986's unreserved characters percent-encoded (UTF-8).
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn open_dialog(world: &mut World, report: Report) {
    let open = world
        .query_filtered::<Entity, With<ReportDialog>>()
        .iter(world)
        .collect::<Vec<_>>();
    for e in open {
        world
            .commands()
            .trigger(cadrs_ui::DialogClose { entity: e });
    }
    let theme = world.resource::<Theme>().clone();
    let (t, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    let (title, intro) = if report.crashed {
        (tr("crash-title"), tr("crash-intro"))
    } else {
        (tr("crash-report-title"), tr("crash-report-intro"))
    };
    let summary = report.summary.clone();
    let crashed = report.crashed;
    let r_footer = report.clone();
    world.commands().spawn((
        Dialog::new("crash-dialog")
            .title(title)
            .width(640.0)
            .body(move |b| {
                b.spawn(tb.text(intro, tb.font_base, FontWeight::NORMAL, tb.foreground))
                    .insert(TextLayout::new(Justify::Left, LineBreak::WordBoundary));
                if crashed {
                    b.spawn((
                        Name::new("crash-summary"),
                        Node {
                            padding: UiRect::all(Val::Px(8.0)),
                            border_radius: BorderRadius::all(Val::Px(tb.radius)),
                            max_height: Val::Px(200.0),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(tb.secondary),
                    ))
                    .with_children(|c| {
                        c.spawn(tb.text(summary, tb.font_sm, FontWeight::NORMAL, tb.foreground))
                            .insert(TextLayout::new(Justify::Left, LineBreak::AnyCharacter));
                    });
                }
                b.spawn(tb.text(
                    tr("crash-privacy"),
                    tb.font_sm,
                    FontWeight::NORMAL,
                    tb.muted_foreground,
                ))
                .insert(TextLayout::new(Justify::Left, LineBreak::WordBoundary));
            })
            .footer(move |f| {
                // Wrapped: the buttons are wider than a narrow window. (The ✕ and Escape close
                // the dialog.)
                f.spawn(Node {
                    flex_grow: 1.0,
                    flex_wrap: FlexWrap::Wrap,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    row_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|f| footer(f, &tf, r_footer));
            })
            .build(&t),
        ReportDialog,
    ));
    world.flush();
}

fn footer(f: &mut ChildSpawner, t: &Theme, report: Report) {
    let dir = report.dir.clone();
    f.spawn((
        Button::new("crash-show-files")
            .label(tr("crash-show-files"))
            .icon("folder")
            .build(t),
        observe(move |_: On<Activate>| crate::open_url(&dir.display().to_string())),
    ));
    let text = report.text.clone();
    f.spawn((
        Button::new("crash-copy")
            .label(tr("crash-copy"))
            .icon("copy")
            .build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            let text = text.clone();
            commands.queue(move |world: &mut World| {
                let copied = world
                    .get_resource_mut::<bevy::clipboard::Clipboard>()
                    .is_some_and(|mut c| c.set_text(text).is_ok());
                notify(
                    world,
                    if copied {
                        cadrs_ui::Notification::info(tr("crash-copied"))
                    } else {
                        cadrs_ui::Notification::warning(tr("crash-copy-failed"))
                    },
                );
            });
        }),
    ));
    #[cfg(not(target_arch = "wasm32"))]
    if let Some((id, name)) = report.document.clone() {
        let dir = report.dir.clone();
        f.spawn((
            Button::new("crash-add-document")
                .label(tr("crash-add-document"))
                .icon("file")
                .tooltip(tr_with(
                    "crash-add-document-tooltip",
                    &[("name", name.as_str())],
                ))
                .build(t),
            observe(move |_: On<Activate>, mut commands: Commands| {
                let (dir, name) = (dir.clone(), name.clone());
                commands.queue(move |world: &mut World| {
                    let note = match native::add_document(world, &dir, id, &name) {
                        Ok(path) => cadrs_ui::Notification::info(tr_with(
                            "crash-document-added",
                            &[("path", &path.display().to_string())],
                        )),
                        Err(e) => cadrs_ui::Notification::warning(tr_with(
                            "crash-document-failed",
                            &[("error", &e)],
                        )),
                    };
                    notify(world, note);
                });
            }),
        ));
    }
    #[cfg(target_arch = "wasm32")]
    let _ = tr_with;
    f.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    let url = issue_url(&issue_title(Some(&report)), &issue_body(Some(&report)));
    let dir = report.dir.clone();
    f.spawn((
        Button::new("crash-report-github")
            .label(tr("crash-report-github"))
            .icon("github")
            .primary()
            .build(t),
        observe(move |_: On<Activate>| {
            // The files to attach, beside the new issue.
            crate::open_url(&dir.display().to_string());
            crate::open_url(&url);
        }),
    ));
}

fn notify(world: &mut World, note: cadrs_ui::Notification) {
    let theme = world.resource::<Theme>().clone();
    cadrs_ui::show_notification(&mut world.commands(), &theme, note.name("crash-toast"));
    world.flush();
}

#[cfg(not(target_arch = "wasm32"))]
pub use native::{Session, clean_exit, start};

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::fs::{self, File, OpenOptions};
    use std::io::{Read, Seek, SeekFrom, Write};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex, OnceLock};

    use bevy::log::BoxedLayer;
    use cadrs_core::ids::DocumentId;

    /// Sessions' logs and crash folders kept.
    const KEEP: usize = 10;
    /// The log lines a report ends with.
    const LOG_TAIL: usize = 200;

    pub(super) static SESSION: OnceLock<Session> = OnceLock::new();

    /// What was going on: written to the session's lock file as it changes.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub(super) struct Context {
        pub version: String,
        pub started: String,
        pub gpu: String,
        pub document: Option<(DocumentId, String)>,
        pub tab: String,
    }

    impl Context {
        fn write(&self) -> String {
            let one = |s: &str| s.replace(['\n', '\r'], " ");
            let mut s = format!(
                "version={}\nstarted={}\ngpu={}\ntab={}\n",
                one(&self.version),
                one(&self.started),
                one(&self.gpu),
                one(&self.tab)
            );
            if let Some((id, name)) = &self.document {
                s.push_str(&format!("document={id}\ndocument_name={}\n", one(name)));
            }
            s
        }

        fn parse(s: &str) -> Self {
            let mut c = Context::default();
            let mut doc = None;
            let mut doc_name = String::new();
            for l in s.lines() {
                let Some((k, v)) = l.split_once('=') else {
                    continue;
                };
                let v = v.to_string();
                match k {
                    "version" => c.version = v,
                    "started" => c.started = v,
                    "gpu" => c.gpu = v,
                    "tab" => c.tab = v,
                    "document" => doc = uuid::Uuid::parse_str(&v).ok().map(DocumentId),
                    "document_name" => doc_name = v,
                    _ => {}
                }
            }
            c.document = doc.map(|d| (d, doc_name));
            c
        }

        /// The report's first lines.
        fn header(&self) -> String {
            let mut s = format!(
                "cadrs: {}\nOS: {} {}\nGPU: {}\nStarted: {}\n",
                self.version,
                std::env::consts::OS,
                std::env::consts::ARCH,
                if self.gpu.is_empty() {
                    "unknown"
                } else {
                    &self.gpu
                },
                self.started
            );
            if let Some((id, name)) = &self.document {
                s.push_str(&format!("Document: {name} ({id})\n"));
            }
            if !self.tab.is_empty() {
                s.push_str(&format!("Tab: {}\n", self.tab));
            }
            s
        }
    }

    /// The running session's files (a person's runs only).
    #[derive(Resource, Clone)]
    pub struct Session(Arc<Inner>);

    pub(super) struct Inner {
        id: String,
        logs: PathBuf,
        crashes: PathBuf,
        log: Arc<File>,
        lock: Mutex<File>,
        context: Mutex<Context>,
    }

    /// A session found ended without closing, for the dialog at start.
    #[derive(Resource)]
    pub(super) struct PendingReport(pub Report);

    /// Starts the session's files in data dir `root`, installs the panic hook and returns the
    /// session (and, as a resource for the app, the report of a session that crashed). `None`
    /// when the files can't be made: the app runs without them.
    pub fn start(root: &Path, app: &mut App) -> Option<Session> {
        let logs = root.join("logs");
        let crashes = root.join("crashes");
        if let Err(e) = fs::create_dir_all(&logs).and_then(|_| fs::create_dir_all(&crashes)) {
            eprintln!("cannot keep a log in {}: {e}", logs.display());
            return None;
        }
        let previous = ended_sessions(&logs);
        let now = chrono::Local::now();
        let id = format!("{}-{}", now.format("%Y%m%d-%H%M%S"), std::process::id());
        let log = File::create(logs.join(format!("cadrs-{id}.log"))).ok()?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(true)
            .read(true)
            .write(true)
            .open(lock_path(&logs, &id))
            .ok()?;
        if lock.try_lock().is_err() {
            return None;
        }
        let context = Context {
            version: VERSION.into(),
            started: now.format("%Y-%m-%d %H:%M:%S %z").to_string(),
            ..default()
        };
        let session = Session(Arc::new(Inner {
            id,
            logs: logs.clone(),
            crashes: crashes.clone(),
            log: Arc::new(log),
            lock: Mutex::new(lock),
            context: Mutex::new(Context::default()),
        }));
        session.set_context(context);
        let _ = SESSION.set(session.clone());
        install_panic_hook();
        // The newest session that ended without closing gets the report; older ones (several
        // crashes without a start between, which can't happen, or a lock left by a copy of the
        // data dir) only lose their lock.
        if let Some((id, context)) = previous.into_iter().max_by(|a, b| a.0.cmp(&b.0)) {
            match write_report(&logs, &crashes, &id, &context, true) {
                Ok(r) => {
                    app.insert_resource(PendingReport(r));
                }
                Err(e) => eprintln!("cannot write the crash report: {e}"),
            }
        }
        prune(&logs, "cadrs-", KEEP);
        prune(&crashes, "", KEEP);
        app.insert_resource(session.clone());
        Some(session)
    }

    /// The session closed normally: its lock file goes.
    pub fn clean_exit() {
        if let Some(s) = SESSION.get() {
            if let Ok(f) = s.0.lock.lock() {
                let _ = f.unlock();
            }
            let _ = fs::remove_file(lock_path(&s.0.logs, &s.0.id));
        }
    }

    fn lock_path(logs: &Path, id: &str) -> PathBuf {
        logs.join(format!("session-{id}.lock"))
    }

    /// The sessions whose lock files are left and unlocked (their processes are gone), with what
    /// was going on; their lock files are removed.
    fn ended_sessions(logs: &Path) -> Vec<(String, Context)> {
        let Ok(dir) = fs::read_dir(logs) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for e in dir.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let Some(id) = name
                .strip_prefix("session-")
                .and_then(|n| n.strip_suffix(".lock"))
            else {
                continue;
            };
            let Ok(mut f) = OpenOptions::new().read(true).write(true).open(e.path()) else {
                continue;
            };
            // Locked: that session is running (another window of cadrs).
            if f.try_lock().is_err() {
                continue;
            }
            let mut s = String::new();
            let _ = f.read_to_string(&mut s);
            let _ = f.unlock();
            drop(f);
            let _ = fs::remove_file(e.path());
            out.push((id.to_string(), Context::parse(&s)));
        }
        out
    }

    /// Writes `crashes/<id>/report.txt` (what was going on, the panics, the log's end) and a copy
    /// of the log beside it.
    fn write_report(
        logs: &Path,
        crashes: &Path,
        id: &str,
        context: &Context,
        crashed: bool,
    ) -> std::io::Result<Report> {
        let dir = crashes.join(id);
        fs::create_dir_all(&dir)?;
        let log_path = logs.join(format!("cadrs-{id}.log"));
        let log = fs::read_to_string(&log_path).unwrap_or_default();
        let panics = fs::read_to_string(dir.join("panic.txt")).unwrap_or_default();
        let summary = if !crashed {
            String::new()
        } else if panics.trim().is_empty() {
            tr("crash-no-panic")
        } else {
            summarize(&panics)
        };
        let mut text = context.header();
        if crashed {
            text.push_str("\n== Panics ==\n");
            text.push_str(if panics.trim().is_empty() {
                "None recorded: the process ended outside Rust's panics (a GPU driver's crash, \
                 an abort, or it was killed).\n"
            } else {
                &panics
            });
        }
        text.push_str(&format!("\n== The log's last {LOG_TAIL} lines ==\n"));
        let lines: Vec<&str> = log.lines().collect();
        for l in &lines[lines.len().saturating_sub(LOG_TAIL)..] {
            text.push_str(l);
            text.push('\n');
        }
        fs::write(dir.join("report.txt"), &text)?;
        if !log.is_empty() {
            fs::write(dir.join(format!("cadrs-{id}.log")), &log)?;
        }
        Ok(Report {
            dir,
            text,
            summary,
            crashed,
            document: context.document.clone(),
        })
    }

    /// The first panic's message and place, and its backtrace's first frames in cadrs's code.
    pub(super) fn summarize(panics: &str) -> String {
        let mut out = Vec::new();
        let mut frames = 0;
        let mut in_trace = false;
        for l in panics.lines() {
            if l.starts_with("== Panic") && !out.is_empty() {
                break;
            }
            if l.starts_with("Backtrace:") {
                in_trace = true;
                continue;
            }
            if !in_trace {
                if !l.starts_with("== Panic") && !l.trim().is_empty() {
                    out.push(l.to_string());
                }
            } else if frames < 6 && is_cadrs_frame(l) {
                let l = l.trim();
                out.push(if l.chars().count() > 160 {
                    format!("{}…", l.chars().take(160).collect::<String>())
                } else {
                    l.to_string()
                });
                frames += 1;
            }
        }
        out.join("\n")
    }

    /// A backtrace's frame line (`  12: name`, not its `at file:line`) in cadrs's own code, other
    /// than the panic hook's.
    fn is_cadrs_frame(l: &str) -> bool {
        let l = l.trim_start();
        let Some((n, name)) = l.split_once(": ") else {
            return false;
        };
        // `cadrs_app::f`, `<cadrs_app::T as …>::f`, `<&mut <cadrs_app::T as …>::f …>`.
        let name = name.trim_start_matches(['<', '&']);
        let name = name.strip_prefix("mut ").unwrap_or(name);
        let name = name.trim_start_matches('<');
        n.chars().all(|c| c.is_ascii_digit())
            && name.starts_with("cadrs")
            && !name.contains("::crash::")
    }

    /// Keeps the `keep` newest entries of `dir` whose names start with `prefix` (names sort by
    /// time: they begin with the session id).
    fn prune(dir: &Path, prefix: &str, keep: usize) {
        let Ok(rd) = fs::read_dir(dir) else {
            return;
        };
        let mut names: Vec<(String, PathBuf)> = rd
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().to_string();
                (n.starts_with(prefix) && !n.ends_with(".lock")).then(|| (n, e.path()))
            })
            .collect();
        names.sort();
        let n = names.len().saturating_sub(keep);
        for (_, p) in names.into_iter().take(n) {
            let _ = if p.is_dir() {
                fs::remove_dir_all(&p)
            } else {
                fs::remove_file(&p)
            };
        }
    }

    impl Session {
        fn set_context(&self, c: Context) {
            let Ok(mut cur) = self.0.context.lock() else {
                return;
            };
            if *cur == c {
                return;
            }
            *cur = c;
            if let Ok(mut f) = self.0.lock.lock() {
                let _ = f.set_len(0);
                let _ = f.seek(SeekFrom::Start(0));
                let _ = f.write_all(cur.write().as_bytes());
            }
        }

        /// The report of the running session (*Report a problem…*).
        pub(super) fn report_now(&self) -> std::io::Result<Report> {
            let context = self.0.context.lock().map(|c| c.clone()).unwrap_or_default();
            write_report(&self.0.logs, &self.0.crashes, &self.0.id, &context, false)
        }

        /// The tracing layer that writes the session's log.
        pub fn log_layer(&self) -> BoxedLayer {
            use bevy::log::tracing_subscriber::fmt;
            Box::new(
                fmt::layer()
                    .with_ansi(false)
                    .with_writer(self.0.log.clone()),
            )
        }
    }

    fn install_panic_hook() {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if let Some(s) = SESSION.get() {
                let _ = write_panic(s, info);
            }
            previous(info);
        }));
    }

    fn write_panic(s: &Session, info: &std::panic::PanicHookInfo) -> std::io::Result<()> {
        let msg = if let Some(m) = info.payload().downcast_ref::<&str>() {
            (*m).to_string()
        } else if let Some(m) = info.payload().downcast_ref::<String>() {
            m.clone()
        } else {
            "(no message)".into()
        };
        let place = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());
        let thread = std::thread::current();
        let trace = std::backtrace::Backtrace::force_capture();
        let dir = s.0.crashes.join(&s.0.id);
        fs::create_dir_all(&dir)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("panic.txt"))?;
        write!(
            f,
            "== Panic at {} ==\n{msg}\nat {place}\nthread: {}\nBacktrace:\n{trace}\n\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            thread.name().unwrap_or("unnamed"),
        )
    }

    /// Keeps the session's lock file saying what is going on: the GPU, the open document and
    /// its tab.
    pub(super) fn track_context(
        session: Option<Res<Session>>,
        gpu: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
        doc: Option<Res<crate::ActiveDocument>>,
        mut had_doc: Local<bool>,
    ) {
        let Some(session) = session else {
            return;
        };
        if !(doc.as_ref().is_some_and(|d| d.is_changed())
            || gpu.as_ref().is_some_and(|g| g.is_changed())
            || *had_doc != doc.is_some())
        {
            return;
        }
        *had_doc = doc.is_some();
        let Ok(mut c) = session.0.context.lock().map(|c| c.clone()) else {
            return;
        };
        if let Some(g) = &gpu {
            c.gpu = format!(
                "{} ({:?}, {} {})",
                g.name, g.backend, g.driver, g.driver_info
            );
        }
        match &doc {
            Some(d) => {
                c.document = d.meta.as_ref().map(|_| (d.doc.id, d.doc.name.clone()));
                c.tab = d
                    .active_element()
                    .map(|e| e.name.clone())
                    .unwrap_or_default();
            }
            None => {
                c.document = None;
                c.tab.clear();
            }
        }
        session.set_context(c);
    }

    /// Opens the dialog for a session that crashed, once the app shows the documents page or a
    /// document.
    pub(super) fn open_pending(world: &mut World) {
        if !world.contains_resource::<PendingReport>() || !world.contains_resource::<Theme>() {
            return;
        }
        let Some(r) = world.remove_resource::<PendingReport>() else {
            return;
        };
        open_dialog(world, r.0);
    }

    /// Writes the stored document `id` into `dir` as a `.cadrs` package.
    pub(super) fn add_document(
        world: &mut World,
        dir: &Path,
        id: DocumentId,
        name: &str,
    ) -> Result<PathBuf, String> {
        let store = &world.resource::<crate::DocumentStore>().0;
        let file = store.load(id).map_err(|e| e.to_string())?;
        let path =
            cadrs_core::export::unique_path_ext(dir, &cadrs_core::export::sanitize(name), "cadrs");
        cadrs_core::native::export(&path, &file.document, &file.meta, store)?;
        Ok(path)
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn context_round_trips() {
            let c = Context {
                version: "0.2.9".into(),
                started: "2026-11-24 17:43:02 +0200".into(),
                gpu: "AMD (Vulkan, radv\nMesa)".into(),
                document: Some((DocumentId::new(), "Untitled = 1".into())),
                tab: "Drawing 2".into(),
            };
            let back = Context::parse(&c.write());
            assert_eq!(back.document, c.document);
            assert_eq!(back.gpu, "AMD (Vulkan, radv Mesa)");
            assert_eq!(back.tab, c.tab);
        }

        #[test]
        fn summary_is_the_message_and_cadrs_frames() {
            let p = "== Panic at x ==\nindex out of bounds\nat crates/a.rs:1:2\nthread: main\n\
                     Backtrace:\n   0: cadrs_app::crash::native::install_panic_hook::{closure#0}\n\
                     \x20  1: std::panicking\n   4: <&mut <cadrs_app::T as Tr>::f as Fn>::call\n\
                     \x20  5: cadrs_app::drawing::views::foo\n\
                     \x20            at ./crates/cadrs_app/src/drawing/views.rs:10:5\n\n\
                     == Panic at y ==\nsecond\n";
            let s = summarize(p);
            assert!(s.starts_with("index out of bounds\nat crates/a.rs:1:2\nthread: main"));
            assert!(s.contains("4: <&mut <cadrs_app::T"), "{s}");
            assert!(s.ends_with("5: cadrs_app::drawing::views::foo"), "{s}");
            assert!(!s.contains("second"));
        }

        #[test]
        fn the_document_that_was_open_is_added_as_a_package() {
            use cadrs_core::{DocumentMeta, store::Store};
            let root = std::env::temp_dir().join(format!("cadrs-crash-{}", uuid::Uuid::new_v4()));
            let store = Store::new(root.join("docs"));
            let doc = cadrs_core::Document::new("Two drawings");
            store.create(&doc, &DocumentMeta::new("me", 1_000)).unwrap();
            let mut world = World::new();
            world.insert_resource(crate::DocumentStore(store));
            let dir = root.join("crash");
            fs::create_dir_all(&dir).unwrap();
            let path = add_document(&mut world, &dir, doc.id, "Two drawings").unwrap();
            assert_eq!(path, dir.join("Two drawings.cadrs"));
            assert!(fs::metadata(&path).unwrap().len() > 0);
            assert!(add_document(&mut world, &dir, DocumentId::new(), "Gone").is_err());
            let _ = fs::remove_dir_all(&root);
        }

        #[test]
        fn a_session_left_unlocked_is_reported_and_a_locked_one_is_not() {
            let root = std::env::temp_dir().join(format!("cadrs-crash-{}", uuid::Uuid::new_v4()));
            let logs = root.join("logs");
            fs::create_dir_all(&logs).unwrap();
            // A session that ended without closing, and one still running (locked).
            fs::write(
                lock_path(&logs, "20260101-000000-1"),
                "version=0.1\ntab=A\n",
            )
            .unwrap();
            let running = File::create(lock_path(&logs, "20260101-000001-2")).unwrap();
            running.lock().unwrap();
            let ended = ended_sessions(&logs);
            assert_eq!(ended.len(), 1);
            assert_eq!(ended[0].0, "20260101-000000-1");
            assert_eq!(ended[0].1.tab, "A");
            assert!(!lock_path(&logs, "20260101-000000-1").exists());
            assert!(lock_path(&logs, "20260101-000001-2").exists());
            drop(running);
            let _ = fs::remove_dir_all(&root);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_links_are_encoded() {
        assert_eq!(percent_encode("a b&c=ü"), "a%20b%26c%3D%C3%BC");
        let r = Report {
            dir: "/x".into(),
            text: "GPU: AMD\n".into(),
            summary: "index out of bounds\nat a.rs:1".into(),
            crashed: true,
            document: None,
        };
        assert_eq!(issue_title(Some(&r)), "Crash: index out of bounds");
        let body = issue_body(Some(&r));
        assert!(body.contains("**GPU** AMD"));
        assert!(body.contains("at a.rs:1"));
        assert!(issue_url("t", &body).starts_with(NEW_ISSUE));
    }
}

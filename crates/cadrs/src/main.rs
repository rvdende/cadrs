//! cadrs: an Onshape-style CAD app.
//!
//! `cadrs [--headless] [--scenario <name>] [--window-size WxH] [--out <dir>] [--data-dir <dir>]
//! [[--open] <file.cadrs>]`
//!
//! `cadrs --list-documents [--data-dir <dir>] [--trash]` lists the stored documents with their ids.
//!
//! `cadrs --headless --jobs N (--scenarios a,b,... | --all)` renders many scenarios, N at a
//! time, each in its own process (see [`batch`]).
//!
//! `cadrs --document <name or id> --check [--headless] [--data-dir <dir>] [--part-studio <name>]
//! [--sketch <name>]` rebuilds a stored
//! document and reports the features in error or warning, in detail (see [`check`]).
//!
//! `cadrs --public-seed [--data-dir <dir>] [--peer <cadrs ID>]…` runs a seed peer for public
//! documents, with no window (see [`seed`]).
//!
//! `cadrs --kernel-process <port> <token>` is the geometry kernel the app starts for itself
//! (`cadrs_core::rebuild::process`); `CADRS_KERNEL=thread` keeps the kernel in the app instead.
//!
//! Documents live in `--data-dir`, else `CADRS_DATA_DIR`, else the platform data dir
//! (`~/.local/share/cadrs/documents`, `%APPDATA%\cadrs\documents`). Scenarios use a fresh
//! `<out dir>/data` instead, a fixed clock and a fixed user, so their screenshots are
//! reproducible.
//!
//! In a browser (`wasm32`) there are no arguments: [`web`] runs the app in its page.

use bevy::prelude::*;
#[cfg(not(target_arch = "wasm32"))]
use cadrs_app::{AppClock, CadrsAppPlugin, DocumentStore, OpenAtStart, StartState, UserProfile};
#[cfg(not(target_arch = "wasm32"))]
use cadrs_core::Store;
#[cfg(not(target_arch = "wasm32"))]
use cadrs_harness::{HarnessOptions, HarnessPlugin};
#[cfg(not(target_arch = "wasm32"))]
use cadrs_ui::CadrsUiPlugin;

/// "Now" in scenarios: 2026-09-24 15:30 UTC.
#[cfg(not(target_arch = "wasm32"))]
const SCENARIO_NOW: i64 = 1_790_263_800;

#[cfg(not(target_arch = "wasm32"))]
mod batch;
#[cfg(not(target_arch = "wasm32"))]
mod check;
#[cfg(not(target_arch = "wasm32"))]
mod install;
#[cfg(not(target_arch = "wasm32"))]
mod seed;
#[cfg(target_arch = "wasm32")]
mod web;

#[cfg(target_arch = "wasm32")]
fn main() -> AppExit {
    web::run()
}

/// Every way to run cadrs (`-h`, `--help`).
#[cfg(not(target_arch = "wasm32"))]
const HELP: &str = "\
cadrs: an Onshape-style CAD app.

Usage:
  cadrs [--headless] [--scenario <name>] [--window-size WxH] [--out <dir>] [--data-dir <dir>] [--no-cursor]
      Runs the app (with --scenario, a scripted scenario; --headless needs one).
  cadrs [--data-dir <dir>] [--open] <file.cadrs>
      Runs the app with a native document (an export, or a document.ron) imported into the
      library and open.
  cadrs --list-documents [--data-dir <dir>] [--trash]
      Lists the stored documents, newest first: id, last modified (UTC), name.
  cadrs --document <name or id> --check [--data-dir <dir>] [--part-studio <name>] [--sketch <name>]
        [--feature <name>] [--steps] [--dof]
      Rebuilds a stored document without a window and reports its features in error or warning.
  cadrs --headless --jobs N (--scenarios a,b,... | --all)
      Renders many scenarios, N at a time, each in its own process.
  cadrs --public-seed [--data-dir <dir>] [--peer <cadrs ID>]...
      Runs a seed peer for public documents without a window: it stays in the public room and
      keeps every listing, star and published document, for others to start from. Its data dir
      defaults to ~/.local/share/cadrs/seed; it prints its cadrs ID.
  cadrs --install
      Installs this build for the current user, with a launcher entry (Linux, macOS, Windows;
      `cargo install-local`).
  cadrs --bundle <cadrs.app> [--libraries <dir>]
      Makes the macOS app bundle of this build, as --install would, without installing it (the
      macOS release, tools/release/package.sh).

Documents live in --data-dir, else $CADRS_DATA_DIR, else the platform data dir
(~/.local/share/cadrs/documents on Linux).";

#[cfg(not(target_arch = "wasm32"))]
fn main() -> AppExit {
    // `--jobs N` / `--scenarios a,b` / `--all`: many scenarios in parallel child processes.
    let args: Vec<String> = std::env::args().skip(1).collect();
    // The kernel process this executable starts for itself (`cadrs_core::rebuild::process`).
    cadrs_app::register_kernel_tasks();
    if args.first().map(String::as_str) == Some(cadrs_core::rebuild::process::ARG) {
        return AppExit::from_code(cadrs_core::rebuild::process::main(&args[1..]) as u8);
    }
    // The kernel runs in a process of its own, so a crash in it costs a rebuild, not the app
    // (`CADRS_KERNEL=thread` keeps it on a thread of this process, for debugging).
    if std::env::var("CADRS_KERNEL").as_deref() != Ok("thread")
        && let Ok(exe) = std::env::current_exe()
    {
        cadrs_core::rebuild::process::use_process(exe, Vec::new());
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{HELP}");
        return AppExit::Success;
    }
    // `--bundle <app>`: the macOS app bundle of this build (`install.rs`).
    if let Some(i) = args.iter().position(|a| a == "--bundle") {
        let Some(app) = args.get(i + 1) else {
            eprintln!("--bundle needs the bundle's path (cadrs.app)");
            return AppExit::from_code(2);
        };
        let libraries = args
            .iter()
            .position(|a| a == "--libraries")
            .and_then(|k| args.get(k + 1))
            .map(std::path::PathBuf::from);
        return match install::bundle(std::path::Path::new(app), libraries.as_deref()) {
            Ok(()) => {
                println!("Made {app}");
                AppExit::Success
            }
            Err(e) => {
                eprintln!("{e}");
                AppExit::from_code(1)
            }
        };
    }
    // `--install`: installs this build for the user, with a launcher entry (`install.rs`;
    // `--install-desktop` is its older name).
    if args
        .iter()
        .any(|a| a == "--install" || a == "--install-desktop")
    {
        return match install::run() {
            Ok(dir) => {
                println!("Installed cadrs to {}", dir.display());
                AppExit::Success
            }
            Err(e) => {
                eprintln!("{e}");
                AppExit::from_code(1)
            }
        };
    }
    // `--public-seed`: a seed peer for public documents, no window.
    if let Some(s) = seed::parse(&args) {
        return match s {
            Ok(s) => seed::run(s),
            Err(e) => {
                eprintln!("{e}");
                AppExit::from_code(2)
            }
        };
    }
    // `--list-documents`: the store's documents with their ids, no window.
    if let Some(code) = check::list_documents(&args) {
        return AppExit::from_code(code);
    }
    // `--document <name> --check`: a stored document's rebuild issues, no window.
    if let Some(c) = check::parse(&args) {
        return match c {
            Ok(c) => AppExit::from_code(check::run(&c)),
            Err(e) => {
                eprintln!("{e}");
                AppExit::from_code(2)
            }
        };
    }
    if let Some(b) = batch::parse(&args) {
        return match b {
            Ok(b) => AppExit::from_code(batch::run(&b) as u8),
            Err(e) => {
                eprintln!("{e}");
                AppExit::from_code(2)
            }
        };
    }
    let mut opts = match HarnessOptions::from_args(args) {
        Ok(o) => o,
        Err(e) => {
            // The harness's own usage line knows only the app's options: point to all of them.
            eprintln!(
                "{}\ncadrs --help lists every option.",
                e.lines().next().unwrap_or("")
            );
            return AppExit::from_code(2);
        }
    };
    let scenario = match opts.load_scenario() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("cannot load scenario: {e}");
            return AppExit::from_code(2);
        }
    };

    let data_dir = opts.resolved_data_dir_for(scenario.as_ref());
    let scripted = scenario.is_some();
    let mut app = App::new();
    // The frame panel's CPU times: every system's and schedule's tracing span, added up. They
    // cost about a fifth of a frame, so only when wanted: the user's preference (a scenario's
    // own setting). Systems make their spans as they're added, so it's decided before the plugins.
    let profile_cpu = match &scenario {
        Some(s) => s.profile_cpu,
        None => data_dir
            .clone()
            .or_else(Store::default_root)
            .is_some_and(|r| cadrs_core::preferences::Preferences::load(&r).profile_cpu),
    };
    app.insert_resource(cadrs_app::frame_profile::CpuProfiling::at_start(
        cfg!(feature = "profiler"),
        profile_cpu,
    ));
    app.add_plugins(
        cadrs_harness::default_plugins(&opts).set(bevy::log::LogPlugin {
            custom_layer: cadrs_app::frame_profile::profiler_layer,
            ..default()
        }),
    );
    if let Some(path) = &opts.open {
        if !path.is_file() {
            eprintln!("cannot open {}: no such file", path.display());
            return AppExit::from_code(2);
        }
        app.insert_resource(OpenAtStart(path.clone()));
    }
    if let Some(start) = scenario.as_ref().and_then(|s| s.start.clone()) {
        app.insert_resource(StartState(start));
    }
    let data_from = opts.data_from_dir(scenario.as_ref());
    let seed = scenario.as_ref().map_or(0, |s| s.seed_documents);
    // Scenarios and headless runs export into their output folder (a BOM's CSV, P3B.6), never
    // into the user's Downloads (Final part 3: golden runs had written there).
    // Nor do they see or add to the user's own libraries.
    // ($CADRS_USER_LIBRARIES still picks a folder: a run that uses the user's parts, read only.)
    let user_libraries = (scripted || opts.headless).then(|| {
        std::env::var_os("CADRS_USER_LIBRARIES")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| opts.resolved_out_dir().join("libraries"))
    });
    if !scripted && !opts.headless {
        // Only a person's own runs ask GitHub for a newer release.
        app.insert_resource(cadrs_app::update::CheckForUpdates);
    }
    // Sharing and collaboration: only a person's own runs are a peer (`CADRS_NET=0` keeps
    // one off the network for good). The identity lives with the documents, so each data dir
    // is a peer. The identity in use (the account menu switches it) picks the key and the name.
    // The node goes online at once only with the user's *Connect when cadrs starts*
    // (`autoconnect_p2p`, off by default: issue #148) or `CADRS_NET=1`; otherwise when the
    // user connects (the top bar's network button, or sharing).
    let net = std::env::var("CADRS_NET").ok();
    if !scripted && !opts.headless && net.as_deref() != Some("0") {
        let root = data_dir.clone().or_else(Store::default_root);
        let ids = root.as_deref().map(cadrs_app::identity::load);
        let autoconnect = root
            .as_deref()
            .is_some_and(|r| cadrs_core::preferences::Preferences::load(r).autoconnect_p2p);
        app.add_plugins(cadrs_net::NetPlugin {
            identity: root.zip(ids.as_ref()).map(|(d, i)| i.key_path(&d)),
            display_name: ids.map_or_else(
                || UserProfile::system().display_name,
                |i| i.active().name.clone(),
            ),
            relays: Default::default(),
            autostart: autoconnect || net.as_deref() == Some("1"),
        });
    }
    // A person's runs ask the system for files (issue #136); scripted and headless runs keep the
    // in-app picker, which a scenario can drive. `CADRS_NATIVE_DIALOGS=0` keeps it too.
    if !scripted
        && !opts.headless
        && std::env::var_os("CADRS_NATIVE_DIALOGS").is_none_or(|v| v != "0")
    {
        app.insert_resource(cadrs_ui::file_picker::NativeFileDialogs(true));
    }
    if scripted || opts.headless {
        // The same light (or `CADRS_COLOR_SCHEME`) look on every machine.
        app.insert_resource(cadrs_ui::Appearance::without_system());
        // Everything drawn where it rests, so steps never act on a moving button.
        app.insert_resource(cadrs_ui::spring::MotionSettings { enabled: false });
        app.insert_resource(cadrs_app::ExportDirOverride(Some(
            opts.resolved_out_dir().join("exports"),
        )));
    }
    // The harness plugin clears the scenario's output directory (and with it the scenario's
    // data dir), so seed documents after adding it.
    app.add_plugins(HarnessPlugin { opts, scenario });
    app.insert_resource(cadrs_harness::BeforeComplete(
        cadrs_core::history_log::flush_saves,
    ));
    app.insert_resource(cadrs_app::mcp::McpScenarioHook(run_live_scenario));
    if let (Some(from), Some(dir)) = (&data_from, &data_dir)
        && let Err(e) = cadrs_harness::prepare_data_from(from, dir)
    {
        eprintln!("cannot load documents: {e}");
        return AppExit::from_code(2);
    }
    if let Some(dir) = data_dir {
        app.insert_resource(DocumentStore(Store::new(dir)));
    }
    if let Some(dir) = user_libraries {
        app.insert_resource(cadrs_app::eda::libraries::UserLibraries::load(Some(dir)));
    }
    if scripted {
        let user = UserProfile {
            id: "user".into(),
            display_name: "Alex Designer".into(),
        };
        if seed > 0
            && let Some(store) = app.world().get_resource::<DocumentStore>()
            && let Err(e) = store.0.seed_samples(seed as usize, &user.id, SCENARIO_NOW)
        {
            eprintln!("cannot seed documents: {e}");
            return AppExit::from_code(2);
        }
        // Wait for every rebuild, so screenshots never show parts from before an edit.
        app.insert_resource(AppClock::fixed(SCENARIO_NOW))
            .insert_resource(user)
            .insert_resource(cadrs_app::parts::RebuildBudget(None));
    }
    let exit = app
        .add_plugins((CadrsUiPlugin, CadrsAppPlugin))
        .add_systems(
            Last,
            (share_sketch_mapping, share_view_mapping, share_flat_mapping),
        )
        .run();
    // The document histories still being written.
    cadrs_core::history_log::flush_saves();
    exit
}

/// Lets scenarios address sketch-plane points (`world(x, y)`): the harness reads the app's
/// current sketch-plane-to-screen mapping (on a Drawing tab, the sheet's, in mm).
#[cfg(not(target_arch = "wasm32"))]
fn share_sketch_mapping(
    screen: Res<cadrs_app::sketch_tools::SketchScreen>,
    sheet: Res<cadrs_app::drawing::SheetScreen>,
    out: Option<ResMut<cadrs_harness::WorldToScreen>>,
) {
    let Some(mut out) = out else {
        return;
    };
    // On a Drawing tab, `world(x, y)` is a sheet point in mm (P3C.3).
    let want = match sheet.0 {
        Some((origin, ppm)) => Some(cadrs_harness::Affine {
            origin,
            x_axis: Vec2::new(ppm, 0.0),
            y_axis: Vec2::new(0.0, -ppm),
        }),
        None => screen.scripted().map(|m| cadrs_harness::Affine {
            origin: m.origin,
            x_axis: m.x,
            y_axis: m.y,
        }),
    };
    if out.0 != want {
        out.0 = want;
    }
}

/// Lets scenarios address points of the sheet metal flat view, or of a native board's
/// Schematic or Layout view (design millimetres), as `flat(x, y)`.
#[cfg(not(target_arch = "wasm32"))]
fn share_flat_mapping(
    table: Res<cadrs_app::sheetmetal_table::SmTable>,
    eda: Res<cadrs_app::eda::Eda2d>,
    eda_ui: Res<cadrs_app::eda::EdaUi>,
    rect: Res<cadrs_app::viewport::ViewportRect>,
    out: Option<ResMut<cadrs_harness::FlatToScreen>>,
) {
    let Some(mut out) = out else {
        return;
    };
    if let Some(key) = eda.0.filter(|_| eda.flat())
        && let Some((v, _)) = eda_ui.views.get(&key)
    {
        let s = v.to_screen([0.0, 0.0]);
        let k = v.scale as f32;
        let want = Some(cadrs_harness::Affine {
            origin: rect.0.min + Vec2::new(s[0] as f32, s[1] as f32),
            x_axis: Vec2::new(k, 0.0),
            y_axis: Vec2::new(0.0, -k),
        });
        if out.0 != want {
            out.0 = want;
        }
        return;
    }
    let want = table
        .body
        .filter(|_| table.scene.is_some())
        .map(|(_, rect)| {
            let v = table.view;
            let z = table.scene.as_ref().map_or(0.0, |s| s.thickness as f32);
            cadrs_harness::Affine {
                origin: rect.center() + v.project(Vec3::new(0.0, 0.0, z)),
                x_axis: v.project_vector(Vec3::X),
                y_axis: v.project_vector(Vec3::Y),
            }
        });
    if out.0 != want {
        out.0 = want;
    }
}

/// Lets scenarios address 3D points (`xyz(x, y, z)`): the current view's projection.
#[cfg(not(target_arch = "wasm32"))]
fn share_view_mapping(
    view: Res<cadrs_app::viewport::ViewportView>,
    rect: Res<cadrs_app::viewport::ViewportRect>,
    out: Option<ResMut<cadrs_harness::SpaceToScreen>>,
) {
    let Some(mut out) = out else {
        return;
    };
    let v = view.view;
    let want = Some(cadrs_harness::Affine3 {
        origin: rect.to_screen(v.project(Vec3::ZERO)),
        x_axis: v.project_vector(Vec3::X),
        y_axis: v.project_vector(Vec3::Y),
        z_axis: v.project_vector(Vec3::Z),
    });
    if out.0 != want {
        out.0 = want;
    }
}

/// Runs a scenario an MCP client sent (`run_scenario`) in the running app: its RON is a list of
/// steps or a whole `Scenario(...)`; screenshots go to a temporary folder.
#[cfg(not(target_arch = "wasm32"))]
fn run_live_scenario(
    world: &mut World,
    ron_text: &str,
    done: cadrs_app::mcp::ScenarioDone,
) -> Result<(), String> {
    let text = ron_text.trim();
    let text = if text.starts_with('[') {
        format!("Scenario(steps: {text})")
    } else {
        text.to_string()
    };
    let scenario: cadrs_harness::Scenario =
        cadrs_harness::Scenario::parse(&text).map_err(|e| format!("bad scenario: {e}"))?;
    static RUNS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let n = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let out = std::env::temp_dir()
        .join(format!("cadrs-mcp-{}", std::process::id()))
        .join(format!("run-{n}"));
    cadrs_harness::runner::start_live(world, scenario, out, done)
}

//! cadrs: an Onshape-style CAD app.
//!
//! `cadrs [--headless] [--scenario <name>] [--window-size WxH] [--out <dir>] [--data-dir <dir>]`
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
//! Documents live in `--data-dir`, else `CADRS_DATA_DIR`, else the platform data dir
//! (`~/.local/share/cadrs/documents`, `%APPDATA%\cadrs\documents`). Scenarios use a fresh
//! `<out dir>/data` instead, a fixed clock and a fixed user, so their screenshots are
//! reproducible.

use bevy::prelude::*;
use cadrs_app::{AppClock, CadrsAppPlugin, DocumentStore, StartState, UserProfile};
use cadrs_core::Store;
use cadrs_harness::{HarnessOptions, HarnessPlugin};
use cadrs_ui::CadrsUiPlugin;

/// "Now" in scenarios: 2026-09-24 15:30 UTC.
const SCENARIO_NOW: i64 = 1_790_263_800;

mod batch;
mod check;

/// Every way to run cadrs (`-h`, `--help`).
const HELP: &str = "\
cadrs: an Onshape-style CAD app.

Usage:
  cadrs [--headless] [--scenario <name>] [--window-size WxH] [--out <dir>] [--data-dir <dir>] [--no-cursor]
      Runs the app (with --scenario, a scripted scenario; --headless needs one).
  cadrs --list-documents [--data-dir <dir>] [--trash]
      Lists the stored documents, newest first: id, last modified (UTC), name.
  cadrs --document <name or id> --check [--data-dir <dir>] [--part-studio <name>] [--sketch <name>]
        [--feature <name>] [--steps] [--dof]
      Rebuilds a stored document without a window and reports its features in error or warning.
  cadrs --headless --jobs N (--scenarios a,b,... | --all)
      Renders many scenarios, N at a time, each in its own process.

Documents live in --data-dir, else $CADRS_DATA_DIR, else the platform data dir
(~/.local/share/cadrs/documents on Linux).";

fn main() -> AppExit {
    // `--jobs N` / `--scenarios a,b` / `--all`: many scenarios in parallel child processes.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("{HELP}");
        return AppExit::Success;
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
    let opts = match HarnessOptions::from_args(args) {
        Ok(o) => o,
        Err(e) => {
            // The harness's own usage line knows only the app's options: point to all of them.
            eprintln!("{}\ncadrs --help lists every option.", e.lines().next().unwrap_or(""));
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

    let mut app = App::new();
    app.add_plugins(cadrs_harness::default_plugins(&opts));
    if let Some(start) = scenario.as_ref().and_then(|s| s.start.clone()) {
        app.insert_resource(StartState(start));
    }
    let data_dir = opts.resolved_data_dir_for(scenario.as_ref());
    let data_from = opts.data_from_dir(scenario.as_ref());
    let seed = scenario.as_ref().map_or(0, |s| s.seed_documents);
    let scripted = scenario.is_some();
    // Scenarios and headless runs export into their output folder (a BOM's CSV, P3B.6), never
    // into the user's Downloads (Final part 3: golden runs had written there).
    // Nor do they see or add to the user's own libraries.
    // ($CADRS_USER_LIBRARIES still picks a folder: a run that uses the user's parts, read only.)
    let user_libraries = (scripted || opts.headless).then(|| std::env::var_os("CADRS_USER_LIBRARIES").map(std::path::PathBuf::from).unwrap_or_else(|| opts.resolved_out_dir().join("libraries")));
    if scripted || opts.headless {
        app.insert_resource(cadrs_app::ExportDirOverride(Some(opts.resolved_out_dir().join("exports"))));
    }
    // The harness plugin clears the scenario's output directory (and with it the scenario's
    // data dir), so seed documents after adding it.
    app.add_plugins(HarnessPlugin { opts, scenario });
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
    app.add_plugins((CadrsUiPlugin, CadrsAppPlugin))
        .add_systems(Last, (share_sketch_mapping, share_view_mapping, share_flat_mapping))
        .run()
}

/// Lets scenarios address sketch-plane points (`world(x, y)`): the harness reads the app's
/// current sketch-plane-to-screen mapping (on a Drawing tab, the sheet's, in mm).
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
    let want = table.body.filter(|_| table.scene.is_some()).map(|(_, rect)| {
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

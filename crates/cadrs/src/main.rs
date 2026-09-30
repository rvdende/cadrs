//! cadrs: an Onshape-style CAD app.
//!
//! `cadrs [--headless] [--scenario <name>] [--window-size WxH] [--out <dir>] [--data-dir <dir>]`
//!
//! `cadrs --headless --jobs N (--scenarios a,b,... | --all)` renders many scenarios, N at a
//! time, each in its own process (see [`batch`]).
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

fn main() -> AppExit {
    // `--jobs N` / `--scenarios a,b` / `--all`: many scenarios in parallel child processes.
    let args: Vec<String> = std::env::args().skip(1).collect();
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
            eprintln!("{e}");
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
        .add_systems(Last, (share_sketch_mapping, share_view_mapping))
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

//! cadrs in a browser (`tools/web`): the app on the page's `<canvas id="cadrs">`, drawn with
//! WebGPU.
//!
//! A page has one thread and no processes, so the kernel's jobs run on it as they are sent
//! (`cadrs_core::rebuild`), and there is no MCP server and no update check (the page is
//! deployed with the site, always the latest). It goes online as the app does ([`cadrs_net`]:
//! iroh over its relays, its key kept with the documents).
//!
//! `?quick-start` in the address opens the document modified last (or a new one) instead of the
//! documents page, as the site's embed does.
//!
//! Its files are `cadrs_fs`'s, kept in the browser: the documents in [`DOCUMENTS`], the user's
//! libraries in [`LIBRARIES`]. Exports go to [`DOWNLOADS`], and each file written there is
//! handed to the browser as a download. Files to import come from the browser's file chooser,
//! copied into `cadrs_ui::file_picker::UPLOADS`. Neither of those two folders is kept.

use std::path::PathBuf;

use bevy::prelude::*;
use cadrs_app::{CadrsAppPlugin, DocumentStore, ExportDirOverride};
use cadrs_core::Store;
use cadrs_harness::{HarnessOptions, HarnessPlugin};
use cadrs_ui::CadrsUiPlugin;

/// The page's canvas the app draws on.
const CANVAS: &str = "#cadrs";
const DOCUMENTS: &str = "/cadrs/documents";
const LIBRARIES: &str = "/cadrs/libraries";
const DOWNLOADS: &str = "/Downloads";

pub fn run() -> AppExit {
    cadrs_app::register_kernel_tasks();
    let mut app = App::new();
    // A page times its systems when built with the `profiler` feature, which tools/web/build.sh
    // leaves out (in a page, Bevy's log layer also hands every span to the browser).
    app.insert_resource(cadrs_app::frame_profile::CpuProfiling::at_start(
        cfg!(feature = "profiler"),
        true,
    ));
    app.add_plugins(
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "cadrs".into(),
                    canvas: Some(CANVAS.into()),
                    // The canvas fills the page, and follows it as the browser window resizes.
                    fit_canvas_to_parent: true,
                    // Keys the app uses (Tab, Ctrl+S, the arrows) are the app's, not the page's.
                    prevent_default_event_handling: true,
                    ..default()
                }),
                ..default()
            })
            // The frame panel's CPU times, as in the app.
            .set(bevy::log::LogPlugin {
                custom_layer: cadrs_app::frame_profile::profiler_layer,
                ..default()
            }),
    );
    app.add_plugins(HarnessPlugin {
        opts: HarnessOptions::default(),
        scenario: None,
    });
    let uploads = cadrs_ui::file_picker::UPLOADS;
    cadrs_fs::web::set_scratch([DOWNLOADS.into(), uploads.into()]);
    for dir in [DOCUMENTS, LIBRARIES, DOWNLOADS, uploads] {
        if let Err(e) = cadrs_fs::create_dir_all(dir) {
            error!("cannot make {dir}: {e}");
        }
    }
    cadrs_fs::web::set_downloads(Some(DOWNLOADS.into()));
    // Sharing, collaboration and public documents, as in the app: iroh over its relays (a page
    // has no UDP). The identity in use names the key, kept with the documents; the page goes
    // online when the user connects, or at start with *Connect when cadrs starts*.
    let root = PathBuf::from(DOCUMENTS);
    let ids = cadrs_app::identity::load(&root);
    app.add_plugins(cadrs_net::NetPlugin {
        identity: Some(ids.key_path(&root)),
        display_name: ids.active().name.clone(),
        relays: Default::default(),
        autostart: cadrs_core::preferences::Preferences::load(&root).autoconnect_p2p,
    });
    app.insert_resource(DocumentStore(Store::new(DOCUMENTS)))
        .insert_resource(ExportDirOverride(Some(DOWNLOADS.into())))
        .insert_resource(cadrs_ui::file_picker::NativeFileDialogs(true))
        .insert_resource(cadrs_app::eda::libraries::UserLibraries::load(Some(
            PathBuf::from(LIBRARIES),
        )));
    if quick_start() {
        app.insert_resource(cadrs_app::QuickStart);
    }
    app.add_plugins((CadrsUiPlugin, CadrsAppPlugin)).run()
}

/// `?quick-start` in the page's address (the site's embed): straight into a document
/// ([`cadrs_app::QuickStart`]).
fn quick_start() -> bool {
    let get = |from: &js_sys::wasm_bindgen::JsValue, key: &str| {
        js_sys::Reflect::get(from, &key.into()).ok()
    };
    let search = get(&js_sys::global().into(), "location")
        .and_then(|l| get(&l, "search"))
        .and_then(|s| s.as_string())
        .unwrap_or_default();
    search
        .trim_start_matches('?')
        .split('&')
        .any(|p| p == "quick-start" || p.starts_with("quick-start="))
}

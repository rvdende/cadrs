//! cadrs_harness: scripted scenarios, synthetic input and screenshots.
//!
//! Headless mode follows Bevy's `headless_renderer` example: no primary window, winit disabled,
//! `ScheduleRunnerPlugin` driving frames, and the main camera rendering into an offscreen
//! [`Image`]. The UI renders into the same image because the main camera is the default UI camera.
//! Screenshots use `Screenshot` on that image.
//!
//! Input without a window: the harness writes `PointerInput` messages for the mouse pointer
//! (see [`runner::HARNESS_POINTER`] for why not a custom one), located on the render surface, so hovering and clicking go
//! through normal picking (UI buttons receive `Pointer<Press>`/`Pointer<Click>`). Keys are sent as
//! `KeyboardInput` messages, which also update `ButtonInput<KeyCode>`. Keyboard focus dispatch in
//! `bevy_input_focus` needs a primary window entity, so headless mode spawns a *virtual* one: a
//! `Window` + `PrimaryWindow` entity that never gets an OS window (winit is disabled).
//!
//! Determinism: virtual time advances exactly 1/60 s per frame, the surface size is fixed,
//! pipelines compile synchronously, the caret never blinks, and animations are finished before
//! every screenshot.

pub mod cursor;
pub mod keys;
pub mod runner;
pub mod scenario;

use std::path::{Path, PathBuf};
use std::time::Duration;

use bevy::app::{PluginGroupBuilder, ScheduleRunnerPlugin};
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::prelude::*;
use bevy::render::RenderPlugin;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::time::TimeUpdateStrategy;
use bevy::window::{ExitCondition, PrimaryWindow, WindowResolution};
use bevy::winit::WinitPlugin;
use cadrs_ui::RenderSurface;
use cadrs_ui::input::CaretBlinkOverride;

pub use runner::{Affine, Affine3, SpaceToScreen, WorldToScreen};
pub use scenario::{Scenario, Step, Target};

/// Command-line options.
#[derive(Debug, Clone)]
pub struct HarnessOptions {
    pub headless: bool,
    pub scenario: Option<String>,
    pub size: UVec2,
    /// Where screenshots go; defaults to `target/scenarios/<name>/` in the workspace.
    pub out_dir: Option<PathBuf>,
    /// Where documents are stored (`--data-dir`). Scenarios default to a fresh
    /// `<out dir>/data`, so they never touch the user's documents.
    pub data_dir: Option<PathBuf>,
    /// `--no-cursor`: never draw the software cursor into screenshots.
    pub no_cursor: bool,
}

impl Default for HarnessOptions {
    fn default() -> Self {
        Self {
            headless: false,
            scenario: None,
            size: UVec2::new(1600, 1000),
            out_dir: None,
            data_dir: None,
            no_cursor: false,
        }
    }
}

pub const USAGE: &str = "usage: cadrs [--headless] [--scenario <name>] [--window-size WxH] \
                         [--out <dir>] [--data-dir <dir>] [--no-cursor]";

impl HarnessOptions {
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut o = Self::default();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            match a.as_str() {
                "--headless" => o.headless = true,
                "--scenario" => o.scenario = Some(args.next().ok_or("--scenario needs a name")?),
                "--window-size" => {
                    let v = args.next().ok_or("--window-size needs WxH")?;
                    let (w, h) = v
                        .split_once(['x', 'X'])
                        .ok_or_else(|| format!("bad --window-size {v:?}, expected WxH"))?;
                    let w: u32 = w.parse().map_err(|_| format!("bad width in {v:?}"))?;
                    let h: u32 = h.parse().map_err(|_| format!("bad height in {v:?}"))?;
                    o.size = UVec2::new(w.max(64), h.max(64));
                }
                "--out" => o.out_dir = Some(args.next().ok_or("--out needs a directory")?.into()),
                "--data-dir" => {
                    o.data_dir = Some(args.next().ok_or("--data-dir needs a directory")?.into())
                }
                "--no-cursor" => o.no_cursor = true,
                "-h" | "--help" => return Err(USAGE.into()),
                other => return Err(format!("unknown argument {other:?}\n{USAGE}")),
            }
        }
        if o.headless && o.scenario.is_none() {
            return Err("--headless needs --scenario <name>".into());
        }
        Ok(o)
    }

    /// Loads `scenarios/<name>.ron` (the name may also be a path to a `.ron` file).
    pub fn load_scenario(&self) -> Result<Option<Scenario>, String> {
        let Some(name) = &self.scenario else {
            return Ok(None);
        };
        let path = if name.ends_with(".ron") {
            PathBuf::from(name)
        } else {
            scenario::scenarios_dir().join(format!("{name}.ron"))
        };
        Scenario::load(&path).map(Some)
    }

    fn scenario_name(&self) -> String {
        let name = self.scenario.clone().unwrap_or_default();
        PathBuf::from(&name)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or(name)
    }

    pub fn resolved_out_dir(&self) -> PathBuf {
        self.out_dir.clone().unwrap_or_else(|| {
            scenario::scenarios_dir()
                .join("../target/scenarios")
                .join(self.scenario_name())
        })
    }
}

impl HarnessOptions {
    /// The document directory for this run: `--data-dir`, else `CADRS_DATA_DIR`, else (for a
    /// scenario) `<out dir>/data`. `None` means the platform default.
    pub fn resolved_data_dir(&self) -> Option<PathBuf> {
        self.resolved_data_dir_for(None)
    }

    /// [`Self::resolved_data_dir`] for a scenario. A scenario with `data_from` also uses its
    /// own `<out dir>/data`, which [`prepare_data_from`] fills with the other one's documents.
    pub fn resolved_data_dir_for(&self, _scenario: Option<&Scenario>) -> Option<PathBuf> {
        self.data_dir
            .clone()
            .or_else(|| std::env::var_os("CADRS_DATA_DIR").map(PathBuf::from))
            .or_else(|| {
                self.scenario
                    .is_some()
                    .then(|| self.resolved_out_dir().join("data"))
            })
    }
}

impl HarnessOptions {
    /// For a scenario with `data_from` and no explicit data dir: the other scenario's data dir
    /// that [`prepare_data_from`] snapshots into this scenario's own one.
    pub fn data_from_dir(&self, scenario: Option<&Scenario>) -> Option<PathBuf> {
        if self.data_dir.is_some() || std::env::var_os("CADRS_DATA_DIR").is_some() {
            return None;
        }
        let from = scenario?.data_from.as_ref()?;
        Some(self.resolved_out_dir().parent()?.join(from))
    }
}

/// Name of the file the runner writes into a scenario's output directory when it finished
/// successfully (removed with the directory when the scenario starts again).
pub const COMPLETE_MARKER: &str = ".complete";

/// Copies the documents of the scenario whose output directory is `from` into `to`, once that
/// scenario has finished (its [`COMPLETE_MARKER`] is there). A relaunch scenario thus always
/// starts from the same saved state, even when it is started while the other one is still
/// running (runs in parallel), and it never writes into the other scenario's documents (it
/// would autosave them when it opens one).
pub fn prepare_data_from(from: &Path, to: &Path) -> Result<(), String> {
    let marker = from.join(COMPLETE_MARKER);
    let start = std::time::Instant::now();
    loop {
        if marker.is_file() {
            let _ = std::fs::remove_dir_all(to);
            // (A run of the other scenario starting meanwhile clears its directory: wait for
            // it to finish again.)
            match copy_dir(&from.join("data"), to) {
                Ok(()) if marker.is_file() => return Ok(()),
                Ok(()) => {}
                Err(e) if start.elapsed() > Duration::from_secs(180) => {
                    return Err(format!("cannot copy {}: {e}", from.display()));
                }
                Err(_) => {}
            }
        }
        if start.elapsed() > Duration::from_secs(180) {
            return Err(format!(
                "{} has not finished (no {}): run that scenario first",
                from.display(),
                COMPLETE_MARKER
            ));
        }
        if !from.is_dir() && start.elapsed() > Duration::from_secs(5) {
            return Err(format!("{} does not exist: run that scenario first", from.display()));
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)? {
        let e = e?;
        let target = to.join(e.file_name());
        if e.file_type()?.is_dir() {
            copy_dir(&e.path(), &target)?;
        } else if e.path().extension().is_none_or(|x| x != "tmp") {
            std::fs::copy(e.path(), target)?;
        }
    }
    Ok(())
}

/// Bevy's default plugins, configured for windowed or headless runs.
pub fn default_plugins(opts: &HarnessOptions) -> PluginGroupBuilder {
    let plugins = DefaultPlugins.build();
    if opts.headless {
        plugins
            .set(WindowPlugin {
                primary_window: None,
                exit_condition: ExitCondition::DontExit,
                ..default()
            })
            .set(RenderPlugin {
                synchronous_pipeline_compilation: true,
                ..default()
            })
            .disable::<WinitPlugin>()
    } else {
        let mut resolution = WindowResolution::new(opts.size.x, opts.size.y);
        if opts.scenario.is_some() {
            // Scenario replays use exact logical = physical pixels.
            resolution = resolution.with_scale_factor_override(1.0);
        }
        plugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "cadrs".into(),
                resolution,
                ..default()
            }),
            ..default()
        })
    }
}

/// Sets up the render surface, and the scenario runner if a scenario was given.
pub struct HarnessPlugin {
    pub opts: HarnessOptions,
    pub scenario: Option<Scenario>,
}

impl Plugin for HarnessPlugin {
    fn build(&self, app: &mut App) {
        let size = self.opts.size;
        if self.opts.headless {
            app.add_plugins(ScheduleRunnerPlugin::run_loop(Duration::ZERO));
            let mut image = Image::new_fill(
                Extent3d {
                    width: size.x,
                    height: size.y,
                    depth_or_array_layers: 1,
                },
                TextureDimension::D2,
                &[255, 255, 255, 255],
                TextureFormat::Rgba8UnormSrgb,
                RenderAssetUsages::default(),
            );
            image.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING
                | TextureUsages::COPY_SRC
                | TextureUsages::COPY_DST
                | TextureUsages::RENDER_ATTACHMENT;
            let handle = app.world_mut().resource_mut::<Assets<Image>>().add(image);
            app.insert_resource(RenderSurface {
                target: RenderTarget::Image(handle.into()),
                size,
                headless: true,
            });
            // A virtual primary window: keyboard focus dispatch needs one, but without winit it
            // never becomes an OS window.
            app.world_mut().spawn((
                Name::new("virtual-window"),
                Window {
                    resolution: WindowResolution::new(size.x, size.y)
                        .with_scale_factor_override(1.0),
                    ..default()
                },
                PrimaryWindow,
            ));
        } else {
            app.insert_resource(RenderSurface { size, ..default() });
        }

        if let Some(scenario) = &self.scenario {
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                1.0 / 60.0,
            )))
            // Keep the caret steadily visible in screenshots.
            .insert_resource(CaretBlinkOverride(Duration::from_secs(1_000_000)));
            let mut scenario = scenario.clone();
            scenario.cursor &= !self.opts.no_cursor;
            runner::add_runner(
                app,
                scenario,
                self.opts.scenario_name(),
                self.opts.resolved_out_dir(),
            );
        }
    }
}

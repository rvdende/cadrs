//! Scenario scripts (`scenarios/*.ron`).
//!
//! ```ron
//! Scenario(
//!     start: Some("landing"),
//!     steps: [
//!         Screenshot("01-landing"),
//!         Click(ui("create")),
//!         Type("Doc 1"),
//!         Key("Enter"),
//!         Hover(ui("help")),
//!         Wait(40),
//!         MoveTo(800, 500),
//!         ClickAt(800, 500),
//!         Drag(ui("a"), at(900, 400)),
//!         ClickWorld(10, 5),            // a point on the sketch plane, in mm
//!         Press(world(-20, 0)), Hover(world(0, -10)), Release,
//!         PressWith("right", at(900, 500)), MoveTo(950, 520), ReleaseWith("right"),
//!         Custom("populate-sketch 500"),  // app set-up commands
//!         MeasureStart("idle"), Wait(120), MeasureEnd,  // writes perf-idle.txt
//!         Screenshot("02-after"),
//!     ],
//! )
//! ```
//!
//! Every step waits for the app to settle before it runs: no rebuild, view animation, section
//! caps or other background work in flight ([`cadrs_ui::PendingWork`]), then two quiet frames
//! for what that produced to be laid out and drawn. A step's UI target is waited for too. So a
//! scenario needs no `Wait`s between steps; `Wait(n)` is only for what runs on its own time
//! (a tooltip's delay, an animation caught half-way). A step that waits longer than the
//! scenario's `timeout` (seconds, default 30; `SetTimeout(s)` changes it from there on) fails
//! the scenario.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct Scenario {
    /// The app state to start in (`"landing"`, `"document"`, `"gallery"`).
    #[serde(default)]
    pub start: Option<String>,
    /// Frames to run before the first step, so shaders, fonts and layout settle.
    #[serde(default = "default_warmup")]
    pub warmup: u32,
    /// Sample documents (fixed names, ids and dates) to put in the scenario's fresh data dir.
    #[serde(default)]
    pub seed_documents: u32,
    /// Draw a software cursor (the app's current cursor kind) at the synthetic pointer in
    /// screenshots, so cursor changes can be judged (default on; `--no-cursor` turns it off).
    #[serde(default = "default_cursor")]
    pub cursor: bool,
    /// Start from a copy of another scenario's data dir (`target/scenarios/<name>/data`)
    /// instead of a fresh one: a relaunch of the app on the documents that scenario saved. Run
    /// that scenario first (the copy waits until it has finished).
    #[serde(default)]
    pub data_from: Option<String>,
    /// Seconds one step may wait (for the app to settle, or for its UI target) before the
    /// scenario fails.
    #[serde(default = "default_timeout")]
    pub timeout: f32,
    pub steps: Vec<Step>,
}

fn default_timeout() -> f32 {
    30.0
}

fn default_cursor() -> bool {
    true
}

fn default_warmup() -> u32 {
    20
}

/// Where a pointer step acts.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Target {
    /// The center of the UI node with this `Name`.
    #[serde(rename = "ui")]
    Ui(String),
    /// A position in logical pixels.
    #[serde(rename = "at")]
    At(f32, f32),
    /// A point on the active sketch plane (or the Top plane when no sketch is open), in
    /// sketch millimetres: `world(10, 5)`.
    #[serde(rename = "world")]
    World(f32, f32),
    /// A point in 3D world space (mm, Z up), wherever the current view shows it:
    /// `xyz(25, 15, 25)` (the middle of a part's top face).
    #[serde(rename = "xyz")]
    Xyz(f32, f32, f32),
    /// A point inside the UI node with this `Name`, as fractions of its width and height from
    /// its top-left corner: `ui_at("appearance-opacity", 0.4, 0.5)` (a slider at 40%).
    #[serde(rename = "ui_at")]
    UiAt(String, f32, f32),
    /// A point of the sheet metal flat view's flat, in the flat's millimetres (the view's
    /// scene: the parts side by side), wherever that view shows it: `flat(40, -20)`.
    #[serde(rename = "flat")]
    Flat(f32, f32),
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Step {
    /// Move there, press and release the primary button.
    Click(Target),
    /// Two clicks in quick succession.
    DoubleClick(Target),
    /// Move there, press and release the secondary button.
    RightClick(Target),
    /// Move the pointer there.
    Hover(Target),
    /// Move the pointer to a position.
    MoveTo(f32, f32),
    /// Click at a position.
    ClickAt(f32, f32),
    /// Click at a sketch-plane point (mm); short for `Click(world(x, y))`.
    ClickWorld(f32, f32),
    /// Move the pointer to a sketch-plane point (mm); short for `Hover(world(x, y))`.
    MoveWorld(f32, f32),
    /// Press at the first sketch-plane point, move to the second in several steps, release:
    /// `DragWorld(x0, y0, x1, y1)`.
    DragWorld(f32, f32, f32, f32),
    /// Press the primary button at the target and keep it down.
    Press(Target),
    /// Release the primary button where the pointer is.
    Release,
    /// Press a button (`"left"`, `"right"` or `"middle"`) at the target and keep it down (for
    /// screenshots in the middle of an orbit or pan).
    PressWith(String, Target),
    /// Release a button pressed with [`Step::PressWith`] where the pointer is.
    ReleaseWith(String),
    /// Press at the first target, move to the second in several steps, release.
    Drag(Target, Target),
    /// Like [`Step::Drag`] with the secondary (right) button: orbits the 3D view.
    RightDrag(Target, Target),
    /// Like [`Step::Drag`] with the middle button: pans the 3D view.
    MiddleDrag(Target, Target),
    /// Move the pointer to the target and turn the mouse wheel by this many lines (positive is
    /// away from the user, which zooms in).
    Scroll(Target, f32),
    /// Press a key (or modifier, such as `"Ctrl"`) and keep it down.
    KeyDown(String),
    /// Release a key pressed with [`Step::KeyDown`].
    KeyUp(String),
    /// Type text into the focused element, one character per frame.
    Type(String),
    /// Press and release a key, optionally with modifiers: `"Enter"`, `"L"`, `"Ctrl+A"`,
    /// `"Shift+7"`, `"Escape"`, `"ArrowLeft"`.
    Key(String),
    /// Run this many frames (only for what runs on its own time: every step already waits for
    /// the app to settle).
    Wait(u32),
    /// From here on, a step may wait this many seconds before the scenario fails.
    SetTimeout(f32),
    /// Wait until a UI node with this name exists (fails after a timeout).
    WaitFor(String),
    /// Check that the text in the UI node with this name (its own and its descendants' text,
    /// including a text field's value) contains the given text, waiting for it like
    /// [`Step::WaitFor`]; fails after the timeout. `ExpectText("hole-tapped-depth", "10.02 mm")`
    /// catches a typed value that got lost.
    ExpectText(String, String),
    /// Fails unless the UI node with this name exists and is enabled (a dialog's ✓ that
    /// should be ready before it is clicked).
    AssertEnabled(String),
    /// Save the frame as `NN-label.png` (the label should carry its own number).
    Screenshot(String),
    /// Send a named set-up command to the app (see `cadrs_ui::ScriptCommand`), e.g.
    /// `Custom("populate-sketch 500")`.
    Custom(String),
    /// Start timing frames; [`Step::MeasureEnd`] writes `perf-<label>.txt` (frame times in ms:
    /// mean, median, 95th percentile, max) to the output directory.
    MeasureStart(String),
    MeasureEnd,
}

impl Scenario {
    pub fn parse(text: &str) -> Result<Self, String> {
        ron::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The `scenarios/` directory: next to the current directory if it exists there, otherwise the
/// one in the source tree.
pub fn scenarios_dir() -> PathBuf {
    let local = PathBuf::from("scenarios");
    if local.is_dir() {
        return local;
    }
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../scenarios"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_step_kinds() {
        let s = Scenario::parse(
            r#"Scenario(
                start: Some("gallery"),
                steps: [
                    Click(ui("create")), DoubleClick(at(1, 2)), RightClick(ui("x")),
                    Hover(ui("h")), MoveTo(10, 20.5), ClickAt(3, 4), Press(ui("p")), Release,
                    Drag(ui("a"), at(5, 6)), Type("Doc 1"), Key("Ctrl+A"), Wait(3),
                    WaitFor("dialog"), Screenshot("01-x"), RightDrag(at(1, 1), at(2, 2)),
                    MiddleDrag(at(1, 1), at(2, 2)), Scroll(at(3, 3), -2.5), KeyDown("Ctrl"),
                    KeyUp("Ctrl"), ClickWorld(1, 2), MoveWorld(3, 4), DragWorld(0, 0, 5, 5),
                    Click(world(2, 3)), Custom("populate-sketch 10"), MeasureStart("x"),
                    MeasureEnd, PressWith("right", at(4, 4)), ReleaseWith("right"),
                    Hover(xyz(1, 2, 3)), ExpectText("a", "b"), SetTimeout(5),
                ],
            )"#,
        )
        .unwrap();
        assert_eq!(s.timeout, 30.0);
        assert_eq!(s.steps[30], Step::SetTimeout(5.0));
        assert_eq!(s.start.as_deref(), Some("gallery"));
        assert_eq!(s.warmup, 20);
        assert_eq!(s.steps.len(), 31);
        assert_eq!(s.steps[28], Step::Hover(Target::Xyz(1.0, 2.0, 3.0)));
        assert_eq!(s.steps[29], Step::ExpectText("a".into(), "b".into()));
        assert!(s.cursor);
        assert_eq!(s.steps[22], Step::Click(Target::World(2.0, 3.0)));
        assert_eq!(s.steps[16], Step::Scroll(Target::At(3.0, 3.0), -2.5));
        assert_eq!(s.steps[0], Step::Click(Target::Ui("create".into())));
        assert_eq!(s.steps[4], Step::MoveTo(10.0, 20.5));
    }

    #[test]
    fn repository_scenarios_parse() {
        for entry in std::fs::read_dir(scenarios_dir()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "ron") {
                Scenario::load(&path).unwrap();
            }
        }
    }
}

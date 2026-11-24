//! Idle frames (#298): with a window, the app sleeps while nothing happens instead of drawing a
//! frame at the screen's rate. Bevy's default is a game's loop (`UpdateMode::Continuous`); a
//! desktop app wants Bevy's reactive mode, which runs a frame when there's input or a redraw is
//! asked for ([`RequestRedraw`]), and otherwise after at most [`WAIT`] (so a caret blinks and a
//! result that is polled for shows soon).
//!
//! What changes with no input asks for frames itself while it runs: here, the moments after
//! input (a hover's or a click's short animations) and a scenario being played; in the app, view
//! animations, Animate and the kernel's results still to come (`cadrs_app::awake`).
//!
//! `CADRS_CONTINUOUS=1` puts the game's loop back, to compare or in case something waits for a
//! frame that doesn't come. `CADRS_IDLE_DEBUG=1` logs, once a second, how many frames ran and how
//! many of them asked for the next one (the app logs its own reasons beside it).

use std::time::Duration;

use bevy::input::keyboard::KeyboardInput;
use bevy::input::mouse::{MouseButtonInput, MouseWheel};
use bevy::prelude::*;
use bevy::window::{CursorMoved, FileDragAndDrop, RequestRedraw, WindowFocused, WindowResized};
use bevy::winit::{UpdateMode, WinitSettings};

/// The longest the app sleeps while its window has focus.
pub const WAIT: Duration = Duration::from_millis(250);
/// The longest it sleeps while another window has focus.
pub const WAIT_UNFOCUSED: Duration = Duration::from_secs(1);
/// How long it keeps drawing every frame after input.
pub const AFTER_INPUT: Duration = Duration::from_millis(500);

/// Whether `CADRS_CONTINUOUS` asks for a frame every vsync, as before.
pub fn continuous() -> bool {
    std::env::var_os("CADRS_CONTINUOUS").is_some_and(|v| v != "0")
}

pub struct IdlePlugin;

impl Plugin for IdlePlugin {
    fn build(&self, app: &mut App) {
        if continuous() {
            return;
        }
        app.insert_resource(WinitSettings {
            focused_mode: UpdateMode::reactive(WAIT),
            unfocused_mode: UpdateMode::reactive_low_power(WAIT_UNFOCUSED),
        })
        .add_systems(Last, (awake_after_input, awake_while_scenario));
        if std::env::var_os("CADRS_IDLE_DEBUG").is_some_and(|v| v != "0") {
            app.add_systems(
                Last,
                log_frames
                    .after(awake_after_input)
                    .after(awake_while_scenario),
            );
        }
    }
}

/// Every frame for [`AFTER_INPUT`] after the last input, so what input starts plays smoothly.
#[allow(clippy::too_many_arguments)]
fn awake_after_input(
    time: Res<Time<Real>>,
    mut last: Local<Option<Duration>>,
    mut keys: MessageReader<KeyboardInput>,
    mut buttons: MessageReader<MouseButtonInput>,
    mut wheel: MessageReader<MouseWheel>,
    mut moved: MessageReader<CursorMoved>,
    mut dropped: MessageReader<FileDragAndDrop>,
    mut resized: MessageReader<WindowResized>,
    mut focused: MessageReader<WindowFocused>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let input = keys.read().count()
        + buttons.read().count()
        + wheel.read().count()
        + moved.read().count()
        + dropped.read().count()
        + resized.read().count()
        + focused.read().count()
        > 0;
    let now = time.elapsed();
    if input {
        *last = Some(now);
    }
    if last.is_some_and(|t| now.saturating_sub(t) < AFTER_INPUT) {
        redraw.write(RequestRedraw);
    }
}

/// Every frame while a scenario plays: it steps a frame at a time.
fn awake_while_scenario(
    runner: Option<Res<crate::runner::Runner>>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    if runner.is_some() {
        redraw.write(RequestRedraw);
    }
}

/// Frames since the last log, and those that asked for another.
#[derive(Default)]
struct FrameTally {
    since: f64,
    frames: u32,
    asked: u32,
}

/// `CADRS_IDLE_DEBUG`: once a second, the frames run and how many asked for the next.
fn log_frames(
    time: Res<Time<Real>>,
    mut asked: MessageReader<RequestRedraw>,
    mut tally: Local<FrameTally>,
) {
    tally.frames += 1;
    tally.asked += u32::from(asked.read().count() > 0);
    let now = time.elapsed_secs_f64();
    if now - tally.since >= 1.0 {
        info!(
            "idle: {} frames in the last second, {} of them asked for the next (the rest woke on \
             input, a window or device event, or the {} ms timer)",
            tally.frames,
            tally.asked,
            WAIT.as_millis()
        );
        *tally = FrameTally {
            since: now,
            ..default()
        };
    }
}

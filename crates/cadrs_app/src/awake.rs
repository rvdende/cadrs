//! Frames while something moves on its own (#298). With a window, the app sleeps while idle
//! (`cadrs_harness::idle`); these ask for a frame each frame while they run, so they play
//! smoothly and a result shows the moment it's there:
//!
//! - a rebuild's or a kernel job's result still to come ([`cadrs_core::rebuild::awaited`]);
//! - a view animation (`N`, fit, a named view);
//! - Animate playing a mate;
//! - a spinner showing, a fade-in, an auto-hiding toast near its end (its fade out).
//!
//! `CADRS_IDLE_DEBUG=1` logs, once a second, which of these kept the app awake.

use bevy::prelude::*;
use bevy::window::RequestRedraw;
use cadrs_ui::FadeIn;
use cadrs_ui::spinner::Spin;
use cadrs_ui::toast::{FADE_OUT, Toast};

use crate::assembly::animate::Playback;
use crate::viewport::ViewportView;

pub struct AwakePlugin;

impl Plugin for AwakePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Last, keep_awake);
    }
}

/// Seconds before an auto-hiding toast's fade begins that frames start coming: the longest the
/// app sleeps, so the fade starts on time.
const BEFORE_FADE: f32 = 0.3;

/// What keeps the app awake, in the order [`keep_awake`] checks them.
const REASONS: [&str; 6] = ["kernel", "view", "animate", "spinner", "fade", "toast"];

/// Frames each reason kept awake since the last log (`CADRS_IDLE_DEBUG`).
#[derive(Default)]
struct Tally {
    since: f64,
    counts: [u32; 6],
}

#[allow(clippy::too_many_arguments)]
fn keep_awake(
    time: Res<Time<Real>>,
    view: Option<Res<ViewportView>>,
    playback: Option<Res<Playback>>,
    // A spinner in a hidden panel (`Display::None`) is laid out at no size: it isn't showing.
    spins: Query<(&InheritedVisibility, &ComputedNode), With<Spin>>,
    fades: Query<(), With<FadeIn>>,
    toasts: Query<&Toast>,
    mut tally: Local<Tally>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let reasons = [
        cadrs_core::rebuild::awaited() > 0,
        view.is_some_and(|v| v.animation.is_some()),
        playback.is_some_and(|p| p.playing),
        spins
            .iter()
            .any(|(v, node)| v.get() && node.size().min_element() > 0.0),
        !fades.is_empty(),
        toasts
            .iter()
            .any(|t| t.autohide && t.remaining < FADE_OUT + BEFORE_FADE),
    ];
    if reasons.iter().any(|r| *r) {
        redraw.write(RequestRedraw);
    }
    if idle_debug() {
        for (count, on) in tally.counts.iter_mut().zip(reasons) {
            *count += u32::from(on);
        }
        let now = time.elapsed_secs_f64();
        if now - tally.since >= 1.0 {
            let kept: Vec<String> = REASONS
                .iter()
                .zip(tally.counts)
                .filter(|(_, n)| *n > 0)
                .map(|(r, n)| format!("{r} {n}"))
                .collect();
            info!(
                "idle: frames kept awake by the app: {} (kernel results awaited now: {})",
                if kept.is_empty() {
                    "none".into()
                } else {
                    kept.join(", ")
                },
                cadrs_core::rebuild::awaited()
            );
            *tally = Tally {
                since: now,
                ..default()
            };
        }
    }
}

/// Whether `CADRS_IDLE_DEBUG` asks for the log.
fn idle_debug() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("CADRS_IDLE_DEBUG").is_some_and(|v| v != "0"))
}

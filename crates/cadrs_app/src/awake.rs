//! Frames while something moves on its own (#298). With a window, the app sleeps while idle
//! (`cadrs_harness::idle`); these ask for a frame each frame while they run, so they play
//! smoothly and a result shows the moment it's there:
//!
//! - a rebuild's or a kernel job's result still to come ([`cadrs_core::rebuild::awaited`]);
//! - a view animation (`N`, fit, a named view);
//! - Animate playing a mate;
//! - a spinner showing, a fade-in, an auto-hiding toast near its end (its fade out).

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

fn keep_awake(
    view: Option<Res<ViewportView>>,
    playback: Option<Res<Playback>>,
    spins: Query<&InheritedVisibility, With<Spin>>,
    fades: Query<(), With<FadeIn>>,
    toasts: Query<&Toast>,
    mut redraw: MessageWriter<RequestRedraw>,
) {
    let moving = cadrs_core::rebuild::awaited() > 0
        || view.is_some_and(|v| v.animation.is_some())
        || playback.is_some_and(|p| p.playing)
        || spins.iter().any(|v| v.get())
        || !fades.is_empty()
        || toasts
            .iter()
            .any(|t| t.autohide && t.remaining < FADE_OUT + BEFORE_FADE);
    if moving {
        redraw.write(RequestRedraw);
    }
}

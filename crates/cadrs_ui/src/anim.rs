//! Small UI animations. The harness sends [`FinishAnimations`] before each screenshot so every
//! running animation jumps to its end state, which keeps screenshots deterministic.

use bevy::prelude::*;

pub struct AnimPlugin;

impl Plugin for AnimPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<FinishAnimations>()
            .add_systems(PostUpdate, fade_in.before(bevy::ui::UiSystems::Prepare));
    }
}

/// Jump every running animation to its end state.
#[derive(Message, Debug, Clone, Copy, Default)]
pub struct FinishAnimations;

/// Fades the background color in from transparent to `target` over `duration` seconds.
#[derive(Component, Debug, Clone, Copy)]
pub struct FadeIn {
    pub target: Color,
    pub elapsed: f32,
    pub duration: f32,
}

fn fade_in(
    mut commands: Commands,
    time: Res<Time>,
    mut finish: MessageReader<FinishAnimations>,
    mut q: Query<(Entity, &mut FadeIn, &mut BackgroundColor)>,
) {
    let finish_all = finish.read().count() > 0;
    for (e, mut fade, mut bg) in &mut q {
        fade.elapsed += time.delta_secs();
        let t = if finish_all || fade.duration <= 0.0 {
            1.0
        } else {
            (fade.elapsed / fade.duration).clamp(0.0, 1.0)
        };
        let alpha = fade.target.alpha() * t;
        bg.0 = fade.target.with_alpha(alpha);
        if t >= 1.0 {
            commands.entity(e).try_remove::<FadeIn>();
        }
    }
}

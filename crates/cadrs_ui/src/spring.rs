//! Springs: buttons that squash under the pointer and wobble back, panels that fly to their
//! place. After Raven's `src/widget/anim.c`.
//!
//! A spring is a damped oscillator pulled toward a target,
//! `x'' = -w² (x - target) - 2 z w x'`: `w` sets how fast it answers (its period without
//! friction is the [`Motion`]'s response) and `z` how much it bounces (1: it lands without
//! overshoot). The step is the equation's exact solution, so the motion doesn't depend on the
//! frame rate and never explodes after a long frame.
//!
//! - Every [`bevy::ui_widgets::Button`] squashes: pressed, it goes flatter and wider at once
//!   (snappy); released, it is thrown past its shape and wobbles back (bouncy). Opt out with
//!   [`NoSquash`].
//! - [`Appear`] plays when a node is spawned: it flies up (or pops) into place, after an
//!   optional delay for a staggered entrance.
//! - Both drive the node's [`UiTransform`], so layout is never touched. [`Motion`] is off in
//!   scripted runs and the harness's [`crate::FinishAnimations`] lands every spring at once,
//!   so screenshots don't depend on timing.

use bevy::prelude::*;
use bevy::ui::UiTransform;

use crate::anim::FinishAnimations;

pub struct SpringPlugin;

impl Plugin for SpringPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MotionSettings>()
            .add_observer(add_squash)
            .add_observer(on_press)
            .add_systems(
                PostUpdate,
                (start_appear, release_squash, step_springs)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// Whether springs move. Off: everything is drawn where it rests (scripted runs, and a user
/// who prefers less motion).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionSettings {
    pub enabled: bool,
}

impl Default for MotionSettings {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// How a spring moves: Raven's presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Motion {
    /// Quick, barely past the target.
    #[default]
    Snappy,
    /// Lands without overshoot.
    Smooth,
    /// Runs past and swings back.
    Bouncy,
    /// Swings back and forth a few times.
    Wobbly,
}

impl Motion {
    /// (period without friction in seconds, damping ratio).
    fn params(self) -> (f32, f32) {
        match self {
            Motion::Snappy => (0.16, 0.90),
            Motion::Smooth => (0.30, 1.0),
            Motion::Bouncy => (0.38, 0.52),
            Motion::Wobbly => (0.50, 0.32),
        }
    }
}

/// One spring: a value and its speed.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
}

impl Spring {
    pub fn at(value: f32) -> Self {
        Self {
            value,
            velocity: 0.0,
        }
    }

    pub fn moving(&self, target: f32) -> bool {
        (self.value - target).abs() > 0.0015 || self.velocity.abs() > 0.02
    }

    /// Advances the spring toward `target` by `dt` seconds.
    pub fn step(&mut self, target: f32, motion: Motion, dt: f32) {
        let (response, zeta) = motion.params();
        let omega = std::f32::consts::TAU / response;
        let x = self.value - target;
        let v = self.velocity;
        let decay = (-zeta * omega * dt).exp();
        if zeta < 0.999 {
            let omega_d = omega * (1.0 - zeta * zeta).sqrt();
            let (s, c) = (omega_d * dt).sin_cos();
            let b = (v + zeta * omega * x) / omega_d;
            self.value = target + decay * (x * c + b * s);
            self.velocity = decay
                * ((b * omega_d - zeta * omega * x) * c - (x * omega_d + zeta * omega * b) * s);
        } else {
            let b = v + omega * x;
            self.value = target + decay * (x + b * dt);
            self.velocity = decay * (b - omega * (x + b * dt));
        }
        if !self.moving(target) {
            *self = Self::at(target);
        }
    }
}

/// Keeps a button from squashing.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct NoSquash;

/// A node that squashes while pressed: `amount` is how much flatter it gets (it widens by
/// about half that).
#[derive(Component, Debug, Clone, Copy)]
pub struct Squash {
    pub amount: f32,
    press: Spring,
    pressed: bool,
}

impl Squash {
    pub fn new(amount: f32) -> Self {
        Self {
            amount,
            press: Spring::default(),
            pressed: false,
        }
    }
}

/// How a node enters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Entrance {
    /// Rises into place, growing a little.
    #[default]
    FlyUp,
    /// Slides in from the right.
    FlyLeft,
    /// Grows from small.
    Pop,
}

/// Plays an entrance when the node is spawned.
#[derive(Component, Debug, Clone, Copy)]
pub struct Appear {
    pub entrance: Entrance,
    /// Seconds before it starts (for a staggered entrance).
    pub delay: f32,
    progress: Spring,
    started: bool,
}

impl Appear {
    pub fn new(entrance: Entrance) -> Self {
        Self {
            entrance,
            delay: 0.0,
            progress: Spring::default(),
            started: false,
        }
    }

    pub fn fly_up() -> Self {
        Self::new(Entrance::FlyUp)
    }

    pub fn delay(mut self, seconds: f32) -> Self {
        self.delay = seconds;
        self
    }
}

/// Every widget button squashes (a little less when it is small).
fn add_squash(
    add: On<Add<bevy::ui_widgets::Button>>,
    q: Query<(), (Without<NoSquash>, Without<Squash>)>,
    mut commands: Commands,
) {
    if q.contains(add.entity) {
        commands.entity(add.entity).try_insert(Squash::new(0.09));
    }
}

fn on_press(press: On<PointerPress>, mut q: Query<&mut Squash>, motion: Res<MotionSettings>) {
    if !motion.enabled || press.button != PointerButton::Primary {
        return;
    }
    if let Ok(mut s) = q.get_mut(press.entity) {
        s.pressed = true;
    }
}

/// A release anywhere lets go of what was pressed: it is thrown past its shape and wobbles
/// once before settling.
fn release_squash(mouse: Res<ButtonInput<MouseButton>>, mut q: Query<&mut Squash>) {
    if mouse.pressed(MouseButton::Left) {
        return;
    }
    for mut s in &mut q {
        if s.pressed {
            s.pressed = false;
            s.press.velocity -= 11.0;
        }
    }
}

fn start_appear(motion: Res<MotionSettings>, mut q: Query<&mut Appear, Added<Appear>>) {
    for mut a in &mut q {
        a.started = true;
        a.progress = Spring::at(if motion.enabled { 0.0 } else { 1.0 });
    }
}

#[allow(clippy::type_complexity)]
fn step_springs(
    time: Res<Time>,
    motion: Res<MotionSettings>,
    mut finish: MessageReader<FinishAnimations>,
    mut q_squash: Query<(&mut Squash, &mut UiTransform), Without<Appear>>,
    mut q_appear: Query<(Entity, &mut Appear, &mut UiTransform), Without<Squash>>,
    mut commands: Commands,
) {
    let finish = finish.read().count() > 0 || !motion.enabled;
    // A long frame (a document loading) shouldn't swallow the motion.
    let dt = time.delta_secs().min(1.0 / 30.0);
    for (mut s, mut tf) in &mut q_squash {
        let target = if s.pressed { 1.0 } else { 0.0 };
        if !s.press.moving(target) && tf.scale == Vec2::ONE {
            continue;
        }
        if finish {
            s.press = Spring::at(target);
            s.pressed = false;
        } else {
            let m = if s.pressed {
                Motion::Snappy
            } else {
                Motion::Bouncy
            };
            s.press.step(target, m, dt);
        }
        let p = if finish { 0.0 } else { s.press.value };
        // Pressed: flatter and wider, like something soft; released, the spring overshoots
        // the other way.
        let scale = Vec2::new(1.0 + s.amount * 0.45 * p, 1.0 - s.amount * p);
        if tf.scale != scale {
            tf.scale = scale;
        }
    }
    for (e, mut a, mut tf) in &mut q_appear {
        if !a.started {
            continue;
        }
        if finish {
            a.progress = Spring::at(1.0);
        } else if a.delay > 0.0 {
            a.delay -= dt;
        } else {
            let m = match a.entrance {
                Entrance::FlyLeft => Motion::Snappy,
                _ => Motion::Bouncy,
            };
            a.progress.step(1.0, m, dt);
        }
        let t = a.progress.value;
        let away = 1.0 - t;
        let shrink = 1.0 - 0.08 * away.clamp(0.0, 1.0);
        let want = match a.entrance {
            Entrance::FlyUp => UiTransform {
                translation: Val2::px(0.0, 56.0 * away),
                scale: Vec2::splat(shrink),
                ..default()
            },
            Entrance::FlyLeft => UiTransform {
                translation: Val2::px(72.0 * away, 0.0),
                scale: Vec2::splat(shrink),
                ..default()
            },
            Entrance::Pop => UiTransform {
                scale: Vec2::splat(0.55 + 0.45 * t),
                ..default()
            },
        };
        if *tf != want {
            *tf = want;
        }
        if !a.progress.moving(1.0) && a.delay <= 0.0 {
            *tf = UiTransform::default();
            commands.entity(e).try_remove::<Appear>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_spring_settles_on_its_target() {
        for m in [
            Motion::Snappy,
            Motion::Smooth,
            Motion::Bouncy,
            Motion::Wobbly,
        ] {
            let mut s = Spring::at(0.0);
            let mut peak = 0.0_f32;
            for _ in 0..600 {
                s.step(1.0, m, 1.0 / 120.0);
                peak = peak.max(s.value);
            }
            assert!(!s.moving(1.0), "{m:?}: {s:?}");
            // Only the springs with a damping ratio well under 1 swing visibly past (snappy's
            // 0.9 runs over by a hair).
            let swings = peak > 1.02;
            assert_eq!(
                swings,
                matches!(m, Motion::Bouncy | Motion::Wobbly),
                "{m:?}: {peak}"
            );
        }
    }

    #[test]
    fn the_step_does_not_depend_on_the_frame_rate() {
        let (mut a, mut b) = (Spring::at(0.0), Spring::at(0.0));
        for _ in 0..30 {
            a.step(1.0, Motion::Bouncy, 1.0 / 60.0);
        }
        for _ in 0..60 {
            b.step(1.0, Motion::Bouncy, 1.0 / 120.0);
        }
        assert!((a.value - b.value).abs() < 1e-3, "{a:?} {b:?}");
    }
}

//! A loading spinner, modeled on gpui-component's `Spinner`: a grey ring with a rotating blue
//! arc (like the "Loading…" indicator shown while a document opens).

use bevy::prelude::*;
use bevy::ui::UiTransform;

use crate::theme::Theme;

pub struct SpinnerPlugin;

impl Plugin for SpinnerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spin);
    }
}

/// Rotates at `speed` turns per second.
#[derive(Component, Debug, Clone, Copy)]
pub struct Spin {
    pub speed: f32,
}

/// Builder for a spinner.
pub struct Spinner {
    name: String,
    size: f32,
    thickness: f32,
}

impl Spinner {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            size: 24.0,
            thickness: 3.0,
        }
    }

    pub fn size(mut self, size: f32) -> Self {
        self.size = size;
        self
    }

    pub fn thickness(mut self, t: f32) -> Self {
        self.thickness = t;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let track = Color::srgb_u8(0xdc, 0xdc, 0xdc);
        (
            Name::new(self.name),
            Node {
                width: Val::Px(self.size),
                height: Val::Px(self.size),
                border: UiRect::all(Val::Px(self.thickness)),
                border_radius: BorderRadius::MAX,
                flex_shrink: 0.0,
                ..default()
            },
            // One side in the accent color: a quarter arc on a grey track.
            BorderColor {
                top: track,
                right: track,
                bottom: theme.primary,
                left: track,
            },
            UiTransform::default(),
            Spin { speed: 1.2 },
        )
    }
}

fn spin(time: Res<Time>, mut q: Query<(&Spin, &mut UiTransform)>) {
    for (s, mut t) in &mut q {
        let angle =
            t.rotation.as_radians() + s.speed * std::f32::consts::TAU * time.delta_secs();
        t.rotation = Rot2::radians(angle % std::f32::consts::TAU);
    }
}

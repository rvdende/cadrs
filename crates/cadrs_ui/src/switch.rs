//! A switch, like gpui-component's `Switch` (`switch.rs`) and the "Automatically select
//! constraints" toggle of Onshape's Constraint manager (`ex1-step4.png`): a 28 × 15 px pill,
//! blue with the white knob on the right when on, grey with it on the left when off, and a
//! label after it (it may wrap to two lines).
//!
//! Clicking the switch or its label flips [`SwitchState`] and triggers [`SwitchChange`] on the
//! switch. The app may also set [`SwitchState`] itself.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::theme::Theme;

pub struct SwitchPlugin;

impl Plugin for SwitchPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_activate)
            .add_systems(PostUpdate, sync_switches.before(bevy::ui::UiSystems::Prepare));
    }
}

/// Whether a switch is on.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchState {
    pub on: bool,
}

/// The switch was flipped by the user. Targets the switch.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SwitchChange {
    pub entity: Entity,
    pub on: bool,
}

#[derive(Component, Debug, Clone, Copy)]
struct SwitchTrack(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct SwitchKnob(Entity);

const TRACK: Vec2 = Vec2::new(28.0, 15.0);
const KNOB: f32 = 11.0;

/// Builder for a switch.
pub struct Switch {
    name: Cow<'static, str>,
    label: String,
    on: bool,
    label_width: Option<f32>,
}

impl Switch {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            label: String::new(),
            on: false,
            label_width: None,
        }
    }

    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = l.into();
        self
    }

    pub fn on(mut self, on: bool) -> Self {
        self.on = on;
        self
    }

    /// Wraps the label to this width (px).
    pub fn label_width(mut self, w: f32) -> Self {
        self.label_width = Some(w);
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let Switch { name, label, on, label_width } = self;
        (
            Name::new(name.into_owned()),
            SwitchState { on },
            WidgetButton,
            Hovered::default(),
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(6.0),
                padding: UiRect::vertical(Val::Px(2.0)),
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let root = p.target_entity();
                p.spawn((
                    SwitchTrack(root),
                    Node {
                        width: Val::Px(TRACK.x),
                        height: Val::Px(TRACK.y),
                        flex_shrink: 0.0,
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(track_color(&t, on)),
                    Pickable::IGNORE,
                ))
                .with_child((
                    SwitchKnob(root),
                    Node {
                        position_type: PositionType::Absolute,
                        top: Val::Px((TRACK.y - KNOB) / 2.0),
                        left: Val::Px(knob_left(on)),
                        width: Val::Px(KNOB),
                        height: Val::Px(KNOB),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(Color::WHITE),
                    Pickable::IGNORE,
                ));
                if !label.is_empty() {
                    let mut text = p.spawn((t.text(label, 10.5, FontWeight::NORMAL, t.foreground), Pickable::IGNORE));
                    if let Some(w) = label_width {
                        text.insert((Node { width: Val::Px(w), ..default() }, TextLayout::default()));
                    }
                }
            })),
        )
    }
}

fn track_color(t: &Theme, on: bool) -> Color {
    if on { Color::srgb_u8(0x1f, 0x6f, 0xc5) } else { t.disabled_foreground }
}

fn knob_left(on: bool) -> f32 {
    let pad = (TRACK.y - KNOB) / 2.0;
    if on { TRACK.x - KNOB - pad } else { pad }
}

fn on_activate(ev: On<Activate>, mut q: Query<&mut SwitchState>, mut commands: Commands) {
    if let Ok(mut s) = q.get_mut(ev.entity) {
        s.on = !s.on;
        commands.trigger(SwitchChange { entity: ev.entity, on: s.on });
    }
}

fn sync_switches(
    theme: Res<Theme>,
    q: Query<(Entity, &SwitchState), Changed<SwitchState>>,
    mut q_track: Query<(&SwitchTrack, &mut BackgroundColor)>,
    mut q_knob: Query<(&SwitchKnob, &mut Node)>,
) {
    for (e, s) in &q {
        for (tr, mut bg) in &mut q_track {
            if tr.0 == e {
                bg.set_if_neq(BackgroundColor(track_color(&theme, s.on)));
            }
        }
        for (k, mut n) in &mut q_knob {
            if k.0 == e {
                n.left = Val::Px(knob_left(s.on));
            }
        }
    }
}

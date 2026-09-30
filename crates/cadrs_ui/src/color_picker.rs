//! Colour pickers, modeled on gpui-component's `ColorPicker` (`color_picker.rs`: featured
//! swatches, palette rows and a slider panel) and styled for Onshape's Edit appearance dialog
//! (PS9.2):
//!
//! - [`ColorSwatch`]: a small clickable square of one colour (a ring when selected). Clicking
//!   triggers [`Activate`](bevy::ui_widgets::Activate) on it; read its [`SwatchColor`].
//! - [`ColorMixer`]: a saturation/value square over the current hue, and a hue strip under it.
//!   Pressing or dragging in either moves its knob and triggers [`ColorMixerChange`] on the mixer
//!   with the new hue (degrees), saturation and value (0–1). Set [`ColorMixerState`] to move the
//!   knobs from outside (a typed hex code).
//!
//! Both are drawn with UI gradients: the square is the hue with a white-to-clear gradient
//! across and a clear-to-black one down, the strip the six hue stops.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{BackgroundGradient, ColorStop, InterpolationColorSpace, LinearGradient};
use bevy::ui_widgets::Button as WidgetButton;

use crate::style::{InitState, StateColors, Visuals};
use crate::theme::Theme;

pub struct ColorPickerPlugin;

impl Plugin for ColorPickerPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_mixer_press)
            .add_observer(on_mixer_drag)
            .add_systems(PostUpdate, (sync_mixers, sync_swatches).before(bevy::ui::UiSystems::Prepare));
    }
}

// ---------------------------------------------------------------------------------------------
// Swatch

/// A swatch's colour.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct SwatchColor(pub Color);

/// Whether a swatch is ringed as the current colour; set it to move the ring.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwatchSelected(pub bool);

const SWATCH_RING: Color = Color::srgb(0x2b as f32 / 255.0, 0x64 as f32 / 255.0, 0xc0 as f32 / 255.0);
const SWATCH_EDGE: Color = Color::srgba(0.0, 0.0, 0.0, 0.22);
const SWATCH_HOVER: Color = Color::srgb(0x55 as f32 / 255.0, 0x55 as f32 / 255.0, 0x55 as f32 / 255.0);

fn swatch_border(selected: bool) -> (StateColors, UiRect) {
    (
        StateColors::new(if selected { SWATCH_RING } else { SWATCH_EDGE }, SWATCH_HOVER, SWATCH_HOVER, SWATCH_EDGE),
        UiRect::all(Val::Px(if selected { 2.0 } else { 1.0 })),
    )
}

/// Moves the ring when [`SwatchSelected`] changes.
fn sync_swatches(mut q: Query<(&SwatchSelected, &mut Visuals, &mut Node), Changed<SwatchSelected>>) {
    for (sel, mut v, mut n) in &mut q {
        let (border, width) = swatch_border(sel.0);
        if v.border != border {
            v.border = border;
        }
        if n.border != width {
            n.border = width;
        }
    }
}

/// Builder for a colour swatch.
pub struct ColorSwatch {
    name: Cow<'static, str>,
    color: Color,
    size: f32,
    selected: bool,
    tooltip: Option<String>,
}

impl ColorSwatch {
    pub fn new(name: impl Into<Cow<'static, str>>, color: Color) -> Self {
        Self {
            name: name.into(),
            color,
            size: 20.0,
            selected: false,
            tooltip: None,
        }
    }

    pub fn size(mut self, px: f32) -> Self {
        self.size = px;
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let (border, width) = swatch_border(self.selected);
        (
            Name::new(self.name.into_owned()),
            SwatchColor(self.color),
            SwatchSelected(self.selected),
            Node {
                width: Val::Px(self.size),
                height: Val::Px(self.size),
                flex_shrink: 0.0,
                border: width,
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            Visuals {
                background: StateColors::all(self.color),
                border,
                foreground: StateColors::all(theme.foreground),
                focus_ring: theme.focus_ring,
            },
            InitState {
                disabled: false,
                selected: false,
                force: None,
            },
            crate::Tooltip::new(self.tooltip.unwrap_or_default()),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// Mixer

/// The mixer's colour: hue in degrees (0–360), saturation and value (0–1).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct ColorMixerState {
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// The mixer was pressed or dragged. Targets the mixer.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct ColorMixerChange {
    pub entity: Entity,
    pub hue: f32,
    pub saturation: f32,
    pub value: f32,
}

/// The saturation/value square; points at the mixer.
#[derive(Component, Debug, Clone, Copy)]
struct MixerSquare(Entity);

/// The hue strip; points at the mixer.
#[derive(Component, Debug, Clone, Copy)]
struct MixerHue(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct SquareKnob(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct HueKnob(Entity);

const SQUARE_KNOB: f32 = 12.0;
const HUE_KNOB: f32 = 6.0;
const HUE_HEIGHT: f32 = 12.0;

/// Builder for a colour mixer.
pub struct ColorMixer {
    name: Cow<'static, str>,
    width: f32,
    height: f32,
    state: ColorMixerState,
}

/// The fully saturated, full-value colour of `hue` (degrees).
pub fn hue_color(hue: f32) -> Color {
    Color::hsv(hue.rem_euclid(360.0), 1.0, 1.0)
}

impl ColorMixer {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            width: 240.0,
            height: 120.0,
            state: ColorMixerState {
                hue: 0.0,
                saturation: 0.0,
                value: 1.0,
            },
        }
    }

    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    /// The colour: hue (degrees), saturation and value (0–1).
    pub fn hsv(mut self, hue: f32, saturation: f32, value: f32) -> Self {
        self.state = ColorMixerState { hue, saturation, value };
        self
    }

    pub fn build(self, _theme: &Theme) -> impl Bundle {
        let (w, h) = (self.width, self.height);
        let state = self.state;
        let name = self.name.into_owned();
        let (sq_name, hue_name) = (format!("{name}-square"), format!("{name}-hue"));
        (
            Name::new(name),
            state,
            Node {
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                width: Val::Px(w),
                flex_shrink: 0.0,
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let mixer = p.target_entity();
                p.spawn((
                    Name::new(sq_name),
                    MixerSquare(mixer),
                    Node {
                        width: Val::Px(w),
                        height: Val::Px(h),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundColor(hue_color(state.hue)),
                    BackgroundGradient(vec![
                        LinearGradient {
                            color_space: InterpolationColorSpace::Srgba,
                            ..LinearGradient::to_right(vec![
                                ColorStop::percent(Color::WHITE, 0.0),
                                ColorStop::percent(Color::WHITE.with_alpha(0.0), 100.0),
                            ])
                        }
                        .into(),
                        LinearGradient {
                            color_space: InterpolationColorSpace::Srgba,
                            ..LinearGradient::to_bottom(vec![
                                ColorStop::percent(Color::BLACK.with_alpha(0.0), 0.0),
                                ColorStop::percent(Color::BLACK, 100.0),
                            ])
                        }
                        .into(),
                    ]),
                ))
                .with_child((
                    SquareKnob(mixer),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(state.saturation * w - SQUARE_KNOB / 2.0),
                        top: Val::Px((1.0 - state.value) * h - SQUARE_KNOB / 2.0),
                        width: Val::Px(SQUARE_KNOB),
                        height: Val::Px(SQUARE_KNOB),
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), Val::ZERO, Val::ZERO, Val::ZERO, Val::Px(2.0)),
                    Pickable::IGNORE,
                ));
                let stops = (0..=6)
                    .map(|i| ColorStop::percent(hue_color(i as f32 * 60.0), i as f32 * 100.0 / 6.0))
                    .collect();
                p.spawn((
                    Name::new(hue_name),
                    MixerHue(mixer),
                    Node {
                        width: Val::Px(w),
                        height: Val::Px(HUE_HEIGHT),
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    BackgroundGradient(vec![
                        LinearGradient {
                            color_space: InterpolationColorSpace::Srgba,
                            ..LinearGradient::to_right(stops)
                        }
                        .into(),
                    ]),
                ))
                .with_child((
                    HueKnob(mixer),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(state.hue / 360.0 * w - HUE_KNOB / 2.0),
                        top: Val::Px(-2.0),
                        width: Val::Px(HUE_KNOB),
                        height: Val::Px(HUE_HEIGHT + 4.0),
                        border: UiRect::all(Val::Px(1.5)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(Color::WHITE),
                    BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.5), Val::ZERO, Val::ZERO, Val::ZERO, Val::Px(2.0)),
                    Pickable::IGNORE,
                ));
            })),
        )
    }
}

/// Where the pointer is in a node, 0–1 across and down.
fn local(node: &ComputedNode, t: &bevy::ui::UiGlobalTransform, at: Vec2) -> Vec2 {
    let scale = node.inverse_scale_factor();
    let size = node.size() * scale;
    let min = t.translation * scale - size / 2.0;
    ((at - min) / size).clamp(Vec2::ZERO, Vec2::ONE)
}

#[allow(clippy::type_complexity)]
fn mix_at(
    entity: Entity,
    at: Vec2,
    q_square: &Query<(&MixerSquare, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    q_hue: &Query<(&MixerHue, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    q_state: &mut Query<&mut ColorMixerState>,
    commands: &mut Commands,
) -> bool {
    let (mixer, update): (Entity, Box<dyn Fn(&mut ColorMixerState)>) =
        if let Ok((m, node, t)) = q_square.get(entity) {
            let p = local(node, t, at);
            (m.0, Box::new(move |s: &mut ColorMixerState| {
                s.saturation = p.x;
                s.value = 1.0 - p.y;
            }))
        } else if let Ok((m, node, t)) = q_hue.get(entity) {
            let p = local(node, t, at);
            (m.0, Box::new(move |s: &mut ColorMixerState| s.hue = (p.x * 360.0).min(359.9)))
        } else {
            return false;
        };
    let Ok(mut s) = q_state.get_mut(mixer) else {
        return false;
    };
    let mut next = *s;
    update(&mut next);
    if next != *s {
        *s = next;
        commands.trigger(ColorMixerChange {
            entity: mixer,
            hue: next.hue,
            saturation: next.saturation,
            value: next.value,
        });
    }
    true
}

#[allow(clippy::type_complexity)]
fn on_mixer_press(
    mut ev: On<Pointer<Press>>,
    q_square: Query<(&MixerSquare, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    q_hue: Query<(&MixerHue, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    mut q_state: Query<&mut ColorMixerState>,
    mut commands: Commands,
) {
    if mix_at(ev.entity, ev.pointer_location.position, &q_square, &q_hue, &mut q_state, &mut commands) {
        ev.propagate(false);
    }
}

#[allow(clippy::type_complexity)]
fn on_mixer_drag(
    mut ev: On<Pointer<Drag>>,
    q_square: Query<(&MixerSquare, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    q_hue: Query<(&MixerHue, &ComputedNode, &bevy::ui::UiGlobalTransform)>,
    mut q_state: Query<&mut ColorMixerState>,
    mut commands: Commands,
) {
    if mix_at(ev.entity, ev.pointer_location.position, &q_square, &q_hue, &mut q_state, &mut commands) {
        ev.propagate(false);
    }
}

/// Moves the knobs and recolours the square when the state changes.
#[allow(clippy::type_complexity)]
fn sync_mixers(
    q: Query<(Entity, &ColorMixerState), Changed<ColorMixerState>>,
    mut q_square: Query<(&MixerSquare, &Node, &mut BackgroundColor), Without<SquareKnob>>,
    mut q_sq_knob: Query<(&SquareKnob, &mut Node), (Without<MixerSquare>, Without<HueKnob>)>,
    mut q_hue_knob: Query<(&HueKnob, &mut Node), (Without<MixerSquare>, Without<SquareKnob>)>,
) {
    for (mixer, s) in &q {
        let mut size = Vec2::ZERO;
        for (sq, node, mut bg) in &mut q_square {
            if sq.0 != mixer {
                continue;
            }
            if let (Val::Px(w), Val::Px(h)) = (node.width, node.height) {
                size = Vec2::new(w, h);
            }
            bg.set_if_neq(BackgroundColor(hue_color(s.hue)));
        }
        for (k, mut n) in &mut q_sq_knob {
            if k.0 == mixer {
                let left = Val::Px(s.saturation * size.x - SQUARE_KNOB / 2.0);
                let top = Val::Px((1.0 - s.value) * size.y - SQUARE_KNOB / 2.0);
                if n.left != left || n.top != top {
                    n.left = left;
                    n.top = top;
                }
            }
        }
        for (k, mut n) in &mut q_hue_knob {
            if k.0 == mixer {
                let left = Val::Px(s.hue / 360.0 * size.x - HUE_KNOB / 2.0);
                if n.left != left {
                    n.left = left;
                }
            }
        }
    }
}

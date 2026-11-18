//! A jog slider: a slider over a real range with the value beside it in a number field, for
//! moving one axis of a machine (or anything else that has a travel and a position), like
//! gpui-component's `Slider` with a bound `NumberInput`.
//!
//! - The **track** runs from [`JogSliderState::min`] to [`JogSliderState::max`]; dragging the
//!   knob, or pressing on the track, sets the value there (rounded to the decimals shown).
//! - A **home tick** on the track marks [`JogSliderState::home`].
//! - The **number field** shows the value with its unit ("12.5 mm", "30°") and takes a typed
//!   one, which may lie beyond the track: the knob then stops at the track's end.
//! - A value outside [`JogSliderState::limits`] shows the knob and the field in the theme's
//!   error colour (an axis jogged past its travel).
//!
//! Every change by the user triggers [`JogSliderChange`] on the slider: `done` is false while
//! the knob is being dragged and true once the drag ends, the track is clicked or a value is
//! typed, so the owner can show a cheap preview during the drag and settle after it. The
//! owner may also set [`JogSliderState`] itself (Home, Reset): the knob and the field follow.
//!
//! Names: the slider `<name>`, its track `<name>-track`, its number field `<name>-value`
//! (the editable text `<name>-value-field`).

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use crate::dialog_fields::{NumberField, NumberFieldCommit, NumberFieldState};
use crate::theme::Theme;

pub struct JogSliderPlugin;

impl Plugin for JogSliderPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_drag)
            .add_observer(on_press)
            .add_observer(on_drag_end)
            .add_observer(on_click)
            .add_observer(on_commit)
            .add_systems(
                PostUpdate,
                sync_jog_sliders.before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// A jog slider's range, value and marks.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct JogSliderState {
    /// The track's ends.
    pub min: f64,
    pub max: f64,
    pub value: f64,
    /// Where the home tick is (none: no tick).
    pub home: Option<f64>,
    /// The least and greatest value that is in range (none: every value is).
    pub limits: Option<(f64, f64)>,
    /// What follows the number: " mm", "°".
    pub unit: Cow<'static, str>,
    /// The decimals shown, and what a drag rounds to.
    pub decimals: usize,
}

impl JogSliderState {
    /// Whether the value is beyond the limits.
    pub fn out_of_range(&self) -> bool {
        self.limits
            .is_some_and(|(lo, hi)| self.value < lo - 1e-9 || self.value > hi + 1e-9)
    }

    /// Where `v` is along the track, 0 to 1 (a value beyond it at its end).
    pub fn fraction(&self, v: f64) -> f32 {
        let span = self.max - self.min;
        if span.abs() < 1e-12 || !span.is_finite() {
            return 0.5;
        }
        ((v - self.min) / span).clamp(0.0, 1.0) as f32
    }

    /// The value at a place along the track (0 to 1), rounded to the decimals shown.
    pub fn value_at(&self, f: f32) -> f64 {
        let v = self.min + (self.max - self.min) * f.clamp(0.0, 1.0) as f64;
        let k = 10f64.powi(self.decimals as i32);
        ((v * k).round() / k).clamp(self.min.min(self.max), self.max.max(self.min))
    }

    /// The value as the field shows it: "12.5 mm".
    pub fn text(&self) -> String {
        format!("{}{}", fmt_value(self.value, self.decimals), self.unit)
    }

    /// The number in typed text: its unit (or any other trailing letters and signs) dropped,
    /// a decimal comma taken as a point.
    pub fn read(&self, text: &str) -> Option<f64> {
        let t = text.trim();
        let t = t.strip_suffix(self.unit.trim()).unwrap_or(t).trim();
        let end = t
            .char_indices()
            .find(|(_, c)| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+' | 'e')))
            .map_or(t.len(), |(i, _)| i);
        t[..end]
            .replace(',', ".")
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
    }
}

/// A number with at most `decimals` decimals, its trailing zeros dropped ("12.5", "30", "-0"
/// as "0").
fn fmt_value(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

/// The user moved a jog slider or typed its value. Targets the slider.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct JogSliderChange {
    pub entity: Entity,
    pub value: f64,
    /// False while the knob is being dragged; true when the drag ends, and for a click on the
    /// track or a typed value.
    pub done: bool,
}

/// The track of a jog slider (what takes the pointer); points at the slider.
#[derive(Component, Debug, Clone, Copy)]
struct JogTrack(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct JogKnob(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct JogHome(Entity);

/// The number field of a jog slider; points at the slider.
#[derive(Component, Debug, Clone, Copy)]
struct JogField(Entity);

const KNOB: f32 = 11.0;

/// Builder for a jog slider.
pub struct JogSlider {
    name: Cow<'static, str>,
    state: JogSliderState,
    field_width: f32,
    tooltip: Option<String>,
    disabled: bool,
}

impl JogSlider {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            state: JogSliderState {
                min: 0.0,
                max: 1.0,
                value: 0.0,
                home: None,
                limits: None,
                unit: Cow::Borrowed(""),
                decimals: 2,
            },
            field_width: 76.0,
            tooltip: None,
            disabled: false,
        }
    }

    /// The track's ends.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.state.min = min;
        self.state.max = max;
        self
    }

    pub fn value(mut self, v: f64) -> Self {
        self.state.value = v;
        self
    }

    /// A tick on the track at `v`.
    pub fn home(mut self, v: f64) -> Self {
        self.state.home = Some(v);
        self
    }

    /// Values outside `min..=max` show in the error colour.
    pub fn limits(mut self, min: f64, max: f64) -> Self {
        self.state.limits = Some((min, max));
        self
    }

    /// What follows the number (" mm", "°").
    pub fn unit(mut self, unit: impl Into<Cow<'static, str>>) -> Self {
        self.state.unit = unit.into();
        self
    }

    pub fn decimals(mut self, n: usize) -> Self {
        self.state.decimals = n;
        self
    }

    /// The number field's width (76 px by default).
    pub fn field_width(mut self, w: f32) -> Self {
        self.field_width = w;
        self
    }

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    /// Shown, not movable.
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.into_owned();
        let track_name = format!("{name}-track");
        let field_name = format!("{name}-value");
        let state = self.state;
        let text = state.text();
        let at = state.fraction(state.value);
        let home = state.home.map(|h| state.fraction(h));
        let field_width = self.field_width;
        let tip = self.tooltip.unwrap_or_default();
        let disabled = self.disabled;
        (
            Name::new(name),
            state,
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(2.0),
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let slider = p.target_entity();
                let mut track = p.spawn((
                    Name::new(track_name),
                    JogTrack(slider),
                    Hovered::default(),
                    crate::Tooltip::new(tip),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(40.0),
                        height: Val::Px(KNOB + 5.0),
                        padding: UiRect::horizontal(Val::Px(KNOB / 2.0)),
                        ..default()
                    },
                ));
                if disabled {
                    track.insert(InteractionDisabled);
                }
                track.with_children(|tr| {
                    tr.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Percent(100.0),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|inner| {
                        inner.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(2.0),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(t.border),
                            Pickable::IGNORE,
                        ));
                        inner.spawn((
                            JogHome(slider),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(home.unwrap_or(0.0) * 100.0),
                                margin: UiRect::left(Val::Px(-1.0)),
                                width: Val::Px(2.0),
                                height: Val::Px(9.0),
                                display: if home.is_some() {
                                    Display::Flex
                                } else {
                                    Display::None
                                },
                                ..default()
                            },
                            BackgroundColor(t.muted_foreground),
                            Pickable::IGNORE,
                        ));
                        inner.spawn((
                            JogKnob(slider),
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Percent(at * 100.0),
                                margin: UiRect::left(Val::Px(-KNOB / 2.0)),
                                width: Val::Px(KNOB),
                                height: Val::Px(KNOB),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(t.primary),
                            BorderColor::all(t.background),
                            Pickable::IGNORE,
                        ));
                    });
                });
                p.spawn((
                    NumberField::new(field_name, "")
                        .text(text)
                        .label_width(0.0)
                        .disabled(disabled)
                        .build(&t),
                    JogField(slider),
                ))
                .entry::<Node>()
                .and_modify(move |mut n| {
                    n.width = Val::Px(field_width);
                    n.flex_shrink = 0.0;
                });
            })),
        )
    }
}

/// Sets the slider of `track` to the pointer's `x` and triggers the change.
fn slide_to(
    track: Entity,
    x: f32,
    q_track: &Query<(
        &JogTrack,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Has<InteractionDisabled>,
    )>,
    q: &mut Query<&mut JogSliderState>,
    commands: &mut Commands,
) {
    let Ok((owner, node, t, disabled)) = q_track.get(track) else {
        return;
    };
    if disabled {
        return;
    }
    let Ok(mut s) = q.get_mut(owner.0) else {
        return;
    };
    let scale = node.inverse_scale_factor();
    let w = node.size().x * scale;
    let left = t.translation.x * scale - w / 2.0;
    let f = ((x - left - KNOB / 2.0) / (w - KNOB).max(1.0)).clamp(0.0, 1.0);
    let v = s.value_at(f);
    if (s.value - v).abs() > 1e-12 {
        s.value = v;
    }
    commands.trigger(JogSliderChange {
        entity: owner.0,
        value: v,
        done: false,
    });
}

fn on_drag(
    ev: On<PointerDrag>,
    q_track: Query<(
        &JogTrack,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Has<InteractionDisabled>,
    )>,
    mut q: Query<&mut JogSliderState>,
    mut commands: Commands,
) {
    slide_to(
        ev.entity,
        ev.pointer.position.x,
        &q_track,
        &mut q,
        &mut commands,
    );
}

/// Pressing on the track moves the knob there.
fn on_press(
    ev: On<PointerPress>,
    q_track: Query<(
        &JogTrack,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        Has<InteractionDisabled>,
    )>,
    mut q: Query<&mut JogSliderState>,
    mut commands: Commands,
) {
    if ev.button == PointerButton::Primary {
        slide_to(
            ev.entity,
            ev.pointer.position.x,
            &q_track,
            &mut q,
            &mut commands,
        );
    }
}

/// The value is settled: the drag ended, or the track was clicked.
fn settle(
    track: Entity,
    q_track: &Query<(&JogTrack, Has<InteractionDisabled>)>,
    q: &Query<&JogSliderState>,
    commands: &mut Commands,
) {
    if let Ok((owner, false)) = q_track.get(track)
        && let Ok(s) = q.get(owner.0)
    {
        commands.trigger(JogSliderChange {
            entity: owner.0,
            value: s.value,
            done: true,
        });
    }
}

fn on_drag_end(
    ev: On<PointerDragEnd>,
    q_track: Query<(&JogTrack, Has<InteractionDisabled>)>,
    q: Query<&JogSliderState>,
    mut commands: Commands,
) {
    settle(ev.entity, &q_track, &q, &mut commands);
}

fn on_click(
    ev: On<PointerClick>,
    q_track: Query<(&JogTrack, Has<InteractionDisabled>)>,
    q: Query<&JogSliderState>,
    mut commands: Commands,
) {
    if ev.button == PointerButton::Primary {
        settle(ev.entity, &q_track, &q, &mut commands);
    }
}

/// A value typed in the number field (it may lie beyond the track).
fn on_commit(
    ev: On<NumberFieldCommit>,
    q_field: Query<&JogField>,
    mut q: Query<&mut JogSliderState>,
    mut commands: Commands,
) {
    let Ok(field) = q_field.get(ev.entity) else {
        return;
    };
    let Ok(mut s) = q.get_mut(field.0) else {
        return;
    };
    // Unreadable text: the field shows the value again.
    let Some(v) = s.read(&ev.text) else {
        s.set_changed();
        return;
    };
    s.value = v;
    commands.trigger(JogSliderChange {
        entity: field.0,
        value: v,
        done: true,
    });
}

/// The knob, the home tick and the number field follow the state.
fn sync_jog_sliders(
    theme: Res<Theme>,
    q: Query<(Entity, &JogSliderState), Changed<JogSliderState>>,
    mut q_knob: Query<(&JogKnob, &mut Node, &mut BackgroundColor), Without<JogHome>>,
    mut q_home: Query<(&JogHome, &mut Node), Without<JogKnob>>,
    mut q_field: Query<(&JogField, &mut NumberFieldState)>,
) {
    for (e, s) in &q {
        let bad = s.out_of_range();
        for (k, mut node, mut bg) in &mut q_knob {
            if k.0 != e {
                continue;
            }
            let left = Val::Percent(s.fraction(s.value) * 100.0);
            if node.left != left {
                node.left = left;
            }
            let c = if bad { theme.danger } else { theme.primary };
            bg.set_if_neq(BackgroundColor(c));
        }
        for (h, mut node) in &mut q_home {
            if h.0 != e {
                continue;
            }
            let (left, display) = match s.home {
                Some(v) => (Val::Percent(s.fraction(v) * 100.0), Display::Flex),
                None => (node.left, Display::None),
            };
            if node.left != left {
                node.left = left;
            }
            if node.display != display {
                node.display = display;
            }
        }
        for (f, mut state) in &mut q_field {
            if f.0 != e {
                continue;
            }
            let want = NumberFieldState {
                text: s.text(),
                error: bad,
            };
            if *state != want {
                *state = want;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(min: f64, max: f64, value: f64) -> JogSliderState {
        JogSliderState {
            min,
            max,
            value,
            home: Some(0.0),
            limits: Some((min, max)),
            unit: " mm".into(),
            decimals: 2,
        }
    }

    #[test]
    fn the_track_maps_its_range_and_stops_at_its_ends() {
        let s = state(-150.0, 150.0, 0.0);
        assert_eq!(s.fraction(-150.0), 0.0);
        assert_eq!(s.fraction(0.0), 0.5);
        assert_eq!(s.fraction(400.0), 1.0);
        assert_eq!(s.value_at(0.25), -75.0);
        assert_eq!(s.value_at(2.0), 150.0);
        // Rounded to the decimals shown.
        assert_eq!(state(0.0, 1.0, 0.0).value_at(0.3333), 0.33);
        // A range of nothing puts the knob in the middle and divides by nothing.
        assert_eq!(state(5.0, 5.0, 5.0).fraction(5.0), 0.5);
    }

    #[test]
    fn a_value_beyond_the_limits_is_out_of_range() {
        assert!(!state(-10.0, 10.0, 10.0).out_of_range());
        assert!(state(-10.0, 10.0, 10.5).out_of_range());
        assert!(state(-10.0, 10.0, -11.0).out_of_range());
        let mut free = state(-180.0, 180.0, 720.0);
        free.limits = None;
        assert!(!free.out_of_range());
    }

    #[test]
    fn the_field_shows_and_reads_the_value_with_its_unit() {
        let mut s = state(-150.0, 150.0, 12.5);
        assert_eq!(s.text(), "12.5 mm");
        s.value = -0.001;
        assert_eq!(s.text(), "0 mm");
        assert_eq!(s.read("30 mm"), Some(30.0));
        assert_eq!(s.read(" -7,25"), Some(-7.25));
        assert_eq!(s.read("45°"), Some(45.0));
        assert_eq!(s.read("far"), None);
        s.unit = "°".into();
        s.value = 30.0;
        assert_eq!(s.text(), "30°");
        assert_eq!(s.read("-90°"), Some(-90.0));
    }

    /// A typed value reaches the owner as a settled change, beyond the track too; the knob
    /// stays at the end and the field turns to the error colour.
    #[test]
    fn a_typed_value_is_a_settled_change() {
        #[derive(Resource, Default)]
        struct Got(Vec<(f64, bool)>);
        let mut app = App::new();
        app.init_resource::<Theme>()
            .init_resource::<Got>()
            .add_plugins(JogSliderPlugin)
            .add_observer(|ev: On<JogSliderChange>, mut got: ResMut<Got>| {
                got.0.push((ev.value, ev.done));
            });
        let theme = Theme::default();
        let slider = app
            .world_mut()
            .spawn(
                JogSlider::new("jog")
                    .range(-100.0, 100.0)
                    .limits(-100.0, 100.0)
                    .unit(" mm")
                    .build(&theme),
            )
            .id();
        app.update();
        let field = {
            let mut q = app.world_mut().query::<(Entity, &JogField)>();
            q.iter(app.world()).next().unwrap().0
        };
        app.world_mut().trigger(NumberFieldCommit {
            entity: field,
            text: "130 mm".into(),
            enter: true,
        });
        app.update();
        assert_eq!(app.world().resource::<Got>().0, vec![(130.0, true)]);
        let s = app.world().get::<JogSliderState>(slider).unwrap();
        assert!(s.out_of_range() && s.fraction(s.value) == 1.0);
        let f = app.world().get::<NumberFieldState>(field).unwrap();
        assert_eq!((f.text.as_str(), f.error), ("130 mm", true));
        // What cannot be read changes nothing.
        app.world_mut().trigger(NumberFieldCommit {
            entity: field,
            text: "up".into(),
            enter: true,
        });
        app.update();
        assert_eq!(app.world().resource::<Got>().0.len(), 1);
    }
}

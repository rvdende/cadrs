//! Selection fields, like the ones in Onshape's feature dialogs ("Sketch plane",
//! `reference/onshape/screens/07` and `08c`). A selection field lists what the user picked in
//! the viewport or the feature list for one parameter of a feature:
//!
//! - **Empty and waiting** (active): a pale blue box `#def1ff` with a darker border and the
//!   placeholder (the parameter's name) in small grey text. The next pick goes here.
//! - **Filled**: a white box with a small caption (the parameter's name) above the value
//!   ("Top plane") and a ✕ (named `<name>-clear`) that clears it. A filled field that still
//!   takes picks (active, like Extrude's regions) stays pale blue (`screens/23`).
//!
//! The app owns the value: it sets [`SelectionFieldState`] and reacts to
//! [`SelectionFieldClear`] (the ✕) and [`SelectionFieldActivate`] (a click on the field).

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};

use crate::button::{Button, visuals_for};
use crate::style::StateColors;
use crate::theme::Theme;

pub struct SelectionFieldPlugin;

impl Plugin for SelectionFieldPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_field_click).add_systems(
            PostUpdate,
            sync_selection_fields.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// What a selection field shows.
#[derive(Component, Debug, Clone, PartialEq, Eq, Default)]
#[require(Hovered)]
pub struct SelectionFieldState {
    /// The picked item's label, such as "Top plane".
    pub value: Option<String>,
    /// Waiting for a pick (highlighted blue while empty).
    pub active: bool,
    /// The picked item is gone (a sketch's face was deleted): the value and border are red.
    pub error: bool,
}

/// The ✕ of a selection field was clicked. Targets the field.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionFieldClear {
    pub entity: Entity,
}

/// The field was clicked (to make it the one that receives the next pick). Targets the field.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct SelectionFieldActivate {
    pub entity: Entity,
}

/// The field's placeholder (shown while empty).
#[derive(Component, Debug, Clone)]
struct FieldPlaceholder(String);

#[derive(Component, Debug, Clone, Copy)]
struct FieldCaption(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct FieldValue(Entity);

#[derive(Component, Debug, Clone, Copy)]
struct FieldClear(Entity);

/// Builder for a selection field.
#[derive(Debug, Clone)]
pub struct SelectionField {
    name: Cow<'static, str>,
    placeholder: String,
    state: SelectionFieldState,
    width: Val,
}

impl SelectionField {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            placeholder: String::new(),
            state: SelectionFieldState::default(),
            width: Val::Auto,
        }
    }

    /// The parameter's name: the placeholder while empty, the caption once filled.
    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn value(mut self, v: Option<String>) -> Self {
        self.state.value = v;
        self
    }

    pub fn active(mut self, a: bool) -> Self {
        self.state.active = a;
        self
    }

    pub fn error(mut self, e: bool) -> Self {
        self.state.error = e;
        self
    }

    pub fn width(mut self, w: Val) -> Self {
        self.width = w;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let name = self.name.into_owned();
        let clear_name = format!("{name}-clear");
        let placeholder = self.placeholder;
        let state = self.state;
        let error = state.error;
        let (node, bg, border) = field_style(&t, &state, self.width);
        let value_text = state.value.clone().unwrap_or_else(|| placeholder.clone());
        let filled = state.value.is_some();
        (
            Name::new(name),
            state,
            FieldPlaceholder(placeholder.clone()),
            node,
            bg,
            border,
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let field = p.target_entity();
                p.spawn((
                    FieldCaption(field),
                    t.text(placeholder, 10.0, FontWeight::NORMAL, t.muted_foreground),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(6.0),
                        top: Val::Px(4.0),
                        display: if filled { Display::Flex } else { Display::None },
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                let (size, color) = value_style(&t, filled, error);
                p.spawn((
                    FieldValue(field),
                    t.text(value_text, size, FontWeight::NORMAL, color),
                    Node {
                        flex_grow: 1.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                let mut ghost = visuals_for(&t, crate::ButtonVariant::Ghost);
                ghost.foreground = StateColors::new(
                    Color::srgb_u8(0x55, 0x55, 0x55),
                    t.foreground,
                    t.foreground,
                    t.disabled_foreground,
                );
                ghost.background = StateColors::all(Color::NONE);
                p.spawn((
                    FieldClear(field),
                    Button::new(clear_name)
                        .icon("close")
                        .icon_size(10.0)
                        .ghost()
                        .tooltip("Clear")
                        .build(&t),
                    observe(|a: On<Activate>, q: Query<&FieldClear>, mut commands: Commands| {
                        if let Ok(c) = q.get(a.entity) {
                            commands.trigger(SelectionFieldClear { entity: c.0 });
                        }
                    }),
                ))
                .insert((
                    ghost,
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(18.0),
                        // Centered on the value line.
                        margin: UiRect::new(Val::ZERO, Val::Px(2.0), Val::Px(4.0), Val::ZERO),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        display: if filled { Display::Flex } else { Display::None },
                        ..default()
                    },
                ));
            })),
        )
    }
}

fn value_style(t: &Theme, filled: bool, error: bool) -> (f32, Color) {
    if filled && error {
        (t.font_base, t.feature_error)
    } else if filled {
        (t.font_base, t.tool_foreground)
    } else {
        (10.5, Color::srgb_u8(0x3d, 0x4b, 0x52))
    }
}

fn field_style(t: &Theme, s: &SelectionFieldState, width: Val) -> (Node, BackgroundColor, BorderColor) {
    let filled = s.value.is_some();
    let node = Node {
        width,
        flex_grow: 1.0,
        height: Val::Px(if filled { 38.0 } else { 24.0 }),
        padding: if filled {
            UiRect::new(Val::Px(6.0), Val::Px(4.0), Val::Px(14.0), Val::ZERO)
        } else {
            UiRect::horizontal(Val::Px(6.0))
        },
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(2.0)),
        ..default()
    };
    // Waiting for picks (empty, or a field that takes several such as Extrude's regions): pale
    // blue (`screens/22`, `23`).
    let (bg, border) = if s.error {
        (t.background, t.feature_error)
    } else if s.active {
        (t.selection_field_active, t.selection_field_active_border)
    } else {
        (t.background, Color::srgb_u8(0xe0, 0xe0, 0xe0))
    };
    (node, BackgroundColor(bg), BorderColor::all(border))
}

fn on_field_click(
    mut click: On<Pointer<Click>>,
    q: Query<&SelectionFieldState>,
    mut commands: Commands,
) {
    if click.button != PointerButton::Primary {
        return;
    }
    if q.contains(click.entity) {
        click.propagate(false);
        commands.trigger(SelectionFieldActivate {
            entity: click.entity,
        });
    }
}

#[allow(clippy::type_complexity)]
fn sync_selection_fields(
    theme: Res<Theme>,
    mut q: Query<
        (
            Entity,
            &SelectionFieldState,
            &FieldPlaceholder,
            &mut Node,
            &mut BackgroundColor,
            &mut BorderColor,
        ),
        Changed<SelectionFieldState>,
    >,
    mut q_caption: Query<(&FieldCaption, &mut Node), (Without<SelectionFieldState>, Without<FieldClear>)>,
    mut q_value: Query<(&FieldValue, &mut Text, &mut TextFont, &mut TextColor)>,
    mut q_clear: Query<(&FieldClear, &mut Node), (Without<SelectionFieldState>, Without<FieldCaption>)>,
) {
    for (e, s, placeholder, mut node, mut bg, mut border) in &mut q {
        let (n, b, br) = field_style(&theme, s, node.width);
        *node = n;
        bg.set_if_neq(b);
        border.set_if_neq(br);
        let filled = s.value.is_some();
        let display = if filled { Display::Flex } else { Display::None };
        for (c, mut n) in &mut q_caption {
            if c.0 == e && n.display != display {
                n.display = display;
            }
        }
        for (c, mut n) in &mut q_clear {
            if c.0 == e && n.display != display {
                n.display = display;
            }
        }
        for (v, mut text, mut font, mut color) in &mut q_value {
            if v.0 != e {
                continue;
            }
            let (size, c) = value_style(&theme, filled, s.error);
            let want = s.value.as_ref().unwrap_or(&placeholder.0);
            if text.0 != *want {
                text.0 = want.clone();
            }
            font.font_size = bevy::text::FontSize::Px(size);
            color.set_if_neq(TextColor(c));
        }
    }
}

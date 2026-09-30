//! A search box with a list of results under it, modeled on gpui-component's command palette
//! and Onshape's toolbar **Search tools** box (P3.9, PS2.3): a text field and, below it, rows of
//! matches (icon, name, shortcut), one of them highlighted.
//!
//! [`CommandPalette::build`] spawns a full-window, invisible layer (so a click outside closes
//! the palette: it triggers [`CommandPaletteClose`] on the layer) holding the field, named
//! `<name>-input`, at `left`/`top`, and the results container `<name>-results`. The app owns the
//! matching: it fills the results with [`palette_row`]s whenever the text changes, and handles
//! `Activate` on the rows, [`TextSubmit`](crate::TextSubmit) (Enter) and
//! [`TextCancel`](crate::TextCancel) (Escape) of the field.

use std::borrow::Cow;

use bevy::prelude::*;

use crate::input::TextInput;
use crate::list::ListItem;
use crate::theme::Theme;
use crate::z;

pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_layer_press);
    }
}

/// The invisible layer of an open palette (its root).
#[derive(Component, Debug, Clone, Copy)]
pub struct CommandPaletteLayer;

/// The container of a palette's result rows.
#[derive(Component, Debug, Clone, Copy)]
pub struct CommandPaletteResults;

/// A press outside the palette. Targets the layer; the owner closes (despawns) it.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct CommandPaletteClose {
    pub entity: Entity,
}

/// Builder for a command palette.
pub struct CommandPalette {
    name: Cow<'static, str>,
    placeholder: String,
    value: String,
    left: f32,
    top: f32,
    width: f32,
}

impl CommandPalette {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self { name: name.into(), placeholder: String::new(), value: String::new(), left: 0.0, top: 0.0, width: 240.0 }
    }

    pub fn placeholder(mut self, p: impl Into<String>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn value(mut self, v: impl Into<String>) -> Self {
        self.value = v.into();
        self
    }

    /// Where the field's top-left corner goes (window px) and how wide it is.
    pub fn at(mut self, left: f32, top: f32, width: f32) -> Self {
        self.left = left;
        self.top = top;
        self.width = width;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme;
        let input = TextInput::new(format!("{}-input", self.name))
            .placeholder(self.placeholder)
            .value(self.value)
            .height(30.0)
            .autofocus()
            .build(t);
        let results = (
            Name::new(format!("{}-results", self.name)),
            CommandPaletteResults,
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(Val::Px(3.0)),
                padding: UiRect::vertical(Val::Px(4.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(t.radius)),
                display: Display::None,
                ..default()
            },
            BackgroundColor(t.popover),
            BorderColor::all(t.border),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.16), Val::Px(0.0), Val::Px(3.0), Val::Px(0.0), Val::Px(8.0)),
        );
        (
            Name::new(self.name.into_owned()),
            CommandPaletteLayer,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(z::MENU),
            children![(
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(self.left),
                    top: Val::Px(self.top),
                    width: Val::Px(self.width),
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                children![input, results],
            )],
        )
    }
}

/// One result row: `id` names it `<palette>-item-<id>`; `detail` is the shortcut, shown on the
/// right.
#[allow(clippy::too_many_arguments)]
pub fn palette_row(
    t: &Theme,
    palette: &str,
    id: &str,
    label: &str,
    icon: &str,
    detail: Option<&str>,
    disabled: bool,
    highlighted: bool,
) -> impl Bundle {
    let mut item = ListItem::new(format!("{palette}-item-{id}"))
        .icon(icon.to_string())
        .label(label.to_string())
        .height(26.0)
        .padding_left(8.0)
        .selected(highlighted)
        .disabled(disabled);
    if let Some(d) = detail {
        item = item.detail(d.to_string());
    }
    item.build(t)
}

fn on_layer_press(press: On<Pointer<Press>>, q: Query<(), With<CommandPaletteLayer>>, mut commands: Commands) {
    if q.contains(press.entity) && press.original_event_target() == press.entity {
        commands.trigger(CommandPaletteClose { entity: press.entity });
    }
}

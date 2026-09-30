//! A list row with trailing action icons, like gpui-component's `ListItem` with a `suffix` of
//! icon buttons, as the rows of Onshape's Constraint manager draw them (`ex1-step5.png`): an
//! optional type icon, the label (semibold for a header row, red for an error), and small grey
//! icon buttons at the right (a trash can, a ×). Child rows are indented and plain.
//!
//! The row is named by the builder, each action `<row>-<action>`. Clicking the row triggers
//! `Activate` on it (as any button); clicking an action triggers [`ActionRowAction`] on the
//! row and not `Activate`. A row is hovered while `Hovered` says so (the app reads it to
//! highlight what the row stands for).

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, Button as WidgetButton, observe};

use crate::button::{IconButton, visuals_for};
use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, Visuals};
use crate::theme::Theme;

/// An action icon of a row was clicked. Targets the row.
#[derive(EntityEvent, Debug, Clone)]
pub struct ActionRowAction {
    pub entity: Entity,
    /// The action's key, as given to [`ActionRow::action`].
    pub action: String,
}

/// An action: its key, icon, tooltip and whether it is enabled.
type ActionSpec = (String, Cow<'static, str>, String, bool);

/// Builder for an action row.
pub struct ActionRow {
    name: Cow<'static, str>,
    label: String,
    icon: Option<Cow<'static, str>>,
    header: bool,
    error: bool,
    selected: bool,
    indent: f32,
    height: f32,
    actions: Vec<ActionSpec>,
}

impl ActionRow {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon: None,
            header: false,
            error: false,
            selected: false,
            indent: 0.0,
            height: 20.0,
            actions: Vec::new(),
        }
    }

    /// A type icon before the label.
    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(i.into());
        self
    }

    /// A group's header row: semibold label.
    pub fn header(mut self, h: bool) -> Self {
        self.header = h;
        self
    }

    /// The label (and icon) in the error red.
    pub fn error(mut self, e: bool) -> Self {
        self.error = e;
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    /// Left indent in px (child rows).
    pub fn indent(mut self, px: f32) -> Self {
        self.indent = px;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    /// A trailing icon button (`key` names it `<row>-<key>` and comes back in
    /// [`ActionRowAction`]).
    pub fn action(mut self, key: impl Into<String>, icon: impl Into<Cow<'static, str>>, tooltip: impl Into<String>, enabled: bool) -> Self {
        self.actions.push((key.into(), icon.into(), tooltip.into(), enabled));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let ActionRow {
            name,
            label,
            icon: icon_name,
            header,
            error,
            selected,
            indent,
            height,
            actions,
        } = self;
        let prefix = name.to_string();
        let fg = if error { t.feature_error } else { t.foreground };
        let visuals = Visuals {
            background: StateColors::new(Color::NONE, Color::srgba(0.0, 0.3, 0.7, 0.08), Color::srgba(0.0, 0.3, 0.7, 0.14), Color::NONE)
                .with_selected(t.list_selected),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::all(fg),
            focus_ring: t.focus_ring,
        };
        (
            Name::new(name.into_owned()),
            WidgetButton,
            Hovered::default(),
            visuals,
            InitState { disabled: false, selected, force: None },
            Node {
                height: Val::Px(height),
                flex_shrink: 0.0,
                padding: UiRect::new(Val::Px(4.0 + indent), Val::Px(2.0), Val::ZERO, Val::ZERO),
                align_items: AlignItems::Center,
                column_gap: Val::Px(4.0),
                ..default()
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let row = p.target_entity();
                if let Some(i) = icon_name {
                    p.spawn((icon(i, 14.0, fg), InheritFg, Pickable::IGNORE));
                }
                p.spawn((
                    Text::new(label),
                    // An error header and a selected row read bold (`ex1-step5.png`,
                    // `ex1-step8.png`).
                    t.font(
                        11.0,
                        if (header && error) || selected {
                            FontWeight::BOLD
                        } else if header {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        },
                    ),
                    TextColor(fg),
                    TextLayout::no_wrap(),
                    InheritFg,
                    crate::ellipsis::Ellipsis::node(),
                    crate::ellipsis::Ellipsis::default(),
                    Pickable::IGNORE,
                ));
                // The actions sit at the right edge.
                p.spawn((Node { flex_grow: 1.0, ..default() }, Pickable::IGNORE));
                for (key, icon_name, tip, enabled) in actions {
                    let mut v = visuals_for(&t, crate::ButtonVariant::Ghost);
                    v.foreground = StateColors::new(
                        Color::srgb_u8(0x8c, 0x8c, 0x8c),
                        t.foreground,
                        t.foreground,
                        Color::srgb_u8(0xcf, 0xcf, 0xcf),
                    );
                    let k = key.clone();
                    p.spawn((
                        IconButton::new(format!("{prefix}-{key}"), icon_name)
                            .icon_size(13.0)
                            .tooltip(tip)
                            .disabled(!enabled)
                            .build(&t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            commands.trigger(ActionRowAction { entity: row, action: k.clone() });
                        }),
                    ))
                    .insert(v)
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(16.0);
                        n.min_width = Val::Px(16.0);
                        n.height = Val::Px(16.0);
                        n.flex_shrink = 0.0;
                    });
                }
            })),
        )
    }
}

//! Document tabs, modeled on gpui-component's `TabBar`/`Tab` (underline variant) and Onshape's
//! bottom tab bar: fixed-width grey tabs with a type icon and a name; the selected tab is white
//! with a blue underline.
//!
//! A [`Tab`] is [`DoubleClickable`], an [`InlineEdit`] (its label is an [`InlineEditLabel`], so
//! double-click-to-rename is one [`crate::begin_inline_edit`] call away) and a
//! [`ContextMenuTarget`]. Clicking it triggers `bevy::ui_widgets::Activate`.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::icon::icon;
use crate::inline_edit::{DoubleClickable, InlineEdit, InlineEditLabel};
use crate::menu::ContextMenuTarget;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

/// Builder for a tab.
pub struct Tab {
    name: Cow<'static, str>,
    label: String,
    icon: Option<Cow<'static, str>>,
    selected: bool,
    force: Option<VisualState>,
    width: f32,
    marked: bool,
}

impl Tab {
    pub fn new(name: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            label: label.into(),
            icon: None,
            selected: false,
            force: None,
            width: 183.0,
            marked: false,
        }
    }

    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(i.into());
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// Marks a tab that isn't selected but holds the selection (a folder tab with the active tab
    /// inside it): a lighter underline in the selection's colour and a bolder label.
    pub fn marked(mut self, m: bool) -> Self {
        self.marked = m;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let visuals = Visuals {
            background: StateColors::new(
                theme.tab_inactive,
                theme.tab_hover,
                theme.tab_hover,
                theme.tab_inactive,
            )
            .with_selected(theme.background),
            border: StateColors::all(if self.marked { theme.tab_underline.with_alpha(0.55) } else { Color::NONE }).with_selected(theme.tab_underline),
            foreground: StateColors::new(
                theme.tool_foreground,
                theme.foreground,
                theme.foreground,
                theme.disabled_foreground,
            )
            .with_selected(theme.foreground),
            focus_ring: theme.focus_ring,
        };
        let fg = visuals.foreground.normal;
        let font = theme.font(theme.font_base, if self.marked { FontWeight::SEMIBOLD } else { FontWeight::MEDIUM });
        let icon_name = self.icon;
        let label = self.label;
        (
            Name::new(self.name.into_owned()),
            Node {
                width: Val::Px(self.width),
                flex_shrink: 0.0,
                margin: UiRect::top(Val::Px(2.0)),
                padding: UiRect::new(Val::Px(10.0), Val::Px(8.0), Val::ZERO, Val::ZERO),
                align_items: AlignItems::Center,
                column_gap: Val::Px(5.0),
                border: UiRect::bottom(Val::Px(2.0)),
                border_radius: BorderRadius::top(Val::Px(theme.radius)),
                overflow: Overflow::clip(),
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            visuals,
            InitState {
                disabled: false,
                selected: self.selected,
                force: self.force,
            },
            DoubleClickable,
            InlineEdit::default(),
            ContextMenuTarget,
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                if let Some(i) = icon_name {
                    p.spawn((icon(i, 16.0, fg), InheritFg, Pickable::IGNORE));
                }
                // A long name ends in "…" rather than being cut through a letter.
                p.spawn((
                    Text::new(label),
                    font,
                    TextColor(fg),
                    TextLayout::no_wrap(),
                    InheritFg,
                    InlineEditLabel,
                    Pickable::IGNORE,
                    crate::ellipsis::Ellipsis::node(),
                    crate::ellipsis::Ellipsis::default(),
                ));
            })),
        )
    }
}

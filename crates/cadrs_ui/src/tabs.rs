//! Document tabs, modeled on gpui-component's `TabBar`/`Tab` (pill variant): fixed-width tabs
//! with a type icon and a name, on the tab bar's glass; the selected tab is a tinted pill.
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

/// The narrowest a tab is (a short name still makes a good target).
pub const MIN_TAB_WIDTH: f32 = 120.0;

/// A tab's icon after its name (a folder tab's chevron, which its owner turns to show the
/// folder open or closed).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TabTrailingIcon;

/// Builder for a tab.
pub struct Tab {
    name: Cow<'static, str>,
    label: String,
    icon: Option<Cow<'static, str>>,
    trailing: Option<Cow<'static, str>>,
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
            trailing: None,
            selected: false,
            force: None,
            width: 240.0,
            marked: false,
        }
    }

    pub fn icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(i.into());
        self
    }

    /// An icon after the name, at the tab's right end ([`TabTrailingIcon`]).
    pub fn trailing_icon(mut self, i: impl Into<Cow<'static, str>>) -> Self {
        self.trailing = Some(i.into());
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

    /// The widest the tab gets: it is as wide as its name, from [`MIN_TAB_WIDTH`] up to this.
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// Marks a tab that isn't selected but holds the selection (a folder tab with the active tab
    /// inside it): an underline and its icon in the accent colour, and a bolder label.
    pub fn marked(mut self, m: bool) -> Self {
        self.marked = m;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let visuals = Visuals {
            background: StateColors::new(
                if self.marked {
                    theme.control
                } else {
                    Color::NONE
                },
                theme.control_hover,
                theme.control_hover,
                Color::NONE,
            )
            .with_selected(theme.list_selected),
            border: StateColors::all(if self.marked {
                theme.primary
            } else {
                Color::NONE
            }),
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
        let icon_fg = if self.marked { theme.primary } else { fg };
        // The chevron in the text's colour, not the tab's lighter grey: it reads at its size.
        let chevron_fg = theme.foreground;
        let font = theme.font(
            theme.font_base,
            if self.marked {
                FontWeight::SEMIBOLD
            } else {
                FontWeight::MEDIUM
            },
        );
        let icon_name = self.icon;
        let trailing = self.trailing;
        let label = self.label;
        (
            Name::new(self.name.into_owned()),
            Node {
                min_width: Val::Px(MIN_TAB_WIDTH.min(self.width)),
                max_width: Val::Px(self.width),
                flex_shrink: 0.0,
                padding: UiRect::new(Val::Px(10.0), Val::Px(10.0), Val::ZERO, Val::ZERO),
                // A short name sits in the middle of the narrowest tab, not to one side.
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(if self.marked { 2.0 } else { 0.0 })),
                column_gap: Val::Px(6.0),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
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
                    let mut e = p.spawn((icon(i, 16.0, icon_fg), Pickable::IGNORE));
                    if icon_fg == fg {
                        e.insert(InheritFg);
                    }
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
                if let Some(i) = trailing {
                    p.spawn((
                        crate::icon::icon_in(
                            i,
                            18.0,
                            chevron_fg,
                            Node {
                                flex_shrink: 0.0,
                                margin: UiRect::left(Val::Px(-2.0)),
                                ..default()
                            },
                        ),
                        Pickable::IGNORE,
                        TabTrailingIcon,
                    ));
                }
            })),
        )
    }
}

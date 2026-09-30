//! Toolbar pieces, modeled on Onshape's feature toolbar and gpui-component's `Button` in
//! toolbar use: [`ToolButton`] (an icon, optionally followed by a ▾ for tools with variants),
//! [`toolbar_separator`] (the thin rule between groups) and [`Kbd`] (a keycap, as in
//! "Search tools… alt c").
//!
//! Tool buttons trigger `bevy::ui_widgets::Activate` like every other button.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::button::ButtonTooltip;
use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

/// Builder for a toolbar button: a 20 px icon in a 28 px square, or a wider button when it has a
/// dropdown caret or a label (like "✎ Sketch").
#[derive(Debug, Clone)]
pub struct ToolButton {
    name: Cow<'static, str>,
    icon: Cow<'static, str>,
    label: Option<String>,
    dropdown: bool,
    disabled: bool,
    selected: bool,
    force: Option<VisualState>,
    tooltip: Option<String>,
    icon_size: f32,
}

impl ToolButton {
    pub fn new(name: impl Into<Cow<'static, str>>, icon: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            icon: icon.into(),
            label: None,
            dropdown: false,
            disabled: false,
            selected: false,
            force: None,
            tooltip: None,
            icon_size: 24.0,
        }
    }

    /// A text label after the icon (Onshape shows one only on Sketch and Insert).
    pub fn label(mut self, l: impl Into<String>) -> Self {
        self.label = Some(l.into());
        self
    }

    /// Adds a small ▾ after the icon, for tools with a menu of variants.
    pub fn dropdown(mut self, d: bool) -> Self {
        self.dropdown = d;
        self
    }

    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
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

    pub fn tooltip(mut self, t: impl Into<String>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_size = px;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let visuals = Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.ghost_hover,
                theme.ghost_active,
                Color::NONE,
            )
            .with_selected(theme.list_selected),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(
                theme.tool_foreground,
                theme.foreground,
                theme.foreground,
                theme.tool_disabled_foreground,
            )
            .with_selected(theme.link),
            focus_ring: theme.focus_ring,
        };
        let fg = visuals.foreground.normal;
        let icon_name = self.icon;
        let icon_size = self.icon_size;
        let dropdown = self.dropdown;
        let label = self.label.clone();
        // The caret gets its own name so scenarios can open the dropdown without coordinates.
        let caret_name = format!("{}-caret", self.name);
        // Onshape's text tool buttons (Sketch, Insert) are semibold (`screens/05a`).
        let font = theme.font(theme.font_base, FontWeight::SEMIBOLD);
        let pad = if label.is_some() {
            6.0
        } else if dropdown {
            4.0
        } else {
            0.0
        };
        (
            Name::new(self.name.into_owned()),
            Node {
                height: Val::Px(32.0),
                min_width: Val::Px(32.0),
                padding: UiRect::horizontal(Val::Px(pad)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(if label.is_some() { 5.0 } else { 3.0 }),
                flex_shrink: 0.0,
                ..default()
            },
            WidgetButton,
            Hovered::default(),
            TabIndex(0),
            visuals,
            InitState {
                disabled: self.disabled,
                selected: self.selected,
                force: self.force,
            },
            ButtonTooltip(self.tooltip),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                p.spawn((icon(icon_name, icon_size, fg), InheritFg, Pickable::IGNORE));
                if let Some(l) = label {
                    p.spawn((
                        Text::new(l),
                        font,
                        TextColor(fg),
                        TextLayout::no_wrap(),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
                if dropdown {
                    p.spawn((
                        Name::new(caret_name),
                        icon("chevron-down", 14.0, fg),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
            })),
        )
    }
}

/// The thin vertical rule between toolbar groups.
pub fn toolbar_separator(theme: &Theme) -> impl Bundle {
    (
        Name::new("toolbar-separator"),
        Node {
            width: Val::Px(1.0),
            height: Val::Px(26.0),
            margin: UiRect::horizontal(Val::Px(4.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme.toolbar_separator),
        Pickable::IGNORE,
    )
}

/// A keycap, like gpui-component's `Kbd`: small text in a bordered box.
pub struct Kbd {
    text: String,
}

impl Kbd {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        (
            Node {
                height: Val::Px(14.0),
                padding: UiRect::horizontal(Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BorderColor::all(theme.kbd_border),
            BackgroundColor(theme.background),
            Pickable::IGNORE,
            children![(
                theme.text(self.text, 10.0, FontWeight::NORMAL, theme.muted_foreground),
                Pickable::IGNORE,
            )],
        )
    }
}

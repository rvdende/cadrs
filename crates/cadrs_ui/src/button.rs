//! Buttons, modeled on gpui-component's `Button` (`button/button.rs`): a builder with variants,
//! sizes, an optional leading icon, an optional dropdown caret, and disabled/selected states.
//!
//! Activation (click, or Enter/Space when focused) triggers `bevy::ui_widgets::Activate` on the
//! button entity. Attach a handler with `observe(|a: On<Activate>| ..)`.
//!
//! ```ignore
//! commands.spawn((
//!     Button::new("create").label("Create").primary().dropdown_caret().build(&theme),
//!     observe(|_: On<Activate>| info!("clicked")),
//! ));
//! ```

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input_focus::tab_navigation::TabIndex;
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Button as WidgetButton;

use crate::icon::icon;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;
use crate::tooltip::Tooltip;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonVariant {
    /// Blue, for the main action ("Create", "Share").
    Primary,
    /// Grey ("Cancel").
    #[default]
    Secondary,
    /// No background until hovered (toolbar and icon buttons, menu triggers).
    Ghost,
    /// Plain text in the link color.
    Link,
    /// White with a grey border ("Plans and pricing"-style secondary actions).
    Outline,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ButtonSize {
    Small,
    #[default]
    Medium,
    Large,
}

/// Builder for a button. Every button needs a unique `name`, used as its `Name` so scenarios can
/// click it.
#[derive(Debug, Clone)]
pub struct Button {
    name: Cow<'static, str>,
    label: Option<String>,
    icon: Option<Cow<'static, str>>,
    variant: ButtonVariant,
    size: ButtonSize,
    caret: bool,
    disabled: bool,
    selected: bool,
    force: Option<VisualState>,
    tooltip: Option<String>,
    width: Option<Val>,
    square: bool,
    icon_px: Option<f32>,
}

impl Button {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            label: None,
            icon: None,
            variant: ButtonVariant::default(),
            size: ButtonSize::default(),
            caret: false,
            disabled: false,
            selected: false,
            force: None,
            tooltip: None,
            width: None,
            square: false,
            icon_px: None,
        }
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// A leading icon (an icon-rs icon name).
    pub fn icon(mut self, icon: impl Into<Cow<'static, str>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn variant(mut self, v: ButtonVariant) -> Self {
        self.variant = v;
        self
    }

    pub fn primary(self) -> Self {
        self.variant(ButtonVariant::Primary)
    }

    pub fn secondary(self) -> Self {
        self.variant(ButtonVariant::Secondary)
    }

    pub fn ghost(self) -> Self {
        self.variant(ButtonVariant::Ghost)
    }

    pub fn link(self) -> Self {
        self.variant(ButtonVariant::Link)
    }

    pub fn outline(self) -> Self {
        self.variant(ButtonVariant::Outline)
    }

    pub fn size(mut self, s: ButtonSize) -> Self {
        self.size = s;
        self
    }

    pub fn small(self) -> Self {
        self.size(ButtonSize::Small)
    }

    pub fn large(self) -> Self {
        self.size(ButtonSize::Large)
    }

    /// Adds a ▾ caret after the label, for buttons that open a menu.
    pub fn dropdown_caret(mut self) -> Self {
        self.caret = true;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Pins the visual state (for galleries and documentation).
    pub fn force_state(mut self, state: VisualState) -> Self {
        self.force = Some(state);
        self
    }

    pub fn tooltip(mut self, text: impl Into<String>) -> Self {
        self.tooltip = Some(text.into());
        self
    }

    pub fn width(mut self, w: Val) -> Self {
        self.width = Some(w);
        self
    }

    /// Overrides the icon size.
    pub fn icon_size(mut self, px: f32) -> Self {
        self.icon_px = Some(px);
        self
    }

    /// Makes the button square (width = height), for icon-only buttons.
    pub fn square(mut self) -> Self {
        self.square = true;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let height = match self.size {
            ButtonSize::Small => theme.button_height_sm,
            ButtonSize::Medium => theme.button_height,
            ButtonSize::Large => theme.button_height_lg,
        };
        let font_size = match self.size {
            ButtonSize::Small => theme.font_sm,
            _ => theme.font_base,
        };
        let icon_size = self.icon_px.unwrap_or(match self.size {
            ButtonSize::Small => 14.0,
            _ => theme.icon_size,
        });
        let pad_x = if self.square {
            0.0
        } else if self.label.is_none() {
            theme.space[3]
        } else {
            match self.size {
                ButtonSize::Small => theme.space[4],
                ButtonSize::Medium => theme.space[5],
                ButtonSize::Large => theme.space[6],
            }
        };
        let visuals = visuals_for(theme, self.variant);
        let fg = visuals.foreground.normal;
        let weight = match self.variant {
            ButtonVariant::Primary => FontWeight::MEDIUM,
            _ => FontWeight::NORMAL,
        };
        let text_font = theme.font(font_size, weight);
        let caret_color = fg;
        let label = self.label.clone();
        let icon_name = self.icon.clone();
        let caret = self.caret;

        let width = if self.square {
            Val::Px(height)
        } else {
            self.width.unwrap_or(Val::Auto)
        };
        (
            Name::new(self.name.into_owned()),
            Node {
                height: Val::Px(height),
                width,
                min_width: Val::Px(height),
                padding: UiRect::horizontal(Val::Px(pad_x)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(theme.radius)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                column_gap: Val::Px(theme.space[3]),
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
                if let Some(name) = icon_name {
                    p.spawn((icon(name, icon_size, fg), InheritFg, Pickable::IGNORE));
                }
                if let Some(text) = label {
                    p.spawn((
                        Text::new(text),
                        text_font,
                        TextColor(fg),
                        TextLayout::no_wrap(),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
                if caret {
                    p.spawn((
                        icon("caret-down", 12.0, caret_color),
                        InheritFg,
                        Pickable::IGNORE,
                    ));
                }
            })),
        )
    }
}

/// Carries an optional tooltip from the builder; turned into a [`Tooltip`] on insert.
#[derive(Component, Clone, Debug, Default)]
#[component(on_insert = on_button_tooltip)]
pub struct ButtonTooltip(pub Option<String>);

fn on_button_tooltip(
    mut world: bevy::ecs::world::DeferredWorld,
    ctx: bevy::ecs::lifecycle::HookContext,
) {
    let Some(text) = world
        .get::<ButtonTooltip>(ctx.entity)
        .and_then(|t| t.0.clone())
    else {
        return;
    };
    world
        .commands()
        .entity(ctx.entity)
        .insert(Tooltip::new(text));
}

/// An icon-only button, like gpui-component's `Button::new(..).icon(..)` with no label: square,
/// ghost by default, usually with a tooltip.
pub struct IconButton;

impl IconButton {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(name: impl Into<Cow<'static, str>>, icon: impl Into<Cow<'static, str>>) -> Button {
        Button::new(name).icon(icon).ghost().square()
    }
}

/// The colors of each button variant.
pub fn visuals_for(theme: &Theme, variant: ButtonVariant) -> Visuals {
    match variant {
        ButtonVariant::Primary => Visuals {
            background: StateColors::new(
                theme.primary,
                theme.primary_hover,
                theme.primary_active,
                theme.primary_disabled,
            )
            .with_selected(theme.primary_open),
            border: StateColors::new(
                theme.primary,
                theme.primary_hover,
                theme.primary_active,
                theme.primary_disabled,
            )
            .with_selected(theme.primary_open_border),
            foreground: StateColors::all(theme.primary_foreground),
            focus_ring: theme.focus_ring,
        },
        ButtonVariant::Secondary => Visuals {
            background: StateColors::new(
                theme.secondary,
                theme.secondary_hover,
                theme.secondary_active,
                theme.secondary_disabled,
            ),
            border: StateColors::new(
                theme.secondary,
                theme.secondary_hover,
                theme.secondary_active,
                theme.secondary_disabled,
            ),
            foreground: StateColors::new(
                theme.foreground,
                theme.foreground,
                theme.foreground,
                theme.disabled_foreground,
            ),
            focus_ring: theme.focus_ring,
        },
        ButtonVariant::Ghost => Visuals {
            background: StateColors::new(
                Color::NONE,
                theme.ghost_hover,
                theme.ghost_active,
                Color::NONE,
            )
            .with_selected(theme.list_selected),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(
                theme.muted_foreground,
                theme.foreground,
                theme.foreground,
                theme.disabled_foreground,
            )
            .with_selected(theme.link),
            focus_ring: theme.focus_ring,
        },
        // Selected: a toggle shown pressed (the feature dialogs' Final), light blue with the
        // link colour's border and label.
        ButtonVariant::Outline => Visuals {
            background: StateColors::new(
                theme.background,
                theme.ghost_hover,
                theme.ghost_active,
                theme.background,
            )
            .with_selected(theme.list_selected),
            border: StateColors::new(
                theme.border_strong,
                theme.border_strong,
                theme.border_strong,
                theme.border,
            )
            .with_selected(theme.link),
            foreground: StateColors::new(
                theme.foreground,
                theme.foreground,
                theme.foreground,
                theme.disabled_foreground,
            )
            .with_selected(theme.link),
            focus_ring: theme.focus_ring,
        },
        ButtonVariant::Link => Visuals {
            background: StateColors::all(Color::NONE),
            border: StateColors::all(Color::NONE),
            foreground: StateColors::new(
                theme.link,
                theme.primary_hover,
                theme.primary_active,
                theme.disabled_foreground,
            ),
            focus_ring: theme.focus_ring,
        },
    }
}

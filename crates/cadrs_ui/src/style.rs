//! Interaction states and the colors that go with them.
//!
//! Interactive widgets carry a [`Visuals`] component with a color per [`VisualState`] for the
//! background, border and foreground. [`apply_visuals`] resolves the current state from
//! `Hovered`, `Pressed`, `InteractionDisabled`, [`Selected`] and keyboard focus, then writes the
//! colors. Descendants marked [`InheritFg`] (labels and icons) take the foreground color.
//! [`ForceState`] pins a state, which the gallery uses to show every state at once.

use bevy::ecs::lifecycle::HookContext;
use bevy::ecs::world::DeferredWorld;
use bevy::input_focus::{InputFocus, InputFocusVisible};
use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::{InteractionDisabled, Pressed};

pub struct StylePlugin;

impl Plugin for StylePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            PostUpdate,
            apply_visuals.before(bevy::ui::UiSystems::Prepare),
        );
    }
}

/// The visual state of an interactive widget, in priority order (disabled wins).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
pub enum VisualState {
    #[default]
    Normal,
    Hover,
    Pressed,
    Selected,
    Disabled,
}

/// One color per state. `selected` falls back to `pressed`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StateColors {
    pub normal: Color,
    pub hover: Color,
    pub pressed: Color,
    pub selected: Color,
    pub disabled: Color,
}

impl StateColors {
    pub const fn all(c: Color) -> Self {
        Self {
            normal: c,
            hover: c,
            pressed: c,
            selected: c,
            disabled: c,
        }
    }

    pub const fn new(normal: Color, hover: Color, pressed: Color, disabled: Color) -> Self {
        Self {
            normal,
            hover,
            pressed,
            selected: pressed,
            disabled,
        }
    }

    pub const fn with_selected(mut self, c: Color) -> Self {
        self.selected = c;
        self
    }

    pub fn get(&self, s: VisualState) -> Color {
        match s {
            VisualState::Normal => self.normal,
            VisualState::Hover => self.hover,
            VisualState::Pressed => self.pressed,
            VisualState::Selected => self.selected,
            VisualState::Disabled => self.disabled,
        }
    }
}

/// Colors for an interactive widget.
#[derive(Component, Debug, Clone, PartialEq)]
#[require(BackgroundColor, BorderColor, Outline)]
pub struct Visuals {
    pub background: StateColors,
    pub border: StateColors,
    pub foreground: StateColors,
    /// Outline color when focused from the keyboard.
    pub focus_ring: Color,
}

/// Marks a widget as selected (a current list row, an active tab, a toggled tool).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct Selected;

/// Pins a widget's visual state regardless of input.
#[derive(Component, Debug, Clone, Copy)]
pub struct ForceState(pub VisualState);

/// Labels and icons inside a [`Visuals`] widget that take its foreground color.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct InheritFg;

/// Initial state flags for a widget built with a builder. Its insert hook adds
/// `InteractionDisabled`, [`Selected`] or [`ForceState`] as needed, which keeps builder bundles
/// a single static type.
#[derive(Component, Debug, Clone, Copy, Default)]
#[component(on_insert = on_init_state)]
pub struct InitState {
    pub disabled: bool,
    pub selected: bool,
    pub force: Option<VisualState>,
}

fn on_init_state(mut world: DeferredWorld, ctx: HookContext) {
    let Some(init) = world.get::<InitState>(ctx.entity).copied() else {
        return;
    };
    let mut commands = world.commands();
    let mut e = commands.entity(ctx.entity);
    if init.disabled {
        e.try_insert(InteractionDisabled);
    }
    if init.selected {
        e.try_insert(Selected);
    }
    if let Some(s) = init.force {
        e.try_insert(ForceState(s));
    }
}

fn resolve_state(
    hovered: Option<&Hovered>,
    pressed: bool,
    disabled: bool,
    selected: bool,
    force: Option<&ForceState>,
) -> VisualState {
    if let Some(f) = force {
        return f.0;
    }
    if disabled {
        VisualState::Disabled
    } else if pressed {
        VisualState::Pressed
    } else if selected {
        VisualState::Selected
    } else if hovered.is_some_and(|h| h.get()) {
        VisualState::Hover
    } else {
        VisualState::Normal
    }
}

#[allow(clippy::type_complexity)]
pub(crate) fn apply_visuals(
    mut q: Query<(
        Entity,
        &Visuals,
        Option<&Hovered>,
        Has<Pressed>,
        Has<InteractionDisabled>,
        Has<Selected>,
        Option<&ForceState>,
        &mut BackgroundColor,
        &mut BorderColor,
        &mut Outline,
        Option<&Children>,
    )>,
    q_children: Query<&Children>,
    mut q_fg: Query<
        (Option<&mut TextColor>, Option<&mut ImageNode>, Has<crate::icon::FullColour>),
        With<InheritFg>,
    >,
    q_nested: Query<(), With<Visuals>>,
    focus: Option<Res<InputFocus>>,
    focus_visible: Option<Res<InputFocusVisible>>,
) {
    let focused = focus.as_ref().and_then(|f| f.get());
    let show_focus = focus_visible.is_some_and(|v| v.0);
    for (
        entity,
        visuals,
        hovered,
        pressed,
        disabled,
        selected,
        force,
        mut bg,
        mut border,
        mut outline,
        children,
    ) in &mut q
    {
        let state = resolve_state(hovered, pressed, disabled, selected, force);
        bg.set_if_neq(BackgroundColor(visuals.background.get(state)));
        border.set_if_neq(BorderColor::all(visuals.border.get(state)));
        let ring = if show_focus && focused == Some(entity) && state != VisualState::Disabled {
            Outline::new(Val::Px(2.0), Val::Px(1.0), visuals.focus_ring)
        } else {
            Outline::new(Val::Px(0.0), Val::Px(0.0), Color::NONE)
        };
        outline.set_if_neq(ring);

        let fg = visuals.foreground.get(state);
        let mut stack: Vec<Entity> = children.map(|c| c.to_vec()).unwrap_or_default();
        while let Some(child) = stack.pop() {
            if q_nested.contains(child) {
                continue;
            }
            if let Ok((text, image, full_colour)) = q_fg.get_mut(child) {
                if let Some(mut t) = text {
                    t.set_if_neq(TextColor(fg));
                }
                // Full-colour icons keep their colours; the state only fades them.
                let tint = if full_colour { Color::WHITE.with_alpha(fg.alpha()) } else { fg };
                if let Some(mut i) = image
                    && i.color != tint
                {
                    i.color = tint;
                }
            }
            if let Ok(c) = q_children.get(child) {
                stack.extend(c.iter());
            }
        }
    }
}

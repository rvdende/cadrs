//! The mouse cursor, chosen per context like a web app's CSS `cursor` (gpui's `CursorStyle`):
//!
//! - a hovered, enabled button (anything with Bevy's widget `Button`: buttons, tool buttons, tabs,
//!   menu items, list rows) shows the **pointer** hand;
//! - a hovered, enabled text input shows the **text** caret;
//! - otherwise the app decides through [`CursorRequest`]: `capture` wins over everything (an
//!   orbit, a pan or a drag in progress keeps its cursor even over a panel), `viewport` applies
//!   when no UI element under the pointer asks for a cursor (crosshair while drawing, move over
//!   draggable geometry).
//!
//! The result is [`CursorState`], which sets the primary window's [`CursorIcon`]. Screenshots
//! cannot show OS cursors, so the scenario harness draws a software sprite of
//! [`CursorState::kind`] at the synthetic pointer (see `cadrs_harness::cursor`).

use bevy::picking::hover::Hovered;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::Button as WidgetButton;
use bevy::window::{CursorIcon, PrimaryWindow, SystemCursorIcon};

use crate::input::TextInputFrame;

pub struct CursorPlugin;

impl Plugin for CursorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CursorRequest>()
            .init_resource::<CursorState>()
            .add_systems(
                PostUpdate,
                (resolve_cursor, apply_window_cursor)
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            );
    }
}

/// The cursors cadrs uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CursorKind {
    /// The arrow.
    #[default]
    Default,
    /// The pointing hand (clickable UI).
    Pointer,
    /// The text caret (I-beam).
    Text,
    /// Drawing in a sketch.
    Crosshair,
    /// Hovering something that can be dragged (a sketch point, curve, dimension label or
    /// glyph), and panning.
    Move,
    /// Orbiting the view, or dragging sketch geometry.
    Grabbing,
    /// Something that cannot be used here.
    NotAllowed,
    /// A divider between stacked panes, hovered or dragged ([`crate::splitter`]).
    RowResize,
}

impl CursorKind {
    /// The OS cursor for this kind.
    pub fn system(self) -> SystemCursorIcon {
        match self {
            CursorKind::Default => SystemCursorIcon::Default,
            CursorKind::Pointer => SystemCursorIcon::Pointer,
            CursorKind::Text => SystemCursorIcon::Text,
            CursorKind::Crosshair => SystemCursorIcon::Crosshair,
            CursorKind::Move => SystemCursorIcon::Move,
            CursorKind::Grabbing => SystemCursorIcon::Grabbing,
            CursorKind::NotAllowed => SystemCursorIcon::NotAllowed,
            CursorKind::RowResize => SystemCursorIcon::RowResize,
        }
    }
}

/// What the app asks for this frame. The app overwrites it every frame it runs its cursor
/// system; leaving a field `None` means "no opinion".
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CursorRequest {
    /// An interaction in progress (orbit, pan, drag): wins over UI hover.
    pub capture: Option<CursorKind>,
    /// The cursor over the viewport when no UI element under the pointer has one.
    pub viewport: Option<CursorKind>,
}

/// The cursor shown now.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CursorState {
    pub kind: CursorKind,
}

/// The cursor a hovered UI element asks for, if any.
fn ui_cursor(
    q_buttons: &Query<(&Hovered, Has<InteractionDisabled>), With<WidgetButton>>,
    q_inputs: &Query<(&Hovered, &Children), With<TextInputFrame>>,
    q_disabled: &Query<(), With<InteractionDisabled>>,
    q_fields: &Query<(&Hovered, Has<InteractionDisabled>), With<crate::input::TextInputField>>,
) -> Option<CursorKind> {
    for (h, children) in q_inputs {
        if h.get() && !children.iter().any(|c| q_disabled.contains(c)) {
            return Some(CursorKind::Text);
        }
    }
    if q_fields.iter().any(|(h, disabled)| h.get() && !disabled) {
        return Some(CursorKind::Text);
    }
    q_buttons
        .iter()
        .any(|(h, disabled)| h.get() && !disabled)
        .then_some(CursorKind::Pointer)
}

#[allow(clippy::too_many_arguments)]
fn resolve_cursor(
    request: Res<CursorRequest>,
    q_hover_cursor: Query<(&Hovered, &crate::splitter::HoverCursor, Option<&crate::splitter::Splitter>)>,
    q_buttons: Query<(&Hovered, Has<InteractionDisabled>), With<WidgetButton>>,
    q_inputs: Query<(&Hovered, &Children), With<TextInputFrame>>,
    q_disabled: Query<(), With<InteractionDisabled>>,
    q_fields: Query<(&Hovered, Has<InteractionDisabled>), With<crate::input::TextInputField>>,
    mut state: ResMut<CursorState>,
) {
    // An element with its own cursor (a divider): while dragged (over anything), else while
    // hovered and nothing captures the pointer.
    let dragged = q_hover_cursor.iter().find(|(_, _, s)| s.is_some_and(|s| s.dragging())).map(|(_, c, _)| c.0);
    let hovered = q_hover_cursor.iter().find(|(h, ..)| h.get()).map(|(_, c, _)| c.0);
    let kind = dragged
        .or(request.capture)
        .or(hovered)
        .or_else(|| ui_cursor(&q_buttons, &q_inputs, &q_disabled, &q_fields))
        .or(request.viewport)
        .unwrap_or_default();
    if state.kind != kind {
        state.kind = kind;
    }
}

fn apply_window_cursor(
    state: Res<CursorState>,
    q: Query<(Entity, Option<&CursorIcon>), With<PrimaryWindow>>,
    mut commands: Commands,
) {
    let want = CursorIcon::System(state.kind.system());
    for (e, icon) in &q {
        if icon != Some(&want) {
            commands.entity(e).insert(want.clone());
        }
    }
}

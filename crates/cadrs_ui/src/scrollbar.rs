//! A slim vertical scrollbar for a scroll area, like gpui-component's `Scrollbar` and the one
//! Onshape's document list shows (`reference/onshape/screens/01`: a 6 px grey thumb in a light
//! track at the right edge, with small ▲/▼ steppers). Built on Bevy's headless
//! [`Scrollbar`] widget, so dragging the thumb and clicking the track work.
//!
//! The scrollbar hides itself while the content fits.

use bevy::prelude::*;
use bevy::ui::ScrollPosition;
use bevy::ui_widgets::{ControlOrientation, Scrollbar, ScrollbarThumb};

use crate::theme::Theme;

pub struct ScrollbarPlugin;

impl Plugin for ScrollbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PostUpdate, (hide_unneeded, sync_gutters).after(bevy::ui::UiSystems::Layout))
            .add_observer(on_step);
    }
}

/// A scrollbar spawned by [`vertical_scrollbar`] or [`horizontal_scrollbar`].
#[derive(Component, Debug, Clone, Copy)]
pub struct SlimScrollbar {
    pub target: Entity,
}

/// On a scroll area whose rows run to its right edge: while its content overflows (the
/// scrollbar shows), this much right padding (px) keeps the rows' ends clear of the scrollbar;
/// while it fits, none, so the rows keep their width.
#[derive(Component, Debug, Clone, Copy)]
pub struct ScrollGutter(pub f32);

fn sync_gutters(mut q: Query<(&ScrollGutter, &ComputedNode, &mut Node)>) {
    for (g, computed, mut node) in &mut q {
        let overflows = computed.content_size().y > computed.size().y + 0.5;
        let want = Val::Px(if overflows { g.0 } else { 0.0 });
        if node.padding.right != want {
            node.padding.right = want;
        }
    }
}

/// Marks a [`SlimScrollbar`] that scrolls sideways.
#[derive(Component, Debug, Clone, Copy)]
struct Horizontal;

/// A slim horizontal scrollbar for `target` (a node with `overflow: scroll` on x), positioned
/// absolutely along the bottom edge of its parent, over the content (a wide table in a narrow
/// panel). It hides while the content fits. Name it with `name`.
pub fn horizontal_scrollbar(name: &str, target: Entity) -> impl Bundle {
    (
        Name::new(name.to_string()),
        SlimScrollbar { target },
        Horizontal,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(2.0),
            right: Val::Px(2.0),
            bottom: Val::Px(1.0),
            height: Val::Px(8.0),
            ..default()
        },
        Scrollbar::new(target, ControlOrientation::Horizontal, 24.0),
        children![(
            Name::new(format!("{name}-thumb")),
            ScrollbarThumb {
                border_radius: BorderRadius::all(Val::Px(3.0)),
                border: UiRect::ZERO,
            },
            BackgroundColor(Color::srgba_u8(0x80, 0x80, 0x80, 0xb0)),
        )],
    )
}

/// A ▲ or ▼ stepper: scrolls the target by one row.
#[derive(Component, Debug, Clone, Copy)]
struct Stepper {
    target: Entity,
    dir: f32,
}

/// How far a stepper click scrolls (px).
const STEP: f32 = 38.0;

/// A vertical scrollbar for `target` (a node with `overflow: scroll_y` and `ScrollArea`),
/// positioned absolutely along the right edge of its parent. Name it with `name`.
pub fn vertical_scrollbar(theme: &Theme, name: &str, target: Entity) -> impl Bundle {
    let arrow = theme.muted_foreground;
    (
        Name::new(name.to_string()),
        SlimScrollbar { target },
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(0.0),
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            width: Val::Px(12.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            ..default()
        },
        children![
            stepper(target, -1.0, "caret-up-filled", arrow),
            (
                Name::new(format!("{name}-track")),
                Node {
                    flex_grow: 1.0,
                    width: Val::Px(6.0),
                    margin: UiRect::vertical(Val::Px(2.0)),
                    ..default()
                },
                Scrollbar::new(target, ControlOrientation::Vertical, 24.0),
                children![(
                    Name::new(format!("{name}-thumb")),
                    ScrollbarThumb {
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        border: UiRect::ZERO,
                    },
                    BackgroundColor(Color::srgb_u8(0x9e, 0x9e, 0x9e)),
                )],
            ),
            stepper(target, 1.0, "caret-down-filled", arrow),
        ],
    )
}

fn stepper(target: Entity, dir: f32, icon_name: &'static str, color: Color) -> impl Bundle {
    (
        Stepper { target, dir },
        Node {
            width: Val::Px(12.0),
            height: Val::Px(10.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            flex_shrink: 0.0,
            ..default()
        },
        bevy::ui_widgets::Button,
        bevy::picking::hover::Hovered::default(),
        children![(crate::icon::icon(icon_name, 10.0, color), Pickable::IGNORE)],
    )
}

fn on_step(
    ev: On<bevy::ui_widgets::Activate>,
    q: Query<&Stepper>,
    mut q_target: Query<(&ComputedNode, &mut ScrollPosition)>,
) {
    let Ok(s) = q.get(ev.entity) else {
        return;
    };
    let Ok((node, mut pos)) = q_target.get_mut(s.target) else {
        return;
    };
    let visible = node.size().y * node.inverse_scale_factor();
    let content = node.content_size().y * node.inverse_scale_factor();
    let max = (content - visible).max(0.0);
    pos.y = (pos.y + s.dir * STEP).clamp(0.0, max);
}

fn hide_unneeded(
    mut q: Query<(&SlimScrollbar, Has<Horizontal>, &mut Visibility)>,
    q_target: Query<&ComputedNode>,
) {
    for (bar, horizontal, mut vis) in &mut q {
        let Ok(node) = q_target.get(bar.target) else {
            continue;
        };
        let overflows = if horizontal {
            node.content_size().x > node.size().x + 0.5
        } else {
            node.content_size().y > node.size().y + 0.5
        };
        vis.set_if_neq(if overflows {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

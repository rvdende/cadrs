//! A small floating tool panel, like gpui-component's `Popover`/`Sheet` content and Onshape's
//! sketch diagnostic panels (`reference/onshape/training/inspection-and-repair/ex1-step5.png`,
//! `ex1-step7.png`): a white card with a thin grey border and a soft shadow, a header with the
//! title (12 px bold) and a grey × on the right, the body in a column, and an optional footer
//! row. The header drags the panel.
//!
//! The panel root gets the builder's name, the × `<name>-close`, the header `<name>-header`
//! and the body `<name>-body`. Clicking the × triggers [`FloatingPanelClose`] on the panel;
//! the app despawns it.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};

use crate::button::{IconButton, visuals_for};
use crate::style::StateColors;
use crate::theme::Theme;

pub struct FloatingPanelPlugin;

impl Plugin for FloatingPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_header_drag);
    }
}

/// The × of a floating panel was clicked. Targets the panel.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct FloatingPanelClose {
    pub entity: Entity,
}

/// A panel's header (drags the panel).
#[derive(Component, Debug, Clone, Copy)]
struct PanelHeader(Entity);

/// The panel's body (a column the app fills; rebuilt freely).
#[derive(Component, Debug, Clone, Copy)]
pub struct FloatingPanelBody(pub Entity);

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// Builder for a floating panel.
pub struct FloatingPanel {
    name: Cow<'static, str>,
    title: String,
    width: f32,
    at: Vec2,
    max_height: Option<f32>,
    body: Option<SpawnFn>,
    footer: Option<SpawnFn>,
}

impl FloatingPanel {
    pub fn new(name: impl Into<Cow<'static, str>>, title: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            title: title.into(),
            width: 206.0,
            at: Vec2::ZERO,
            max_height: None,
            body: None,
            footer: None,
        }
    }

    /// Width in px (default 206, the Constraint manager's width at our dialog scale).
    pub fn width(mut self, w: f32) -> Self {
        self.width = w;
        self
    }

    /// Where the top-left corner goes in its parent (px).
    pub fn at(mut self, left: f32, top: f32) -> Self {
        self.at = Vec2::new(left, top);
        self
    }

    /// The tallest the panel may get (px); its body then shrinks, and a scrolling list in it
    /// (`overflow: scroll_y`, `min_height` small) takes what is left.
    pub fn max_height(mut self, h: f32) -> Self {
        self.max_height = Some(h);
        self
    }

    /// Spawns the body's content (laid out in a column).
    pub fn body(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.body = Some(Box::new(f));
        self
    }

    /// Spawns a footer row under the body (laid out in a row, items centred).
    pub fn footer(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.footer = Some(Box::new(f));
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let t = theme.clone();
        let FloatingPanel {
            name,
            title,
            width,
            at,
            max_height,
            body,
            footer,
        } = self;
        let close_name = format!("{name}-close");
        let header_name = format!("{name}-header");
        let body_name = format!("{name}-body");
        let title_name = format!("{name}-title");
        (
            Name::new(name.into_owned()),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x),
                top: Val::Px(at.y),
                width: Val::Px(width),
                max_height: max_height.map_or(Val::Auto, Val::Px),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(t.background),
            BorderColor::all(Color::srgb_u8(0xdc, 0xdc, 0xdc)),
            BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.35), Val::Px(1.0), Val::Px(2.0), Val::Px(0.0), Val::Px(6.0)),
            GlobalZIndex(crate::z::DIALOG - 9),
            Pickable::default(),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let panel = p.target_entity();
                p.spawn((
                    Name::new(header_name),
                    PanelHeader(panel),
                    Node {
                        height: Val::Px(26.0),
                        flex_shrink: 0.0,
                        padding: UiRect::new(Val::Px(6.0), Val::Px(2.0), Val::ZERO, Val::ZERO),
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(Color::srgb_u8(0xe6, 0xe6, 0xe6)),
                    Pickable::default(),
                ))
                .with_children(|h| {
                    h.spawn((
                        Name::new(title_name),
                        t.text(title, t.font_sm, FontWeight::BOLD, t.foreground),
                        Node { flex_grow: 1.0, ..default() },
                        Pickable::IGNORE,
                    ));
                    let mut close = visuals_for(&t, crate::ButtonVariant::Ghost);
                    close.foreground = StateColors::new(
                        Color::srgb_u8(0x8a, 0x8a, 0x8a),
                        t.foreground,
                        t.foreground,
                        t.disabled_foreground,
                    );
                    h.spawn((
                        IconButton::new(close_name, "close").icon_size(14.0).tooltip("Close").build(&t),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            commands.trigger(FloatingPanelClose { entity: panel });
                        }),
                    ))
                    .insert(close)
                    .entry::<Node>()
                    .and_modify(|mut n| {
                        n.width = Val::Px(20.0);
                        n.height = Val::Px(20.0);
                    });
                });
                p.spawn((
                    Name::new(body_name),
                    FloatingPanelBody(panel),
                    Node {
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 1.0,
                        min_height: Val::Px(0.0),
                        padding: UiRect::new(Val::Px(6.0), Val::Px(6.0), Val::Px(5.0), Val::Px(2.0)),
                        ..default()
                    },
                ))
                .with_children(|b| {
                    if let Some(f) = body {
                        f(b);
                    }
                });
                if let Some(f) = footer {
                    p.spawn(Node {
                        min_height: Val::Px(24.0),
                        flex_shrink: 0.0,
                        padding: UiRect::new(Val::Px(6.0), Val::Px(4.0), Val::Px(2.0), Val::Px(4.0)),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|r| f(r));
                }
            })),
        )
    }
}

/// Dragging the header moves the panel.
fn on_header_drag(ev: On<Pointer<Drag>>, q: Query<&PanelHeader>, mut q_node: Query<&mut Node>) {
    let Ok(h) = q.get(ev.entity) else { return };
    let Ok(mut n) = q_node.get_mut(h.0) else { return };
    let px = |v: Val| if let Val::Px(x) = v { x } else { 0.0 };
    let (left, top) = (px(n.left), px(n.top));
    n.left = Val::Px(left + ev.delta.x);
    n.top = Val::Px(top + ev.delta.y);
}

/// A small grey section caption inside a panel ("Type", "Mode", "Loose ends").
pub fn panel_caption(t: &Theme, text: impl Into<String>) -> impl Bundle {
    (
        t.text(text, 10.5, FontWeight::MEDIUM, Color::srgb_u8(0x55, 0x5b, 0x60)),
        Node { margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(4.0), Val::Px(2.0)), ..default() },
        Pickable::IGNORE,
    )
}

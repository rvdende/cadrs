//! A draggable divider between two stacked panes, like gpui-component's `Resizable` panels and
//! the divider between Onshape's feature list and Parts list: a 1 px line with a taller
//! invisible grip; hovering it shows the row-resize cursor, dragging it resizes the pane above
//! (the one before it), and the pane below takes the rest. Each pane keeps a minimum height.
//!
//! The pane above needs a fixed height (`Val::Px` or `Val::Percent`) and `flex_shrink: 0`; the
//! pane below grows (`flex_grow: 1`, `min_height: 0`). A finished drag triggers
//! [`SplitterMoved`] on the splitter with the new height, so the app can remember it.

use bevy::picking::hover::Hovered;
use bevy::prelude::*;

use crate::cursor::CursorKind;
use crate::theme::Theme;

pub struct SplitterPlugin;

impl Plugin for SplitterPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end);
    }
}

/// A divider that resizes `target`, the pane before it.
#[derive(Component, Debug, Clone, Copy)]
pub struct Splitter {
    pub target: Entity,
    /// The least height (px) the pane above may have.
    pub min_before: f32,
    /// The least height (px) left for the pane below.
    pub min_after: f32,
    start: f32,
    dragging: bool,
}

impl Splitter {
    /// Whether the divider is being dragged.
    pub fn dragging(&self) -> bool {
        self.dragging
    }
}

/// A splitter's drag ended: the pane above is now `height` px tall.
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct SplitterMoved {
    pub entity: Entity,
    pub target: Entity,
    pub height: f32,
}

/// A horizontal divider for `target` (the pane before it in their parent's column). Name it with
/// `name`; each pane keeps at least `min_before` / `min_after` px.
pub fn horizontal_splitter(theme: &Theme, name: &str, target: Entity, min_before: f32, min_after: f32) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Splitter {
            target,
            min_before,
            min_after,
            start: 0.0,
            dragging: false,
        },
        HoverCursor(CursorKind::RowResize),
        Hovered::default(),
        Node {
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(theme.panel_border),
        // The grip reaches over the panes' edges: above them.
        ZIndex(10),
        children![(
            Name::new(format!("{name}-grip")),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(-3.0),
                bottom: Val::Px(-3.0),
                ..default()
            },
            // Transparent, but picked (a node without a background is still hit).
            Pickable::default(),
        )],
    )
}

/// Shows its cursor kind while the element is hovered (see [`crate::cursor`]).
#[derive(Component, Debug, Clone, Copy)]
pub struct HoverCursor(pub CursorKind);

fn on_drag_start(
    mut ev: On<Pointer<DragStart>>,
    mut q: Query<&mut Splitter>,
    q_node: Query<&ComputedNode>,
) {
    let Ok(mut s) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    let Ok(n) = q_node.get(s.target) else {
        return;
    };
    s.start = n.size().y * n.inverse_scale_factor();
    s.dragging = true;
}

fn on_drag(
    mut ev: On<Pointer<Drag>>,
    q: Query<(&Splitter, &ChildOf)>,
    q_computed: Query<&ComputedNode>,
    mut q_node: Query<&mut Node>,
) {
    let Ok((s, parent)) = q.get(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !s.dragging {
        return;
    }
    // The pane above may take the parent's height less the divider and the pane below's minimum.
    let Ok(p) = q_computed.get(parent.parent()) else {
        return;
    };
    let room = p.size().y * p.inverse_scale_factor() - 1.0 - s.min_after;
    let h = (s.start + ev.distance.y).clamp(s.min_before, room.max(s.min_before)).round();
    if let Ok(mut n) = q_node.get_mut(s.target)
        && n.height != Val::Px(h)
    {
        n.height = Val::Px(h);
    }
}

fn on_drag_end(mut ev: On<Pointer<DragEnd>>, mut q: Query<&mut Splitter>, q_node: Query<&Node>, mut commands: Commands) {
    let Ok(mut s) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !s.dragging {
        return;
    }
    s.dragging = false;
    if let Ok(Node { height: Val::Px(h), .. }) = q_node.get(s.target) {
        commands.trigger(SplitterMoved {
            entity: ev.entity,
            target: s.target,
            height: *h,
        });
    }
}

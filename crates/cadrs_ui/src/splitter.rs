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
    /// The pane it resizes is the one after it (it is that pane's top edge, [`top_edge`]):
    /// dragging up makes it taller. `min_before` is then the least height left above it.
    below: bool,
    /// It resizes the pane's width, as its left edge ([`left_edge`]): dragging left makes it
    /// wider. `min_before` is then the least width left beside it.
    across: bool,
}

impl Splitter {
    /// Whether the divider is being dragged.
    pub fn dragging(&self) -> bool {
        self.dragging
    }
}

/// A splitter's drag ended: the pane is now `height` px tall (for a [`left_edge`], that
/// many px wide).
#[derive(EntityEvent, Clone, Copy, Debug)]
pub struct SplitterMoved {
    pub entity: Entity,
    pub target: Entity,
    pub height: f32,
}

/// A horizontal divider for `target` (the pane before it in their parent's column). Name it with
/// `name`; each pane keeps at least `min_before` / `min_after` px.
pub fn horizontal_splitter(
    theme: &Theme,
    name: &str,
    target: Entity,
    min_before: f32,
    min_after: f32,
) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Splitter {
            target,
            min_before,
            min_after,
            start: 0.0,
            dragging: false,
            below: false,
            across: false,
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

/// The top edge of `target`, a pane at the bottom of a column (a panel under the 3D view), as a
/// grip that resizes it: dragging up makes it taller, the pane above taking the rest. Goes in
/// `target` (absolutely placed over its top edge, `reach` px above it); `target` needs a fixed
/// height and `flex_shrink: 0`. Each keeps at least `min_above` / `min_height` px.
pub fn top_edge(
    name: &str,
    target: Entity,
    reach: f32,
    min_above: f32,
    min_height: f32,
) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Splitter {
            target,
            min_before: min_above,
            min_after: min_height,
            start: 0.0,
            dragging: false,
            below: true,
            across: false,
        },
        HoverCursor(CursorKind::RowResize),
        Hovered::default(),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(-reach),
            height: Val::Px(reach + 3.0),
            ..default()
        },
        // Transparent, but picked (a node without a background is still hit).
        Pickable::default(),
        ZIndex(10),
    )
}

/// The left edge of `target`, a pane on the right of a row (a side panel beside the 3D view),
/// as a grip that resizes it: dragging left makes it wider, the panes beside it taking the
/// rest. Goes in `target` (absolutely placed over its left edge, `reach` px out of it, so
/// `target` must not clip); `target` needs a fixed width and `flex_shrink: 0`. It keeps at
/// least `min_width` px, and leaves at least `min_beside` px beside it.
pub fn left_edge(
    name: &str,
    target: Entity,
    reach: f32,
    min_beside: f32,
    min_width: f32,
) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Splitter {
            target,
            min_before: min_beside,
            min_after: min_width,
            start: 0.0,
            dragging: false,
            below: true,
            across: true,
        },
        HoverCursor(CursorKind::ColResize),
        Hovered::default(),
        Node {
            position_type: PositionType::Absolute,
            top: Val::Px(0.0),
            bottom: Val::Px(0.0),
            left: Val::Px(-reach),
            width: Val::Px(reach + 3.0),
            ..default()
        },
        // Transparent, but picked (a node without a background is still hit).
        Pickable::default(),
        ZIndex(10),
    )
}

/// Shows its cursor kind while the element is hovered (see [`crate::cursor`]).
#[derive(Component, Debug, Clone, Copy)]
pub struct HoverCursor(pub CursorKind);

fn on_drag_start(
    mut ev: On<PointerDragStart>,
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
    let size = n.size() * n.inverse_scale_factor();
    s.start = if s.across { size.x } else { size.y };
    s.dragging = true;
}

fn on_drag(
    mut ev: On<PointerDrag>,
    q: Query<(&Splitter, &ChildOf)>,
    q_parent: Query<&ChildOf>,
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
    if s.across {
        // The pane's row: it may take the row's width less the least left beside it.
        let Some(p) = q_parent
            .get(s.target)
            .ok()
            .and_then(|c| q_computed.get(c.parent()).ok())
        else {
            return;
        };
        let room = p.size().x * p.inverse_scale_factor() - s.min_before;
        let w = (s.start - ev.distance.x)
            .clamp(s.min_after, room.max(s.min_after))
            .round();
        if let Ok(mut n) = q_node.get_mut(s.target)
            && n.width != Val::Px(w)
        {
            n.width = Val::Px(w);
        }
        return;
    }
    if s.below {
        // The pane's column: it may take the column's height less the pane above's minimum.
        let Some(p) = q_parent
            .get(s.target)
            .ok()
            .and_then(|c| q_computed.get(c.parent()).ok())
        else {
            return;
        };
        let room = p.size().y * p.inverse_scale_factor() - s.min_before;
        let h = (s.start - ev.distance.y)
            .clamp(s.min_after, room.max(s.min_after))
            .round();
        if let Ok(mut n) = q_node.get_mut(s.target)
            && n.height != Val::Px(h)
        {
            n.height = Val::Px(h);
        }
        return;
    }
    // The pane above may take the parent's height less the divider and the pane below's minimum.
    let Ok(p) = q_computed.get(parent.parent()) else {
        return;
    };
    let room = p.size().y * p.inverse_scale_factor() - 1.0 - s.min_after;
    let h = (s.start + ev.distance.y)
        .clamp(s.min_before, room.max(s.min_before))
        .round();
    if let Ok(mut n) = q_node.get_mut(s.target)
        && n.height != Val::Px(h)
    {
        n.height = Val::Px(h);
    }
}

fn on_drag_end(
    mut ev: On<PointerDragEnd>,
    mut q: Query<&mut Splitter>,
    q_node: Query<&Node>,
    mut commands: Commands,
) {
    let Ok(mut s) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !s.dragging {
        return;
    }
    s.dragging = false;
    let size = q_node
        .get(s.target)
        .map(|n| if s.across { n.width } else { n.height });
    if let Ok(Val::Px(h)) = size {
        commands.trigger(SplitterMoved {
            entity: ev.entity,
            target: s.target,
            height: h,
        });
    }
}

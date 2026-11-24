//! A node graph editor's canvas (Blender's node editors, Houdini's networks; interaction rules
//! after egui-graph-edit): an endless surface that pans and zooms, nodes placed on it, sockets on
//! their sides and wires between sockets. The widget knows nothing of what the nodes mean: the
//! app builds the nodes' contents, says which sockets are wired ([`Wire`]), and hears what the
//! user does ([`WireConnect`], [`WireDisconnect`], [`GraphNodeMoved`]).
//!
//! - **[`NodeCanvas`]** is a clipped area with two layers that pan and zoom together, the wires
//!   under the nodes. The wheel zooms about the pointer (0.25× to 1×: past 1× the text would be
//!   stretched, not drawn sharper); a middle or right drag pans; [`FitCanvas`] frames every node.
//!   The maths is [`PanZoom`].
//! - **[`GraphNode`]** is a node at a place in graph coordinates (px at 1×). Dragging any
//!   [`GraphNodeHandle`] inside it moves it, and the other [`GraphNodeSelected`] nodes with it;
//!   the drag's end triggers [`GraphNodeMoved`] on each.
//! - **[`Socket`]**: a small circle on a node's left (an input) or right (an output) edge,
//!   level with the row it is [anchored](Socket::anchor) to. Dragging from an output to an
//!   input of the same [type](Socket::ty) (within [`SNAP`] px of it) triggers [`WireConnect`];
//!   dragging a wire off an input triggers [`WireDisconnect`] (and the drag goes on from the
//!   wire's output, so it can be dropped on another input). An input takes one wire.
//! - **[`Wire`]**: a cubic Bézier from an output socket to an input socket, drawn as short thin
//!   segments (rotated nodes) on the wire layer, each frame its ends moved.
//!
//! Names: the canvas gets the builder's, its layers `<name>-world` and `<name>-wires`.

use std::borrow::Cow;

use bevy::picking::events::{
    PointerDrag, PointerDragEnd, PointerDragStart, PointerPress, PointerScroll,
};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui::{UiGlobalTransform, UiTransform, Val2};

use crate::theme::Theme;

pub struct NodeGraphPlugin;

impl Plugin for NodeGraphPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_scroll)
            .add_observer(on_pan_drag)
            .add_observer(on_node_press)
            .add_observer(on_node_drag)
            .add_observer(on_node_drag_end)
            .add_observer(on_socket_drag_start)
            .add_observer(on_socket_drag)
            .add_observer(on_socket_drag_end)
            .add_observer(on_fit)
            .add_systems(
                PostUpdate,
                (apply_views, place_nodes, mark_static_copies).before(bevy::ui::UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                (place_sockets, draw_wires)
                    .chain()
                    .after(bevy::ui::UiSystems::Layout),
            );
    }
}

/// How near (px on screen) a dropped wire must be to an input to connect to it.
pub const SNAP: f32 = 14.0;
/// The zoom range.
pub const MIN_ZOOM: f32 = 0.25;
pub const MAX_ZOOM: f32 = 1.0;
/// A socket's diameter (px at 1×).
pub const SOCKET: f32 = 10.0;

// ---------------------------------------------------------------------------------------------
// Pan and zoom

/// Where the graph is on the canvas: `screen = graph · zoom + pan` (canvas px, from its top
/// left).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanZoom {
    pub pan: Vec2,
    pub zoom: f32,
}

impl Default for PanZoom {
    fn default() -> Self {
        Self {
            pan: Vec2::new(40.0, 40.0),
            zoom: 1.0,
        }
    }
}

impl PanZoom {
    pub fn to_screen(&self, g: Vec2) -> Vec2 {
        g * self.zoom + self.pan
    }

    pub fn to_graph(&self, s: Vec2) -> Vec2 {
        (s - self.pan) / self.zoom
    }

    /// Zooms by `factor` keeping the graph point under `at` (canvas px) where it is.
    pub fn zoom_at(&mut self, at: Vec2, factor: f32) {
        let g = self.to_graph(at);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.pan = at - g * self.zoom;
    }

    /// The view that shows `bounds` (graph px) whole in a canvas of `size` px, with `margin` px
    /// around, as large as the zoom range allows.
    pub fn fit(bounds: Rect, size: Vec2, margin: f32) -> Self {
        if bounds.is_empty() || size.x <= 2.0 * margin || size.y <= 2.0 * margin {
            return Self::default();
        }
        let room = size - Vec2::splat(2.0 * margin);
        let zoom = (room / bounds.size())
            .min_element()
            .clamp(MIN_ZOOM, MAX_ZOOM);
        let pan = size / 2.0 - bounds.center() * zoom;
        Self { pan, zoom }
    }
}

/// The points of the wire from `a` (an output, leaving rightwards) to `b` (an input, entered
/// from the left): a cubic Bézier whose handles reach half the run across, at least 30 px.
pub fn wire_points(a: Vec2, b: Vec2, n: usize) -> Vec<Vec2> {
    let reach = ((b.x - a.x).abs() / 2.0).max(30.0);
    let (c1, c2) = (a + Vec2::X * reach, b - Vec2::X * reach);
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            let u = 1.0 - t;
            a * u * u * u + c1 * 3.0 * u * u * t + c2 * 3.0 * u * t * t + b * t * t * t
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The canvas

/// A node canvas: its view, and its layers.
#[derive(Component, Debug, Clone, Copy)]
pub struct NodeCanvas {
    pub view: PanZoom,
    world: Entity,
    wires: Entity,
}

impl NodeCanvas {
    /// The layer the nodes go in (as children).
    pub fn world(&self) -> Entity {
        self.world
    }

    /// The layer the wires go in (as children).
    pub fn wires(&self) -> Entity {
        self.wires
    }
}

/// A layer of a canvas, which moves with its view.
#[derive(Component, Debug, Clone, Copy)]
struct CanvasLayer;

/// Builder for a [`NodeCanvas`].
pub struct NodeCanvasBuilder {
    name: Cow<'static, str>,
    view: PanZoom,
}

impl NodeCanvasBuilder {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            view: PanZoom::default(),
        }
    }

    pub fn view(mut self, v: PanZoom) -> Self {
        self.view = v;
        self
    }

    /// Spawns the canvas under `parent`; returns it.
    pub fn spawn(self, commands: &mut Commands, parent: Entity, t: &Theme) -> Entity {
        let canvas = commands
            .spawn((
                Name::new(self.name.to_string()),
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    min_width: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(t.viewport_background),
                Pickable::default(),
                ChildOf(parent),
            ))
            .id();
        let layer = |name: String| {
            (
                Name::new(name),
                CanvasLayer,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(0.0),
                    height: Val::Px(0.0),
                    overflow: Overflow::visible(),
                    ..default()
                },
                UiTransform::default(),
                Pickable::IGNORE,
                ChildOf(canvas),
            )
        };
        let wires = commands.spawn(layer(format!("{}-wires", self.name))).id();
        let world = commands.spawn(layer(format!("{}-world", self.name))).id();
        commands.entity(canvas).insert(NodeCanvas {
            view: self.view,
            world,
            wires,
        });
        canvas
    }
}

/// Frames every node of the canvas it is triggered on.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct FitCanvas {
    pub entity: Entity,
}

/// The canvas's view changed (a pan, a zoom, a fit): the app may keep it.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct CanvasViewChanged {
    pub entity: Entity,
    pub view: PanZoom,
}

fn apply_views(
    q_canvas: Query<&NodeCanvas, Changed<NodeCanvas>>,
    mut q_layer: Query<&mut UiTransform, With<CanvasLayer>>,
) {
    for c in &q_canvas {
        for layer in [c.world, c.wires] {
            if let Ok(mut tf) = q_layer.get_mut(layer) {
                let want = UiTransform {
                    translation: Val2::px(c.view.pan.x, c.view.pan.y),
                    scale: Vec2::splat(c.view.zoom),
                    ..default()
                };
                if *tf != want {
                    *tf = want;
                }
            }
        }
    }
}

/// The pointer's place on a canvas, in canvas px from its top left.
fn canvas_point(
    pointer: Vec2,
    canvas: Entity,
    q: &Query<(&ComputedNode, &UiGlobalTransform)>,
) -> Option<Vec2> {
    let (node, gt) = q.get(canvas).ok()?;
    let s = node.inverse_scale_factor();
    let top_left = gt.translation * s - node.size() * s / 2.0;
    Some(pointer - top_left)
}

/// The canvas an entity is on (itself or an ancestor).
fn canvas_of(
    e: Entity,
    q_parent: &Query<&ChildOf>,
    q_canvas: &Query<&mut NodeCanvas>,
) -> Option<Entity> {
    std::iter::once(e)
        .chain(q_parent.iter_ancestors(e))
        .find(|a| q_canvas.contains(*a))
}

fn on_scroll(
    mut ev: On<PointerScroll>,
    q_parent: Query<&ChildOf>,
    mut q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    let Some(canvas) = canvas_of(ev.entity, &q_parent, &q_canvas) else {
        return;
    };
    if ev.entity != canvas {
        // Bubbles up to the canvas, which handles it once.
        return;
    }
    ev.propagate(false);
    let Some(at) = canvas_point(ev.pointer.position, canvas, &q_geo) else {
        return;
    };
    let lines = match ev.unit {
        bevy::input::mouse::MouseScrollUnit::Line => ev.y,
        bevy::input::mouse::MouseScrollUnit::Pixel => ev.y / 40.0,
    };
    if lines == 0.0 {
        return;
    }
    let Ok(mut c) = q_canvas.get_mut(canvas) else {
        return;
    };
    let mut v = c.view;
    v.zoom_at(at, (lines * 0.15).exp());
    if v != c.view {
        c.view = v;
        commands.trigger(CanvasViewChanged {
            entity: canvas,
            view: v,
        });
    }
}

fn on_pan_drag(
    mut ev: On<PointerDrag>,
    mut q_canvas: Query<&mut NodeCanvas>,
    mut commands: Commands,
) {
    if !matches!(ev.button, PointerButton::Middle | PointerButton::Secondary) {
        return;
    }
    let Ok(mut c) = q_canvas.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    c.view.pan += ev.delta;
    let view = c.view;
    commands.trigger(CanvasViewChanged {
        entity: ev.entity,
        view,
    });
}

fn on_fit(
    ev: On<FitCanvas>,
    mut q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    q_nodes: Query<(&GraphNode, &ComputedNode, &ChildOf)>,
    mut commands: Commands,
) {
    let Ok(mut c) = q_canvas.get_mut(ev.entity) else {
        return;
    };
    let Ok((cn, _)) = q_geo.get(ev.entity) else {
        return;
    };
    let mut bounds: Option<Rect> = None;
    for (n, node, parent) in &q_nodes {
        if parent.parent() != c.world {
            continue;
        }
        let r = Rect::from_corners(n.pos, n.pos + node.size() * node.inverse_scale_factor());
        bounds = Some(bounds.map_or(r, |b| b.union(r)));
    }
    let Some(b) = bounds else { return };
    let size = cn.size() * cn.inverse_scale_factor();
    c.view = PanZoom::fit(b, size, 24.0);
    let view = c.view;
    commands.trigger(CanvasViewChanged {
        entity: ev.entity,
        view,
    });
}

// ---------------------------------------------------------------------------------------------
// Nodes

/// A node on a canvas (a child of its [world layer](NodeCanvas::world)), at `pos` in graph
/// px (its top left).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GraphNode {
    pub pos: Vec2,
}

impl GraphNode {
    /// The node's own bundle: absolutely placed at `pos`.
    pub fn bundle(name: impl Into<Cow<'static, str>>, pos: Vec2) -> impl Bundle {
        (
            Name::new(name.into()),
            GraphNode { pos },
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(pos.x),
                top: Val::Px(pos.y),
                flex_direction: FlexDirection::Column,
                ..default()
            },
        )
    }
}

/// Dragging this (or anything in it that lets the drag bubble up) moves the node it is in.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GraphNodeHandle;

/// A selected node: dragging another selected node moves it too.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GraphNodeSelected;

/// A node was dragged to `pos` (graph px), triggered on the node at the drag's end.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct GraphNodeMoved {
    pub entity: Entity,
    pub pos: Vec2,
}

/// A node was pressed (to select it, or bring it to the front).
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct GraphNodePressed {
    pub entity: Entity,
    pub button: PointerButton,
}

fn place_nodes(mut q: Query<(&GraphNode, &mut Node), Changed<GraphNode>>) {
    for (g, mut n) in &mut q {
        let (l, t) = (Val::Px(g.pos.x), Val::Px(g.pos.y));
        if n.left != l || n.top != t {
            n.left = l;
            n.top = t;
        }
    }
}

/// The node `e` is (in), if it is (in) one; whether through a handle.
fn node_of(
    e: Entity,
    q_parent: &Query<&ChildOf>,
    q_node: &Query<&mut GraphNode>,
    q_handle: &Query<(), With<GraphNodeHandle>>,
) -> Option<(Entity, bool)> {
    let mut handle = false;
    for a in std::iter::once(e).chain(q_parent.iter_ancestors(e)) {
        handle |= q_handle.contains(a);
        if q_node.contains(a) {
            return Some((a, handle));
        }
    }
    None
}

fn on_node_press(
    ev: On<PointerPress>,
    q_parent: Query<&ChildOf>,
    q_node: Query<&mut GraphNode>,
    q_handle: Query<(), With<GraphNodeHandle>>,
    mut commands: Commands,
) {
    // Only where the press starts (it bubbles up through the node).
    if ev.original_event_target() != ev.entity {
        return;
    }
    if let Some((node, _)) = node_of(ev.entity, &q_parent, &q_node, &q_handle) {
        commands.trigger(GraphNodePressed {
            entity: node,
            button: ev.button,
        });
    }
}

/// The nodes a drag moves: the dragged one, and the other selected ones if it is selected.
fn moving(
    node: Entity,
    q_sel: &Query<Entity, (With<GraphNode>, With<GraphNodeSelected>)>,
) -> Vec<Entity> {
    if q_sel.contains(node) {
        q_sel.iter().collect()
    } else {
        vec![node]
    }
}

#[allow(clippy::too_many_arguments)]
fn on_node_drag(
    mut ev: On<PointerDrag>,
    q_parent: Query<&ChildOf>,
    mut q_node: Query<&mut GraphNode>,
    q_handle: Query<(), With<GraphNodeHandle>>,
    q_sel: Query<Entity, (With<GraphNode>, With<GraphNodeSelected>)>,
    q_canvas: Query<&mut NodeCanvas>,
) {
    if ev.button != PointerButton::Primary || ev.original_event_target() != ev.entity {
        return;
    }
    let Some((node, true)) = node_of(ev.entity, &q_parent, &q_node, &q_handle) else {
        return;
    };
    let Some(canvas) = canvas_of(node, &q_parent, &q_canvas) else {
        return;
    };
    let Ok(zoom) = q_canvas.get(canvas).map(|c| c.view.zoom) else {
        return;
    };
    ev.propagate(false);
    for n in moving(node, &q_sel) {
        if let Ok(mut g) = q_node.get_mut(n) {
            g.pos += ev.delta / zoom;
        }
    }
}

fn on_node_drag_end(
    ev: On<PointerDragEnd>,
    q_parent: Query<&ChildOf>,
    q_node: Query<&mut GraphNode>,
    q_handle: Query<(), With<GraphNodeHandle>>,
    q_sel: Query<Entity, (With<GraphNode>, With<GraphNodeSelected>)>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary || ev.original_event_target() != ev.entity {
        return;
    }
    let Some((node, true)) = node_of(ev.entity, &q_parent, &q_node, &q_handle) else {
        return;
    };
    for n in moving(node, &q_sel) {
        if let Ok(g) = q_node.get(n) {
            commands.trigger(GraphNodeMoved {
                entity: n,
                pos: g.pos,
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Sockets and wires

/// Which edge of its node a socket is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketSide {
    /// The left edge: an input (one wire).
    In,
    /// The right edge: an output (any number of wires).
    Out,
}

/// A socket: a child of a [`GraphNode`], on its left or right edge, level with `anchor`'s
/// middle (a row of the node's body; the node's header when `None`). Only sockets of the same
/// `ty` connect.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Socket {
    pub side: SocketSide,
    pub ty: u32,
    pub anchor: Option<Entity>,
}

impl Socket {
    /// A socket's bundle (a circle of `color`): spawn it as a child of its node.
    pub fn bundle(
        name: impl Into<Cow<'static, str>>,
        side: SocketSide,
        ty: u32,
        anchor: Option<Entity>,
        color: Color,
        t: &Theme,
    ) -> impl Bundle {
        let edge = Val::Px(-SOCKET / 2.0 - 1.0);
        (
            Name::new(name.into()),
            Socket { side, ty, anchor },
            Node {
                position_type: PositionType::Absolute,
                left: if side == SocketSide::In {
                    edge
                } else {
                    Val::Auto
                },
                right: if side == SocketSide::Out {
                    edge
                } else {
                    Val::Auto
                },
                top: Val::Px(15.0 - SOCKET / 2.0),
                width: Val::Px(SOCKET),
                height: Val::Px(SOCKET),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            BorderColor::all(t.border_strong),
            ZIndex(5),
            Pickable::default(),
            crate::splitter::HoverCursor(crate::cursor::CursorKind::Crosshair),
            bevy::picking::hover::Hovered::default(),
        )
    }
}

/// A wire from an output socket to an input socket: a child of the canvas's
/// [wire layer](NodeCanvas::wires).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Wire {
    pub from: Entity,
    pub to: Entity,
    pub color: Color,
    /// Drawn dashed (an expression that only mentions its source).
    pub dashed: bool,
}

impl Wire {
    pub fn bundle(name: impl Into<Cow<'static, str>>, wire: Wire) -> impl Bundle {
        (
            Name::new(name.into()),
            wire,
            WireDrawn::default(),
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Pickable::IGNORE,
        )
    }
}

/// The ends a wire was last drawn with.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
struct WireDrawn(Option<(Vec2, Vec2)>);

/// One straight piece of a wire.
#[derive(Component, Debug, Clone, Copy)]
struct WireSegment;

/// The wire being dragged from a socket: its output end, and where the pointer is (graph px).
#[derive(Component, Debug, Clone, Copy)]
struct DragWire {
    from: Entity,
    to: Vec2,
}

/// A wire was dropped on an input: connect `from` (an output) to `to` (an input). Triggered on
/// the canvas.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct WireConnect {
    pub entity: Entity,
    pub from: Entity,
    pub to: Entity,
}

/// A wire was dragged off its input. Triggered on the canvas.
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct WireDisconnect {
    pub entity: Entity,
    pub from: Entity,
    pub to: Entity,
}

/// Moves each socket level with its anchor row (in its node's coordinates).
fn place_sockets(
    mut q_socket: Query<(&Socket, &ChildOf, &mut Node)>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
) {
    for (s, parent, mut n) in &mut q_socket {
        let Some(anchor) = s.anchor else { continue };
        let (Ok((pn, pg)), Ok((an, ag))) = (q_geo.get(parent.parent()), q_geo.get(anchor)) else {
            continue;
        };
        if an.size().y <= 0.0 {
            continue;
        }
        // The anchor's middle in the node's frame (physical px from its centre), to logical px
        // from its top.
        let Some(inv) = pg.try_inverse() else {
            continue;
        };
        let local = inv.transform_point2(ag.translation);
        let y = (local.y + pn.size().y / 2.0) * pn.inverse_scale_factor();
        let top = Val::Px((y - SOCKET / 2.0).round());
        if n.top != top {
            n.top = top;
        }
    }
}

/// A socket's centre on its canvas's layers (graph px).
fn socket_point(
    socket: Entity,
    layer: &UiGlobalTransform,
    layer_scale: f32,
    q_geo: &Query<(&ComputedNode, &UiGlobalTransform)>,
) -> Option<Vec2> {
    let (_, sg) = q_geo.get(socket).ok()?;
    let inv = layer.try_inverse()?;
    Some(inv.transform_point2(sg.translation) * layer_scale)
}

#[allow(clippy::type_complexity)]
fn draw_wires(
    q_canvas: Query<&NodeCanvas>,
    mut q_wire: Query<
        (
            Entity,
            Option<&Wire>,
            Option<&DragWire>,
            &mut WireDrawn,
            &ChildOf,
            Option<&Children>,
        ),
        Or<(With<Wire>, With<DragWire>)>,
    >,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    q_seg: Query<(), With<WireSegment>>,
    mut commands: Commands,
) {
    for (entity, wire, drag, mut drawn, parent, children) in &mut q_wire {
        let layer = parent.parent();
        let Ok((ln, lg)) = q_geo.get(layer) else {
            continue;
        };
        let s = ln.inverse_scale_factor();
        let ends = match (wire, drag) {
            (Some(w), _) => {
                socket_point(w.from, lg, s, &q_geo).zip(socket_point(w.to, lg, s, &q_geo))
            }
            (None, Some(d)) => socket_point(d.from, lg, s, &q_geo).map(|a| (a, d.to)),
            _ => None,
        };
        let Some((a, b)) = ends else { continue };
        if drawn.0 == Some((a, b)) {
            continue;
        }
        drawn.0 = Some((a, b));
        let (color, dashed) = match wire {
            Some(w) => (w.color, w.dashed),
            None => (Color::WHITE, false),
        };
        let zoom = q_canvas
            .iter()
            .find(|c| c.wires == layer)
            .map_or(1.0, |c| c.view.zoom);
        // A wire stays at least a pixel thick however far out it is zoomed.
        let thick = (2.0f32).max(1.2 / zoom);
        let pts = wire_points(a, b, 24);
        if let Some(children) = children {
            for c in children.iter() {
                if q_seg.contains(c) {
                    commands.entity(c).despawn();
                }
            }
        }
        for (i, w) in pts.windows(2).enumerate() {
            if dashed && i % 2 == 1 {
                continue;
            }
            let (p, q) = (w[0], w[1]);
            let d = q - p;
            let len = d.length() + 0.6;
            let mid = (p + q) / 2.0;
            commands.spawn((
                WireSegment,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(mid.x - len / 2.0),
                    top: Val::Px(mid.y - thick / 2.0),
                    width: Val::Px(len),
                    height: Val::Px(thick),
                    ..default()
                },
                UiTransform {
                    rotation: Rot2::radians(d.y.atan2(d.x)),
                    ..default()
                },
                BackgroundColor(color),
                Pickable::IGNORE,
                ChildOf(entity),
            ));
        }
    }
}

/// The socket a drag from `e` starts at: `e` itself or its ancestor.
fn socket_at(e: Entity, q_parent: &Query<&ChildOf>, q_socket: &Query<&Socket>) -> Option<Entity> {
    std::iter::once(e)
        .chain(q_parent.iter_ancestors(e))
        .take(2)
        .find(|a| q_socket.contains(*a))
}

#[allow(clippy::too_many_arguments)]
fn on_socket_drag_start(
    mut ev: On<PointerDragStart>,
    q_parent: Query<&ChildOf>,
    q_socket: Query<&Socket>,
    q_wire: Query<&Wire>,
    q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let Some(socket) = socket_at(ev.entity, &q_parent, &q_socket) else {
        return;
    };
    ev.propagate(false);
    let Some(canvas) = canvas_of(socket, &q_parent, &q_canvas) else {
        return;
    };
    let Ok(c) = q_canvas.get(canvas) else { return };
    let s = q_socket.get(socket).unwrap();
    // From an input with a wire: the wire comes off it, and the drag goes on from its output.
    let from = match s.side {
        SocketSide::Out => socket,
        SocketSide::In => {
            let Some(w) = q_wire.iter().find(|w| w.to == socket) else {
                return;
            };
            commands.trigger(WireDisconnect {
                entity: canvas,
                from: w.from,
                to: socket,
            });
            w.from
        }
    };
    let to = canvas_point(ev.pointer.position, canvas, &q_geo)
        .map_or(Vec2::ZERO, |p| c.view.to_graph(p));
    commands.spawn((
        Name::new("node-graph-drag-wire"),
        DragWire { from, to },
        WireDrawn::default(),
        Node {
            position_type: PositionType::Absolute,
            ..default()
        },
        Pickable::IGNORE,
        ChildOf(c.wires),
    ));
}

fn on_socket_drag(
    ev: On<PointerDrag>,
    q_parent: Query<&ChildOf>,
    q_socket: Query<&Socket>,
    q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut q_drag: Query<&mut DragWire>,
) {
    if ev.button != PointerButton::Primary || socket_at(ev.entity, &q_parent, &q_socket).is_none() {
        return;
    }
    for mut d in &mut q_drag {
        let Some(canvas) = canvas_of(d.from, &q_parent, &q_canvas) else {
            continue;
        };
        let Ok(c) = q_canvas.get(canvas) else {
            continue;
        };
        if let Some(p) = canvas_point(ev.pointer.position, canvas, &q_geo) {
            d.to = c.view.to_graph(p);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn on_socket_drag_end(
    ev: On<PointerDragEnd>,
    q_parent: Query<&ChildOf>,
    q_socket: Query<(Entity, &Socket)>,
    q_sock: Query<&Socket>,
    q_drag: Query<(Entity, &DragWire)>,
    q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary || socket_at(ev.entity, &q_parent, &q_sock).is_none() {
        return;
    }
    for (e, d) in &q_drag {
        commands.entity(e).despawn();
        let Some(canvas) = canvas_of(d.from, &q_parent, &q_canvas) else {
            continue;
        };
        let (Ok(c), Ok(from)) = (q_canvas.get(canvas), q_sock.get(d.from)) else {
            continue;
        };
        let Some(at) = canvas_point(ev.pointer.position, canvas, &q_geo) else {
            continue;
        };
        // The nearest input of the same type within SNAP px on screen.
        let mut best: Option<(f32, Entity)> = None;
        for (s, sock) in &q_socket {
            if sock.side != SocketSide::In || sock.ty != from.ty {
                continue;
            }
            if canvas_of(s, &q_parent, &q_canvas) != Some(canvas) {
                continue;
            }
            let Ok((sn, sg)) = q_geo.get(s) else { continue };
            let Ok((cn, cg)) = q_geo.get(canvas) else {
                continue;
            };
            let sc = cn.inverse_scale_factor();
            let canvas_tl = cg.translation * sc - cn.size() * sc / 2.0;
            let p = sg.translation * sn.inverse_scale_factor() - canvas_tl;
            let dist = p.distance(at);
            if dist <= SNAP + SOCKET * c.view.zoom && best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, s));
            }
        }
        if let Some((_, to)) = best {
            commands.trigger(WireConnect {
                entity: canvas,
                from: d.from,
                to,
            });
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Static copies

/// The root of a copy of a dialog shown for looks only (a node of a feature not being edited):
/// everything under it is marked [`StaticCopy`] and left out of keyboard navigation, so the
/// systems that keep the live dialog in step can leave it alone (`Without<StaticCopy>`).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StaticCopyRoot;

/// In a [`StaticCopyRoot`].
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct StaticCopy;

fn mark_static_copies(
    q_root: Query<Entity, With<StaticCopyRoot>>,
    q_children: Query<&Children>,
    q_marked: Query<(), With<StaticCopy>>,
    mut commands: Commands,
) {
    for root in &q_root {
        for e in std::iter::once(root).chain(q_children.iter_descendants(root)) {
            if !q_marked.contains(e) {
                commands
                    .entity(e)
                    .insert(StaticCopy)
                    .remove::<bevy::input_focus::tab_navigation::TabIndex>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_keeps_the_point_under_the_pointer() {
        let mut v = PanZoom {
            pan: Vec2::new(10.0, 20.0),
            zoom: 0.5,
        };
        let at = Vec2::new(200.0, 100.0);
        let g = v.to_graph(at);
        v.zoom_at(at, 1.5);
        assert!((v.zoom - 0.75).abs() < 1e-6);
        assert!(v.to_screen(g).distance(at) < 1e-3);
        // Clamped to the range.
        v.zoom_at(at, 100.0);
        assert_eq!(v.zoom, MAX_ZOOM);
        assert!(v.to_screen(g).distance(at) < 1e-3);
        v.zoom_at(at, 1e-6);
        assert_eq!(v.zoom, MIN_ZOOM);
    }

    #[test]
    fn fit_frames_the_bounds() {
        let b = Rect::new(0.0, 0.0, 2000.0, 1000.0);
        let v = PanZoom::fit(b, Vec2::new(800.0, 600.0), 20.0);
        assert!(v.zoom < 1.0);
        let (lo, hi) = (v.to_screen(b.min), v.to_screen(b.max));
        assert!(lo.x >= 19.9 && lo.y >= 19.9 && hi.x <= 780.1 && hi.y <= 580.1);
        // A small graph isn't blown up past 1×.
        let v = PanZoom::fit(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Vec2::new(800.0, 600.0),
            20.0,
        );
        assert_eq!(v.zoom, 1.0);
        assert!(
            v.to_screen(Vec2::new(5.0, 5.0))
                .distance(Vec2::new(400.0, 300.0))
                < 1e-3
        );
    }

    #[test]
    fn wires_run_from_end_to_end() {
        let (a, b) = (Vec2::new(0.0, 0.0), Vec2::new(100.0, 50.0));
        let p = wire_points(a, b, 10);
        assert_eq!(p.len(), 11);
        assert!(p[0].distance(a) < 1e-4 && p[10].distance(b) < 1e-4);
        // It leaves the output rightwards and enters the input from the left.
        assert!(p[1].x > a.x && (p[1].y - a.y).abs() < p[1].x - a.x);
        assert!(p[9].x < b.x);
        // A wire going back left still loops out to the right first.
        let p = wire_points(Vec2::new(100.0, 0.0), Vec2::new(0.0, 0.0), 10);
        assert!(p[1].x > 100.0);
    }
}

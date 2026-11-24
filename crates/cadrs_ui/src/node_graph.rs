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
//! - **Selecting**: a drag on the empty canvas draws a box; at its end [`BoxSelected`] lists the
//!   nodes it touches (the app selects them; [`GraphNodeSelected`] marks what is selected).
//! - Nodes well outside the view are taken out of the layout (`cull_nodes`), so a large graph draws only what
//!   shows.
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
        bevy::asset::embedded_asset!(app, "node_grid.wesl");
        app.add_plugins(bevy::ui_render::prelude::UiMaterialPlugin::<GridMaterial>::default())
            .init_resource::<WireRouter>()
            .add_observer(on_scroll)
            .add_observer(on_pan_drag)
            .add_observer(on_node_press)
            .add_observer(on_node_drag)
            .add_observer(on_node_drag_end)
            .add_observer(on_socket_drag_start)
            .add_observer(on_socket_drag)
            .add_observer(on_socket_drag_end)
            .add_observer(on_fit)
            .add_observer(on_box_start)
            .add_observer(on_box_drag)
            .add_observer(on_box_end)
            // The wires are drawn before the layout, from where the nodes are now (a dragged
            // node's wires move with it in the same frame, with no frame left without them).
            .add_systems(
                PostUpdate,
                (
                    apply_views,
                    place_nodes,
                    draw_wires,
                    sync_grids,
                    mark_static_copies,
                )
                    .chain()
                    .before(bevy::ui::UiSystems::Prepare),
            )
            .add_systems(
                PostUpdate,
                (place_sockets, route_wires, cull_nodes)
                    .chain()
                    .after(bevy::ui::UiSystems::Layout),
            );
    }
}

/// How near (px on screen) a dropped wire must be to an input to connect to it.
pub const SNAP: f32 = 14.0;
/// The zoom range.
pub const MIN_ZOOM: f32 = 0.1;
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
    grid: Option<CanvasGrid>,
    background: Option<Color>,
}

impl NodeCanvasBuilder {
    pub fn new(name: impl Into<Cow<'static, str>>) -> Self {
        Self {
            name: name.into(),
            view: PanZoom::default(),
            grid: None,
            background: None,
        }
    }

    pub fn view(mut self, v: PanZoom) -> Self {
        self.view = v;
        self
    }

    /// Draws a grid under the wires ([`CanvasGrid`]).
    pub fn grid(mut self, g: CanvasGrid) -> Self {
        self.grid = Some(g);
        self
    }

    /// The canvas's colour (the theme's view background by default).
    pub fn background(mut self, c: Color) -> Self {
        self.background = Some(c);
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
                BackgroundColor(self.background.unwrap_or(t.viewport_background)),
                Pickable::default(),
                ChildOf(parent),
            ))
            .id();
        if let Some(g) = self.grid {
            // First, so under the wires and the nodes; not moved with the view (its lines are
            // drawn by its shader from the view, [`GridMaterial`]).
            commands.spawn((
                Name::new(format!("{}-grid", self.name)),
                g,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    right: Val::Px(0.0),
                    bottom: Val::Px(0.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                Pickable::IGNORE,
                ChildOf(canvas),
            ));
        }
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
        commands.entity(canvas).insert((
            NodeCanvas {
                view: self.view,
                world,
                wires,
            },
            CanvasRoutes::default(),
        ));
        canvas
    }
}

/// A canvas's grid, after the PCB Studio's schematic (Horizon EDA's look), in graph units: faint
/// lines every `step` (every 2nd, 4th, … step when they would be closer than 12 px on screen)
/// and a cross where every fifth line meets every fifth. A transparent `line` draws the crosses
/// alone. One node draws it ([`GridMaterial`]): a pan or a zoom changes two numbers.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct CanvasGrid {
    pub step: f32,
    pub line: Color,
    pub mark: Color,
}

/// The grid's step at a zoom: `step` graph units, doubled until the lines are at least 12 px
/// apart on screen.
pub fn grid_step(step: f32, zoom: f32) -> f32 {
    let mut s = step.max(1e-3);
    while s * zoom < 12.0 {
        s *= 2.0;
    }
    s
}

/// What the grid's shader (`node_grid.wesl`) reads.
#[derive(Debug, Clone, Copy, Default, bevy::render::render_resource::ShaderType)]
struct GridParams {
    view: Vec4,
    step: Vec4,
    line: Vec4,
    mark: Vec4,
}

/// A canvas's grid, drawn by one node covering the canvas (`node_grid.wesl`): in graph units,
/// from the canvas's pan and zoom.
#[derive(Asset, TypePath, bevy::render::render_resource::AsBindGroup, Debug, Clone)]
pub struct GridMaterial {
    #[uniform(0)]
    params: GridParams,
}

impl bevy::ui_render::prelude::UiMaterial for GridMaterial {
    fn fragment_shader() -> bevy::shader::ShaderRef {
        "embedded://cadrs_ui/node_grid.wesl".into()
    }

    fn specialize(
        descriptor: &mut bevy::render::render_resource::RenderPipelineDescriptor,
        _key: bevy::ui_render::prelude::UiMaterialKey<Self>,
    ) {
        if let Some(target) = descriptor
            .fragment
            .as_mut()
            .and_then(|f| f.targets.first_mut())
            .and_then(|t| t.as_mut())
        {
            target.blend =
                Some(bevy::render::render_resource::BlendState::PREMULTIPLIED_ALPHA_BLENDING);
        }
    }
}

/// The grid's numbers: the view's pan and zoom, the step for that zoom, the colours.
fn grid_params(g: &CanvasGrid, view: &PanZoom, inv_scale: f32) -> GridParams {
    let lin = |c: Color| Vec4::from_array(c.to_linear().to_f32_array());
    GridParams {
        view: Vec4::new(view.pan.x, view.pan.y, view.zoom, inv_scale),
        step: Vec4::new(
            grid_step(g.step, view.zoom),
            (g.step * view.zoom * 0.3).clamp(3.0, 8.0),
            0.0,
            0.0,
        ),
        line: lin(g.line),
        mark: lin(g.mark),
    }
}

/// Gives each grid its material, and its numbers again when the view (or the scale) changed.
#[allow(clippy::type_complexity)]
fn sync_grids(
    q_canvas: Query<(&NodeCanvas, &ComputedNode, &Children)>,
    q_grid: Query<(
        Entity,
        &CanvasGrid,
        Option<&bevy::ui_render::prelude::MaterialNode<GridMaterial>>,
    )>,
    mut materials: ResMut<Assets<GridMaterial>>,
    mut commands: Commands,
) {
    for (c, cn, kids) in &q_canvas {
        for k in kids.iter() {
            let Ok((e, g, mat)) = q_grid.get(k) else {
                continue;
            };
            let params = grid_params(g, &c.view, cn.inverse_scale_factor());
            let have = mat.map(|m| m.0.clone()).filter(|h| materials.contains(h));
            match have {
                Some(h) => {
                    let stale = materials.get(&h).is_some_and(|m| {
                        m.params.view != params.view || m.params.step != params.step
                    });
                    if stale && let Some(mut m) = materials.get_mut(&h) {
                        m.params = params;
                    }
                }
                None => {
                    let h = materials.add(GridMaterial { params });
                    commands
                        .entity(e)
                        .insert(bevy::ui_render::prelude::MaterialNode(h));
                }
            }
        }
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

/// A box being dragged on a canvas's empty ground: where it started and where it is (canvas
/// px), and the node drawing it.
#[derive(Component, Debug, Clone, Copy)]
struct BoxDrag {
    from: Vec2,
    to: Vec2,
    shape: Entity,
}

/// A box was dragged on a canvas (triggered on it): the nodes it touches; `add` when Shift or
/// Ctrl was held (add to the selection instead of replacing it).
#[derive(EntityEvent, Debug, Clone)]
pub struct BoxSelected {
    pub entity: Entity,
    pub nodes: Vec<Entity>,
    pub add: bool,
}

fn on_box_start(
    mut ev: On<PointerDragStart>,
    q_canvas: Query<&NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    theme: Option<Res<Theme>>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary
        || ev.original_event_target() != ev.entity
        || !q_canvas.contains(ev.entity)
    {
        return;
    }
    ev.propagate(false);
    let Some(at) = canvas_point(ev.pointer.position, ev.entity, &q_geo) else {
        return;
    };
    let color = theme.map_or(Color::srgb(0.3, 0.5, 1.0), |t| t.primary);
    let shape = commands
        .spawn((
            Name::new("node-canvas-box"),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(at.x),
                top: Val::Px(at.y),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(color.with_alpha(0.12)),
            BorderColor::all(color),
            ZIndex(20),
            Pickable::IGNORE,
            ChildOf(ev.entity),
        ))
        .id();
    commands.entity(ev.entity).insert(BoxDrag {
        from: at,
        to: at,
        shape,
    });
}

fn on_box_drag(
    ev: On<PointerDrag>,
    mut q_box: Query<&mut BoxDrag>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut q_node: Query<&mut Node>,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    let Ok(mut b) = q_box.get_mut(ev.entity) else {
        return;
    };
    let Some(at) = canvas_point(ev.pointer.position, ev.entity, &q_geo) else {
        return;
    };
    b.to = at;
    let r = Rect::from_corners(b.from, b.to);
    if let Ok(mut n) = q_node.get_mut(b.shape) {
        n.left = Val::Px(r.min.x);
        n.top = Val::Px(r.min.y);
        n.width = Val::Px(r.width());
        n.height = Val::Px(r.height());
    }
}

#[allow(clippy::too_many_arguments)]
fn on_box_end(
    ev: On<PointerDragEnd>,
    q_box: Query<&BoxDrag>,
    q_canvas: Query<&NodeCanvas>,
    q_nodes: Query<(Entity, &GraphNode, &ComputedNode, &ChildOf)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    let Ok(b) = q_box.get(ev.entity) else {
        return;
    };
    commands.entity(b.shape).despawn();
    commands.entity(ev.entity).remove::<BoxDrag>();
    let Ok(c) = q_canvas.get(ev.entity) else {
        return;
    };
    let r = Rect::from_corners(c.view.to_graph(b.from), c.view.to_graph(b.to));
    let nodes: Vec<Entity> = q_nodes
        .iter()
        .filter(|(_, _, _, p)| p.parent() == c.world)
        .filter(|(_, g, n, _)| {
            let nr = Rect::from_corners(g.pos, g.pos + n.size() * n.inverse_scale_factor());
            !nr.intersect(r).is_empty()
        })
        .map(|(e, ..)| e)
        .collect();
    let add = keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]);
    commands.trigger(BoxSelected {
        entity: ev.entity,
        nodes,
        add,
    });
}

/// A node culled off screen ([`cull_nodes`]): its size then, and what its contents' display
/// was (they are taken out of the layout; its sockets stay, so its wires still reach it).
#[derive(Component, Debug, Clone)]
struct Culled {
    size: Vec2,
    height: Val,
    hidden: Vec<(Entity, Display)>,
}

/// Takes the nodes well outside the view out of the layout (their contents, not their sockets
/// or their place): a large graph lays out and draws only what shows. They come back when
/// they near the view.
#[allow(clippy::type_complexity)]
fn cull_nodes(
    q_canvas: Query<(&NodeCanvas, &ComputedNode)>,
    mut q_nodes: Query<
        (
            Entity,
            &GraphNode,
            &ComputedNode,
            &ChildOf,
            &Children,
            Option<&Culled>,
            Has<GraphNodeDragging>,
        ),
        Without<Socket>,
    >,
    mut q_node: Query<&mut Node, Without<GraphNode>>,
    mut q_root: Query<&mut Node, With<GraphNode>>,
    q_socket: Query<(), With<Socket>>,
    mut commands: Commands,
) {
    /// How far outside the view (px on screen) a node still lays out.
    const MARGIN: f32 = 400.0;
    for (c, cn) in &q_canvas {
        let size = cn.size() * cn.inverse_scale_factor();
        let view = Rect::from_corners(Vec2::splat(-MARGIN), size + MARGIN);
        for (e, g, n, p, kids, culled, dragging) in &mut q_nodes {
            if p.parent() != c.world {
                continue;
            }
            let nsize = culled.map_or(n.size() * n.inverse_scale_factor(), |k| k.size);
            if nsize == Vec2::ZERO {
                continue;
            }
            let r = Rect::from_corners(c.view.to_screen(g.pos), c.view.to_screen(g.pos + nsize));
            let off = r.intersect(view).is_empty() && !dragging;
            match (off, culled) {
                (true, None) => {
                    let mut hidden = Vec::new();
                    for k in kids.iter().filter(|k| !q_socket.contains(*k)) {
                        if let Ok(mut kn) = q_node.get_mut(k) {
                            hidden.push((k, kn.display));
                            kn.display = Display::None;
                        }
                    }
                    let Ok(mut rn) = q_root.get_mut(e) else {
                        continue;
                    };
                    let height = rn.height;
                    rn.height = Val::Px(nsize.y);
                    rn.width = Val::Px(nsize.x);
                    commands.entity(e).insert(Culled {
                        size: nsize,
                        height,
                        hidden,
                    });
                }
                (false, Some(k)) => {
                    for (child, d) in &k.hidden {
                        if let Ok(mut kn) = q_node.get_mut(*child) {
                            kn.display = *d;
                        }
                    }
                    if let Ok(mut rn) = q_root.get_mut(e) {
                        rn.height = k.height;
                    }
                    commands.entity(e).remove::<Culled>();
                }
                _ => {}
            }
        }
    }
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

/// A node being dragged (until the drag ends): whoever places nodes leaves it where it is.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct GraphNodeDragging;

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
    mut commands: Commands,
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
            commands.entity(n).insert(GraphNodeDragging);
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
            commands.entity(n).remove::<GraphNodeDragging>();
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

/// The ends a wire was last drawn with, and whether along a route.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq)]
struct WireDrawn(Option<(Vec2, Vec2, bool, u32)>);

/// What a [`WireRouter`] is asked: the nodes to go round (graph px rectangles) and each wire's
/// ends (its output's socket, its input's).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteProblem {
    pub obstacles: Vec<Rect>,
    pub wires: Vec<(Vec2, Vec2)>,
}

/// Routes wires round the nodes once they have stopped moving (the app's: a PCB-style router).
/// For each wire of the problem, the points to pass through between its ends (graph px), or
/// `None` to draw it as a plain curve. Without one, every wire is a curve.
pub type RouteFn = dyn Fn(&RouteProblem) -> Vec<Option<Vec<Vec2>>> + Send + Sync;

#[derive(Resource, Default, Clone)]
pub struct WireRouter(pub Option<std::sync::Arc<RouteFn>>);

/// How long (frames) a canvas's nodes and wires must stay put before its wires are routed.
const SETTLE: u8 = 3;

/// A canvas's routing: the geometry routed last, and each wire's route.
#[derive(Component, Debug, Clone, Default)]
struct CanvasRoutes {
    /// The geometry now, and for how many frames it has been so.
    key: u64,
    still: u8,
    /// The geometry the routes are for.
    routed: u64,
    /// The view the geometry was last taken in.
    view: Option<PanZoom>,
    routes: std::collections::HashMap<Entity, Vec<Vec2>>,
}

/// How far a trace runs straight out of an output and into an input (graph px).
pub const STUB: f32 = 18.0;

/// A wire drawn as a circuit trace (as the PCB Studio's tracks run): straight out of the output,
/// a 45° run across, straight into the input; a wire going back left runs out, down (or up)
/// and back along a row between them. Every leg is horizontal, vertical or at 45°.
pub fn trace_points(a: Vec2, b: Vec2) -> Vec<Vec2> {
    let (x1, x2) = (a.x + STUB, b.x - STUB);
    let dy = b.y - a.y;
    if x2 - x1 >= dy.abs() {
        // Room for the 45° run: centred between the stubs.
        let cx = (x1 + x2) / 2.0;
        let half = dy.abs() / 2.0;
        let mut p = vec![a, Vec2::new(cx - half, a.y), Vec2::new(cx + half, b.y), b];
        p.dedup();
        return p;
    }
    // Back left (or too close): out, down to the row halfway, across, down, in.
    let ym = (a.y + b.y) / 2.0;
    vec![
        a,
        Vec2::new(x1, a.y),
        Vec2::new(x1, ym),
        Vec2::new(x2, ym),
        Vec2::new(x2, b.y),
        b,
    ]
}

/// A trace's right-angle corners cut at 45° (by up to `c`, less on short legs); 45° bends stay.
pub fn chamfered(pts: &[Vec2], c: f32) -> Vec<Vec2> {
    if pts.len() < 3 {
        return pts.to_vec();
    }
    let mut out = vec![pts[0]];
    for w in pts.windows(3) {
        let (a, b, d) = (w[0], w[1], w[2]);
        let (u, v) = (a - b, d - b);
        let square =
            u.length() > 1e-3 && v.length() > 1e-3 && u.normalize().dot(v.normalize()).abs() < 0.1;
        let k = c.min(u.length() / 2.0).min(v.length() / 2.0);
        if !square || k < 0.5 {
            out.push(b);
            continue;
        }
        out.push(b + u.normalize() * k);
        out.push(b + v.normalize() * k);
    }
    out.push(pts[pts.len() - 1]);
    out
}

/// One straight piece of a wire.
#[derive(Component, Debug, Clone, Copy)]
struct WireSegment;

/// The wire being dragged from a socket: its output end, and where the pointer is (graph px).
#[derive(Component, Debug, Clone, Copy)]
struct DragWire {
    from: Entity,
    to: Vec2,
    /// Dragged off an input: the wire (hidden while it is dragged) and the input it came off.
    detached: Option<(Entity, Entity)>,
}

/// A socket wires can't be dragged from or dropped on (wires the app draws for what is fixed
/// elsewhere: a feature's references).
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct ReadOnlySocket;

/// A wire was dropped on an input: connect `from` (an output) to `to` (an input). Triggered on
/// the canvas. `replaces`: the input the wire was dragged off (to disconnect in the same step).
#[derive(EntityEvent, Debug, Clone, Copy)]
pub struct WireConnect {
    pub entity: Entity,
    pub from: Entity,
    pub to: Entity,
    pub replaces: Option<Entity>,
}

/// A wire was dragged off its input and dropped on no input. Triggered on the canvas.
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

/// Routes each canvas's wires once its nodes and wires have stayed put for [`SETTLE`] frames;
/// while anything moves, the wires are curves.
#[allow(clippy::type_complexity)]
fn route_wires(
    router: Res<WireRouter>,
    mut q_canvas: Query<(&NodeCanvas, &mut CanvasRoutes)>,
    q_wire: Query<(Entity, &Wire, &ChildOf)>,
    q_nodes: Query<(&GraphNode, &ComputedNode, &ChildOf)>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    q_dragging: Query<(), Or<(With<GraphNodeDragging>, With<DragWire>)>>,
) {
    use std::hash::{Hash, Hasher};
    let Some(route) = router.0.clone() else {
        return;
    };
    // Not while something is dragged: a pause mid-drag would swap the curves for routes and
    // back again.
    let dragging = !q_dragging.is_empty();
    for (c, mut r) in &mut q_canvas {
        let Ok((ln, lg)) = q_geo.get(c.wires) else {
            continue;
        };
        let s = ln.inverse_scale_factor();
        let mut problem = RouteProblem::default();
        let mut ids = Vec::new();
        for (g, n, p) in &q_nodes {
            if p.parent() == c.world {
                let size = n.size() * n.inverse_scale_factor();
                problem
                    .obstacles
                    .push(Rect::from_corners(g.pos, g.pos + size));
            }
        }
        for (e, w, p) in &q_wire {
            if p.parent() != c.wires {
                continue;
            }
            if let (Some(a), Some(b)) = (
                socket_point(w.from, lg, s, &q_geo),
                socket_point(w.to, lg, s, &q_geo),
            ) {
                problem.wires.push((a, b));
                ids.push(e);
            }
        }
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for o in &problem.obstacles {
            [o.min, o.max]
                .map(|v| v.round().to_array().map(|x| x as i32))
                .hash(&mut h);
        }
        // (Sockets to 4 graph units: their places come through the view and wobble by a hair.)
        for (a, b) in &problem.wires {
            [*a, *b]
                .map(|v| (v / 4.0).round().to_array().map(|x| x as i32))
                .hash(&mut h);
        }
        ids.hash(&mut h);
        let key = h.finish();
        // A pan or a zoom moves nothing: the sockets' places, worked out through the view, may
        // round a hair differently, but the routes stand (taken as this geometry's).
        if r.view != Some(c.view) {
            r.view = Some(c.view);
            if r.key != key && !r.routes.is_empty() && r.routed == r.key {
                r.routed = key;
            }
            r.key = key;
            continue;
        }
        if key != r.key {
            r.key = key;
            r.still = 0;
            if !r.routes.is_empty() {
                r.routes.clear();
            }
            continue;
        }
        if dragging {
            r.still = 0;
            continue;
        }
        r.still = r.still.saturating_add(1);
        if r.still < SETTLE || r.routed == key {
            continue;
        }
        r.routed = key;
        let paths = route(&problem);
        r.routes = ids
            .into_iter()
            .zip(paths)
            .filter_map(|(e, p)| p.map(|p| (e, p)))
            .collect();
    }
}

/// A socket's centre (graph px) where its node is now: where it was in the node at the last
/// layout, from the node's place now (a dragged node's [`GraphNode::pos`] runs ahead of the
/// layout by a frame).
fn socket_now(
    socket: Entity,
    q_parent: &Query<&ChildOf>,
    q_node: &Query<&GraphNode>,
    q_geo: &Query<(&ComputedNode, &UiGlobalTransform)>,
    layer: &UiGlobalTransform,
    layer_scale: f32,
) -> Option<Vec2> {
    let was = socket_point(socket, layer, layer_scale, q_geo)?;
    let node = q_parent.get(socket).ok()?.parent();
    let Ok(g) = q_node.get(node) else {
        return Some(was);
    };
    let (nn, ng) = q_geo.get(node).ok()?;
    let inv = layer.try_inverse()?;
    let centre = inv.transform_point2(ng.translation) * layer_scale;
    let top_left = centre - nn.size() * nn.inverse_scale_factor() / 2.0;
    Some(g.pos + (was - top_left))
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn draw_wires(
    q_canvas: Query<(&NodeCanvas, Option<&CanvasRoutes>)>,
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
    q_parent: Query<&ChildOf>,
    q_node: Query<&GraphNode>,
    mut q_seg: Query<(&mut Node, &mut UiTransform, &mut BackgroundColor), With<WireSegment>>,
    mut commands: Commands,
) {
    for (entity, wire, drag, mut drawn, parent, children) in &mut q_wire {
        let layer = parent.parent();
        let Ok((ln, lg)) = q_geo.get(layer) else {
            continue;
        };
        let s = ln.inverse_scale_factor();
        let at = |socket| socket_now(socket, &q_parent, &q_node, &q_geo, lg, s);
        let ends = match (wire, drag) {
            (Some(w), _) => at(w.from).zip(at(w.to)),
            (None, Some(d)) => at(d.from).map(|a| (a, d.to)),
            _ => None,
        };
        let Some((a, b)) = ends else { continue };
        let canvas = q_canvas.iter().find(|(c, _)| c.wires == layer);
        let route = canvas
            .and_then(|(_, r)| r)
            .and_then(|r| r.routes.get(&entity).cloned());
        let zoom = canvas.map_or(1.0, |(c, _)| c.view.zoom);
        // (Its thickness follows the zoom: drawn again when that changes.)
        let now = Some((a, b, route.is_some(), zoom.to_bits()));
        if drawn.0 == now {
            continue;
        }
        drawn.0 = now;
        let (color, dashed) = match wire {
            Some(w) => (w.color, w.dashed),
            None => (Color::WHITE, false),
        };
        // A wire stays at least a pixel thick however far out it is zoomed.
        let thick = (2.0f32).max(1.2 / zoom);
        let pts = match &route {
            Some(via) => {
                let mut p = vec![a];
                p.extend(via.iter().copied());
                p.push(b);
                chamfered(&p, 10.0)
            }
            None => chamfered(&trace_points(a, b), 10.0),
        };
        // The pieces: the wire's own, moved where they go (spawning only those it lacks), so
        // it is never left without them for a frame.
        let pieces: Vec<(Vec2, Vec2)> = pts
            .windows(2)
            .enumerate()
            .filter(|(i, _)| !dashed || i % 2 == 0)
            .map(|(_, w)| (w[0], w[1]))
            .collect();
        let mut have: Vec<Entity> = children
            .map(|c| c.iter().filter(|e| q_seg.contains(*e)).collect())
            .unwrap_or_default();
        for (k, (p, q)) in pieces.iter().enumerate() {
            let d = *q - *p;
            let len = d.length() + 0.6;
            let mid = (*p + *q) / 2.0;
            let node = Node {
                position_type: PositionType::Absolute,
                left: Val::Px(mid.x - len / 2.0),
                top: Val::Px(mid.y - thick / 2.0),
                width: Val::Px(len),
                height: Val::Px(thick),
                ..default()
            };
            let tf = UiTransform {
                rotation: Rot2::radians(d.y.atan2(d.x)),
                ..default()
            };
            match have.get(k).and_then(|e| q_seg.get_mut(*e).ok()) {
                Some((mut n, mut t, mut bg)) => {
                    *n = node;
                    *t = tf;
                    if bg.0 != color {
                        bg.0 = color;
                    }
                }
                None => {
                    commands.spawn((
                        Name::new("node-graph-wire-piece"),
                        WireSegment,
                        node,
                        tf,
                        BackgroundColor(color),
                        Pickable::IGNORE,
                        ChildOf(entity),
                    ));
                }
            }
        }
        for extra in have.drain(pieces.len().min(have.len())..) {
            commands.entity(extra).despawn();
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
    q_fixed: Query<(), With<ReadOnlySocket>>,
    q_wire: Query<(Entity, &Wire)>,
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
    if q_fixed.contains(socket) {
        return;
    }
    let Some(canvas) = canvas_of(socket, &q_parent, &q_canvas) else {
        return;
    };
    let Ok(c) = q_canvas.get(canvas) else { return };
    let s = q_socket.get(socket).unwrap();
    // From an input with a wire: the wire is picked up (hidden; nothing changes until it is
    // dropped), and the drag goes on from its output.
    let (from, detached) = match s.side {
        SocketSide::Out => (socket, None),
        SocketSide::In => {
            let Some((we, w)) = q_wire.iter().find(|(_, w)| w.to == socket) else {
                return;
            };
            commands.entity(we).insert(Visibility::Hidden);
            (w.from, Some((we, socket)))
        }
    };
    let to = canvas_point(ev.pointer.position, canvas, &q_geo)
        .map_or(Vec2::ZERO, |p| c.view.to_graph(p));
    commands.spawn((
        Name::new("node-graph-drag-wire"),
        DragWire { from, to, detached },
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
    q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut q_drag: Query<&mut DragWire>,
) {
    if ev.button != PointerButton::Primary || q_drag.is_empty() {
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
    q_socket: Query<(Entity, &Socket), Without<ReadOnlySocket>>,
    q_sock: Query<&Socket>,
    q_drag: Query<(Entity, &DragWire)>,
    q_canvas: Query<&mut NodeCanvas>,
    q_geo: Query<(&ComputedNode, &UiGlobalTransform)>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary {
        return;
    }
    // Every wire being dragged ends with the drag, wherever it began (its socket may be gone).
    for (e, d) in &q_drag {
        commands.entity(e).despawn();
        let canvas = canvas_of(d.from, &q_parent, &q_canvas);
        // Dropped back where it came from, or not dropped on an input (below): the picked-up
        // wire shows again until the app says otherwise.
        if let Some((we, _)) = d.detached
            && let Ok(mut w) = commands.get_entity(we)
        {
            w.insert(Visibility::Inherited);
        }
        let Some(canvas) = canvas else {
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
        let replaces = d.detached.map(|(_, old)| old);
        match best {
            Some((_, to)) if Some(to) == replaces => {}
            Some((_, to)) => {
                commands.trigger(WireConnect {
                    entity: canvas,
                    from: d.from,
                    to,
                    replaces,
                });
            }
            None => {
                if let Some(old) = replaces {
                    commands.trigger(WireDisconnect {
                        entity: canvas,
                        from: d.from,
                        to: old,
                    });
                }
            }
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

#[allow(clippy::type_complexity)]
fn mark_static_copies(
    q_root: Query<
        Entity,
        (
            With<StaticCopyRoot>,
            Or<(Added<StaticCopyRoot>, Changed<Children>)>,
        ),
    >,
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
    fn the_grid_spaces_out_when_zoomed_out() {
        assert_eq!(grid_step(20.0, 1.0), 20.0);
        // At a quarter, 20 units would be 5 px apart: every 4th line, 20 px apart.
        assert_eq!(grid_step(20.0, 0.25), 80.0);
        assert!(grid_step(20.0, 0.1) * 0.1 >= 12.0);
    }

    /// Every leg of a trace is horizontal, vertical or at 45°.
    fn octilinear(p: &[Vec2]) -> bool {
        p.windows(2).all(|w| {
            let d = w[1] - w[0];
            d.x.abs() < 1e-3 || d.y.abs() < 1e-3 || (d.x.abs() - d.y.abs()).abs() < 1e-3
        })
    }

    #[test]
    fn traces_run_straight_and_at_45() {
        for (a, b) in [
            (Vec2::ZERO, Vec2::new(300.0, 80.0)),
            (Vec2::ZERO, Vec2::new(300.0, -40.0)),
            (Vec2::ZERO, Vec2::new(300.0, 0.0)),
            // Back left, and too close for the 45° run.
            (Vec2::new(300.0, 0.0), Vec2::new(0.0, 200.0)),
            (Vec2::ZERO, Vec2::new(40.0, 300.0)),
        ] {
            let p = chamfered(&trace_points(a, b), 10.0);
            assert_eq!(p[0], a);
            assert_eq!(*p.last().unwrap(), b);
            assert!(octilinear(&p), "{a} → {b}: {p:?}");
            // It leaves the output and enters the input horizontally.
            assert!((p[1].y - a.y).abs() < 1e-3 && p[1].x > a.x);
            let n = p.len();
            assert!((p[n - 2].y - b.y).abs() < 1e-3 && p[n - 2].x < b.x);
        }
    }

    #[test]
    fn right_angles_are_cut_at_45() {
        let p = chamfered(
            &[Vec2::ZERO, Vec2::new(100.0, 0.0), Vec2::new(100.0, 100.0)],
            10.0,
        );
        assert_eq!(
            p,
            vec![
                Vec2::ZERO,
                Vec2::new(90.0, 0.0),
                Vec2::new(100.0, 10.0),
                Vec2::new(100.0, 100.0)
            ]
        );
        // A 45° bend stays as it is.
        let q = [Vec2::ZERO, Vec2::new(50.0, 0.0), Vec2::new(100.0, 50.0)];
        assert_eq!(chamfered(&q, 10.0), q.to_vec());
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

//! The **node view** of a Part Studio: its feature list as a node graph (Blender's geometry
//! nodes, Houdini's networks), with the studio's configuration as the node its values come from
//! (see `cadrs_core::configuration`). The feature list's header has a button that turns it on:
//! a panel opens under the 3D view (the feature list staying on the left; its top edge drags to
//! resize it) with an endless canvas ([`cadrs_ui::node_graph`]), the floating feature dialog
//! moving into its node, where:
//!
//! - **every feature is a node whose body is its dialog**. The feature being edited has its live
//!   dialog moved into its node; the others show a copy of their dialog with their parameters
//!   ([`cadrs_ui::node_graph::StaticCopyRoot`]: for looks, the systems keeping the live dialog in
//!   step leave it alone). Dialogs that can't be copied yet show their fields as rows. Clicking
//!   a node's body edits the feature (a sketch: double-click, as in the list); the context menu
//!   is the feature list's;
//! - **the Configuration node** (on the left, as Blender's Group Input) has the presets and the
//!   inputs, each input's value in the active preset and an output socket;
//! - **wires are expressions**: a field whose text is `#length` is wired from the input (or
//!   Variable) `length`; one that only mentions it (`#length / 2`) dashed. Dropping a wire on a
//!   field's socket writes `#name` into it, dragging it off writes the value back
//!   (`ConnectParameter`, `DisconnectParameter`), and a node's **Bypass** socket is its
//!   suppression variable (`SetSuppressByVariable`). The geometry wires (a sketch into its
//!   extrude, a face into its fillet) follow the features' references and are only shown.
//!
//! Where the nodes are is kept with the Part Studio (`MoveGraphNodes`, an undo step); a node
//! nobody placed is laid out in columns by how deep its references go.

use std::collections::{HashMap, HashSet};

use bevy::picking::events::{PointerClick, PointerPress};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::ui_widgets::Activate;
use cadrs_core::configuration::{
    self as cfg, ConfigInput, Configuration, ConnectParameter, DisconnectParameter, InputKind,
    MoveGraphNodes, SetNodeCollapsed,
};
use cadrs_core::ids::{ConfigInputId, ConfigRowId};
use cadrs_core::{Element, ElementId, Feature, FeatureId, FeatureKind};
use cadrs_sketch::units::{Quantity, Units};
use cadrs_ui::i18n::tr;
use cadrs_ui::node_graph::{
    CanvasViewChanged, FitCanvas, GraphNode, GraphNodeHandle, GraphNodeMoved, KeepLaidOut,
    NodeCanvas, NodeCanvasBuilder, PanZoom, Socket, SocketSide, StaticCopyRoot, Wire, WireConnect,
    WireDisconnect,
};
use cadrs_ui::{
    DoubleClickable, FeatureDialog, FeatureDialogState, IconButton, Menu, MenuItem, NumberField,
    Select, SelectChange, Theme,
};

use crate::ActiveDocument;
use crate::config_table::{
    self, ConfigElement, ConfigRole, begin_title_edit, edit_config, run, show_value, toast,
};
use crate::configuration_ui::ProblemAt;
use crate::document::FeatureRow;
use crate::parts::PartCache;
use crate::viewport::{Pick, PickRow, Selection, ViewportArea};

pub struct NodeViewPlugin;

impl Plugin for NodeViewPlugin {
    fn build(&self, app: &mut App) {
        // Wires go round the nodes as the PCB autorouter's tracks go round copper.
        app.insert_resource(cadrs_ui::node_graph::WireRouter(Some(std::sync::Arc::new(
            route_node_wires,
        ))))
        .init_resource::<NodeView>()
        .init_resource::<PendingMoves>()
        .init_resource::<MeasuredHeights>()
        .init_resource::<PointerOverNodes>()
        // (Before the key handlers of the frame: the 3D view's F looks at it.)
        .add_systems(
            PreUpdate,
            track_pointer_over.after(bevy::picking::PickingSystems::Hover),
        )
        .add_observer(on_toggle)
        .add_observer(on_panel_resized)
        .add_observer(on_drive_dimension)
        .add_observer(on_view_changed)
        .add_observer(on_node_moved)
        .add_observer(on_wire_connect)
        .add_observer(on_wire_disconnect)
        .add_observer(on_wire_dropped_outside)
        .add_observer(on_body_press)
        .add_observer(on_body_click)
        .add_observer(on_create_menu)
        .add_observer(on_collapse)
        .add_observer(on_header_preset)
        .add_observer(on_box_selected)
        .add_systems(
            Update,
            (
                sync_mode,
                flush_moves,
                sync_nodes,
                open_pending_title,
                host_live_dialog,
                sync_selected,
                fit_later,
                pan_to_problem,
                sync_header_preset,
                measure_heights,
                fit_on_f,
                sync_grid_scheme,
            )
                .chain()
                .run_if(resource_exists::<ActiveDocument>),
        );
    }
}

// ---------------------------------------------------------------------------------------------
// State

/// Which Part Studios show their nodes, and how each canvas was last panned and zoomed (kept
/// while the app runs: view state, not the document's).
#[derive(Resource, Debug, Default)]
pub struct NodeView {
    on: HashSet<ElementId>,
    views: HashMap<ElementId, PanZoom>,
    /// The node panel's height (px), as its top edge was last dragged to.
    height: Option<f32>,
}

impl NodeView {
    /// Whether `element` shows its nodes.
    pub fn is_on(&self, element: ElementId) -> bool {
        self.on.contains(&element)
    }

    /// Shows or hides `element`'s nodes.
    pub fn set(&mut self, element: ElementId, on: bool) {
        if on {
            self.on.insert(element);
        } else {
            self.on.remove(&element);
        }
    }
}

/// The panel under the 3D view the node view shows in ([`panel`]), the feature list staying
/// where it is.
#[derive(Component, Debug, Clone, Copy)]
pub struct NodePanel;

/// The node panel's top edge, which resizes it.
const PANEL_EDGE: &str = "node-panel-edge";

/// The node view's canvas, for the Part Studio it shows.
#[derive(Component, Debug, Clone, Copy)]
struct NodeViewCanvas(ElementId);

/// A feature's node, and what it was built from.
#[derive(Component, Debug, Clone)]
struct FeatureNode {
    feature: FeatureId,
    key: NodeKey,
}

/// What a node's look depends on: a new key rebuilds it. A node being edited depends on less,
/// as its live dialog shows the parameters (rebuilding it would take the dialog's focus).
#[derive(Debug, Clone, PartialEq)]
struct NodeKey {
    /// A hash of the feature (none while it is edited).
    feature: Option<u64>,
    live: bool,
    collapsed: bool,
    suppressed: bool,
    rolled_back: bool,
    error: Option<String>,
    /// What its output row lists (the parts it makes or changes).
    outputs: Vec<String>,
    theme: u64,
}

/// The Parts node (on the right, as Blender's Group Output): a row for each part the studio
/// ends with, wired from the last feature that changes it.
#[derive(Component, Debug, Clone)]
struct PartsNode {
    key: (Vec<(cadrs_core::PartId, String)>, u64),
}

/// An input node (one per configuration input), and what it was built from.
#[derive(Component, Debug, Clone)]
struct ConfigNode {
    input: ConfigInputId,
    key: ConfigKey,
}

/// What an input node's look depends on: the configuration (an output's kind may follow a
/// feature's field), the units, the theme, the value a feature can't take (outlined red).
type ConfigKey = (Configuration, Units, u64, Option<ProblemAt>);

/// A node's copy of its feature's dialog (hidden while the live dialog is in the node).
#[derive(Component, Debug, Clone, Copy)]
struct StaticCard;

/// The transparent cover over a static card's body: a click on it edits the feature.
#[derive(Component, Debug, Clone, Copy)]
struct BodyCover(FeatureId);

/// The live dialog of the feature being edited, moved into its node.
#[derive(Component, Debug, Clone, Copy)]
struct HostedDialog;

/// What a socket stands for.
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash)]
enum SocketKey {
    /// The geometry a feature makes (its parts, sketch or plane), on its right.
    GeoOut(FeatureId),
    /// The geometry a feature references, on its left.
    GeoIn(FeatureId),
    /// The parts a feature adds to, cuts or merges with, on its Merge scope row.
    Scope(FeatureId),
    /// A field of a feature ("Depth").
    Slot(FeatureId, &'static str),
    /// A feature's suppression variable.
    Bypass(FeatureId),
    /// A configuration input, by name.
    Input(String),
    /// A Variable feature's value (the feature: a name can be defined again further down).
    Variable(FeatureId),
    /// A part of the Parts node.
    Part(cadrs_core::PartId),
    /// A sketch dimension an expression drives ([`cfg::dimension_slots`]).
    Dimension(FeatureId, cadrs_sketch::DimensionId),
    /// A sketch's "Drive a dimension" input: a wire dropped there asks which dimension.
    DriveDimension(FeatureId),
}

/// A node's collapse chevron.
#[derive(Component, Debug, Clone, Copy)]
struct CollapseButton(FeatureId);

/// The press that may become a click on a body (where it was), so a drag isn't taken for one.
#[derive(Component, Debug, Clone, Copy)]
struct PressedAt(Vec2);

/// F with the pointer over the node view (and no field typed in) frames every node, as F frames
/// the model in the 3D view.
/// Whether the pointer is over a node view (where F fits the nodes, and not the 3D view).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct PointerOverNodes(pub bool);

fn track_pointer_over(
    hover: Res<bevy::picking::hover::HoverMap>,
    q_canvas: Query<Entity, With<NodeViewCanvas>>,
    q_parent: Query<&ChildOf>,
    mut over: ResMut<PointerOverNodes>,
) {
    let now = q_canvas.iter().any(|canvas| {
        hover
            .values()
            .flat_map(|m| m.keys())
            .any(|e| *e == canvas || q_parent.iter_ancestors(*e).any(|a| a == canvas))
    });
    if over.0 != now {
        over.0 = now;
    }
}

fn fit_on_f(
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<bevy::input_focus::InputFocus>,
    over: Res<PointerOverNodes>,
    q_canvas: Query<Entity, With<NodeViewCanvas>>,
    mut commands: Commands,
) {
    if !keys.just_pressed(KeyCode::KeyF) || focus.get().is_some() || !over.0 {
        return;
    }
    for canvas in &q_canvas {
        commands.trigger(FitCanvas { entity: canvas });
    }
}

/// A canvas just made: its nodes are built whatever changed.
#[derive(Component, Debug, Clone, Copy)]
struct FreshCanvas;

/// The feature nodes' heights as laid out (graph px), so the auto layout stacks them without
/// overlapping once they have been laid out once.
#[derive(Resource, Debug, Default)]
struct MeasuredHeights(HashMap<FeatureId, f32>);

fn measure_heights(
    q: Query<(&FeatureNode, &ComputedNode), Changed<ComputedNode>>,
    mut heights: ResMut<MeasuredHeights>,
) {
    for (n, c) in &q {
        let h = (c.size().y * c.inverse_scale_factor()).round();
        if h > 0.0 && heights.0.get(&n.feature) != Some(&h) {
            heights.0.insert(n.feature, h);
        }
    }
}

/// Frames the canvas's nodes in this many frames (once they are laid out).
#[derive(Component, Debug, Clone, Copy)]
struct FitLater(u8);

fn fit_later(mut q: Query<(Entity, &mut FitLater)>, mut commands: Commands) {
    for (e, mut f) in &mut q {
        if f.0 == 0 {
            commands.entity(e).remove::<FitLater>();
            commands.trigger(FitCanvas { entity: e });
        } else {
            f.0 -= 1;
        }
    }
}

/// Pans the node view so the field of a value a feature can't take
/// ([`crate::configuration_ui::reveal_problem`]) is in its middle (zoomed in to be read), once
/// its node is laid out; a node culled out of view is brought near first. Given up after two
/// seconds.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn pan_to_problem(
    problem: Option<Res<crate::configuration_ui::ConfigProblem>>,
    mut seen: Local<Option<ProblemAt>>,
    mut pending: Local<Option<(ProblemAt, ElementId, u32)>>,
    mut q_canvas: Query<(
        Entity,
        &NodeViewCanvas,
        &mut NodeCanvas,
        &ComputedNode,
        &UiGlobalTransform,
        Has<FitLater>,
    )>,
    q_roles: Query<(Entity, &ConfigRole, &ComputedNode, &UiGlobalTransform)>,
    q_parent: Query<&ChildOf>,
    q_layer: Query<&UiGlobalTransform>,
    q_nodes: Query<(&ConfigNode, &GraphNode)>,
    mut commands: Commands,
) {
    match problem.as_deref() {
        Some(p) if *seen != Some(p.at) => {
            *seen = Some(p.at);
            *pending = Some((p.at, p.element, 0));
        }
        Some(_) => {}
        None => {
            *seen = None;
            *pending = None;
        }
    }
    let Some((at, element, frames)) = pending.as_mut() else {
        return;
    };
    *frames += 1;
    if *frames > 120 {
        *pending = None;
        return;
    }
    let Some((canvas_e, _, mut canvas, cn, cg, fitting)) =
        q_canvas.iter_mut().find(|(_, c, ..)| c.0 == *element)
    else {
        return;
    };
    // (A canvas just opened frames its nodes first.)
    if fitting || cn.size().x <= 0.0 {
        return;
    }
    let s = cn.inverse_scale_factor();
    let shown = Rect::from_center_size(cg.translation * s, cn.size() * s);
    let at = *at;
    // (The Configuration panel has the same field: this canvas's.)
    let field = q_roles
        .iter()
        .filter(|(_, r, ..)| match (r, at) {
            (ConfigRole::Cell(i, j, k), ProblemAt::Cell { input, row, param }) => {
                *i == input && *j == param && *k == row
            }
            (ConfigRole::Value(i), ProblemAt::Value(v)) => *i == v,
            _ => false,
        })
        .find(|(e, ..)| q_parent.iter_ancestors(*e).any(|a| a == canvas_e))
        .map(|(_, r, n, g)| (r, n, g));
    let mut view = canvas.view;
    match field {
        // (After a few frames: its table scrolls sideways to it first.)
        Some((_, n, g)) if n.size().x > 0.0 && *frames >= 6 => {
            // Where it is in the graph, from its place against the nodes' layer as last laid
            // out (both from the same layout, though the view may have changed since: a canvas
            // just opened is framed first).
            let Ok(layer) = q_layer.get(canvas.world()) else {
                return;
            };
            let laid_zoom = layer.matrix2.x_axis.length().max(1e-3);
            let cell = (g.translation - layer.translation) * s / laid_zoom;
            view.zoom = view.zoom.max(0.8);
            view.pan = shown.size() / 2.0 - cell * view.zoom;
            *pending = None;
        }
        _ => {
            let input = match at {
                ProblemAt::Cell { input, .. } | ProblemAt::Value(input) => input,
            };
            let Some((_, g)) = q_nodes.iter().find(|(c, _)| c.input == input) else {
                return;
            };
            view.pan = shown.size() / 3.0 - g.pos * view.zoom;
        }
    }
    if view != canvas.view {
        canvas.view = view;
        commands.trigger(CanvasViewChanged {
            entity: canvas_e,
            view,
        });
    }
}

/// Node moves to save as one undo step (a drag of several selected nodes ends on each).
#[derive(Resource, Debug, Default)]
struct PendingMoves(Vec<(Moved, Vec2)>);

/// Which node was moved.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Moved {
    Feature(FeatureId),
    Input(ConfigInputId),
    Parts,
}

// ---------------------------------------------------------------------------------------------
// Socket types and colours

const GEOMETRY: u32 = 1;
const LENGTH: u32 = 2;
const ANGLE: u32 = 3;
const NUMBER: u32 = 4;
const BOOLEAN: u32 = 5;

fn quantity_type(q: Quantity) -> u32 {
    match q {
        Quantity::Length => LENGTH,
        Quantity::Angle => ANGLE,
        Quantity::Count => NUMBER,
    }
}

fn socket_color(ty: u32) -> Color {
    match ty {
        GEOMETRY => Color::srgb_u8(0x4c, 0xb0, 0x5c),
        // (Red, as the input nodes' headers: grey was lost in the grid.)
        LENGTH => Color::srgb_u8(0xe5, 0x55, 0x50),
        ANGLE => Color::srgb_u8(0x8f, 0x6f, 0xd6),
        NUMBER => Color::srgb_u8(0x3f, 0xa7, 0xc4),
        _ => Color::srgb_u8(0xd0, 0x7c, 0xc0),
    }
}

/// A sketch's wires are blue, a plane's grey, the rest (parts) green.
fn geometry_color(f: &Feature) -> Color {
    match f.kind {
        FeatureKind::Sketch(_) => Color::srgb_u8(0x3d, 0x84, 0xe0),
        FeatureKind::Plane(_) | FeatureKind::MateConnector(_) => PLANE_GREY,
        _ => socket_color(GEOMETRY),
    }
}

/// What a plane (a Plane feature's output, or a default plane a sketch is on) is drawn in.
const PLANE_GREY: Color = Color::srgb(
    0x90 as f32 / 255.0,
    0x90 as f32 / 255.0,
    0x90 as f32 / 255.0,
);

/// A node's header tint by what the feature is (Blender colours its node headers by kind).
fn category_color(f: &Feature) -> Color {
    let (r, g, b) = match &f.kind {
        FeatureKind::Sketch(_) => (0x3d, 0x84, 0xe0),
        FeatureKind::Variable(_) => (0x8f, 0x6f, 0xd6),
        FeatureKind::Plane(_) | FeatureKind::MateConnector(_) => (0x90, 0x90, 0x90),
        FeatureKind::Fillet(_)
        | FeatureKind::Chamfer(_)
        | FeatureKind::Shell(_)
        | FeatureKind::Hole(_)
        | FeatureKind::Draft(_) => (0xe0, 0x8a, 0x2e),
        FeatureKind::Pattern(_) | FeatureKind::Mirror(_) | FeatureKind::Transform(_) => {
            (0x3f, 0xa7, 0xc4)
        }
        FeatureKind::SheetMetalModel(_)
        | FeatureKind::SheetMetal(_)
        | FeatureKind::SheetMetalTool(_)
        | FeatureKind::SheetMetalLoft(_)
        | FeatureKind::ModifyJoint(_)
        | FeatureKind::Form(_)
        | FeatureKind::TagForm(_)
        | FeatureKind::FlatExtrude(_) => (0x2e, 0x9e, 0x8f),
        _ => (0x4c, 0xb0, 0x5c),
    };
    Color::srgba_u8(r, g, b, 0x40)
}

/// The Configuration node's header tint.
fn config_color() -> Color {
    Color::srgba_u8(0xd0, 0x4f, 0x4f, 0x40)
}

/// Lowercase letters and digits only: how a field's label is matched to its row's name.
fn squash(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

// ---------------------------------------------------------------------------------------------
// The toggle and the panel

/// The node panel: under the 3D view, the width of it, hidden until the feature list header's
/// node view button shows it (`document`'s shell spawns it, [`panel_edge`] in it).
pub fn panel(t: &Theme, gap: f32) -> impl Bundle {
    (
        Name::new("node-panel"),
        NodePanel,
        Node {
            height: Val::Percent(40.0),
            flex_shrink: 0.0,
            min_height: Val::Px(0.0),
            margin: UiRect::top(Val::Px(gap)),
            flex_direction: FlexDirection::Column,
            // (Not clipped: its top edge's grip reaches into the gap above it. The canvas clips
            // itself.)
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(t.radius_panel)),
            display: Display::None,
            ..default()
        },
        cadrs_ui::glass::panel(t),
    )
}

/// The node panel's top edge (in the gap above it): dragged, the panel grows or shrinks.
pub fn panel_edge(panel: Entity, gap: f32) -> impl Bundle {
    cadrs_ui::top_edge(PANEL_EDGE, panel, gap + 1.0, 120.0, 120.0)
}

/// Remembers the node panel's height when its edge is let go.
fn on_panel_resized(ev: On<cadrs_ui::SplitterMoved>, q: Query<&Name>, mut view: ResMut<NodeView>) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == PANEL_EDGE) {
        view.height = Some(ev.height);
    }
}

fn on_toggle(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
    if q.get(a.entity)
        .is_ok_and(|n| n.as_str() == "feature-node-view")
    {
        commands.queue(toggle);
    }
}

/// Turns the active Part Studio's node view on or off.
pub fn toggle(world: &mut World) {
    let Some(el) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active_element().map(|e| e.id))
    else {
        return;
    };
    let mut v = world.resource_mut::<NodeView>();
    let on = !v.is_on(el);
    v.set(el, on);
}

/// Shows the node panel while the active Part Studio's node view is on (at the height it was
/// last dragged to), and keeps the header button pressed meanwhile.
#[allow(clippy::type_complexity)]
fn sync_mode(
    view: Res<NodeView>,
    doc: Res<ActiveDocument>,
    mut q_panel: Query<&mut Node, With<NodePanel>>,
    q_edge: Query<&cadrs_ui::Splitter>,
    q_button: Query<(Entity, &Name, Has<cadrs_ui::Selected>), With<bevy::ui_widgets::Button>>,
    mut commands: Commands,
) {
    let active = doc.active_element().map(|e| e.id);
    let on = active.is_some_and(|e| view.is_on(e));
    let dragging = q_edge.iter().any(|s| s.dragging());
    for mut n in &mut q_panel {
        let want = if on { Display::Flex } else { Display::None };
        if n.display != want {
            n.display = want;
        }
        if let Some(h) = view.height
            && !dragging
            && n.height != Val::Px(h)
        {
            n.height = Val::Px(h);
        }
    }
    for (e, name, sel) in &q_button {
        if name.as_str() == "feature-node-view" && sel != on {
            if on {
                commands.entity(e).insert(cadrs_ui::Selected);
            } else {
                commands.entity(e).remove::<cadrs_ui::Selected>();
            }
        }
    }
}

fn on_view_changed(
    ev: On<CanvasViewChanged>,
    q: Query<&NodeViewCanvas>,
    mut view: ResMut<NodeView>,
) {
    if let Ok(c) = q.get(ev.entity) {
        view.views.insert(c.0, ev.view);
    }
}

// ---------------------------------------------------------------------------------------------
// Building the nodes

/// The width of a node (as the floating dialogs').
const NODE_W: f32 = 236.0;
/// Columns are this far apart.
const COLUMN: f32 = NODE_W + 90.0;

/// A guess of a node's height before it is laid out, for placing the ones below it.
fn height_guess(f: &Feature, collapsed: bool) -> f32 {
    if collapsed {
        return 34.0;
    }
    match &f.kind {
        FeatureKind::Extrude(_) => 330.0,
        FeatureKind::Revolve(_) => 300.0,
        FeatureKind::Sketch(_) => 320.0,
        _ => 70.0 + 26.0 * cfg::parameter_slots(f).len().max(1) as f32,
    }
}

/// Where the nodes go: each feature's place, and the Parts node's.
struct Layout {
    features: HashMap<FeatureId, Vec2>,
    parts: Vec2,
}

/// Where each input node goes: its saved place (the first's, the single Configuration node's of
/// earlier versions until moved), or stacked at the left, their left edges lined up (a node
/// growing a configuration stays where it is; the features laid out make room, [`layout`]).
fn input_positions(el: &Element) -> Vec<Vec2> {
    let saved = el.node_layout();
    let mut y = 0.0;
    el.configuration
        .inputs
        .iter()
        .enumerate()
        .map(|(k, input)| {
            let default = Vec2::new(0.0, y);
            y += input_node_height(input) + 30.0;
            saved
                .and_then(|l| {
                    l.inputs
                        .get(&input.id)
                        .copied()
                        .or(if k == 0 { l.inputs_node } else { None })
                })
                .map_or(default, Vec2::from_array)
        })
        .collect()
}

/// Where each node goes: its saved place, or laid out as trees that follow the wires:
///
/// - Each node hangs off one **primary parent**: of the features it references, the one with
///   the fewest other children (a plane's only sketch hangs off the plane, a sketch's only
///   extrude off the sketch); a node referencing nothing starts a tree.
/// - A node sits **right of its primary parent, on its row**; a parent's further children
///   stack below the first, each below the whole branch above it, so branches never overlap
///   and a chain (plane → sketch → extrude) lies on one row, its wires short and straight.
/// - Trees stack top to bottom in list order. Wires to other parents (a merge scope, a
///   fillet's edges) are routed to wherever those are.
///
/// The Parts node is right of them all. Saved places win over all this.
fn layout(el: &Element, measured: &HashMap<FeatureId, f32>) -> Layout {
    let saved = el.node_layout();
    let features = el.features();
    let collapsed = |f: FeatureId| saved.is_some_and(|l| l.collapsed.contains(&f));
    let index: HashMap<FeatureId, usize> = features
        .iter()
        .enumerate()
        .map(|(i, f)| (f.id, i))
        .collect();
    let parents: Vec<Vec<usize>> = features
        .iter()
        .map(|f| {
            cadrs_core::feature_list::parents(features, f.id)
                .iter()
                .filter_map(|p| index.get(p).copied())
                .collect()
        })
        .collect();
    let mut fan_out = vec![0usize; features.len()];
    for ps in &parents {
        for p in ps {
            fan_out[*p] += 1;
        }
    }
    // Each node's primary parent: the one with the fewest children (the latest on a tie: the
    // nearer step in the history).
    let primary: Vec<Option<usize>> = parents
        .iter()
        .map(|ps| {
            ps.iter()
                .copied()
                .min_by(|a, b| fan_out[*a].cmp(&fan_out[*b]).then(b.cmp(a)))
        })
        .collect();
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); features.len()];
    for (i, p) in primary.iter().enumerate() {
        if let Some(p) = p {
            kids[*p].push(i);
        }
    }
    let height = |i: usize| {
        let f = &features[i];
        // Its height as laid out, once it has been; a guess before.
        measured
            .get(&f.id)
            .copied()
            .unwrap_or_else(|| height_guess(f, collapsed(f.id)))
    };
    const GAP: f32 = 24.0;
    // A branch's height: its node's, or its children's branches stacked, whichever is more.
    fn branch(
        i: usize,
        kids: &[Vec<usize>],
        height: &dyn Fn(usize) -> f32,
        memo: &mut Vec<Option<f32>>,
    ) -> f32 {
        if let Some(h) = memo[i] {
            return h;
        }
        let stacked: f32 = kids[i]
            .iter()
            .map(|k| branch(*k, kids, height, memo))
            .sum::<f32>()
            + GAP * kids[i].len().saturating_sub(1) as f32;
        let h = height(i).max(stacked);
        memo[i] = Some(h);
        h
    }
    let mut memo = vec![None; features.len()];
    for i in 0..features.len() {
        branch(i, &kids, &height, &mut memo);
    }
    // A tree's width: as many columns as its deepest branch.
    fn reach(i: usize, kids: &[Vec<usize>]) -> usize {
        1 + kids[i].iter().map(|k| reach(*k, kids)).max().unwrap_or(0)
    }
    let roots: Vec<usize> = (0..features.len())
        .filter(|i| primary[*i].is_none())
        .collect();
    // The trees go in bands side by side (as shelves fill a page), each band about as tall as
    // makes the whole about half again as wide as it is tall, the shape of the panel.
    let area: f32 = roots
        .iter()
        .map(|r| (memo[*r].unwrap_or(0.0) + GAP) * reach(*r, &kids) as f32 * COLUMN)
        .sum();
    let band = (area / 1.6).sqrt().max(
        roots
            .iter()
            .map(|r| memo[*r].unwrap_or(0.0))
            .fold(0.0, f32::max),
    );
    // Place each branch: the node at (x, y), its children from (x + COLUMN, y) down.
    let mut pos: Vec<Vec2> = vec![Vec2::ZERO; features.len()];
    let mut stack: Vec<(usize, Vec2)> = Vec::new();
    // (Right of the widest input node.)
    let start = el
        .configuration
        .inputs
        .iter()
        .map(|i| input_node_width(i) + 90.0)
        .fold(COLUMN, f32::max);
    let (mut x, mut y, mut widest) = (start, 0.0f32, 0usize);
    for root in roots {
        let h = memo[root].unwrap_or(0.0);
        if y > 0.0 && y + h > band {
            x += widest as f32 * COLUMN + 60.0;
            y = 0.0;
            widest = 0;
        }
        widest = widest.max(reach(root, &kids));
        stack.push((root, Vec2::new(x, y)));
        y += h + GAP;
    }
    while let Some((i, at)) = stack.pop() {
        pos[i] = at;
        let mut cy = at.y;
        for k in &kids[i] {
            stack.push((*k, Vec2::new(at.x + COLUMN, cy)));
            cy += memo[*k].unwrap_or(0.0) + GAP;
        }
    }
    let mut out: HashMap<FeatureId, Vec2> = HashMap::new();
    let mut right = 0.0f32;
    for (i, f) in features.iter().enumerate() {
        let p = saved
            .and_then(|l| l.positions.get(&f.id))
            .map_or(pos[i], |p| Vec2::from_array(*p));
        right = right.max(p.x + NODE_W);
        out.insert(f.id, p);
    }
    Layout {
        features: out,
        parts: saved
            .and_then(|l| l.parts_node)
            .map_or(Vec2::new(right.max(COLUMN) + 90.0, 0.0), Vec2::from_array),
    }
}

/// The parts each feature makes or changes (its output row lists them).
fn outputs_of(
    parts: &[cadrs_core::Part],
    props: &[cadrs_core::PartProps],
) -> HashMap<FeatureId, Vec<String>> {
    let mut out: HashMap<FeatureId, Vec<String>> = HashMap::new();
    for p in parts {
        let name = cadrs_core::parts::display_name(p, props).to_string();
        let mut fs = p.features.clone();
        if !fs.contains(&p.feature) {
            fs.insert(0, p.feature);
        }
        for f in fs {
            let v = out.entry(f).or_default();
            if !v.contains(&name) {
                v.push(name.clone());
            }
        }
    }
    out
}

/// The last feature (in list order) that makes or changes each part: the Parts node's wires.
fn last_feature_of(features: &[Feature], part: &cadrs_core::Part) -> Option<FeatureId> {
    features
        .iter()
        .rev()
        .find(|f| f.id == part.feature || part.features.contains(&f.id))
        .map(|f| f.id)
}

/// The canvas's grid: the PCB Studio schematic's (`cadrs_eda::render::SchematicTheme`), in the
/// style of the 3D view's ground grid (Preferences › Interface › Grid: off, crosses on every
/// fifth line, or every tenth line stronger), for the scheme in force (it is drawn by a
/// shader, whose colours the UI doesn't map to the dark scheme: [`sync_grid_scheme`] swaps
/// them when the scheme or the preference changes).
fn canvas_grid(style: cadrs_core::preferences::GridStyle) -> cadrs_ui::node_graph::CanvasGrid {
    use cadrs_core::preferences::GridStyle as Pref;
    use cadrs_ui::node_graph::GridStyle;
    let rgba = |c: [u8; 4]| Color::srgba_u8(c[0], c[1], c[2], c[3]);
    let style = match style {
        Pref::Off => GridStyle::Off,
        Pref::Crossed => GridStyle::Crossed,
        Pref::Lines => GridStyle::Lines,
    };
    if cadrs_ui::dark_in_force() {
        let th = cadrs_eda::render::SchematicTheme::dark();
        cadrs_ui::node_graph::CanvasGrid {
            step: 20.0,
            line: rgba(th.grid_line),
            mark: rgba(th.grid),
            style,
        }
    } else {
        // Light: the schematic's grey on the view's light ground, the lines as faint (a whole
        // line every tenth is fainter than a cross).
        let th = cadrs_eda::render::SchematicTheme::default();
        let mark = rgba(th.grid);
        cadrs_ui::node_graph::CanvasGrid {
            step: 20.0,
            line: Color::srgba_u8(0x40, 0x40, 0x40, 0x14),
            mark: if style == GridStyle::Lines {
                mark.with_alpha(0.28)
            } else {
                mark
            },
            style,
        }
    }
}

/// The grid follows the scheme (light or dark) and the grid preference when they change.
fn sync_grid_scheme(
    prefs: Res<crate::preferences_ui::LocalPreferences>,
    mut q: Query<&mut cadrs_ui::node_graph::CanvasGrid>,
) {
    let want = canvas_grid(prefs.0.grid);
    for mut g in &mut q {
        if *g != want {
            *g = want;
        }
    }
}

/// A hash of a feature (its name, parameters and suppression rule), for telling whether its
/// node must be built again.
fn feature_hash(f: &Feature) -> u64 {
    use std::hash::Hasher;
    /// Hashes what is written to it (the feature's debug text, never built as a string).
    struct Sink(std::collections::hash_map::DefaultHasher);
    impl std::fmt::Write for Sink {
        fn write_str(&mut self, s: &str) -> std::fmt::Result {
            self.0.write(s.as_bytes());
            Ok(())
        }
    }
    let mut h = Sink(Default::default());
    let _ = std::fmt::Write::write_fmt(
        &mut h,
        format_args!("{:?}{:?}{:?}", f.name, f.kind, f.suppress_by),
    );
    h.0.finish()
}

/// Moves the live dialog hosted under `under` (a node or the canvas about to go) back over the
/// view, floating as it was, so its session keeps its dialog.
fn rescue_hosted(world: &mut World, under: Entity) {
    let mut q = world.query_filtered::<Entity, With<HostedDialog>>();
    let hosted: Vec<Entity> = q
        .iter(world)
        .filter(|d| on_canvas(world, *d, under))
        .collect();
    if hosted.is_empty() {
        return;
    }
    let mut q_area = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else {
        return;
    };
    let t = world.resource::<Theme>().clone();
    for d in hosted {
        {
            let mut commands = world.commands();
            cadrs_ui::feature_dialog::unembed(&mut commands, d, &t);
            commands
                .entity(d)
                .remove::<HostedDialog>()
                .insert(ChildOf(area));
        }
        world.flush();
        if let Some(mut n) = world.get_mut::<Node>(d) {
            n.position_type = PositionType::Absolute;
            // (Shown, whatever its node's culling left.)
            n.display = Display::Flex;
        }
        // Its header no longer drags a node.
        if let Some(h) = world.get::<Children>(d).and_then(|c| c.first().copied()) {
            world
                .entity_mut(h)
                .remove::<GraphNodeHandle>()
                .insert(BackgroundColor(Color::NONE));
        }
    }
}

/// A cheap fingerprint of the theme (a scheme change rebuilds the nodes).
fn theme_key(t: &Theme) -> u64 {
    let c = t.foreground.to_srgba();
    // The scheme in force too: images (sketch thumbnails) are drawn for it.
    (cadrs_ui::dark_in_force() as u64) << 24
        | ((c.red * 255.0) as u64) << 16
        | ((c.green * 255.0) as u64) << 8
        | (c.blue * 255.0) as u64
}

/// Builds, updates and removes the nodes and wires of the shown Part Studio.
fn sync_nodes(
    world: &mut World,
    mut last_edited: Local<Option<Option<FeatureId>>>,
    mut last_problem: Local<Option<ProblemAt>>,
) {
    // The host of the active Part Studio, if its node view is on.
    let active = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id);
    let on = active.filter(|e| world.resource::<NodeView>().is_on(*e));
    let mut q_host = world.query_filtered::<Entity, With<NodePanel>>();
    let host = on.and_then(|_| q_host.iter(world).next());
    let mut q_canvas = world.query::<(Entity, &NodeViewCanvas, &NodeCanvas)>();
    let canvases: Vec<(Entity, ElementId, NodeCanvas)> =
        q_canvas.iter(world).map(|(e, c, n)| (e, c.0, *n)).collect();
    for (e, el, _) in &canvases {
        if Some(*el) != on || host.is_none() {
            rescue_hosted(world, *e);
            world.entity_mut(*e).despawn();
        }
    }
    let (Some(element), Some(host)) = (on, host) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let canvas = match canvases.iter().find(|(_, el, _)| *el == element) {
        Some((e, _, c)) => (*e, *c),
        None => {
            let saved = world.resource::<NodeView>().views.get(&element).copied();
            let grid_pref = world
                .get_resource::<crate::preferences_ui::LocalPreferences>()
                .map(|p| p.0.grid)
                .unwrap_or_default();
            let mut commands = world.commands();
            let e = NodeCanvasBuilder::new("node-canvas")
                .view(saved.unwrap_or_default())
                .grid(canvas_grid(grid_pref))
                .spawn(&mut commands, host, &theme);
            commands
                .entity(e)
                .insert((NodeViewCanvas(element), FreshCanvas));
            world.flush();
            spawn_canvas_tools(world, e, &theme);
            if saved.is_none() {
                // Framed once its nodes are laid out.
                world.entity_mut(e).insert(FitLater(3));
            }
            let c = *world.get::<NodeCanvas>(e).unwrap();
            (e, c)
        }
    };
    let (canvas_e, canvas) = canvas;
    let edited = crate::variables_ui::edited_feature(world)
        .filter(|(e, _)| *e == element)
        .map(|(_, f)| f);
    // Only when something it shows changed (the document, the rebuild, the view, the node
    // heights, the theme, the feature being edited) or the canvas is new: a large studio's nodes
    // are not looked over every frame.
    let fresh = world.get::<FreshCanvas>(canvas_e).is_some();
    let dirty = fresh
        || world.resource_ref::<ActiveDocument>().is_changed()
        || world.resource_ref::<PartCache>().is_changed()
        || world.resource_ref::<NodeView>().is_changed()
        || world.resource_ref::<MeasuredHeights>().is_changed()
        || world.resource_ref::<Theme>().is_changed()
        || *last_edited != Some(edited)
        || *last_problem != crate::configuration_ui::problem_in(world, element);
    if !dirty {
        return;
    }
    *last_edited = Some(edited);
    *last_problem = crate::configuration_ui::problem_in(world, element);
    if fresh {
        world.entity_mut(canvas_e).remove::<FreshCanvas>();
    }
    let doc = world.resource::<ActiveDocument>();
    let Some(el) = doc.doc.element(element).cloned() else {
        return;
    };
    let units = doc.doc.units;
    let cache_errors: HashMap<FeatureId, String> = world.resource::<PartCache>().errors.clone();
    let suppressed = el.all_suppressed();
    let bar = el.rollback_index();
    let tk = theme_key(&theme);
    let Layout {
        features: positions,
        parts: parts_pos,
    } = layout(&el, &world.resource::<MeasuredHeights>().0);
    let parts: Vec<cadrs_core::Part> = world.resource::<PartCache>().parts.clone();
    let outputs = outputs_of(&parts, el.part_props());
    let collapsed = |f: FeatureId| el.node_layout().is_some_and(|l| l.collapsed.contains(&f));

    // The nodes there are, and those to rebuild.
    let mut q_nodes = world.query::<(Entity, &FeatureNode, &ChildOf)>();
    let have: Vec<(Entity, FeatureNode)> = q_nodes
        .iter(world)
        .filter(|(_, _, p)| p.parent() == canvas.world())
        .map(|(e, n, _)| (e, n.clone()))
        .collect();
    let mut keep: HashMap<FeatureId, Entity> = HashMap::new();
    let mut changed = false;
    for (i, f) in el.features().iter().enumerate() {
        let live = edited == Some(f.id);
        // A node being edited depends on less: its live dialog shows the parameters and the
        // errors, and rebuilding the node would take the dialog's focus.
        let key = NodeKey {
            feature: (!live).then(|| feature_hash(f)),
            live,
            collapsed: collapsed(f.id),
            suppressed: suppressed.contains(&f.id),
            rolled_back: i >= bar,
            error: if live {
                None
            } else {
                cache_errors.get(&f.id).cloned()
            },
            outputs: if live {
                Vec::new()
            } else {
                outputs.get(&f.id).cloned().unwrap_or_default()
            },
            theme: tk,
        };
        let pos = positions.get(&f.id).copied().unwrap_or_default();
        if let Some((e, n)) = have.iter().find(|(_, n)| n.feature == f.id)
            && n.key == key
        {
            keep.insert(f.id, *e);
            place(world, *e, pos);
            continue;
        }
        changed = true;
        let shown = outputs.get(&f.id).cloned().unwrap_or_default();
        let e = spawn_feature_node(
            world,
            &el,
            f,
            key,
            &shown,
            pos,
            canvas.world(),
            &theme,
            &units,
        );
        keep.insert(f.id, e);
    }
    for (e, n) in &have {
        if keep.get(&n.feature) != Some(e) {
            changed = true;
            rescue_hosted(world, *e);
            if let Ok(em) = world.get_entity_mut(*e) {
                em.despawn();
            }
        }
    }
    // The input nodes, one per configuration input.
    let ckey = (el.configuration.clone(), units, tk, *last_problem);
    let mut q_cfg = world.query::<(Entity, &ConfigNode)>();
    let cfg_have: Vec<(Entity, ConfigNode)> =
        q_cfg.iter(world).map(|(e, c)| (e, c.clone())).collect();
    let resolved = el.configuration.resolve(&el.configuration.active);
    let places = input_positions(&el);
    let mut kept: HashSet<Entity> = HashSet::new();
    for ((input, r), pos) in el.configuration.inputs.iter().zip(resolved).zip(places) {
        match cfg_have.iter().find(|(_, c)| c.input == input.id) {
            Some((e, c)) if c.key == ckey => {
                place(world, *e, pos);
                kept.insert(*e);
            }
            _ => {
                spawn_input_node(
                    world,
                    element,
                    input,
                    r,
                    ckey.clone(),
                    pos,
                    canvas.world(),
                    &theme,
                );
                changed = true;
            }
        }
    }
    for (e, _) in &cfg_have {
        if !kept.contains(e) {
            world.entity_mut(*e).despawn();
            changed = true;
        }
    }
    // The Parts node.
    let pkey = (
        parts
            .iter()
            .map(|p| {
                (
                    p.id,
                    cadrs_core::parts::display_name(p, el.part_props()).to_string(),
                )
            })
            .collect::<Vec<_>>(),
        tk,
    );
    let mut q_parts = world.query::<(Entity, &PartsNode)>();
    let parts_have: Vec<(Entity, PartsNode)> =
        q_parts.iter(world).map(|(e, c)| (e, c.clone())).collect();
    match parts_have.first() {
        Some((e, c)) if c.key == pkey => place(world, *e, parts_pos),
        _ => {
            for (e, _) in &parts_have {
                world.entity_mut(*e).despawn();
            }
            spawn_parts_node(world, pkey, parts_pos, canvas.world(), &theme);
            changed = true;
        }
    }
    // The wires, again whenever a node or the list changed.
    // (And after a rebuild: a cut's part, which wires it, is known from it.)
    let doc_changed = world.resource_ref::<ActiveDocument>().is_changed()
        || world.resource_ref::<PartCache>().is_changed();
    if changed || doc_changed {
        rebuild_wires(world, &el, &parts, canvas_e, canvas.wires());
    }
}

/// Puts a node at its place, unless it is being dragged (its place is saved when the drag
/// ends).
fn place(world: &mut World, e: Entity, pos: Vec2) {
    if world
        .get::<cadrs_ui::node_graph::GraphNodeDragging>(e)
        .is_some()
    {
        return;
    }
    if let Some(mut g) = world.get_mut::<GraphNode>(e)
        && g.pos != pos
    {
        g.pos = pos;
    }
}

/// The canvas's Create menu (its anchor): what can be added to the graph.
#[derive(Component, Debug, Clone, Copy)]
struct CreateMenu;

/// Create ▾ opens a menu under it: Input Configuration.
fn open_create_menu(world: &mut World, button: Entity) {
    let at = world
        .get::<ComputedNode>(button)
        .zip(world.get::<UiGlobalTransform>(button))
        .map(|(n, g)| {
            let s = n.inverse_scale_factor();
            (g.translation - n.size() / 2.0 + Vec2::new(0.0, n.size().y + 2.0)) * s
        })
        .unwrap_or(Vec2::splat(200.0));
    let menu = Menu::new("node-create-menu")
        .min_width(180.0)
        .item_height(24.0)
        .item(
            MenuItem::new(
                "node-create-input-configuration",
                tr("node-view-input-configuration"),
            )
            .icon("list"),
        );
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = cadrs_ui::open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((CreateMenu, DespawnOnExit(crate::AppState::Document)));
    world.flush();
}

fn on_create_menu(
    ev: On<cadrs_ui::MenuAction>,
    q: Query<(), With<CreateMenu>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) && ev.item == "node-create-input-configuration" {
        commands.queue(add_input_configuration);
    }
}

/// The canvas's corner buttons: Create ▾ (top left); Auto arrange and Fit (top right).
fn spawn_canvas_tools(world: &mut World, canvas: Entity, t: &Theme) {
    world.spawn((
        Name::new("node-canvas-create-tools"),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(6.0),
            top: Val::Px(6.0),
            ..default()
        },
        ZIndex(10),
        ChildOf(canvas),
        children![(
            cadrs_ui::Button::new("node-canvas-create")
                .label(tr("node-view-create"))
                .icon("plus")
                .small()
                .build(t),
            cadrs_ui::Tooltip::new(tr("node-view-create-tooltip")),
            bevy::ui_widgets::observe(|a: On<Activate>, mut commands: Commands| {
                let button = a.entity;
                commands.queue(move |world: &mut World| open_create_menu(world, button));
            }),
        )],
    ));
    world.spawn((
        Name::new("node-canvas-tools"),
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(6.0),
            top: Val::Px(6.0),
            column_gap: Val::Px(4.0),
            ..default()
        },
        ZIndex(10),
        ChildOf(canvas),
        children![
            (
                IconButton::new("node-canvas-arrange", "auto-arrange")
                    .icon_size(16.0)
                    .tooltip(tr("node-view-arrange"))
                    .build(t),
                bevy::ui_widgets::observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |world: &mut World| arrange(world, canvas));
                }),
            ),
            (
                IconButton::new("node-canvas-fit", "zoom-fit")
                    .icon_size(16.0)
                    .tooltip(tr("node-view-fit"))
                    .build(t),
                bevy::ui_widgets::observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.trigger(FitCanvas { entity: canvas });
                }),
            ),
        ],
    ));
}

/// Auto arrange: forgets where the nodes were put (one undo step), so they are laid out again
/// ([`layout`]), and frames them.
fn arrange(world: &mut World, canvas: Entity) {
    let Some(element) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    run(world, &cfg::ResetNodeLayout { element });
    if let Ok(mut e) = world.get_entity_mut(canvas) {
        e.insert(FitLater(3));
    }
}

/// The Parts node: a row for each part, with an input socket.
fn spawn_parts_node(
    world: &mut World,
    key: (Vec<(cadrs_core::PartId, String)>, u64),
    pos: Vec2,
    parent: Entity,
    t: &Theme,
) {
    let parts = key.0.clone();
    let root = world
        .spawn((
            GraphNode::bundle("node-parts", pos),
            PartsNode { key },
            ChildOf(parent),
        ))
        .id();
    let tb = t.clone();
    let rows = parts.clone();
    let card = FeatureDialog::new("node-parts-card")
        .static_copy()
        .width(160.0)
        .title(tr("node-view-parts"))
        .header_color(Color::srgba_u8(0x4c, 0xb0, 0x5c, 0x40))
        .body(move |b| {
            let t = &tb;
            if rows.is_empty() {
                b.spawn((
                    t.text(
                        tr("node-view-no-parts"),
                        t.font_sm,
                        bevy::text::FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node {
                        margin: UiRect::all(Val::Px(4.0)),
                        ..default()
                    },
                ));
            }
            for (i, (_, name)) in rows.iter().enumerate() {
                b.spawn((
                    Name::new(format!("node-part-{i}")),
                    row(),
                    children![(
                        t.text(
                            name.clone(),
                            t.font_sm,
                            bevy::text::FontWeight::MEDIUM,
                            t.foreground
                        ),
                        Node {
                            margin: UiRect::left(Val::Px(8.0)),
                            ..default()
                        },
                    )],
                ));
            }
        });
    let card = world.spawn((card.build(t), ChildOf(root))).id();
    if let Some(h) = world.get::<Children>(card).and_then(|c| c.first().copied()) {
        world.entity_mut(h).insert(GraphNodeHandle);
    }
    let named = named_descendants(world, card);
    for (i, (id, _)) in parts.iter().enumerate() {
        let want = squash(&format!("node-part-{i}"));
        let Some(row) = named.iter().find(|(n, _)| *n == want).map(|(_, e)| *e) else {
            continue;
        };
        world.spawn((
            SocketKey::Part(*id),
            cadrs_ui::node_graph::ReadOnlySocket,
            Socket::bundle(
                format!("node-part-{i}-in"),
                SocketSide::In,
                GEOMETRY,
                Some(row),
                socket_color(GEOMETRY),
                t,
            ),
            ChildOf(root),
        ));
    }
    if let Some(mut n) = world.get_mut::<Node>(root) {
        n.width = Val::Px(160.0);
    }
}

/// Spawns a feature's node: its card (a copy of its dialog, or its fields as rows), a cover
/// over the card's body, and its sockets.
#[allow(clippy::too_many_arguments)]
fn spawn_feature_node(
    world: &mut World,
    el: &Element,
    f: &Feature,
    key: NodeKey,
    outputs: &[String],
    pos: Vec2,
    parent: Entity,
    t: &Theme,
    units: &Units,
) -> Entity {
    // Named after the feature, as its list row is ("node-extrude-1"), so a script can find it.
    let slug = crate::feature_list::row_name(&f.name).replacen("feature-", "", 1);
    let collapsed = key.collapsed;
    let dim = key.suppressed || key.rolled_back;
    let root = world
        .spawn((
            GraphNode::bundle(format!("node-{slug}"), pos),
            FeatureNode {
                feature: f.id,
                key: key.clone(),
            },
            ChildOf(parent),
        ))
        .id();
    let valid = key.error.is_none();
    // A sketch's node shows its curves, fitted into a square.
    let thumbnail = f.sketch().map(|sk| {
        // (An image isn't mapped to the dark scheme as the UI's colours are: its ink is
        // chosen for the scheme in force.)
        let ink = if cadrs_ui::dark_in_force() {
            Color::srgb_u8(0xe6, 0xe6, 0xe6)
        } else {
            Color::srgb_u8(0x26, 0x26, 0x26)
        };
        let img = sketch_thumbnail(&sk.geometry, ink);
        world.resource_mut::<Assets<Image>>().add(img)
    });
    let settings = *world.resource::<crate::sketch::SketchViewSettings>();
    let card = {
        let cache = world.resource::<PartCache>();
        crate::sketch::static_card(t, el, cache, f, settings, thumbnail)
            .or_else(|| crate::extrude_dialog::static_card(t, el, cache, f))
            .or_else(|| crate::revolve_dialog::static_card(t, el, cache, f))
            .or_else(|| crate::applied_dialog::static_card(t, el, cache, f))
    }
    .unwrap_or_else(|| generic_card(f, units, t));
    let card = card
        .static_copy()
        // One width for every node (the dialogs differ by a few px), as wide as its output row.
        .width(NODE_W)
        .valid(valid)
        .icon(crate::document::feature_icon(&f.kind))
        .kind(cadrs_core::feature_list::type_label(&f.kind))
        .title(&f.name)
        .header_color(if dim {
            Color::srgba_u8(0x80, 0x80, 0x80, 0x30)
        } else {
            category_color(f)
        });
    let card = world
        .spawn((StaticCopyRoot, StaticCard, card.build(t), ChildOf(root)))
        .id();
    // Marked now, before the live dialogs' systems run this frame (they leave it alone).
    for e in descendants(world, card) {
        world
            .entity_mut(e)
            .insert(cadrs_ui::node_graph::StaticCopy)
            .remove::<bevy::input_focus::tab_navigation::TabIndex>();
    }
    // The card's header (its first child) drags the node and holds the collapse chevron.
    let header = world.get::<Children>(card).and_then(|c| c.first().copied());
    if let Some(h) = header {
        world.entity_mut(h).insert(GraphNodeHandle);
        let chevron = world
            .spawn((
                CollapseButton(f.id),
                IconButton::new(
                    format!("node-{slug}-collapse"),
                    if collapsed {
                        "chevron-right"
                    } else {
                        "chevron-down"
                    },
                )
                .icon_size(12.0)
                .tooltip(tr(if collapsed {
                    "node-view-expand"
                } else {
                    "node-view-collapse"
                }))
                .build(t),
            ))
            .id();
        world.entity_mut(h).insert_children(0, &[chevron]);
    }
    if collapsed {
        let kids: Vec<Entity> = world
            .get::<Children>(card)
            .map(|c| c.iter().skip(1).collect())
            .unwrap_or_default();
        for k in kids {
            if let Some(mut n) = world.get_mut::<Node>(k) {
                n.display = Display::None;
            }
        }
    }
    // A sketch's driven dimensions, each an input of its own, and the input that drives another.
    let dimension_rows = if f.sketch().is_some() && !collapsed {
        spawn_dimension_rows(world, f, &slug, root, t, units)
    } else {
        Vec::new()
    };
    // The cover: below the header, over the rest.
    world.spawn((
        Name::new(format!("node-{slug}-body")),
        BodyCover(f.id),
        FeatureRow(f.id),
        PickRow(Pick::Feature(f.id)),
        bevy::picking::hover::Hovered::default(),
        cadrs_ui::ContextMenuTarget,
        cadrs_ui::DoubleClickable,
        GraphNodeHandle,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(30.0),
            bottom: Val::Px(24.0),
            ..default()
        },
        ZIndex(4),
        Pickable::default(),
        ChildOf(root),
    ));
    // The output row: what the feature makes, its geometry socket beside it.
    let what = if outputs.is_empty() {
        match f.kind {
            FeatureKind::Sketch(_) => tr("node-view-output-sketch"),
            FeatureKind::Plane(_) => tr("node-view-output-plane"),
            _ => tr("node-view-output"),
        }
    } else {
        outputs.join(", ")
    };
    let output = world
        .spawn((
            Name::new(format!("node-{slug}-output")),
            Node {
                height: Val::Px(24.0),
                margin: UiRect::top(Val::Px(-1.0)),
                padding: UiRect::new(Val::Px(8.0), Val::Px(12.0), Val::ZERO, Val::ZERO),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexEnd,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::bottom(Val::Px(t.radius_panel)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(t.floating),
            BorderColor::all(t.glass_rim),
            ChildOf(root),
        ))
        .id();
    world
        .spawn((
            t.text(
                what,
                t.font_sm,
                bevy::text::FontWeight::MEDIUM,
                t.muted_foreground,
            ),
            ChildOf(output),
        ))
        // One line, clipped when long (insert replaces the text's own layout).
        .insert(TextLayout::no_wrap());
    // Sockets. Geometry in: level with the dialog's first selection field (a sketch's plane,
    // an extrude's regions, a fillet's edges), coloured as what it references (a plane grey, a
    // sketch blue, a part green); on the header when the node has none (or is collapsed).
    let parents: Vec<FeatureId> = cadrs_core::feature_list::direct_parents(el.features(), f.id)
        .into_iter()
        .filter(|p| f.reference_parents().contains(p) || f.merge_scope().is_none())
        .collect();
    // (A sketch on a default plane, Top, Front or Right, takes a plane too.)
    let in_color = match parents.first().and_then(|p| el.feature(*p)) {
        Some(p) => geometry_color(p),
        None if matches!(f.kind, FeatureKind::Sketch(_) | FeatureKind::Plane(_)) => PLANE_GREY,
        None => socket_color(GEOMETRY),
    };
    let field_row = (!collapsed)
        .then(|| {
            descendants(world, card).into_iter().find(|e| {
                world.get::<cadrs_ui::SelectionFieldState>(*e).is_some()
                    || world.get::<cadrs_ui::SelectionListState>(*e).is_some()
            })
        })
        .flatten();
    let geo = world
        .spawn((
            SocketKey::GeoIn(f.id),
            cadrs_ui::node_graph::ReadOnlySocket,
            Socket::bundle(
                format!("node-{slug}-geometry-in"),
                SocketSide::In,
                GEOMETRY,
                field_row,
                in_color,
                t,
            ),
            ChildOf(root),
        ))
        .id();
    if field_row.is_none()
        && let Some(mut n) = world.get_mut::<Node>(geo)
    {
        n.top = Val::Px(4.0);
    }
    // Merge scope in: level with the dialog's Merge scope row (the parts it adds to, cuts or
    // merges with), green as parts are; on the header when the node is collapsed.
    if f.merge_scope().is_some() {
        let scope_row = (!collapsed)
            .then(|| {
                descendants(world, card).into_iter().find(|e| {
                    world
                        .get::<Name>(*e)
                        .is_some_and(|n| n.as_str().ends_with("-merge-scope-field"))
                })
            })
            .flatten();
        let scope = world
            .spawn((
                SocketKey::Scope(f.id),
                cadrs_ui::node_graph::ReadOnlySocket,
                Socket::bundle(
                    format!("node-{slug}-merge-scope-in"),
                    SocketSide::In,
                    GEOMETRY,
                    scope_row,
                    socket_color(GEOMETRY),
                    t,
                ),
                ChildOf(root),
            ))
            .id();
        if scope_row.is_none()
            && let Some(mut n) = world.get_mut::<Node>(scope)
        {
            n.top = Val::Px(4.0);
        }
    }
    world.spawn((
        SocketKey::GeoOut(f.id),
        cadrs_ui::node_graph::ReadOnlySocket,
        Socket::bundle(
            format!("node-{slug}-geometry-out"),
            SocketSide::Out,
            GEOMETRY,
            Some(output),
            geometry_color(f),
            t,
        ),
        ChildOf(root),
    ));
    // Enabled (suppression by a switch): only where it can be wired, a configuration with a
    // Checkbox or a Boolean column, or where it already is.
    let switches = el
        .configuration()
        .is_some_and(|c| c.names().iter().any(|n| config_type(el, n) == BOOLEAN));
    if switches || f.suppress_by.is_some() {
        let bypass = world
            .spawn((
                SocketKey::Bypass(f.id),
                Socket::bundle(
                    format!("node-{slug}-bypass"),
                    SocketSide::In,
                    BOOLEAN,
                    None,
                    socket_color(BOOLEAN),
                    t,
                ),
                cadrs_ui::Tooltip::new(tr("node-view-enabled")),
                ChildOf(root),
            ))
            .id();
        if let Some(mut n) = world.get_mut::<Node>(bypass) {
            n.top = Val::Px(10.0);
        }
    }
    if let Some(mut n) = world.get_mut::<Node>(root) {
        n.width = Val::Px(NODE_W);
    }
    if collapsed {
        return root;
    }
    // A socket for each field that takes an expression, level with its row.
    let rows = named_descendants(world, card);
    for (label, q, _) in cfg::parameter_slots(f) {
        let want = squash(label);
        let row = rows
            .iter()
            .filter(|(n, _)| n.ends_with(&want))
            .min_by_key(|(n, _)| n.len())
            .map(|(_, e)| *e);
        let Some(row) = row else { continue };
        let ty = quantity_type(q);
        world.spawn((
            SocketKey::Slot(f.id, label),
            Socket::bundle(
                format!("node-{slug}-{want}"),
                SocketSide::In,
                ty,
                Some(row),
                socket_color(ty),
                t,
            ),
            ChildOf(root),
        ));
    }
    // The driven dimensions' inputs, and "Drive a dimension".
    for (key, row, ty) in dimension_rows {
        let color = if ty == cadrs_ui::node_graph::ANY_TYPE {
            t.muted_foreground
        } else {
            socket_color(ty)
        };
        let name = match &key {
            SocketKey::Dimension(_, d) => format!("node-{slug}-dim-{}", dim_index(f, *d)),
            _ => format!("node-{slug}-dim-add"),
        };
        world.spawn((
            key,
            Socket::bundle(name, SocketSide::In, ty, Some(row), color, t),
            ChildOf(root),
        ));
    }
    // A Variable's value, out.
    if let FeatureKind::Variable(v) = &f.kind
        && let Some(row) = rows
            .iter()
            .find(|(n, _)| n.ends_with("value"))
            .map(|(_, e)| *e)
    {
        let ty = quantity_type(v.kind.quantity());
        world.spawn((
            SocketKey::Variable(f.id),
            Socket::bundle(
                format!("node-{slug}-out"),
                SocketSide::Out,
                ty,
                Some(row),
                socket_color(ty),
                t,
            ),
            cadrs_ui::node_graph::DragLabel(format!("#{}", v.name)),
            ChildOf(root),
        ));
    }
    root
}

/// A dimension's place among its sketch's driving ones (1 for `D1`).
fn dim_index(f: &Feature, d: cadrs_sketch::DimensionId) -> usize {
    cfg::dimension_slots(f)
        .iter()
        .position(|s| s.id == d)
        .map_or(0, |k| k + 1)
}

/// A sketch node's "Driven dimensions": a row for each dimension an expression drives
/// (`D3 · 1.5 mm   #input1`), and **Drive a dimension**, whose input takes a wire of any
/// value and asks which dimension. Returns each row's socket (key, row, socket type), for
/// [`spawn_feature_node`] to spawn once the card's other sockets are placed.
fn spawn_dimension_rows(
    world: &mut World,
    f: &Feature,
    slug: &str,
    root: Entity,
    t: &Theme,
    units: &Units,
) -> Vec<(SocketKey, Entity, u32)> {
    let slots = cfg::dimension_slots(f);
    if slots.is_empty() {
        return Vec::new();
    }
    let section = world
        .spawn((
            Name::new(format!("node-{slug}-dimensions")),
            Node {
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(Val::Px(-1.0)),
                padding: UiRect::vertical(Val::Px(3.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(t.floating),
            BorderColor::all(t.glass_rim),
            ChildOf(root),
        ))
        .id();
    let row = |world: &mut World, name: String, left: String, right: String, muted: bool| {
        let r = world
            .spawn((
                Name::new(name),
                Node {
                    height: Val::Px(22.0),
                    padding: UiRect::horizontal(Val::Px(14.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: Val::Px(8.0),
                    ..default()
                },
                ChildOf(section),
            ))
            .id();
        let fg = if muted {
            t.muted_foreground
        } else {
            t.foreground
        };
        world
            .spawn((
                t.text(left, t.font_sm, bevy::text::FontWeight::NORMAL, fg),
                ChildOf(r),
            ))
            .insert(TextLayout::no_wrap());
        world
            .spawn((
                t.text(
                    right,
                    t.font_sm,
                    bevy::text::FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                ChildOf(r),
            ))
            .insert(TextLayout::no_wrap());
        r
    };
    let mut out = Vec::new();
    for (k, s) in slots.iter().enumerate() {
        if !s.is_driven() {
            continue;
        }
        let r = row(
            world,
            format!("node-{slug}-dim-{}-row", k + 1),
            format!("{} · {}", s.label, units.with_unit(s.value, s.quantity)),
            s.expr.clone().unwrap_or_default(),
            false,
        );
        out.push((
            SocketKey::Dimension(f.id, s.id),
            r,
            quantity_type(s.quantity),
        ));
    }
    if slots.iter().any(|s| !s.is_driven()) {
        let r = row(
            world,
            format!("node-{slug}-dim-add-row"),
            tr("node-view-drive-dimension"),
            String::new(),
            true,
        );
        out.push((
            SocketKey::DriveDimension(f.id),
            r,
            cadrs_ui::node_graph::ANY_TYPE,
        ));
    }
    out
}

/// `root` and every entity under it.
fn descendants(world: &mut World, root: Entity) -> Vec<Entity> {
    let mut q_children = world.query::<&Children>();
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        if let Ok(c) = q_children.get(world, all[i]) {
            all.extend(c.iter());
        }
        i += 1;
    }
    all
}

/// A square picture of a sketch's curves (construction curves left out), fitted with a margin:
/// the sketch node's thumbnail. Lines a pixel and a half wide, smoothed, in `color` on clear.
fn sketch_thumbnail(sk: &cadrs_sketch::Sketch, color: Color) -> Image {
    const SIZE: u32 = 128;
    let lines: Vec<Vec<cadrs_sketch::Vec2>> = sk
        .curves
        .iter()
        .filter(|(_, c)| !c.construction)
        .map(|(id, _)| cadrs_sketch::hit::curve_polyline(sk, id))
        .filter(|p| p.len() > 1)
        .collect();
    let mut px = vec![0.0f32; (SIZE * SIZE) as usize];
    let pts = lines.iter().flatten();
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for p in pts {
        let q = Vec2::new(p.x as f32, p.y as f32);
        lo = lo.min(q);
        hi = hi.max(q);
    }
    if lo.x <= hi.x {
        let span = (hi - lo).max_element().max(1e-6);
        let scale = (SIZE as f32 - 12.0) / span;
        let mid = (lo + hi) / 2.0;
        // Sketch y up, picture y down; centred.
        let to = |p: &cadrs_sketch::Vec2| {
            let q = (Vec2::new(p.x as f32, p.y as f32) - mid) * scale;
            Vec2::new(SIZE as f32 / 2.0 + q.x, SIZE as f32 / 2.0 - q.y)
        };
        for l in &lines {
            for w in l.windows(2) {
                let (a, b) = (to(&w[0]), to(&w[1]));
                let n = (a.distance(b) * 2.0).ceil().max(1.0) as usize;
                for i in 0..=n {
                    let c = a.lerp(b, i as f32 / n as f32);
                    // A soft dot, 1.5 px across.
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (x, y) = (c.x.floor() as i32 + dx, c.y.floor() as i32 + dy);
                            if x < 0 || y < 0 || x >= SIZE as i32 || y >= SIZE as i32 {
                                continue;
                            }
                            let d = Vec2::new(x as f32 + 0.5, y as f32 + 0.5).distance(c);
                            let a = (1.25 - d).clamp(0.0, 1.0);
                            let k = (y as u32 * SIZE + x as u32) as usize;
                            px[k] = px[k].max(a);
                        }
                    }
                }
            }
        }
    }
    let c = color.to_srgba();
    let mut data = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for a in px {
        // Premultiplied isn't needed: UI images blend straight alpha.
        data.extend_from_slice(&[
            (c.red * 255.0) as u8,
            (c.green * 255.0) as u8,
            (c.blue * 255.0) as u8,
            (a * 255.0) as u8,
        ]);
    }
    Image::new(
        bevy::render::render_resource::Extent3d {
            width: SIZE,
            height: SIZE,
            depth_or_array_layers: 1,
        },
        bevy::render::render_resource::TextureDimension::D2,
        data,
        bevy::render::render_resource::TextureFormat::Rgba8UnormSrgb,
        bevy::asset::RenderAssetUsages::default(),
    )
}

/// The entities under `root` with their names squashed ([`squash`]).
fn named_descendants(world: &mut World, root: Entity) -> Vec<(String, Entity)> {
    let mut q_children = world.query::<&Children>();
    let mut all = vec![root];
    let mut i = 0;
    while i < all.len() {
        if let Ok(c) = q_children.get(world, all[i]) {
            all.extend(c.iter());
        }
        i += 1;
    }
    all.into_iter()
        .filter_map(|e| world.get::<Name>(e).map(|n| (squash(n.as_str()), e)))
        .collect()
}

/// The card of a feature whose dialog isn't copied yet: its fields as rows (each named after
/// its label, so its socket finds it), or what it is.
fn generic_card(f: &Feature, units: &Units, t: &Theme) -> FeatureDialog {
    let slots = cfg::parameter_slots(f);
    let tb = t.clone();
    let what = cadrs_core::feature_list::type_label(&f.kind).to_string();
    let variable = match &f.kind {
        FeatureKind::Variable(v) => Some((v.name.clone(), v.expr.clone(), v.display(units))),
        _ => None,
    };
    let slug = crate::feature_list::row_name(&f.name).replacen("feature-", "", 1);
    FeatureDialog::new(format!("node-{slug}-card")).body(move |b| {
        let t = &tb;
        if let Some((name, expr, value)) = variable {
            b.spawn(
                NumberField::new("node-variable-value", format!("#{name}"))
                    .text(expr)
                    .label_width(90.0)
                    .build(t),
            );
            b.spawn((
                t.text(
                    value,
                    t.font_sm,
                    bevy::text::FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Node {
                    margin: UiRect::left(Val::Px(94.0)),
                    ..default()
                },
            ));
            return;
        }
        if slots.is_empty() {
            b.spawn((
                t.text(
                    what,
                    t.font_sm,
                    bevy::text::FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Node {
                    margin: UiRect::all(Val::Px(4.0)),
                    ..default()
                },
            ));
        }
        for (label, _, expr) in &slots {
            b.spawn(
                NumberField::new(format!("node-slot-{}", squash(label)), label.to_string())
                    .text(expr.clone())
                    .label_width(110.0)
                    .build(t),
            );
        }
    })
}

/// The wires between the shown nodes ([`graph_wires`]).
fn rebuild_wires(
    world: &mut World,
    el: &Element,
    parts: &[cadrs_core::Part],
    canvas: Entity,
    layer: Entity,
) {
    let mut q_sock = world.query::<(Entity, &SocketKey)>();
    let sockets: HashMap<SocketKey, Entity> = q_sock
        .iter(world)
        .filter(|(e, _)| on_canvas(world, *e, canvas))
        .map(|(e, k)| (k.clone(), e))
        .collect();
    let uses = world.resource::<PartCache>().uses.clone();
    let wanted: Vec<Wire> = graph_wires(el, parts, &uses)
        .into_iter()
        .filter_map(|(a, b, color, dashed)| {
            Some(Wire {
                from: *sockets.get(&a)?,
                to: *sockets.get(&b)?,
                color,
                dashed,
            })
        })
        .collect();
    update_wires(world, layer, wanted);
}

/// A Part Studio's node graph's wires, from the model: geometry from each feature's references
/// and the parts it changes (the parents no other parent depends on, as the last rebuild knows
/// them: `parts` and `uses`), values from the expressions that name an input or
/// a Variable (dashed when the expression is more than the name), Bypass from suppression
/// variables, and each part from the last feature that changes it into the Parts node.
fn graph_wires(
    el: &Element,
    parts: &[cadrs_core::Part],
    uses: &HashMap<FeatureId, Vec<FeatureId>>,
) -> Vec<(SocketKey, SocketKey, Color, bool)> {
    let features = el.features();
    let mut wires: Vec<(SocketKey, SocketKey, Color, bool)> = Vec::new();
    // Where a name comes from at a feature: the last Variable above it of that name, else the
    // configuration's input.
    let source = |name: &str, at: usize| -> Option<SocketKey> {
        if let Some(g) = features[..at]
            .iter()
            .rev()
            .find(|g| matches!(&g.kind, FeatureKind::Variable(v) if v.name == name))
        {
            return Some(SocketKey::Variable(g.id));
        }
        el.configuration()?
            .names()
            .contains(&name.to_string())
            .then(|| SocketKey::Input(name.to_string()))
    };
    for (i, f) in features.iter().enumerate() {
        // Only the parents no other parent depends on: an edge between Extrude 5's cut and
        // Extrude 1's plate needs both, but Extrude 5 cut Extrude 1's part already.
        // What it builds from into its geometry input; the parts it adds to, cuts or merges
        // with (picked, or found by the rebuild) into its merge scope's.
        let refs = f.reference_parents();
        let scope: Vec<FeatureId> = f
            .merge_scope()
            .map(|s| s.iter().map(|p| p.feature).collect())
            .unwrap_or_default();
        for p in cadrs_core::feature_list::direct_parents_with(features, parts, uses, f.id) {
            let Some(pf) = el.feature(p) else { continue };
            let to_scope = f.merge_scope().is_some() && (scope.contains(&p) || !refs.contains(&p));
            if refs.contains(&p) || !to_scope {
                wires.push((
                    SocketKey::GeoOut(p),
                    SocketKey::GeoIn(f.id),
                    geometry_color(pf),
                    false,
                ));
            }
            if to_scope {
                wires.push((
                    SocketKey::GeoOut(p),
                    SocketKey::Scope(f.id),
                    geometry_color(pf),
                    false,
                ));
            }
        }
        for (label, q, expr) in cfg::parameter_slots(f) {
            let names = cadrs_sketch::units::variable_names(&expr);
            let exact = names.len() == 1 && expr.trim() == format!("#{}", names[0]);
            for n in names {
                if let Some(src) = source(&n, i) {
                    wires.push((
                        src,
                        SocketKey::Slot(f.id, label),
                        socket_color(quantity_type(q)),
                        !exact,
                    ));
                }
            }
        }
        // A sketch's dimensions driven by an input or a Variable.
        for d in cfg::dimension_slots(f) {
            let Some(expr) = d.expr.as_deref().filter(|_| d.is_driven()) else {
                continue;
            };
            let names = cadrs_sketch::units::variable_names(expr);
            let exact = names.len() == 1 && expr.trim() == format!("#{}", names[0]);
            for n in names {
                if let Some(src) = source(&n, i) {
                    wires.push((
                        src,
                        SocketKey::Dimension(f.id, d.id),
                        socket_color(quantity_type(d.quantity)),
                        !exact,
                    ));
                }
            }
        }
        if let Some(rule) = &f.suppress_by {
            let names = cadrs_sketch::units::variable_names(&rule.expr);
            let exact =
                !rule.invert && names.len() == 1 && rule.expr.trim() == format!("#{}", names[0]);
            for n in names {
                if let Some(src) = source(&n, i) {
                    wires.push((src, SocketKey::Bypass(f.id), socket_color(BOOLEAN), !exact));
                }
            }
        }
    }
    // Each part from the last feature that changes it into the Parts node.
    for p in parts {
        if let Some(f) = last_feature_of(features, p) {
            wires.push((
                SocketKey::GeoOut(f),
                SocketKey::Part(p.id),
                socket_color(GEOMETRY),
                false,
            ));
        }
    }
    wires
}

/// A Part Studio's node graph as data (the MCP's `get_node_graph`): its nodes (each feature's,
/// the Configuration node, the Parts node) with their sockets and places, and the wires between
/// them, as [`graph_wires`] draws them. `parts`: the studio's built parts (the Parts node), when
/// known. `ui_name` is the node's UI name while the node view shows it (for `get_ui_layout` and
/// scenario steps).
pub fn node_graph_json(
    el: &Element,
    parts: &[cadrs_core::Part],
    uses: &HashMap<FeatureId, Vec<FeatureId>>,
) -> serde_json::Value {
    use serde_json::json;
    let features = el.features();
    let layout = el.node_layout();
    let name_of = |id: FeatureId| el.feature(id).map(|f| f.name.clone());
    let pos = |id: FeatureId| layout.and_then(|l| l.positions.get(&id)).copied();
    let mut nodes = Vec::new();
    if let Some(c) = el.configuration() {
        for input in &c.inputs {
            let mut outputs: Vec<String> = Vec::new();
            if input.defines_variable() {
                outputs.push(input.name.clone());
            }
            outputs.extend(
                input
                    .params
                    .iter()
                    .filter(|p| p.binds())
                    .map(|p| p.name.clone()),
            );
            nodes.push(json!({
                "node": input.name,
                "type": input.kind.label(),
                "ui_name": format!("node-config-{}", input.name),
                "position": layout.and_then(|l| l.inputs.get(&input.id).copied()),
                "configurations": input.row_names(),
                "outputs": outputs,
            }));
        }
    }
    for f in features {
        let slug = crate::feature_list::row_name(&f.name).replacen("feature-", "", 1);
        let mut inputs = vec![json!("geometry")];
        if f.merge_scope().is_some() {
            inputs.push(json!("merge scope"));
        }
        inputs.extend(
            cfg::parameter_slots(f)
                .into_iter()
                .map(|(label, _, expr)| json!({ "field": label, "expression": expr })),
        );
        inputs.extend(
            cfg::dimension_slots(f)
                .into_iter()
                .filter(|d| d.is_driven())
                .map(|d| json!({ "dimension": d.label, "expression": d.expr, "value": d.value })),
        );
        nodes.push(json!({
            "node": f.name,
            "type": cadrs_core::feature_list::type_label(&f.kind),
            "ui_name": format!("node-{slug}"),
            "position": pos(f.id),
            "suppressed": el.is_suppressed(f.id),
            "collapsed": layout.is_some_and(|l| l.collapsed.contains(&f.id)),
            "inputs": inputs,
            "bypass": f.suppress_by.as_ref().map(|r| r.expr.clone()),
        }));
    }
    nodes.push(json!({
        "node": "Parts",
        "ui_name": "node-parts",
        "inputs": parts.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
    }));
    // A socket as (node, socket).
    let end = |k: &SocketKey| -> serde_json::Value {
        match k {
            SocketKey::GeoOut(f) => json!({ "node": name_of(*f), "socket": "geometry out" }),
            SocketKey::GeoIn(f) => json!({ "node": name_of(*f), "socket": "geometry in" }),
            SocketKey::Scope(f) => json!({ "node": name_of(*f), "socket": "merge scope" }),
            SocketKey::Dimension(f, d) => json!({
                "node": name_of(*f),
                "socket": el
                    .feature(*f)
                    .and_then(|g| cfg::dimension_slots(g).into_iter().find(|s| s.id == *d))
                    .map(|s| s.label),
            }),
            SocketKey::DriveDimension(f) => {
                json!({ "node": name_of(*f), "socket": "drive a dimension" })
            }
            SocketKey::Slot(f, label) => json!({ "node": name_of(*f), "socket": label }),
            SocketKey::Bypass(f) => json!({ "node": name_of(*f), "socket": "bypass" }),
            SocketKey::Input(n) => json!({
                "node": el
                    .configuration
                    .inputs
                    .iter()
                    .find(|i| i.name == *n || i.params.iter().any(|p| p.name == *n))
                    .map(|i| i.name.clone()),
                "socket": n,
            }),
            SocketKey::Variable(f) => json!({ "node": name_of(*f), "socket": "value" }),
            SocketKey::Part(p) => json!({
                "node": "Parts",
                "socket": parts.iter().find(|x| x.id == *p).map(|x| x.name.clone()),
            }),
        }
    };
    let kind = |a: &SocketKey, b: &SocketKey| match (a, b) {
        (_, SocketKey::Bypass(_)) => "bypass",
        (SocketKey::GeoOut(_), _) => "geometry",
        _ => "value",
    };
    let wires: Vec<serde_json::Value> = graph_wires(el, parts, uses)
        .iter()
        .map(|(a, b, _, dashed)| {
            json!({
                "from": end(a),
                "to": end(b),
                "kind": kind(a, b),
                // A value wire whose field's expression is more than the name (`#length/2`).
                "expression": dashed,
            })
        })
        .collect();
    json!({ "nodes": nodes, "wires": wires })
}

/// Makes the wires under `layer` the `wanted` ones: a wire already there is kept (with its
/// route, so a document change that leaves it as it was neither redraws nor reroutes it), the
/// others are spawned, and the ones no longer wanted go.
fn update_wires(world: &mut World, layer: Entity, mut wanted: Vec<Wire>) {
    let mut q_wires = world.query::<(Entity, &ChildOf, &Wire)>();
    let old: Vec<(Entity, Wire)> = q_wires
        .iter(world)
        .filter(|(_, p, _)| p.parent() == layer)
        .map(|(e, _, w)| (e, *w))
        .collect();
    for (e, w) in old {
        if let Some(i) = wanted.iter().position(|x| *x == w) {
            wanted.swap_remove(i);
        } else {
            world.entity_mut(e).despawn();
        }
    }
    for w in wanted {
        let name = format!("node-wire-{}-{}", w.from.index(), w.to.index());
        world.spawn((Wire::bundle(name, w), ChildOf(layer)));
    }
}

/// Whether `e` is on the canvas `canvas`.
fn on_canvas(world: &World, e: Entity, canvas: Entity) -> bool {
    let mut cur = e;
    for _ in 0..64 {
        if cur == canvas {
            return true;
        }
        match world.get::<ChildOf>(cur) {
            Some(p) => cur = p.parent(),
            None => return false,
        }
    }
    false
}

// ---------------------------------------------------------------------------------------------
// The Configuration node

/// The quantity of `#name` when the configuration defines it: a Value's, a Checkbox's (a
/// number, 1 or 0), a column's.
fn config_quantity(el: &Element, name: &str) -> Option<Quantity> {
    let c = el.configuration()?;
    if let Some(i) = c.input_named(name).filter(|i| i.defines_variable()) {
        return Some(match &i.kind {
            InputKind::Value { kind, .. } => kind.quantity(),
            _ => Quantity::Count,
        });
    }
    c.column_named(name)
        .map(|(_, p)| p.kind(el.features()).quantity())
}

/// The socket type of `#name` (a Checkbox or a Boolean column: a switch).
fn config_type(el: &Element, name: &str) -> u32 {
    let Some(c) = el.configuration() else {
        return NUMBER;
    };
    let boolean = c
        .input_named(name)
        .is_some_and(|i| matches!(i.kind, InputKind::Checkbox { .. }))
        || c.column_named(name)
            .is_some_and(|(_, p)| p.kind(el.features()) == cfg::ColumnKind::Boolean);
    if boolean {
        BOOLEAN
    } else {
        config_quantity(el, name).map_or(NUMBER, quantity_type)
    }
}

/// An input node's width: its table's (at least a feature node's).
fn input_node_width(input: &ConfigInput) -> f32 {
    // (Ten configurations in view at most: the others are scrolled to.)
    (12.0 + config_table::table_width_showing(input, 10)).max(NODE_W)
}

/// A guess of an input node's height before it is laid out (its heading and rows, + Output).
fn input_node_height(input: &ConfigInput) -> f32 {
    let rows = input.params.len() + usize::from(input.defines_variable());
    30.0 + 12.0 + 28.0 * (rows + 1) as f32 + 34.0
}

/// An input node (Blender's Group Input, one per input): a table whose rows are the input's
/// outputs (each a `#name` with a socket on the right) and whose columns are its
/// configurations, the one shown highlighted. A List's is an Input Configuration: [+] adds a
/// configuration, + Output a row.
#[allow(clippy::too_many_arguments)]
fn spawn_input_node(
    world: &mut World,
    element: ElementId,
    input: &ConfigInput,
    resolved: cfg::Resolved,
    key: ConfigKey,
    pos: Vec2,
    parent: Entity,
    t: &Theme,
) {
    let units = key.1;
    let problem = key.3;
    let el = world
        .resource::<ActiveDocument>()
        .doc
        .element(element)
        .cloned();
    let features: Vec<Feature> = el
        .as_ref()
        .map(|e| e.features().to_vec())
        .unwrap_or_default();
    let id = input.id;
    let slug = input.name.clone();
    let width = input_node_width(input);
    let root = world
        .spawn((
            GraphNode::bundle(format!("node-config-{slug}"), pos),
            ConfigNode { input: id, key },
            ConfigElement(element),
            ChildOf(parent),
        ))
        .id();
    let (kind, icon) = match input.kind {
        InputKind::List { .. } => ("node-view-input-configuration", "list"),
        InputKind::Checkbox { .. } => ("node-view-input-checkbox", "check"),
        InputKind::Value { .. } => ("node-view-input-value", "variables"),
    };
    let tb = t.clone();
    let body_input = input.clone();
    let body_features = features.clone();
    let card_name = format!("node-config-{slug}-card");
    let card = FeatureDialog::new(card_name.clone())
        .static_copy()
        .width(width)
        .icon(icon)
        .kind(tr(kind))
        .title(input.name.clone())
        .header_color(config_color())
        .body(move |b| {
            config_table::spawn_table(
                b,
                &tb,
                &config_table::TableSpec {
                    prefix: "node",
                    input: &body_input,
                    resolved,
                    features: &body_features,
                    units,
                    problem,
                    link: config_table::ColumnsLink {
                        element,
                        input: id,
                        panel: false,
                    },
                },
            );
        });
    let card = world
        .spawn((card.build(t), ConfigElement(element), ChildOf(root)))
        .id();
    if let Some(h) = world.get::<Children>(card).and_then(|c| c.first().copied()) {
        world.entity_mut(h).insert(GraphNodeHandle);
    }
    // The title: double-clicked, renamed.
    let named = named_descendants(world, card);
    let find = |want: &str| {
        let want = squash(want);
        named.iter().find(|(n, _)| *n == want).map(|(_, e)| *e)
    };
    if let Some(title) = find(&format!("{card_name}-title")) {
        world.entity_mut(title).insert((
            cadrs_ui::InlineEditLabel,
            DoubleClickable,
            Pickable::default(),
        ));
        if let Some(clip) = world.get::<ChildOf>(title).map(|c| c.parent()) {
            world
                .entity_mut(clip)
                .insert((ConfigRole::Title(id), cadrs_ui::InlineEdit::default()));
        }
    }
    // An output socket level with each output's row.
    let mut outs: Vec<(String, Entity)> = Vec::new();
    if input.defines_variable()
        && let Some(row) = find(&format!("node-output-{}", input.name))
    {
        outs.push((input.name.clone(), row));
    }
    for p in input.params.iter().filter(|p| p.binds()) {
        if let Some(row) = find(&format!("node-output-{}", p.name)) {
            outs.push((p.name.clone(), row));
        }
    }
    for (name, row) in outs {
        let ty = el.as_ref().map_or(NUMBER, |e| config_type(e, &name));
        world.spawn((
            SocketKey::Input(name.clone()),
            Socket::bundle(
                format!("node-output-{name}-out"),
                SocketSide::Out,
                ty,
                Some(row),
                socket_color(ty),
                t,
            ),
            cadrs_ui::node_graph::DragLabel(format!("#{name}")),
            ChildOf(root),
        ));
    }
    if let Some(mut n) = world.get_mut::<Node>(root) {
        n.width = Val::Px(width);
    }
}

/// A name for a new input: `base`, then `base2`, `base3`… (not taken in the configuration nor
/// as a variable).
/// A row of a node's controls.
fn row() -> Node {
    Node {
        height: Val::Px(28.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        flex_shrink: 0.0,
        ..default()
    }
}

fn new_input_name(c: &Configuration, taken: &[String], base: &str) -> String {
    std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base}{n}")))
        .find(|n| c.name_problem(n, None, taken).is_none())
        .unwrap_or_default()
}

/// Adds an Input Configuration (a List with one configuration) to the active Part Studio, its
/// node's title open for a name.
fn add_input_configuration(world: &mut World) {
    let Some(element) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    let mut made = None;
    edit_config(world, element, "Add input configuration", |c, taken| {
        let name = new_input_name(c, taken, "configuration");
        let input = ConfigInput::list(&name, &["default"]);
        made = Some(input.id);
        c.inputs.push(input);
        true
    });
    if let Some(id) = made {
        world.insert_resource(PendingTitle(id));
        // Framed once laid out, so the new node is in view (it goes left of the features).
        let mut q = world.query::<(Entity, &NodeViewCanvas)>();
        let canvases: Vec<Entity> = q
            .iter(world)
            .filter(|(_, c)| c.0 == element)
            .map(|(e, _)| e)
            .collect();
        for c in canvases {
            world.entity_mut(c).insert(FitLater(8));
        }
    }
}

/// An input node's title to open for a name once the node is built (one just added).
#[derive(Resource, Debug, Clone, Copy)]
struct PendingTitle(ConfigInputId);

/// Opens the pending title for a name, once its node is there.
fn open_pending_title(world: &mut World) {
    let Some(PendingTitle(id)) = world.get_resource::<PendingTitle>().copied() else {
        return;
    };
    let mut q = world.query::<(Entity, &ConfigRole)>();
    let Some(e) = q
        .iter(world)
        .find(|(_, r)| matches!(r, ConfigRole::Title(i) if *i == id))
        .map(|(e, _)| e)
    else {
        return;
    };
    world.remove_resource::<PendingTitle>();
    begin_title_edit(world, e, id);
}

// ---------------------------------------------------------------------------------------------
// Interaction

fn on_collapse(
    a: On<Activate>,
    q: Query<&CollapseButton>,
    q_node: Query<&FeatureNode>,
    mut commands: Commands,
) {
    let Ok(b) = q.get(a.entity) else { return };
    let feature = b.0;
    let collapsed = q_node
        .iter()
        .find(|n| n.feature == feature)
        .is_some_and(|n| n.key.collapsed);
    commands.queue(move |world: &mut World| {
        let Some(element) = world
            .resource::<ActiveDocument>()
            .active_element()
            .map(|e| e.id)
        else {
            return;
        };
        run(
            world,
            &SetNodeCollapsed {
                element,
                feature,
                collapsed: !collapsed,
            },
        );
    });
}

fn on_node_moved(
    ev: On<GraphNodeMoved>,
    q_feature: Query<&FeatureNode>,
    q_config: Query<&ConfigNode>,
    q_parts: Query<(), With<PartsNode>>,
    mut moves: ResMut<PendingMoves>,
) {
    if let Ok(n) = q_feature.get(ev.entity) {
        moves.0.push((Moved::Feature(n.feature), ev.pos));
    } else if let Ok(c) = q_config.get(ev.entity) {
        moves.0.push((Moved::Input(c.input), ev.pos));
    } else if q_parts.contains(ev.entity) {
        moves.0.push((Moved::Parts, ev.pos));
    }
}

/// Saves the nodes moved this frame as one undo step.
fn flush_moves(world: &mut World) {
    let moves = std::mem::take(&mut world.resource_mut::<PendingMoves>().0);
    if moves.is_empty() {
        return;
    }
    let Some(element) = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id)
    else {
        return;
    };
    let round = |p: Vec2| [p.x.round(), p.y.round()];
    run(
        world,
        &MoveGraphNodes {
            element,
            positions: moves
                .iter()
                .filter_map(|(m, p)| match m {
                    Moved::Feature(f) => Some((*f, round(*p))),
                    _ => None,
                })
                .collect(),
            inputs: moves
                .iter()
                .filter_map(|(m, p)| match m {
                    Moved::Input(i) => Some((*i, round(*p))),
                    _ => None,
                })
                .collect(),
            parts_node: moves
                .iter()
                .find(|(m, _)| *m == Moved::Parts)
                .map(|(_, p)| round(*p)),
        },
    );
}

/// The feature whose input or Bypass socket `e` is, and the field.
fn target_of(world: &World, e: Entity) -> Option<SocketKey> {
    world.get::<SocketKey>(e).cloned()
}

/// The command that unwires a field or Bypass (what a socket stands for).
fn unwire(element: ElementId, dst: &SocketKey) -> Option<Box<dyn cadrs_core::Command>> {
    match dst {
        SocketKey::Slot(feature, slot) => Some(Box::new(DisconnectParameter {
            element,
            feature: *feature,
            slot: slot.to_string(),
        })),
        SocketKey::Bypass(feature) => Some(Box::new(cadrs_core::commands::SetSuppressByVariable {
            element,
            feature: *feature,
            rule: None,
        })),
        SocketKey::Dimension(feature, dimension) => Some(Box::new(cfg::DisconnectDimension {
            element,
            feature: *feature,
            dimension: *dimension,
        })),
        _ => None,
    }
}

/// A wire dropped on a field or a Bypass: `#name` written there (and, a wire moved off another
/// input, that one unwired: one undo step). A Variable only reaches the features below it.
fn on_wire_connect(ev: On<WireConnect>, mut commands: Commands) {
    let (from, to, replaces) = (ev.from, ev.to, ev.replaces);
    commands.queue(move |world: &mut World| {
        let (Some(src), Some(dst)) = (target_of(world, from), target_of(world, to)) else {
            return;
        };
        let Some(el) = world.resource::<ActiveDocument>().active_element().cloned() else {
            return;
        };
        let element = el.id;
        let name = match &src {
            SocketKey::Input(n) => n.clone(),
            SocketKey::Variable(f) => match el.feature(*f).map(|g| &g.kind) {
                Some(FeatureKind::Variable(v)) => v.name.clone(),
                _ => return,
            },
            _ => return,
        };
        let target = match &dst {
            SocketKey::Slot(f, _)
            | SocketKey::Bypass(f)
            | SocketKey::Dimension(f, _)
            | SocketKey::DriveDimension(f) => *f,
            _ => return,
        };
        if let SocketKey::Variable(v) = &src {
            let at = |id: FeatureId| el.features().iter().position(|g| g.id == id);
            if at(*v) >= at(target) {
                let why = tr("node-view-variable-below");
                toast(world, why);
                world.resource_mut::<ActiveDocument>().set_changed();
                return;
            }
        }
        let wire: Box<dyn cadrs_core::Command> = match &dst {
            SocketKey::Slot(feature, slot) => Box::new(ConnectParameter {
                element,
                feature: *feature,
                slot: slot.to_string(),
                expr: format!("#{name}"),
            }),
            SocketKey::Bypass(feature) => Box::new(cadrs_core::commands::SetSuppressByVariable {
                element,
                feature: *feature,
                rule: Some(cadrs_core::variables::SuppressByVariable::variable(&name)),
            }),
            SocketKey::Dimension(feature, dimension) => Box::new(cfg::ConnectDimension {
                element,
                feature: *feature,
                dimension: *dimension,
                expr: format!("#{name}"),
            }),
            // Which dimension: asked (the wire's quantity's, not yet driven).
            SocketKey::DriveDimension(feature) => {
                ask_dimension(world, &el, *feature, &name);
                world.resource_mut::<ActiveDocument>().set_changed();
                return;
            }
            _ => return,
        };
        let old = replaces
            .and_then(|r| target_of(world, r))
            .and_then(|k| unwire(element, &k));
        match old {
            Some(old) => run(
                world,
                &cadrs_core::command::Batch {
                    label: format!("Wire #{name}"),
                    scope: cadrs_core::Scope::Element(element),
                    commands: vec![old, wire],
                },
            ),
            None => run(world, wire.as_ref()),
        }
        world.resource_mut::<ActiveDocument>().set_changed();
    });
}

/// The menu a wire dropped on a sketch's **Drive a dimension** opens: the sketch's dimensions
/// that the wire's value can drive (lengths for a length, angles for an angle; any for a plain
/// number) and no expression drives yet. Its anchor holds the sketch and the name.
#[derive(Component, Debug, Clone)]
struct DriveDimensionMenu {
    feature: FeatureId,
    name: String,
}

/// Opens [`DriveDimensionMenu`] at the pointer (a message when no dimension fits).
fn ask_dimension(world: &mut World, el: &Element, feature: FeatureId, name: &str) {
    let Some(f) = el.feature(feature) else { return };
    // What the wire carries: a configuration's `#name`, a Variable's (a new column: any).
    let quantity = config_quantity(el, name).or_else(|| {
        el.features().iter().find_map(|g| match &g.kind {
            FeatureKind::Variable(v) if v.name == name => Some(v.kind.quantity()),
            _ => None,
        })
    });
    let fits = |q: cadrs_sketch::units::Quantity| match quantity {
        Some(cadrs_sketch::units::Quantity::Count) | None => true,
        Some(w) => w == q,
    };
    let units = world.resource::<crate::WorkspaceUnits>().0;
    let free: Vec<cfg::DimensionSlot> = cfg::dimension_slots(f)
        .into_iter()
        .filter(|d| !d.is_driven() && fits(d.quantity))
        .collect();
    if free.is_empty() {
        toast(world, tr("node-view-no-dimension"));
        return;
    }
    let mut menu = Menu::new("node-drive-dimension-menu")
        .min_width(180.0)
        .item_height(23.0);
    for d in &free {
        menu = menu.item(MenuItem::new(
            format!("node-drive-dimension-{}", dim_index(f, d.id)),
            format!("{} · {}", d.label, units.with_unit(d.value, d.quantity)),
        ));
    }
    let at = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
        .and_then(|w| w.cursor_position())
        .unwrap_or(Vec2::splat(200.0));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = cadrs_ui::open_context_menu(&mut commands, at, menu.build(&theme));
    commands.entity(anchor).insert((
        DriveDimensionMenu {
            feature,
            name: name.to_string(),
        },
        DespawnOnExit(crate::AppState::Document),
    ));
    world.flush();
}

/// A dimension chosen in [`DriveDimensionMenu`]: `#name` drives it.
fn on_drive_dimension(
    ev: On<cadrs_ui::MenuAction>,
    q: Query<&DriveDimensionMenu>,
    mut commands: Commands,
) {
    let Ok(m) = q.get(ev.entity) else { return };
    let Some(k) = ev
        .item
        .strip_prefix("node-drive-dimension-")
        .and_then(|k| k.parse::<usize>().ok())
    else {
        return;
    };
    let (feature, name) = (m.feature, m.name.clone());
    commands.queue(move |world: &mut World| {
        let Some(el) = world.resource::<ActiveDocument>().active_element().cloned() else {
            return;
        };
        let Some(d) = el
            .feature(feature)
            .and_then(|f| cfg::dimension_slots(f).into_iter().nth(k.wrapping_sub(1)))
        else {
            return;
        };
        run(
            world,
            &cfg::ConnectDimension {
                element: el.id,
                feature,
                dimension: d.id,
                expr: format!("#{name}"),
            },
        );
        world.resource_mut::<ActiveDocument>().set_changed();
    });
}

/// A wire dropped on the 3D view: on a dimension label of the sketch being edited, its `#name`
/// drives that dimension (as a drop on the sketch node's **Drive a dimension** does); elsewhere,
/// a note says where it goes.
fn on_wire_dropped_outside(
    ev: On<cadrs_ui::node_graph::WireDroppedOutside>,
    mut commands: Commands,
) {
    let (from, at) = (ev.from, ev.position);
    commands.queue(move |world: &mut World| {
        let Some(src) = target_of(world, from) else {
            return;
        };

        let Some(el) = world.resource::<ActiveDocument>().active_element().cloned() else {
            return;
        };
        let name = match &src {
            SocketKey::Input(n) => n.clone(),
            SocketKey::Variable(f) => match el.feature(*f).map(|g| &g.kind) {
                Some(FeatureKind::Variable(v)) => v.name.clone(),
                _ => return,
            },
            _ => return,
        };
        let session = world
            .get_resource::<crate::sketch::SketchSession>()
            .filter(|s| s.element == el.id)
            .map(|s| s.feature);
        let dimension = session.and_then(|_| {
            world
                .get_resource::<crate::sketch_glyphs::SketchOverlay>()
                .and_then(|o| crate::sketch_dimension::dim_at(o, at))
        });
        let (Some(feature), Some(dimension)) = (session, dimension) else {
            toast(world, tr("node-view-drop-on-dimension"));
            return;
        };
        run(
            world,
            &cfg::ConnectDimension {
                element: el.id,
                feature,
                dimension,
                expr: format!("#{name}"),
            },
        );
        world.resource_mut::<ActiveDocument>().set_changed();
    });
}

fn on_wire_disconnect(ev: On<WireDisconnect>, mut commands: Commands) {
    let to = ev.to;
    commands.queue(move |world: &mut World| {
        let Some(dst) = target_of(world, to) else {
            return;
        };
        let Some(element) = world
            .resource::<ActiveDocument>()
            .active_element()
            .map(|e| e.id)
        else {
            return;
        };
        if let Some(c) = unwire(element, &dst) {
            run(world, c.as_ref());
        }
        world.resource_mut::<ActiveDocument>().set_changed();
    });
}

fn on_body_press(ev: On<PointerPress>, q: Query<(), With<BodyCover>>, mut commands: Commands) {
    if ev.button == PointerButton::Primary && q.contains(ev.entity) {
        commands
            .entity(ev.entity)
            .insert(PressedAt(ev.pointer.position));
    }
}

/// A click on a node's body selects its feature and opens it for editing (in the node); a
/// sketch only selects (double-click edits it, as in the list).
fn on_body_click(
    ev: On<PointerClick>,
    q: Query<(&BodyCover, Option<&PressedAt>)>,
    keys: Res<ButtonInput<KeyCode>>,
    mut commands: Commands,
) {
    if ev.button != PointerButton::Primary || ev.count > 1 {
        return;
    }
    let Ok((cover, at)) = q.get(ev.entity) else {
        return;
    };
    // A drag moved the node: not a click.
    if at.is_some_and(|p| p.0.distance(ev.pointer.position) > 4.0) {
        return;
    }
    let id = cover.0;
    // Shift or Ctrl: add it to the selection (or take it out), without editing.
    if keys.any_pressed([
        KeyCode::ShiftLeft,
        KeyCode::ShiftRight,
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
    ]) {
        commands.queue(move |world: &mut World| {
            let mut sel = world.resource_mut::<Selection>();
            let p = Pick::Feature(id);
            if let Some(i) = sel.0.iter().position(|x| *x == p) {
                sel.0.remove(i);
            } else {
                sel.0.push(p);
            }
        });
        return;
    }
    commands.queue(move |world: &mut World| {
        world.resource_mut::<Selection>().0 = vec![Pick::Feature(id)];
        let sketch = world
            .resource::<ActiveDocument>()
            .active_element()
            .and_then(|e| e.feature(id))
            .is_some_and(|f| matches!(f.kind, FeatureKind::Sketch(_)));
        if !sketch {
            crate::document::edit_feature(world, id);
        }
    });
}

/// Moves the dialog of the feature being edited into its node (laid out there, opaque), and
/// hides the node's copy while it is.
fn host_live_dialog(world: &mut World) {
    let edited = crate::variables_ui::edited_feature(world);
    // The edited feature's node is never culled (its live dialog would go with it).
    let nodes: Vec<(Entity, bool, bool)> = world
        .query::<(Entity, &FeatureNode, Has<KeepLaidOut>)>()
        .iter(world)
        .map(|(e, n, k)| (e, Some(n.feature) == edited.map(|x| x.1), k))
        .collect();
    for (e, want, has) in nodes {
        match (want, has) {
            (true, false) => {
                world.entity_mut(e).insert(KeepLaidOut);
            }
            (false, true) => {
                world.entity_mut(e).remove::<KeepLaidOut>();
            }
            _ => {}
        }
    }
    let Some((element, feature)) = edited else {
        return;
    };
    if !world.resource::<NodeView>().is_on(element) {
        return;
    }
    let node = world
        .query::<(Entity, &FeatureNode)>()
        .iter(world)
        .find(|(_, n)| n.feature == feature)
        .map(|(e, _)| e);
    let Some(node) = node else { return };
    // A culled node first comes back (its copy's display is restored then, not after).
    if world.get::<cadrs_ui::node_graph::Culled>(node).is_some() {
        return;
    }
    let mut q_area = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else {
        return;
    };
    // A feature's dialog, not a panel such as Mass properties.
    let mut q_dialog = world.query_filtered::<(Entity, &ChildOf), (
        With<FeatureDialogState>,
        Without<cadrs_ui::feature_dialog::PlainPanel>,
    )>();
    let floating: Vec<Entity> = q_dialog
        .iter(world)
        .filter(|(_, p)| p.parent() == area)
        .map(|(e, _)| e)
        .collect();
    let Some(dialog) = floating.first().copied() else {
        return;
    };
    let t = world.resource::<Theme>().clone();
    {
        let mut commands = world.commands();
        cadrs_ui::feature_dialog::embed(&mut commands, dialog, &t);
        commands.entity(dialog).insert(HostedDialog);
        // First in the node: above its output row.
        commands.entity(node).insert_children(0, &[dialog]);
    }
    world.flush();
    if let Some(mut n) = world.get_mut::<Node>(dialog) {
        n.position_type = PositionType::Relative;
    }
    // Its header drags the node, tinted as the node's copy is.
    let tint = world
        .resource::<ActiveDocument>()
        .doc
        .element(element)
        .and_then(|e| e.feature(feature))
        .map(category_color);
    if let Some(h) = world
        .get::<Children>(dialog)
        .and_then(|c| c.first().copied())
    {
        world.entity_mut(h).insert(GraphNodeHandle);
        if let Some(c) = tint {
            world.entity_mut(h).insert(BackgroundColor(c));
        }
    }
    // The copy and its cover give way to the live dialog.
    let kids: Vec<Entity> = world
        .get::<Children>(node)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    for k in kids {
        if (world.get::<StaticCard>(k).is_some() || world.get::<BodyCover>(k).is_some())
            && let Some(mut n) = world.get_mut::<Node>(k)
        {
            n.display = Display::None;
        }
    }
}

/// The features header's configuration dropdown: the first List's configurations, and which
/// one the Part Studio shows (the other inputs: the Configuration panel and node).
#[derive(Component, Debug, Clone, PartialEq)]
struct HeaderPreset {
    element: ElementId,
    input: ConfigInputId,
    rows: Vec<(ConfigRowId, String)>,
    active: usize,
}

/// Shows the active Part Studio's first List in the features header (in the list and the node
/// view alike), with the configuration shown chosen; none without a List.
fn sync_header_preset(
    doc: Res<ActiveDocument>,
    theme: Res<Theme>,
    q_header: Query<Entity, With<crate::document::FeaturesHeader>>,
    q_have: Query<(Entity, &HeaderPreset)>,
    mut commands: Commands,
) {
    if !doc.is_changed() && !q_have.is_empty() {
        return;
    }
    let want = doc.active_element().and_then(|e| {
        let c = e.configuration()?;
        let (k, i) = c
            .inputs
            .iter()
            .enumerate()
            .find(|(_, i)| matches!(i.kind, InputKind::List { .. }))?;
        let InputKind::List { rows, .. } = &i.kind else {
            return None;
        };
        let active = match c.resolve(&c.active).get(k) {
            Some(cfg::Resolved::Row(r)) => *r,
            _ => 0,
        };
        Some(HeaderPreset {
            element: e.id,
            input: i.id,
            rows: rows.iter().map(|r| (r.id, r.name.clone())).collect(),
            active,
        })
    });
    let have = q_have.iter().next();
    if have.map(|(_, h)| h) == want.as_ref() {
        return;
    }
    if let Some((e, _)) = have {
        commands.entity(e).despawn();
    }
    let (Some(want), Some(header)) = (want, q_header.iter().next()) else {
        return;
    };
    let mut sel = Select::new("feature-preset").width(Val::Px(110.0));
    for (_, name) in &want.rows {
        sel = sel.option(name.clone(), true);
    }
    let at = want.active;
    let e = commands
        .spawn((
            want,
            sel.selected(at).build(&theme),
            cadrs_ui::Tooltip::new(tr("node-view-active-preset")),
        ))
        .id();
    commands.entity(e).entry::<Node>().and_modify(|mut n| {
        n.margin = UiRect::right(Val::Px(4.0));
        n.width = Val::Px(120.0);
        n.flex_grow = 0.0;
        n.flex_shrink = 0.0;
    });
    // After the count (which grows), before the buttons.
    commands.entity(header).insert_children(2, &[e]);
}

fn on_header_preset(ev: On<SelectChange>, q: Query<&HeaderPreset>, mut commands: Commands) {
    let Ok(h) = q.get(ev.entity) else { return };
    let (element, input, row) = (h.element, h.input, h.rows.get(ev.index).map(|r| r.0));
    if let Some(row) = row {
        commands.queue(move |world: &mut World| {
            show_value(world, element, input, cfg::InputValue::Row(row));
        });
    }
}

/// Outlines the nodes of the selected features.
fn sync_selected(
    selection: Res<Selection>,
    theme: Res<Theme>,
    q: Query<(Entity, &FeatureNode, Option<&Outline>)>,
    mut commands: Commands,
) {
    for (e, n, o) in &q {
        let on = selection.0.contains(&Pick::Feature(n.feature));
        match (on, o.is_some()) {
            (true, false) => {
                // Selected nodes move together when one is dragged.
                commands.entity(e).insert((
                    Outline::new(Val::Px(2.0), Val::Px(1.0), theme.primary),
                    cadrs_ui::node_graph::GraphNodeSelected,
                ));
            }
            (false, true) => {
                commands
                    .entity(e)
                    .remove::<(Outline, cadrs_ui::node_graph::GraphNodeSelected)>();
            }
            _ => {}
        }
    }
}

/// A box dragged on the canvas selects the features of the nodes it touches (added to the
/// selection with Shift or Ctrl).
fn on_box_selected(
    ev: On<cadrs_ui::node_graph::BoxSelected>,
    q: Query<&FeatureNode>,
    mut selection: ResMut<Selection>,
) {
    let picked: Vec<Pick> = ev
        .nodes
        .iter()
        .filter_map(|e| q.get(*e).ok())
        .map(|n| Pick::Feature(n.feature))
        .collect();
    if !ev.add {
        selection.0.retain(|p| !matches!(p, Pick::Feature(_)));
    }
    for p in picked {
        if !selection.0.contains(&p) {
            selection.0.push(p);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Routing the wires

/// The routing grid's pitch (graph px).
const PITCH: f32 = 10.0;
/// The room kept round a node (graph px): a wire doesn't hug its edge.
const CLEAR: f32 = 12.0;
/// How far a wire runs straight out of its output and into its input (graph px).
const STUB: f32 = 18.0;

/// The corners of the two 45° paths from `a` to `b`: the straight leg first, or the 45° leg
/// first (none needed when `a` and `b` are on one straight or 45° line).
fn octilinear_legs(a: Vec2, b: Vec2) -> Vec<Vec2> {
    let d = b - a;
    let (ax, ay) = (d.x.abs(), d.y.abs());
    if ax < 1e-3 || ay < 1e-3 || (ax - ay).abs() < 1e-3 {
        return vec![a];
    }
    let diag = ax.min(ay);
    let s = Vec2::new(d.x.signum(), d.y.signum());
    let straight = if ax > ay {
        Vec2::new(d.x - s.x * diag, 0.0)
    } else {
        Vec2::new(0.0, d.y - s.y * diag)
    };
    vec![a + straight, a + s * diag]
}

/// A path whose every leg is made horizontal, vertical or 45° by a corner where it isn't.
fn octilinear_ends(p: &[Vec2]) -> Vec<Vec2> {
    let mut out = vec![p[0]];
    for w in p.windows(2) {
        let c = octilinear_legs(w[0], w[1])[0];
        if c != w[0] && c != w[1] {
            out.push(c);
        }
        out.push(w[1]);
    }
    out
}

/// How far round a wire's ends (grid steps, at least) its search looks first.
const WINDOW: u32 = 30;

/// The cheapest path for a wire from any of `starts` to `target` (cells).
fn search(
    grid: &cadrs_router::grid::Grid,
    occ: &[u16],
    hist: &[u32],
    searcher: &mut cadrs_router::search::Searcher,
    starts: &[u32],
    target: u32,
) -> Option<Vec<u32>> {
    let costs = cadrs_router::search::Costs {
        grid,
        occ,
        hist,
        // A step through another output's trace costs as much as a hundred steps round it:
        // traces take lanes of their own and cross only where they must.
        present: 1000,
    };
    searcher.search(&costs, 1, starts, &[target])
}

/// Routes the node view's wires round its nodes with the PCB autorouter's search
/// (`cadrs_router::search`: A* over octilinear steps, a cell's cost raised by the wires already
/// through it, as its negotiated congestion does): the nodes, grown by [`CLEAR`], are walls; each
/// wire leaves its output and enters its input straight ([`STUB`]); a route is then pulled
/// straight between the points that can see each other. A wire with no way round is a curve.
/// The routes of the problem's `previous` routing stand where nothing near them changed
/// ([`kept_routes`]), so dragging a node reroutes its own wires and those it passes, not all.
fn route_node_wires(p: &cadrs_ui::node_graph::RouteProblem) -> Vec<Option<Vec<Vec2>>> {
    use cadrs_router::grid::{Grid, Sizes};
    use cadrs_router::search::Searcher;
    use cadrs_router_kernels::{FREE, Params, WALL};
    let mut bounds: Option<Rect> = None;
    let mut grow = |r: Rect| bounds = Some(bounds.map_or(r, |b| b.union(r)));
    for o in &p.obstacles {
        grow(*o);
    }
    for (a, b) in &p.wires {
        grow(Rect::from_corners(*a, *b));
    }
    let Some(b) = bounds else {
        return vec![None; p.wires.len()];
    };
    let b = b.inflate(160.0);
    // A coarser grid on a large graph: at most MAX_CELLS cells, so a route never stalls a frame
    // for long (the page has no threads to put it on).
    const MAX_CELLS: f32 = 250_000.0;
    let pitch = PITCH.max((b.width() * b.height() / MAX_CELLS).sqrt());
    // The cells where they are in the graph, not where the graph's bounds put them: a node
    // dragged to the edge moved every cell, and the wires with them.
    let pitch = (pitch / PITCH).ceil() * PITCH;
    let b = Rect::from_corners((b.min / pitch).floor() * pitch, b.max);
    let (nx, ny) = (
        (b.width() / pitch).ceil() as u32 + 1,
        (b.height() / pitch).ceil() as u32 + 1,
    );
    let cell = |v: Vec2| -> (u32, u32) {
        let c = ((v - b.min) / pitch).floor();
        (
            (c.x.max(0.0) as u32).min(nx - 1),
            (c.y.max(0.0) as u32).min(ny - 1),
        )
    };
    let centre = |x: u32, y: u32| b.min + (Vec2::new(x as f32, y as f32) + 0.5) * pitch;
    let mut owner = vec![FREE; (nx * ny) as usize];
    for o in &p.obstacles {
        let o = o.inflate(CLEAR);
        let ((x0, y0), (x1, y1)) = (cell(o.min), cell(o.max));
        for y in y0..=y1 {
            for x in x0..=x1 {
                owner[(y * nx + x) as usize] = WALL;
            }
        }
    }
    let walls = owner.clone();
    // Whether the straight line from `a` to `b` stays off the walls, and off other outputs'
    // traces (its ends excepted: where it leaves and meets its own).
    let free = |a: Vec2, b: Vec2, occ: &[u16]| -> bool {
        let n = (a.distance(b) / (pitch / 3.0)).ceil().max(1.0) as usize;
        (0..=n).all(|i| {
            let (x, y) = cell(a.lerp(b, i as f32 / n as f32));
            let c = (y * nx + x) as usize;
            walls[c] != WALL && (i == 0 || i == n || occ[c] == 0)
        })
    };
    let params = Params {
        nx,
        ny,
        nl: 1,
        slots: 1,
        ortho: 10,
        diag: 14,
        via: u32::MAX / 4,
        ..Default::default()
    };
    let sizes = Sizes {
        track: 0,
        clearance: 0,
        via: 0,
        drill: 0,
        edge: 0,
        hole: 0,
        hole_to_hole: 0,
    };
    let mut grid = Grid {
        params,
        origin: cadrs_eda::units::Pt::ZERO,
        pitch: 1,
        layers: vec![cadrs_eda::layer::Layer::TopCopper],
        owner,
        via_owner: vec![WALL; (nx * ny) as usize],
        nets: Vec::new(),
        sizes,
    };
    // The cells other outputs' traces take: dear to the search and closed to straightening, so
    // traces keep at least a grid step apart (wires from one output share their trunk).
    let mut occ = vec![0u16; (nx * ny) as usize];
    let hist = vec![0u32; (nx * ny) as usize];
    let mut searcher = Searcher::default();
    // Wires grouped by their output (the group with the shortest wire first), short wires first
    // within a group: they have the least choice.
    let len = |k: usize| p.wires[k].0.distance(p.wires[k].1);
    let source = |k: usize| {
        let a = p.wires[k].0.round();
        (a.x as i64, a.y as i64)
    };
    let mut groups: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for k in 0..p.wires.len() {
        groups.entry(source(k)).or_default().push(k);
    }
    let mut groups: Vec<Vec<usize>> = groups.into_values().collect();
    for g in &mut groups {
        g.sort_by(|i, j| len(*i).total_cmp(&len(*j)));
    }
    groups.sort_by(|g, h| len(g[0]).total_cmp(&len(h[0])).then(g[0].cmp(&h[0])));
    // What the last routing found that still stands (a dragged node reroutes its own wires and
    // those near where it was and is, not all of them).
    let kept = match p.previous.as_deref() {
        Some(prev) => kept_routes(p, prev, &groups, CLEAR + 2.0 * pitch),
        None => vec![None; p.wires.len()],
    };
    // Pulled straight as the PCB router straightens its tracks: from each point to the farthest
    // one a 45° path reaches clear of the nodes and of other outputs' traces (one straight leg
    // and one at 45°, either way round), so every leg is horizontal, vertical or at 45°.
    let pull = |pts: &[Vec2], occ: &[u16]| -> Vec<Vec2> {
        let mut pulled = vec![pts[0]];
        let mut i = 0;
        while i + 1 < pts.len() {
            let mut j = pts.len() - 1;
            let free = |a: Vec2, b: Vec2| free(a, b, occ);
            let leg = loop {
                let found = octilinear_legs(pts[i], pts[j])
                    .into_iter()
                    .find(|corner| free(pts[i], *corner) && free(*corner, pts[j]));
                match found {
                    Some(c) => break Some(c),
                    None if j > i + 1 => j -= 1,
                    None => break None,
                }
            };
            // Neighbouring cells are a step apart, straight or at 45°: one leg.
            if let Some(c) = leg
                && c != pts[i]
                && c != pts[j]
            {
                pulled.push(c);
            }
            pulled.push(pts[j]);
            i = j;
        }
        pulled
    };
    // The cells a trace takes for other outputs' wires: those it runs through and those beside
    // them, so another output's trace keeps two steps away (one step, and a trunk drawn thick
    // touched it zoomed out).
    let taken = |tr: &[Vec2], mine: &mut Vec<u32>| {
        for w in tr.windows(2) {
            let n = (w[0].distance(w[1]) / (pitch / 3.0)).ceil().max(1.0) as usize;
            for i in 0..=n {
                let (x, y) = cell(w[0].lerp(w[1], i as f32 / n as f32));
                for (dx, dy) in [(0i64, 0i64), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (cx, cy) = (x as i64 + dx, y as i64 + dy);
                    if cx >= 0 && cy >= 0 && cx < nx as i64 && cy < ny as i64 {
                        mine.push(cy as u32 * nx + cx as u32);
                    }
                }
            }
        }
    };
    // Each output's traces count once in a cell.
    let claim = |mut mine: Vec<u32>, occ: &mut [u16]| {
        mine.sort_unstable();
        mine.dedup();
        for c in mine {
            occ[c as usize] = occ[c as usize].saturating_add(1);
        }
    };
    let mut out: Vec<Option<Vec<Vec2>>> = vec![None; p.wires.len()];
    // The kept routes first, taken, so the wires routed again go round them.
    for group in groups.iter().filter(|g| kept[g[0]].is_some()) {
        let mut mine: Vec<u32> = Vec::new();
        for &k in group {
            out[k] = kept[k].clone().flatten();
            if let Some(tr) = &out[k] {
                taken(tr, &mut mine);
            }
        }
        claim(mine, &mut occ);
    }
    for group in groups {
        if kept[group[0]].is_some() {
            continue;
        }
        // This output's traces so far (each from its stub's end), and the points where later
        // ones branch off them.
        let mut traces: Vec<(usize, Vec<Vec2>)> = Vec::new();
        let mut branches: Vec<Vec2> = Vec::new();
        for k in group.iter().copied() {
            let (a, z) = p.wires[k];
            let (s, t) = (a + Vec2::X * STUB, z - Vec2::X * STUB);
            let (cs, ct) = (cell(s), cell(t));
            let (is, it) = (
                grid.params.index(cs.0, cs.1, 0),
                grid.params.index(ct.0, ct.1, 0),
            );
            // Where it may start: its stub's end, or anywhere along this output's traces so far
            // (not their last steps into their inputs), so wires from one output share a trunk
            // and branch off it instead of running side by side. Each start cell with the trace,
            // the leg and the point on it.
            let mut starts: Vec<u32> = vec![is];
            let mut on_trace: HashMap<u32, (usize, usize, Vec2)> = HashMap::new();
            for (ti, (_, tr)) in traces.iter().enumerate() {
                let legs = tr.len().saturating_sub(1);
                for li in 0..legs.saturating_sub(1) {
                    let (u, v) = (tr[li], tr[li + 1]);
                    let n = (u.distance(v) / (pitch / 3.0)).ceil().max(1.0) as usize;
                    for i in 0..=n {
                        let q = u.lerp(v, i as f32 / n as f32);
                        let (x, y) = cell(q);
                        let c = grid.params.index(x, y, 0);
                        if let std::collections::hash_map::Entry::Vacant(e) = on_trace.entry(c) {
                            e.insert((ti, li, q));
                            starts.push(c);
                        }
                    }
                }
            }
            // The stubs' ends may sit in a neighbour's clearance: open them for this wire.
            let keep = (grid.owner[is as usize], grid.owner[it as usize]);
            grid.owner[is as usize] = FREE;
            grid.owner[it as usize] = FREE;
            // Searched in a window round its ends first, walled off: a wire that must cross
            // another output's trace (dear, below) otherwise searches the whole graph for a
            // way round before it does. Out of the window only when there is no way in it.
            let half = (cs.0.abs_diff(ct.0).max(cs.1.abs_diff(ct.1)) / 2).max(WINDOW);
            let window = (
                cs.0.min(ct.0).saturating_sub(half),
                cs.1.min(ct.1).saturating_sub(half),
                (cs.0.max(ct.0) + half).min(nx - 1),
                (cs.1.max(ct.1) + half).min(ny - 1),
            );
            let mut path = None;
            for windowed in [true, false] {
                let (x0, y0, x1, y1) = window;
                let mut ring: Vec<(u32, u32)> = Vec::new();
                if windowed {
                    let inside: Vec<u32> = starts
                        .iter()
                        .copied()
                        .filter(|c| {
                            let (x, y, _) = grid.params.coords(*c);
                            x > x0 && x < x1 && y > y0 && y < y1
                        })
                        .collect();
                    for x in x0..=x1 {
                        for y in [y0, y1] {
                            let c = y * nx + x;
                            ring.push((c, grid.owner[c as usize]));
                        }
                    }
                    for y in y0..=y1 {
                        for x in [x0, x1] {
                            let c = y * nx + x;
                            ring.push((c, grid.owner[c as usize]));
                        }
                    }
                    for (c, _) in &ring {
                        grid.owner[*c as usize] = WALL;
                    }
                    path = search(&grid, &occ, &hist, &mut searcher, &inside, it);
                    for (c, was) in ring.iter().rev() {
                        grid.owner[*c as usize] = *was;
                    }
                } else {
                    path = search(&grid, &occ, &hist, &mut searcher, &starts, it);
                }
                if path.is_some() {
                    break;
                }
            }
            grid.owner[is as usize] = keep.0;
            grid.owner[it as usize] = keep.1;
            let Some(path) = path else { continue };
            // Where it leaves: its own stub, or a point of an earlier trace (whose way there it
            // shares).
            let (prefix, from) = match path.first().and_then(|c| on_trace.get(c)) {
                Some(&(ti, li, q)) if path[0] != is => {
                    let tr = &traces[ti].1;
                    let mut pre = tr[..=li].to_vec();
                    if pre.last() != Some(&q) {
                        pre.push(q);
                    }
                    branches.push(q);
                    (pre, q)
                }
                _ => (vec![s], s),
            };
            // The cells' centres, from where it leaves to its stub's end, pulled straight.
            let mut pts: Vec<Vec2> = path
                .iter()
                .map(|c| {
                    let (x, y, _) = grid.params.coords(*c);
                    centre(x, y)
                })
                .collect();
            if let Some(f) = pts.first_mut() {
                *f = from;
            }
            if let Some(l) = pts.last_mut() {
                *l = t;
            }
            // (One cell: it leaves where it ends.)
            if pts.len() == 1 {
                pts.push(t);
            }
            let pulled = pull(&pts, &occ);
            // The ends are off the cells' centres: the first and last legs are made octilinear by
            // a corner of their own.
            let own = octilinear_ends(&pulled);
            let mut trace = prefix;
            trace.extend(own.into_iter().skip(1));
            traces.push((k, trace));
        }
        // Every trace split where another branches off it, so a shared run is the same legs in
        // each (drawn thicker for the wires it carries).
        for (_, tr) in &mut traces {
            let mut split = vec![tr[0]];
            for w in tr.windows(2) {
                let (u, v) = (w[0], w[1]);
                let mut on: Vec<Vec2> = branches
                    .iter()
                    .copied()
                    .filter(|q| {
                        let d = v - u;
                        let len = d.length();
                        if len < 1e-3 {
                            return false;
                        }
                        let along = (*q - u).dot(d) / len;
                        along > 0.5
                            && along < len - 0.5
                            && (u + d * (along / len)).distance(*q) < 0.5
                    })
                    .collect();
                on.sort_by(|x, y| u.distance(*x).total_cmp(&u.distance(*y)));
                on.dedup_by(|x, y| x.distance(*y) < 0.5);
                split.extend(on);
                split.push(v);
            }
            *tr = split;
        }
        // This output's traces, taken for the next outputs' wires.
        let mut mine: Vec<u32> = Vec::new();
        for (k, tr) in traces {
            taken(&tr, &mut mine);
            out[k] = Some(tr);
        }
        claim(mine, &mut occ);
    }
    out
}

/// Which wires keep the route `prev` gave them, as `Some(route)` (`Some(None)`: a curve still),
/// by output: an output's wires keep theirs unless one of them is new or moved its ends, one
/// left it, or one runs within `near` of a node that moved (where it is now, which it may cross,
/// or where it was, round which it may have gone and now needn't). Nothing is kept when more
/// than half the nodes moved (an arrangement): routing them all afresh costs no more.
fn kept_routes(
    p: &cadrs_ui::node_graph::RouteProblem,
    prev: &cadrs_ui::node_graph::Routed,
    groups: &[Vec<usize>],
    near: f32,
) -> Vec<Option<Option<Vec<Vec2>>>> {
    let mut kept = vec![None; p.wires.len()];
    let key = |v: Vec2| {
        let r = v.round();
        (r.x as i64, r.y as i64)
    };
    // The nodes that moved: where they are and where they were (a rectangle in one list and
    // not the other).
    let mut count: HashMap<[(i64, i64); 2], (i32, Rect)> = HashMap::new();
    for (r, d) in p
        .obstacles
        .iter()
        .map(|r| (r, 1))
        .chain(prev.problem.obstacles.iter().map(|r| (r, -1)))
    {
        count.entry([key(r.min), key(r.max)]).or_insert((0, *r)).0 += d;
    }
    let moved: Vec<Rect> = count
        .into_values()
        .filter(|(n, _)| *n != 0)
        .map(|(_, r)| r.inflate(near))
        .collect();
    if moved.len() > p.obstacles.len() {
        return kept;
    }
    let ends = |(a, b): &(Vec2, Vec2)| (key(*a), key(*b));
    let last: HashMap<_, &Option<Vec<Vec2>>> = prev
        .problem
        .wires
        .iter()
        .map(ends)
        .zip(&prev.routes)
        .collect();
    let now: std::collections::HashSet<_> = p.wires.iter().map(ends).collect();
    // The outputs a wire left (removed, or moved to another output or input).
    let left: std::collections::HashSet<(i64, i64)> = prev
        .problem
        .wires
        .iter()
        .filter(|w| !now.contains(&ends(w)))
        .map(|(a, _)| key(*a))
        .collect();
    let near_moved = |a: Vec2, b: Vec2| moved.iter().any(|r| segment_meets_rect(a, b, *r));
    for g in groups {
        if left.contains(&key(p.wires[g[0]].0)) {
            continue;
        }
        let routes: Option<Vec<Option<Vec<Vec2>>>> = g
            .iter()
            .map(|&k| {
                let (a, b) = p.wires[k];
                let route = *last.get(&ends(&(a, b)))?;
                let pts: Vec<Vec2> = std::iter::once(a)
                    .chain(route.iter().flatten().copied())
                    .chain(std::iter::once(b))
                    .collect();
                (!pts.windows(2).any(|w| near_moved(w[0], w[1]))).then(|| route.clone())
            })
            .collect();
        if let Some(routes) = routes {
            for (&k, r) in g.iter().zip(routes) {
                kept[k] = Some(r);
            }
        }
    }
    kept
}

/// Whether the segment from `a` to `b` meets the rectangle `r` (Liang–Barsky clipping).
fn segment_meets_rect(a: Vec2, b: Vec2, r: Rect) -> bool {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - r.min.x),
        (d.x, r.max.x - a.x),
        (-d.y, a.y - r.min.y),
        (d.y, r.max.y - a.y),
    ] {
        if p.abs() < 1e-6 {
            if q < 0.0 {
                return false;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A large graph: ten columns of eight nodes, each node fed by two or three nodes of the
    /// columns before it.
    fn big_graph() -> cadrs_ui::node_graph::RouteProblem {
        let mut p = cadrs_ui::node_graph::RouteProblem::default();
        let rect = |c: usize, r: usize| {
            let min = Vec2::new(c as f32 * 300.0, r as f32 * 220.0);
            Rect::from_corners(
                min,
                min + Vec2::new(180.0, 90.0 + (c * 7 + r * 13) as f32 % 80.0),
            )
        };
        for c in 0..10 {
            for r in 0..8 {
                p.obstacles.push(rect(c, r));
                if c == 0 {
                    continue;
                }
                let from = [
                    (c - 1, r),
                    (c.saturating_sub(1 + r % 3), (r + 3) % 8),
                    (c - 1, (r + 5) % 8),
                ];
                for (i, (fc, fr)) in from.into_iter().enumerate().take(2 + (c + r) % 2) {
                    let a = rect(fc, fr);
                    let b = rect(c, r);
                    p.wires.push((
                        Vec2::new(a.max.x, a.min.y + 20.0),
                        Vec2::new(b.min.x, b.min.y + 20.0 + 20.0 * i as f32),
                    ));
                }
            }
        }
        p
    }

    /// `p` with node `i` moved by `d`, its wires' ends with it, and the routing `prev` as the
    /// last one.
    fn moved(
        p: &cadrs_ui::node_graph::RouteProblem,
        prev: &[Option<Vec<Vec2>>],
        i: usize,
        d: Vec2,
    ) -> cadrs_ui::node_graph::RouteProblem {
        let was = p.obstacles[i];
        let on = |v: Vec2| was.inflate(0.5).contains(v);
        let mut q = cadrs_ui::node_graph::RouteProblem {
            obstacles: p.obstacles.clone(),
            wires: p.wires.clone(),
            previous: Some(std::sync::Arc::new(cadrs_ui::node_graph::Routed {
                problem: cadrs_ui::node_graph::RouteProblem {
                    previous: None,
                    ..p.clone()
                },
                routes: prev.to_vec(),
            })),
        };
        q.obstacles[i] = Rect::from_corners(was.min + d, was.max + d);
        for (a, b) in &mut q.wires {
            if on(*a) {
                *a += d;
            }
            if on(*b) {
                *b += d;
            }
        }
        q
    }

    /// Every route keeps off every node (its stubs, out of and into its own, aside).
    fn assert_off_nodes(p: &cadrs_ui::node_graph::RouteProblem, r: &[Option<Vec<Vec2>>]) {
        for via in r.iter().flatten() {
            for w in via.windows(2) {
                for o in &p.obstacles {
                    assert!(
                        !segment_meets_rect(w[0], w[1], o.inflate(-1.0)),
                        "{w:?} crosses {o:?}"
                    );
                }
            }
        }
    }

    /// What a routing kept of the last one: how many wires have the same route.
    fn same(a: &[Option<Vec<Vec2>>], b: &[Option<Vec<Vec2>>]) -> usize {
        a.iter().zip(b).filter(|(x, y)| x == y).count()
    }

    #[test]
    fn a_routing_keeps_what_a_move_leaves_alone() {
        let p = big_graph();
        let first = route_node_wires(&p);
        assert_off_nodes(&p, &first);
        // Nothing moved: everything kept.
        let again = route_node_wires(&moved(&p, &first, 0, Vec2::ZERO));
        assert_eq!(again, first);
        // A node with no wires taken far off: nothing rerouted.
        let mut lone = p.clone();
        lone.obstacles
            .push(Rect::new(-2000.0, -2000.0, -1800.0, -1900.0));
        let lone_first = route_node_wires(&lone);
        let n = lone.obstacles.len() - 1;
        let r = route_node_wires(&moved(&lone, &lone_first, n, Vec2::new(-500.0, 0.0)));
        assert_eq!(r, lone_first);
        // A node in the middle moved: its wires and those near it rerouted, the rest kept, and
        // all of them round the nodes where they are now.
        let i = 5 * 8 + 3;
        let q = moved(&p, &first, i, Vec2::new(40.0, 60.0));
        let r = route_node_wires(&q);
        assert_off_nodes(&q, &r);
        let kept = same(&r, &first);
        assert!(
            kept > p.wires.len() / 2 && kept < p.wires.len(),
            "{kept} of {} kept",
            p.wires.len()
        );
    }

    /// How long a route of [`big_graph`] takes, from scratch and after one node moved (run with
    /// `--ignored --nocapture`).
    #[test]
    #[ignore]
    fn route_timing() {
        let p = big_graph();
        let t = std::time::Instant::now();
        let first = route_node_wires(&p);
        println!(
            "{} nodes, {} wires: from scratch {:?}",
            p.obstacles.len(),
            p.wires.len(),
            t.elapsed()
        );
        let t = std::time::Instant::now();
        let _ = route_node_wires(&moved(&p, &first, 0, Vec2::ZERO));
        println!("nothing moved: {:?}", t.elapsed());
        let q = moved(&p, &first, 5 * 8 + 3, Vec2::new(40.0, 60.0));
        let t = std::time::Instant::now();
        let r = route_node_wires(&q);
        println!(
            "a node moved: {:?}, {} of {} rerouted",
            t.elapsed(),
            p.wires.len() - same(&r, &first),
            p.wires.len()
        );
    }

    #[test]
    fn wires_go_round_nodes() {
        use cadrs_ui::node_graph::RouteProblem;
        // A node between the two ends: the wire can't go straight through it.
        let block = Rect::new(100.0, -100.0, 200.0, 100.0);
        let p = RouteProblem {
            obstacles: vec![block],
            wires: vec![(Vec2::new(0.0, 0.0), Vec2::new(300.0, 0.0))],
            ..Default::default()
        };
        let r = route_node_wires(&p);
        let via = r[0].as_ref().expect("routed");
        let mut pts = vec![Vec2::ZERO];
        pts.extend(via.iter().copied());
        pts.push(Vec2::new(300.0, 0.0));
        let grown = block.inflate(CLEAR - 1.0);
        for w in pts.windows(2) {
            for i in 0..=50 {
                let q = w[0].lerp(w[1], i as f32 / 50.0);
                assert!(!grown.contains(q), "{q} is in the node: {pts:?}");
            }
        }
        // Every leg is a circuit trace's: horizontal, vertical or at 45°.
        assert!(
            pts.windows(2).all(|w| {
                let d = w[1] - w[0];
                d.x.abs() < 1e-3 || d.y.abs() < 1e-3 || (d.x.abs() - d.y.abs()).abs() < 1e-2
            }),
            "{pts:?}"
        );
        // With nothing in the way it is short and straight-ish.
        let r = route_node_wires(&RouteProblem {
            obstacles: Vec::new(),
            wires: vec![(Vec2::new(0.0, 0.0), Vec2::new(300.0, 40.0))],
            ..Default::default()
        });
        assert!(r[0].as_ref().unwrap().len() <= 4, "{:?}", r[0]);
    }

    /// Two sketch → extrude chains: each extrude level with its sketch (the second sketch moved
    /// down beside its extrude, which the first extrude, taller, pushed down).
    /// Two wires from different outputs along the same run keep two grid steps apart; two from
    /// one output may share their trunk.
    #[test]
    fn traces_keep_apart() {
        use cadrs_ui::node_graph::RouteProblem;
        let p = RouteProblem {
            obstacles: Vec::new(),
            wires: vec![
                (Vec2::new(0.0, 0.0), Vec2::new(400.0, 100.0)),
                (Vec2::new(0.0, 4.0), Vec2::new(400.0, 104.0)),
            ],
            ..Default::default()
        };
        let r = route_node_wires(&p);
        let trace = |k: usize| {
            let mut t = vec![p.wires[k].0];
            t.extend(r[k].clone().expect("routed"));
            t.push(p.wires[k].1);
            t
        };
        let (a, b) = (trace(0), trace(1));
        // The least distance between the two (sampled along the middle, away from the ends).
        let sample = |t: &[Vec2]| -> Vec<Vec2> {
            t.windows(2)
                .flat_map(|w| (0..=20).map(move |i| w[0].lerp(w[1], i as f32 / 20.0)))
                .filter(|q| q.x > 60.0 && q.x < 340.0)
                .collect()
        };
        let (sa, sb) = (sample(&a), sample(&b));
        let gap = sa
            .iter()
            .flat_map(|q| sb.iter().map(move |w| q.distance(*w)))
            .fold(f32::MAX, f32::min);
        // Two grid steps: a step's lane beside each trace is kept clear.
        assert!(gap >= PITCH * 1.8, "{gap}: {a:?} {b:?}");
    }

    /// Three wires from one output to three inputs far apart share a trunk and branch off it,
    /// and their shared legs are the same legs.
    #[test]
    fn wires_from_one_output_share_a_trunk() {
        use cadrs_ui::node_graph::RouteProblem;
        let a = Vec2::new(0.0, 0.0);
        let p = RouteProblem {
            obstacles: Vec::new(),
            wires: vec![
                (a, Vec2::new(600.0, 200.0)),
                (a, Vec2::new(600.0, 300.0)),
                (a, Vec2::new(600.0, 400.0)),
            ],
            ..Default::default()
        };
        let r = route_node_wires(&p);
        let legs = |k: usize| -> Vec<(Vec2, Vec2)> {
            let mut t = vec![p.wires[k].0];
            t.extend(r[k].clone().expect("routed"));
            t.push(p.wires[k].1);
            t.windows(2).map(|w| (w[0], w[1])).collect()
        };
        let (l0, l1, l2) = (legs(0), legs(1), legs(2));
        // How far (graph px) two wires run along the same legs.
        let common = |x: &[(Vec2, Vec2)], y: &[(Vec2, Vec2)]| -> f32 {
            x.iter()
                .filter(|l| y.contains(l))
                .map(|(u, v)| u.distance(*v))
                .sum()
        };
        assert!(common(&l0, &l1) > 150.0, "{l0:?}\n{l1:?}");
        assert!(common(&l1, &l2) > 150.0, "{l1:?}\n{l2:?}");
    }

    #[test]
    fn single_wires_lie_level() {
        use cadrs_core::document::ExtrudeFeature;
        let mut el = Element::part_studio("Part Studio 1");
        let (s1, s2, e1, e2) = (
            FeatureId::new(),
            FeatureId::new(),
            FeatureId::new(),
            FeatureId::new(),
        );
        let fs = el.features_mut().unwrap();
        for (id, name) in [(s1, "Sketch 1"), (s2, "Sketch 2")] {
            fs.push(Feature::new(
                id,
                name,
                FeatureKind::Sketch(Default::default()),
            ));
        }
        for (id, sk, name) in [(e1, s1, "Extrude 1"), (e2, s2, "Extrude 2")] {
            let mut e = ExtrudeFeature::default();
            e.sketches.push(sk);
            fs.push(Feature::new(id, name, FeatureKind::Extrude(e)));
        }
        let pos = layout(&el, &HashMap::new()).features;
        assert_eq!(pos[&s1].y, pos[&e1].y);
        assert_eq!(pos[&s2].y, pos[&e2].y, "{pos:?}");
        assert!(pos[&e1].x > pos[&s1].x);
        assert_apart(&el, &pos);
    }

    /// No two nodes overlap (as their heights are guessed before they are laid out).
    #[track_caller]
    fn assert_apart(el: &Element, pos: &HashMap<FeatureId, Vec2>) {
        let rect = |f: &Feature| {
            let p = pos[&f.id];
            Rect::from_corners(p, p + Vec2::new(NODE_W, height_guess(f, false)))
        };
        let fs = el.features();
        for (i, a) in fs.iter().enumerate() {
            for b in &fs[i + 1..] {
                let o = rect(a).intersect(rect(b));
                assert!(o.is_empty(), "{} and {} overlap: {pos:?}", a.name, b.name);
            }
        }
    }

    /// A sketch with two extrudes: the first beside it, the second below the first; a lone
    /// sketch → extrude chain below that, on its own row.
    #[test]
    fn branches_stack_and_chains_lie_level() {
        use cadrs_core::document::ExtrudeFeature;
        let mut el = Element::part_studio("Part Studio 1");
        let ids: Vec<FeatureId> = (0..5).map(|_| FeatureId::new()).collect();
        let fs = el.features_mut().unwrap();
        let sk = |id, n: &str| Feature::new(id, n, FeatureKind::Sketch(Default::default()));
        let ex = |id, s, n: &str| {
            let mut e = ExtrudeFeature::default();
            e.sketches.push(s);
            Feature::new(id, n, FeatureKind::Extrude(e))
        };
        fs.push(sk(ids[0], "Sketch 1"));
        fs.push(ex(ids[1], ids[0], "Extrude 1"));
        fs.push(ex(ids[2], ids[0], "Extrude 2"));
        fs.push(sk(ids[3], "Sketch 2"));
        fs.push(ex(ids[4], ids[3], "Extrude 3"));
        let pos = layout(&el, &HashMap::new()).features;
        let p = |k: usize| pos[&ids[k]];
        assert_eq!(p(1).y, p(0).y);
        assert!(p(1).x > p(0).x && p(2).x == p(1).x);
        assert!(p(2).y >= p(1).y + 330.0, "{pos:?}");
        // The second tree is clear of the first (below it or beside it).
        assert_eq!(p(4).y, p(3).y);
        assert_apart(&el, &pos);
    }

    /// A sketch's thumbnail shows its curves, fitted into the square.
    #[test]
    fn sketch_thumbnails_draw_the_curves() {
        let mut sk = cadrs_sketch::Sketch::default();
        sk.add_line(
            cadrs_sketch::Vec2::new(0.0, 0.0),
            cadrs_sketch::Vec2::new(40.0, 0.0),
        );
        sk.add_line(
            cadrs_sketch::Vec2::new(40.0, 0.0),
            cadrs_sketch::Vec2::new(40.0, 20.0),
        );
        let img = sketch_thumbnail(&sk, Color::WHITE);
        let data = img.data.as_ref().unwrap();
        let inked = data.chunks(4).filter(|p| p[3] > 128).count();
        assert!(inked > 100, "{inked} pixels inked");
        // Fitted: the long line runs nearly across.
        let size = 128usize;
        let row_inked = |y: usize| {
            (0..size)
                .filter(|x| data[(y * size + x) * 4 + 3] > 128)
                .count()
        };
        assert!((0..size).any(|y| row_inked(y) > 100));
    }

    #[test]
    fn labels_match_row_names() {
        assert_eq!(squash("Depth"), "depth");
        assert_eq!(squash("extrude-depth"), "extrudedepth");
        assert_eq!(squash("Thickness 1"), "thickness1");
        assert!(squash("extrude-second-depth").ends_with(&squash("Second depth")));
    }

    #[test]
    fn nodes_are_laid_out_by_reference_depth() {
        use cadrs_core::document::ExtrudeFeature;
        let mut el = Element::part_studio("Part Studio 1");
        let sketch = FeatureId::new();
        let ext = FeatureId::new();
        let fs = el.features_mut().unwrap();
        fs.push(Feature::new(
            sketch,
            "Sketch 1",
            FeatureKind::Sketch(Default::default()),
        ));
        let mut e = ExtrudeFeature::default();
        e.sketches.push(sketch);
        fs.push(Feature::new(ext, "Extrude 1", FeatureKind::Extrude(e)));
        let pos = layout(&el, &HashMap::new()).features;
        assert!(pos[&ext].x > pos[&sketch].x);
        assert_eq!(pos[&sketch].x, COLUMN);
        // A saved place wins.
        el.node_layout_mut()
            .unwrap()
            .positions
            .insert(ext, [5.0, 6.0]);
        let pos = layout(&el, &HashMap::new()).features;
        assert_eq!(pos[&ext], Vec2::new(5.0, 6.0));
    }
}

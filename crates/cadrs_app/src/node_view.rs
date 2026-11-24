//! The **node view** of a Part Studio: its feature list as a node graph (Blender's geometry
//! nodes, Houdini's networks), with the studio's configuration as the node its values come from
//! (see `cadrs_core::configuration`). The feature list's header has a button that turns it on;
//! the feature panel then widens, and the list and the floating feature dialog give way to an
//! endless canvas ([`cadrs_ui::node_graph`]) where:
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
    self as cfg, AddConfigInput, AddPreset, ConfigInput, Configuration, ConnectParameter,
    DisconnectParameter, EditConfigInput, InputKind, MoveGraphNodes, RemoveConfigInput,
    RemovePreset, RenamePreset, SetActivePreset, SetNodeCollapsed, SetPresetValue,
};
use cadrs_core::ids::{InputId, PresetId};
use cadrs_core::{Element, ElementId, Feature, FeatureId, FeatureKind};
use cadrs_sketch::units::{Quantity, Units};
use cadrs_ui::i18n::tr;
use cadrs_ui::node_graph::{
    CanvasViewChanged, FitCanvas, GraphNode, GraphNodeHandle, GraphNodeMoved, NodeCanvas,
    NodeCanvasBuilder, PanZoom, Socket, SocketSide, StaticCopyRoot, Wire, WireConnect,
    WireDisconnect,
};
use cadrs_ui::{
    CheckboxChange, FeatureDialog, FeatureDialogState, IconButton, NumberField, NumberFieldCommit,
    Select, SelectChange, TextInput, TextSubmit, Theme,
};

use crate::ActiveDocument;
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
        .add_observer(on_toggle)
        .add_observer(on_view_changed)
        .add_observer(on_node_moved)
        .add_observer(on_wire_connect)
        .add_observer(on_wire_disconnect)
        .add_observer(on_body_press)
        .add_observer(on_body_click)
        .add_observer(on_config_button)
        .add_observer(on_config_number)
        .add_observer(on_config_select)
        .add_observer(on_config_text)
        .add_observer(on_config_checkbox)
        .add_observer(on_collapse)
        .add_observer(on_header_preset)
        .add_observer(on_box_selected)
        .add_systems(
            Update,
            (
                sync_mode,
                flush_moves,
                sync_nodes,
                host_live_dialog,
                sync_selected,
                fit_later,
                sync_header_preset,
                measure_heights,
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
    /// The Part Studios whose Configuration node shows its presets table.
    tables: HashSet<ElementId>,
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

/// Where the node view goes in the feature panel (a sibling of the feature list's body, which it
/// takes the place of). Spawned hidden with the feature list ([`host_bundle`]).
#[derive(Component, Debug, Clone, Copy)]
pub struct NodeViewHost(pub ElementId);

/// The feature list's scrolling body: hidden while the node view shows.
#[derive(Component, Debug, Clone, Copy)]
pub struct FeatureListBody;

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
    feature: Option<Feature>,
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

/// The Configuration node, and what it was built from.
#[derive(Component, Debug, Clone)]
struct ConfigNode {
    key: ConfigKey,
}

/// What the Configuration node's look depends on: the configuration, the units, the theme, and
/// whether its presets table shows.
type ConfigKey = (Option<Configuration>, Units, u64, bool);

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
    /// A field of a feature ("Depth").
    Slot(FeatureId, &'static str),
    /// A feature's suppression variable.
    Bypass(FeatureId),
    /// A configuration input, by name.
    Input(String),
    /// A Variable feature's value, by name.
    Variable(String),
    /// A part of the Parts node.
    Part(cadrs_core::PartId),
}

/// A node's collapse chevron.
#[derive(Component, Debug, Clone, Copy)]
struct CollapseButton(FeatureId);

/// The press that may become a click on a body (where it was), so a drag isn't taken for one.
#[derive(Component, Debug, Clone, Copy)]
struct PressedAt(Vec2);

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

/// Node moves to save as one undo step (a drag of several selected nodes ends on each).
#[derive(Resource, Debug, Default)]
struct PendingMoves(Vec<(Option<FeatureId>, Vec2)>);

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

fn input_type(k: InputKind) -> u32 {
    match k {
        InputKind::Boolean => BOOLEAN,
        k => quantity_type(k.quantity()),
    }
}

fn socket_color(ty: u32) -> Color {
    match ty {
        GEOMETRY => Color::srgb_u8(0x4c, 0xb0, 0x5c),
        LENGTH => Color::srgb_u8(0xa1, 0xa1, 0xa1),
        ANGLE => Color::srgb_u8(0x8f, 0x6f, 0xd6),
        NUMBER => Color::srgb_u8(0x3f, 0xa7, 0xc4),
        _ => Color::srgb_u8(0xd0, 0x7c, 0xc0),
    }
}

/// A sketch's wires are blue, a plane's grey, the rest (parts) green.
fn geometry_color(f: &Feature) -> Color {
    match f.kind {
        FeatureKind::Sketch(_) => Color::srgb_u8(0x3d, 0x84, 0xe0),
        FeatureKind::Plane(_) | FeatureKind::MateConnector(_) => Color::srgb_u8(0x90, 0x90, 0x90),
        _ => socket_color(GEOMETRY),
    }
}

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

/// The button in the feature list's header, and the host the canvas goes in (both spawned by
/// `document::feature_list`).
pub fn host_bundle(element: ElementId) -> impl Bundle {
    (
        Name::new("node-view-host"),
        NodeViewHost(element),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            display: Display::None,
            ..default()
        },
    )
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

/// Shows the canvas or the list, and keeps the header button pressed while the canvas shows.
/// (The feature panel's width follows in `assembly::list::list_width`.)
#[allow(clippy::type_complexity)]
fn sync_mode(
    view: Res<NodeView>,
    doc: Res<ActiveDocument>,
    mut q_host: Query<(&NodeViewHost, &mut Node), Without<FeatureListBody>>,
    mut q_body: Query<&mut Node, (With<FeatureListBody>, Without<NodeViewHost>)>,
    q_button: Query<(Entity, &Name, Has<cadrs_ui::Selected>), With<bevy::ui_widgets::Button>>,
    mut commands: Commands,
) {
    let active = doc.active_element().map(|e| e.id);
    let on = active.is_some_and(|e| view.is_on(e));
    for (host, mut n) in &mut q_host {
        let want = if Some(host.0) == active && on {
            Display::Flex
        } else {
            Display::None
        };
        if n.display != want {
            n.display = want;
        }
    }
    for mut n in &mut q_body {
        let want = if on { Display::None } else { Display::Flex };
        if n.display != want {
            n.display = want;
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
/// The Configuration node's width (room for an input's name, kind and value).
const CONFIG_W: f32 = 300.0;
/// A column taller than this (graph px) goes on in another beside it.
const TALL: f32 = 2400.0;
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
        FeatureKind::Sketch(_) => 90.0,
        _ => 70.0 + 26.0 * cfg::parameter_slots(f).len().max(1) as f32,
    }
}

/// Where the nodes go: the Configuration node's place, each feature's, and the Parts node's.
struct Layout {
    inputs: Vec2,
    features: HashMap<FeatureId, Vec2>,
    parts: Vec2,
}

/// Where each node goes: its saved place, or laid out in layers (Sugiyama's way, roughly): a
/// column by how deep its references go (a sketch with none in the first column, its extrude in
/// the next…), each column ordered by where its nodes' references are (the mean height of
/// their parents, so wires cross less; list order breaks ties) and stacked. The Parts node is
/// right of them all.
fn layout(el: &Element, measured: &HashMap<FeatureId, f32>) -> Layout {
    let saved = el.node_layout();
    let features = el.features();
    let collapsed = |f: FeatureId| saved.is_some_and(|l| l.collapsed.contains(&f));
    let parents: Vec<Vec<FeatureId>> = features
        .iter()
        .map(|f| cadrs_core::feature_list::parents(features, f.id))
        .collect();
    let mut depth: HashMap<FeatureId, usize> = HashMap::new();
    let mut columns: Vec<Vec<usize>> = Vec::new();
    for (i, f) in features.iter().enumerate() {
        let d = parents[i]
            .iter()
            .filter_map(|p| depth.get(p))
            .max()
            .map_or(0, |d| d + 1);
        depth.insert(f.id, d);
        if columns.len() <= d {
            columns.resize(d + 1, Vec::new());
        }
        columns[d].push(i);
    }
    let mut out: HashMap<FeatureId, Vec2> = HashMap::new();
    // The middle of each node placed so far, for ordering the next column.
    let mut middle: HashMap<FeatureId, f32> = HashMap::new();
    let mut right = 0.0f32;
    // Where the next column starts: a column taller than TALL wraps into several side by side.
    let mut x = COLUMN;
    for (d, column) in columns.iter_mut().enumerate() {
        let key = |i: &usize| -> f32 {
            let ys: Vec<f32> = parents[*i]
                .iter()
                .filter_map(|p| middle.get(p).copied())
                .collect();
            if ys.is_empty() {
                f32::MAX
            } else {
                ys.iter().sum::<f32>() / ys.len() as f32
            }
        };
        if d > 0 {
            column.sort_by(|a, b| key(a).total_cmp(&key(b)).then(a.cmp(b)));
        }
        let mut y = 0.0;
        for &i in column.iter() {
            let f = &features[i];
            // Its height as laid out, once it has been; a guess before.
            let h = measured
                .get(&f.id)
                .copied()
                .unwrap_or_else(|| height_guess(f, collapsed(f.id)));
            if y > 0.0 && y + h > TALL {
                x += COLUMN;
                y = 0.0;
            }
            let auto = Vec2::new(x, y);
            y += h + 24.0;
            let pos = saved
                .and_then(|l| l.positions.get(&f.id))
                .map_or(auto, |p| Vec2::from_array(*p));
            middle.insert(f.id, pos.y + h / 2.0);
            right = right.max(pos.x + NODE_W);
            out.insert(f.id, pos);
        }
        x += COLUMN;
    }
    let inputs = saved
        .and_then(|l| l.inputs_node)
        .map_or(Vec2::ZERO, Vec2::from_array);
    Layout {
        inputs,
        features: out,
        parts: Vec2::new(right.max(COLUMN) + 90.0, 0.0),
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

/// The canvas's grid: the PCB Studio schematic's (`cadrs_eda::render::SchematicTheme`), its
/// faint lines and crosses on every fifth line; in light, crosses alone, darker.
fn canvas_grid() -> cadrs_ui::node_graph::CanvasGrid {
    let rgba = |c: [u8; 4]| Color::srgba_u8(c[0], c[1], c[2], c[3]);
    if cadrs_ui::dark_in_force() {
        let th = cadrs_eda::render::SchematicTheme::dark();
        cadrs_ui::node_graph::CanvasGrid {
            step: 20.0,
            line: rgba(th.grid_line),
            mark: rgba(th.grid),
        }
    } else {
        let th = cadrs_eda::render::SchematicTheme::default();
        cadrs_ui::node_graph::CanvasGrid {
            step: 20.0,
            line: Color::srgba_u8(0, 0, 0, 0x0c),
            mark: rgba(th.grid),
        }
    }
}

/// The canvas's colour: the dark schematic's ground in dark, the view's background in light.
fn canvas_background(t: &Theme) -> Color {
    if cadrs_ui::dark_in_force() {
        let g = cadrs_eda::render::SchematicTheme::dark().background;
        Color::srgba_u8(g[0], g[1], g[2], g[3])
    } else {
        t.viewport_background
    }
}

/// A cheap fingerprint of the theme (a scheme change rebuilds the nodes).
fn theme_key(t: &Theme) -> u64 {
    let c = t.foreground.to_srgba();
    ((c.red * 255.0) as u64) << 16 | ((c.green * 255.0) as u64) << 8 | (c.blue * 255.0) as u64
}

/// Builds, updates and removes the nodes and wires of the shown Part Studio.
fn sync_nodes(world: &mut World) {
    // The host of the active Part Studio, if its node view is on.
    let active = world
        .resource::<ActiveDocument>()
        .active_element()
        .map(|e| e.id);
    let on = active.filter(|e| world.resource::<NodeView>().is_on(*e));
    let mut q_host = world.query::<(Entity, &NodeViewHost)>();
    let host = on.and_then(|el| q_host.iter(world).find(|(_, h)| h.0 == el).map(|(e, _)| e));
    let mut q_canvas = world.query::<(Entity, &NodeViewCanvas, &NodeCanvas)>();
    let canvases: Vec<(Entity, ElementId, NodeCanvas)> =
        q_canvas.iter(world).map(|(e, c, n)| (e, c.0, *n)).collect();
    for (e, el, _) in &canvases {
        if Some(*el) != on || host.is_none() {
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
            let mut commands = world.commands();
            let e = NodeCanvasBuilder::new("node-canvas")
                .view(saved.unwrap_or_default())
                .grid(canvas_grid())
                .background(canvas_background(&theme))
                .spawn(&mut commands, host, &theme);
            commands.entity(e).insert(NodeViewCanvas(element));
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
    let doc = world.resource::<ActiveDocument>();
    let Some(el) = doc.doc.element(element).cloned() else {
        return;
    };
    let units = doc.doc.units;
    let edited = crate::variables_ui::edited_feature(world)
        .filter(|(e, _)| *e == element)
        .map(|(_, f)| f);
    let cache_errors: HashMap<FeatureId, String> = world.resource::<PartCache>().errors.clone();
    let suppressed = el.all_suppressed();
    let bar = el.rollback_index();
    let tk = theme_key(&theme);
    let Layout {
        inputs: inputs_pos,
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
        let key = NodeKey {
            feature: (!live).then(|| f.clone()),
            live,
            collapsed: collapsed(f.id),
            suppressed: suppressed.contains(&f.id),
            rolled_back: i >= bar,
            error: cache_errors.get(&f.id).cloned(),
            outputs: outputs.get(&f.id).cloned().unwrap_or_default(),
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
        let e = spawn_feature_node(world, &el, f, key, pos, canvas.world(), &theme, &units);
        keep.insert(f.id, e);
    }
    for (e, n) in &have {
        if keep.get(&n.feature) != Some(e) {
            changed = true;
            if let Ok(em) = world.get_entity_mut(*e) {
                em.despawn();
            }
        }
    }
    // The Configuration node.
    let table = world.resource::<NodeView>().tables.contains(&element);
    let ckey = (el.configuration().cloned(), units, tk, table);
    let mut q_cfg = world.query::<(Entity, &ConfigNode)>();
    let cfg_have: Vec<(Entity, ConfigNode)> =
        q_cfg.iter(world).map(|(e, c)| (e, c.clone())).collect();
    match cfg_have.first() {
        Some((e, c)) if c.key == ckey => {
            place(world, *e, inputs_pos);
        }
        _ => {
            for (e, _) in &cfg_have {
                world.entity_mut(*e).despawn();
            }
            spawn_config_node(
                world,
                element,
                &el,
                ckey,
                inputs_pos,
                canvas.world(),
                &theme,
            );
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
        Some((_, c)) if c.key == pkey => {}
        _ => {
            for (e, _) in &parts_have {
                world.entity_mut(*e).despawn();
            }
            spawn_parts_node(world, pkey, parts_pos, canvas.world(), &theme);
            changed = true;
        }
    }
    // The wires, again whenever a node or the list changed.
    let doc_changed = world.resource_ref::<ActiveDocument>().is_changed();
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

/// The canvas's corner buttons: Fit.
fn spawn_canvas_tools(world: &mut World, canvas: Entity, t: &Theme) {
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
    let card = {
        let cache = world.resource::<PartCache>();
        crate::extrude_dialog::static_card(t, el, cache, f)
            .or_else(|| crate::revolve_dialog::static_card(t, el, cache, f))
    }
    .unwrap_or_else(|| generic_card(f, units, t));
    let card = card
        .static_copy()
        .valid(valid)
        .title(&f.name)
        .header_color(if dim {
            Color::srgba_u8(0x80, 0x80, 0x80, 0x30)
        } else {
            category_color(f)
        });
    let card = world
        .spawn((StaticCopyRoot, StaticCard, card.build(t), ChildOf(root)))
        .id();
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
    let what = if key.outputs.is_empty() {
        match f.kind {
            FeatureKind::Sketch(_) => tr("node-view-output-sketch"),
            FeatureKind::Plane(_) => tr("node-view-output-plane"),
            _ => tr("node-view-output"),
        }
    } else {
        key.outputs.join(", ")
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
    // Sockets: geometry in on the header, Bypass under it, geometry out on the output row.
    let geo = world.spawn((
        SocketKey::GeoIn(f.id),
        Socket::bundle(
            format!("node-{slug}-geometry-in"),
            SocketSide::In,
            GEOMETRY,
            None,
            socket_color(GEOMETRY),
            t,
        ),
        ChildOf(root),
    ));
    let _ = geo;
    world.spawn((
        SocketKey::GeoOut(f.id),
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
            cadrs_ui::Tooltip::new(tr("node-view-bypass")),
            ChildOf(root),
        ))
        .id();
    if let Some(mut n) = world.get_mut::<Node>(bypass) {
        n.top = Val::Px(20.0);
    }
    if let Some(mut n) = world.get_mut::<Node>(root) {
        n.width = Val::Px(NODE_W);
    }
    // Geometry in a little higher than Bypass.
    if let Some(mut n) = world
        .query_filtered::<(&SocketKey, &mut Node), With<Socket>>()
        .iter_mut(world)
        .find(|(k, _)| **k == SocketKey::GeoIn(f.id))
        .map(|(_, n)| n)
    {
        n.top = Val::Px(4.0);
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
    // A Variable's value, out.
    if let FeatureKind::Variable(v) = &f.kind
        && let Some(row) = rows
            .iter()
            .find(|(n, _)| n.ends_with("value"))
            .map(|(_, e)| *e)
    {
        let ty = quantity_type(v.kind.quantity());
        world.spawn((
            SocketKey::Variable(v.name.clone()),
            Socket::bundle(
                format!("node-{slug}-out"),
                SocketSide::Out,
                ty,
                Some(row),
                socket_color(ty),
                t,
            ),
            ChildOf(root),
        ));
    }
    root
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

/// The wires between the shown nodes: geometry from each feature's references, values from the
/// expressions that name an input or a Variable, Bypass from suppression variables.
fn rebuild_wires(
    world: &mut World,
    el: &Element,
    parts: &[cadrs_core::Part],
    canvas: Entity,
    layer: Entity,
) {
    let mut q_wires = world.query::<(Entity, &ChildOf, &Wire)>();
    let old: Vec<Entity> = q_wires
        .iter(world)
        .filter(|(_, p, _)| p.parent() == layer)
        .map(|(e, ..)| e)
        .collect();
    for e in old {
        world.entity_mut(e).despawn();
    }
    let mut q_sock = world.query::<(Entity, &SocketKey)>();
    let sockets: HashMap<SocketKey, Entity> = q_sock
        .iter(world)
        .filter(|(e, _)| on_canvas(world, *e, canvas))
        .map(|(e, k)| (k.clone(), e))
        .collect();
    let features = el.features();
    let mut wires: Vec<(SocketKey, SocketKey, Color, bool)> = Vec::new();
    // Where a name comes from at a feature: the last Variable above it of that name, else the
    // configuration's input.
    let source = |name: &str, at: usize| -> Option<SocketKey> {
        if features[..at]
            .iter()
            .any(|g| matches!(&g.kind, FeatureKind::Variable(v) if v.name == name))
        {
            return Some(SocketKey::Variable(name.to_string()));
        }
        el.configuration()?
            .input_named(name)
            .map(|_| SocketKey::Input(name.to_string()))
    };
    for (i, f) in features.iter().enumerate() {
        for p in cadrs_core::feature_list::parents(features, f.id) {
            if let Some(pf) = el.feature(p) {
                wires.push((
                    SocketKey::GeoOut(p),
                    SocketKey::GeoIn(f.id),
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
    for (i, (a, b, color, dashed)) in wires.into_iter().enumerate() {
        let (Some(from), Some(to)) = (sockets.get(&a), sockets.get(&b)) else {
            continue;
        };
        world.spawn((
            Wire::bundle(
                format!("node-wire-{i}"),
                Wire {
                    from: *from,
                    to: *to,
                    color,
                    dashed,
                },
            ),
            ChildOf(layer),
        ));
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

/// What a Configuration node's control is for.
#[derive(Component, Debug, Clone)]
enum ConfigRole {
    Configure,
    PresetSelect(Vec<PresetId>),
    PresetName(PresetId),
    AddPreset,
    RemovePreset(PresetId),
    AddInput,
    InputName(InputId),
    InputKind(InputId),
    InputValue(InputId),
    RemoveInput(InputId),
    /// Shows or hides the presets table.
    Table,
    /// A preset's value of an input, in the table.
    PresetCell(PresetId, InputId),
}

/// The Part Studio a Configuration node's controls change.
#[derive(Component, Debug, Clone, Copy)]
struct ConfigElement(ElementId);

fn spawn_config_node(
    world: &mut World,
    element: ElementId,
    el: &Element,
    key: ConfigKey,
    pos: Vec2,
    parent: Entity,
    t: &Theme,
) {
    let config = key.0.clone();
    let units = key.1;
    let table = key.3;
    // The table adds a column a preset.
    let width = CONFIG_W
        + if table {
            config
                .as_ref()
                .map_or(0.0, |c| c.presets.len() as f32 * TABLE_COLUMN)
        } else {
            0.0
        };
    let root = world
        .spawn((
            GraphNode::bundle("node-configuration", pos),
            ConfigNode { key },
            ConfigElement(element),
            ChildOf(parent),
        ))
        .id();
    let tb = t.clone();
    let rows: Vec<(InputId, String)> = config
        .as_ref()
        .map(|c| c.inputs.iter().map(|i| (i.id, i.name.clone())).collect())
        .unwrap_or_default();
    let _ = el;
    let card = FeatureDialog::new("node-configuration-card")
        .static_copy()
        .width(width)
        .title(tr("node-view-configuration"))
        .header_color(config_color())
        .body(move |b| {
            let t = &tb;
            let Some(c) = config else {
                b.spawn((
                    ConfigRole::Configure,
                    cadrs_ui::Button::new("node-configure")
                        .label(tr("node-view-add-configuration"))
                        .small()
                        .outline()
                        .build(t),
                ));
                return;
            };
            let active = c.active_preset().map(|p| p.id);
            // Presets: the active one to pick, its name to edit, add and remove.
            let ids: Vec<PresetId> = c.presets.iter().map(|p| p.id).collect();
            let mut sel = Select::new("node-preset").width(Val::Percent(100.0));
            for p in &c.presets {
                sel = sel.option(p.name.clone(), true);
            }
            let at = ids.iter().position(|p| Some(*p) == active).unwrap_or(0);
            b.spawn(row()).with_children(|r| {
                r.spawn((
                    t.text(
                        tr("node-view-preset"),
                        t.font_sm,
                        bevy::text::FontWeight::MEDIUM,
                        t.muted_foreground,
                    ),
                    Node {
                        width: Val::Px(52.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
                r.spawn((
                    ConfigRole::PresetSelect(ids.clone()),
                    sel.selected(at).build(t),
                ))
                .entry::<Node>()
                .and_modify(|mut n| n.flex_grow = 1.0);
                r.spawn((
                    ConfigRole::AddPreset,
                    IconButton::new("node-preset-add", "plus")
                        .icon_size(14.0)
                        .tooltip(tr("node-view-add-preset"))
                        .build(t),
                ));
                if let Some(p) = active
                    && c.presets.len() > 1
                {
                    r.spawn((
                        ConfigRole::RemovePreset(p),
                        IconButton::new("node-preset-remove", "close")
                            .icon_size(14.0)
                            .tooltip(tr("node-view-remove-preset"))
                            .build(t),
                    ));
                }
            });
            if let Some(p) = c.active_preset() {
                b.spawn(row()).with_children(|r| {
                    r.spawn((
                        t.text(
                            tr("node-view-name"),
                            t.font_sm,
                            bevy::text::FontWeight::MEDIUM,
                            t.muted_foreground,
                        ),
                        Node {
                            width: Val::Px(52.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    r.spawn((
                        ConfigRole::PresetName(p.id),
                        TextInput::new("node-preset-name")
                            .value(p.name.clone())
                            .width(Val::Percent(100.0))
                            .height(22.0)
                            .select_all_on_focus()
                            .build(t),
                    ))
                    .entry::<Node>()
                    .and_modify(|mut n| n.flex_grow = 1.0);
                });
            }
            b.spawn((
                Node {
                    height: Val::Px(1.0),
                    margin: UiRect::vertical(Val::Px(4.0)),
                    ..default()
                },
                BackgroundColor(t.separator),
            ));
            // Inputs: name, kind, value (the active preset's), remove.
            for i in &c.inputs {
                let value = c.active_preset().map_or(i.default, |p| p.value(i));
                let id = i.id;
                let slug = id.0.simple().to_string()[..8].to_string();
                b.spawn((Name::new(format!("node-input-{}", i.name)), row()))
                    .with_children(|r| {
                        r.spawn((
                            ConfigRole::InputName(id),
                            TextInput::new(format!("node-input-{slug}-name"))
                                .value(i.name.clone())
                                .width(Val::Px(70.0))
                                .height(22.0)
                                .select_all_on_focus()
                                .build(t),
                        ));
                        let mut kind =
                            Select::new(format!("node-input-{slug}-kind")).width(Val::Px(66.0));
                        for k in InputKind::ALL {
                            kind = kind.option(tr(kind_key(k)), true);
                        }
                        let ki = InputKind::ALL
                            .iter()
                            .position(|k| *k == i.kind)
                            .unwrap_or(0);
                        r.spawn((ConfigRole::InputKind(id), kind.selected(ki).build(t)));
                        if i.kind == InputKind::Boolean {
                            r.spawn((
                                ConfigRole::InputValue(id),
                                cadrs_ui::Checkbox::new(format!("node-input-{slug}-value"))
                                    .checked(value != 0.0)
                                    .build(t),
                            ));
                            r.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                        } else {
                            r.spawn((
                                ConfigRole::InputValue(id),
                                NumberField::new(format!("node-input-{slug}-value"), "")
                                    .text(units.with_unit(value, i.kind.quantity()))
                                    .label_width(0.0)
                                    .build(t),
                            ))
                            .entry::<Node>()
                            .and_modify(|mut n| {
                                n.flex_grow = 1.0;
                                n.min_width = Val::Px(0.0);
                            });
                        }
                        r.spawn((
                            ConfigRole::RemoveInput(id),
                            IconButton::new(format!("node-input-{slug}-remove"), "close")
                                .icon_size(12.0)
                                .tooltip(tr("node-view-remove-input"))
                                .build(t),
                        ));
                    });
            }
            b.spawn((
                ConfigRole::AddInput,
                cadrs_ui::Button::new("node-input-add")
                    .label(tr("node-view-add-input"))
                    .small()
                    .ghost()
                    .build(t),
            ));
            // The presets table: what each preset sets each input to (Onshape's
            // configuration table), every value editable in place.
            b.spawn((
                ConfigRole::Table,
                cadrs_ui::Button::new("node-presets-table")
                    .label(tr(if table {
                        "node-view-hide-table"
                    } else {
                        "node-view-show-table"
                    }))
                    .small()
                    .ghost()
                    .build(t),
            ));
            if table && !c.inputs.is_empty() {
                presets_table(b, t, &c, &units);
            }
        });
    let card = world
        .spawn((card.build(t), ConfigElement(element), ChildOf(root)))
        .id();
    if let Some(h) = world.get::<Children>(card).and_then(|c| c.first().copied()) {
        world.entity_mut(h).insert(GraphNodeHandle);
    }
    // An output socket for each input, level with its row.
    let named = named_descendants(world, card);
    let kinds: HashMap<InputId, InputKind> = world
        .get::<ConfigNode>(root)
        .and_then(|c| c.key.0.as_ref())
        .map(|c| c.inputs.iter().map(|i| (i.id, i.kind)).collect())
        .unwrap_or_default();
    for (id, name) in rows {
        let want = squash(&format!("node-input-{name}"));
        let Some(row) = named.iter().find(|(n, _)| *n == want).map(|(_, e)| *e) else {
            continue;
        };
        let ty = input_type(kinds.get(&id).copied().unwrap_or_default());
        world.spawn((
            SocketKey::Input(name.clone()),
            Socket::bundle(
                format!("node-input-{name}-out"),
                SocketSide::Out,
                ty,
                Some(row),
                socket_color(ty),
                t,
            ),
            ChildOf(root),
        ));
    }
    if let Some(mut n) = world.get_mut::<Node>(root) {
        n.width = Val::Px(width);
    }
}

/// A presets table's column width (px).
const TABLE_COLUMN: f32 = 76.0;

/// The presets table: a header of preset names, then a row an input with its value in each.
fn presets_table(b: &mut ChildSpawner, t: &Theme, c: &Configuration, units: &Units) {
    let cell = |n: &mut Node| {
        n.width = Val::Px(TABLE_COLUMN - 4.0);
        n.flex_shrink = 0.0;
    };
    b.spawn((
        Name::new("node-presets-table"),
        Node {
            flex_direction: FlexDirection::Column,
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ))
    .with_children(|tbl| {
        tbl.spawn(row()).with_children(|r| {
            r.spawn(Node {
                width: Val::Px(80.0),
                flex_shrink: 0.0,
                ..default()
            });
            for p in &c.presets {
                let bold = c.active_preset().is_some_and(|a| a.id == p.id);
                r.spawn(t.text(
                    p.name.clone(),
                    t.font_sm,
                    if bold {
                        bevy::text::FontWeight::BOLD
                    } else {
                        bevy::text::FontWeight::MEDIUM
                    },
                    t.muted_foreground,
                ))
                .insert(Node::default())
                .entry::<Node>()
                .and_modify(|mut n| cell(&mut n));
            }
        });
        for i in &c.inputs {
            tbl.spawn(row()).with_children(|r| {
                r.spawn(t.text(
                    format!("#{}", i.name),
                    t.font_sm,
                    bevy::text::FontWeight::MEDIUM,
                    t.foreground,
                ))
                .insert(Node {
                    width: Val::Px(80.0),
                    flex_shrink: 0.0,
                    overflow: Overflow::clip(),
                    ..default()
                });
                for p in &c.presets {
                    let v = p.value(i);
                    let name = format!("node-cell-{}-{}", squash(&p.name), i.name);
                    let role = ConfigRole::PresetCell(p.id, i.id);
                    if i.kind == InputKind::Boolean {
                        r.spawn((
                            role,
                            cadrs_ui::Checkbox::new(name).checked(v != 0.0).build(t),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| cell(&mut n));
                    } else {
                        r.spawn((
                            role,
                            NumberField::new(name, "")
                                .text(units.with_unit(v, i.kind.quantity()))
                                .label_width(0.0)
                                .build(t),
                        ))
                        .entry::<Node>()
                        .and_modify(|mut n| cell(&mut n));
                    }
                }
            });
        }
    });
}

fn kind_key(k: InputKind) -> &'static str {
    match k {
        InputKind::Length => "node-view-kind-length",
        InputKind::Angle => "node-view-kind-angle",
        InputKind::Number => "node-view-kind-number",
        InputKind::Boolean => "node-view-kind-boolean",
    }
}

/// A row of controls in the Configuration node.
fn row() -> Node {
    Node {
        height: Val::Px(28.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(4.0),
        flex_shrink: 0.0,
        ..default()
    }
}

/// The Configuration node's controls' role, and its Part Studio, for an entity in them.
fn config_role(world: &World, e: Entity) -> Option<(ElementId, ConfigRole)> {
    let mut cur = e;
    let mut role = None;
    for _ in 0..12 {
        if role.is_none() {
            role = world.get::<ConfigRole>(cur).cloned();
        }
        if let Some(el) = world.get::<ConfigElement>(cur) {
            return role.map(|r| (el.0, r));
        }
        cur = world.get::<ChildOf>(cur)?.parent();
    }
    None
}

fn run(world: &mut World, cmd: &dyn cadrs_core::Command) {
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = d.execute(cmd)
    {
        toast(world, e.to_string());
    }
}

fn toast(world: &mut World, why: String) {
    let t = world.resource::<Theme>().clone();
    cadrs_ui::show_toast(&mut world.commands(), &t, why);
}

fn config_of(world: &World, element: ElementId) -> Option<Configuration> {
    world
        .resource::<ActiveDocument>()
        .doc
        .element(element)?
        .configuration()
        .cloned()
}

fn on_config_button(a: On<Activate>, mut commands: Commands) {
    let e = a.entity;
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        match role {
            ConfigRole::Configure => run(
                world,
                &cfg::SetConfigured {
                    element,
                    configured: true,
                },
            ),
            ConfigRole::AddPreset => {
                let Some(c) = config_of(world, element) else {
                    return;
                };
                let name = (c.presets.len() + 1..)
                    .map(|n| format!("Preset {n}"))
                    .find(|n| c.preset_named(n).is_none())
                    .unwrap_or_default();
                run(
                    world,
                    &AddPreset {
                        element,
                        preset: PresetId::new(),
                        name,
                        copy_of: None,
                    },
                );
            }
            ConfigRole::RemovePreset(preset) => run(world, &RemovePreset { element, preset }),
            ConfigRole::AddInput => {
                let taken = world
                    .resource::<ActiveDocument>()
                    .doc
                    .element(element)
                    .map(cfg::taken_names)
                    .unwrap_or_default();
                let c = config_of(world, element).unwrap_or_default();
                let name = (1..)
                    .map(|n| format!("input{n}"))
                    .find(|n| c.name_problem(n, None, &taken).is_none())
                    .unwrap_or_default();
                run(
                    world,
                    &AddConfigInput {
                        element,
                        input: ConfigInput::new(name, InputKind::Length, 10.0),
                    },
                );
            }
            ConfigRole::RemoveInput(input) => run(world, &RemoveConfigInput { element, input }),
            ConfigRole::Table => {
                let mut v = world.resource_mut::<NodeView>();
                if !v.tables.remove(&element) {
                    v.tables.insert(element);
                }
            }
            _ => {}
        }
    });
}

fn on_config_number(ev: On<NumberFieldCommit>, mut commands: Commands) {
    let (e, text) = (ev.entity, ev.text.clone());
    commands.queue(move |world: &mut World| {
        // The active preset's value (the input's row), or a preset's (the table's cell).
        let (element, input, preset) = match config_role(world, e) {
            Some((el, ConfigRole::InputValue(i))) => (el, i, None),
            Some((el, ConfigRole::PresetCell(p, i))) => (el, i, Some(p)),
            _ => return,
        };
        let Some(c) = config_of(world, element) else {
            return;
        };
        let p = match preset {
            Some(id) => c.preset(id),
            None => c.active_preset(),
        };
        let (Some(i), Some(p)) = (c.input(input), p) else {
            return;
        };
        let units = world.resource::<ActiveDocument>().doc.units;
        match units.eval(&text, i.kind.quantity()) {
            Ok(v) => run(
                world,
                &SetPresetValue {
                    element,
                    preset: p.id,
                    input,
                    value: Some(v),
                },
            ),
            Err(err) => {
                toast(world, err.to_string());
                // Shown again as it was.
                world.resource_mut::<ActiveDocument>().set_changed();
            }
        }
    });
}

fn on_config_checkbox(ev: On<CheckboxChange>, mut commands: Commands) {
    let (e, checked) = (ev.entity, ev.checked);
    commands.queue(move |world: &mut World| {
        // The active preset's value (the input's row), or a preset's (the table's cell).
        let (element, input, preset) = match config_role(world, e) {
            Some((el, ConfigRole::InputValue(i))) => (el, i, None),
            Some((el, ConfigRole::PresetCell(p, i))) => (el, i, Some(p)),
            _ => return,
        };
        let Some(p) = preset
            .or_else(|| config_of(world, element).and_then(|c| c.active_preset().map(|p| p.id)))
        else {
            return;
        };
        run(
            world,
            &SetPresetValue {
                element,
                preset: p,
                input,
                value: Some(if checked { 1.0 } else { 0.0 }),
            },
        );
    });
}

fn on_config_select(ev: On<SelectChange>, mut commands: Commands) {
    let (e, index) = (ev.entity, ev.index);
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        match role {
            ConfigRole::PresetSelect(ids) => {
                if let Some(preset) = ids.get(index).copied() {
                    run(world, &SetActivePreset { element, preset });
                }
            }
            ConfigRole::InputKind(input) => {
                let Some(mut i) = config_of(world, element).and_then(|c| c.input(input).cloned())
                else {
                    return;
                };
                let Some(k) = InputKind::ALL.get(index).copied() else {
                    return;
                };
                if k != i.kind {
                    i.kind = k;
                    if k == InputKind::Boolean {
                        i.default = if i.default != 0.0 { 1.0 } else { 0.0 };
                    }
                    run(world, &EditConfigInput { element, input: i });
                }
            }
            _ => {}
        }
    });
}

fn on_config_text(ev: On<TextSubmit>, mut commands: Commands) {
    let (e, value) = (ev.entity, ev.value.trim().to_string());
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        match role {
            ConfigRole::PresetName(preset) => run(
                world,
                &RenamePreset {
                    element,
                    preset,
                    name: value,
                },
            ),
            ConfigRole::InputName(input) => {
                let Some(mut i) = config_of(world, element).and_then(|c| c.input(input).cloned())
                else {
                    return;
                };
                if i.name != value {
                    i.name = value;
                    run(world, &EditConfigInput { element, input: i });
                    // A refused name shows the old one again.
                    world.resource_mut::<ActiveDocument>().set_changed();
                }
            }
            _ => {}
        }
    });
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
    q_config: Query<(), With<ConfigNode>>,
    mut moves: ResMut<PendingMoves>,
) {
    if let Ok(n) = q_feature.get(ev.entity) {
        moves.0.push((Some(n.feature), ev.pos));
    } else if q_config.contains(ev.entity) {
        moves.0.push((None, ev.pos));
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
                .filter_map(|(f, p)| f.map(|f| (f, round(*p))))
                .collect(),
            inputs_node: moves
                .iter()
                .find(|(f, _)| f.is_none())
                .map(|(_, p)| round(*p)),
        },
    );
}

/// The feature whose input or Bypass socket `e` is, and the field.
fn target_of(world: &World, e: Entity) -> Option<SocketKey> {
    world.get::<SocketKey>(e).cloned()
}

fn on_wire_connect(ev: On<WireConnect>, mut commands: Commands) {
    let (from, to) = (ev.from, ev.to);
    commands.queue(move |world: &mut World| {
        let (Some(src), Some(dst)) = (target_of(world, from), target_of(world, to)) else {
            return;
        };
        let name = match src {
            SocketKey::Input(n) | SocketKey::Variable(n) => n,
            // Geometry wires follow the features' references: picked in the view.
            _ => {
                world.resource_mut::<ActiveDocument>().set_changed();
                return;
            }
        };
        let Some(element) = world
            .resource::<ActiveDocument>()
            .active_element()
            .map(|e| e.id)
        else {
            return;
        };
        match dst {
            SocketKey::Slot(feature, slot) => run(
                world,
                &ConnectParameter {
                    element,
                    feature,
                    slot: slot.to_string(),
                    expr: format!("#{name}"),
                },
            ),
            SocketKey::Bypass(feature) => run(
                world,
                &cadrs_core::commands::SetSuppressByVariable {
                    element,
                    feature,
                    rule: Some(cadrs_core::variables::SuppressByVariable::variable(&name)),
                },
            ),
            _ => {}
        }
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
        match dst {
            SocketKey::Slot(feature, slot) => run(
                world,
                &DisconnectParameter {
                    element,
                    feature,
                    slot: slot.to_string(),
                },
            ),
            SocketKey::Bypass(feature) => run(
                world,
                &cadrs_core::commands::SetSuppressByVariable {
                    element,
                    feature,
                    rule: None,
                },
            ),
            _ => {}
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
    let Some((element, feature)) = crate::variables_ui::edited_feature(world) else {
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
    let mut q_area = world.query_filtered::<Entity, With<ViewportArea>>();
    let Some(area) = q_area.iter(world).next() else {
        return;
    };
    let mut q_dialog = world.query_filtered::<(Entity, &ChildOf), With<FeatureDialogState>>();
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

/// The features header's preset dropdown: which presets it lists and which is active.
#[derive(Component, Debug, Clone, PartialEq)]
struct HeaderPreset {
    element: ElementId,
    presets: Vec<(PresetId, String)>,
    active: PresetId,
}

/// Shows the active Part Studio's presets in the features header (in the list and the node
/// view alike), with the active one chosen; none without a configuration.
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
        Some(HeaderPreset {
            element: e.id,
            presets: c.presets.iter().map(|p| (p.id, p.name.clone())).collect(),
            active: c.active_preset()?.id,
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
    for (_, name) in &want.presets {
        sel = sel.option(name.clone(), true);
    }
    let at = want
        .presets
        .iter()
        .position(|(id, _)| *id == want.active)
        .unwrap_or(0);
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
    let (element, preset) = (h.element, h.presets.get(ev.index).map(|p| p.0));
    if let Some(preset) = preset {
        commands.queue(move |world: &mut World| {
            run(world, &SetActivePreset { element, preset });
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
/// The room kept round a node (graph px).
const CLEAR: f32 = 8.0;
/// How far a wire runs straight out of its output and into its input (graph px).
const STUB: f32 = 18.0;

/// Routes the node view's wires round its nodes with the PCB autorouter's search
/// (`cadrs_router::search`: A* over octilinear steps, a cell's cost raised by the wires already
/// through it, as its negotiated congestion does): the nodes, grown by [`CLEAR`], are walls; each
/// wire leaves its output and enters its input straight ([`STUB`]); a route is then pulled
/// straight between the points that can see each other. A wire with no way round is a curve.
fn route_node_wires(p: &cadrs_ui::node_graph::RouteProblem) -> Vec<Option<Vec<Vec2>>> {
    use cadrs_router::grid::{Grid, Sizes};
    use cadrs_router::search::{Costs, Searcher};
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
    let (nx, ny) = (
        (b.width() / PITCH).ceil() as u32 + 1,
        (b.height() / PITCH).ceil() as u32 + 1,
    );
    if (nx as u64) * (ny as u64) > 3_000_000 {
        return vec![None; p.wires.len()];
    }
    let cell = |v: Vec2| -> (u32, u32) {
        let c = ((v - b.min) / PITCH).floor();
        (
            (c.x.max(0.0) as u32).min(nx - 1),
            (c.y.max(0.0) as u32).min(ny - 1),
        )
    };
    let centre = |x: u32, y: u32| b.min + (Vec2::new(x as f32, y as f32) + 0.5) * PITCH;
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
    let blocked = |v: Vec2| -> bool {
        let (x, y) = cell(v);
        walls[(y * nx + x) as usize] == WALL
    };
    // Whether the straight line from `a` to `b` stays off the walls.
    let clear = |a: Vec2, b: Vec2| -> bool {
        let n = (a.distance(b) / (PITCH / 3.0)).ceil().max(1.0) as usize;
        (0..=n).all(|i| !blocked(a.lerp(b, i as f32 / n as f32)))
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
    let mut occ = vec![0u16; (nx * ny) as usize];
    let hist = vec![0u32; (nx * ny) as usize];
    let mut searcher = Searcher::default();
    // Short wires first: they have the least choice.
    let mut order: Vec<usize> = (0..p.wires.len()).collect();
    order.sort_by(|i, j| {
        let d = |k: usize| p.wires[k].0.distance(p.wires[k].1);
        d(*i).total_cmp(&d(*j))
    });
    let mut out = vec![None; p.wires.len()];
    for k in order {
        let (a, z) = p.wires[k];
        let (s, t) = (a + Vec2::X * STUB, z - Vec2::X * STUB);
        let (cs, ct) = (cell(s), cell(t));
        let (is, it) = (
            grid.params.index(cs.0, cs.1, 0),
            grid.params.index(ct.0, ct.1, 0),
        );
        // The stubs' ends may sit in a neighbour's clearance: open them for this wire.
        let keep = (grid.owner[is as usize], grid.owner[it as usize]);
        grid.owner[is as usize] = FREE;
        grid.owner[it as usize] = FREE;
        let path = {
            let costs = Costs {
                grid: &grid,
                occ: &occ,
                hist: &hist,
                // Dear enough that wires sharing a channel take lanes of their own.
                present: 40,
            };
            searcher.search(&costs, 1, &[is], &[it])
        };
        grid.owner[is as usize] = keep.0;
        grid.owner[it as usize] = keep.1;
        let Some(path) = path else { continue };
        for c in &path {
            occ[*c as usize] = occ[*c as usize].saturating_add(1);
        }
        // The cells' centres, from the stub's end to the other's, pulled straight.
        let mut pts: Vec<Vec2> = path
            .iter()
            .map(|c| {
                let (x, y, _) = grid.params.coords(*c);
                centre(x, y)
            })
            .collect();
        if let Some(f) = pts.first_mut() {
            *f = s;
        }
        if let Some(l) = pts.last_mut() {
            *l = t;
        }
        let mut pulled = vec![pts[0]];
        let mut i = 0;
        while i + 1 < pts.len() {
            let mut j = pts.len() - 1;
            while j > i + 1 && !clear(pts[i], pts[j]) {
                j -= 1;
            }
            pulled.push(pts[j]);
            i = j;
        }
        out[k] = Some(pulled);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wires_go_round_nodes() {
        use cadrs_ui::node_graph::RouteProblem;
        // A node between the two ends: the wire can't go straight through it.
        let block = Rect::new(100.0, -100.0, 200.0, 100.0);
        let p = RouteProblem {
            obstacles: vec![block],
            wires: vec![(Vec2::new(0.0, 0.0), Vec2::new(300.0, 0.0))],
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
        // With nothing in the way it is short and straight-ish.
        let r = route_node_wires(&RouteProblem {
            obstacles: Vec::new(),
            wires: vec![(Vec2::new(0.0, 0.0), Vec2::new(300.0, 40.0))],
        });
        assert!(r[0].as_ref().unwrap().len() <= 3, "{:?}", r[0]);
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
        use cadrs_core::document::{ExtrudeFeature, RegionRef};
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
        let _ = RegionRef::new;
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

//! The Layout view's tools (docs/PLAN.md GS13–GS21, GS25), keys as in the guide:
//!
//! - **Select** (Esc): what is under the pointer is drawn lighter; click picks a footprint,
//!   track, via, zone, drawn shape (a filled one anywhere inside it) or text (Shift adds); **E**
//!   or a double-click opens a footprint's properties (exact position, angle, side, 3D model
//!   shown), a drawn shape's (width, filled, net) or a text's (its words, height, stroke);
//!   dragging on empty board boxes — left to right takes what is wholly inside (a blue box),
//!   right to left what it touches (dashed yellow), shown selected as it is dragged (Shift
//!   adds); pressing on a footprint and dragging moves it (it lands where the button is let
//!   go), and so does pressing on a selected track, via, zone, shape or text, or on any of
//!   several selected items (they all move); a selected zone or
//!   polygon (or one under the pointer) shows its corners as white handles, the one under the
//!   pointer blue, and pressing on one drags that corner; **M** moves
//!   the selected footprint (a click puts it down), **D** drags it with its tracks attached,
//!   **R** rotates it, **F** flips it to the other side (F with nothing selected fits the
//!   view); R and F turn and flip any selection (zones, shapes, text, tracks) about its
//!   middle; **U** selects the connection's tracks (again: through vias), a double-click on a
//!   track or via every track and via of its net; pressing on a track drags it (the
//!   tracks joined to it stretch, an end on a pad or via stays joined by a new short track),
//!   near one of its ends that end alone; on a via, the via with
//!   the track ends on it; **Del** deletes.
//! - **X** routes from the pointer on the active layer (a bottom pad switches to the bottom);
//!   clicks add 45° runs, **V** drops a via and changes layer, a click on a pad of the net or a
//!   double-click ends. **PgUp** / **PgDn** pick the top / bottom copper.
//! - The strip: Update PCB from schematic (**F8**), Board setup, Board outline (two corners on
//!   a 1 mm grid), Route, Auto-route (the unconnected nets, off the main thread, put down as one
//!   undo step: `cadrs_router`), Add zone (**Z**: its corners clicked straight away on the active
//!   copper layer, on no net until copper of a net is wired to it, which it then takes), Add
//!   keep-out (its layers and what it forbids asked first, then corners), Draw lines / a
//!   rectangle / a circle, a filled Polygon (a copper area on a copper layer, on the net it
//!   starts on: clicks add corners) and Text on the drawing layer (picked in the layers panel),
//!   DRC, Plot. A zone, keep-out or polygon ends with **Enter**, a
//!   right-click or a double-click. Zones are poured again with every change to the board
//!   (in the same undo step), so they always show filled.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_eda::Design;
use cadrs_eda::board_edit::{self as be, BoardItem};
use cadrs_eda::layer::Layer;
use cadrs_eda::units::{Nm, Pt, mm};
use cadrs_ui::ShortcutKey;

use super::ui::{self, StripAction};
use super::{EdaClick, EdaPointer, Mode, Preview, SceneInputs};
use crate::AppState;

const STRIP: ui::StripSpec = &[
    Some(("pcb-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some((
        "pcb-update",
        "update-pcb",
        "Update PCB from schematic (F8)",
        "update",
    )),
    Some(("pcb-board-setup", "settings", "Board setup", "setup")),
    Some((
        "pcb-outline",
        "board-outline",
        "Draw the board outline",
        "outline",
    )),
    Some(("pcb-route", "route-track", "Route tracks (X)", "route")),
    Some((
        "pcb-autoroute",
        "ratsnest",
        "Auto-route the unconnected nets",
        "autoroute",
    )),
    Some(("pcb-zone", "copper-zone", "Add a filled zone (Z)", "zone")),
    Some(("pcb-keepout", "keepout-zone", "Add a keep-out", "keepout")),
    Some((
        "pcb-draw-line",
        "line",
        "Draw lines on the drawing layer",
        "line",
    )),
    Some((
        "pcb-draw-rect",
        "corner-rectangle",
        "Draw a rectangle on the drawing layer",
        "rect",
    )),
    Some((
        "pcb-draw-circle",
        "center-circle",
        "Draw a circle on the drawing layer",
        "circle",
    )),
    Some((
        "pcb-draw-polygon",
        "inscribed-polygon",
        "Draw a filled polygon on the drawing layer (a copper area on a copper layer)",
        "polygon",
    )),
    Some(("pcb-text", "text", "Add text on the drawing layer", "text")),
    Some((
        "pcb-graphic",
        "image",
        "Import a graphic (SVG) onto the drawing layer",
        "graphic",
    )),
    Some(("pcb-measure", "ruler", "Measure (Ctrl+Shift+M)", "measure")),
    Some((
        "pcb-layer",
        "layers",
        "Switch the active layer (front / back)",
        "layer",
    )),
    None,
    Some(("pcb-drc", "drc", "Design rules checker", "drc")),
    Some((
        "pcb-plot",
        "plot-gerbers",
        "Plot fabrication outputs",
        "plot",
    )),
];

#[derive(Clone, Debug, Default)]
pub enum Tool {
    #[default]
    Select,
    /// The outline's first corner, once clicked.
    Outline(Option<Pt>),
    /// A route: its points so far on each layer run, the layer now, its net.
    Route {
        runs: Vec<(Layer, Vec<Pt>)>,
        vias: Vec<Pt>,
        net: String,
    },
    /// A zone of `net` on `layer`: its corners so far.
    Zone {
        net: String,
        layer: Layer,
        pts: Vec<Pt>,
    },
    /// A line, rectangle or circle on the drawing layer: its first point once clicked.
    Draw { kind: DrawKind, start: Option<Pt> },
    /// A filled polygon on the drawing layer: its corners so far (a double-click ends).
    Polygon(Vec<Pt>),
    /// Text, placed on the drawing layer with the next click.
    Text(String),
    /// Imported artwork (filled polygons, mm, its top-left corner at the origin), placed on the
    /// drawing layer with the next click.
    Graphic(Vec<Vec<Pt>>),
    /// A keep-out on `layers` forbidding what `rules` says: its corners so far.
    Keepout {
        layers: cadrs_eda::layer::LayerSet,
        rules: cadrs_eda::board::Keepout,
        pts: Vec<Pt>,
    },
    /// The ruler: its start, and its end once a second click froze it.
    Measure { a: Option<Pt>, b: Option<Pt> },
}

/// What the Draw tools make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DrawKind {
    Line,
    Rect,
    Circle,
}

impl Tool {
    fn action(&self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Outline(_) => "outline",
            Tool::Route { .. } => "route",
            Tool::Zone { .. } => "zone",
            Tool::Draw {
                kind: DrawKind::Line,
                ..
            } => "line",
            Tool::Draw {
                kind: DrawKind::Rect,
                ..
            } => "rect",
            Tool::Draw {
                kind: DrawKind::Circle,
                ..
            } => "circle",
            Tool::Polygon(_) => "polygon",
            Tool::Text(_) => "text",
            Tool::Graphic(_) => "graphic",
            Tool::Keepout { .. } => "keepout",
            Tool::Measure { .. } => "measure",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Moving {
    pub fp: uuid::Uuid,
    pub from: Pt,
    pub drag: bool,
    /// Started by pressing on the footprint: it lands when the button is let go (M and D: on
    /// the next click).
    pub on_release: bool,
    /// What is moved instead of the one footprint: a selection (several items, or a track,
    /// via, zone, shape or text); with `corner`, only that corner of the first (a zone or
    /// shape: ring, index).
    pub items: Vec<BoardItem>,
    pub corner: Option<(usize, usize)>,
    /// A track end or corner, or a via, being dragged: its net, and the points that move.
    pub points: Option<(String, Vec<Pt>)>,
    /// A track being dragged whole ([`be::drag_track`]: what joins it follows).
    pub track: Option<uuid::Uuid>,
}

#[derive(Resource, Debug)]
pub struct LayoutState {
    /// A board edited in a 3D view: the side it is seen from (its footprints are picked first).
    pub facing: Option<cadrs_eda::layer::Side>,
    /// A board edited in a 3D view: the footprint whose model the next press is on (the press
    /// is where the pointer meets the board, perhaps beside the footprint's pads); taken by it.
    pub pick: Option<uuid::Uuid>,
    pub tool: Tool,
    pub selection: Vec<BoardItem>,
    /// What is under the pointer (Select), drawn lighter; a zone or polygon shows its corners.
    pub hover: Option<BoardItem>,
    /// The corner of the hovered or selected zone or polygon under the pointer (item, ring,
    /// index, where): a press drags it.
    pub hot_corner: Option<(BoardItem, usize, usize, Pt)>,
    /// The corner handles' half size (a few pixels at the view's zoom).
    pub handle: Nm,
    /// A box selection started here (a press on empty board), and what it would take now.
    pub boxing: Option<Pt>,
    pub box_preview: Vec<BoardItem>,
    pub moving: Option<Moving>,
    pub active: Layer,
    /// U pressed once already on this selection: the next U goes through vias.
    pub expanded: bool,
    /// Layers hidden in the layers panel.
    pub hidden: Vec<Layer>,
    /// Other layers than the active one drawn faint.
    pub dim: bool,
    /// The layers panel is open (else just its button).
    pub layers_open: bool,
    /// What the Draw and Text tools draw on (a click in the layers panel picks it).
    pub draw_layer: Layer,
    /// The net highlighted (` on its copper).
    pub net: Option<String>,
    /// The width new tracks get (W / Shift+W step it); `None`: the net class'.
    pub track_width: Option<Nm>,
    /// How the Text tool writes: height and stroke width (mm), alignment.
    pub text_style: TextLook,
    /// Where a moved footprint's grab point goes exactly, off the grid, this frame: set by a
    /// board edited in a Part Studio when the footprint snaps onto the parts around it
    /// (`crate::board_fit`).
    pub exact: Option<Pt>,
}

impl Default for LayoutState {
    fn default() -> Self {
        LayoutState {
            facing: None,
            pick: None,
            tool: Tool::Select,
            selection: vec![],
            hover: None,
            hot_corner: None,
            handle: mm(0.25),
            boxing: None,
            box_preview: vec![],
            moving: None,
            active: Layer::TopCopper,
            expanded: false,
            hidden: vec![],
            dim: false,
            layers_open: false,
            draw_layer: Layer::TopSilk,
            net: None,
            track_width: None,
            text_style: TextLook::default(),
            exact: None,
        }
    }
}

/// How the Text tool writes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextLook {
    pub height: f64,
    pub thickness: f64,
    pub align: cadrs_eda::graphics::HAlign,
}

impl Default for TextLook {
    fn default() -> Self {
        TextLook {
            height: 1.0,
            thickness: 0.15,
            align: cadrs_eda::graphics::HAlign::Center,
        }
    }
}

/// Writes text as the Text tool's style says.
fn add_text(d: &mut Design, text: &str, at: Pt, layer: Layer, look: TextLook) {
    let id = be::add_text(&mut d.board, text, at, layer);
    if let Some(t) = d.board.texts.iter_mut().find(|t| t.id == id) {
        t.text.style.size = cadrs_eda::units::Size::mm(look.height, look.height);
        t.text.style.thickness = Some(mm(look.thickness));
        t.text.style.h_align = look.align;
    }
}

/// An auto-route running off the main thread ([`cadrs_router`]): the board it started from.
#[derive(Resource)]
struct Autorouting {
    task: bevy::tasks::Task<cadrs_router::Outcome>,
    board: cadrs_eda::board::Board,
    started: web_time::Instant,
}

/// Auto-route: the shown board's unconnected nets routed off the main thread (the view stays
/// live); [`poll_autoroute`] puts the tracks and vias down as one undo step.
fn start_autoroute(w: &mut World) {
    if w.contains_resource::<Autorouting>() {
        ui::toast(w, "Already auto-routing");
        return;
    }
    let Some(d) = board_of(w) else { return };
    if cadrs_eda::copper::ratsnest(&d.board).is_empty() {
        ui::toast(w, "Nothing to route: every net is connected");
        return;
    }
    let board = d.board.clone();
    let routed = d.board;
    let task = bevy::tasks::AsyncComputeTaskPool::get()
        .spawn(async move { cadrs_router::route(&routed, &cadrs_router::Options::default()) });
    w.insert_resource(Autorouting {
        task,
        board,
        started: web_time::Instant::now(),
    });
    ui::toast(w, "Auto-routing…");
}

/// The auto-route's tracks and vias put down when it finishes (one undo step), with what it
/// left unrouted; not if the board changed meanwhile.
fn poll_autoroute(world: &mut World) {
    let Some(mut job) = world.get_resource_mut::<Autorouting>() else {
        return;
    };
    let Some(out) = bevy::tasks::futures::check_ready(&mut job.task) else {
        return;
    };
    let job = world.remove_resource::<Autorouting>().expect("just polled");
    if board_of(world).map(|d| d.board).as_ref() != Some(&job.board) {
        ui::toast(
            world,
            "The board changed while it was being auto-routed: nothing was put down (auto-route again)",
        );
        return;
    }
    let took = job.started.elapsed().as_secs_f64();
    if !out.tracks.is_empty() || !out.vias.is_empty() {
        ui::commit(world, "Auto-route", |d| {
            cadrs_router::apply(&mut d.board, &out);
            Ok(())
        });
    }
    let on = out
        .gpu
        .as_deref()
        .map_or_else(|| "the CPU".to_string(), |g| g.to_string());
    let mut msg = format!(
        "Auto-routed in {took:.1} s on {on}: {} tracks, {} vias",
        out.tracks.len(),
        out.vias.len()
    );
    if !out.unrouted.is_empty() {
        let shown: Vec<&str> = out.unrouted.iter().take(6).map(|s| s.as_str()).collect();
        msg.push_str(&format!(
            "; not fully routed: {}{}",
            shown.join(", "),
            if out.unrouted.len() > 6 { ", …" } else { "" }
        ));
    }
    ui::toast(world, &msg);
}

/// Import graphic: an SVG file, then its artwork follows the pointer to a click.
fn pick_graphic(w: &mut World) {
    let theme = w.resource::<cadrs_ui::prelude::Theme>().clone();
    let dir = std::env::current_dir().unwrap_or_default();
    let mut c = w.commands();
    cadrs_ui::file_picker::open_file_picker(
        &mut c,
        &theme,
        "eda-graphic-picker",
        "Import a graphic",
        "eda-graphic-file",
        dir,
        &["svg"],
    );
    w.flush();
}

fn on_graphic_picked(
    mut msgs: MessageReader<cadrs_ui::file_picker::FilePicked>,
    mut commands: Commands,
) {
    for m in msgs.read() {
        if m.tag != "eda-graphic-file" {
            continue;
        }
        let path = m.path.clone();
        commands.queue(move |w: &mut World| {
            let polys = cadrs_fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|t| cadrs_eda::graphic_import::svg_polygons(&t));
            match polys {
                Ok(p) => set_tool(
                    w,
                    Tool::Graphic(cadrs_eda::graphic_import::placed_at(&p, Pt::ZERO)),
                ),
                Err(e) => ui::toast(w, &format!("{}: {e}", path.display())),
            }
        });
    }
}
pub fn register(app: &mut App) {
    app.init_resource::<LayoutState>().add_systems(
        Update,
        (
            strip,
            on_strip,
            on_click,
            on_keys,
            on_graphic_picked,
            follow_pointer,
            track_hover,
            preview_box,
            poll_autoroute,
            publish,
            draw_box,
        )
            .chain()
            .after(super::navigate)
            .run_if(in_state(AppState::Document)),
    );
    super::lay_dialogs::register(app);
}

fn strip(world: &mut World, q: &mut QueryState<(Entity, &'static ui::ToolStrip)>) {
    ui::sync_strip(world, q, Mode::Layout, STRIP);
    let a = world.resource::<LayoutState>().tool.action();
    ui::mark_active(world, Mode::Layout, a);
}

fn shown(w: &World) -> bool {
    w.resource::<super::Eda2d>()
        .0
        .is_some_and(|(_, _, m)| m == Mode::Layout)
}

/// The routing grid.
fn grid() -> Nm {
    mm(0.5)
}

fn snap(p: Pt, g: Nm) -> Pt {
    cadrs_eda::sch_edit::snap(p, g)
}

fn tolerance(world: &World) -> Nm {
    let ui = world.resource::<super::EdaUi>();
    let key = world.resource::<super::Eda2d>().0;
    let scale = key
        .and_then(|k| ui.views.get(&k))
        .map_or(4.0, |v| v.0.scale);
    mm(4.0 / scale)
}

fn on_strip(mut actions: MessageReader<StripAction>, mut commands: Commands) {
    for a in actions.read() {
        if a.0 == Mode::Layout {
            let action = a.1;
            commands.queue(move |w: &mut World| run_action(w, action));
        }
    }
}

pub fn run_action(w: &mut World, action: &str) {
    match action {
        "select" => set_tool(w, Tool::Select),
        "update" => super::lay_dialogs::open_update(w),
        "setup" => super::lay_dialogs::open_setup(w),
        // A linked outline: what is drawn now is a cut-out in it (the sketch keeps the outer
        // edge; its shapes can't be changed here).
        "outline" if !linked_outline(w).is_empty() => {
            let note = format!(
                "{}. A rectangle drawn now cuts out of the board",
                outline_link_note(w)
            );
            ui::toast(w, &note);
            set_tool(w, Tool::Outline(None));
        }
        "outline" => set_tool(w, Tool::Outline(None)),
        "route" => {
            let at = w.resource::<EdaPointer>().at;
            start_route(w, at);
        }
        // Drawn straight away on the active copper layer, on no net: it takes the net of what
        // is wired to it.
        "autoroute" => start_autoroute(w),
        "zone" => {
            let layer = w.resource::<LayoutState>().active;
            set_tool(
                w,
                Tool::Zone {
                    net: String::new(),
                    layer,
                    pts: vec![],
                },
            );
        }
        "line" => set_tool(
            w,
            Tool::Draw {
                kind: DrawKind::Line,
                start: None,
            },
        ),
        "rect" => set_tool(
            w,
            Tool::Draw {
                kind: DrawKind::Rect,
                start: None,
            },
        ),
        "circle" => set_tool(
            w,
            Tool::Draw {
                kind: DrawKind::Circle,
                start: None,
            },
        ),
        "polygon" => set_tool(w, Tool::Polygon(vec![])),
        "text" => super::lay_dialogs::open_text(w),
        "graphic" => pick_graphic(w),
        "keepout" => super::lay_dialogs::open_keepout(w),
        "measure" => set_tool(w, Tool::Measure { a: None, b: None }),
        "layer" => {
            let mut s = w.resource_mut::<LayoutState>();
            s.active = if s.active == Layer::TopCopper {
                Layer::BottomCopper
            } else {
                Layer::TopCopper
            };
        }
        "drc" => super::lay_dialogs::open_drc(w),
        "plot" => super::lay_dialogs::open_plot(w),
        _ => {}
    }
}

/// The shown board's outline shapes a sketch drives, if its outline follows one.
fn linked_outline(w: &World) -> Vec<uuid::Uuid> {
    let Some((el, b, _)) = w.resource::<super::Eda2d>().board() else {
        return vec![];
    };
    let doc = w.resource::<crate::ActiveDocument>();
    doc.doc
        .element(el)
        .and_then(|e| e.pcb())
        .and_then(|s| s.board(b))
        .and_then(|b| b.outline_link.as_ref())
        .map(|l| l.shapes.clone())
        .unwrap_or_default()
}

/// "The outline follows Sketch 1 in Part Studio 1": edit it there, or break the link.
fn outline_link_note(w: &World) -> String {
    let Some((el, b, _)) = w.resource::<super::Eda2d>().board() else {
        return String::new();
    };
    let doc = &w.resource::<crate::ActiveDocument>().doc;
    let Some(link) = doc
        .element(el)
        .and_then(|e| e.pcb())
        .and_then(|s| s.board(b))
        .and_then(|b| b.outline_link.clone())
    else {
        return String::new();
    };
    let studio = doc.element(link.element);
    let sketch = studio
        .and_then(|e| e.feature(link.sketch))
        .map(|f| f.name.clone())
        .unwrap_or_default();
    format!(
        "The outline follows {sketch} in {}: edit the sketch, or break the link (the board's Derived feature's menu)",
        studio.map(|e| e.name.clone()).unwrap_or_default()
    )
}

pub fn set_tool(w: &mut World, tool: Tool) {
    let mut s = w.resource_mut::<LayoutState>();
    s.tool = tool;
    s.moving = None;
    s.boxing = None;
    s.box_preview.clear();
    w.resource_mut::<Preview>().set(None);
}

fn board_of(w: &World) -> Option<Design> {
    ui::current(w).map(|(_, _, d)| d)
}

/// A footprint of the shown board as stored (a copy of it alone).
fn board_of_footprint(w: &World, fp: uuid::Uuid) -> Option<cadrs_eda::board::PlacedFootprint> {
    let (el, b, _) = w.resource::<super::Eda2d>().board()?;
    super::design(w.resource::<crate::ActiveDocument>(), el, b)?
        .board
        .footprints
        .iter()
        .find(|f| f.id == fp)
        .cloned()
}

/// A point snapped to a pad's centre when on a pad, else to the routing grid.
fn snap_to_copper(d: &Design, p: Pt) -> Pt {
    match be::pad_at(&d.board, p) {
        Some((fi, pi)) => be::pad_center(&d.board, fi, pi),
        None => d
            .board
            .vias
            .iter()
            .find(|v| v.at.dist(p) <= v.diameter as f64 / 2.0)
            .map_or(snap(p, grid()), |v| v.at),
    }
}

fn start_route(w: &mut World, at: Pt) {
    let Some(d) = board_of(w) else { return };
    let start = snap_to_copper(&d, at);
    let active = w.resource::<LayoutState>().active;
    let layer = be::start_layer(&d.board, start, active);
    let net = be::net_at(&d.board, start, layer);
    w.resource_mut::<LayoutState>().active = layer;
    set_tool(
        w,
        Tool::Route {
            runs: vec![(layer, vec![start])],
            vias: vec![],
            net,
        },
    );
}

/// Lays the route as tracks and vias (one undo step).
fn finish_route(w: &mut World, runs: Vec<(Layer, Vec<Pt>)>, vias: Vec<Pt>, net: String) {
    let any = runs.iter().any(|(_, p)| p.len() >= 2);
    let width = w.resource::<LayoutState>().track_width;
    if any {
        ui::commit(w, "Route", |d| {
            for v in &vias {
                be::add_via(&mut d.board, *v, &net);
            }
            for (l, pts) in &runs {
                if pts.len() >= 2 {
                    let ids = be::route(&mut d.board, pts, *l, width);
                    // The route's net is the start's (a run starting at a via gets it too).
                    for t in d.board.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
                        t.net = net.clone();
                    }
                }
            }
            Ok(())
        });
    }
    set_tool(w, Tool::Select);
}

fn on_click(mut reader: MessageReader<EdaClick>, mut commands: Commands) {
    let clicks: Vec<EdaClick> = reader.read().copied().collect();
    if clicks.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        if !shown(world) {
            return;
        }
        for c in clicks {
            match c {
                EdaClick::Press { at, shift, .. } => press(world, at, shift),
                EdaClick::Double { at, shift } => double(world, at, shift),
                EdaClick::Release { at } => release(world, at),
                EdaClick::Secondary { .. } => {
                    finish_drawing(world);
                }
            }
        }
    });
}

fn press(w: &mut World, at: Pt, shift: bool) {
    let pick = w.resource_mut::<LayoutState>().pick.take();
    // A move started with M or D ends with a click.
    if let Some(m) = w
        .resource::<LayoutState>()
        .moving
        .clone()
        .filter(|m| !m.on_release)
    {
        finish_move(w, &m, at);
        return;
    }
    let tool = w.resource::<LayoutState>().tool.clone();
    let Some(d) = board_of(w) else { return };
    match tool {
        Tool::Select => {
            let pick = pick.filter(|id| d.board.footprints.iter().any(|f| f.id == *id));
            let tol = tolerance(w);
            // A corner of the hovered or selected zone or polygon (its handle shows): drag that
            // corner, whatever else is under it.
            if !shift && let Some((item, ring, i, from)) = corner_under(w, &d, at) {
                let mut s = w.resource_mut::<LayoutState>();
                if !s.selection.contains(&item) {
                    s.selection = vec![item];
                }
                s.moving = Some(Moving {
                    fp: uuid::Uuid::nil(),
                    from,
                    drag: false,
                    on_release: true,
                    items: vec![item],
                    corner: Some((ring, i)),
                    points: None,
                    track: None,
                });
                return;
            }
            let hit = pick
                .map(BoardItem::Footprint)
                .or_else(|| be::hit_from(&d.board, at, tol, w.resource::<LayoutState>().facing));
            let mut s = w.resource_mut::<LayoutState>();
            s.expanded = false;
            match hit {
                Some(h) if shift => {
                    if !s.selection.contains(&h) {
                        s.selection.push(h);
                    }
                }
                // Pressing on one of several selected items drags them all.
                Some(h) if s.selection.len() > 1 && s.selection.contains(&h) => {
                    let items = s.selection.clone();
                    s.moving = Some(Moving {
                        fp: uuid::Uuid::nil(),
                        from: snap(at, grid()),
                        drag: false,
                        on_release: true,
                        items,
                        corner: None,
                        points: None,
                        track: None,
                    });
                }
                Some(h) => {
                    // Pressing on a footprint starts dragging it (from the grid point pressed,
                    // so it keeps its place under the pointer); on a track, dragging it with
                    // the tracks joined to it stretching, or near one of its ends that end
                    // alone (it lands on the grid point let go at); on a via, the via and the
                    // track ends on it; on a selected zone, shape or text, that.
                    let drag = |net: &str, points: Vec<Pt>, from: Pt| Moving {
                        fp: uuid::Uuid::nil(),
                        from,
                        drag: false,
                        on_release: true,
                        items: vec![],
                        corner: None,
                        points: Some((net.to_string(), points)),
                        track: None,
                    };
                    match h {
                        BoardItem::Footprint(fp) => {
                            s.moving = Some(Moving {
                                fp,
                                from: snap(at, grid()),
                                drag: false,
                                on_release: true,
                                items: vec![],
                                corner: None,
                                points: None,
                                track: None,
                            })
                        }
                        BoardItem::Track(id) => {
                            if let Some(t) = d.board.tracks.iter().find(|t| t.id == id) {
                                let end = [t.a, t.b]
                                    .into_iter()
                                    .filter(|e| e.dist(at) <= (tol * 3 / 2) as f64)
                                    .min_by(|a, b| a.dist(at).total_cmp(&b.dist(at)));
                                s.moving = Some(match end {
                                    Some(e) => drag(&t.net, vec![e], e),
                                    None => Moving {
                                        fp: uuid::Uuid::nil(),
                                        from: snap(at, grid()),
                                        drag: false,
                                        on_release: true,
                                        items: vec![],
                                        corner: None,
                                        points: None,
                                        track: Some(id),
                                    },
                                });
                            }
                        }
                        BoardItem::Via(id) => {
                            if let Some(v) = d.board.vias.iter().find(|v| v.id == id) {
                                s.moving = Some(drag(&v.net, vec![v.at], v.at));
                            }
                        }
                        _ if s.selection.contains(&h) => {
                            s.moving = Some(Moving {
                                fp: uuid::Uuid::nil(),
                                from: snap(at, grid()),
                                drag: false,
                                on_release: true,
                                items: vec![h],
                                corner: None,
                                points: None,
                                track: None,
                            })
                        }
                        _ => {}
                    }
                    s.selection = vec![h];
                }
                // On empty board: a box selection (Shift adds to what is selected).
                None => {
                    if !shift {
                        s.selection.clear();
                    }
                    s.boxing = Some(at);
                }
            }
        }
        Tool::Outline(None) => {
            w.resource_mut::<LayoutState>().tool = Tool::Outline(Some(snap(at, mm(1.0))))
        }
        Tool::Outline(Some(a)) => {
            let b = snap(at, mm(1.0));
            ui::commit(w, "Board outline", |d| {
                cadrs_eda::outline::add_rect(&mut d.board, a, b);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Route {
            mut runs,
            vias,
            net,
        } => {
            let to = snap_to_copper(&d, at);
            let (layer, pts) = runs.last_mut().unwrap();
            let from = *pts.last().unwrap();
            pts.extend(be::posture(from, to, false).into_iter().skip(1));
            let layer = *layer;
            // Ending on copper of the net (a pad, a via) finishes the route.
            let ends = to != from
                && be::pad_at(&d.board, to).is_some_and(|(fi, pi)| {
                    d.board.footprints[fi].footprint.pads[pi].net.as_deref() == Some(net.as_str())
                        && d.board.footprints[fi]
                            .placement
                            .layers(d.board.footprints[fi].footprint.pads[pi].layers)
                            .contains(layer)
                });
            if ends {
                finish_route(w, runs, vias, net);
            } else {
                w.resource_mut::<LayoutState>().tool = Tool::Route { runs, vias, net };
            }
        }
        Tool::Zone {
            net,
            layer,
            mut pts,
        } => {
            pts.push(snap(at, grid()));
            w.resource_mut::<LayoutState>().tool = Tool::Zone { net, layer, pts };
        }
        Tool::Polygon(mut pts) => {
            pts.push(snap(at, grid()));
            w.resource_mut::<LayoutState>().tool = Tool::Polygon(pts);
        }
        Tool::Draw { kind, start: None } => {
            w.resource_mut::<LayoutState>().tool = Tool::Draw {
                kind,
                start: Some(snap(at, grid())),
            }
        }
        Tool::Draw {
            kind,
            start: Some(a),
        } => {
            let b = snap(at, grid());
            if a == b {
                return;
            }
            let layer = w.resource::<LayoutState>().draw_layer;
            ui::commit(w, "Draw", |d| {
                be::add_shape(&mut d.board, draw_geom(kind, a, b), layer);
                Ok(())
            });
            // Lines chain on from where the last one ended.
            w.resource_mut::<LayoutState>().tool = Tool::Draw {
                kind,
                start: (kind == DrawKind::Line).then_some(b),
            };
        }
        Tool::Text(text) => {
            let layer = w.resource::<LayoutState>().draw_layer;
            let look = w.resource::<LayoutState>().text_style;
            let at = snap(at, grid());
            ui::commit(w, "Add text", |d| {
                add_text(d, &text, at, layer, look);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Graphic(polys) => {
            let layer = w.resource::<LayoutState>().draw_layer;
            let at = snap(at, grid());
            ui::commit(w, "Import graphic", |d| {
                be::add_artwork(
                    &mut d.board,
                    &cadrs_eda::graphic_import::placed_at(&polys, at),
                    layer,
                );
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Keepout {
            layers,
            rules,
            mut pts,
        } => {
            pts.push(snap(at, grid()));
            w.resource_mut::<LayoutState>().tool = Tool::Keepout { layers, rules, pts };
        }
        // The ruler: a start, an end (it stays), then a click starts another.
        Tool::Measure { a, b } => {
            let p = snap(at, grid());
            w.resource_mut::<LayoutState>().tool = match (a, b) {
                (Some(a), None) => Tool::Measure {
                    a: Some(a),
                    b: Some(p),
                },
                _ => Tool::Measure {
                    a: Some(p),
                    b: None,
                },
            };
        }
    }
}

/// The ruler's line and its reading, on the comments layer.
fn ruler(d: &mut Design, a: Pt, b: Pt) {
    let (dx, dy) = (
        cadrs_eda::units::to_mm(b.x - a.x),
        cadrs_eda::units::to_mm(b.y - a.y),
    );
    be::add_shape(
        &mut d.board,
        cadrs_eda::graphics::Geom::Line { a, b },
        Layer::Comments,
    );
    let mid = Pt::new((a.x + b.x) / 2, (a.y + b.y) / 2 + mm(1.0));
    be::add_text(
        &mut d.board,
        &format!("{:.3} mm  (dx {dx:.3}, dy {dy:.3})", dx.hypot(dy)),
        mid,
        Layer::Comments,
    );
}

/// The shape a Draw tool makes from `a` to `b`.
fn draw_geom(kind: DrawKind, a: Pt, b: Pt) -> cadrs_eda::graphics::Geom {
    use cadrs_eda::graphics::Geom;
    match kind {
        DrawKind::Line => Geom::Line { a, b },
        DrawKind::Rect => Geom::Rect { a, b },
        DrawKind::Circle => Geom::Circle {
            center: a,
            radius: a.dist(b).round() as Nm,
        },
    }
}

fn double(w: &mut World, at: Pt, shift: bool) {
    if !matches!(w.resource::<LayoutState>().tool, Tool::Select) {
        finish_drawing(w);
        return;
    }
    // On a track or via: every track and via of its net (with Shift, added to what is selected).
    if let Some(d) = board_of(w) {
        let under = be::hit_from(
            &d.board,
            at,
            tolerance(w),
            w.resource::<LayoutState>().facing,
        );
        let net = match under {
            Some(BoardItem::Track(id)) => d
                .board
                .tracks
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.net.clone()),
            Some(BoardItem::Via(id)) => d
                .board
                .vias
                .iter()
                .find(|v| v.id == id)
                .map(|v| v.net.clone()),
            _ => None,
        };
        if let Some(net) = net.filter(|n| !n.is_empty()) {
            let mut items: Vec<BoardItem> = d
                .board
                .tracks
                .iter()
                .filter(|t| t.net == net)
                .map(|t| BoardItem::Track(t.id))
                .collect();
            items.extend(
                d.board
                    .vias
                    .iter()
                    .filter(|v| v.net == net)
                    .map(|v| BoardItem::Via(v.id)),
            );
            let mut s = w.resource_mut::<LayoutState>();
            if shift {
                for i in items {
                    if !s.selection.contains(&i) {
                        s.selection.push(i);
                    }
                }
            } else {
                s.selection = items;
            }
            s.expanded = true;
            s.moving = None;
            return;
        }
    }
    open_properties(w, at);
}

/// Ends what is being drawn (a double-click, Enter or a right-click): a zone, keep-out or
/// polygon closes on the corners clicked so far, a route ends, a chain of lines stops.
/// Returns whether something was being drawn.
fn finish_drawing(w: &mut World) -> bool {
    let tool = w.resource::<LayoutState>().tool.clone();
    match tool {
        Tool::Route { runs, vias, net } => finish_route(w, runs, vias, net),
        Tool::Zone {
            net,
            layer,
            mut pts,
        } => {
            // A double-click's second press adds its point twice.
            pts.dedup();
            if pts.len() >= 3 {
                ui::commit(w, "Add zone", |d| {
                    cadrs_eda::zone::add_zone(&mut d.board, &net, layer, pts.clone());
                    Ok(())
                });
            }
            set_tool(w, Tool::Select);
        }
        Tool::Keepout {
            layers,
            rules,
            mut pts,
        } => {
            pts.dedup();
            if pts.len() >= 3 {
                ui::commit(w, "Add keep-out", |d| {
                    cadrs_eda::zone::add_keepout(&mut d.board, layers, pts.clone(), rules);
                    Ok(())
                });
            }
            set_tool(w, Tool::Select);
        }
        Tool::Polygon(mut pts) => {
            pts.dedup();
            if pts.len() >= 3 {
                let layer = w.resource::<LayoutState>().draw_layer;
                // On copper: the net of the copper it starts on.
                let net = if layer.is_copper() {
                    board_of(w)
                        .map(|d| be::net_at(&d.board, pts[0], layer))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                ui::commit(w, "Draw polygon", |d| {
                    be::add_polygon(&mut d.board, pts.clone(), layer, &net);
                    Ok(())
                });
            }
            set_tool(w, Tool::Polygon(vec![]));
        }
        // A chain of lines stops (the next click starts another).
        Tool::Draw {
            kind: DrawKind::Line,
            start: Some(_),
        } => set_tool(
            w,
            Tool::Draw {
                kind: DrawKind::Line,
                start: None,
            },
        ),
        _ => return false,
    }
    true
}

/// Letting go of a footprint being dragged puts it down (where it was: just a selection).
fn release(w: &mut World, at: Pt) {
    if let Some(m) = w
        .resource::<LayoutState>()
        .moving
        .clone()
        .filter(|m| m.on_release)
    {
        finish_move(w, &m, at);
        return;
    }
    // A box selection: left to right takes what is wholly inside, right to left what it touches.
    let Some(start) = w.resource_mut::<LayoutState>().boxing.take() else {
        return;
    };
    w.resource_mut::<LayoutState>().box_preview.clear();
    if start.dist(at) < tolerance(w) as f64 {
        return;
    }
    let Some(d) = board_of(w) else { return };
    let found = be::box_select(&d.board, box_of(start, at), at.x < start.x);
    let mut s = w.resource_mut::<LayoutState>();
    for f in found {
        if !s.selection.contains(&f) {
            s.selection.push(f);
        }
    }
}

fn box_of(a: Pt, b: Pt) -> cadrs_eda::units::Bounds {
    cadrs_eda::units::Bounds {
        min: Pt::new(a.x.min(b.x), a.y.min(b.y)),
        max: Pt::new(a.x.max(b.x), a.y.max(b.y)),
    }
}

fn finish_move(w: &mut World, m: &Moving, at: Pt) {
    let d = w
        .resource_mut::<LayoutState>()
        .exact
        .take()
        .unwrap_or_else(|| snap(at, grid()))
        - m.from;
    let (fp, drag) = (m.fp, m.drag);
    w.resource_mut::<LayoutState>().moving = None;
    w.resource_mut::<Preview>().set(None);
    if d == Pt::ZERO {
        return;
    }
    if let Some((net, points)) = m.points.clone() {
        ui::commit(w, "Drag track", |des| {
            be::move_track_points(&mut des.board, &net, &points, d);
            Ok(())
        });
        return;
    }
    if let Some(id) = m.track {
        ui::commit(w, "Drag track", |des| {
            be::drag_track(&mut des.board, id, d);
            Ok(())
        });
        return;
    }
    if !m.items.is_empty() {
        let (items, corner) = (m.items.clone(), m.corner);
        let to = m.from + d;
        ui::commit(
            w,
            if corner.is_some() {
                "Move corner"
            } else {
                "Move"
            },
            |des| {
                match corner {
                    Some((ring, i)) => be::move_corner(&mut des.board, items[0], ring, i, to),
                    None => be::move_items(&mut des.board, &items, d),
                }
                Ok(())
            },
        );
        return;
    }
    ui::commit(
        w,
        if drag {
            "Drag footprint"
        } else {
            "Move footprint"
        },
        |des| {
            let i = be::footprint_by_id(&des.board, fp).ok_or("The footprint is gone")?;
            if drag {
                be::drag_footprint(&mut des.board, i, d);
            } else {
                let to = des.board.footprints[i].placement.at + d;
                be::move_footprint(&mut des.board, i, to);
            }
            Ok(())
        },
    );
}

fn selected_footprint(w: &World) -> Option<uuid::Uuid> {
    let s = w.resource::<LayoutState>();
    s.selection.iter().find_map(|i| {
        if let BoardItem::Footprint(id) = i {
            Some(*id)
        } else {
            None
        }
    })
}

fn on_keys(mut reader: MessageReader<KeyboardInput>, mut commands: Commands) {
    let keys: Vec<KeyboardInput> = reader.read().cloned().collect();
    if !keys.is_empty() {
        commands.queue(move |world: &mut World| handle_keys(world, keys));
    }
}

fn handle_keys(world: &mut World, keys: Vec<KeyboardInput>) {
    if !ui::keys_for(world, Mode::Layout) {
        return;
    }
    let held = world.resource::<ButtonInput<KeyCode>>().clone();
    let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    for k in keys {
        if k.state != ButtonState::Pressed {
            continue;
        }
        if ctrl {
            if shift && k.shortcut_code() == KeyCode::KeyM {
                set_tool(world, Tool::Measure { a: None, b: None });
            }
            continue;
        }
        let pointer = world.resource::<EdaPointer>().at;
        match k.shortcut_code() {
            KeyCode::Escape => {
                let busy = !matches!(world.resource::<LayoutState>().tool, Tool::Select)
                    || world.resource::<LayoutState>().moving.is_some();
                set_tool(world, Tool::Select);
                if !busy {
                    world.resource_mut::<LayoutState>().selection.clear();
                    world.resource_mut::<LayoutState>().net = None;
                }
            }
            KeyCode::F8 => run_action(world, "update"),
            // Highlight the net of the copper under the pointer (again: off).
            KeyCode::Backquote => {
                let net = board_of(world).and_then(|d| {
                    let items = cadrs_eda::copper::items(&d.board);
                    items
                        .iter()
                        .find(|c| {
                            !c.net.is_empty()
                                && c.layers
                                    .iter()
                                    .any(|(_, r)| cadrs_eda::poly::contains(r, pointer))
                        })
                        .map(|c| c.net.clone())
                });
                let mut s = world.resource_mut::<LayoutState>();
                s.net = if net == s.net { None } else { net };
            }
            // Step the width new tracks get through the net class' and the predefined ones.
            KeyCode::KeyW => {
                let Some(d) = board_of(world) else { continue };
                let class = d.board.rules.class_of("").track_width;
                let mut sizes: Vec<Nm> = std::iter::once(class)
                    .chain(d.board.rules.track_widths.iter().copied())
                    .chain([mm(0.25), mm(0.4), mm(0.6), mm(1.0)])
                    .collect();
                sizes.sort();
                sizes.dedup();
                let now = world.resource::<LayoutState>().track_width.unwrap_or(class);
                let i = sizes.iter().position(|s| *s == now).unwrap_or(0);
                let next = if shift {
                    sizes[i.saturating_sub(1)]
                } else {
                    sizes[(i + 1).min(sizes.len() - 1)]
                };
                world.resource_mut::<LayoutState>().track_width = Some(next);
                ui::toast(
                    world,
                    &format!("Track width {} mm", cadrs_eda::units::to_mm(next)),
                );
            }
            KeyCode::KeyX => run_action(world, "route"),
            KeyCode::KeyZ => run_action(world, "zone"),
            // Enter ends what is being drawn (a zone, keep-out, polygon, route, lines).
            KeyCode::Enter | KeyCode::NumpadEnter => {
                finish_drawing(world);
            }
            KeyCode::PageUp => world.resource_mut::<LayoutState>().active = Layer::TopCopper,
            KeyCode::PageDown => world.resource_mut::<LayoutState>().active = Layer::BottomCopper,
            KeyCode::KeyV => {
                // A via at the pointer, the route going on from it on the other side.
                if let Tool::Route {
                    mut runs,
                    mut vias,
                    net,
                } = world.resource::<LayoutState>().tool.clone()
                {
                    let at = snap(pointer, grid());
                    let (layer, pts) = runs.last_mut().unwrap();
                    let from = *pts.last().unwrap();
                    pts.extend(be::posture(from, at, false).into_iter().skip(1));
                    let next = if *layer == Layer::TopCopper {
                        Layer::BottomCopper
                    } else {
                        Layer::TopCopper
                    };
                    vias.push(at);
                    runs.push((next, vec![at]));
                    world.resource_mut::<LayoutState>().active = next;
                    world.resource_mut::<LayoutState>().tool = Tool::Route { runs, vias, net };
                }
            }
            // Shift+M: Move by (typed distances), the selected footprints (else the one under
            // the pointer).
            KeyCode::KeyM if shift => {
                let selected: Vec<uuid::Uuid> = world
                    .resource::<LayoutState>()
                    .selection
                    .iter()
                    .filter_map(|i| {
                        if let BoardItem::Footprint(id) = i {
                            Some(*id)
                        } else {
                            None
                        }
                    })
                    .collect();
                let ids = if selected.is_empty() {
                    target_footprint(world, pointer).into_iter().collect()
                } else {
                    selected
                };
                world.resource_mut::<LayoutState>().moving = None;
                super::lay_dialogs::open_move_by(world, ids);
            }
            KeyCode::KeyM | KeyCode::KeyD => {
                let fp = target_footprint(world, pointer);
                // The footprint's anchor follows the pointer: it lands on the clicked grid point.
                let anchor = fp.and_then(|id| {
                    board_of(world).and_then(|d| {
                        be::footprint_by_id(&d.board, id)
                            .map(|i| d.board.footprints[i].placement.at)
                    })
                });
                if let (Some(fp), Some(from)) = (fp, anchor) {
                    let mut s = world.resource_mut::<LayoutState>();
                    s.selection = vec![BoardItem::Footprint(fp)];
                    s.moving = Some(Moving {
                        fp,
                        from,
                        drag: k.shortcut_code() == KeyCode::KeyD,
                        on_release: false,
                        items: vec![],
                        corner: None,
                        points: None,
                        track: None,
                    });
                }
            }
            // The properties of the footprint or shape under the pointer (else the selected one).
            KeyCode::KeyE => open_properties(world, pointer),
            // R turns, F flips: several selected items (zones, shapes, text, tracks, footprints)
            // together about their middle, else the footprint under the pointer (or selected),
            // else the one selected item.
            KeyCode::KeyR | KeyCode::KeyF => {
                let rotate = k.shortcut_code() == KeyCode::KeyR;
                let sel = world.resource::<LayoutState>().selection.clone();
                let fp = (sel.len() <= 1)
                    .then(|| target_footprint(world, pointer))
                    .flatten();
                if let Some(fp) = fp {
                    ui::commit(
                        world,
                        if rotate {
                            "Rotate footprint"
                        } else {
                            "Flip footprint"
                        },
                        |d| {
                            let i =
                                be::footprint_by_id(&d.board, fp).ok_or("The footprint is gone")?;
                            if rotate {
                                be::rotate_footprint(&mut d.board, i, 90.0);
                            } else {
                                be::flip_footprint(&mut d.board, i);
                            }
                            Ok(())
                        },
                    );
                } else if !sel.is_empty() {
                    ui::commit(world, if rotate { "Rotate" } else { "Flip" }, |d| {
                        let c = be::items_center(&d.board, &sel, grid());
                        if rotate {
                            be::rotate_items(&mut d.board, &sel, c, 90.0);
                        } else {
                            be::flip_items(&mut d.board, &sel, c);
                        }
                        Ok(())
                    });
                }
            }
            KeyCode::KeyU => {
                let s = world.resource::<LayoutState>();
                let track = s.selection.iter().find_map(|i| {
                    if let BoardItem::Track(id) = i {
                        Some(*id)
                    } else {
                        None
                    }
                });
                let through = s.expanded;
                if let (Some(t), Some(d)) = (track, board_of(world)) {
                    let ids = be::select_connected(&d.board, t, through);
                    let items: Vec<BoardItem> = ids
                        .into_iter()
                        .map(|id| {
                            if d.board.vias.iter().any(|v| v.id == id) {
                                BoardItem::Via(id)
                            } else {
                                BoardItem::Track(id)
                            }
                        })
                        .collect();
                    let mut s = world.resource_mut::<LayoutState>();
                    s.selection = items;
                    s.expanded = true;
                }
            }
            KeyCode::Delete | KeyCode::Backspace => {
                let mut sel = world.resource::<LayoutState>().selection.clone();
                // The outline a sketch drives stays (`cadrs_core::pcb::outline_link`).
                let linked = linked_outline(world);
                let n = sel.len();
                sel.retain(|i| !linked.contains(&i.id()));
                if sel.len() < n {
                    ui::toast(world, &outline_link_note(world));
                }
                if !sel.is_empty() {
                    ui::commit(world, "Delete", |d| {
                        be::delete_items(&mut d.board, &sel);
                        Ok(())
                    });
                    world.resource_mut::<LayoutState>().selection.clear();
                }
            }
            _ => {}
        }
    }
}

/// Previews a move, a route or a zone in progress.
fn follow_pointer(world: &mut World, mut last: Local<Option<Pt>>, mut base: Local<Option<u64>>) {
    if !shown(world) {
        return;
    }
    let at = world.resource::<EdaPointer>().at;
    let s = world.resource::<LayoutState>();
    let (tool, moving) = (s.tool.clone(), s.moving.clone());
    let busy = moving.is_some() || !matches!(tool, Tool::Select);
    if !busy {
        if last.is_some() {
            *last = None;
            world.resource_mut::<Preview>().set(None);
        }
        return;
    }
    let key = world.resource::<LayoutState>().exact.unwrap_or(at);
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    // The stored board's layout hash: while it is the one the preview was made from, a moved
    // footprint changes only its own place in the preview (no copy of the whole design each
    // frame of a drag, a big zone fill and schematic included).
    let stored = world
        .resource::<super::Eda2d>()
        .board()
        .and_then(|(el, b, _)| {
            world
                .resource::<crate::ActiveDocument>()
                .doc
                .element(el)?
                .pcb()?
                .board(b)
                .map(|x| x.layout_hash())
        });
    let reuse = moving
        .as_ref()
        .is_some_and(|m| !m.drag && m.items.is_empty() && m.points.is_none() && m.track.is_none())
        && stored.is_some()
        && *base == stored;
    let preview = if reuse {
        world.resource_mut::<Preview>().take()
    } else {
        None
    };
    let d = match preview {
        Some(mut d) => {
            // Back to the stored place first, then moved below.
            let m = moving.as_ref().map(|m| m.fp);
            let from = m.and_then(|fp| board_of_footprint(world, fp));
            match (m.and_then(|fp| be::footprint_by_id(&d.board, fp)), from) {
                (Some(i), Some(f)) => {
                    d.board.footprints[i] = f;
                    Some(d)
                }
                _ => board_of(world),
            }
        }
        None => board_of(world),
    };
    let Some(mut d) = d else { return };
    *base = stored;
    if let Some(m) = moving {
        let exact = world.resource::<LayoutState>().exact;
        let delta = exact.unwrap_or_else(|| snap(at, grid())) - m.from;
        match (m.items.first(), m.corner) {
            (Some(item), Some((ring, i))) => {
                be::move_corner(&mut d.board, *item, ring, i, m.from + delta)
            }
            (Some(_), None) => be::move_items(&mut d.board, &m.items, delta),
            _ => {}
        }
        if let Some((net, points)) = &m.points {
            be::move_track_points(&mut d.board, net, points, delta);
        }
        if let Some(id) = m.track {
            be::drag_track(&mut d.board, id, delta);
        }
        if let Some(i) = be::footprint_by_id(&d.board, m.fp) {
            if m.drag {
                be::drag_footprint(&mut d.board, i, delta);
            } else {
                let to = d.board.footprints[i].placement.at + delta;
                be::move_footprint(&mut d.board, i, to);
            }
        }
    } else {
        match tool {
            Tool::Route { runs, vias, net } => {
                let width = world.resource::<LayoutState>().track_width;
                let to = snap_to_copper(&d, at);
                for (k, (l, pts)) in runs.iter().enumerate() {
                    let mut p = pts.clone();
                    if k + 1 == runs.len() {
                        p.extend(
                            be::posture(*pts.last().unwrap(), to, false)
                                .into_iter()
                                .skip(1),
                        );
                    }
                    let ids = be::route(&mut d.board, &p, *l, width);
                    for t in d.board.tracks.iter_mut().filter(|t| ids.contains(&t.id)) {
                        t.net = net.clone();
                    }
                }
                for v in vias {
                    be::add_via(&mut d.board, v, &net);
                }
            }
            Tool::Outline(Some(a)) => {
                cadrs_eda::outline::add_rect(&mut d.board, a, snap(at, mm(1.0)));
            }
            Tool::Draw {
                kind,
                start: Some(a),
            } => {
                let layer = world.resource::<LayoutState>().draw_layer;
                be::add_shape(&mut d.board, draw_geom(kind, a, snap(at, grid())), layer);
            }
            Tool::Text(text) => {
                let layer = world.resource::<LayoutState>().draw_layer;
                let look = world.resource::<LayoutState>().text_style;
                add_text(&mut d, &text, snap(at, grid()), layer, look);
            }
            Tool::Graphic(polys) => {
                let layer = world.resource::<LayoutState>().draw_layer;
                be::add_artwork(
                    &mut d.board,
                    &cadrs_eda::graphic_import::placed_at(&polys, snap(at, grid())),
                    layer,
                );
            }
            Tool::Measure { a: Some(a), b } => {
                ruler(&mut d, a, b.unwrap_or_else(|| snap(at, grid())))
            }
            Tool::Polygon(pts) => {
                // Filled as it will be, on the drawing layer.
                let mut ring = pts.clone();
                ring.push(snap(at, grid()));
                if ring.len() >= 3 {
                    let layer = world.resource::<LayoutState>().draw_layer;
                    be::add_polygon(&mut d.board, ring, layer, "");
                } else if ring.len() == 2 {
                    let layer = world.resource::<LayoutState>().draw_layer;
                    be::add_shape(
                        &mut d.board,
                        cadrs_eda::graphics::Geom::Line {
                            a: ring[0],
                            b: ring[1],
                        },
                        layer,
                    );
                }
            }
            Tool::Keepout { pts, .. } | Tool::Zone { pts, .. } => {
                let mut ring = pts.clone();
                ring.push(snap(at, grid()));
                if ring.len() >= 2 {
                    d.board.shapes.push(cadrs_eda::board::BoardShape {
                        id: uuid::Uuid::nil(),
                        shape: cadrs_eda::graphics::Shape {
                            geom: cadrs_eda::graphics::Geom::Polyline {
                                pts: ring,
                                closed: true,
                            },
                            stroke: cadrs_eda::graphics::Stroke::width(mm(0.1)),
                            fill: Default::default(),
                        },
                        layer: Layer::Comments,
                        locked: false,
                        net: String::new(),
                    });
                }
            }
            _ => return,
        }
    }
    world.resource_mut::<Preview>().set(Some(d));
}

/// The selection and active layer, for the picture.
/// The corner of the hovered or selected zones and polygons (whose handles show) within a
/// handle of `at`: the nearest, as (item, ring, index, where).
fn corner_under(w: &World, d: &Design, at: Pt) -> Option<(BoardItem, usize, usize, Pt)> {
    let s = w.resource::<LayoutState>();
    let reach = tolerance(w) * 3 / 2;
    let mut items: Vec<BoardItem> = s.selection.clone();
    items.extend(s.hover);
    items
        .into_iter()
        .filter(|i| matches!(i, BoardItem::Zone(_) | BoardItem::Shape(_)))
        .flat_map(|i| {
            be::corners(&d.board, i)
                .into_iter()
                .map(move |(r, k, p)| (i, r, k, p))
        })
        .filter(|c| c.3.dist(at) <= reach as f64)
        .min_by(|a, b| a.3.dist(at).total_cmp(&b.3.dist(at)))
}

/// What is under the pointer while selecting (drawn lighter), when nothing is being moved.
fn track_hover(world: &mut World, mut last: Local<Option<(Pt, usize)>>) {
    let s = world.resource::<LayoutState>();
    let idle = shown(world) && matches!(s.tool, Tool::Select) && s.moving.is_none();
    if !idle {
        let s = world.resource::<LayoutState>();
        if s.hover.is_some() || s.hot_corner.is_some() {
            let mut s = world.resource_mut::<LayoutState>();
            (s.hover, s.hot_corner) = (None, None);
        }
        *last = None;
        return;
    }
    let at = world.resource::<EdaPointer>().at;
    let key = (
        at,
        world.resource::<crate::ActiveDocument>().history.undo_len(),
    );
    if *last == Some(key) {
        return;
    }
    *last = Some(key);
    let facing = world.resource::<LayoutState>().facing;
    let tol = tolerance(world);
    let over = world.resource::<EdaPointer>().over;
    // The stored board, read in place (no copy of the design each time the pointer moves).
    let Some((el, b, _)) = world.resource::<super::Eda2d>().board() else {
        return;
    };
    let Some(d) = super::design(world.resource::<crate::ActiveDocument>(), el, b) else {
        return;
    };
    let corner = if over {
        corner_under(world, d, at)
    } else {
        None
    };
    let hover = if !over {
        None
    } else if let Some((item, ..)) = corner {
        // On a corner's handle (perhaps just outside the shape): its zone or polygon stays
        // hovered, its handles shown.
        Some(item)
    } else {
        be::hit_from(&d.board, at, tol, facing)
    };
    let mut s = world.resource_mut::<LayoutState>();
    if s.hover != hover {
        s.hover = hover;
    }
    if s.hot_corner != corner {
        s.hot_corner = corner;
    }
    if s.handle != tol {
        s.handle = tol;
    }
}

/// What the box being dragged would select ([`LayoutState::box_preview`]), shown selected as
/// it is dragged.
fn preview_box(world: &mut World, mut last: Local<Option<(Pt, Pt)>>) {
    let start = world
        .resource::<LayoutState>()
        .boxing
        .filter(|_| shown(world));
    let at = world.resource::<EdaPointer>().at;
    let found = match start {
        Some(start) if start.dist(at) >= tolerance(world) as f64 => {
            if *last == Some((start, at)) {
                return;
            }
            *last = Some((start, at));
            let (el, b, _) = match world.resource::<super::Eda2d>().board() {
                Some(k) => k,
                None => return,
            };
            let Some(d) = super::design(world.resource::<crate::ActiveDocument>(), el, b) else {
                return;
            };
            be::box_select(&d.board, box_of(start, at), at.x < start.x)
        }
        _ => {
            *last = None;
            vec![]
        }
    };
    let mut s = world.resource_mut::<LayoutState>();
    if s.box_preview != found {
        s.box_preview = found;
    }
}

/// The box being dragged ([`cadrs_ui::selection_box`]): window blue, crossing dashed yellow.
#[allow(clippy::too_many_arguments)]
fn draw_box(
    state: Res<LayoutState>,
    eda: Res<super::Eda2d>,
    ui_state: Res<super::EdaUi>,
    pointer: Res<EdaPointer>,
    rect: Res<crate::viewport::ViewportRect>,
    q_area: Query<Entity, With<crate::viewport::ViewportArea>>,
    mut boxes: cadrs_ui::selection_box::Boxes,
    mut commands: Commands,
) {
    let flat = eda.0.is_some_and(|(_, _, m)| m == Mode::Layout) && eda.flat();
    let span = match (state.boxing, super::current_view(&ui_state, &eda)) {
        (Some(start), Some(view)) if flat => {
            let s = view.to_screen([start.x as f64, start.y as f64]);
            let start = Vec2::new(s[0] as f32, s[1] as f32);
            let end = pointer.screen - rect.0.min;
            (start.distance(end) >= 4.0).then_some((start, end))
        }
        _ => None,
    };
    cadrs_ui::selection_box::sync(
        &mut commands,
        &mut boxes,
        "pcb-box-select",
        q_area.iter().next(),
        span,
    );
}

fn publish(s: Res<LayoutState>, eda: Res<super::Eda2d>, mut inputs: ResMut<SceneInputs>) {
    if eda.board().is_none() {
        return;
    }
    let mut sel: Vec<uuid::Uuid> = s.selection.iter().map(|i| i.id()).collect();
    sel.extend(
        s.box_preview
            .iter()
            .map(|i| i.id())
            .filter(|id| !s.selection.iter().any(|x| x.id() == *id)),
    );
    if inputs.board_selected != sel {
        inputs.board_selected = sel;
    }
    let hover = s.hover.map(|h| h.id());
    if inputs.board_hover != hover {
        inputs.board_hover = hover;
    }
    let hot = s.hot_corner.map(|c| c.3);
    if inputs.board_hot_corner != hot || inputs.board_handle != s.handle {
        inputs.board_hot_corner = hot;
        inputs.board_handle = s.handle;
    }
    if inputs.active_layer != Some(s.active) {
        inputs.active_layer = Some(s.active);
    }
    if inputs.hidden_layers != s.hidden {
        inputs.hidden_layers = s.hidden.clone();
    }
    if inputs.dim_inactive != s.dim {
        inputs.dim_inactive = s.dim;
    }
    if inputs.board_net != s.net {
        inputs.board_net = s.net.clone();
    }
}

/// The footprint a key acts on: the one under the pointer, else the selected one.
fn target_footprint(w: &World, pointer: Pt) -> Option<uuid::Uuid> {
    let under = board_of(w).and_then(|d| {
        match be::hit_from(
            &d.board,
            pointer,
            tolerance(w),
            w.resource::<LayoutState>().facing,
        ) {
            Some(BoardItem::Footprint(id)) => Some(id),
            _ => None,
        }
    });
    under.or_else(|| selected_footprint(w))
}

/// E or a double-click: the properties of the footprint or shape at `at` (else the selected
/// one).
fn open_properties(w: &mut World, at: Pt) {
    let under = board_of(w).and_then(|d| {
        be::hit_from(
            &d.board,
            at,
            tolerance(w),
            w.resource::<LayoutState>().facing,
        )
    });
    let target = under.or_else(|| w.resource::<LayoutState>().selection.first().copied());
    match target {
        Some(BoardItem::Footprint(id)) => {
            w.resource_mut::<LayoutState>().moving = None;
            super::lay_dialogs::open_footprint(w, id);
        }
        Some(BoardItem::Shape(id)) => super::lay_dialogs::open_shape(w, id),
        Some(BoardItem::Zone(id)) => super::lay_dialogs::open_zone(w, id),
        Some(BoardItem::Text(id)) => super::lay_dialogs::open_board_text(w, id),
        _ => {}
    }
}

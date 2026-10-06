//! The Layout view's tools (docs/PLAN.md GS13–GS21, GS25), keys as in the guide:
//!
//! - **Select** (Esc): click picks a footprint, track, via or zone (Shift adds); **M** moves
//!   the selected footprint (a click puts it down), **D** drags it with its tracks attached,
//!   **R** rotates it, **F** flips it to the other side (F with nothing selected fits the
//!   view), **U** selects the connection's tracks (again: through vias), **Del** deletes.
//! - **X** routes from the pointer on the active layer (a bottom pad switches to the bottom);
//!   clicks add 45° runs, **V** drops a via and changes layer, a click on a pad of the net or a
//!   double-click ends. **PgUp** / **PgDn** pick the top / bottom copper.
//! - The strip: Update PCB from schematic (**F8**), Board setup, Board outline (two corners on
//!   a 1 mm grid), Route, Add zone (its net and layer asked first, then corners, double-click
//!   ends), Add keep-out (its layers and what it forbids asked first, then corners), Draw lines /
//!   a rectangle / a circle and Text on the drawing layer (picked in the layers panel), Fill
//!   zones (**B**), DRC, Plot.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_eda::Design;
use cadrs_eda::board_edit::{self as be, BoardItem};
use cadrs_eda::layer::Layer;
use cadrs_eda::units::{Nm, Pt, mm};

use super::ui::{self, StripAction};
use super::{EdaClick, EdaPointer, Mode, Preview, SceneInputs};
use crate::AppState;

const STRIP: ui::StripSpec = &[
    Some(("pcb-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some(("pcb-update", "update-pcb", "Update PCB from schematic (F8)", "update")),
    Some(("pcb-board-setup", "settings", "Board setup", "setup")),
    Some(("pcb-outline", "board-outline", "Draw the board outline", "outline")),
    Some(("pcb-route", "route-track", "Route tracks (X)", "route")),
    Some(("pcb-zone", "copper-zone", "Add a filled zone", "zone")),
    Some(("pcb-keepout", "keepout-zone", "Add a keep-out", "keepout")),
    Some(("pcb-draw-line", "line", "Draw lines on the drawing layer", "line")),
    Some(("pcb-draw-rect", "corner-rectangle", "Draw a rectangle on the drawing layer", "rect")),
    Some(("pcb-draw-circle", "center-circle", "Draw a circle on the drawing layer", "circle")),
    Some(("pcb-text", "text", "Add text on the drawing layer", "text")),
    Some(("pcb-fill", "fill-zones", "Fill all zones (B)", "fill")),
    Some(("pcb-layer", "layers", "Switch the active layer (front / back)", "layer")),
    None,
    Some(("pcb-drc", "drc", "Design rules checker", "drc")),
    Some(("pcb-plot", "plot-gerbers", "Plot fabrication outputs", "plot")),
];

#[derive(Clone, Debug, Default)]
pub enum Tool {
    #[default]
    Select,
    /// The outline's first corner, once clicked.
    Outline(Option<Pt>),
    /// A route: its points so far on each layer run, the layer now, its net.
    Route { runs: Vec<(Layer, Vec<Pt>)>, vias: Vec<Pt>, net: String },
    /// A zone of `net` on `layer`: its corners so far.
    Zone { net: String, layer: Layer, pts: Vec<Pt> },
    /// A line, rectangle or circle on the drawing layer: its first point once clicked.
    Draw { kind: DrawKind, start: Option<Pt> },
    /// Text, placed on the drawing layer with the next click.
    Text(String),
    /// A keep-out on `layers` forbidding what `rules` says: its corners so far.
    Keepout { layers: cadrs_eda::layer::LayerSet, rules: cadrs_eda::board::Keepout, pts: Vec<Pt> },
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
            Tool::Draw { kind: DrawKind::Line, .. } => "line",
            Tool::Draw { kind: DrawKind::Rect, .. } => "rect",
            Tool::Draw { kind: DrawKind::Circle, .. } => "circle",
            Tool::Text(_) => "text",
            Tool::Keepout { .. } => "keepout",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Moving {
    pub fp: uuid::Uuid,
    pub from: Pt,
    pub drag: bool,
}

#[derive(Resource, Debug)]
pub struct LayoutState {
    pub tool: Tool,
    pub selection: Vec<BoardItem>,
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
}

impl Default for LayoutState {
    fn default() -> Self {
        LayoutState { tool: Tool::Select, selection: vec![], moving: None, active: Layer::TopCopper, expanded: false, hidden: vec![], dim: false, layers_open: false, draw_layer: Layer::TopSilk }
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<LayoutState>().add_systems(
        Update,
        (strip, on_strip, on_click, on_keys, follow_pointer, publish).chain().after(super::navigate).run_if(in_state(AppState::Document)),
    );
    super::lay_dialogs::register(app);
}

fn strip(world: &mut World) {
    ui::sync_strip(world, Mode::Layout, STRIP);
    let a = world.resource::<LayoutState>().tool.action();
    ui::mark_active(world, Mode::Layout, a);
}

fn shown(w: &World) -> bool {
    w.resource::<super::Eda2d>().0.is_some_and(|(_, _, m)| m == Mode::Layout)
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
    let scale = key.and_then(|k| ui.views.get(&k)).map_or(4.0, |v| v.0.scale);
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
        "outline" => set_tool(w, Tool::Outline(None)),
        "route" => {
            let at = w.resource::<EdaPointer>().at;
            start_route(w, at);
        }
        "zone" => super::lay_dialogs::open_zone(w),
        "line" => set_tool(w, Tool::Draw { kind: DrawKind::Line, start: None }),
        "rect" => set_tool(w, Tool::Draw { kind: DrawKind::Rect, start: None }),
        "circle" => set_tool(w, Tool::Draw { kind: DrawKind::Circle, start: None }),
        "text" => super::lay_dialogs::open_text(w),
        "keepout" => super::lay_dialogs::open_keepout(w),
        "fill" => {
            ui::commit(w, "Fill zones", |d| {
                cadrs_eda::zone::fill_all(&mut d.board);
                Ok(())
            });
        }
        "layer" => {
            let mut s = w.resource_mut::<LayoutState>();
            s.active = if s.active == Layer::TopCopper { Layer::BottomCopper } else { Layer::TopCopper };
        }
        "drc" => super::lay_dialogs::open_drc(w),
        "plot" => super::lay_dialogs::open_plot(w),
        _ => {}
    }
}

pub fn set_tool(w: &mut World, tool: Tool) {
    let mut s = w.resource_mut::<LayoutState>();
    s.tool = tool;
    s.moving = None;
    w.resource_mut::<Preview>().set(None);
}

fn board_of(w: &World) -> Option<Design> {
    ui::current(w).map(|(_, _, d)| d)
}

/// A point snapped to a pad's centre when on a pad, else to the routing grid.
fn snap_to_copper(d: &Design, p: Pt) -> Pt {
    match be::pad_at(&d.board, p) {
        Some((fi, pi)) => be::pad_center(&d.board, fi, pi),
        None => d.board.vias.iter().find(|v| v.at.dist(p) <= v.diameter as f64 / 2.0).map_or(snap(p, grid()), |v| v.at),
    }
}

fn start_route(w: &mut World, at: Pt) {
    let Some(d) = board_of(w) else { return };
    let start = snap_to_copper(&d, at);
    let active = w.resource::<LayoutState>().active;
    let layer = be::start_layer(&d.board, start, active);
    let net = be::net_at(&d.board, start, layer);
    w.resource_mut::<LayoutState>().active = layer;
    set_tool(w, Tool::Route { runs: vec![(layer, vec![start])], vias: vec![], net });
}

/// Lays the route as tracks and vias (one undo step).
fn finish_route(w: &mut World, runs: Vec<(Layer, Vec<Pt>)>, vias: Vec<Pt>, net: String) {
    let any = runs.iter().any(|(_, p)| p.len() >= 2);
    if any {
        ui::commit(w, "Route", |d| {
            for v in &vias {
                be::add_via(&mut d.board, *v, &net);
            }
            for (l, pts) in &runs {
                if pts.len() >= 2 {
                    let ids = be::route(&mut d.board, pts, *l, None);
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
                EdaClick::Double { at } => double(world, at),
                EdaClick::Release { .. } => {}
            }
        }
    });
}

fn press(w: &mut World, at: Pt, shift: bool) {
    if let Some(m) = w.resource::<LayoutState>().moving.clone() {
        finish_move(w, &m, at);
        return;
    }
    let tool = w.resource::<LayoutState>().tool.clone();
    let Some(d) = board_of(w) else { return };
    match tool {
        Tool::Select => {
            let hit = be::hit(&d.board, at, tolerance(w));
            let mut s = w.resource_mut::<LayoutState>();
            s.expanded = false;
            match hit {
                Some(h) if shift => {
                    if !s.selection.contains(&h) {
                        s.selection.push(h);
                    }
                }
                Some(h) => s.selection = vec![h],
                None if !shift => s.selection.clear(),
                None => {}
            }
        }
        Tool::Outline(None) => w.resource_mut::<LayoutState>().tool = Tool::Outline(Some(snap(at, mm(1.0)))),
        Tool::Outline(Some(a)) => {
            let b = snap(at, mm(1.0));
            ui::commit(w, "Board outline", |d| {
                cadrs_eda::outline::add_rect(&mut d.board, a, b);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Route { mut runs, vias, net } => {
            let to = snap_to_copper(&d, at);
            let (layer, pts) = runs.last_mut().unwrap();
            let from = *pts.last().unwrap();
            pts.extend(be::posture(from, to, false).into_iter().skip(1));
            let layer = *layer;
            // Ending on copper of the net (a pad, a via) finishes the route.
            let ends = to != from && be::pad_at(&d.board, to).is_some_and(|(fi, pi)| d.board.footprints[fi].footprint.pads[pi].net.as_deref() == Some(net.as_str()) && d.board.footprints[fi].placement.layers(d.board.footprints[fi].footprint.pads[pi].layers).contains(layer));
            if ends {
                finish_route(w, runs, vias, net);
            } else {
                w.resource_mut::<LayoutState>().tool = Tool::Route { runs, vias, net };
            }
        }
        Tool::Zone { net, layer, mut pts } => {
            pts.push(snap(at, grid()));
            w.resource_mut::<LayoutState>().tool = Tool::Zone { net, layer, pts };
        }
        Tool::Draw { kind, start: None } => w.resource_mut::<LayoutState>().tool = Tool::Draw { kind, start: Some(snap(at, grid())) },
        Tool::Draw { kind, start: Some(a) } => {
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
            w.resource_mut::<LayoutState>().tool = Tool::Draw { kind, start: (kind == DrawKind::Line).then_some(b) };
        }
        Tool::Text(text) => {
            let layer = w.resource::<LayoutState>().draw_layer;
            let at = snap(at, grid());
            ui::commit(w, "Add text", |d| {
                be::add_text(&mut d.board, &text, at, layer);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Keepout { layers, rules, mut pts } => {
            pts.push(snap(at, grid()));
            w.resource_mut::<LayoutState>().tool = Tool::Keepout { layers, rules, pts };
        }
    }
}

/// The shape a Draw tool makes from `a` to `b`.
fn draw_geom(kind: DrawKind, a: Pt, b: Pt) -> cadrs_eda::graphics::Geom {
    use cadrs_eda::graphics::Geom;
    match kind {
        DrawKind::Line => Geom::Line { a, b },
        DrawKind::Rect => Geom::Rect { a, b },
        DrawKind::Circle => Geom::Circle { center: a, radius: a.dist(b).round() as Nm },
    }
}

fn double(w: &mut World, _at: Pt) {
    let tool = w.resource::<LayoutState>().tool.clone();
    match tool {
        Tool::Route { runs, vias, net } => finish_route(w, runs, vias, net),
        Tool::Zone { net, layer, mut pts } => {
            // The double-click's second press added its point twice.
            pts.dedup();
            if pts.len() >= 3 {
                ui::commit(w, "Add zone", |d| {
                    cadrs_eda::zone::add_zone(&mut d.board, &net, layer, pts.clone());
                    Ok(())
                });
            }
            set_tool(w, Tool::Select);
        }
        Tool::Keepout { layers, rules, mut pts } => {
            pts.dedup();
            if pts.len() >= 3 {
                ui::commit(w, "Add keep-out", |d| {
                    cadrs_eda::zone::add_keepout(&mut d.board, layers, pts.clone(), rules);
                    Ok(())
                });
            }
            set_tool(w, Tool::Select);
        }
        // A double-click ends a chain of lines.
        Tool::Draw { kind: DrawKind::Line, .. } => set_tool(w, Tool::Draw { kind: DrawKind::Line, start: None }),
        _ => {}
    }
}

fn finish_move(w: &mut World, m: &Moving, at: Pt) {
    let d = snap(at, grid()) - m.from;
    let (fp, drag) = (m.fp, m.drag);
    w.resource_mut::<LayoutState>().moving = None;
    w.resource_mut::<Preview>().set(None);
    if d == Pt::ZERO {
        return;
    }
    ui::commit(w, if drag { "Drag footprint" } else { "Move footprint" }, |des| {
        let i = be::footprint_by_id(&des.board, fp).ok_or("The footprint is gone")?;
        if drag {
            be::drag_footprint(&mut des.board, i, d);
        } else {
            let to = des.board.footprints[i].placement.at + d;
            be::move_footprint(&mut des.board, i, to);
        }
        Ok(())
    });
}

fn selected_footprint(w: &World) -> Option<uuid::Uuid> {
    let s = w.resource::<LayoutState>();
    s.selection.iter().find_map(|i| if let BoardItem::Footprint(id) = i { Some(*id) } else { None })
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
    for k in keys {
        if k.state != ButtonState::Pressed || ctrl {
            continue;
        }
        let pointer = world.resource::<EdaPointer>().at;
        match k.key_code {
            KeyCode::Escape => {
                let busy = !matches!(world.resource::<LayoutState>().tool, Tool::Select) || world.resource::<LayoutState>().moving.is_some();
                set_tool(world, Tool::Select);
                if !busy {
                    world.resource_mut::<LayoutState>().selection.clear();
                }
            }
            KeyCode::F8 => run_action(world, "update"),
            KeyCode::KeyB => run_action(world, "fill"),
            KeyCode::KeyX => run_action(world, "route"),
            KeyCode::PageUp => world.resource_mut::<LayoutState>().active = Layer::TopCopper,
            KeyCode::PageDown => world.resource_mut::<LayoutState>().active = Layer::BottomCopper,
            KeyCode::KeyV => {
                // A via at the pointer, the route going on from it on the other side.
                if let Tool::Route { mut runs, mut vias, net } = world.resource::<LayoutState>().tool.clone() {
                    let at = snap(pointer, grid());
                    let (layer, pts) = runs.last_mut().unwrap();
                    let from = *pts.last().unwrap();
                    pts.extend(be::posture(from, at, false).into_iter().skip(1));
                    let next = if *layer == Layer::TopCopper { Layer::BottomCopper } else { Layer::TopCopper };
                    vias.push(at);
                    runs.push((next, vec![at]));
                    world.resource_mut::<LayoutState>().active = next;
                    world.resource_mut::<LayoutState>().tool = Tool::Route { runs, vias, net };
                }
            }
            KeyCode::KeyM | KeyCode::KeyD => {
                let fp = target_footprint(world, pointer);
                // The footprint's anchor follows the pointer: it lands on the clicked grid point.
                let anchor = fp.and_then(|id| board_of(world).and_then(|d| be::footprint_by_id(&d.board, id).map(|i| d.board.footprints[i].placement.at)));
                if let (Some(fp), Some(from)) = (fp, anchor) {
                    let mut s = world.resource_mut::<LayoutState>();
                    s.selection = vec![BoardItem::Footprint(fp)];
                    s.moving = Some(Moving { fp, from, drag: k.key_code == KeyCode::KeyD });
                }
            }
            KeyCode::KeyR => {
                if let Some(fp) = target_footprint(world, pointer) {
                    ui::commit(world, "Rotate footprint", |d| {
                        let i = be::footprint_by_id(&d.board, fp).ok_or("The footprint is gone")?;
                        be::rotate_footprint(&mut d.board, i, 90.0);
                        Ok(())
                    });
                }
            }
            KeyCode::KeyF => {
                if let Some(fp) = target_footprint(world, pointer) {
                    ui::commit(world, "Flip footprint", |d| {
                        let i = be::footprint_by_id(&d.board, fp).ok_or("The footprint is gone")?;
                        be::flip_footprint(&mut d.board, i);
                        Ok(())
                    });
                }
            }
            KeyCode::KeyU => {
                let s = world.resource::<LayoutState>();
                let track = s.selection.iter().find_map(|i| if let BoardItem::Track(id) = i { Some(*id) } else { None });
                let through = s.expanded;
                if let (Some(t), Some(d)) = (track, board_of(world)) {
                    let ids = be::select_connected(&d.board, t, through);
                    let items: Vec<BoardItem> = ids.into_iter().map(|id| if d.board.vias.iter().any(|v| v.id == id) { BoardItem::Via(id) } else { BoardItem::Track(id) }).collect();
                    let mut s = world.resource_mut::<LayoutState>();
                    s.selection = items;
                    s.expanded = true;
                }
            }
            KeyCode::Delete | KeyCode::Backspace => {
                let sel = world.resource::<LayoutState>().selection.clone();
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
fn follow_pointer(world: &mut World, mut last: Local<Option<Pt>>) {
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
    if *last == Some(at) {
        return;
    }
    *last = Some(at);
    let Some(mut d) = board_of(world) else { return };
    if let Some(m) = moving {
        let delta = snap(at, grid()) - m.from;
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
                let to = snap_to_copper(&d, at);
                for (k, (l, pts)) in runs.iter().enumerate() {
                    let mut p = pts.clone();
                    if k + 1 == runs.len() {
                        p.extend(be::posture(*pts.last().unwrap(), to, false).into_iter().skip(1));
                    }
                    let ids = be::route(&mut d.board, &p, *l, None);
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
            Tool::Draw { kind, start: Some(a) } => {
                let layer = world.resource::<LayoutState>().draw_layer;
                be::add_shape(&mut d.board, draw_geom(kind, a, snap(at, grid())), layer);
            }
            Tool::Text(text) => {
                let layer = world.resource::<LayoutState>().draw_layer;
                be::add_text(&mut d.board, &text, snap(at, grid()), layer);
            }
            Tool::Keepout { pts, .. } | Tool::Zone { pts, .. } => {
                let mut ring = pts.clone();
                ring.push(snap(at, grid()));
                if ring.len() >= 2 {
                    d.board.shapes.push(cadrs_eda::board::BoardShape {
                        id: uuid::Uuid::nil(),
                        shape: cadrs_eda::graphics::Shape { geom: cadrs_eda::graphics::Geom::Polyline { pts: ring, closed: true }, stroke: cadrs_eda::graphics::Stroke::width(mm(0.1)), fill: Default::default() },
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
fn publish(s: Res<LayoutState>, eda: Res<super::Eda2d>, mut inputs: ResMut<SceneInputs>) {
    if eda.board().is_none() {
        return;
    }
    let sel: Vec<uuid::Uuid> = s.selection.iter().map(|i| i.id()).collect();
    if inputs.board_selected != sel {
        inputs.board_selected = sel;
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
}

/// The footprint a key acts on: the one under the pointer, else the selected one.
fn target_footprint(w: &World, pointer: Pt) -> Option<uuid::Uuid> {
    let under = board_of(w).and_then(|d| match be::hit(&d.board, pointer, tolerance(w)) {
        Some(BoardItem::Footprint(id)) => Some(id),
        _ => None,
    });
    under.or_else(|| selected_footprint(w))
}

//! The component editors (docs/PLAN.md GS22–GS26): a component made with + under Components
//! opens here when clicked, as a **Symbol | Footprint** pair of 2D views.
//!
//! - **Symbol**: Add pin (the Pin properties dialog, then the pin is placed where it says),
//!   **Insert** repeats the last pin 100 mil lower numbered on, Line / Circle / Rectangle
//!   (clicks on a 25 mil grid), Symbol properties (name, reference prefix, value, default
//!   footprint, keywords, pin texts shown). Click selects, double-click edits a pin, **Del**
//!   deletes.
//! - **Footprint**: Add pad (a click places it, numbered on from the last), Pad properties
//!   (double-click; sizes take expressions like `1.62+2*0.3`), Push pad properties to the
//!   other pads, Line / Rectangle on the drawing layer (Fab → Silkscreen → Courtyard), a
//!   rectangle's corners typed in (E), Footprint properties, 3D model.
//!
//! The component's symbol and footprint are its studio's project library: a symbol without a
//! default footprint gets the component's own.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use cadrs_core::pcb::{ComponentId, SetComponent};
use cadrs_eda::Component;
use cadrs_eda::graphics::{Fill, Geom};
use cadrs_eda::layer::Layer;
use cadrs_eda::lib_edit::{self as le, SymbolPart};
use cadrs_eda::units::{MIL, Nm, Pt, mm};

use super::ui::{self, StripAction};
use super::{EdaClick, EdaPointer, Mode, Preview, SceneInputs};
use crate::{ActiveDocument, AppState};

const SYMBOL_STRIP: ui::StripSpec = &[
    Some(("sym-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some(("sym-pin", "pin", "Add a pin", "pin")),
    Some(("sym-repeat", "copy", "Repeat the last pin (Insert)", "repeat")),
    Some(("sym-line", "line", "Draw a line", "line")),
    Some(("sym-circle", "center-circle", "Draw a circle", "circle")),
    Some(("sym-rect", "corner-rectangle", "Draw a rectangle", "rect")),
    None,
    Some(("sym-props", "properties", "Symbol properties", "props")),
    Some(("sym-from-library", "books", "Start from a library symbol", "fromlib")),
];

const FOOTPRINT_STRIP: ui::StripSpec = &[
    Some(("fp-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some(("fp-pad", "pad", "Add a pad", "pad")),
    Some(("fp-push", "copy", "Push the selected pad's properties to the other pads", "push")),
    Some(("fp-line", "line", "Draw a line", "line")),
    Some(("fp-rect", "corner-rectangle", "Draw a rectangle", "rect")),
    Some(("fp-layer", "layers", "Drawing layer: Fab, Silkscreen, Courtyard", "layer")),
    None,
    Some(("fp-props", "properties", "Footprint properties", "props")),
    Some(("fp-from-library", "books", "Start from a library footprint", "fromlib")),
    Some(("fp-model", "board-3d", "3D model", "model")),
];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Tool {
    #[default]
    Select,
    Pad,
    /// Two-click tools: the first point once clicked.
    Line(Option<Pt>),
    Circle(Option<Pt>),
    Rect(Option<Pt>),
}

impl Tool {
    fn action(&self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Pad => "pad",
            Tool::Line(_) => "line",
            Tool::Circle(_) => "circle",
            Tool::Rect(_) => "rect",
        }
    }
}

#[derive(Resource, Debug)]
pub struct PartState {
    pub tool: Tool,
    pub sym_selection: Vec<SymbolPart>,
    /// Selected pads and shapes of the footprint (their ids).
    pub fp_selection: Vec<uuid::Uuid>,
    /// The layer footprint drawings go on.
    pub layer: Layer,
}

impl Default for PartState {
    fn default() -> Self {
        PartState { tool: Tool::Select, sym_selection: vec![], fp_selection: vec![], layer: Layer::TopFab }
    }
}

pub fn register(app: &mut App) {
    app.init_resource::<PartState>().add_systems(
        Update,
        (strips, on_strip, on_click, on_keys, follow_pointer, publish).chain().after(super::navigate).run_if(in_state(AppState::Document)),
    );
}

fn strips(world: &mut World) {
    ui::sync_strip(world, Mode::Symbol, SYMBOL_STRIP);
    ui::sync_strip(world, Mode::Footprint, FOOTPRINT_STRIP);
    let a = world.resource::<PartState>().tool.action();
    ui::mark_active(world, Mode::Symbol, a);
    ui::mark_active(world, Mode::Footprint, a);
}

fn mode(w: &World) -> Option<Mode> {
    w.resource::<super::Eda2d>().component().map(|(_, _, m)| m)
}

/// The component being edited (a copy, its symbol and footprint started if missing).
pub fn current(w: &World) -> Option<(cadrs_core::ElementId, ComponentId, Component)> {
    let (el, c, _) = w.resource::<super::Eda2d>().component()?;
    let mut comp = w.resource::<ActiveDocument>().doc.element(el)?.pcb()?.component(c)?.component.clone();
    if comp.symbol.is_none() {
        comp.symbol = Some(le::new_symbol(&comp.name, "U", true));
    }
    if comp.footprint.is_none() {
        comp.footprint = Some(le::new_footprint(&comp.name, &comp.name, cadrs_eda::footprint::MountKind::ThroughHole));
    }
    Some((el, c, comp))
}

/// Applies `f` to the component and stores it as one undo step.
pub fn commit(w: &mut World, label: &str, f: impl FnOnce(&mut Component) -> Result<(), String>) -> bool {
    let Some((element, component, mut comp)) = current(w) else { return false };
    if let Err(e) = f(&mut comp) {
        ui::toast(w, &e);
        return false;
    }
    let cmd = SetComponent { element, component, value: Box::new(comp), label: label.into() };
    match w.resource_mut::<ActiveDocument>().execute(&cmd) {
        Ok(()) => true,
        Err(e) => {
            ui::toast(w, &e.to_string());
            false
        }
    }
}

fn grid(m: Mode) -> Nm {
    match m {
        Mode::Symbol => 25 * MIL,
        _ => mm(0.05),
    }
}

fn snap(p: Pt, g: Nm) -> Pt {
    cadrs_eda::sch_edit::snap(p, g)
}

fn tolerance(w: &World) -> Nm {
    let ui = w.resource::<super::EdaUi>();
    let scale = w.resource::<super::Eda2d>().0.and_then(|k| ui.views.get(&k)).map_or(4.0, |v| v.0.scale);
    mm(4.0 / scale)
}

fn on_strip(mut actions: MessageReader<StripAction>, mut commands: Commands) {
    for a in actions.read() {
        if matches!(a.0, Mode::Symbol | Mode::Footprint) {
            let (m, action) = (a.0, a.1);
            commands.queue(move |w: &mut World| run_action(w, m, action));
        }
    }
}

pub fn run_action(w: &mut World, m: Mode, action: &str) {
    let tool = match action {
        "select" => Some(Tool::Select),
        "pad" => Some(Tool::Pad),
        "line" => Some(Tool::Line(None)),
        "circle" => Some(Tool::Circle(None)),
        "rect" => Some(Tool::Rect(None)),
        _ => None,
    };
    if let Some(t) = tool {
        set_tool(w, t);
        return;
    }
    match (m, action) {
        (_, "pin") => super::part_dialogs::open_pin(w, None),
        (_, "repeat") => {
            commit(w, "Repeat pin", |c| {
                le::repeat_pin(c.symbol.as_mut().unwrap()).ok_or("Add a first pin to repeat")?;
                Ok(())
            });
        }
        (Mode::Symbol, "props") => super::part_dialogs::open_symbol_props(w),
        (Mode::Footprint, "props") => super::part_dialogs::open_footprint_props(w),
        (_, "push") => {
            let sel = w.resource::<PartState>().fp_selection.clone();
            commit(w, "Push pad properties", |c| {
                let f = c.footprint.as_mut().unwrap();
                let i = f.pads.iter().position(|p| sel.contains(&p.id)).ok_or("Select the pad to copy from")?;
                le::push_pad_properties(f, i);
                Ok(())
            });
        }
        (_, "layer") => {
            let mut s = w.resource_mut::<PartState>();
            s.layer = match s.layer {
                Layer::TopFab => Layer::TopSilk,
                Layer::TopSilk => Layer::TopCourtyard,
                _ => Layer::TopFab,
            };
            let name = s.layer.name();
            ui::toast(w, &format!("Drawing on {name}"));
        }
        (_, "model") => super::part_dialogs::open_model(w),
        (Mode::Symbol, "fromlib") => super::browser::open(w, super::browser::Kind::Symbols { power: false }, super::browser::Purpose::ComponentSymbol),
        (_, "fromlib") => {
            // Footprints the component's symbol allows, when it says.
            let globs = current(w).and_then(|(_, _, c)| c.symbol.map(|s| s.footprint_filters)).unwrap_or_default();
            super::browser::open(w, super::browser::Kind::Footprints { globs }, super::browser::Purpose::ComponentFootprint);
        }
        _ => {}
    }
}

pub fn set_tool(w: &mut World, t: Tool) {
    w.resource_mut::<PartState>().tool = t;
    w.resource_mut::<Preview>().set_component(None);
}

fn on_click(mut reader: MessageReader<EdaClick>, mut commands: Commands) {
    let clicks: Vec<EdaClick> = reader.read().copied().collect();
    if clicks.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        let Some(m) = mode(world) else { return };
        for c in clicks {
            match c {
                EdaClick::Press { at, shift, .. } => press(world, m, at, shift),
                EdaClick::Double { at } => double(world, m, at),
                EdaClick::Release { .. } => {}
            }
        }
    });
}

/// The shape a two-click tool makes from `a` to `b`.
fn shape_from(tool: Tool, a: Pt, b: Pt) -> Option<Geom> {
    match tool {
        Tool::Line(_) => Some(Geom::Line { a, b }),
        Tool::Rect(_) => Some(Geom::Rect { a, b }),
        Tool::Circle(_) => Some(Geom::Circle { center: a, radius: a.dist(b).round() as Nm }),
        _ => None,
    }
}

fn press(w: &mut World, m: Mode, at: Pt, shift: bool) {
    let tool = w.resource::<PartState>().tool;
    let p = snap(at, grid(m));
    match tool {
        Tool::Select => {
            let Some((_, _, c)) = current(w) else { return };
            let tol = tolerance(w);
            let mut s = w.resource_mut::<PartState>();
            match m {
                Mode::Symbol => {
                    let hit = le::symbol_hit(c.symbol.as_ref().unwrap(), at, tol);
                    match hit {
                        Some(h) if shift => s.sym_selection.push(h),
                        Some(h) => s.sym_selection = vec![h],
                        None => s.sym_selection.clear(),
                    }
                }
                _ => {
                    let f = c.footprint.as_ref().unwrap();
                    let pad = f.pads.iter().find(|p| cadrs_eda::poly::contains(&cadrs_eda::poly::pad_local(p, tol), at)).map(|p| p.id);
                    let shape = || {
                        f.shapes.iter().find(|sh| {
                            let (pts, closed) = cadrs_eda::poly::geom_points(&sh.shape.geom);
                            let r = if closed { cadrs_eda::poly::stroke_closed(&pts, sh.shape.stroke.width.max(1) + 2 * tol) } else { cadrs_eda::poly::stroke(&pts, sh.shape.stroke.width.max(1) + 2 * tol) };
                            cadrs_eda::poly::contains(&r, at)
                        })
                        .map(|sh| sh.id)
                    };
                    match pad.or_else(shape) {
                        Some(h) if shift => s.fp_selection.push(h),
                        Some(h) => s.fp_selection = vec![h],
                        None => s.fp_selection.clear(),
                    }
                }
            }
        }
        Tool::Pad => {
            commit(w, "Add pad", |c| {
                le::add_pad(c.footprint.as_mut().unwrap(), p);
                Ok(())
            });
        }
        Tool::Line(None) | Tool::Circle(None) | Tool::Rect(None) => {
            let next = match tool {
                Tool::Line(_) => Tool::Line(Some(p)),
                Tool::Circle(_) => Tool::Circle(Some(p)),
                _ => Tool::Rect(Some(p)),
            };
            w.resource_mut::<PartState>().tool = next;
        }
        Tool::Line(Some(a)) | Tool::Circle(Some(a)) | Tool::Rect(Some(a)) => {
            let Some(geom) = shape_from(tool, a, p) else { return };
            let layer = w.resource::<PartState>().layer;
            commit(w, "Draw", |c| {
                match m {
                    Mode::Symbol => le::add_symbol_shape(c.symbol.as_mut().unwrap(), geom, mm(0.254), Fill::None),
                    _ => {
                        let width = match layer {
                            Layer::TopSilk => mm(0.12),
                            Layer::TopCourtyard => mm(0.05),
                            _ => mm(0.1),
                        };
                        le::add_fp_shape(c.footprint.as_mut().unwrap(), geom, layer, width);
                    }
                }
                Ok(())
            });
            // Lines chain on from where the last ended.
            let next = if let Tool::Line(_) = tool { Tool::Line(Some(p)) } else { tool_reset(tool) };
            w.resource_mut::<PartState>().tool = next;
        }
    }
}

fn tool_reset(t: Tool) -> Tool {
    match t {
        Tool::Circle(_) => Tool::Circle(None),
        Tool::Rect(_) => Tool::Rect(None),
        Tool::Line(_) => Tool::Line(None),
        t => t,
    }
}

fn double(w: &mut World, m: Mode, at: Pt) {
    if let Tool::Line(_) = w.resource::<PartState>().tool {
        w.resource_mut::<PartState>().tool = Tool::Line(None);
        return;
    }
    let Some((_, _, c)) = current(w) else { return };
    let tol = tolerance(w);
    match m {
        Mode::Symbol => {
            if let Some(SymbolPart::Pin(i)) = le::symbol_hit(c.symbol.as_ref().unwrap(), at, tol) {
                super::part_dialogs::open_pin(w, Some(i));
            }
        }
        _ => {
            let f = c.footprint.as_ref().unwrap();
            if let Some(i) = f.pads.iter().position(|p| cadrs_eda::poly::contains(&cadrs_eda::poly::pad_local(p, tol), at)) {
                super::part_dialogs::open_pad(w, i);
            }
        }
    }
}

fn on_keys(mut reader: MessageReader<KeyboardInput>, mut commands: Commands) {
    let keys: Vec<KeyboardInput> = reader.read().cloned().collect();
    if !keys.is_empty() {
        commands.queue(move |world: &mut World| handle_keys(world, keys));
    }
}

fn handle_keys(world: &mut World, keys: Vec<KeyboardInput>) {
    let Some(m) = mode(world) else { return };
    if !ui::keys_for(world, m) {
        return;
    }
    for k in keys {
        if k.state != ButtonState::Pressed {
            continue;
        }
        match k.key_code {
            KeyCode::Escape => {
                let idle = world.resource::<PartState>().tool == Tool::Select;
                set_tool(world, Tool::Select);
                if idle {
                    let mut s = world.resource_mut::<PartState>();
                    s.sym_selection.clear();
                    s.fp_selection.clear();
                }
            }
            KeyCode::Insert if m == Mode::Symbol => run_action(world, m, "repeat"),
            KeyCode::KeyE => {
                let s = world.resource::<PartState>();
                match m {
                    Mode::Symbol => match s.sym_selection.first().copied() {
                        Some(SymbolPart::Pin(i)) => super::part_dialogs::open_pin(world, Some(i)),
                        _ => super::part_dialogs::open_symbol_props(world),
                    },
                    _ => {
                        let sel = s.fp_selection.first().copied();
                        let Some((_, _, c)) = current(world) else { return };
                        let f = c.footprint.unwrap();
                        if let Some(i) = sel.and_then(|id| f.pads.iter().position(|p| p.id == id)) {
                            super::part_dialogs::open_pad(world, i);
                        } else if let Some(i) = sel.and_then(|id| f.shapes.iter().position(|s| s.id == id)) {
                            super::part_dialogs::open_rect(world, i);
                        } else {
                            super::part_dialogs::open_footprint_props(world);
                        }
                    }
                }
            }
            KeyCode::Delete | KeyCode::Backspace => {
                let (sym, fp) = {
                    let s = world.resource::<PartState>();
                    (s.sym_selection.clone(), s.fp_selection.clone())
                };
                commit(world, "Delete", |c| {
                    if m == Mode::Symbol {
                        le::delete_symbol_parts(c.symbol.as_mut().unwrap(), &sym);
                    } else {
                        let f = c.footprint.as_mut().unwrap();
                        f.pads.retain(|p| !fp.contains(&p.id));
                        f.shapes.retain(|s| !fp.contains(&s.id));
                    }
                    Ok(())
                });
                let mut s = world.resource_mut::<PartState>();
                s.sym_selection.clear();
                s.fp_selection.clear();
            }
            _ => {}
        }
    }
}

/// Previews the shape a two-click tool is drawing.
fn follow_pointer(world: &mut World, mut last: Local<Option<Pt>>) {
    let Some(m) = mode(world) else { return };
    let tool = world.resource::<PartState>().tool;
    let a = match tool {
        Tool::Line(Some(a)) | Tool::Circle(Some(a)) | Tool::Rect(Some(a)) => a,
        _ => {
            if last.take().is_some() {
                world.resource_mut::<Preview>().set_component(None);
            }
            return;
        }
    };
    let p = snap(world.resource::<EdaPointer>().at, grid(m));
    if *last == Some(p) {
        return;
    }
    *last = Some(p);
    let Some((_, _, mut c)) = current(world) else { return };
    let Some(geom) = shape_from(tool, a, p) else { return };
    let layer = world.resource::<PartState>().layer;
    match m {
        Mode::Symbol => le::add_symbol_shape(c.symbol.as_mut().unwrap(), geom, mm(0.254), Fill::None),
        _ => {
            le::add_fp_shape(c.footprint.as_mut().unwrap(), geom, layer, mm(0.1));
        }
    }
    world.resource_mut::<Preview>().set_component(Some(c));
}

fn publish(s: Res<PartState>, eda: Res<super::Eda2d>, mut inputs: ResMut<SceneInputs>) {
    if eda.component().is_none() {
        return;
    }
    if inputs.sym_selected != s.sym_selection {
        inputs.sym_selected = s.sym_selection.clone();
    }
    if inputs.board_selected != s.fp_selection {
        inputs.board_selected = s.fp_selection.clone();
    }
}

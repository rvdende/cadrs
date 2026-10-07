//! The Schematic view's tools (docs/PLAN.md GS3–GS12), keys as in the guide:
//!
//! - **Select** (Esc): click picks (a field alone when its text is clicked), Shift+click adds,
//!   Ctrl+click toggles; dragging on empty paper boxes — left to right takes what is wholly
//!   inside, right to left what the box touches; dragging an item moves it with its
//!   wires attached (they stay on the pins and square: corners slide, bends are added).
//!   **M** moves the selection (wires stay), **G** drags it (wires follow),
//!   **R** rotates it and **X** / **Y** mirror it (wires on it follow, square), **Del**
//!   deletes it, **E** (or a double-click) edits a symbol's fields (Footprint: Choose… browses
//!   the footprints its filters allow) or a wire's colour (its whole connected run).
//! - **A** the library browser ([`super::browser`]), **P** the same for power symbols; the
//!   chosen symbol follows the pointer (**R** turns it, **X** / **Y** mirror it) and a click
//!   places it (annotated). Tools that put something at a point show a crosshair on the grid
//!   point they will use, its square filled green when that point connects (a wire or pin).
//!   The sheet shows the grid as dots.
//! - **W** wire: click, click, … ; double-click (or a click on a pin or wire) ends; Esc cancels.
//! - **X** / **Y** mirror the selection left for right / top for bottom; **Ctrl+C** copies,
//!   **Ctrl+V** pastes and **Ctrl+D** duplicates (both follow the pointer to a click);
//!   **Ctrl+L** a global label, **T** a text note; **`** highlights the net under the pointer
//!   (again, or Esc: off); **Ctrl+F** finds a symbol by reference or value.
//! - **L** net label (its name asked first; **R** turns it while it follows the pointer),
//!   **Q** no-connect flag.
//! - The strip: those tools, Annotate, Assign footprints, ERC, BOM export, Page settings.
//!
//! Every change is one undo step (`ui::commit`).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

use cadrs_eda::Design;
use cadrs_eda::sch_edit::{self as se, SchItem};
use cadrs_eda::schematic::LabelKind;
use cadrs_eda::symbol::Symbol;
use cadrs_eda::units::{Bounds, Pt, SCHEMATIC_GRID, mm};

use super::ui::{self, StripAction};
use super::{EdaClick, EdaPointer, Mode, Preview, SceneInputs};
use crate::ActiveDocument;
use crate::AppState;

const STRIP: ui::StripSpec = &[
    Some(("sch-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some(("sch-add-symbol", "add-symbol", "Add a symbol (A)", "symbol")),
    Some(("sch-add-power", "add-power", "Add a power symbol (P)", "power")),
    Some(("sch-wire", "wire", "Add a wire (W)", "wire")),
    Some(("sch-label", "net-label", "Add a net label (L)", "label")),
    Some(("sch-no-connect", "no-connect", "Add a no-connect flag (Q)", "noconnect")),
    None,
    Some(("sch-annotate", "annotate", "Fill in reference designators", "annotate")),
    Some(("sch-assign", "assign-footprints", "Assign footprints", "assign")),
    Some(("sch-erc", "erc", "Electrical rules check", "erc")),
    Some(("sch-fields", "custom-table", "Symbol fields table", "fields")),
    Some(("sch-bom", "bill-of-materials", "Bill of materials", "bom")),
    Some(("sch-page", "properties", "Page settings", "page")),
    Some(("sch-plot", "file-export", "Plot the schematic (SVG, PDF) or export its netlist", "plot")),
];

#[derive(Clone, Debug, Default)]
pub enum Tool {
    #[default]
    Select,
    /// A symbol following the pointer, placed on the next click.
    Place(Box<Symbol>),
    /// The wire so far (its points).
    Wire(Vec<Pt>),
    /// A label of this name, placed on the next click.
    Label(String),
    NoConnect,
    /// Copied or duplicated items following the pointer, pasted on the next click.
    Paste(Box<se::SchClip>),
    /// A global label of this name, placed on the next click.
    GlobalLabel(String),
    /// A text note, placed on the next click.
    Text(String),
}

impl Tool {
    fn action(&self) -> &'static str {
        match self {
            Tool::Select => "select",
            Tool::Place(s) if s.power => "power",
            Tool::Place(_) => "symbol",
            Tool::Wire(_) => "wire",
            Tool::Label(_) => "label",
            Tool::NoConnect => "noconnect",
            Tool::Paste(_) => "paste",
            Tool::GlobalLabel(_) => "label",
            Tool::Text(_) => "text",
        }
    }
}

/// A move in progress: the items, where it started, wires following (G) or not (M), and
/// whether it ends on release (a mouse drag) or on a click.
#[derive(Clone, Debug)]
pub struct Moving {
    pub items: Vec<SchItem>,
    pub from: Pt,
    pub drag: bool,
    pub on_release: bool,
}

/// The Schematic tools' state (view state: not saved, not undone).
#[derive(Resource, Default, Debug)]
pub struct SchState {
    pub tool: Tool,
    pub selection: Vec<SchItem>,
    pub moving: Option<Moving>,
    /// A box selection started here.
    pub boxing: Option<Pt>,
    /// What Ctrl+C copied.
    pub clipboard: Option<se::SchClip>,
    /// How the symbol or label being placed is turned (R: quarter turns) and mirrored (X, Y).
    pub orient: Orient,
    /// The net highlighted (` on a wire or pin): its name and items.
    pub net: Option<(String, Vec<SchItem>)>,
}


/// How a symbol or label being placed sits: quarter turns counter-clockwise, then mirrored left
/// for right (X) and top for bottom (Y).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Orient {
    pub turns: u8,
    pub mirror_x: bool,
    pub mirror_y: bool,
}

/// Places a symbol at `at`, turned and mirrored as `o` says (no wires are touched).
fn place_oriented(d: &mut Design, sym: &Symbol, at: Pt, id: uuid::Uuid, o: Orient) {
    let id = se::place_symbol(&mut d.schematic, 0, sym, at, id);
    let item = [SchItem::Symbol(id)];
    for _ in 0..o.turns {
        se::turn_items(&mut d.schematic, 0, &item, at);
    }
    if o.mirror_x {
        se::flip_items(&mut d.schematic, 0, &item, at, false);
    }
    if o.mirror_y {
        se::flip_items(&mut d.schematic, 0, &item, at, true);
    }
}

/// A label's angle for an orientation: turned, and pointed the other way by a mirror across it.
fn label_angle(o: Orient) -> f64 {
    let mut a = o.turns as f64 * 90.0;
    let level = o.turns.is_multiple_of(2);
    if (o.mirror_x && level) || (o.mirror_y && !level) {
        a += 180.0;
    }
    cadrs_eda::units::normalize_deg(a)
}

pub fn register(app: &mut App) {
    app.init_resource::<SchState>().add_systems(
        Update,
        (strip, on_strip, on_click, on_keys, follow_pointer, publish).chain().after(super::navigate).run_if(in_state(AppState::Document)),
    );
    super::sch_dialogs::register(app);
}

fn strip(world: &mut World) {
    ui::sync_strip(world, Mode::Schematic, STRIP);
    let a = world.resource::<SchState>().tool.action();
    ui::mark_active(world, Mode::Schematic, a);
}

fn snap(p: Pt) -> Pt {
    se::snap(p, SCHEMATIC_GRID)
}

fn tolerance(world: &World) -> i64 {
    // Five pixels at the current zoom.
    let ui = world.resource::<super::EdaUi>();
    let key = world.resource::<super::Eda2d>().0;
    let scale = key.and_then(|k| ui.views.get(&k)).map_or(4.0, |v| v.0.scale);
    mm(5.0 / scale)
}

fn on_strip(mut actions: MessageReader<StripAction>, mut commands: Commands) {
    for a in actions.read() {
        if a.0 != Mode::Schematic {
            continue;
        }
        let action = a.1;
        commands.queue(move |w: &mut World| run_action(w, action));
    }
}

/// A strip button or its key.
pub fn run_action(w: &mut World, action: &str) {
    match action {
        "select" => set_tool(w, Tool::Select),
        "symbol" => super::browser::open(w, super::browser::Kind::Symbols { power: false }, super::browser::Purpose::Place),
        "power" => super::browser::open(w, super::browser::Kind::Symbols { power: true }, super::browser::Purpose::Place),
        "wire" => set_tool(w, Tool::Wire(vec![])),
        "label" => dialogs::open_label(w),
        "noconnect" => set_tool(w, Tool::NoConnect),
        "annotate" => {
            ui::commit(w, "Annotate", |d| {
                se::annotate(&mut d.schematic, false);
                Ok(())
            });
        }
        "assign" => dialogs::open_assign(w),
        "erc" => dialogs::open_erc(w),
        "bom" => dialogs::open_bom(w),
        "page" => dialogs::open_page(w),
        "plot" => dialogs::open_plot(w),
        "fields" => dialogs::open_fields_table(w),
        _ => {}
    }
}

pub fn set_tool(w: &mut World, tool: Tool) {
    let mut s = w.resource_mut::<SchState>();
    s.tool = tool;
    s.orient = Orient::default();
    s.moving = None;
    s.boxing = None;
    w.resource_mut::<Preview>().set(None);
}

fn shown(w: &World) -> bool {
    w.resource::<super::Eda2d>().0.is_some_and(|(_, _, m)| m == Mode::Schematic)
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
                EdaClick::Press { at, shift, ctrl } => press(world, at, shift, ctrl),
                EdaClick::Release { at } => release(world, at),
                EdaClick::Double { at } => double(world, at),
            }
        }
    });
}

fn press(w: &mut World, at: Pt, shift: bool, ctrl: bool) {
    let tool = w.resource::<SchState>().tool.clone();
    let orient = w.resource::<SchState>().orient;
    // A move started with M or G ends with a click.
    if let Some(m) = w.resource::<SchState>().moving.clone().filter(|m| !m.on_release) {
        finish_move(w, &m, at);
        return;
    }
    match tool {
        Tool::Select => {
            let Some((_, _, d)) = ui::current(w) else { return };
            let hit = se::hit(&d.schematic, 0, at, tolerance(w));
            let mut s = w.resource_mut::<SchState>();
            match hit {
                Some(item) => {
                    if ctrl {
                        if let Some(i) = s.selection.iter().position(|x| *x == item) {
                            s.selection.remove(i);
                        } else {
                            s.selection.push(item);
                        }
                    } else if shift {
                        if !s.selection.contains(&item) {
                            s.selection.push(item);
                        }
                    } else {
                        if !s.selection.contains(&item) {
                            s.selection = vec![item];
                        }
                        // Pressing on a selected item starts dragging it.
                        let items = s.selection.clone();
                        s.moving = Some(Moving { items, from: snap(at), drag: true, on_release: true });
                    }
                }
                None => {
                    if !shift && !ctrl {
                        s.selection.clear();
                    }
                    s.boxing = Some(at);
                }
            }
        }
        Tool::Place(sym) => {
            let at = snap(at);
            let mark = w.resource::<ActiveDocument>().history.undo_len();
            let placed = ui::commit(w, "Place symbol", |d| {
                place_oriented(d, &sym, at, uuid::Uuid::new_v4(), orient);
                Ok(())
            });
            // A library part placed for the first time joins the studio's Components (one
            // undo step with the placing).
            if placed && let Some((element, _, _)) = ui::current(w) && ui::add_library_component(w, element, &sym) {
                w.resource_mut::<ActiveDocument>().squash_element_since(mark, element, "Place symbol");
            }
            set_tool(w, Tool::Select);
        }
        Tool::Wire(mut pts) => {
            let at = snap(at);
            if let Some(last) = pts.last().copied() {
                pts.extend(se::manhattan(last, at).into_iter().skip(1));
                // Ending on a pin or another wire finishes the wire.
                let ends_on_something = ui::current(w).is_some_and(|(_, _, d)| {
                    se::pin_at(&d.schematic, 0, at, 1000).is_some() || d.schematic.sheets[0].wires.iter().any(|x| cadrs_eda::connectivity::on_segment(at, x.a, x.b))
                });
                if ends_on_something {
                    finish_wire(w, &pts);
                    return;
                }
            } else {
                pts.push(at);
            }
            w.resource_mut::<SchState>().tool = Tool::Wire(pts);
        }
        Tool::Label(text) => {
            let at = snap(at);
            ui::commit(w, "Add label", |d| {
                se::add_label(&mut d.schematic, 0, &text, at, label_angle(orient), LabelKind::Local);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::NoConnect => {
            let at = snap(at);
            ui::commit(w, "Add no-connect flag", |d| {
                se::add_no_connect(&mut d.schematic, 0, at);
                Ok(())
            });
        }
        Tool::Paste(clip) => {
            let mut pasted = vec![];
            ui::commit(w, "Paste", |d| {
                pasted = se::paste(&mut d.schematic, 0, &clip, at);
                se::fix_junctions(&mut d.schematic, 0);
                Ok(())
            });
            set_tool(w, Tool::Select);
            w.resource_mut::<SchState>().selection = pasted;
        }
        Tool::GlobalLabel(text) => {
            let at = snap(at);
            ui::commit(w, "Add global label", |d| {
                se::add_label(&mut d.schematic, 0, &text, at, label_angle(orient), LabelKind::Global(Default::default()));
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
        Tool::Text(text) => {
            let at = snap(at);
            ui::commit(w, "Add text", |d| {
                se::add_note(&mut d.schematic, 0, &text, at);
                Ok(())
            });
            set_tool(w, Tool::Select);
        }
    }
}

fn finish_wire(w: &mut World, pts: &[Pt]) {
    if pts.len() >= 2 {
        let pts = pts.to_vec();
        ui::commit(w, "Add wire", |d| {
            se::add_wire(&mut d.schematic, 0, &pts);
            Ok(())
        });
    }
    w.resource_mut::<SchState>().tool = Tool::Wire(vec![]);
    w.resource_mut::<Preview>().set(None);
}

fn release(w: &mut World, at: Pt) {
    if let Some(m) = w.resource::<SchState>().moving.clone().filter(|m| m.on_release) {
        if snap(at) != m.from {
            finish_move(w, &m, at);
        } else {
            let mut s = w.resource_mut::<SchState>();
            s.moving = None;
            w.resource_mut::<Preview>().set(None);
        }
        return;
    }
    let Some(start) = w.resource_mut::<SchState>().boxing.take() else { return };
    if start.dist(at) < tolerance(w) as f64 {
        return;
    }
    let Some((_, _, d)) = ui::current(w) else { return };
    let r = Bounds { min: Pt::new(start.x.min(at.x), start.y.min(at.y)), max: Pt::new(start.x.max(at.x), start.y.max(at.y)) };
    let crossing = at.x < start.x;
    let found = se::box_select(&d.schematic, 0, r, crossing);
    let mut s = w.resource_mut::<SchState>();
    for f in found {
        if !s.selection.contains(&f) {
            s.selection.push(f);
        }
    }
}

fn double(w: &mut World, at: Pt) {
    let tool = w.resource::<SchState>().tool.clone();
    match tool {
        Tool::Wire(pts) => finish_wire(w, &pts),
        Tool::Select => {
            // Double-clicking a symbol edits its fields; a wire, its colour.
            let Some((_, _, d)) = ui::current(w) else { return };
            match se::hit(&d.schematic, 0, at, tolerance(w)) {
                Some(SchItem::Symbol(id) | SchItem::Field(id, _)) => {
                    w.resource_mut::<SchState>().moving = None;
                    dialogs::open_properties(w, id);
                }
                Some(SchItem::Wire(id)) => {
                    w.resource_mut::<SchState>().moving = None;
                    dialogs::open_wire(w, id);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

fn finish_move(w: &mut World, m: &Moving, at: Pt) {
    let d = snap(at) - m.from;
    let (items, drag) = (m.items.clone(), m.drag);
    w.resource_mut::<SchState>().moving = None;
    w.resource_mut::<Preview>().set(None);
    if d == Pt::ZERO {
        return;
    }
    ui::commit(w, if drag { "Drag" } else { "Move" }, |des| {
        se::move_items(&mut des.schematic, 0, &items, d, drag);
        se::fix_junctions(&mut des.schematic, 0);
        Ok(())
    });
}

fn on_keys(mut reader: MessageReader<KeyboardInput>, mut commands: Commands) {
    let keys: Vec<KeyboardInput> = reader.read().cloned().collect();
    if !keys.is_empty() {
        commands.queue(move |world: &mut World| handle_keys(world, keys));
    }
}

fn handle_keys(world: &mut World, keys: Vec<KeyboardInput>) {
    if !ui::keys_for(world, Mode::Schematic) {
        return;
    }
    let held = world.resource::<ButtonInput<KeyCode>>().clone();
    let (ctrl, shift) = (held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]), held.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]));
    for k in keys {
        if k.state != ButtonState::Pressed {
            continue;
        }
        let pointer = world.resource::<EdaPointer>().at;
        let sel = world.resource::<SchState>().selection.clone();
        if ctrl {
            match k.key_code {
                // Copy (from under the pointer); paste or duplicate follow the pointer.
                KeyCode::KeyC | KeyCode::KeyD => {
                    let items = if sel.is_empty() { hit_items(world, pointer) } else { sel };
                    let Some((_, _, d)) = ui::current(world) else { continue };
                    let clip = se::copy_items(&d.schematic, 0, &items, pointer);
                    if clip.is_empty() {
                        continue;
                    }
                    if k.key_code == KeyCode::KeyD {
                        set_tool(world, Tool::Paste(Box::new(clip)));
                    } else {
                        world.resource_mut::<SchState>().clipboard = Some(clip);
                    }
                }
                KeyCode::KeyV => {
                    if let Some(clip) = world.resource::<SchState>().clipboard.clone() {
                        set_tool(world, Tool::Paste(Box::new(clip)));
                    }
                }
                KeyCode::KeyL => dialogs::open_global_label(world),
                KeyCode::KeyF => dialogs::open_find(world),
                _ => {}
            }
            continue;
        }
        // While placing a symbol or a label: R turns it, X and Y mirror it.
        let placing = matches!(world.resource::<SchState>().tool, Tool::Place(_) | Tool::Label(_) | Tool::GlobalLabel(_));
        if placing && matches!(k.key_code, KeyCode::KeyR | KeyCode::KeyX | KeyCode::KeyY) {
            let mut s = world.resource_mut::<SchState>();
            let o = &mut s.orient;
            match k.key_code {
                KeyCode::KeyR => o.turns = (o.turns + 1) % 4,
                KeyCode::KeyX => o.mirror_x = !o.mirror_x,
                _ => o.mirror_y = !o.mirror_y,
            }
            continue;
        }
        match k.key_code {
            KeyCode::Escape => {
                let had = !matches!(world.resource::<SchState>().tool, Tool::Select) || world.resource::<SchState>().moving.is_some();
                set_tool(world, Tool::Select);
                if !had {
                    world.resource_mut::<SchState>().selection.clear();
                    world.resource_mut::<SchState>().net = None;
                }
            }
            KeyCode::KeyA if !shift => run_action(world, "symbol"),
            KeyCode::KeyP => run_action(world, "power"),
            KeyCode::KeyW => run_action(world, "wire"),
            KeyCode::KeyL => run_action(world, "label"),
            KeyCode::KeyT => dialogs::open_text(world),
            // Mirror the selection (or what is under the pointer) left for right, or top for bottom.
            KeyCode::KeyX | KeyCode::KeyY => {
                let items = if sel.is_empty() { hit_items(world, pointer) } else { sel };
                if !items.is_empty() {
                    let up_down = k.key_code == KeyCode::KeyY;
                    ui::commit(world, "Mirror", |d| {
                        let c = snap(se::selection_center(&d.schematic, 0, &items));
                        se::mirror_items(&mut d.schematic, 0, &items, c, up_down);
                        se::fix_junctions(&mut d.schematic, 0);
                        Ok(())
                    });
                }
            }
            // Highlight the net under the pointer (again: off).
            KeyCode::Backquote => {
                let tol = tolerance(world);
                let found = ui::current(world).and_then(|(_, _, d)| se::net_items_at(&d.schematic, 0, pointer, tol));
                let mut s = world.resource_mut::<SchState>();
                s.net = if found.as_ref().map(|f| &f.0) == s.net.as_ref().map(|n| &n.0) { None } else { found };
            }
            KeyCode::KeyQ => run_action(world, "noconnect"),
            KeyCode::KeyM | KeyCode::KeyG => {
                let items = if sel.is_empty() { hit_items(world, pointer) } else { sel };
                if !items.is_empty() {
                    world.resource_mut::<SchState>().selection = items.clone();
                    world.resource_mut::<SchState>().moving = Some(Moving { items, from: snap(pointer), drag: k.key_code == KeyCode::KeyG, on_release: false });
                }
            }
            KeyCode::KeyR => {
                let items = if sel.is_empty() { hit_items(world, pointer) } else { sel };
                if !items.is_empty() {
                    ui::commit(world, "Rotate", |d| {
                        let c = se::selection_center(&d.schematic, 0, &items);
                        se::rotate_items(&mut d.schematic, 0, &items, c);
                        se::fix_junctions(&mut d.schematic, 0);
                        Ok(())
                    });
                }
            }
            KeyCode::Delete | KeyCode::Backspace if !sel.is_empty() => {
                ui::commit(world, "Delete", |d| {
                    se::delete_items(&mut d.schematic, 0, &sel);
                    se::fix_junctions(&mut d.schematic, 0);
                    Ok(())
                });
                world.resource_mut::<SchState>().selection.clear();
            }
            KeyCode::KeyE => {
                // A symbol's fields, or a wire's colour.
                let target = sel.iter().chain(hit_items(world, pointer).iter()).find(|i| matches!(i, SchItem::Symbol(_) | SchItem::Field(..) | SchItem::Wire(_))).cloned();
                match target {
                    Some(SchItem::Symbol(id) | SchItem::Field(id, _)) => dialogs::open_properties(world, id),
                    Some(SchItem::Wire(id)) => dialogs::open_wire(world, id),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn hit_items(w: &World, at: Pt) -> Vec<SchItem> {
    let Some((_, _, d)) = ui::current(w) else { return vec![] };
    se::hit(&d.schematic, 0, at, tolerance(w)).into_iter().collect()
}

/// Shows what the tool would do at the pointer: a symbol to place, the wire so far, a move.
fn follow_pointer(world: &mut World, mut last: Local<Option<(Pt, u64)>>) {
    // The crosshair of a tool that puts something down at a point: on the grid point it will
    // use, marked when that point connects (a wire or a pin is there).
    let placing = shown(world) && matches!(world.resource::<SchState>().tool, Tool::Place(_) | Tool::Wire(_) | Tool::Label(_) | Tool::GlobalLabel(_) | Tool::NoConnect);
    let cross = placing.then(|| {
        let at = snap(world.resource::<EdaPointer>().at);
        let connects = ui::current(world).is_some_and(|(_, _, d)| se::pin_at(&d.schematic, 0, at, 1000).is_some() || d.schematic.sheets[0].wires.iter().any(|w| cadrs_eda::connectivity::on_segment(at, w.a, w.b)));
        (at, connects)
    });
    if world.resource::<super::Crosshair>().0 != cross {
        world.resource_mut::<super::Crosshair>().0 = cross;
    }
    if !shown(world) {
        return;
    }
    let at = snap(world.resource::<EdaPointer>().at);
    let s = world.resource::<SchState>();
    let key = (at, s.selection.len() as u64 ^ (s.tool.action().len() as u64) << 8);
    let tool = s.tool.clone();
    let orient = s.orient;
    let moving = s.moving.clone();
    if *last == Some(key) && moving.is_none() && matches!(tool, Tool::Select) {
        return;
    }
    *last = Some(key);
    let Some((_, _, mut d)) = ui::current(world) else { return };
    let preview: Option<Design> = if let Some(m) = moving {
        let delta = at - m.from;
        se::move_items(&mut d.schematic, 0, &m.items, delta, m.drag);
        Some(d)
    } else {
        match tool {
            Tool::Place(sym) => {
                place_oriented(&mut d, &sym, at, uuid::Uuid::nil(), orient);
                Some(d)
            }
            Tool::Wire(pts) if !pts.is_empty() => {
                let mut all = pts.clone();
                all.extend(se::manhattan(*pts.last().unwrap(), at).into_iter().skip(1));
                for s in all.windows(2) {
                    if s[0] != s[1] {
                        d.schematic.sheets[0].wires.push(cadrs_eda::schematic::Wire { id: uuid::Uuid::nil(), a: s[0], b: s[1], stroke: Default::default() });
                    }
                }
                Some(d)
            }
            Tool::Label(text) => {
                se::add_label(&mut d.schematic, 0, &text, at, label_angle(orient), LabelKind::Local);
                Some(d)
            }
            Tool::GlobalLabel(text) => {
                se::add_label(&mut d.schematic, 0, &text, at, label_angle(orient), LabelKind::Global(Default::default()));
                Some(d)
            }
            Tool::Text(text) => {
                se::add_note(&mut d.schematic, 0, &text, at);
                Some(d)
            }
            Tool::Paste(clip) => {
                se::paste(&mut d.schematic, 0, &clip, at);
                Some(d)
            }
            _ => None,
        }
    };
    let had = world.resource::<Preview>().get().is_some();
    if preview.is_some() || had {
        world.resource_mut::<Preview>().set(preview);
    }
}

/// The selection shown highlighted.
fn publish(s: Res<SchState>, mut inputs: ResMut<SceneInputs>) {
    // The selection, and the highlighted net.
    let mut hl = s.selection.clone();
    if let Some((_, items)) = &s.net {
        hl.extend(items.iter().filter(|i| !s.selection.contains(i)).cloned());
    }
    if inputs.sch_highlight != hl {
        inputs.sch_highlight = hl;
    }
}

// ---------------------------------------------------------------------------------------------

pub mod dialogs {
    //! The Schematic's dialogs: label name, symbol fields (E), page settings, footprint
    //! assignment, ERC, BOM.
    pub use super::super::sch_dialogs::*;
}

//! The Schematic view's tools (docs/PLAN.md GS3–GS12), keys as in the guide:
//!
//! - **Select** (Esc): click picks (a field alone when its text is clicked), Shift+click adds,
//!   Ctrl+click toggles; dragging on empty paper boxes — left to right takes what is wholly
//!   inside, right to left what the box touches; dragging a selected item moves it with its
//!   wires attached. **M** moves the selection (wires stay), **G** drags it (wires follow),
//!   **R** rotates it, **Del** deletes it, **E** edits the symbol's fields.
//! - **A** the symbol chooser, **P** the power chooser; the chosen symbol follows the pointer
//!   and a click places it (annotated).
//! - **W** wire: click, click, … ; double-click (or a click on a pin or wire) ends; Esc cancels.
//! - **L** net label (its name asked first), **Q** no-connect flag.
//! - The strip: those tools, Annotate, Assign footprints, ERC, BOM export, Page settings.
//!
//! Every change is one undo step (`ui::commit`).

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;

use bevy::ui_widgets::{Activate, observe};
use cadrs_eda::Design;
use cadrs_eda::sch_edit::{self as se, SchItem};
use cadrs_eda::schematic::LabelKind;
use cadrs_eda::symbol::Symbol;
use cadrs_eda::units::{Bounds, Pt, SCHEMATIC_GRID, mm};
use cadrs_ui::prelude::*;
use cadrs_ui::{Dialog, TextSubmit};

use super::ui::{self, StripAction};
use super::{EdaClick, EdaPointer, Mode, Preview, SceneInputs};
use crate::AppState;

const STRIP: ui::StripSpec = &[
    Some(("sch-select", "drag-handle", "Select (Esc)", "select")),
    None,
    Some(("sch-add-symbol", "chip", "Add a symbol (A)", "symbol")),
    Some(("sch-add-power", "plus", "Add a power symbol (P)", "power")),
    Some(("sch-wire", "line", "Add a wire (W)", "wire")),
    Some(("sch-label", "text", "Add a net label (L)", "label")),
    Some(("sch-no-connect", "close", "Add a no-connect flag (Q)", "noconnect")),
    None,
    Some(("sch-annotate", "tag", "Fill in reference designators", "annotate")),
    Some(("sch-assign", "link", "Assign footprints", "assign")),
    Some(("sch-erc", "diagnostics", "Electrical rules check", "erc")),
    Some(("sch-bom", "bill-of-materials", "Bill of materials", "bom")),
    Some(("sch-page", "properties", "Page settings", "page")),
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
    /// The chooser lists power symbols only.
    pub chooser_power: bool,
}

pub fn register(app: &mut App) {
    app.init_resource::<SchState>().add_systems(
        Update,
        (strip, on_strip, on_click, on_keys, follow_pointer, publish).chain().after(super::navigate).run_if(in_state(AppState::Document)),
    );
    app.add_systems(Update, chooser::refresh.run_if(in_state(AppState::Document)));
    app.add_observer(chooser::on_row).add_observer(chooser::on_submit);
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
        "symbol" => chooser::open(w, false),
        "power" => chooser::open(w, true),
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
        _ => {}
    }
}

pub fn set_tool(w: &mut World, tool: Tool) {
    let mut s = w.resource_mut::<SchState>();
    s.tool = tool;
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
            ui::commit(w, "Place symbol", |d| {
                se::place_symbol(&mut d.schematic, 0, &sym, at, uuid::Uuid::new_v4());
                Ok(())
            });
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
                se::add_label(&mut d.schematic, 0, &text, at, 0.0, LabelKind::Local);
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
            // Double-clicking a symbol edits its fields.
            let Some((_, _, d)) = ui::current(w) else { return };
            if let Some(SchItem::Symbol(id) | SchItem::Field(id, _)) = se::hit(&d.schematic, 0, at, tolerance(w)) {
                w.resource_mut::<SchState>().moving = None;
                dialogs::open_properties(w, id);
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
        if k.state != ButtonState::Pressed || ctrl {
            continue;
        }
        let pointer = world.resource::<EdaPointer>().at;
        let sel = world.resource::<SchState>().selection.clone();
        match k.key_code {
            KeyCode::Escape => {
                let had = !matches!(world.resource::<SchState>().tool, Tool::Select) || world.resource::<SchState>().moving.is_some();
                set_tool(world, Tool::Select);
                if !had {
                    world.resource_mut::<SchState>().selection.clear();
                }
            }
            KeyCode::KeyA if !shift => run_action(world, "symbol"),
            KeyCode::KeyP => run_action(world, "power"),
            KeyCode::KeyW => run_action(world, "wire"),
            KeyCode::KeyL => run_action(world, "label"),
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
                let target = sel.iter().chain(hit_items(world, pointer).iter()).find_map(|i| match i {
                    SchItem::Symbol(id) | SchItem::Field(id, _) => Some(*id),
                    _ => None,
                });
                if let Some(id) = target {
                    dialogs::open_properties(world, id);
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
    if !shown(world) {
        return;
    }
    let at = snap(world.resource::<EdaPointer>().at);
    let s = world.resource::<SchState>();
    let key = (at, s.selection.len() as u64 ^ (s.tool.action().len() as u64) << 8);
    let tool = s.tool.clone();
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
                se::place_symbol(&mut d.schematic, 0, &sym, at, uuid::Uuid::nil());
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
                se::add_label(&mut d.schematic, 0, &text, at, 0.0, LabelKind::Local);
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
    if inputs.sch_highlight != s.selection {
        inputs.sch_highlight = s.selection.clone();
    }
}

// ---------------------------------------------------------------------------------------------

/// The symbol chooser (A, P): a filter field, the matching symbols, OK.
pub mod chooser {
    use super::*;

    #[derive(Component)]
    pub struct Chooser;

    #[derive(Component)]
    pub struct ChooserList;

    #[derive(Component, Clone)]
    pub struct ChooserRow(pub String);

    #[derive(Resource, Default)]
    pub struct ChooserState {
        pub filter: Option<String>,
        pub chosen: Option<String>,
    }

    pub fn open(w: &mut World, power: bool) {
        w.resource_mut::<SchState>().chooser_power = power;
        w.init_resource::<ChooserState>();
        *w.resource_mut::<ChooserState>() = ChooserState::default();
        let theme = w.resource::<Theme>().clone();
        let (tb, tf) = (theme.clone(), theme.clone());
        w.spawn((
            Dialog::new("eda-chooser")
                .title(if power { "Choose a power symbol" } else { "Choose a symbol" })
                .width(460.0)
                .body(move |b| {
                    b.spawn(TextInput::new("eda-chooser-filter").placeholder("Filter").width(Val::Percent(100.0)).height(28.0).build(&tb));
                    b.spawn((
                        Name::new("eda-chooser-list"),
                        ChooserList,
                        Node { flex_direction: FlexDirection::Column, height: Val::Px(300.0), overflow: Overflow::scroll_y(), margin: UiRect::top(Val::Px(6.0)), ..default() },
                    ));
                })
                .footer(move |f| {
                    f.spawn((
                        cadrs_ui::Button::new("eda-chooser-ok").label("OK").primary().build(&tf),
                        observe(|_: On<Activate>, mut commands: Commands| {
                            commands.queue(accept);
                        }),
                    ));
                    f.spawn((
                        cadrs_ui::Button::new("eda-chooser-cancel").label("Cancel").build(&tf),
                        observe(|_: On<Activate>, mut commands: Commands| {
                            commands.queue(close);
                        }),
                    ));
                })
                .build(&theme),
            Chooser,
            DespawnOnExit(AppState::Document),
        ));
    }

    pub fn close(w: &mut World) {
        let mut q = w.query_filtered::<Entity, With<Chooser>>();
        let es: Vec<Entity> = q.iter(w).collect();
        for e in es {
            w.entity_mut(e).despawn();
        }
    }

    fn accept(w: &mut World) {
        let chosen = w.get_resource::<ChooserState>().and_then(|s| s.chosen.clone());
        let lib = ui::libraries(w);
        let filter = ui::text_value(w, "eda-chooser-filter");
        let power = w.resource::<SchState>().chooser_power;
        let sym = chosen.and_then(|id| lib.symbol(&id).cloned()).or_else(|| lib.search_symbols(&filter, power).first().map(|s| (*s).clone()));
        close(w);
        if let Some(sym) = sym {
            set_tool(w, Tool::Place(Box::new(sym)));
        }
    }

    /// Rebuilds the list when the filter changes.
    pub fn refresh(world: &mut World) {
        let mut q = world.query_filtered::<Entity, With<ChooserList>>();
        let Some(list) = q.iter(world).next() else { return };
        let filter = ui::text_value(world, "eda-chooser-filter");
        let state = world.resource::<ChooserState>();
        if state.filter.as_deref() == Some(filter.as_str()) {
            return;
        }
        let chosen = state.chosen.clone();
        let power = world.resource::<SchState>().chooser_power;
        let lib = ui::libraries(world);
        let hits: Vec<(String, String)> = lib
            .search_symbols(&filter, power)
            .into_iter()
            .take(60)
            .map(|s| (s.id.clone(), s.field(cadrs_eda::symbol::fields::DESCRIPTION).map_or(String::new(), |f| f.value().to_string())))
            .collect();
        world.resource_mut::<ChooserState>().filter = Some(filter);
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        commands.entity(list).despawn_children();
        commands.entity(list).with_children(|l| {
            for (id, desc) in hits {
                let name = format!("eda-chooser-{}", crate::pcb::slug(&id));
                l.spawn((ListItem::new(name).label(id.clone()).detail(desc).height(24.0).selected(chosen.as_deref() == Some(id.as_str())).build(&theme), ChooserRow(id), cadrs_ui::DoubleClickable));
            }
        });
        world.flush();
    }

    pub fn on_row(a: On<Activate>, q: Query<&ChooserRow>, mut commands: Commands) {
        if let Ok(r) = q.get(a.entity) {
            let id = r.0.clone();
            commands.queue(move |w: &mut World| {
                w.resource_mut::<ChooserState>().chosen = Some(id);
                // Redraw the selection.
                w.resource_mut::<ChooserState>().filter = None;
            });
        }
    }

    /// Enter in the filter accepts the first match.
    pub fn on_submit(ev: On<TextSubmit>, q: Query<&Name>, mut commands: Commands) {
        if q.get(ev.entity).is_ok_and(|n| n.as_str() == "eda-chooser-filter-field") {
            commands.queue(accept);
        }
    }
}

pub mod dialogs {
    //! The Schematic's dialogs: label name, symbol fields (E), page settings, footprint
    //! assignment, ERC, BOM.
    pub use super::super::sch_dialogs::*;
}

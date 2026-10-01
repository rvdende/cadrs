//! BOM tables and callouts (P3C.5, D11, D12, X10).
//!
//! - **Insert BOM** (toolbar, and the view menu's Insert BOM): the "Insert BOM" card
//!   (`ex2-step8.png`) with the assembly (the one the sheet or its views show, by default), the
//!   **BOM type** (Flattened, Structured - Top level, Structured - Multi level), the **Order**
//!   (Top to bottom, Bottom to top) and the fixed corner; the table follows the cursor (orange)
//!   and snaps to the frame's matching corner when it comes near it; a click places it. Its rows
//!   are the assembly's BOM with that assembly's own columns (D11.1,
//!   `cadrs_core::drawing_assembly::bom_data`). It resizes and formats like any table (D11.3);
//!   right-click → **BOM Table properties…** changes its type, order and fixed corner.
//! - **Callout** (toolbar, D11.4): the "Callout" card (`ex2-step10.png`) with the component
//!   property menu (`Part: Name`, …) and the BOM table property menu (`Table: Item No.`,
//!   `Table: Qty.`) inserting into the field last typed in, five text fields (upper; left,
//!   centre, right; lower), the text height, the border and its size. A click on a part's edge in
//!   an assembly view attaches the leader; the callout then follows the cursor and a click places
//!   it; more callouts follow until ✓ or Esc. Right-click → **Edit…** (D11.5) opens the card on
//!   a callout, changed live, ✓ applies.
//! - **Inference lines** (D11.6): a callout being placed or dragged snaps to the vertical and
//!   horizontal through the other callouts' anchors on the sheet (within 2.5 mm), with dashed
//!   guides.

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::ElementId;
use cadrs_core::drawing_assembly as da;
use cadrs_drawing::DrawingOp;
use cadrs_drawing::annotation::{Annotation, AnnotationId, AnnotationKind};
use cadrs_drawing::assembly::{BomData, BomOrder, BomType, Border, Callout, CalloutFields, SIZES, bom_table, inference, refreshed_bom_table, snap_table_corner, token};
use cadrs_drawing::table::{Corner, Table, TableId};
use cadrs_drawing::{SheetId, ViewId};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;
use cadrs_ui::{Select, SelectChange, SelectState};

use super::annotations::{AnnTool, AnnotationUi, Hover};
use super::note_bar::{card_bundle, corner_buttons, header, refresh_corner_buttons};
use super::notes::{self, Item, NotesUi, find_table};
use super::view_tools::edit_drawing;
use super::views::ViewCache;
use super::{DrawingUi, active_drawing, sheet_area};
use crate::viewport::ViewportRect;
use crate::{ActiveDocument, AppState};

pub struct BomToolsPlugin;

impl Plugin for BomToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BomUi>()
            .init_resource::<CalloutUi>()
            .add_systems(
                Update,
                (read_bom_card, read_callout_card, track_callout_focus, callout_preview)
                    .chain()
                    .after(super::annotations::AnnotationInputSet)
                    .before(super::annotations::AnnotationDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut b: ResMut<BomUi>, mut c: ResMut<CalloutUi>| {
                *b = BomUi::default();
                *c = CalloutUi::default();
            })
            .add_observer(on_select)
            .add_observer(on_property_menu);
    }
}

// ---------------------------------------------------------------------------------------------
// Insert BOM

/// The Insert BOM card's settings and the rows they give.
#[derive(Resource, Debug, Clone, Default)]
pub struct BomUi {
    pub assemblies: Vec<(ElementId, String)>,
    pub assembly: Option<ElementId>,
    pub kind: BomType,
    pub order: BomOrder,
    pub fixed: Corner,
    /// The rows of the chosen assembly as the workspace has them (what a click places).
    pub data: Option<BomData>,
    /// BOM Table properties… is open on this table.
    pub editing: Option<(SheetId, TableId)>,
}

#[derive(Component)]
struct BomCard;

/// How near (sheet mm) the fixed corner must come to the frame's corner to snap to it.
const SNAP: f64 = 8.0;

/// The assemblies of the document, and the one the active sheet shows (its reference or a
/// view's).
fn assemblies(world: &World) -> (Vec<(ElementId, String)>, Option<ElementId>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return (Vec::new(), None);
    };
    let list: Vec<(ElementId, String)> =
        doc.doc.elements.iter().filter(|e| e.assembly_model().is_some()).map(|e| (e.id, e.name.clone())).collect();
    let shown = active_drawing(doc).and_then(|(id, d)| {
        let s = d.sheets.get(world.resource::<DrawingUi>().sheet_index(id, d))?;
        s.reference
            .into_iter()
            .chain(s.views.iter().map(|v| v.reference))
            .map(|r| ElementId(r.element))
            .find(|e| list.iter().any(|(a, _)| a == e))
    });
    (list.clone(), shown.or_else(|| list.first().map(|(e, _)| *e)))
}

/// Recomputes the rows the card's settings give.
pub fn refresh(world: &mut World) {
    refresh_data(world);
}

fn refresh_data(world: &mut World) {
    let (asm, kind, order) = {
        let b = world.resource::<BomUi>();
        (b.assembly, b.kind, b.order)
    };
    let data = asm.and_then(|a| {
        let doc = world.get_resource::<ActiveDocument>()?;
        da::live_bom(&doc.doc, a, kind, order).map_err(|e| warn!("Insert BOM: {e}")).ok()
    });
    world.resource_mut::<BomUi>().data = data;
}

/// The toolbar's Insert BOM (and the view menu's): the card, then the table follows the cursor.
pub fn open_bom_tool(world: &mut World) {
    close_bom_card(world);
    notes::commit_edit(world);
    world.resource_mut::<NotesUi>().clear_selection();
    super::annotations::start_tool(world, AnnTool::PlaceBom);
    if world.resource::<AnnotationUi>().tool != AnnTool::PlaceBom {
        return;
    }
    let (list, shown) = assemblies(world);
    {
        let mut b = world.resource_mut::<BomUi>();
        b.assemblies = list;
        b.assembly = shown;
        b.editing = None;
    }
    refresh_data(world);
    spawn_bom_card(world, "Insert BOM", "bom-dialog", None);
}

fn spawn_bom_card(world: &mut World, title: &str, prefix: &'static str, accept: Option<fn(&mut World)>) {
    let b = world.resource::<BomUi>().clone();
    let area = {
        let ui = world.resource::<DrawingUi>();
        sheet_area(world.resource::<ViewportRect>(), ui)
    };
    let t = world.resource::<Theme>().clone();
    let at = Vec2::new(area.max.x - 262.0, area.min.y + (area.height() * 0.3).min(240.0));
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&t, prefix, at), BomCard))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(236.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        })
        .with_children(|c| {
            header(c, &t, title, prefix, accept, cancel_bom);
            c.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), row_gap: Val::Px(6.0), ..default() })
                .with_children(|body| {
                    // The assembly (an assembly icon and its name).
                    body.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                        r.spawn((icon("assembly", 16.0, t.tool_foreground), Pickable::IGNORE));
                        let mut s = Select::new("bom-assembly").width(Val::Px(190.0));
                        for (_, n) in &b.assemblies {
                            s = s.option(n.clone(), b.editing.is_none());
                        }
                        let i = b.assemblies.iter().position(|(e, _)| Some(*e) == b.assembly).unwrap_or(0);
                        r.spawn(s.selected(i).build(&t));
                    });
                    let row = |p: &mut ChildSpawnerCommands, label: &str, child: Select| {
                        p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(8.0), ..default() }).with_children(|r| {
                            r.spawn((t.text(label, t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { width: Val::Px(62.0), ..default() }));
                            r.spawn(child.build(&t));
                        });
                    };
                    let mut s = Select::new("bom-type").width(Val::Px(150.0));
                    for k in BomType::ALL {
                        s = s.option(k.label(), true);
                    }
                    row(body, "BOM type", s.selected(BomType::ALL.iter().position(|k| *k == b.kind).unwrap_or(0)));
                    let mut s = Select::new("bom-order").width(Val::Px(150.0));
                    for o in BomOrder::ALL {
                        s = s.option(o.label(), true);
                    }
                    row(body, "Order", s.selected(BomOrder::ALL.iter().position(|o| *o == b.order).unwrap_or(0)));
                    corner_buttons(body, &t, &format!("{prefix}-corner"), b.fixed, |w, c| {
                        w.resource_mut::<BomUi>().fixed = c;
                        let prefix = if w.resource::<BomUi>().editing.is_some() { "bom-properties-corner" } else { "bom-dialog-corner" };
                        refresh_corner_buttons(w, prefix, c);
                    });
                    if b.editing.is_none() {
                        body.spawn((
                            t.text("Click the sheet to place the table; it snaps to the border's corner or the title block.", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                            Node { max_width: Val::Px(218.0), ..default() },
                        ))
                        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
                    }
                });
        });
    world.flush();
}

pub fn close_bom_card(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<BomCard>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn cancel_bom(world: &mut World) {
    close_bom_card(world);
    world.resource_mut::<BomUi>().editing = None;
    if world.resource::<AnnotationUi>().tool == AnnTool::PlaceBom {
        world.resource_mut::<AnnotationUi>().tool = AnnTool::None;
    }
}

/// The card closes when its tool ends (Esc, another tool).
fn read_bom_card(ann: Res<AnnotationUi>, bom: Res<BomUi>, q: Query<Entity, With<BomCard>>, mut commands: Commands) {
    if ann.tool != AnnTool::PlaceBom && bom.editing.is_none() {
        for e in &q {
            commands.entity(e).despawn();
        }
    }
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else { return };
    let i = ev.index;
    match name.as_str() {
        "bom-assembly" | "bom-type" | "bom-order" => {
            let which = name.as_str().to_string();
            commands.queue(move |w: &mut World| {
                {
                    let mut b = w.resource_mut::<BomUi>();
                    match which.as_str() {
                        "bom-assembly" => b.assembly = b.assemblies.get(i).map(|(e, _)| *e).or(b.assembly),
                        "bom-type" => b.kind = BomType::ALL.get(i).copied().unwrap_or(b.kind),
                        _ => b.order = BomOrder::ALL.get(i).copied().unwrap_or(b.order),
                    }
                }
                refresh_data(w);
            });
        }
        "callout-border" | "callout-size" => {
            let which = name.as_str().to_string();
            commands.queue(move |w: &mut World| {
                let mut c = w.resource_mut::<CalloutUi>();
                if which == "callout-border" {
                    c.spec.border = Border::ALL.get(i).copied().unwrap_or(c.spec.border);
                } else {
                    c.spec.size = SIZES.get(i).map(|s| s.0).unwrap_or(c.spec.size);
                }
            });
        }
        _ => {}
    }
}

/// The BOM table the card's settings place with the cursor at `p` on `sheet` of `d` (snapped
/// to the frame's corner or the title block).
pub fn preview_table(b: &BomUi, d: &cadrs_drawing::Drawing, sheet: &cadrs_drawing::Sheet, p: [f64; 2]) -> Option<Table> {
    let data = b.data.clone()?;
    let f = cadrs_drawing::standard::frame(sheet.format).inner;
    // The frame's corner, or the title block (TD10.5: its left edge, P3E.5).
    let block = sheet.title_block.then(|| cadrs_drawing::title_block::placement(f, sheet.format.size)).map(|r| (r.min, r.max));
    let at = snap_table_corner(p, b.fixed, (f.min, f.max), block, SNAP).unwrap_or(p);
    Some(bom_table(data, b.fixed, at, &d.style))
}

/// The BOM table that would be placed with the cursor at `p` (snapped to the frame's corner or
/// the title block).
pub fn bom_preview(world: &World, p: [f64; 2]) -> Option<Table> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let (id, d) = active_drawing(doc)?;
    let sheet = d.sheets.get(world.resource::<DrawingUi>().sheet_index(id, d))?;
    preview_table(world.resource::<BomUi>(), d, sheet, p)
}

/// A click with the BOM table following the cursor: it is placed (one undoable step).
pub fn place_bom(w: &mut World, p: [f64; 2]) {
    let Some(t) = bom_preview(w, p) else { return };
    let Some(sheet) = w.get_resource::<ActiveDocument>().and_then(|doc| {
        let (id, d) = active_drawing(doc)?;
        d.sheets.get(w.resource::<DrawingUi>().sheet_index(id, d)).map(|s| s.id)
    }) else {
        return;
    };
    let id = t.id;
    w.resource_mut::<AnnotationUi>().tool = AnnTool::None;
    close_bom_card(w);
    let t = notes::fit(w, t);
    if edit_drawing(w, DrawingOp::AddTable { sheet, table: t }) {
        w.resource_mut::<NotesUi>().selected = vec![Item::Table(id)];
    }
}

/// Right-click → BOM Table properties… (D11.3): the type, order and fixed corner of a placed
/// BOM table; ✓ applies them (the rows are the workspace's for the new type).
pub fn open_bom_properties(world: &mut World, id: TableId) {
    close_bom_card(world);
    let found = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let dui = world.resource::<DrawingUi>();
        let (eid, d) = active_drawing(doc)?;
        let i = dui.sheet_index(eid, d);
        Some((d.sheets[i].id, find_table(d, i, id)?.clone()))
    })();
    let Some((sheet, t)) = found else { return };
    let Some(bom) = t.bom.clone() else { return };
    let (list, _) = assemblies(world);
    {
        let mut b = world.resource_mut::<BomUi>();
        b.assemblies = list;
        b.assembly = Some(ElementId(bom.assembly));
        b.kind = bom.kind;
        b.order = bom.order;
        b.fixed = t.fixed;
        b.editing = Some((sheet, id));
    }
    refresh_data(world);
    spawn_bom_card(world, "BOM Table properties", "bom-properties", Some(apply_bom_properties));
}

/// Table `t` as BOM Table properties… would leave it (a live preview while the card is open
/// on it): the card's type and order (the workspace's rows) and fixed corner; `None` when the
/// card isn't open on `t`.
pub fn properties_preview(b: &BomUi, t: &Table, style: &cadrs_drawing::DrawingStyle) -> Option<Table> {
    let (_, id) = b.editing?;
    if id != t.id {
        return None;
    }
    let old = t.bom.as_ref()?;
    let mut table = t.clone();
    if (old.kind != b.kind || old.order != b.order)
        && let Some(data) = b.data.clone().filter(|d| d.kind == b.kind && d.order == b.order && d.assembly == old.assembly)
    {
        table = refreshed_bom_table(&table, data, style);
    }
    if table.fixed != b.fixed {
        table = table.set_fixed(b.fixed);
    }
    Some(table)
}

fn apply_bom_properties(world: &mut World) {
    let b = world.resource::<BomUi>().clone();
    close_bom_card(world);
    world.resource_mut::<BomUi>().editing = None;
    let Some((sheet, id)) = b.editing else { return };
    let Some((t, style)) = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let (_, d) = active_drawing(doc)?;
        let t = d.sheet(sheet)?.tables.iter().find(|t| t.id == id)?.clone();
        Some((t, d.style.clone()))
    })() else {
        return;
    };
    let Some(old) = t.bom.clone() else { return };
    let mut table = t.clone();
    if old.kind != b.kind || old.order != b.order {
        let data = world
            .get_resource::<ActiveDocument>()
            .and_then(|doc| da::live_bom(&doc.doc, ElementId(old.assembly), b.kind, b.order).ok());
        if let Some(data) = data {
            table = refreshed_bom_table(&table, data, &style);
        }
    }
    if table.fixed != b.fixed {
        table = table.set_fixed(b.fixed);
    }
    if table != t {
        let table = notes::fit(world, table);
        edit_drawing(world, DrawingOp::SetTable { sheet, table, label: "BOM Table properties".into() });
    }
    world.resource_mut::<NotesUi>().selected = vec![Item::Table(id)];
}

// ---------------------------------------------------------------------------------------------
// Callouts

/// What the Callout card makes.
#[derive(Debug, Clone, PartialEq)]
pub struct CalloutSpec {
    pub border: Border,
    pub size: u8,
    /// Cap height (sheet mm).
    pub text_height: f64,
    pub fields: CalloutFields,
}

impl Default for CalloutSpec {
    fn default() -> Self {
        Self { border: Border::Circle, size: 0, text_height: 3.048, fields: CalloutFields { center: token(false, "Item No."), ..Default::default() } }
    }
}

/// The Callout tool's state.
#[derive(Resource, Debug, Clone, Default)]
pub struct CalloutUi {
    pub spec: CalloutSpec,
    /// The leader's end picked: the view, the occurrence and the point (view 2D).
    pub attach: Option<(ViewId, uuid::Uuid, [f64; 2])>,
    /// Edit… of a callout (D11.5).
    pub editing: Option<(ViewId, AnnotationId)>,
    /// The field last typed in (the property menus insert there).
    pub field: Option<String>,
    /// The card was built from this spec (read back each frame).
    open: bool,
    /// The drawing is in inches (the text height field's unit).
    pub inch: bool,
}

#[derive(Component)]
struct CalloutCard;

/// The five fields: (name, label as a placeholder).
const FIELDS: [(&str, &str); 5] = [("callout-upper", "Upper"), ("callout-lower", "Lower"), ("callout-left", "Left"), ("callout-right", "Right"), ("callout-center", "Center")];

/// The Callout tool (toolbar): the card, then pick a part and place.
pub fn start_callout_tool(world: &mut World) {
    close_callout_card(world);
    super::annotations::start_tool(world, AnnTool::Callout);
    if world.resource::<AnnotationUi>().tool != AnnTool::Callout {
        return;
    }
    let (h, inch) = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc).map(|(_, d)| (d.style.dim_text_height, d.units == cadrs_drawing::DrawingUnits::Inch)))
        .unwrap_or((3.048, true));
    world.resource_mut::<CalloutUi>().inch = inch;
    {
        let mut c = world.resource_mut::<CalloutUi>();
        c.attach = None;
        c.editing = None;
        c.spec.text_height = h;
    }
    spawn_callout_card(world);
}

/// Right-click → Edit… on a callout (D11.5).
pub fn edit_callout(world: &mut World, view: ViewId, id: AnnotationId) {
    let found = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc))
        .and_then(|(_, d)| super::annotations::find_annotation(d, view, id))
        .and_then(|(_, a)| match a.kind {
            AnnotationKind::Callout(c) => Some(c),
            _ => None,
        });
    let Some(c) = found else { return };
    close_callout_card(world);
    let inch = world
        .get_resource::<ActiveDocument>()
        .and_then(|doc| active_drawing(doc).map(|(_, d)| d.units == cadrs_drawing::DrawingUnits::Inch))
        .unwrap_or(true);
    {
        let mut ui = world.resource_mut::<CalloutUi>();
        ui.inch = inch;
        ui.spec = CalloutSpec { border: c.border, size: c.size, text_height: c.text_height, fields: c.fields.clone() };
        ui.editing = Some((view, id));
        ui.attach = None;
    }
    let mut ann = world.resource_mut::<AnnotationUi>();
    ann.selected = vec![(view, id)];
    spawn_callout_card(world);
}

fn spawn_callout_card(world: &mut World) {
    let spec = world.resource::<CalloutUi>().spec.clone();
    let unit = if world.resource::<CalloutUi>().inch { 25.4 } else { 1.0 };
    let area = {
        let ui = world.resource::<DrawingUi>();
        sheet_area(world.resource::<ViewportRect>(), ui)
    };
    let t = world.resource::<Theme>().clone();
    // Top left, as `ex2-step10.png`.
    let at = Vec2::new(area.min.x + 8.0, area.min.y + 8.0);
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&t, "callout-dialog", at), CalloutCard))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(326.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        })
        .with_children(|c| {
            header(c, &t, "Callout", "callout-dialog", Some(accept_callout), cancel_callout);
            c.spawn(Node { flex_direction: FlexDirection::Column, padding: UiRect::all(Val::Px(8.0)), row_gap: Val::Px(6.0), ..default() })
                .with_children(|b| {
                    // The property menus: a part's, the BOM table's.
                    b.spawn(Node { column_gap: Val::Px(4.0), ..default() }).with_children(|r| {
                        for (name, ic, tip) in [("callout-part-property", "part", "Insert component property"), ("callout-table-property", "bill-of-materials", "Insert BOM table property")] {
                            let n: &'static str = name;
                            r.spawn((
                                ToolButton::new(name, ic).dropdown(true).icon_size(16.0).tooltip(tip).build(&t),
                                observe(move |a: On<Activate>, mut commands: Commands| {
                                    let button = a.entity;
                                    commands.queue(move |w: &mut World| open_property_menu(w, button, n));
                                }),
                            ));
                        }
                    });
                    // The five fields: upper; left, centre, right; lower.
                    let field = |p: &mut ChildSpawnerCommands, name: &str, label: &str, value: &str, w: f32| {
                        p.spawn(TextInput::new(name.to_string()).placeholder(label).value(value.to_string()).width(Val::Px(w)).chips().build(&t));
                    };
                    let f = &spec.fields;
                    b.spawn(Node { justify_content: JustifyContent::Center, ..default() }).with_children(|r| field(r, FIELDS[0].0, FIELDS[0].1, &f.upper, 120.0));
                    b.spawn(Node { column_gap: Val::Px(4.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                        field(r, FIELDS[2].0, FIELDS[2].1, &f.left, 72.0);
                        field(r, FIELDS[4].0, FIELDS[4].1, &f.center, 120.0);
                        field(r, FIELDS[3].0, FIELDS[3].1, &f.right, 110.0);
                    });
                    b.spawn(Node { justify_content: JustifyContent::Center, ..default() }).with_children(|r| field(r, FIELDS[1].0, FIELDS[1].1, &f.lower, 120.0));
                    // Text height, border, size.
                    b.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, margin: UiRect::top(Val::Px(4.0)), ..default() }).with_children(|r| {
                        r.spawn(TextInput::new("callout-text-height").value(if unit > 1.0 { format!("{:.4}", spec.text_height / unit) } else { format!("{:.2}", spec.text_height) }).width(Val::Px(62.0)).select_all_on_focus().build(&t));
                        let mut s = Select::new("callout-border").width(Val::Px(100.0));
                        for bd in Border::ALL {
                            s = s.option(bd.label(), true);
                        }
                        r.spawn(s.selected(Border::ALL.iter().position(|x| *x == spec.border).unwrap_or(0)).build(&t));
                        let mut s = Select::new("callout-size").width(Val::Px(100.0));
                        for (_, l) in SIZES {
                            s = s.option(l, true);
                        }
                        r.spawn(s.selected(SIZES.iter().position(|x| x.0 == spec.size).unwrap_or(0)).build(&t));
                    });
                });
            c.spawn((
                t.text("Pick an edge of a part, then click to place", t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                Node { margin: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(0.0), Val::Px(8.0)), ..default() },
            ));
        });
    world.resource_mut::<CalloutUi>().open = true;
    world.flush();
}

pub fn close_callout_card(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<CalloutCard>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
    world.resource_mut::<CalloutUi>().open = false;
}

fn cancel_callout(world: &mut World) {
    close_callout_card(world);
    {
        let mut c = world.resource_mut::<CalloutUi>();
        c.attach = None;
        c.editing = None;
    }
    let mut ann = world.resource_mut::<AnnotationUi>();
    ann.callout_preview = None;
    ann.guides.clear();
    if ann.tool == AnnTool::Callout {
        ann.tool = AnnTool::None;
        ann.preview = None;
    }
}

/// ✓: an edited callout takes the card's settings; the tool ends.
fn accept_callout(world: &mut World) {
    let editing = world.resource::<CalloutUi>().editing;
    if let Some((view, id)) = editing {
        let spec = world.resource::<CalloutUi>().spec.clone();
        let found = world
            .get_resource::<ActiveDocument>()
            .and_then(|doc| active_drawing(doc))
            .and_then(|(_, d)| super::annotations::find_annotation(d, view, id));
        if let Some((_, mut a)) = found
            && let AnnotationKind::Callout(c) = &mut a.kind
        {
            c.border = spec.border;
            c.size = spec.size;
            c.text_height = spec.text_height;
            c.fields = spec.fields.clone();
            edit_drawing(world, DrawingOp::SetAnnotation { view, annotation: a, label: "Edit callout".into() });
        }
    }
    cancel_callout(world);
}

/// Reads the card into the spec (and an edited callout's live preview); closes it when the
/// tool ends.
#[allow(clippy::type_complexity)]
fn read_callout_card(
    ann: Res<AnnotationUi>,
    q_card: Query<Entity, With<CalloutCard>>,
    q_text: Query<(&Name, &EditableText)>,
    q_select: Query<(&Name, &SelectState)>,
    mut ui: ResMut<CalloutUi>,
    mut commands: Commands,
) {
    if q_card.is_empty() {
        return;
    }
    if ann.tool != AnnTool::Callout && ui.editing.is_none() {
        for e in &q_card {
            commands.entity(e).despawn();
        }
        ui.open = false;
        return;
    }
    let mut s = ui.spec.clone();
    for (n, t) in &q_text {
        let v = t.value().to_string();
        match n.as_str() {
            "callout-upper-field" => s.fields.upper = v,
            "callout-lower-field" => s.fields.lower = v,
            "callout-left-field" => s.fields.left = v,
            "callout-right-field" => s.fields.right = v,
            "callout-center-field" => s.fields.center = v,
            "callout-text-height-field" => {
                if let Ok(x) = v.trim().parse::<f64>()
                    && x > 0.0
                {
                    s.text_height = if ui.inch { x * 25.4 } else { x };
                }
            }
            _ => {}
        }
    }
    for (n, st) in &q_select {
        match n.as_str() {
            "callout-border" => s.border = Border::ALL.get(st.selected).copied().unwrap_or(s.border),
            "callout-size" => s.size = SIZES.get(st.selected).map(|x| x.0).unwrap_or(s.size),
            _ => {}
        }
    }
    if ui.spec != s {
        ui.spec = s;
    }
}

/// The field last focused (where the property menus insert).
fn track_callout_focus(focus: Res<bevy::input_focus::InputFocus>, q: Query<&Name, With<TextInputField>>, mut ui: ResMut<CalloutUi>) {
    if let Some(e) = focus.get()
        && let Ok(n) = q.get(e)
        && FIELDS.iter().any(|(f, _)| n.as_str() == format!("{f}-field"))
        && ui.field.as_deref() != Some(n.as_str())
    {
        ui.field = Some(n.as_str().to_string());
    }
}

#[derive(Component, Clone, Copy)]
struct PropertyMenuFor;

fn open_property_menu(world: &mut World, button: Entity, which: &str) {
    let Some((left, bottom)) = world.get::<ComputedNode>(button).zip(world.get::<bevy::ui::UiGlobalTransform>(button)).map(|(n, t)| {
        let s = n.inverse_scale_factor();
        ((t.translation.x - n.size().x / 2.0) * s, (t.translation.y + n.size().y / 2.0) * s)
    }) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut menu = Menu::new(format!("{which}-menu")).min_width(170.0);
    if which == "callout-part-property" {
        for p in cadrs_drawing::assembly::PART_PROPERTIES {
            menu = menu.item(MenuItem::new(format!("callout-part-{}", slug(p)), format!("Part: {p}")));
        }
    } else {
        for p in cadrs_drawing::assembly::TABLE_PROPERTIES {
            menu = menu.item(MenuItem::new(format!("callout-table-{}", slug(p)), format!("Table: {p}")));
        }
    }
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, Vec2::new(left, bottom + 2.0), menu.build(&theme));
    commands.entity(anchor).insert((PropertyMenuFor, DespawnOnExit(AppState::Document)));
    world.flush();
}

/// "Part number" → "part-number", "Item No." → "item-no".
fn slug(s: &str) -> String {
    s.to_lowercase().replace('.', "").split_whitespace().collect::<Vec<_>>().join("-")
}

fn on_property_menu(ev: On<MenuAction>, q: Query<(), With<PropertyMenuFor>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let item = ev.item.clone();
    let tok = cadrs_drawing::assembly::PART_PROPERTIES
        .iter()
        .find(|p| item == format!("callout-part-{}", slug(p)))
        .map(|p| token(true, p))
        .or_else(|| cadrs_drawing::assembly::TABLE_PROPERTIES.iter().find(|p| item == format!("callout-table-{}", slug(p))).map(|p| token(false, p)));
    let Some(tok) = tok else { return };
    commands.queue(move |w: &mut World| {
        let field = w.resource::<CalloutUi>().field.clone().unwrap_or_else(|| "callout-center-field".into());
        let mut q = w.query_filtered::<(&Name, &mut EditableText), With<TextInputField>>();
        if let Some((_, mut t)) = q.iter_mut(w).find(|(n, _)| n.as_str() == field) {
            // At the caret (after what was typed), else at the end.
            let old = t.value().to_string();
            if old.is_empty() || old.ends_with(' ') {
                t.queue_edit(bevy::text::TextEdit::SelectAll);
                t.queue_edit(bevy::text::TextEdit::Insert(format!("{old}{tok}").into()));
            } else {
                t.queue_edit(bevy::text::TextEdit::SelectAll);
                t.queue_edit(bevy::text::TextEdit::Insert(format!("{old} {tok}").into()));
            }
            // The menu took the focus: the field shows its start again (not a scrolled tail).
            t.queue_edit(bevy::text::TextEdit::TextStart(false));
        }
    });
}

/// The other callouts' anchors on the active sheet (sheet mm), less `skip`.
fn callout_anchors(d: &cadrs_drawing::Drawing, sheet: usize, skip: Option<(ViewId, AnnotationId)>) -> Vec<[f64; 2]> {
    let Some(s) = d.sheets.get(sheet) else { return Vec::new() };
    s.views
        .iter()
        .flat_map(|v| {
            v.annotations.iter().filter_map(move |a| match &a.kind {
                AnnotationKind::Callout(c) if Some((v.id, a.id)) != skip => Some(v.to_sheet(c.text)),
                _ => None,
            })
        })
        .collect()
}

/// How near (sheet mm) a callout must come to another's line to snap to it (D11.6).
const INFER: f64 = 2.5;

/// Inference guides: (from, to) on the sheet (mm).
pub type Guides = Vec<([f64; 2], [f64; 2])>;

/// A callout of view `v` with its anchor at the sheet point `p`, snapped to the other callouts
/// of the sheet: the callout's view-2D anchor and the guides (sheet mm).
pub fn snapped_anchor(d: &cadrs_drawing::Drawing, sheet: usize, v: &cadrs_drawing::View, p: [f64; 2], skip: Option<(ViewId, AnnotationId)>) -> ([f64; 2], Guides) {
    let others = callout_anchors(d, sheet, skip);
    let (q, lines) = inference(p, &others, INFER);
    (v.from_sheet(q), lines)
}

/// A click with the Callout tool: the first on a part's edge attaches the leader, the next
/// places the callout (the tool stays on for the next one).
pub fn callout_click(w: &mut World, hover: Option<Hover>, p: Vec2) {
    let attach = w.resource::<CalloutUi>().attach;
    match attach {
        None => {
            let _ = hover;
            let Some((vid, occ, at)) = part_under(w, p) else { return };
            w.resource_mut::<CalloutUi>().attach = Some((vid, occ, at));
            w.resource_mut::<AnnotationUi>().pick_view = Some(vid);
        }
        Some(_) => {
            let preview = w.resource::<AnnotationUi>().preview.clone();
            if let Some((vid, a)) = preview
                && matches!(a.kind, AnnotationKind::Callout(_))
                && edit_drawing(w, DrawingOp::AddAnnotation { view: vid, annotation: a })
            {
                w.resource_mut::<CalloutUi>().attach = None;
                let mut ann = w.resource_mut::<AnnotationUi>();
                ann.preview = None;
                ann.pick_view = None;
                ann.guides.clear();
            }
        }
    }
}

/// The part under the sheet point `p` in an assembly view of the active sheet: the view, the
/// occurrence and the nearest point of its nearest visible edge (view 2D), within 2 mm.
fn part_under(w: &World, p: Vec2) -> Option<(ViewId, uuid::Uuid, [f64; 2])> {
    let doc = w.get_resource::<ActiveDocument>()?;
    let (id, d) = active_drawing(doc)?;
    let sheet = d.sheets.get(w.resource::<DrawingUi>().sheet_index(id, d))?;
    let cache = w.resource::<ViewCache>();
    let q = [p.x as f64, p.y as f64];
    let mut best: Option<(f64, ViewId, uuid::Uuid, [f64; 2])> = None;
    for v in sheet.views.iter().filter(|v| is_assembly_view(doc, v)) {
        let Some(g) = cache.geometry(v) else { continue };
        let local = v.from_sheet(q);
        let k = v.scale.factor();
        for (i, e) in g.projection.edges.iter().enumerate() {
            if e.visibility != cadrs_kernel::ProjVisibility::Visible {
                continue;
            }
            let Some(occ) = da::edge_occurrence(&g, i) else { continue };
            for s in e.points.windows(2) {
                let (a, b) = ([s[0].x, s[0].y], [s[1].x, s[1].y]);
                let dd = [b[0] - a[0], b[1] - a[1]];
                let l2 = dd[0] * dd[0] + dd[1] * dd[1];
                let t = if l2 > 1e-18 { (((local[0] - a[0]) * dd[0] + (local[1] - a[1]) * dd[1]) / l2).clamp(0.0, 1.0) } else { 0.0 };
                let n = [a[0] + dd[0] * t, a[1] + dd[1] * t];
                let dist = (n[0] - local[0]).hypot(n[1] - local[1]) * k;
                if dist <= 2.0 && best.is_none_or(|(bd, ..)| dist < bd) {
                    best = Some((dist, v.id, occ.0, n));
                }
            }
        }
    }
    best.map(|(_, v, o, n)| (v, o, n))
}

/// The callout being placed follows the cursor (snapped to the others, D11.6); an edited one
/// shows its card's settings live.
fn callout_preview(doc: Option<Res<ActiveDocument>>, dui: Res<DrawingUi>, cui: Res<CalloutUi>, mut ann: ResMut<AnnotationUi>) {
    let Some(doc) = doc else { return };
    let Some((id, d)) = active_drawing(&doc) else { return };
    let index = dui.sheet_index(id, d);
    // Edit…: the callout as the card would leave it.
    if let Some((view, aid)) = cui.editing {
        let want = super::annotations::find_annotation(d, view, aid).and_then(|(_, mut a)| {
            let AnnotationKind::Callout(c) = &mut a.kind else { return None };
            c.border = cui.spec.border;
            c.size = cui.spec.size;
            c.text_height = cui.spec.text_height;
            c.fields = cui.spec.fields.clone();
            Some((view, a))
        });
        if ann.callout_preview != want {
            ann.callout_preview = want;
        }
        return;
    }
    if ann.tool != AnnTool::Callout {
        return;
    }
    let (Some((vid, occ, at)), Some(p)) = (cui.attach, dui.pointer) else {
        if ann.preview.is_some() || !ann.guides.is_empty() {
            ann.preview = None;
            ann.guides.clear();
        }
        return;
    };
    let Some((_, v)) = d.view(vid) else { return };
    let (text, guides) = snapped_anchor(d, index, v, [p.x as f64, p.y as f64], None);
    let keep_id = ann.preview.as_ref().map(|(_, a)| a.id).unwrap_or_default();
    let a = Annotation {
        id: keep_id,
        kind: AnnotationKind::Callout(Callout {
            occurrence: occ,
            attach: at,
            text,
            border: cui.spec.border,
            size: cui.spec.size,
            text_height: cui.spec.text_height,
            fields: cui.spec.fields.clone(),
            last: None,
        }),
    };
    let want = Some((vid, a));
    if ann.preview != want {
        ann.preview = want;
    }
    if ann.guides != guides {
        ann.guides = guides;
    }
}

/// Whether view `v` of the active drawing shows an assembly.
pub fn is_assembly_view(doc: &ActiveDocument, v: &cadrs_drawing::View) -> bool {
    da::is_assembly(&doc.doc, ElementId(v.reference.element))
}

/// Used by the scenarios' and menus' Insert BOM from a view: opens the tool.
pub fn insert_bom_from_menu(world: &mut World) {
    open_bom_tool(world);
}

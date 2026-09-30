//! Notes and tables on the sheet (P3C.4, D9, D10, X3, X9, X12).
//!
//! - **Note (N).** A click on empty sheet places a note without a leader; a click on an edge or
//!   point of a view first picks the leader's end (attached by its persistent name, like the
//!   dimensions), then a click places the text (D9.1). The text is typed **in place** on the
//!   sheet, with a caret and a selection, while the note toolbar floats above it
//!   ([`super::note_bar`]: bold, italic, underline, strikethrough, alignment, lists, text height,
//!   symbols, sheet reference and drawing properties, D9.3, D9.4) and a ruler with the wrap
//!   width's double-arrow handles runs along its top. ✓, Esc or a click elsewhere accepts it
//!   (one undoable edit); ✗ drops the changes. Right-click → Add leader adds more leaders (D9.2).
//! - **Editing notes (D9.5).** A click selects a note (orange, like the other annotations) and
//!   dragging moves it; its grips turn it (the handle above it, in 5° steps), set its wrap width
//!   (side handles) and scale its text (the corner); a leader's end grip re-attaches it. A
//!   double-click edits the text.
//! - **Tables (D10).** The Table dialog sets rows, columns, title and header rows and the fixed
//!   corner; a click on the sheet places the table and typing goes into its first cell, **Tab**
//!   and **Shift+Tab** moving between cells (D10.2). A click on a cell selects it and shows the
//!   cell toolbar (insert and remove rows and columns, merge, unmerge, bold, italic, underline,
//!   alignment); Shift+click selects a range (D10.3). A double-click edits the cell with the
//!   note toolbar. The selected table shows its grips: the midpoint grips resize it from the
//!   fixed corner, drawn black; any corner grip moves it (D10.4). Right-click → Table
//!   properties… changes the fixed corner.
//! - Delete removes the selected notes and tables. Every change goes through the command layer.

use std::sync::Arc;

use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input_focus::InputFocus;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::{PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use cadrs_core::views::ViewGeometry;
use cadrs_drawing::annotation::{Pick, ViewModel};
use cadrs_drawing::note::{Leader, LeaderEnd, Note, NoteGraphics, NoteGrip, NoteId, drag_grip, note_graphics};
use cadrs_drawing::rich::{self, Attr, DrawingContext, Editor, FieldContext, RichText};
use cadrs_drawing::table::{Cell, Table, TableGraphics, TableGrip, TableId, table_graphics};
use cadrs_drawing::{Drawing, DrawingOp, ReferenceProps, SheetId, View, ViewId};
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;

use super::annotations::{AnnTool, AnnotationUi, Hover, blue, hover_in, hover_orange, ink, orange, sheet_views, square};
use super::view_tools::edit_drawing;
use super::views::ViewCache;
use super::{DrawingUi, active_drawing, current_view, screen_to_sheet, sheet_area};
use crate::viewport::{ActiveKind, ViewportArea, ViewportRect};
use crate::{ActiveDocument, AppClock, AppState};

pub struct NotesPlugin;

/// The notes' pointer and key systems (after the annotations', before the sheet sketch's).
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NotesInputSet;

impl Plugin for NotesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NotesUi>()
            .init_resource::<NoteScene>()
            .add_systems(Startup, spawn_editor_focus)
            .add_systems(
                Update,
                (notes_pointer, notes_keys, keep_editor_focus)
                    .chain()
                    .in_set(NotesInputSet)
                    .after(super::annotations::AnnotationInputSet)
                    .before(super::view_tools::ViewToolsSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(
                Update,
                rebuild_note_scene
                    .after(super::views::ViewsSet)
                    .before(super::annotations::AnnotationDrawSet)
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |mut u: ResMut<NotesUi>, mut s: ResMut<NoteScene>| {
                *u = NotesUi::default();
                *s = NoteScene::default();
            })
            .add_observer(on_item_menu);
    }
}

// ---------------------------------------------------------------------------------------------
// State

/// What a press hit: an item and, for a table, its cell.
pub type Hit = (Item, Option<Cell>);

/// A note or table of the active sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    Note(NoteId),
    Table(TableId),
}

/// What is being typed into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    /// A note (`new`: not in the drawing yet).
    Note { id: NoteId, new: bool },
    Cell { table: TableId, cell: Cell },
}

/// Text being typed on the sheet.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEdit {
    pub target: EditTarget,
    pub editor: Editor,
    /// The note's geometry while it is edited (its text is the editor's).
    pub note: Option<Note>,
    /// With the note toolbar (a note, or a double-clicked cell); without, plain cell typing.
    pub toolbar: bool,
}

/// A grip (or an item's body) being dragged.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DragGrip {
    Body,
    Note(NoteGrip),
    Table(TableGrip),
    /// Selecting text in the note being edited.
    Select,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ItemDrag {
    pub item: Item,
    pub grip: DragGrip,
    pub start: Vec2,
    pub at: Vec2,
    pub moving: bool,
}

/// Selected cells: from `anchor` to `head` (Shift+click extends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellSel {
    pub table: TableId,
    pub anchor: Cell,
    pub head: Cell,
}

impl CellSel {
    /// The selected rectangle: (top-left, bottom-right).
    pub fn range(&self) -> (Cell, Cell) {
        (
            (self.anchor.0.min(self.head.0), self.anchor.1.min(self.head.1)),
            (self.anchor.0.max(self.head.0), self.anchor.1.max(self.head.1)),
        )
    }

    pub fn single(&self) -> bool {
        self.anchor == self.head
    }
}

/// What the Table tool places (the Table dialog, D10.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TableSpec {
    pub rows: usize,
    pub cols: usize,
    pub title: bool,
    pub header: bool,
    pub fixed: cadrs_drawing::table::Corner,
    /// The revision table preset (P3C.8): REV | DESCRIPTION | DATE | APPROVED.
    pub revision: bool,
}

impl Default for TableSpec {
    fn default() -> Self {
        Self { rows: 3, cols: 4, title: true, header: true, fixed: cadrs_drawing::table::Corner::TopLeft, revision: false }
    }
}

impl TableSpec {
    /// The table this spec places with its fixed corner at `at`.
    pub fn make(&self, at: [f64; 2], style: &cadrs_drawing::DrawingStyle) -> Table {
        if self.revision {
            Table::revision(self.rows, self.fixed, at, style)
        } else {
            Table::new(self.rows, self.cols, self.title, self.header, self.fixed, at, style)
        }
    }
}

/// Note and table state (not saved).
#[derive(Resource, Debug, Default)]
pub struct NotesUi {
    pub selected: Vec<Item>,
    pub hovered: Option<Item>,
    pub edit: Option<TextEdit>,
    pub drag: Option<ItemDrag>,
    pub cells: Option<CellSel>,
    /// The table the Table tool places.
    pub table_spec: TableSpec,
    /// The last press: (time, screen position, what it hit), for double-clicks.
    last_press: Option<(f64, Vec2, Option<Hit>)>,
    /// The frame a tool click was handled in (that press isn't the notes' to handle again).
    tool_press: Option<u32>,
    /// The leader the Note tool picked first (D9.1).
    pub pending_leader: Option<Leader>,
}

impl NotesUi {
    pub fn clear_selection(&mut self) {
        self.selected.clear();
        self.cells = None;
    }
}

/// The entity that holds keyboard focus while text is typed on the sheet (a text field for the
/// other shortcuts, so they stay quiet).
#[derive(Resource, Debug, Clone, Copy)]
pub struct EditorFocus(pub Entity);

fn spawn_editor_focus(mut commands: Commands) {
    let e = commands.spawn((Name::new("note-editor"), TextInputField)).id();
    commands.insert_resource(EditorFocus(e));
}

// ---------------------------------------------------------------------------------------------
// Access

/// The active sheet: (drawing element, sheet index, sheet id).
fn active_sheet(doc: &ActiveDocument, dui: &DrawingUi) -> Option<(cadrs_core::ElementId, usize, SheetId)> {
    let (id, d) = active_drawing(doc)?;
    let i = dui.sheet_index(id, d);
    Some((id, i, d.sheets.get(i)?.id))
}

pub fn find_note(d: &Drawing, sheet: usize, id: NoteId) -> Option<&Note> {
    d.sheets.get(sheet)?.notes.iter().find(|n| n.id == id)
}

pub fn find_table(d: &Drawing, sheet: usize, id: TableId) -> Option<&Table> {
    d.sheets.get(sheet)?.tables.iter().find(|t| t.id == id)
}

/// Today (year, month, day) by the app clock.
pub(crate) fn today(clock: Option<&AppClock>) -> Option<(i32, u32, u32)> {
    use chrono::Datelike;
    let c = clock?;
    let d = chrono::DateTime::from_timestamp(c.now() + c.utc_offset, 0)?;
    Some((d.year(), d.month(), d.day()))
}

/// What fields read on sheet `index` (D9.4).
pub fn drawing_context(doc: &ActiveDocument, d: &Drawing, index: usize, clock: Option<&AppClock>) -> DrawingContext {
    let s = &d.sheets[index.min(d.sheets.len().saturating_sub(1))];
    DrawingContext {
        drawing_name: doc.active_element().map(|e| e.name.clone()).unwrap_or_default(),
        sheet_name: s.name.clone(),
        sheet_index: index,
        sheet_count: d.sheets.len(),
        scale: s.scale,
        size: s.format.size,
        projection: d.projection,
        units: d.units,
        date: today(clock),
        date_format: d.style.date_format,
        title: d.title.clone(),
    }
}

/// The sheet's reference properties and drawing context, for fields.
pub fn field_sources(world: &World) -> Option<(ReferenceProps, DrawingContext)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let dui = world.resource::<DrawingUi>();
    let (_, index, _) = active_sheet(doc, dui)?;
    let (_, d) = active_drawing(doc)?;
    let props = super::reference_props(&doc.doc, d.sheets[index].reference);
    let ctx = drawing_context(doc, d, index, world.get_resource::<AppClock>());
    Some((props, ctx))
}

fn px_mm(doc: &ActiveDocument, dui: &DrawingUi) -> f64 {
    current_view(doc, dui).map(|(_, v)| 1.0 / v.ppm as f64).unwrap_or(1.0)
}

fn v2(p: [f64; 2]) -> Vec2 {
    Vec2::new(p[0] as f32, p[1] as f32)
}

fn f2(p: Vec2) -> [f64; 2] {
    [p.x as f64, p.y as f64]
}

// ---------------------------------------------------------------------------------------------
// The scene

/// An item as drawn now.
#[derive(Debug, Clone)]
pub enum ItemGraphics {
    Note(Box<NoteGraphics>),
    Table(Box<TableGraphics>),
}

/// The notes and tables of the active sheet as drawn now (drawn by the annotation systems).
#[derive(Resource, Default)]
pub struct NoteScene {
    key: Option<NoteKey>,
    pub items: Vec<(Item, ItemGraphics)>,
    pub strokes: Vec<(Vec<Vec2>, Color)>,
    pub fills: Vec<([Vec2; 3], Color)>,
    /// Drawn under the text (selections, fields).
    pub backs: Vec<([Vec2; 3], Color)>,
    pub texts: Vec<super::annotations::SceneText>,
    /// The edited note's or cell's graphics (for the toolbar's place and the caret).
    pub edit_frame: Option<[[f64; 2]; 4]>,
    /// Every table's box (min, max; sheet mm), placed or being placed: tables are opaque, so
    /// the view lines are clipped out of them (the views' own gizmos draw over any mesh).
    pub table_rects: Vec<(Vec2, Vec2)>,
}

#[derive(Clone, PartialEq)]
struct NoteKey {
    notes: Vec<Note>,
    tables: Vec<Table>,
    views: Vec<View>,
    geometry: Vec<Option<usize>>,
    props: ReferenceProps,
    ctx: DrawingContext,
    arrow: f64,
    units: cadrs_drawing::DrawingUnits,
    selected: Vec<Item>,
    hovered: Option<Item>,
    edit: Option<TextEdit>,
    drag: Option<ItemDrag>,
    cells: Option<CellSel>,
    tool: AnnTool,
    pick: Option<Pick>,
    pick_view: Option<ViewId>,
    pointer: Option<Vec2>,
    ppm: f32,
    table_spec: TableSpec,
    /// The BOM table being placed (P3C.5).
    bom_preview: Option<Table>,
}

/// The views of the sheet with their geometry, by id.
pub type Geo<'a> = dyn Fn(ViewId) -> Option<(&'a View, &'a ViewGeometry)> + 'a;

/// A note as the drag (or edit) would leave it.
fn shown_note(n: &Note, ui: &NotesUi, ctx: &FieldContext, views: &Geo, px: f64) -> Note {
    if let Some(e) = &ui.edit
        && e.target == (EditTarget::Note { id: n.id, new: false })
        && let Some(note) = &e.note
    {
        let mut shown = note.clone();
        shown.text = e.editor.text();
        return shown;
    }
    match ui.drag {
        Some(d) if d.moving && d.item == Item::Note(n.id) => dragged_note(n, d, ctx, views, px).unwrap_or_else(|| n.clone()),
        _ => n.clone(),
    }
}

/// A note after a drag of its body or a grip.
fn dragged_note(n: &Note, d: ItemDrag, ctx: &FieldContext, views: &Geo, px: f64) -> Option<Note> {
    match d.grip {
        DragGrip::Body => {
            let mut m = n.clone();
            m.at = [n.at[0] + (d.at.x - d.start.x) as f64, n.at[1] + (d.at.y - d.start.y) as f64];
            Some(m)
        }
        DragGrip::Note(NoteGrip::Leader(i)) => {
            let l = n.leaders.get(i)?;
            let (v, g) = views(l.view)?;
            let h = hover_in(v, g, d.at, 5.0 * px, 7.0 * px, |_| true)?;
            let mut out = n.clone();
            out.leaders[i] = leader_of(&h, v, d.at);
            Some(out)
        }
        DragGrip::Note(g) => Some(drag_grip(n, ctx, g, f2(d.start), f2(d.at))),
        _ => None,
    }
}

/// A table after a drag of a grip.
fn dragged_table(t: &Table, d: ItemDrag) -> Option<Table> {
    match d.grip {
        DragGrip::Table(TableGrip::Corner(_)) => Some(t.moved([(d.at.x - d.start.x) as f64, (d.at.y - d.start.y) as f64])),
        DragGrip::Table(TableGrip::Side(s)) => t.resize(s, f2(d.at)).ok(),
        DragGrip::Table(TableGrip::Column(i)) => t.set_column_edge(i, d.at.x as f64).ok(),
        _ => None,
    }
}

/// A leader to what is under the cursor (`at`, sheet mm).
pub fn leader_of(h: &Hover, v: &View, at: Vec2) -> Leader {
    let end = match h.pick() {
        Pick::Point(p) => LeaderEnd::Point(p),
        Pick::Edge(e) => LeaderEnd::Edge { edge: e, at: h.shape.nearest(v.from_sheet(f2(at))) },
    };
    Leader { view: v.id, end }
}

#[allow(clippy::too_many_arguments)]
fn rebuild_note_scene(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    dui: Res<DrawingUi>,
    ann: Res<AnnotationUi>,
    ui: Res<NotesUi>,
    cache: Res<ViewCache>,
    clock: Option<Res<AppClock>>,
    bom: Res<super::bom_tools::BomUi>,
    mut scene: ResMut<NoteScene>,
) {
    let Some(doc) = doc.filter(|_| *kind == ActiveKind::Drawing) else {
        if scene.key.is_some() {
            *scene = NoteScene::default();
        }
        return;
    };
    let Some((_, index, _)) = active_sheet(&doc, &dui) else {
        if scene.key.is_some() {
            *scene = NoteScene::default();
        }
        return;
    };
    let Some((_, d)) = active_drawing(&doc) else {
        return;
    };
    let sheet = &d.sheets[index];
    let views = sheet_views(&doc, &dui, &cache);
    let ppm = current_view(&doc, &dui).map(|(_, v)| v.ppm).unwrap_or(1.0);
    let key = NoteKey {
        notes: sheet.notes.clone(),
        // BOM Table properties… previews its settings live on its table (P3C wrap-up).
        tables: sheet.tables.iter().map(|t| super::bom_tools::properties_preview(&bom, t, &d.style).unwrap_or_else(|| t.clone())).collect(),
        views: views.iter().map(|(v, _)| v.clone()).collect(),
        geometry: views.iter().map(|(_, g)| g.as_ref().map(|g| Arc::as_ptr(g) as usize)).collect(),
        props: super::reference_props(&doc.doc, sheet.reference),
        ctx: drawing_context(&doc, d, index, clock.as_deref()),
        arrow: d.style.dim_arrow_length,
        units: d.units,
        selected: ui.selected.clone(),
        hovered: ui.hovered,
        edit: ui.edit.clone(),
        drag: ui.drag,
        cells: ui.cells,
        tool: ann.tool,
        pick: ann.picks.first().copied(),
        pick_view: ann.pick_view,
        pointer: dui.pointer,
        ppm,
        table_spec: ui.table_spec,
        bom_preview: match (ann.tool, dui.pointer) {
            (AnnTool::PlaceBom, Some(p)) if (0.0..=sheet.size_mm().0).contains(&(p.x as f64)) && (0.0..=sheet.size_mm().1).contains(&(p.y as f64)) => {
                super::bom_tools::preview_table(&bom, d, sheet, f2(p))
            }
            _ => None,
        },
    };
    if scene.key.as_ref() == Some(&key) {
        return;
    }
    let bom_preview = &key.bom_preview;
    let px = 1.0 / ppm as f64;
    let geo = |id: ViewId| -> Option<(&View, &ViewGeometry)> {
        views.iter().find(|(v, _)| v.id == id).and_then(|(v, g)| g.as_ref().map(|g| (v, &**g)))
    };
    let lookup = |id: ViewId| -> Option<(&View, &dyn ViewModel)> { geo(id).map(|(v, g)| (v, g as &dyn ViewModel)) };
    let fctx = FieldContext { reference: &key.props, drawing: &key.ctx };
    let mut out = NoteScene::default();
    let grip_h = 3.0 / ppm;
    let sel_back = Color::srgb_u8(0xc6, 0xdc, 0xfa);
    let field_back = Color::srgb_u8(0xe6, 0xe9, 0xee);
    let quad = |q: [[f64; 2]; 4], c: Color| [([v2(q[0]), v2(q[1]), v2(q[2])], c), ([v2(q[0]), v2(q[2]), v2(q[3])], c)];
    // Notes (the one being placed or edited too).
    let mut notes: Vec<(Note, bool)> = sheet.notes.iter().map(|n| (shown_note(n, &ui, &fctx, &geo, px), false)).collect();
    if let Some(e) = &ui.edit
        && let EditTarget::Note { new: true, .. } = e.target
        && let Some(n) = &e.note
    {
        let mut n = n.clone();
        n.text = e.editor.text();
        notes.push((n, true));
    }
    // The Note tool's leader note following the cursor.
    if ann.tool == AnnTool::Note
        && !ann.picks.is_empty()
        && let (Some(l), Some(p)) = (ui.pending_leader, dui.pointer)
    {
        let mut n = Note::new(f2(p), d.style.note_text_height);
        n.text = RichText::plain("Note");
        n.leaders.push(l);
        notes.push((n, true));
    }
    for (n, preview) in &notes {
        let item = Item::Note(n.id);
        let editing = ui.edit.as_ref().filter(|e| matches!(e.target, EditTarget::Note { id, .. } if id == n.id));
        let selected = ui.selected.contains(&item);
        let g = note_graphics(n, &fctx, &lookup, key.arrow, selected && editing.is_none());
        let color = if editing.is_some() {
            ink()
        } else if (*preview && ann.tool == AnnTool::Note) || selected || ui.drag.is_some_and(|d| d.moving && d.item == item) {
            orange()
        } else if ui.hovered == Some(item) {
            hover_orange()
        } else {
            ink()
        };
        emit_texts(&mut out, &g.texts, color);
        // A leader whose geometry is gone is red (P3C.6), unless the note is highlighted.
        let red = |i: usize, dangling: &[usize]| if color == ink() && dangling.contains(&i) { super::annotations::dangling_red() } else { color };
        for (i, s) in g.strokes.iter().enumerate() {
            out.strokes.push((s.iter().map(|p| v2(*p)).collect(), red(i, &g.dangling_strokes)));
        }
        for (i, t) in g.fills.iter().enumerate() {
            out.fills.push((t.map(v2), red(i, &g.dangling_fills)));
        }
        if let Some(e) = editing {
            // Fields shaded, the selection highlighted, the caret, the frame and the ruler.
            for q in &g.fields {
                out.backs.extend(quad(*q, field_back));
            }
            let r = e.editor.selection();
            if !r.is_empty() {
                selection_quads(&mut out, &g.layout, r, |p| n.to_sheet(p), sel_back);
            }
            if let Some((base, top)) = g.stops.get(e.editor.caret) {
                out.strokes.push((vec![v2(*base), v2(*top)], ink()));
            }
            let f = g.frame;
            out.strokes.push((vec![v2(f[0]), v2(f[1]), v2(f[2]), v2(f[3]), v2(f[0])], Color::srgb_u8(0x9a, 0xb4, 0xd8)));
            ruler(&mut out, n, &g, key.units, px);
            out.edit_frame = Some(f);
        } else if selected {
            let f = g.frame;
            out.strokes.push((vec![v2(f[0]), v2(f[1]), v2(f[2]), v2(f[3]), v2(f[0])], Color::srgb_u8(0xf3, 0xc0, 0x90)));
            for (p, k) in &g.grips {
                match k {
                    NoteGrip::Rotate => out.fills.extend(disc(v2(*p), grip_h * 1.1, blue())),
                    _ => out.fills.extend(square(v2(*p), grip_h, blue())),
                }
            }
        }
        if !preview || editing.is_some() {
            out.items.push((item, ItemGraphics::Note(Box::new(g))));
        }
    }
    // Tables.
    let mut tables: Vec<(Table, bool)> = key
        .tables
        .iter()
        .map(|t| {
            let shown = match ui.drag {
                Some(d) if d.moving && d.item == Item::Table(t.id) => dragged_table(t, d).unwrap_or_else(|| t.clone()),
                _ => t.clone(),
            };
            (shown, false)
        })
        .collect();
    // The table follows the cursor only over the sheet (not off it on the grey).
    let (sw, sh) = sheet.size_mm();
    if ann.tool == AnnTool::Table
        && let Some(p) = dui.pointer
        && (0.0..=sw).contains(&(p.x as f64))
        && (0.0..=sh).contains(&(p.y as f64))
    {
        let s = ui.table_spec;
        tables.push((s.make(f2(p), &d.style), true));
    }
    // A BOM table being placed (P3C.5): at the cursor, snapped to the frame's corner.
    if let Some(t) = bom_preview.clone() {
        tables.push((t, true));
    }
    for (t, preview) in &tables {
        let item = Item::Table(t.id);
        let selected = ui.selected.contains(&item);
        let edited = ui.edit.as_ref().and_then(|e| match e.target {
            EditTarget::Cell { table, cell } if table == t.id => Some((cell, e.editor.text(), e)),
            _ => None,
        });
        // Selected cells show as cells (and the cell toolbar), not as the whole table with its
        // grips (P3C.4's delta: table and cell selection were mixed).
        let cell_sel = ui.cells.is_some_and(|c| c.table == t.id);
        let g = table_graphics(t, &fctx, selected && !preview && !cell_sel, edited.as_ref().map(|(c, text, _)| (*c, text)));
        // A table selected as a whole, or being dragged by a grip: its outline orange (the grid
        // stays ink, P3C.8).
        let dragging = ui.drag.is_some_and(|d| d.moving && d.item == item);
        let outline_color = if (selected && !cell_sel && !preview) || dragging { orange() } else { Color::NONE };
        let color = if *preview {
            orange()
        } else if ui.hovered == Some(item) && !selected {
            hover_orange()
        } else {
            ink()
        };
        // An opaque sheet-coloured background, so view lines and shading don't show through
        // (placed or following the cursor).
        out.backs.extend(quad([g.min, [g.max[0], g.min[1]], g.max, [g.min[0], g.max[1]]], Color::WHITE));
        out.table_rects.push((v2(g.min), v2(g.max)));
        // Selected cells.
        if let Some(cs) = ui.cells.filter(|c| c.table == t.id) {
            let ((r0, c0), (r1, c1)) = cs.range();
            for (cell, lo, hi) in &g.cells {
                if cell.0 >= r0 && cell.0 <= r1 && cell.1 >= c0 && cell.1 <= c1 {
                    out.backs.extend(quad([*lo, [hi[0], lo[1]], *hi, [lo[0], hi[1]]], Color::srgb_u8(0xdd, 0xea, 0xfb)));
                }
            }
        }
        for (a, b) in &g.thin {
            out.strokes.push((vec![v2(*a), v2(*b)], color));
        }
        let outline = if outline_color == Color::NONE { color } else { outline_color };
        for (a, b) in &g.outline {
            out.strokes.push((vec![v2(*a), v2(*b)], outline));
        }
        // A heavier outline: a second stroke just inside it.
        let inset = 0.35 / 2.0;
        let (lo, hi) = (g.min, g.max);
        out.strokes.push((
            vec![
                v2([lo[0] + inset, lo[1] + inset]),
                v2([hi[0] - inset, lo[1] + inset]),
                v2([hi[0] - inset, hi[1] - inset]),
                v2([lo[0] + inset, hi[1] - inset]),
                v2([lo[0] + inset, lo[1] + inset]),
            ],
            outline,
        ));
        let text_color = if *preview { orange() } else { ink() };
        emit_texts(&mut out, &g.texts, text_color);
        for s in &g.strokes {
            out.strokes.push((s.iter().map(|p| v2(*p)).collect(), text_color));
        }
        if let Some((cell, _, e)) = &edited
            && let Some((_, origin, l)) = g.layouts.iter().find(|(c, _, _)| c == cell)
        {
            let at = |p: [f64; 2]| [origin[0] + p[0], origin[1] + p[1]];
            let r = e.editor.selection();
            if !r.is_empty() {
                selection_quads(&mut out, l, r, at, sel_back);
            }
            for (lo, hi) in &l.fields {
                out.backs.extend(quad([at(*lo), at([hi[0], lo[1]]), at(*hi), at([lo[0], hi[1]])], field_back));
            }
            if let Some(s) = l.stops.get(e.editor.caret) {
                out.strokes.push((
                    vec![v2(at([s.x, s.baseline - 0.25 * s.height])), v2(at([s.x, s.baseline + 1.15 * s.height]))],
                    ink(),
                ));
            }
            if let Some((lo, hi)) = g.cell_box(*cell) {
                out.strokes.push((
                    vec![v2(lo), v2([hi[0], lo[1]]), v2(hi), v2([lo[0], hi[1]]), v2(lo)],
                    blue(),
                ));
                if e.toolbar {
                    // The toolbar goes above the table, not over it.
                    let (tlo, thi) = (g.min, g.max);
                    out.edit_frame = Some([[tlo[0], thi[1]], thi, [thi[0], tlo[1]], tlo]);
                }
            }
        }
        if selected && !preview && !cell_sel {
            for (p, k) in &g.grips {
                let c = match k {
                    TableGrip::Corner(c) if *c == t.fixed => Color::BLACK,
                    // The column lines are dragged where they meet the top edge; no square.
                    TableGrip::Column(_) => continue,
                    _ => blue(),
                };
                out.fills.extend(square(v2(*p), grip_h, c));
            }
        }
        if !preview {
            out.items.push((item, ItemGraphics::Table(Box::new(g))));
        }
    }
    out.key = Some(key);
    *scene = out;
}

fn emit_texts(out: &mut NoteScene, texts: &[cadrs_drawing::note::NoteText], color: Color) {
    for t in texts {
        out.texts.push(super::annotations::SceneText {
            text: cadrs_drawing::annotation::PlacedText { pos: t.pos, height: t.piece.height, text: t.piece.text.clone() },
            color,
            bold: t.piece.bold,
            italic: t.piece.italic,
            rotation: t.rotation as f32,
        });
    }
}

/// Highlight boxes behind the selected text of a layout (`to` maps its points to the sheet).
fn selection_quads(out: &mut NoteScene, l: &rich::RichLayout, r: std::ops::Range<usize>, to: impl Fn([f64; 2]) -> [f64; 2], c: Color) {
    // One box per line: from the first to the last selected stop on that line.
    let mut i = r.start;
    while i < r.end {
        let s0 = l.stops[i];
        let mut j = i;
        while j < r.end && (l.stops[j + 1].baseline - s0.baseline).abs() < 1e-9 {
            j += 1;
        }
        let x1 = if j > i { l.stops[j].x } else { s0.x + 0.6 * s0.height };
        let (y0, y1) = (s0.baseline - 0.3 * s0.height, s0.baseline + 1.25 * s0.height);
        let q = [to([s0.x, y0]), to([x1, y0]), to([x1, y1]), to([s0.x, y1])];
        out.backs.push(([v2(q[0]), v2(q[1]), v2(q[2])], c));
        out.backs.push(([v2(q[0]), v2(q[2]), v2(q[3])], c));
        i = j + 1;
    }
}

/// A filled disc (a rotation grip).
fn disc(c: Vec2, r: f32, color: Color) -> Vec<([Vec2; 3], Color)> {
    let n = 16;
    (0..n)
        .map(|k| {
            let a0 = std::f32::consts::TAU * k as f32 / n as f32;
            let a1 = std::f32::consts::TAU * (k + 1) as f32 / n as f32;
            ([c, c + Vec2::new(a0.cos(), a0.sin()) * r, c + Vec2::new(a1.cos(), a1.sin()) * r], color)
        })
        .collect()
}

/// The ruler along the top of the note being edited, with the wrap width's handles (D9.3).
fn ruler(out: &mut NoteScene, n: &Note, g: &NoteGraphics, units: cadrs_drawing::DrawingUnits, px: f64) {
    let h = n.height;
    let (left, right) = ruler_ends(n, g);
    let y0 = ruler_y(n, g);
    let s = |x: f64, y: f64| v2(n.to_sheet([x, y]));
    let grey = Color::srgb_u8(0x70, 0x78, 0x84);
    let band = h * 1.1;
    // The band.
    let lo = left - 3.0 * h;
    let hi = right + 6.0 * h;
    let back = Color::srgb_u8(0xf3, 0xf5, 0xf8);
    out.backs.push(([s(lo, y0), s(hi, y0), s(hi, y0 + band)], back));
    out.backs.push(([s(lo, y0), s(hi, y0 + band), s(lo, y0 + band)], back));
    out.strokes.push((vec![s(lo, y0), s(hi, y0)], grey));
    // Ticks: tenths of an inch or millimetres by 2, longer each half inch or centimetre.
    let (minor, every) = match units {
        cadrs_drawing::DrawingUnits::Inch => (2.54, 5),
        cadrs_drawing::DrawingUnits::Millimeter => (2.0, 5),
    };
    let mut k = 0;
    let mut x = left;
    while x <= hi {
        let long = k % every == 0;
        let t = if long { band * 0.6 } else { band * 0.3 };
        out.strokes.push((vec![s(x, y0), s(x, y0 + t)], grey));
        x += minor;
        k += 1;
    }
    // Double-arrow handles at the text's left edge and the wrap width.
    let hs = (h * 0.55).max(4.0 * px);
    for x in [left, right] {
        let c = n.to_sheet([x, y0 + band * 0.5]);
        let (a, b) = (s(x - hs * 1.6, y0 + band * 0.5), s(x + hs * 1.6, y0 + band * 0.5));
        let up = s(x, y0 + band * 0.5 + hs) - v2(c);
        out.fills.push(([a, v2(c) + up * 0.9 - (b - a) * 0.12, v2(c) - up * 0.9 - (b - a) * 0.12], blue()));
        out.fills.push(([b, v2(c) + up * 0.9 + (b - a) * 0.12, v2(c) - up * 0.9 + (b - a) * 0.12], blue()));
    }
}

/// The ruler's left and right handles (box x).
fn ruler_ends(n: &Note, g: &NoteGraphics) -> (f64, f64) {
    (0.0, g.layout.width.max(n.height))
}

/// The ruler's bottom (box y).
fn ruler_y(n: &Note, _g: &NoteGraphics) -> f64 {
    n.height * 0.9
}

/// The grips of the note being edited: the ruler's handles.
fn edit_grips(n: &Note, g: &NoteGraphics) -> Vec<([f64; 2], NoteGrip)> {
    let (l, r) = ruler_ends(n, g);
    let y = ruler_y(n, g) + n.height * 0.55;
    vec![(n.to_sheet([l, y]), NoteGrip::Left), (n.to_sheet([r, y]), NoteGrip::Right)]
}

// ---------------------------------------------------------------------------------------------
// Pointer

fn item_at(scene: &NoteScene, p: Vec2, tol: f64) -> Option<(Item, Option<Cell>)> {
    let q = f2(p);
    scene.items.iter().rev().find_map(|(item, g)| match g {
        ItemGraphics::Note(n) => (n.distance(q) <= tol).then_some((*item, None)),
        ItemGraphics::Table(t) => t.contains(q, tol).then(|| (*item, t.cell_at(q))),
    })
}

fn grip_at(scene: &NoteScene, ui: &NotesUi, p: Vec2, tol: f64) -> Option<(Item, DragGrip)> {
    let q = f2(p);
    let mut best: Option<(f64, (Item, DragGrip))> = None;
    for (item, g) in &scene.items {
        if !ui.selected.contains(item) {
            continue;
        }
        let grips: Vec<([f64; 2], DragGrip)> = match g {
            ItemGraphics::Note(n) => n.grips.iter().map(|(p, k)| (*p, DragGrip::Note(*k))).collect(),
            ItemGraphics::Table(t) => t.grips.iter().map(|(p, k)| (*p, DragGrip::Table(*k))).collect(),
        };
        for (at, k) in grips {
            let d = (at[0] - q[0]).hypot(at[1] - q[1]);
            if d <= tol && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, (*item, k)));
            }
        }
    }
    best.map(|(_, x)| x)
}

/// Double-click time (s) and distance (px).
const DOUBLE_TIME: f64 = 0.45;
const DOUBLE_DIST: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
fn notes_pointer(
    mut inputs: MessageReader<PointerInput>,
    kind: Res<ActiveKind>,
    doc: Option<Res<ActiveDocument>>,
    rect: Res<ViewportRect>,
    hover_map: Res<HoverMap>,
    q_area: Query<Entity, With<ViewportArea>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    keys: Res<ButtonInput<KeyCode>>,
    time: Res<Time<Real>>,
    frame: Res<bevy::diagnostic::FrameCount>,
    scene: Res<NoteScene>,
    ann: Res<AnnotationUi>,
    mut dui: ResMut<DrawingUi>,
    mut ui: ResMut<NotesUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        inputs.clear();
        return;
    }
    let Some(doc) = doc else {
        inputs.clear();
        return;
    };
    let Some((_, view)) = current_view(&doc, &dui) else {
        inputs.clear();
        return;
    };
    let area = sheet_area(&rect, &dui);
    let over = super::view_tools::pointer_over_sheet(&hover_map, &q_area) && q_menus.is_empty() && q_dialogs.is_empty();
    let px = px_mm(&doc, &dui);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let now = time.elapsed_secs_f64();
    for input in inputs.read() {
        if input.pointer_id != PointerId::Mouse {
            continue;
        }
        let pos = input.location.position;
        let p = screen_to_sheet(view, area, pos);
        match input.action {
            PointerAction::Move { .. } => {
                if let Some(mut d) = ui.drag {
                    d.at = p;
                    if !d.moving && (p - d.start).length() as f64 > 3.0 * px {
                        d.moving = true;
                    }
                    ui.drag = Some(d);
                    if d.grip == DragGrip::Select {
                        set_caret_at(&mut ui, &scene, p, true);
                    } else if d.moving
                        && let DragGrip::Note(g) = d.grip
                        && let Some(e) = ui.edit.as_mut()
                        && let Some(n) = e.note.as_ref()
                    {
                        // Dragging the ruler's handles re-wraps the note being edited.
                        let mut shown = n.clone();
                        shown.text = e.editor.text();
                        let r = ReferenceProps::default();
                        let c = DrawingContext::default();
                        let ctx = FieldContext { reference: &r, drawing: &c };
                        let mut m = drag_grip(&shown, &ctx, g, f2(d.start), f2(d.at));
                        m.text = n.text.clone();
                        e.note = Some(m);
                        ui.drag.as_mut().unwrap().start = p;
                    }
                }
            }
            PointerAction::Press(PointerButton::Primary) if over => {
                if ann.tool != AnnTool::None || ui.tool_press == Some(frame.0) {
                    continue;
                }
                let hit = item_at(&scene, p, 4.0 * px);
                let double = ui
                    .last_press
                    .is_some_and(|(t, at, h)| now - t < DOUBLE_TIME && at.distance(pos) < DOUBLE_DIST && h == hit && hit.is_some());
                ui.last_press = Some((now, pos, hit));
                if dui.annotation_press {
                    // An annotation took the press.
                    if ui.edit.is_some() {
                        commands.queue(commit_edit);
                    }
                    if !ctrl {
                        ui.clear_selection();
                    }
                    continue;
                }
                // Typing on the sheet: the caret, the ruler's handles; elsewhere accepts.
                if let Some(e) = ui.edit.clone() {
                    if let (Some(n), EditTarget::Note { id, .. }) = (&e.note, e.target)
                        && let Some((_, ItemGraphics::Note(g))) = scene.items.iter().find(|(i, _)| *i == Item::Note(id))
                    {
                        let mut shown = n.clone();
                        shown.text = e.editor.text();
                        let grip = edit_grips(&shown, g)
                            .into_iter()
                            .find(|(at, _)| (at[0] - p.x as f64).hypot(at[1] - p.y as f64) <= 6.0 * px);
                        if let Some((_, k)) = grip {
                            dui.annotation_press = true;
                            ui.drag = Some(ItemDrag { item: Item::Note(id), grip: DragGrip::Note(k), start: p, at: p, moving: false });
                            continue;
                        }
                        if g.distance(f2(p)) <= 2.0 * px {
                            dui.annotation_press = true;
                            set_caret_at(&mut ui, &scene, p, shift);
                            ui.drag = Some(ItemDrag { item: Item::Note(id), grip: DragGrip::Select, start: p, at: p, moving: false });
                            continue;
                        }
                    }
                    if let EditTarget::Cell { table, cell } = e.target
                        && hit == Some((Item::Table(table), Some(cell)))
                    {
                        dui.annotation_press = true;
                        set_caret_at(&mut ui, &scene, p, shift);
                        ui.drag = Some(ItemDrag { item: Item::Table(table), grip: DragGrip::Select, start: p, at: p, moving: false });
                        continue;
                    }
                    commands.queue(commit_edit);
                }
                if let Some((item, grip)) = grip_at(&scene, &ui, p, 5.0 * px) {
                    dui.annotation_press = true;
                    ui.drag = Some(ItemDrag { item, grip, start: p, at: p, moving: false });
                    continue;
                }
                match hit {
                    Some((item, cell)) => {
                        dui.annotation_press = true;
                        dui.selected.clear();
                        match (item, cell) {
                            (Item::Table(t), Some(c)) => {
                                if double {
                                    ui.selected = vec![item];
                                    ui.cells = Some(CellSel { table: t, anchor: c, head: c });
                                    commands.queue(move |w: &mut World| start_cell_edit(w, t, c, true, None));
                                } else if shift && ui.cells.is_some_and(|s| s.table == t) {
                                    if let Some(s) = ui.cells.as_mut() {
                                        s.head = c;
                                    }
                                } else if !ui.selected.contains(&item) {
                                    // A first click selects the whole table (its grips); a click
                                    // on a cell of the selected table selects that cell (its
                                    // toolbar), so the two selections don't mix.
                                    ui.selected = vec![item];
                                    ui.cells = None;
                                } else {
                                    ui.selected = vec![item];
                                    ui.cells = Some(CellSel { table: t, anchor: c, head: c });
                                }
                            }
                            (Item::Note(id), _) => {
                                ui.cells = None;
                                if ctrl {
                                    if let Some(i) = ui.selected.iter().position(|s| *s == item) {
                                        ui.selected.remove(i);
                                    } else {
                                        ui.selected.push(item);
                                    }
                                } else if !ui.selected.contains(&item) {
                                    ui.selected = vec![item];
                                }
                                if double {
                                    commands.queue(move |w: &mut World| start_note_edit(w, id));
                                } else {
                                    ui.drag = Some(ItemDrag { item, grip: DragGrip::Body, start: p, at: p, moving: false });
                                }
                            }
                            (Item::Table(_), None) => {
                                ui.selected = vec![item];
                                ui.cells = None;
                            }
                        }
                    }
                    None => {
                        if !ctrl {
                            ui.clear_selection();
                        }
                    }
                }
            }
            PointerAction::Release(PointerButton::Primary) => {
                if let Some(d) = ui.drag.take()
                    && d.moving
                    && d.grip != DragGrip::Select
                {
                    // The ruler of the note being edited changes the draft, not the drawing.
                    let editing = ui.edit.as_ref().is_some_and(|e| matches!(e.target, EditTarget::Note { id, .. } if Item::Note(id) == d.item));
                    if !editing {
                        commands.queue(move |w: &mut World| finish_drag(w, d));
                    }
                }
            }
            PointerAction::Cancel => {
                ui.drag = None;
            }
            _ => {}
        }
    }
    // Hover highlight (no tool, not dragging, not editing).
    let hovered = match (ann.tool, dui.pointer, over, ui.drag) {
        // Nothing under a note being edited or its popups (P3C.8: hover showed through them).
        (AnnTool::None, Some(p), true, None) if !dui.annotation_hover && ui.edit.is_none() => item_at(&scene, p, 4.0 * px).map(|(i, _)| i),
        _ => None,
    };
    if ui.hovered != hovered {
        ui.hovered = hovered;
    }
    if (hovered.is_some() || ui.drag.is_some()) && !dui.annotation_hover {
        dui.annotation_hover = true;
    }
}

/// Puts the caret of the text being edited at a sheet point.
fn set_caret_at(ui: &mut NotesUi, scene: &NoteScene, p: Vec2, extend: bool) {
    let Some(e) = ui.edit.as_mut() else {
        return;
    };
    let q = f2(p);
    let pos = match e.target {
        EditTarget::Note { id, .. } => {
            let Some((_, ItemGraphics::Note(g))) = scene.items.iter().find(|(i, _)| *i == Item::Note(id)) else {
                return;
            };
            let Some(n) = e.note.as_ref() else {
                return;
            };
            rich::stop_at(&g.layout, n.from_sheet(q))
        }
        EditTarget::Cell { table, cell } => {
            let Some((_, ItemGraphics::Table(g))) = scene.items.iter().find(|(i, _)| *i == Item::Table(table)) else {
                return;
            };
            let Some((_, origin, l)) = g.layouts.iter().find(|(c, _, _)| *c == cell) else {
                return;
            };
            rich::stop_at(l, [q[0] - origin[0], q[1] - origin[1]])
        }
    };
    e.editor.set_caret(pos, extend);
}

fn finish_drag(w: &mut World, d: ItemDrag) {
    let Some(doc) = w.get_resource::<ActiveDocument>() else {
        return;
    };
    let dui = w.resource::<DrawingUi>();
    let Some((_, index, sheet)) = active_sheet(doc, dui) else {
        return;
    };
    let Some((_, dr)) = active_drawing(doc) else {
        return;
    };
    let px = px_mm(doc, dui);
    let op = match d.item {
        Item::Note(id) => {
            let Some(n) = find_note(dr, index, id).cloned() else {
                return;
            };
            let Some((props, ctx)) = field_sources(w) else {
                return;
            };
            let fctx = FieldContext { reference: &props, drawing: &ctx };
            let views = sheet_views(doc, dui, w.resource::<ViewCache>());
            let geo = |id: ViewId| -> Option<(&View, &ViewGeometry)> {
                views.iter().find(|(v, _)| v.id == id).and_then(|(v, g)| g.as_ref().map(|g| (v, &**g)))
            };
            let Some(m) = dragged_note(&n, d, &fctx, &geo, px) else {
                return;
            };
            if m == n {
                return;
            }
            let label = match d.grip {
                DragGrip::Body => "Move note",
                DragGrip::Note(NoteGrip::Rotate) => "Rotate note",
                DragGrip::Note(NoteGrip::Scale) => "Resize note text",
                DragGrip::Note(NoteGrip::Leader(_)) => "Re-attach leader",
                _ => "Resize note",
            };
            DrawingOp::SetNote { sheet, note: m, label: label.into() }
        }
        Item::Table(id) => {
            let Some(t) = find_table(dr, index, id) else {
                return;
            };
            let Some(m) = dragged_table(t, d) else {
                return;
            };
            let label = match d.grip {
                DragGrip::Table(TableGrip::Side(_)) => "Resize table",
                DragGrip::Table(TableGrip::Column(_)) => "Resize column",
                _ => "Move table",
            };
            DrawingOp::SetTable { sheet, table: fit(w, m), label: label.into() }
        }
    };
    edit_drawing(w, op);
}

// ---------------------------------------------------------------------------------------------
// Tools

/// A click with the Note, Table or Add leader tool.
pub fn tool_click(w: &mut World, tool: AnnTool, hover: Option<Hover>, p: Vec2) {
    let frame = w.resource::<bevy::diagnostic::FrameCount>().0;
    w.resource_mut::<NotesUi>().tool_press = Some(frame);
    let (sheet, style) = {
        let Some(doc) = w.get_resource::<ActiveDocument>() else {
            return;
        };
        let dui = w.resource::<DrawingUi>();
        let Some((_, _, sheet)) = active_sheet(doc, dui) else {
            return;
        };
        let Some((_, d)) = active_drawing(doc) else {
            return;
        };
        (sheet, d.style.clone())
    };
    let view_of_hover = |w: &World, h: &Hover| -> Option<View> {
        let doc = w.get_resource::<ActiveDocument>()?;
        let (_, d) = active_drawing(doc)?;
        d.view(h.view).map(|(_, v)| v.clone())
    };
    match tool {
        AnnTool::Note => {
            let picked = !w.resource::<AnnotationUi>().picks.is_empty();
            if !picked
                && let Some(h) = hover
                && let Some(v) = view_of_hover(w, &h)
            {
                // The leader's end first; the text follows the cursor.
                w.resource_mut::<NotesUi>().pending_leader = Some(leader_of(&h, &v, p));
                let mut ann = w.resource_mut::<AnnotationUi>();
                ann.picks = vec![h.pick()];
                ann.pick_view = Some(h.view);
                return;
            }
            let mut note = Note::new(f2(p), style.note_text_height);
            if picked && let Some(l) = w.resource_mut::<NotesUi>().pending_leader.take() {
                note.leaders.push(l);
            }
            {
                let mut ann = w.resource_mut::<AnnotationUi>();
                ann.tool = AnnTool::None;
                ann.reset_picks();
            }
            let id = note.id;
            let editor = Editor::new(&note.text);
            let mut ui = w.resource_mut::<NotesUi>();
            ui.clear_selection();
            ui.edit = Some(TextEdit { target: EditTarget::Note { id, new: true }, editor, note: Some(note), toolbar: true });
            focus_editor(w);
        }
        AnnTool::Table => {
            let spec = w.resource::<NotesUi>().table_spec;
            let t = spec.make(f2(p), &style);
            let id = t.id;
            w.resource_mut::<AnnotationUi>().tool = AnnTool::None;
            super::note_bar::close_table_dialog(w);
            let t = fit(w, t);
            if edit_drawing(w, DrawingOp::AddTable { sheet, table: t }) {
                {
                    let mut ui = w.resource_mut::<NotesUi>();
                    ui.selected = vec![Item::Table(id)];
                    ui.cells = Some(CellSel { table: id, anchor: (0, 0), head: (0, 0) });
                }
                // Typing goes straight into the first cell (D10.2).
                start_cell_edit(w, id, (0, 0), false, None);
            }
        }
        AnnTool::AddLeader(id) => {
            let Some(h) = hover else {
                return;
            };
            let Some(v) = view_of_hover(w, &h) else {
                return;
            };
            let note = (|| {
                let doc = w.get_resource::<ActiveDocument>()?;
                let dui = w.resource::<DrawingUi>();
                let (_, index, _) = active_sheet(doc, dui)?;
                let (_, d) = active_drawing(doc)?;
                find_note(d, index, id).cloned()
            })();
            let Some(mut note) = note else {
                return;
            };
            note.leaders.push(leader_of(&h, &v, p));
            edit_drawing(w, DrawingOp::SetNote { sheet, note, label: "Add leader".into() });
            w.resource_mut::<NotesUi>().selected = vec![Item::Note(id)];
        }
        _ => {}
    }
}

/// Starts the Note tool (N, the toolbar), ending any text edit.
pub fn start_note_tool(w: &mut World, tool: AnnTool) {
    commit_edit(w);
    w.resource_mut::<NotesUi>().clear_selection();
    super::annotations::start_tool(w, tool);
}

// ---------------------------------------------------------------------------------------------
// Editing

fn focus_editor(w: &mut World) {
    let e = w.resource::<EditorFocus>().0;
    w.resource_mut::<InputFocus>().set(e, bevy::input_focus::FocusCause::Navigated);
}

/// Double-click (or Edit… on its menu, or Enter): edits a note in place with the toolbar.
pub fn start_note_edit(w: &mut World, id: NoteId) {
    let note = (|| {
        let doc = w.get_resource::<ActiveDocument>()?;
        let dui = w.resource::<DrawingUi>();
        let (_, index, _) = active_sheet(doc, dui)?;
        let (_, d) = active_drawing(doc)?;
        find_note(d, index, id).cloned()
    })();
    let Some(note) = note else {
        return;
    };
    commit_edit(w);
    let mut editor = Editor::new(&note.text);
    editor.select_all();
    let mut ui = w.resource_mut::<NotesUi>();
    ui.selected = vec![Item::Note(id)];
    ui.cells = None;
    ui.edit = Some(TextEdit { target: EditTarget::Note { id, new: false }, editor, note: Some(note), toolbar: true });
    focus_editor(w);
}

/// Types into a cell: with the note toolbar (a double-click) or without (Tab, typing). `typed`
/// is the first character.
pub fn start_cell_edit(w: &mut World, table: TableId, cell: Cell, toolbar: bool, typed: Option<&str>) {
    let t = (|| {
        let doc = w.get_resource::<ActiveDocument>()?;
        let dui = w.resource::<DrawingUi>();
        let (_, index, _) = active_sheet(doc, dui)?;
        let (_, d) = active_drawing(doc)?;
        find_table(d, index, table).cloned()
    })();
    let Some(t) = t else {
        return;
    };
    let cell = t.origin(cell.0, cell.1);
    let mut editor = Editor::new(&t.cells[cell.0][cell.1]);
    if toolbar {
        editor.select_all();
    }
    if let Some(s) = typed {
        editor.insert_str(s);
    }
    let mut ui = w.resource_mut::<NotesUi>();
    ui.selected = vec![Item::Table(table)];
    ui.cells = Some(CellSel { table, anchor: cell, head: cell });
    ui.edit = Some(TextEdit { target: EditTarget::Cell { table, cell }, editor, note: None, toolbar });
    focus_editor(w);
}

/// Accepts the text being typed (✓, Esc, a click elsewhere): one undoable edit.
pub fn commit_edit(w: &mut World) {
    let Some(e) = w.resource_mut::<NotesUi>().edit.take() else {
        return;
    };
    release_focus(w);
    let Some((sheet, index, d)) = w.get_resource::<ActiveDocument>().and_then(|doc| {
        let dui = w.resource::<DrawingUi>();
        let (_, index, sheet) = active_sheet(doc, dui)?;
        Some((sheet, index, active_drawing(doc)?.1.clone()))
    }) else {
        return;
    };
    let text = e.editor.text();
    match e.target {
        EditTarget::Note { id, new } => {
            let Some(mut n) = e.note else {
                return;
            };
            n.text = text;
            if new {
                if n.text.is_empty() {
                    return;
                }
                if edit_drawing(w, DrawingOp::AddNote { sheet, note: n }) {
                    w.resource_mut::<NotesUi>().selected = vec![Item::Note(id)];
                }
            } else if find_note(&d, index, id) != Some(&n) {
                edit_drawing(w, DrawingOp::SetNote { sheet, note: n, label: "Edit note".into() });
            }
        }
        EditTarget::Cell { table, cell } => {
            let Some(t) = find_table(&d, index, table) else {
                return;
            };
            if t.cells[cell.0][cell.1] == text {
                return;
            }
            if let Ok(m) = t.set_cell(cell, text) {
                let m = fit(w, m);
                edit_drawing(w, DrawingOp::SetTable { sheet, table: m, label: "Edit cell".into() });
            }
        }
    }
}

/// ✗: drops the changes.
pub fn cancel_edit(w: &mut World) {
    w.resource_mut::<NotesUi>().edit = None;
    release_focus(w);
}

fn release_focus(w: &mut World) {
    let e = w.resource::<EditorFocus>().0;
    let mut f = w.resource_mut::<InputFocus>();
    if f.get() == Some(e) {
        f.clear();
    }
}

/// Moves cell typing on to the next cell (Tab) or back (Shift+Tab).
fn tab_cell(w: &mut World, back: bool) {
    let target = w.resource::<NotesUi>().edit.as_ref().map(|e| e.target);
    let sel = w.resource::<NotesUi>().cells;
    let (table, cell, editing) = match (target, sel) {
        (Some(EditTarget::Cell { table, cell }), _) => (table, cell, true),
        (_, Some(s)) => (s.table, s.head, false),
        _ => return,
    };
    if editing {
        commit_edit(w);
    }
    let t = (|| {
        let doc = w.get_resource::<ActiveDocument>()?;
        let dui = w.resource::<DrawingUi>();
        let (_, index, _) = active_sheet(doc, dui)?;
        let (_, d) = active_drawing(doc)?;
        find_table(d, index, table).cloned()
    })();
    let Some(t) = t else {
        return;
    };
    let next = t.next_cell(cell, back);
    if editing {
        start_cell_edit(w, table, next, false, None);
    } else {
        w.resource_mut::<NotesUi>().cells = Some(CellSel { table, anchor: next, head: next });
    }
}

/// Applies an editor change to the text being edited.
pub fn with_editor(w: &mut World, f: impl FnOnce(&mut Editor)) {
    if let Some(e) = w.resource_mut::<NotesUi>().edit.as_mut() {
        f(&mut e.editor);
    }
    focus_editor(w);
}

// ---------------------------------------------------------------------------------------------
// Keys

#[allow(clippy::too_many_arguments)]
fn notes_keys(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    editor_focus: Option<Res<EditorFocus>>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
    ann: Res<AnnotationUi>,
    mut ui: ResMut<NotesUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::Drawing {
        keys_in.clear();
        return;
    }
    let editor = editor_focus.map(|e| e.0);
    let in_editor = ui.edit.is_some() && focus.get().is_some() && focus.get() == editor;
    let typing = !in_editor && focus.get().is_some_and(|e| q_fields.contains(e));
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for k in keys_in.read() {
        if k.state != bevy::input::ButtonState::Pressed || typing || !q_dialogs.is_empty() || !q_menus.is_empty() || alt {
            continue;
        }
        if in_editor {
            let cell = matches!(ui.edit.as_ref().map(|e| e.target), Some(EditTarget::Cell { .. }));
            let e = &mut ui.edit.as_mut().unwrap().editor;
            match k.key_code {
                KeyCode::Escape => {
                    commands.queue(commit_edit);
                }
                KeyCode::Tab if cell => {
                    commands.queue(move |w: &mut World| tab_cell(w, shift));
                }
                KeyCode::Tab => {}
                KeyCode::Enter | KeyCode::NumpadEnter if ctrl => {
                    commands.queue(commit_edit);
                }
                KeyCode::Enter | KeyCode::NumpadEnter => e.newline(),
                KeyCode::Backspace => e.backspace(),
                KeyCode::Delete => e.delete(),
                KeyCode::ArrowLeft => e.left(shift),
                KeyCode::ArrowRight => e.right(shift),
                KeyCode::Home => e.home(shift),
                KeyCode::End => e.end(shift),
                KeyCode::KeyA if ctrl => e.select_all(),
                KeyCode::KeyB if ctrl => e.toggle(Attr::Bold),
                KeyCode::KeyI if ctrl => e.toggle(Attr::Italic),
                KeyCode::KeyU if ctrl => e.toggle(Attr::Underline),
                _ if ctrl => {}
                _ => {
                    let text = k.text.as_ref().map(|t| t.to_string()).or(match &k.logical_key {
                        Key::Character(c) => Some(c.to_string()),
                        Key::Space => Some(" ".into()),
                        _ => None,
                    });
                    if let Some(t) = text.filter(|t| t.chars().all(|c| !c.is_control())) {
                        e.insert_str(&t);
                    }
                }
            }
            continue;
        }
        if ctrl || ann.tool != AnnTool::None {
            continue;
        }
        match k.key_code {
            KeyCode::Tab if ui.cells.is_some() => {
                commands.queue(move |w: &mut World| tab_cell(w, shift));
            }
            // With cells selected, Delete clears their text; the table goes only when it is
            // selected as a whole.
            KeyCode::Delete | KeyCode::Backspace if ui.cells.is_some() => {
                let c = ui.cells.unwrap();
                commands.queue(move |w: &mut World| clear_cells(w, c));
            }
            KeyCode::Delete | KeyCode::Backspace if !ui.selected.is_empty() => {
                let items = std::mem::take(&mut ui.selected);
                ui.cells = None;
                commands.queue(move |w: &mut World| delete_items(w, &items));
            }
            KeyCode::Enter if ui.selected.len() == 1 => {
                match (ui.selected[0], ui.cells) {
                    (Item::Note(id), _) => commands.queue(move |w: &mut World| start_note_edit(w, id)),
                    (Item::Table(t), Some(c)) => commands.queue(move |w: &mut World| start_cell_edit(w, t, c.head, true, None)),
                    _ => {}
                }
            }
            KeyCode::Escape => {
                ui.clear_selection();
            }
            _ => {
                // Typing into a selected cell starts editing it.
                if let Some(c) = ui.cells.filter(|c| c.single())
                    && let Some(t) = k.text.as_ref().map(|t| t.to_string()).filter(|t| t.chars().all(|c| !c.is_control()))
                {
                    commands.queue(move |w: &mut World| start_cell_edit(w, c.table, c.head, false, Some(&t)));
                }
            }
        }
    }
}

/// Clears the text of the selected cells (one undo step).
fn clear_cells(w: &mut World, sel: CellSel) {
    let Some(doc) = w.get_resource::<ActiveDocument>() else { return };
    let Some((_, index, sheet)) = active_sheet(doc, w.resource::<DrawingUi>()) else { return };
    let Some((_, d)) = active_drawing(doc) else { return };
    let Some(t) = find_table(d, index, sel.table).cloned() else { return };
    let ((r0, c0), (r1, c1)) = sel.range();
    let mut m = t.clone();
    for r in r0..=r1.min(t.n_rows().saturating_sub(1)) {
        for c in c0..=c1.min(t.n_cols().saturating_sub(1)) {
            if !t.covered(r, c) {
                m.cells[r][c] = RichText::default();
            }
        }
    }
    if m != t {
        let m = fit(w, m);
        edit_drawing(w, DrawingOp::SetTable { sheet, table: m, label: "Clear cells".into() });
    }
}

/// Keeps keyboard focus on the sheet editor while text is typed there (after a click on the
/// toolbar or the sheet), and lets it go when typing ends.
fn keep_editor_focus(
    ui: Res<NotesUi>,
    editor: Option<Res<EditorFocus>>,
    mut focus: ResMut<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_menus: Query<(), With<cadrs_ui::menu::MenuDismissLayer>>,
) {
    let Some(editor) = editor else {
        return;
    };
    let current = focus.get();
    if ui.edit.is_some() {
        let other_field = current.is_some_and(|e| e != editor.0 && q_fields.contains(e));
        if current != Some(editor.0) && !other_field && q_menus.is_empty() {
            focus.set(editor.0, bevy::input_focus::FocusCause::Navigated);
        }
    } else if current == Some(editor.0) {
        focus.clear();
    }
}

/// A table as stored: its rows grown to fit their text.
pub fn fit(w: &World, t: Table) -> Table {
    match field_sources(w) {
        Some((r, d)) => cadrs_drawing::table::fit_rows(&t, &FieldContext { reference: &r, drawing: &d }),
        None => t,
    }
}

/// Deletes notes and tables.
pub fn delete_items(w: &mut World, items: &[Item]) {
    let Some(sheet) = w.get_resource::<ActiveDocument>().and_then(|doc| active_sheet(doc, w.resource::<DrawingUi>()).map(|s| s.2)) else {
        return;
    };
    let notes = items.iter().filter_map(|i| if let Item::Note(n) = i { Some(*n) } else { None }).collect();
    let tables = items.iter().filter_map(|i| if let Item::Table(t) = i { Some(*t) } else { None }).collect();
    edit_drawing(w, DrawingOp::DeleteSheetItems { sheet, notes, tables });
}

// ---------------------------------------------------------------------------------------------
// Menus

#[derive(Component, Clone, Copy)]
struct ItemMenuFor(Item, Option<Cell>);

/// Right-click on a note or table: its menu. Returns false when there is none under `pos`.
pub fn open_item_menu(world: &mut World, pos: Vec2) -> bool {
    let rect = *world.resource::<ViewportRect>();
    let hit = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let (_, view) = current_view(doc, ui)?;
        let p = screen_to_sheet(view, sheet_area(&rect, ui), pos);
        item_at(world.resource::<NoteScene>(), p, 4.0 / view.ppm as f64)
    })();
    let Some((item, cell)) = hit else {
        return false;
    };
    commit_edit(world);
    {
        // A right-click selects the table as a whole, unless its cells are selected already
        // (the table and cell selections don't mix); Edit cell… edits the cell clicked.
        let mut ui = world.resource_mut::<NotesUi>();
        let keep_cells = matches!((item, ui.cells), (Item::Table(t), Some(s)) if s.table == t);
        ui.selected = vec![item];
        if !keep_cells {
            ui.cells = None;
        }
    }
    let theme = world.resource::<Theme>().clone();
    let mut menu = Menu::new("note-context-menu").min_width(180.0);
    match item {
        Item::Note(_) => {
            menu = menu
                .item(MenuItem::new("note-menu-edit", "Edit…").icon("edit"))
                .item(MenuItem::new("note-menu-add-leader", "Add leader"))
        }
        Item::Table(id) => {
            menu = menu
                .item(MenuItem::new("table-menu-edit-cell", "Edit cell…").icon("edit").disabled(cell.is_none()))
                .item(MenuItem::new("table-menu-properties", "Table properties…").icon("properties"));
            // A BOM table's own properties (P3C.5, D11.3).
            let is_bom = world
                .get_resource::<ActiveDocument>()
                .and_then(|doc| {
                    let (eid, d) = active_drawing(doc)?;
                    find_table(d, world.resource::<DrawingUi>().sheet_index(eid, d), id).map(|t| t.bom.is_some())
                })
                .unwrap_or(false);
            if is_bom {
                menu = menu.item(MenuItem::new("table-menu-bom-properties", "BOM Table properties…").icon("bill-of-materials"));
            }
        }
    }
    menu = menu.separator().item(MenuItem::new("note-menu-delete", "Delete").icon("delete"));
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, pos, menu.build(&theme));
    commands.entity(anchor).insert((ItemMenuFor(item, cell), DespawnOnExit(AppState::Document)));
    world.flush();
    true
}

fn on_item_menu(ev: On<MenuAction>, q: Query<&ItemMenuFor>, mut commands: Commands) {
    let Ok(t) = q.get(ev.entity).copied() else {
        return;
    };
    let item = ev.item.clone();
    commands.queue(move |w: &mut World| match (item.as_str(), t.0) {
        ("note-menu-edit", Item::Note(id)) => start_note_edit(w, id),
        ("note-menu-add-leader", Item::Note(id)) => {
            super::annotations::start_tool(w, AnnTool::AddLeader(id));
            w.resource_mut::<NotesUi>().selected = vec![Item::Note(id)];
        }
        ("table-menu-edit-cell", Item::Table(id)) => {
            let c = t.1.or_else(|| w.resource::<NotesUi>().cells.filter(|c| c.table == id).map(|c| c.head));
            if let Some(c) = c {
                start_cell_edit(w, id, c, true, None);
            }
        }
        ("table-menu-properties", Item::Table(id)) => super::note_bar::open_table_properties(w, id),
        ("table-menu-bom-properties", Item::Table(id)) => super::bom_tools::open_bom_properties(w, id),
        ("note-menu-delete", i) => {
            w.resource_mut::<NotesUi>().clear_selection();
            delete_items(w, &[i]);
        }
        _ => {}
    });
}

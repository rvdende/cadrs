//! The **Variable Studio** tab (Onshape's Variable Studio, `VariableStudio/variable_studio.htm`
//! and the `variables-and-expressions` course; the model is [`cadrs_core::variable_studio`]).
//!
//! - **The table** fills the tab (no feature list, no 3D view, no side panel): a collapsible
//!   section per table (`cadrs_ui::Collapsible`) with its name and how many variables it has,
//!   over a `cadrs_ui` table whose columns can be dragged wider (all tables keep the widths).
//!   The studio's own table has **Name**, **Type**, **Value**, **Evaluates to** and
//!   **Description**; Name, Value and Description are edited in place
//!   (`variable-studio-row-<n>-name`, `-value`, `-description`, `cadrs_ui::GridCell`). Enter or
//!   Tab commits a cell, Esc puts it back; each commit is one undo step, and every Part Studio
//!   and Assembly naming the variable follows (Assemblies re-solved off the UI thread, their
//!   placements joining the same step; a second edit of the definitions while that solve runs
//!   waits for it, so each edit undoes with its own placements).
//! - **Adding a variable**: type a name in the last row (`variable-studio-new-name`, "Add
//!   variable"; a name takes letters, digits and `_`); the new row is a Length of 0 in the
//!   workspace unit and the caret moves to its Value. The Type cell (`-type`) is a menu:
//!   Length, Angle, Number, Any (a plain value takes the new type's unit).
//! - **Errors** show in red in Evaluates to (cut with "…", the whole text on hover): a name
//!   used before it is defined, a cycle, a duplicate name, a row in error, a bad expression.
//!   A name another available studio also defines shows which one Part Studios use.
//! - **Order and deleting**: drag a row by its handle (`-handle`); click the handle to pick
//!   the row, then Delete deletes it; or right-click it for **Move up**, **Move down**,
//!   **Insert variable above**, **Duplicate** and **Delete** (also the row's ✕).
//! - **Insert Variable Studio** (the toolbar's `variable-studio-insert`): another Variable
//!   Studio's table, whole or some of its variables, from this document (followed live) or a
//!   version of another (a frozen copy, labelled "Document › Studio V1"). Inserted tables show
//!   read-only above the own table with a ✕ that removes them; a version's says when its
//!   document has a newer one and has **Change version…**, which puts another version in its
//!   place. The own rows can name the inserted variables.
//! - **⋯** on the studio's section: **Collapse all tables**, **Expand all tables**.
//! - **The bottom bar**: the checkbox **Make variables available to Part Studios and
//!   Assemblies** (`variable-studio-available`; uncheck it while the variables are a draft)
//!   and a summary ("4 variables · 1 error · used in 2 tabs").
//! - The table is rebuilt only when the document, the tab or the table's own state changes
//!   ([`VsUi`]'s `dirty`), and the cell being edited is edited again afterwards.
//!
//! What Onshape's Variable Studio has and this doesn't (CSV From import, non-numeric Any,
//! configurations, Release) is listed in [`cadrs_core::variable_studio`].

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::Activate;
use cadrs_core::ElementId;
use cadrs_core::assembly::mate::MateFeature;
use cadrs_core::assembly::{Assembly, InstanceId, Pose};
use cadrs_core::rebuild::Build;
use cadrs_core::variable_studio::{self as vs, InsertedTable, SetVariableStudio, VariableStudio};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, Column, ContextMenuRequested, GridCell, GridCellCommit, MenuAction, TabStrip,
    TabStripSelect, TableHeader, TableRoot, TableRow, VersionGraphSelect, toolbar_separator,
};
use cadrs_vars::studio::{self, StudioVariable};
use cadrs_vars::{VariableFeature, VariableType};
use uuid::Uuid;

use crate::linked::{self, Browse, GraphPick, Owner, VersionTarget};
use crate::viewport::{ActiveKind, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct VariableStudioUiPlugin;

impl Plugin for VariableStudioUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VsUi>()
            .add_systems(
                Update,
                (
                    mark_dirty,
                    row_keys,
                    sync_view,
                    restore_focus,
                    sync_drop_marker,
                    read_insert_search,
                    sync_insert_dialog,
                    sync_insert_list,
                    finish_studio_solve,
                )
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_systems(OnExit(AppState::Document), |world: &mut World| {
                *world.resource_mut::<VsUi>() = VsUi::default();
                world.remove_resource::<InsertSession>();
                world.remove_resource::<StudioSolve>();
            })
            .add_observer(on_cell_commit)
            .add_observer(on_button)
            .add_observer(on_menu_action)
            .add_observer(on_row_menu)
            .add_observer(on_available)
            .add_observer(on_drag_start)
            .add_observer(on_drag)
            .add_observer(on_drag_end)
            .add_observer(on_insert_tab)
            .add_observer(on_insert_graph_pick)
            .add_observer(on_insert_check)
            .add_observer(on_insert_close)
            .add_observer(on_handle_click)
            .add_observer(on_section_toggled)
            .add_observer(on_column_resize);
    }
}

/// A row's height in the table (px).
const ROW_H: f32 = 30.0;

/// Which table of a studio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TableKey {
    Own,
    Inserted(Uuid),
}

/// The table view's own state (this session).
#[derive(Resource, Debug, Default)]
pub struct VsUi {
    /// Collapsed tables, by studio.
    collapsed: HashSet<(ElementId, TableKey)>,
    /// A cell to edit once the table is rebuilt (the next cell after a commit).
    pending_focus: Option<CellKey>,
    /// A row being dragged.
    drag: Option<RowDrag>,
    /// The row picked by its handle (Delete deletes it).
    selected: Option<Uuid>,
    /// Column widths dragged to (every table of every studio shares them).
    widths: Vec<(String, f32)>,
    /// Something the table shows changed outside the document (set by `mark_dirty`).
    dirty: bool,
}

#[derive(Debug, Clone, Copy)]
struct RowDrag {
    studio: ElementId,
    row: Uuid,
    /// Where it would go (its index among the rows without it).
    to: usize,
    moved: bool,
}

/// Which row of the own table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RowKey {
    Row(Uuid),
    /// The last, empty row ("Add variable").
    New,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Col {
    Name,
    Value,
    Description,
}

/// An editable cell: its row and column.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CellKey {
    row: RowKey,
    col: Col,
}

/// The overlay over the viewport.
#[derive(Component)]
struct VsView;

/// The scrolling content of the overlay.
#[derive(Component)]
struct VsContent;

/// The bottom bar (its checkbox and summary).
#[derive(Component)]
struct VsFooter;

/// An own row (right-click: its menu).
#[derive(Component, Debug, Clone, Copy)]
struct RowLine {
    row: Uuid,
    index: usize,
}

/// The line shown where a dragged row would land: above this row, or (`None`) at the end.
#[derive(Component, Debug, Clone, Copy)]
struct DropLine(Option<Uuid>);

/// A row's drag handle (a click picks the row).
#[derive(Component, Debug, Clone, Copy)]
struct DragHandle(Uuid);

/// A row's Type cell (a menu).
#[derive(Component, Debug, Clone, Copy)]
struct TypeButton(Uuid);

/// A row's ✕.
#[derive(Component, Debug, Clone, Copy)]
struct DeleteRow(Uuid);

/// A Type menu's anchor: the row it sets.
#[derive(Component, Debug, Clone, Copy)]
struct TypeMenu(Uuid);

/// The ⋯ menu's anchor.
#[derive(Component)]
struct MoreMenu;

/// A row's context menu anchor.
#[derive(Component, Debug, Clone, Copy)]
struct RowMenu(Uuid);

/// A table's section (its open state is remembered).
#[derive(Component, Debug, Clone, Copy)]
struct Section(TableKey);

/// An inserted table's ✕.
#[derive(Component, Debug, Clone, Copy)]
struct RemoveTable(Uuid);

/// An inserted version's Change version button.
#[derive(Component, Debug, Clone, Copy)]
struct ChangeVersion(Uuid);

/// The ⋯ button of the studio's section.
#[derive(Component)]
struct MoreButton;

/// The availability checkbox.
#[derive(Component)]
struct AvailableBox;

// ---------------------------------------------------------------------------------------------
// The toolbar

/// The Variable Studio's toolbar: undo, redo and Insert Variable Studio.
pub fn toolbar(tb: &mut ChildSpawnerCommands, t: &Theme) {
    crate::document::undo_redo(tb, t);
    tb.spawn(toolbar_separator(t));
    tb.spawn(
        ToolButton::new("variable-studio-insert", "file-import")
            .label("Insert Variable Studio")
            .tooltip("Insert the variables of another Variable Studio, of this document or a version of another")
            .build(t),
    );
}

// ---------------------------------------------------------------------------------------------
// What the table shows

#[derive(Debug, Clone, PartialEq)]
struct RowView {
    id: Uuid,
    name: String,
    var_type: VariableType,
    expr: String,
    /// What it evaluates to, or why it doesn't.
    result: Result<String, String>,
    /// Another available studio defines the same name (which one Part Studios use).
    note: Option<String>,
    description: String,
}

#[derive(Debug, Clone, PartialEq)]
struct InsertedView {
    id: Uuid,
    label: String,
    live: bool,
    error: Option<String>,
    /// A version: the newer version its document has, if any.
    newer: Option<String>,
    collapsed: bool,
    rows: Vec<InsertedRow>,
}

/// A variable of an inserted table, as its read-only row shows it.
#[derive(Debug, Clone, PartialEq)]
struct InsertedRow {
    name: String,
    var_type: VariableType,
    expr: String,
    /// Its value with its unit.
    value: String,
    description: String,
}

#[derive(Debug, Clone, PartialEq)]
struct View {
    studio: ElementId,
    name: String,
    available: bool,
    inserted: Vec<InsertedView>,
    rows: Vec<RowView>,
    collapsed: bool,
    errors: usize,
    /// How many Part Studios and Assemblies name a variable of it.
    users: usize,
    selected: Option<Uuid>,
    widths: Vec<(String, f32)>,
}

/// The newest version of the document an inserted version came from, if newer than it.
fn newer_version(world: &mut World, t: &InsertedTable) -> Option<String> {
    let d = t.source.document?;
    let at = t.source.version_id()?;
    let latest = linked::resolver(world).0.latest(d)?;
    (latest.id() != at).then(|| latest.name().to_string())
}

fn view(world: &mut World) -> Option<View> {
    let (id, s) = active_studio(world)?;
    let newer: Vec<Option<String>> = s
        .inserted
        .iter()
        .map(|t| (!t.is_live()).then(|| newer_version(world, t)).flatten())
        .collect();
    let doc = world.get_resource::<ActiveDocument>()?;
    let el = doc.doc.element(id)?;
    let ui = world.resource::<VsUi>();
    let units = doc.doc.units;
    let eval = vs::evaluate(&doc.doc, el.id)?;
    let conflicts = vs::conflicts(&doc.doc);
    let tab_name = |e: ElementId| {
        doc.doc
            .element(e)
            .map_or_else(String::new, |e| e.name.clone())
    };
    let inserted = s
        .inserted
        .iter()
        .zip(&eval.inserted)
        .zip(newer)
        .map(|((t, e), newer)| InsertedView {
            id: t.id,
            label: e.label.clone(),
            live: t.is_live(),
            error: e.error.clone(),
            newer,
            collapsed: ui.collapsed.contains(&(el.id, TableKey::Inserted(t.id))),
            rows: e
                .vars
                .iter()
                .map(|x| InsertedRow {
                    name: x.var.name.clone(),
                    var_type: x.var.kind.var_type(),
                    expr: x.var.expr.clone(),
                    value: x.var.display(&units),
                    description: x.var.description.clone(),
                })
                .collect(),
        })
        .collect();
    let rows = s
        .variables
        .iter()
        .map(|r| {
            let result = match eval.table.row(r.id) {
                Some(x) => match (&x.error, x.value) {
                    (Some(e), _) => Err(e.clone()),
                    (None, Some(v)) => Ok(cadrs_vars::display(
                        v.value,
                        cadrs_vars::VariableKind::of(v.quantity),
                        &units,
                    )),
                    (None, None) => Ok(String::new()),
                },
                None => Ok(String::new()),
            };
            let note = conflicts
                .iter()
                .find(|(n, ..)| *n == r.var.name)
                .filter(|_| s.available)
                .map(|(n, winner, others)| {
                    if *winner == el.id {
                        let others: Vec<String> = others.iter().map(|o| tab_name(*o)).collect();
                        format!("#{n} is also in {}: this one is used", others.join(", "))
                    } else {
                        format!(
                            "Part Studios use #{n} of {} (a later tab)",
                            tab_name(*winner)
                        )
                    }
                });
            RowView {
                id: r.id,
                name: r.var.name.clone(),
                var_type: r.var.var_type,
                expr: r.var.expr.clone(),
                result,
                note,
                description: r.var.description.clone(),
            }
        })
        .collect();
    let users = doc
        .doc
        .elements
        .iter()
        .filter(|e| {
            e.studio_variables
                .iter()
                .any(|v| v.used && v.studio == el.id)
        })
        .count();
    Some(View {
        studio: el.id,
        name: el.name.clone(),
        available: s.available,
        inserted,
        rows,
        collapsed: ui.collapsed.contains(&(el.id, TableKey::Own)),
        errors: eval.table.errors() + eval.inserted.iter().filter(|i| i.error.is_some()).count(),
        users,
        selected: ui.selected,
        widths: ui.widths.clone(),
    })
}

/// Marks the table out of date when what it shows may have changed: the document, the tab,
/// the table's own state, or the library (a newer version of an inserted one).
fn mark_dirty(
    doc: Option<Res<ActiveDocument>>,
    kind: Res<ActiveKind>,
    library: Res<linked::LibrarySnapshot>,
    mut ui: ResMut<VsUi>,
) {
    if doc.is_some_and(|d| d.is_changed()) || kind.is_changed() || library.is_changed() {
        ui.dirty = true;
    }
}

/// Spawns the overlay on the first Variable Studio tab, shows or hides it, and rebuilds the
/// table when what it shows changes (the cell being edited is edited again afterwards).
fn sync_view(
    world: &mut World,
    q: &mut QueryState<(Entity, &mut Node), With<VsView>>,
    mut last: Local<Option<View>>,
) {
    let show = *world.resource::<ActiveKind>() == ActiveKind::VariableStudio;
    let overlay = q.iter(world).next().map(|(e, _)| e);
    if let Some(e) = overlay {
        let d = if show { Display::Flex } else { Display::None };
        if let Some(mut n) = world.get_mut::<Node>(e)
            && n.display != d
        {
            n.display = d;
        }
    }
    if !show {
        *last = None;
        return;
    }
    // Only when something it shows may have changed (not every frame).
    if overlay.is_some()
        && last.is_some()
        && !std::mem::take(&mut world.resource_mut::<VsUi>().dirty)
    {
        return;
    }
    world.resource_mut::<VsUi>().dirty = false;
    let Some(v) = view(world) else { return };
    let theme = world.resource::<Theme>().clone();
    if overlay.is_none() {
        let mut qa = world.query_filtered::<Entity, With<ViewportArea>>();
        let Some(area) = qa.iter(world).next() else {
            return;
        };
        let e = world.spawn(overlay_bundle(&theme)).id();
        world.entity_mut(area).add_child(e);
        *last = None;
    }
    if last.as_ref() == Some(&v) {
        return;
    }
    let mut qc = world.query_filtered::<(Entity, &ScrollPosition), With<VsContent>>();
    let Some((content, scroll)) = qc.iter(world).next().map(|(e, s)| (e, s.clone())) else {
        return;
    };
    // The cell being edited is edited again once rebuilt (a commit's own rebuild moves on to
    // the cell it set in `pending_focus`).
    let focused = world
        .resource::<InputFocus>()
        .get()
        .and_then(|f| world.get::<cadrs_ui::GridCellEdit>(f).map(|g| g.0))
        .and_then(|root| world.get::<CellKey>(root).copied());
    if world.resource::<VsUi>().pending_focus.is_none() {
        world.resource_mut::<VsUi>().pending_focus = focused;
    }
    let same_studio = last.as_ref().is_some_and(|l| l.studio == v.studio);
    world.entity_mut(content).despawn_children();
    let mut qf = world.query_filtered::<Entity, With<VsFooter>>();
    if let Some(f) = qf.iter(world).next() {
        world.entity_mut(f).despawn_children();
        let vv = v.clone();
        let t = theme.clone();
        world
            .commands()
            .entity(f)
            .with_children(|p| footer(p, &t, &vv));
    }
    let vv = v.clone();
    world
        .commands()
        .entity(content)
        .with_children(|p| tables(p, &theme, &vv));
    world.entity_mut(content).insert(if same_studio {
        scroll
    } else {
        ScrollPosition(Vec2::ZERO)
    });
    world.flush();
    *last = Some(v);
}

fn overlay_bundle(t: &Theme) -> impl Bundle {
    (
        Name::new("variable-studio-view"),
        VsView,
        DespawnOnExit(AppState::Document),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            right: Val::Px(0.0),
            bottom: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BackgroundColor(t.background),
        ZIndex(0),
        children![
            (
                Name::new("variable-studio-content"),
                VsContent,
                // Column resizes reach every table's cells.
                TableRoot,
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(14.0),
                    padding: UiRect::new(
                        Val::Px(20.0),
                        Val::Px(20.0),
                        Val::Px(16.0),
                        Val::Px(24.0)
                    ),
                    overflow: Overflow::scroll(),
                    ..default()
                },
                ScrollPosition::default(),
            ),
            (
                Name::new("variable-studio-footer"),
                VsFooter,
                Node {
                    height: Val::Px(40.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(16.0),
                    padding: UiRect::horizontal(Val::Px(16.0)),
                    border: UiRect::top(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::all(t.panel_border),
                BackgroundColor(t.background),
            ),
        ],
    )
}

fn footer(p: &mut ChildSpawnerCommands, t: &Theme, v: &View) {
    p.spawn((
        AvailableBox,
        Checkbox::new("variable-studio-available")
            .label("Make variables available to Part Studios and Assemblies")
            .checked(v.available)
            .build(t),
        Tooltip::new(
            "Unchecked, this studio's variables are a draft: no Part Studio or Assembly sees them",
        ),
    ));
    p.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    let n: usize = v.rows.len() + v.inserted.iter().map(|i| i.rows.len()).sum::<usize>();
    let mut summary = match n {
        1 => "1 variable".to_string(),
        n => format!("{n} variables"),
    };
    match v.errors {
        0 => {}
        1 => summary.push_str(" · 1 error"),
        e => summary.push_str(&format!(" · {e} errors")),
    }
    if v.available {
        match v.users {
            0 => {}
            1 => summary.push_str(" · used in 1 tab"),
            u => summary.push_str(&format!(" · used in {u} tabs")),
        }
    }
    p.spawn((
        Name::new("variable-studio-summary"),
        t.text(
            summary,
            t.font_sm,
            FontWeight::NORMAL,
            if v.errors > 0 {
                t.feature_error
            } else {
                t.muted_foreground
            },
        ),
    ));
}

/// The table's columns, with the widths dragged to.
fn columns(widths: &[(String, f32)]) -> Vec<Column> {
    let w = |key: &str, default: f32| {
        widths
            .iter()
            .find(|(k, _)| k == key)
            .map_or(default, |(_, w)| *w)
    };
    vec![
        Column::new("handle", "").width(24.0).min_width(24.0),
        Column::new("name", "Name")
            .width(w("name", 190.0))
            .min_width(80.0),
        Column::new("type", "Type")
            .width(w("type", 110.0))
            .min_width(80.0),
        Column::new("value", "Value").grow(2.0).min_width(100.0),
        Column::new("result", "Evaluates to")
            .width(w("result", 220.0))
            .min_width(90.0),
        Column::new("description", "Description")
            .grow(3.0)
            .min_width(80.0),
        Column::new("delete", "").width(36.0).min_width(36.0),
    ]
}

/// A table's section: a collapsible band with its title and `extra` header items, then (open)
/// the column header and the rows.
#[allow(clippy::too_many_arguments)]
fn section(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    name: String,
    title: String,
    icon: &'static str,
    key: TableKey,
    collapsed: bool,
    extra: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static,
    body: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static,
) {
    p.spawn((
        Name::new(format!("{name}-card")),
        Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BorderColor::all(t.border),
        BackgroundColor(t.input_background),
    ))
    .with_children(|s| {
        s.spawn((
            Section(key),
            Collapsible::new(name, title)
                .icon(icon)
                .open(!collapsed)
                .header_height(38.0)
                .header_background(t.sidebar)
                .flush()
                .trailing(extra)
                .content(body)
                .build(t),
        ));
    });
}

fn count_text(t: &Theme, n: usize) -> impl Bundle {
    t.text(
        format!("{n} {}", if n == 1 { "variable" } else { "variables" }),
        t.font_sm,
        FontWeight::NORMAL,
        t.muted_foreground,
    )
}

/// A plain text cell, cut with "…" (the whole text in a tooltip) where it doesn't fit.
fn text_cell(t: &Theme, name: Option<String>, text: String, color: Color) -> impl Bundle {
    (
        Name::new(name.unwrap_or_default()),
        t.text(text.clone(), t.font_base, FontWeight::NORMAL, color),
        cadrs_ui::ellipsis::Ellipsis::default().with_tooltip(),
        cadrs_ui::ellipsis::Ellipsis::node(),
    )
}

fn row_name(i: usize) -> String {
    format!("variable-studio-row-{}", i + 1)
}

fn tables(p: &mut ChildSpawnerCommands, t: &Theme, v: &View) {
    let cols = columns(&v.widths);
    for (n, ins) in v.inserted.iter().enumerate() {
        let name = format!("variable-studio-inserted-{}", n + 1);
        let ins = ins.clone();
        let (te, tb) = (t.clone(), t.clone());
        let (ne, nb) = (name.clone(), name.clone());
        let cols = cols.clone();
        let rows = ins.rows.clone();
        section(
            p,
            t,
            name,
            ins.label.clone(),
            "link",
            TableKey::Inserted(ins.id),
            ins.collapsed,
            move |h| {
                let t = &te;
                h.spawn(count_text(t, ins.rows.len()));
                h.spawn(t.text(
                    match (&ins.newer, ins.live) {
                        (_, true) => "Follows this document".to_string(),
                        (Some(v), false) => format!("Newer version: {v}"),
                        (None, false) => "Version: stays as inserted".to_string(),
                    },
                    t.font_sm,
                    FontWeight::NORMAL,
                    if ins.newer.is_some() {
                        t.link
                    } else {
                        t.subtle_foreground
                    },
                ));
                if let Some(e) = ins.error.clone() {
                    h.spawn((
                        Name::new(format!("{ne}-error")),
                        t.text(e, t.font_sm, FontWeight::MEDIUM, t.feature_error),
                    ));
                }
                h.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                if !ins.live {
                    h.spawn((
                        ChangeVersion(ins.id),
                        cadrs_ui::Button::new(format!("{ne}-change-version"))
                            .label("Change version…")
                            .icon("versions")
                            .ghost()
                            .small()
                            .tooltip("Insert another version of this Variable Studio in its place")
                            .build(t),
                    ));
                }
                h.spawn((
                    RemoveTable(ins.id),
                    IconButton::new(format!("{ne}-remove"), "close")
                        .ghost()
                        .small()
                        .tooltip("Remove this table from the Variable Studio")
                        .build(t),
                ));
            },
            move |b| {
                let t = &tb;
                b.spawn(
                    TableHeader::new(format!("{nb}-columns"), cols.clone())
                        .grid()
                        .resizable()
                        .height(26.0)
                        .build(t),
                );
                for (i, r) in rows.into_iter().enumerate() {
                    b.spawn(
                        TableRow::new(format!("{nb}-row-{}", i + 1), &cols)
                            .grid()
                            .height(ROW_H)
                            .cell(|_| {})
                            .cell(spawned(text_cell(t, None, r.name, t.foreground)))
                            .cell(spawned(text_cell(
                                t,
                                None,
                                r.var_type.label().into(),
                                t.muted_foreground,
                            )))
                            .cell(spawned(text_cell(t, None, r.expr, t.muted_foreground)))
                            .cell(spawned(text_cell(t, None, r.value, t.foreground)))
                            .cell(spawned(text_cell(
                                t,
                                None,
                                r.description,
                                t.muted_foreground,
                            )))
                            .build(t),
                    );
                }
            },
        );
    }
    // The studio's own table.
    let rows = v.rows.clone();
    let count = rows.len();
    let selected = v.selected;
    let (te, tb) = (t.clone(), t.clone());
    section(
        p,
        t,
        "variable-studio-table".into(),
        v.name.clone(),
        "variables",
        TableKey::Own,
        v.collapsed,
        move |h| {
            let t = &te;
            h.spawn(count_text(t, count));
            h.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            h.spawn((
                MoreButton,
                IconButton::new("variable-studio-more", "more-horizontal")
                    .ghost()
                    .small()
                    .tooltip("More")
                    .build(t),
            ));
        },
        move |b| {
            let t = &tb;
            b.spawn(
                TableHeader::new("variable-studio-table-columns", cols.clone())
                    .grid()
                    .resizable()
                    .height(26.0)
                    .build(t),
            );
            for (i, r) in rows.into_iter().enumerate() {
                own_row(b, t, &cols, i, r, selected);
            }
            new_row(b, t, &cols);
        },
    );
    if v.rows.is_empty() && v.inserted.is_empty() {
        p.spawn((
            Name::new("variable-studio-empty"),
            t.text(
                "Type a name in the table's last row to add a variable. Every Part Studio and Assembly of this document can then use it in any value field as #name, and follows it when it changes.",
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
            Node { max_width: Val::Px(560.0), margin: UiRect::left(Val::Px(4.0)), ..default() },
        ))
        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
    }
}

/// A name cell takes ASCII letters, digits and `_` (Onshape's names are alphanumeric; the `#`
/// is only how expressions name it).
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// The marker a dragged row's landing place shows (a line at a row's top).
fn drop_line(t: &Theme, at: Option<Uuid>) -> impl Bundle {
    (
        DropLine(at),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            right: Val::Px(0.0),
            top: Val::Px(-1.0),
            height: Val::Px(2.0),
            ..default()
        },
        BackgroundColor(t.input_border_focus),
        Visibility::Hidden,
        Pickable::IGNORE,
    )
}

/// A table cell that spawns `b`.
fn spawned(b: impl Bundle) -> impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static {
    move |c| {
        c.spawn(b);
    }
}

fn own_row(
    b: &mut ChildSpawner,
    t: &Theme,
    cols: &[Column],
    i: usize,
    r: RowView,
    selected: Option<Uuid>,
) {
    let name = row_name(i);
    let id = r.id;
    let cell = |col: Col| CellKey {
        row: RowKey::Row(id),
        col,
    };
    let handle = (
        Name::new(format!("{name}-handle")),
        DragHandle(id),
        Node {
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Tooltip::new("Drag to reorder; click to pick (Delete deletes it)"),
        children![(
            cadrs_ui::icon("drag-handle", 12.0, t.subtle_foreground),
            Pickable::IGNORE
        )],
    );
    let name_cell = (
        cell(Col::Name),
        GridCell::new(format!("{name}-name"))
            .text(r.name)
            .grow(1.0)
            .font(t.font_base, FontWeight::MEDIUM)
            .accept(name_char)
            .build(t),
    );
    let type_cell = (
        TypeButton(id),
        cadrs_ui::Button::new(format!("{name}-type"))
            .label(r.var_type.label())
            .ghost()
            .small()
            .dropdown_caret()
            .width(Val::Percent(100.0))
            .build(t),
    );
    let value_cell = (
        cell(Col::Value),
        GridCell::new(format!("{name}-value"))
            .text(r.expr)
            .grow(1.0)
            .error(r.result.is_err())
            .build(t),
    );
    let (result, color) = match (r.result, r.note) {
        (Err(e), _) => (e, t.feature_error),
        (Ok(v), Some(n)) => (format!("{v}  ⚠ {n}"), t.warning_icon),
        (Ok(v), None) => (v, t.foreground),
    };
    let result_cell = text_cell(t, Some(format!("{name}-result")), result, color);
    let description_cell = (
        cell(Col::Description),
        GridCell::new(format!("{name}-description"))
            .text(r.description)
            .grow(1.0)
            .build(t),
    );
    let delete = (
        DeleteRow(id),
        IconButton::new(format!("{name}-delete"), "close")
            .ghost()
            .small()
            .tooltip("Delete this variable")
            .build(t),
    );
    b.spawn((
        RowLine { row: id, index: i },
        TableRow::new(name, cols)
            .grid()
            .height(ROW_H)
            .selected(selected == Some(id))
            .cell(spawned(handle))
            .cell(spawned(name_cell))
            .cell(spawned(type_cell))
            .cell(spawned(value_cell))
            .cell(spawned(result_cell))
            .cell(spawned(description_cell))
            .cell(spawned(delete))
            .build(t),
    ))
    .with_children(|r| {
        r.spawn(drop_line(t, Some(id)));
    });
}

fn new_row(b: &mut ChildSpawner, t: &Theme, cols: &[Column]) {
    let plus = (
        Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::Center,
            ..default()
        },
        children![(
            cadrs_ui::icon("plus", 11.0, t.subtle_foreground),
            Pickable::IGNORE
        )],
    );
    let name_cell = (
        CellKey {
            row: RowKey::New,
            col: Col::Name,
        },
        GridCell::new("variable-studio-new-name")
            .placeholder("Add variable")
            .grow(1.0)
            .accept(name_char)
            .build(t),
    );
    b.spawn(
        TableRow::new("variable-studio-new", cols)
            .height(ROW_H)
            .cell(spawned(plus))
            .cell(spawned(name_cell))
            .build(t),
    )
    .with_children(|r| {
        r.spawn(drop_line(t, None));
    });
}

// ---------------------------------------------------------------------------------------------
// Editing

/// The active Variable Studio: its id and contents.
fn active_studio(world: &World) -> Option<(ElementId, VariableStudio)> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let el = doc.active_element()?;
    Some((el.id, vs::studio_of(el)?.clone()))
}

/// Replaces the active Variable Studio's contents: one undo step. Assemblies whose mate offsets
/// name a changed variable are solved again afterwards, off the UI thread ([`StudioSolve`]),
/// and their placements join the same undo step.
fn set_studio(world: &mut World, element: ElementId, studio: VariableStudio, label: &str) -> bool {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return false;
    };
    let cmd = SetVariableStudio::new(&doc.doc, element, studio, label);
    // An edit that may move mates while the last edit's solve runs: that one finishes first, so
    // each step keeps its own placements. (Only then: a description, or a studio no tab uses,
    // goes on at once, and the solve still running places its result after it.)
    let current = doc.doc.element(element).and_then(vs::studio_of);
    if cmd.reaches_others && current.is_some_and(|s| !same_definitions(s, &cmd.studio)) {
        finish_studio_solve_now(world);
    }
    let doc = world.resource::<ActiveDocument>();
    let mates_before: Vec<(ElementId, Vec<MateFeature>)> = doc
        .doc
        .elements
        .iter()
        .filter_map(|e| Some((e.id, e.assembly_model()?.mates.clone())))
        .collect();
    let revision = doc.history.revision();
    let mut doc = world.resource_mut::<ActiveDocument>();
    if let Err(e) = doc.execute(&cmd) {
        linked::error_toast(world, e.to_string());
        return false;
    }
    let doc = world.resource::<ActiveDocument>();
    let assemblies: Vec<ElementId> = mates_before
        .iter()
        .filter(|(id, mates)| {
            doc.doc
                .element(*id)
                .and_then(|e| e.assembly_model())
                .is_some_and(|a| a.mates != *mates)
        })
        .map(|(id, _)| *id)
        .collect();
    if !assemblies.is_empty() && doc.history.revision() != revision {
        let solve = StudioSolve {
            studio: element,
            label: label.to_string(),
            history: doc.history.id(),
            edit: Some(doc.history.revision()),
            assemblies,
            stage: Stage::Builds,
        };
        world.insert_resource(solve);
    }
    true
}

/// Whether two studios define the same variables (they differ at most in descriptions).
fn same_definitions(a: &VariableStudio, b: &VariableStudio) -> bool {
    let bare = |s: &VariableStudio| {
        let mut s = s.clone();
        for r in &mut s.variables {
            r.var.description.clear();
        }
        s
    };
    bare(a) == bare(b)
}

type Poses = Vec<(ElementId, Vec<(InstanceId, Pose)>)>;

/// Assemblies being solved again after a Variable Studio edit moved their mate offsets, so a
/// large assembly doesn't hold the UI: the Part Studios they use are rebuilt (polled each frame
/// through [`crate::assembly::AssemblyParts`], which knows linked and inserted ones too), then
/// the solve runs on a task. What [`finish_studio_solve`] does with the result, by how the
/// document is when it is ready:
///
/// | The document                                   | The placements                        |
/// |------------------------------------------------|---------------------------------------|
/// | as the edit left it, nothing since             | join the edit's undo step             |
/// | assemblies and parts as solved, other steps since | a step of their own ("… placements") |
/// | undone to before the edit (redo stack not empty) | wait: a redo brings the solved state back |
/// | changed by another edit                        | solved again as it is now             |
/// | another workspace or document                  | dropped                               |
#[derive(Resource)]
struct StudioSolve {
    studio: ElementId,
    label: String,
    /// The history it belongs to ([`cadrs_core::History::id`]).
    history: u64,
    /// The history's revision right after the edit; `None` once anything happened since (the
    /// placements are then a step of their own).
    edit: Option<u64>,
    assemblies: Vec<ElementId>,
    stage: Stage,
}

enum Stage {
    /// Waiting for the Part Studios' rebuilds.
    Builds,
    /// Solving `inputs` on a task.
    Solving {
        inputs: Vec<SolveInput>,
        task: bevy::tasks::Task<Poses>,
    },
    /// Solved `inputs`: placed once the document is as they were.
    Solved {
        inputs: Vec<SolveInput>,
        poses: Poses,
        /// The history's revision when the document was last compared with `inputs` (`None`:
        /// not yet).
        checked: Option<u64>,
    },
}

/// What an assembly was solved with: its model, and each source studio's build.
struct SolveInput {
    id: ElementId,
    asm: Assembly,
    builds: Vec<(ElementId, Option<Arc<Build>>)>,
    solids: HashMap<InstanceId, Arc<cadrs_core::solid::Solid>>,
}

impl SolveInput {
    /// Whether `other` is the same assembly with the same builds.
    fn same(&self, other: &SolveInput) -> bool {
        let same_build = |a: &Option<Arc<Build>>, b: &Option<Arc<Build>>| match (a, b) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.id == other.id
            && self.asm == other.asm
            && self.builds.len() == other.builds.len()
            && self
                .builds
                .iter()
                .zip(&other.builds)
                .all(|((e, a), (f, b))| e == f && same_build(a, b))
    }
}

/// The solver's inputs for `assemblies` as the document is now; `None` while a Part Studio they
/// use is still rebuilding (with `wait`, it is waited for).
fn solve_inputs(
    world: &mut World,
    assemblies: &[ElementId],
    wait: bool,
) -> Option<Vec<SolveInput>> {
    let budget = (!wait).then_some(std::time::Duration::ZERO);
    world.resource_scope(|world, mut parts: Mut<crate::assembly::AssemblyParts>| {
        let doc = &world.resource::<ActiveDocument>().doc;
        let mut ready = true;
        let inputs = assemblies
            .iter()
            .filter_map(|id| {
                let asm = doc.element(*id)?.assembly_model()?;
                let mut builds = Vec::new();
                let solids = cadrs_core::assembly::occurrence_solids(doc, asm, |e| {
                    let (b, done) = parts.build_within(doc, e, budget);
                    ready &= done;
                    builds.push((e, b.clone()));
                    b
                });
                Some(SolveInput {
                    id: *id,
                    asm: asm.clone(),
                    builds,
                    solids,
                })
            })
            .collect();
        ready.then_some(inputs)
    })
}

/// Advances the [`StudioSolve`] (see there).
fn finish_studio_solve(world: &mut World) {
    step_studio_solve(world, false);
}

/// Finishes the [`StudioSolve`] now, waiting for it (an edit is about to start). One waiting
/// for a redo is dropped: the edit goes on from the undone state.
fn finish_studio_solve_now(world: &mut World) {
    while world.contains_resource::<StudioSolve>() {
        if !step_studio_solve(world, true) {
            world.remove_resource::<StudioSolve>();
        }
    }
}

/// One step of the [`StudioSolve`]; false if it is waiting: for the document to come back to
/// the state it solved, or (without `wait`) for its builds or its task.
fn step_studio_solve(world: &mut World, wait: bool) -> bool {
    let Some(mut solve) = world.remove_resource::<StudioSolve>() else {
        return true;
    };
    let (history, revision, browsing) = {
        let h = &world.resource::<ActiveDocument>().history;
        (h.id(), h.revision(), h.redo_len() > 0)
    };
    if history != solve.history {
        // Another workspace or document.
        return true;
    }
    if solve.edit.is_some_and(|r| r != revision) {
        solve.edit = None;
    }
    // Browsing the history away from the edit: nothing is solved or placed until it comes back.
    let browsing = browsing && solve.edit.is_none();
    let progressed = match std::mem::replace(&mut solve.stage, Stage::Builds) {
        Stage::Builds if browsing => false,
        Stage::Builds => match solve_inputs(world, &solve.assemblies, wait) {
            Some(inputs) => {
                let jobs: Vec<(ElementId, Assembly, HashMap<_, _>)> = {
                    let doc = &world.resource::<ActiveDocument>().doc;
                    inputs
                        .iter()
                        .map(|i| {
                            let model = cadrs_core::assembly::structure::solver_model(doc, &i.asm);
                            (i.id, model, i.solids.clone())
                        })
                        .collect()
                };
                let task = bevy::tasks::AsyncComputeTaskPool::get().spawn(async move {
                    jobs.iter()
                        .filter_map(|(id, model, solids)| {
                            let sol =
                                cadrs_core::assembly::solve(model, solids, &Default::default());
                            let poses = sol.changed(model);
                            (!poses.is_empty()).then_some((*id, poses))
                        })
                        .collect()
                });
                solve.stage = Stage::Solving { inputs, task };
                true
            }
            None => false,
        },
        Stage::Solving { inputs, task } if browsing => {
            solve.stage = Stage::Solving { inputs, task };
            false
        }
        Stage::Solving { inputs, mut task } => {
            let done = if wait {
                Some(bevy::tasks::block_on(&mut task))
            } else {
                bevy::tasks::futures::check_ready(&mut task)
            };
            match done {
                Some(poses) => {
                    solve.stage = Stage::Solved {
                        inputs,
                        poses,
                        checked: None,
                    };
                    true
                }
                None => {
                    solve.stage = Stage::Solving { inputs, task };
                    false
                }
            }
        }
        Stage::Solved {
            inputs,
            poses,
            checked,
        } if browsing || (checked == Some(revision) && !wait) => {
            // Undone (or redone with more to redo, where placing would drop the redo steps):
            // wait for the history to move again.
            solve.stage = Stage::Solved {
                inputs,
                poses,
                checked: Some(revision),
            };
            false
        }
        Stage::Solved { inputs, poses, .. } => match solve_inputs(world, &solve.assemblies, wait) {
            // Still rebuilding: look again next frame.
            None => {
                solve.stage = Stage::Solved {
                    inputs,
                    poses,
                    checked: None,
                };
                false
            }
            Some(now)
                if now.len() == inputs.len() && now.iter().zip(&inputs).all(|(a, b)| a.same(b)) =>
            {
                place_studio_poses(world, &solve, poses);
                return true;
            }
            // Changed by another edit: solve again as the document is now.
            Some(_) => {
                solve.stage = Stage::Builds;
                true
            }
        },
    };
    world.insert_resource(solve);
    progressed
}

/// Commits a finished [`StudioSolve`]'s placements: in the edit's step if nothing happened
/// since, else as a step of their own.
fn place_studio_poses(world: &mut World, solve: &StudioSolve, poses: Poses) {
    if poses.is_empty() {
        return;
    }
    let doc = world.resource::<ActiveDocument>();
    let Some(studio) = doc.doc.element(solve.studio).and_then(vs::studio_of) else {
        return;
    };
    let label = match solve.edit {
        Some(_) => solve.label.clone(),
        None => format!("{} (placements)", solve.label),
    };
    let place = SetVariableStudio {
        poses,
        ..SetVariableStudio::new(&doc.doc, solve.studio, studio.clone(), &label)
    };
    let mut doc = world.resource_mut::<ActiveDocument>();
    let revision = doc.history.revision();
    if doc.execute(&place).is_ok() && solve.edit.is_some() && doc.history.revision() != revision {
        // The edit's step and this one, counted from the top (at the step limit the oldest
        // step went, so the edit's index isn't the one it had).
        let mark = doc.history.undo_len().saturating_sub(2);
        doc.squash_since(mark, &solve.label);
    }
}

/// Applies `f` to the active Variable Studio's contents and commits them as `label`.
fn edit(
    world: &mut World,
    label: &str,
    f: impl FnOnce(&mut VariableStudio, &cadrs_sketch::units::Units) -> bool,
) -> bool {
    let Some((id, mut s)) = active_studio(world) else {
        return false;
    };
    let units = world.resource::<ActiveDocument>().doc.units;
    if !f(&mut s, &units) {
        return false;
    }
    set_studio(world, id, s, label)
}

fn on_cell_commit(ev: On<GridCellCommit>, q: Query<&CellKey>, mut commands: Commands) {
    let Ok(key) = q.get(ev.entity).copied() else {
        return;
    };
    let text = ev.text.trim().to_string();
    let enter = ev.enter;
    commands.queue(move |world: &mut World| commit(world, key, text, enter));
}

fn commit(world: &mut World, key: CellKey, text: String, enter: bool) {
    match key.row {
        RowKey::New => {
            let name = text.trim_start_matches('#').trim().to_string();
            if name.is_empty() {
                return;
            }
            let mut new_id = None;
            let label = format!("Add #{name}");
            edit(world, &label, |s, units| {
                let r = studio::new_row(&name, units);
                new_id = Some(r.id);
                s.variables.push(r);
                true
            });
            // On to the new row's value (Enter or Tab alike, as Onshape's table).
            if let Some(id) = new_id {
                world.resource_mut::<VsUi>().pending_focus = Some(CellKey {
                    row: RowKey::Row(id),
                    col: Col::Value,
                });
            }
        }
        RowKey::Row(id) => {
            let old = active_studio(world)
                .and_then(|(_, s)| s.row(id).map(|r| r.var.name.clone()))
                .unwrap_or_default();
            match key.col {
                Col::Name => {
                    let name = text.trim_start_matches('#').trim().to_string();
                    edit(world, &format!("Rename #{old} to #{name}"), |s, _| {
                        let Some(r) = s.row_mut(id) else { return false };
                        if r.var.name == name {
                            return false;
                        }
                        r.var.name = name;
                        true
                    });
                }
                Col::Value => {
                    edit(world, &format!("Edit #{old}"), |s, units| {
                        let Some(r) = s.row_mut(id) else { return false };
                        let expr = r.var.var_type.with_unit(&text, units);
                        if r.var.expr == expr {
                            return false;
                        }
                        r.var.expr = expr;
                        true
                    });
                }
                Col::Description => {
                    edit(world, &format!("Describe #{old}"), |s, _| {
                        let Some(r) = s.row_mut(id) else { return false };
                        if r.var.description == text {
                            return false;
                        }
                        r.var.description = text.clone();
                        true
                    });
                }
            }
            // Enter in a Name moves on to its Value; in a Value, to the next row's.
            if enter {
                let next = match key.col {
                    Col::Name => Some(CellKey {
                        row: RowKey::Row(id),
                        col: Col::Value,
                    }),
                    Col::Value => {
                        let after = active_studio(world).and_then(|(_, s)| {
                            let i = s.variables.iter().position(|r| r.id == id)?;
                            Some(s.variables.get(i + 1).map(|r| r.id))
                        });
                        match after {
                            Some(Some(n)) => Some(CellKey {
                                row: RowKey::Row(n),
                                col: Col::Value,
                            }),
                            Some(None) => Some(CellKey {
                                row: RowKey::New,
                                col: Col::Name,
                            }),
                            None => None,
                        }
                    }
                    Col::Description => None,
                };
                world.resource_mut::<VsUi>().pending_focus = next;
            }
        }
    }
}

/// Edits the cell the last commit asked for, once the table is rebuilt.
fn restore_focus(world: &mut World) {
    let Some(key) = world.resource::<VsUi>().pending_focus else {
        return;
    };
    let mut q = world.query::<(Entity, &CellKey)>();
    let Some(root) = q.iter(world).find(|(_, k)| **k == key).map(|(e, _)| e) else {
        return;
    };
    world.resource_mut::<VsUi>().pending_focus = None;
    cadrs_ui::focus_cell(world, root);
}

fn move_by(world: &mut World, id: Uuid, by: isize) {
    edit(world, "Move variable", |s, _| {
        let Some(i) = s.variables.iter().position(|r| r.id == id) else {
            return false;
        };
        let to = i as isize + by;
        if to < 0 || to as usize >= s.variables.len() {
            return false;
        }
        studio::move_row(&mut s.variables, id, to as usize)
    });
}

fn delete_row(world: &mut World, id: Uuid) {
    let name = active_studio(world)
        .and_then(|(_, s)| s.row(id).map(|r| r.var.name.clone()))
        .unwrap_or_default();
    edit(world, &format!("Delete #{name}"), |s, _| {
        let before = s.variables.len();
        s.variables.retain(|r| r.id != id);
        s.variables.len() != before
    });
}

#[allow(clippy::too_many_arguments)]
fn on_button(
    a: On<Activate>,
    q_name: Query<&Name>,
    q_change: Query<&ChangeVersion>,
    q_type: Query<&TypeButton>,
    q_delete: Query<&DeleteRow>,
    q_remove: Query<&RemoveTable>,
    q_more: Query<(), With<MoreButton>>,
    q_place: Query<(&UiGlobalTransform, &ComputedNode)>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let e = a.entity;
    if let Ok(ChangeVersion(table)) = q_change.get(e).copied() {
        commands.queue(move |world: &mut World| open_change_version(world, table));
        return;
    }
    if let Ok(TypeButton(row)) = q_type.get(e).copied() {
        let current = doc
            .as_ref()
            .and_then(|d| d.active_element())
            .and_then(vs::studio_of)
            .and_then(|s| s.row(row))
            .map(|r| r.var.var_type);
        let mut menu = Menu::new("variable-studio-type-menu").min_width(110.0);
        for k in VariableType::ALL {
            menu = menu.item(
                MenuItem::new(format!("type-{}", k.label().to_lowercase()), k.label())
                    .checked(current == Some(k)),
            );
        }
        // A floating menu under the cell: the table's cells clip their children.
        let anchor = open_context_menu(&mut commands, below(&q_place, e, 0.0), menu.build(&theme));
        commands.entity(anchor).insert(TypeMenu(row));
        return;
    }
    if let Ok(DeleteRow(row)) = q_delete.get(e).copied() {
        commands.queue(move |world: &mut World| delete_row(world, row));
        return;
    }
    if let Ok(RemoveTable(table)) = q_remove.get(e).copied() {
        commands.queue(move |world: &mut World| {
            edit(world, "Remove inserted table", |s, _| {
                let before = s.inserted.len();
                s.inserted.retain(|t| t.id != table);
                s.inserted.len() != before
            });
        });
        return;
    }
    if q_more.contains(e) {
        let menu = Menu::new("variable-studio-more-menu")
            .item(MenuItem::new("collapse-all", "Collapse all tables"))
            .item(MenuItem::new("expand-all", "Expand all tables"))
            .min_width(170.0)
            .build(&theme);
        let anchor = open_context_menu(&mut commands, below(&q_place, e, 170.0), menu);
        commands.entity(anchor).insert(MoreMenu);
        return;
    }
    let Ok(n) = q_name.get(e) else { return };
    match n.as_str() {
        "variable-studio-insert" => {
            commands.queue(open_insert);
        }
        "vs-insert-cancel" => commands.queue(|world: &mut World| {
            world.remove_resource::<InsertSession>();
        }),
        "vs-insert-ok" => commands.queue(insert_picked),
        "vs-insert-other-back" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::VariableStudio, |b| {
                b.opened = None;
                b.graph = false;
            });
            if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                s.source = None;
                s.only.clear();
            }
        }),
        "vs-insert-other-graph" => commands.queue(|world: &mut World| {
            linked::with_browse(world, Owner::VariableStudio, |b| b.graph = !b.graph);
        }),
        "vs-insert-other-create-version" | "vs-insert-other-no-version-create" => {
            commands.queue(|world: &mut World| {
                let opened = world
                    .get_resource::<InsertSession>()
                    .and_then(|s| s.browse.opened.as_ref().map(|o| o.document));
                if let Some(d) = opened {
                    linked::open_create_version_dialog(
                        world,
                        VersionTarget::Other(d, Owner::VariableStudio),
                    );
                }
            });
        }
        n if n.starts_with("vs-insert-studio-") => {
            let Some(src) = n
                .strip_prefix("vs-insert-studio-")
                .and_then(|x| x.parse::<usize>().ok())
            else {
                return;
            };
            commands.queue(move |world: &mut World| {
                let studios = insert_sources(world);
                let Some(s) = studios.get(src).filter(|s| s.blocked.is_none()) else {
                    return;
                };
                let id = s.id;
                if let Some(mut s) = world.get_resource_mut::<InsertSession>() {
                    s.source = Some(id);
                    s.only.clear();
                }
            });
        }
        _ => {}
    }
}

/// A plain number with a unit ("4 mm", "30 deg", "4") as the new type writes it ("4" for a
/// Number, "4 mm" for a Length, "4 deg" for an Angle); `None` for anything else (an expression
/// keeps its text, and says so if it no longer fits the type).
fn retyped(expr: &str, t: VariableType, units: &cadrs_sketch::units::Units) -> Option<String> {
    let mut words = expr.split_whitespace();
    let number = words.next()?;
    number.parse::<f64>().ok()?;
    let unit = words.next();
    if words.next().is_some() {
        return None;
    }
    if let Some(u) = unit
        && !matches!(u, "mm" | "cm" | "m" | "in" | "ft" | "yd" | "deg" | "rad")
    {
        return None;
    }
    Some(match t {
        VariableType::Any => expr.trim().to_string(),
        t => t.with_unit(number, units),
    })
}

/// Where a menu under the button `e` opens (logical px): its left edge, or its right edge less
/// `width` for a menu aligned to the button's right.
fn below(q: &Query<(&UiGlobalTransform, &ComputedNode)>, e: Entity, width: f32) -> Vec2 {
    q.get(e)
        .map(|(tf, n)| {
            let s = n.inverse_scale_factor();
            let x = if width > 0.0 {
                (tf.translation.x + n.size().x / 2.0) * s - width
            } else {
                (tf.translation.x - n.size().x / 2.0) * s
            };
            Vec2::new(x, (tf.translation.y + n.size().y / 2.0) * s + 2.0)
        })
        .unwrap_or(Vec2::new(200.0, 200.0))
}

fn on_menu_action(
    ev: On<MenuAction>,
    q_type: Query<&TypeMenu>,
    q_row: Query<&RowMenu>,
    q_more: Query<(), With<MoreMenu>>,
    mut commands: Commands,
) {
    let item = ev.item.clone();
    if let Ok(TypeMenu(row)) = q_type.get(ev.entity).copied() {
        let Some(t) = VariableType::ALL
            .into_iter()
            .find(|k| format!("type-{}", k.label().to_lowercase()) == item)
        else {
            return;
        };
        commands.queue(move |world: &mut World| {
            edit(
                world,
                &format!("Variable type {}", t.label()),
                |s, units| {
                    let Some(r) = s.row_mut(row) else {
                        return false;
                    };
                    if r.var.var_type == t {
                        return false;
                    }
                    r.var.var_type = t;
                    if let Some(e) = retyped(&r.var.expr, t, units) {
                        r.var.expr = e;
                    }
                    true
                },
            );
        });
        return;
    }
    if let Ok(RowMenu(row)) = q_row.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| match item.as_str() {
            "row-move-up" => move_by(world, row, -1),
            "row-move-down" => move_by(world, row, 1),
            "row-delete" => delete_row(world, row),
            "row-insert-above" | "row-duplicate" => {
                let above = item == "row-insert-above";
                let mut new_id = None;
                edit(
                    world,
                    if above {
                        "Insert variable"
                    } else {
                        "Duplicate variable"
                    },
                    |s, units| {
                        let Some(i) = s.variables.iter().position(|r| r.id == row) else {
                            return false;
                        };
                        let name = if above {
                            studio::fresh_name(&s.variables)
                        } else {
                            let base = s.variables[i].var.name.clone();
                            (2..)
                                .map(|n| format!("{base}_{n}"))
                                .find(|n| !s.variables.iter().any(|r| r.var.name == *n))
                                .unwrap_or_default()
                        };
                        let r = if above {
                            studio::new_row(&name, units)
                        } else {
                            StudioVariable::new(VariableFeature {
                                name,
                                ..s.variables[i].var.clone()
                            })
                        };
                        new_id = Some(r.id);
                        s.variables.insert(if above { i } else { i + 1 }, r);
                        true
                    },
                );
                if let Some(id) = new_id {
                    world.resource_mut::<VsUi>().pending_focus = Some(CellKey {
                        row: RowKey::Row(id),
                        col: Col::Name,
                    });
                }
            }
            _ => {}
        });
        return;
    }
    if q_more.contains(ev.entity) {
        commands.queue(move |world: &mut World| {
            let Some((id, s)) = active_studio(world) else {
                return;
            };
            let keys: Vec<TableKey> = s
                .inserted
                .iter()
                .map(|t| TableKey::Inserted(t.id))
                .chain([TableKey::Own])
                .collect();
            let mut ui = world.resource_mut::<VsUi>();
            ui.dirty = true;
            for k in keys {
                if item == "collapse-all" {
                    ui.collapsed.insert((id, k));
                } else {
                    ui.collapsed.remove(&(id, k));
                }
            }
        });
    }
}

fn on_row_menu(
    ev: On<ContextMenuRequested>,
    q: Query<&RowLine>,
    doc: Option<Res<ActiveDocument>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(line) = q.get(ev.entity).copied() else {
        return;
    };
    let n = doc
        .as_ref()
        .and_then(|d| d.active_element())
        .and_then(vs::studio_of)
        .map_or(0, |s| s.variables.len());
    let menu = Menu::new("variable-studio-row-menu")
        .item(
            MenuItem::new("row-move-up", "Move up")
                .icon("arrow-up")
                .disabled(line.index == 0),
        )
        .item(
            MenuItem::new("row-move-down", "Move down")
                .icon("arrow-down")
                .disabled(line.index + 1 >= n),
        )
        .separator()
        .item(MenuItem::new("row-insert-above", "Insert variable above").icon("plus"))
        .item(MenuItem::new("row-duplicate", "Duplicate").icon("copy"))
        .separator()
        .item(MenuItem::new("row-delete", "Delete").icon("delete"))
        .build(&theme);
    let anchor = open_context_menu(&mut commands, ev.position, menu);
    commands.entity(anchor).insert(RowMenu(line.row));
}

fn on_available(ev: On<CheckboxChange>, q: Query<(), With<AvailableBox>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        edit(
            world,
            if on {
                "Make variables available"
            } else {
                "Make variables unavailable"
            },
            |s, _| {
                if s.available == on {
                    return false;
                }
                s.available = on;
                true
            },
        );
    });
}

// ---------------------------------------------------------------------------------------------
// Dragging rows

fn on_drag_start(
    ev: On<PointerDragStart>,
    q: Query<&DragHandle>,
    doc: Option<Res<ActiveDocument>>,
    mut ui: ResMut<VsUi>,
) {
    let Ok(DragHandle(row)) = q.get(ev.entity).copied() else {
        return;
    };
    let Some(el) = doc.as_ref().and_then(|d| d.active_element()) else {
        return;
    };
    let Some(i) = vs::studio_of(el).and_then(|s| s.variables.iter().position(|r| r.id == row))
    else {
        return;
    };
    ui.drag = Some(RowDrag {
        studio: el.id,
        row,
        to: i,
        moved: false,
    });
}

/// The pointer over the rows: where the dragged row would go (the rows' midpoints above it).
fn on_drag(
    ev: On<PointerDrag>,
    q: Query<(), With<DragHandle>>,
    q_rows: Query<(&RowLine, &UiGlobalTransform, &ComputedNode)>,
    mut ui: ResMut<VsUi>,
) {
    if !q.contains(ev.entity) {
        return;
    }
    let Some(d) = ui.drag.as_mut() else { return };
    let y = ev.pointer.position.y;
    let mut to = 0;
    for (line, tf, node) in &q_rows {
        if line.row == d.row {
            continue;
        }
        let mid = tf.translation.y * node.inverse_scale_factor();
        if mid < y {
            to += 1;
        }
    }
    if ev.distance.length() > 4.0 {
        d.moved = true;
    }
    d.to = to;
}

fn on_drag_end(ev: On<PointerDragEnd>, q: Query<(), With<DragHandle>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    commands.queue(|world: &mut World| {
        let Some(d) = world.resource_mut::<VsUi>().drag.take() else {
            return;
        };
        if !d.moved {
            return;
        }
        let Some((id, s)) = active_studio(world) else {
            return;
        };
        if id != d.studio || s.variables.iter().position(|r| r.id == d.row) == Some(d.to) {
            return;
        }
        edit(world, "Move variable", |s, _| {
            studio::move_row(&mut s.variables, d.row, d.to)
        });
    });
}

/// While a row is dragged, a blue line where it would go.
fn sync_drop_marker(
    ui: Res<VsUi>,
    q_rows: Query<&RowLine>,
    mut q_lines: Query<(&DropLine, &mut Visibility)>,
) {
    if !ui.is_changed() {
        return;
    }
    let drag = ui.drag.filter(|d| d.moved);
    // The row the dragged one would land above, among the rows without it (`None`: the end).
    let mut order: Vec<&RowLine> = q_rows.iter().collect();
    order.sort_by_key(|l| l.index);
    let others: Vec<Uuid> = order
        .iter()
        .map(|l| l.row)
        .filter(|r| drag.is_none_or(|d| d.row != *r))
        .collect();
    let target = drag.map(|d| others.get(d.to).copied());
    for (line, mut vis) in &mut q_lines {
        vis.set_if_neq(if target == Some(line.0) {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
}

/// A click on a row's handle picks the row (again: unpicks it); Delete then deletes it.
fn on_handle_click(ev: On<PointerClick>, q: Query<&DragHandle>, mut ui: ResMut<VsUi>) {
    let Ok(DragHandle(row)) = q.get(ev.entity).copied() else {
        return;
    };
    if ui.drag.is_some_and(|d| d.moved) {
        return;
    }
    ui.selected = if ui.selected == Some(row) {
        None
    } else {
        Some(row)
    };
    ui.dirty = true;
}

/// Delete (or Backspace) deletes the picked row, unless a field is being edited; Esc unpicks it.
fn row_keys(
    keys: Res<ButtonInput<KeyCode>>,
    kind: Res<ActiveKind>,
    focus: Res<InputFocus>,
    q_field: Query<(), With<cadrs_ui::KeyboardField>>,
    mut ui: ResMut<VsUi>,
    mut commands: Commands,
) {
    if *kind != ActiveKind::VariableStudio || focus.get().is_some_and(|f| q_field.contains(f)) {
        return;
    }
    let Some(row) = ui.selected else { return };
    if keys.just_pressed(KeyCode::Escape) {
        ui.selected = None;
        ui.dirty = true;
    } else if keys.any_just_pressed([KeyCode::Delete, KeyCode::Backspace]) {
        ui.selected = None;
        commands.queue(move |world: &mut World| delete_row(world, row));
    }
}

/// A section opened or closed: remembered for its studio.
fn on_section_toggled(
    ev: On<cadrs_ui::CollapsibleToggled>,
    q: Query<&Section>,
    doc: Option<Res<ActiveDocument>>,
    mut ui: ResMut<VsUi>,
) {
    let (Ok(Section(key)), Some(id)) = (q.get(ev.entity), doc.and_then(|d| d.active)) else {
        return;
    };
    if ev.open {
        ui.collapsed.remove(&(id, *key));
    } else {
        ui.collapsed.insert((id, *key));
    }
}

/// A column dragged to a width: every table keeps it.
fn on_column_resize(
    ev: On<cadrs_ui::TableColumnResize>,
    kind: Res<ActiveKind>,
    mut ui: ResMut<VsUi>,
) {
    if *kind != ActiveKind::VariableStudio {
        return;
    }
    let column = ev.column.clone();
    ui.widths.retain(|(k, _)| *k != column);
    if let Some(w) = ev.width {
        ui.widths.push((column, w));
    }
    ui.dirty = true;
}

// ---------------------------------------------------------------------------------------------
// Insert Variable Studio

/// The Insert Variable Studio dialog's state.
#[derive(Resource, Debug, Clone)]
pub struct InsertSession {
    /// The Variable Studio it inserts into.
    pub studio: ElementId,
    /// 0: Current document, 1: Other documents.
    pub tab: usize,
    pub browse: Browse,
    /// The source studio picked (in this document, or in the opened one).
    pub source: Option<ElementId>,
    /// The variables picked; empty: the whole table.
    pub only: Vec<String>,
    /// Change version: the inserted table the pick replaces.
    pub replace: Option<Uuid>,
}

#[derive(Component)]
struct InsertDialog;

#[derive(Component)]
struct InsertBody;

/// A variable's checkbox in the dialog (`None`: the whole table).
#[derive(Component, Debug, Clone)]
struct PickVar(Option<String>);

/// A Variable Studio the dialog can insert.
#[derive(Debug, Clone, PartialEq)]
struct Source {
    id: ElementId,
    name: String,
    /// Its variables: name and value.
    vars: Vec<(String, String)>,
    /// Why it can't be inserted.
    blocked: Option<String>,
}

fn open_insert(world: &mut World) {
    let Some((id, _)) = active_studio(world) else {
        return;
    };
    world.insert_resource(InsertSession {
        studio: id,
        tab: 0,
        browse: Browse::default(),
        source: None,
        only: Vec::new(),
        replace: None,
    });
}

/// Change version of an inserted version: the dialog on Other documents, its document open at
/// its newest version with the same studio and variables picked.
fn open_change_version(world: &mut World, table: Uuid) {
    let Some((id, s)) = active_studio(world) else {
        return;
    };
    let Some(t) = s.inserted.iter().find(|t| t.id == table) else {
        return;
    };
    let this = world.resource::<ActiveDocument>().doc.id;
    world.insert_resource(InsertSession {
        studio: id,
        tab: 1,
        browse: Browse::default(),
        source: Some(t.source.element),
        only: t.only.clone(),
        replace: Some(table),
    });
    linked::open_in(
        world,
        Owner::VariableStudio,
        t.source.document_or(this),
        None,
    );
}

/// The studios of the open tab of the dialog: this document's others, or the opened document's.
fn insert_sources(world: &World) -> Vec<Source> {
    let Some(s) = world.get_resource::<InsertSession>() else {
        return Vec::new();
    };
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return Vec::new();
    };
    let units = doc.doc.units;
    let list = |d: &cadrs_core::Document, this: bool| -> Vec<Source> {
        d.elements
            .iter()
            .filter(|e| vs::studio_of(e).is_some() && !(this && e.id == s.studio))
            .map(|e| {
                let vars = vs::evaluate(d, e.id)
                    .map(|x| {
                        x.exported
                            .iter()
                            .map(|v| (v.var.name.clone(), v.var.display(&units)))
                            .collect()
                    })
                    .unwrap_or_default();
                let blocked = if this && vs::would_cycle(d, s.studio, e.id) {
                    Some("Inserts this Variable Studio".to_string())
                } else if this
                    && d.element(s.studio)
                        .and_then(vs::studio_of)
                        .is_some_and(|me| {
                            me.inserted
                                .iter()
                                .any(|t| t.is_live() && t.source.element == e.id)
                        })
                {
                    Some("Already inserted".to_string())
                } else {
                    None
                };
                Source {
                    id: e.id,
                    name: e.name.clone(),
                    vars,
                    blocked,
                }
            })
            .collect()
    };
    if s.tab == 0 {
        list(&doc.doc, true)
    } else {
        match &s.browse.opened {
            Some(o) if o.version.is_some() => list(&o.doc, false),
            _ => Vec::new(),
        }
    }
}

fn insert_picked(world: &mut World) {
    let Some(s) = world.get_resource::<InsertSession>().cloned() else {
        return;
    };
    let Some(src) = s.source else { return };
    let table = if s.tab == 0 {
        let name = world
            .resource::<ActiveDocument>()
            .doc
            .element(src)
            .map(|e| e.name.clone())
            .unwrap_or_default();
        InsertedTable::live(src, name, s.only.clone())
    } else {
        let Some(o) = s.browse.opened.as_ref() else {
            return;
        };
        let Some(r) = o.reference(src) else { return };
        let version = o.version.as_ref().map(|v| v.1.clone()).unwrap_or_default();
        match vs::snapshot(&o.doc, r, &version, s.only.clone()) {
            Ok(t) => t,
            Err(e) => {
                linked::error_toast(world, e);
                return;
            }
        }
    };
    let into = s.studio;
    let Some(mut st) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| vs::studio_of(d.doc.element(into)?).cloned())
    else {
        return;
    };
    // Change version puts the new copy where the old one was.
    let label = match s
        .replace
        .and_then(|r| st.inserted.iter().find(|t| t.id == r))
    {
        Some(old) => {
            let label = format!(
                "Change {} to {}",
                old.label(&world.resource::<ActiveDocument>().doc),
                table.version_name
            );
            st.replace_inserted(old.id, table);
            label
        }
        None => {
            let label = format!("Insert {}", table.name);
            st.inserted.push(table);
            label
        }
    };
    if set_studio(world, into, st, &label) {
        world.remove_resource::<InsertSession>();
    }
}

/// The dialog's ✕ or Esc ends the session.
fn on_insert_close(
    ev: On<cadrs_ui::DialogClose>,
    q: Query<(), With<InsertDialog>>,
    mut commands: Commands,
) {
    if q.contains(ev.entity) {
        commands.queue(|world: &mut World| {
            world.remove_resource::<InsertSession>();
        });
    }
}

fn on_insert_tab(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    if !q
        .get(ev.entity)
        .is_ok_and(|n| n.as_str() == "vs-insert-tabs")
    {
        return;
    }
    let i = ev.index;
    commands.queue(move |world: &mut World| {
        if let Some(mut s) = world.get_resource_mut::<InsertSession>()
            && s.tab != i
        {
            s.tab = i;
            s.source = None;
            s.only.clear();
        }
    });
}

fn on_insert_graph_pick(ev: On<VersionGraphSelect>, mut commands: Commands) {
    if ev.graph != "vs-insert-other-graph-view" {
        return;
    }
    let index = ev.index;
    commands.queue(move |world: &mut World| {
        let Some(document) = world
            .get_resource::<InsertSession>()
            .and_then(|s| s.browse.opened.as_ref().map(|o| o.document))
        else {
            return;
        };
        let (_, picks) = linked::version_graph(world, "x", document, None);
        if let Some(GraphPick::Version(v)) = picks.get(index).copied() {
            linked::open_in(world, Owner::VariableStudio, document, Some(v));
            // (Change version keeps the studio and variables it replaces picked.)
            if let Some(mut s) = world.get_resource_mut::<InsertSession>()
                && s.replace.is_none()
            {
                s.source = None;
                s.only.clear();
            }
        }
    });
}

fn on_insert_check(ev: On<CheckboxChange>, q: Query<&PickVar>, mut commands: Commands) {
    let Ok(PickVar(name)) = q.get(ev.entity).cloned() else {
        return;
    };
    let on = ev.checked;
    commands.queue(move |world: &mut World| {
        let sources = insert_sources(world);
        let Some(mut s) = world.get_resource_mut::<InsertSession>() else {
            return;
        };
        let Some(src) = s.source.and_then(|id| sources.iter().find(|x| x.id == id)) else {
            return;
        };
        let all: Vec<String> = src.vars.iter().map(|(n, _)| n.clone()).collect();
        match name {
            // The whole table: none picked one by one.
            None => s.only.clear(),
            Some(n) => {
                let mut picked = if s.only.is_empty() {
                    all.clone()
                } else {
                    s.only.clone()
                };
                picked.retain(|x| *x != n);
                if on {
                    picked.push(n);
                }
                // Back in the table's order; all of them is the whole table.
                let picked: Vec<String> =
                    all.iter().filter(|x| picked.contains(x)).cloned().collect();
                s.only = if picked.len() == all.len() {
                    Vec::new()
                } else {
                    picked
                };
                if s.only.is_empty() && !on {
                    // Unchecking the last one leaves nothing: keep that one.
                    s.only = Vec::new();
                }
            }
        }
    });
}

fn read_insert_search(
    q: Query<(&Name, &bevy::text::EditableText)>,
    session: Option<ResMut<InsertSession>>,
) {
    let Some(mut s) = session else { return };
    if let Some((_, t)) = q
        .iter()
        .find(|(n, _)| n.as_str() == "vs-insert-other-search-field")
    {
        let v = t.value().to_string();
        if s.browse.search != v {
            s.browse.search = v;
        }
    }
}

/// What the dialog shows (it is rebuilt when this changes).
#[derive(Debug, Clone, PartialEq)]
struct InsertView {
    tab: usize,
    browse_location: linked::Location,
    opened: Option<linked::Opened>,
    graph: bool,
    sources: Vec<Source>,
    source: Option<ElementId>,
    only: Vec<String>,
}

fn sync_insert_dialog(
    world: &mut World,
    q: &mut QueryState<Entity, With<InsertDialog>>,
    mut last: Local<Option<InsertView>>,
) {
    let dialog = q.iter(world).next();
    let Some(s) = world.get_resource::<InsertSession>().cloned() else {
        if let Some(e) = dialog {
            world.entity_mut(e).despawn();
        }
        *last = None;
        return;
    };
    // Leaving the studio's tab closes the dialog.
    if world
        .get_resource::<ActiveDocument>()
        .and_then(|d| d.active)
        != Some(s.studio)
    {
        world.remove_resource::<InsertSession>();
        return;
    }
    let v = InsertView {
        tab: s.tab,
        browse_location: s.browse.location,
        opened: s.browse.opened.clone(),
        graph: s.browse.graph,
        sources: insert_sources(world),
        source: s.source,
        only: s.only.clone(),
    };
    let theme = world.resource::<Theme>().clone();
    if dialog.is_none() {
        {
            let tf = theme.clone();
            let ok = if s.replace.is_some() {
                "Change version"
            } else {
                "Insert"
            };
            world.spawn((
                InsertDialog,
                DespawnOnExit(AppState::Document),
                Dialog::new("vs-insert-dialog")
                    .title(if s.replace.is_some() {
                        "Change version"
                    } else {
                        "Insert Variable Studio"
                    })
                    .width(460.0)
                    .body(|b| {
                        b.spawn((
                            InsertBody,
                            Name::new("vs-insert-body"),
                            Node {
                                flex_direction: FlexDirection::Column,
                                width: Val::Percent(100.0),
                                row_gap: Val::Px(6.0),
                                ..default()
                            },
                        ));
                    })
                    .footer(move |f| {
                        f.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        f.spawn(
                            cadrs_ui::Button::new("vs-insert-cancel")
                                .label("Cancel")
                                .build(&tf),
                        );
                        f.spawn(
                            cadrs_ui::Button::new("vs-insert-ok")
                                .label(ok)
                                .primary()
                                .build(&tf),
                        );
                    })
                    .build(&theme),
            ));
            *last = None;
        }
    }
    if last.as_ref() == Some(&v) {
        return;
    }
    let mut qb = world.query_filtered::<Entity, With<InsertBody>>();
    let Some(body) = qb.iter(world).next() else {
        return;
    };
    world.entity_mut(body).despawn_children();
    let (list, thumbs) = if v.tab == 1 && s.browse.opened.is_none() {
        let list = linked::browser_list(world, &s.browse);
        let thumbs = linked::browser_thumbs(world, &list.rows);
        (list, thumbs)
    } else {
        (linked::BrowserList::default(), Default::default())
    };
    let graph = match (&s.browse.opened, v.tab == 1 && s.browse.graph) {
        (Some(o), true) => Some(
            linked::version_graph(
                world,
                "vs-insert-other-graph-view",
                o.document,
                o.version.as_ref().map(|x| x.0),
            )
            .0,
        ),
        _ => None,
    };
    let doc_name = world.resource::<ActiveDocument>().doc.name.clone();
    let vv = v.clone();
    let t = theme.clone();
    world.commands().entity(body).with_children(|p| {
        p.spawn(
            TabStrip::new("vs-insert-tabs")
                .compact()
                .equal()
                .tab("Current document")
                .tab("Other documents")
                .selected(vv.tab)
                .build(&t),
        );
        if vv.tab == 0 {
            p.spawn(t.text(
                format!(
                    "Variable Studios of {doc_name}: inserted live, so they follow their edits"
                ),
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ));
            source_list(p, &t, &vv);
        } else {
            match &s.browse.opened {
                None => {
                    linked::spawn_browser(p, &t, Owner::VariableStudio, &s.browse, &list, &thumbs)
                }
                Some(o) => {
                    p.spawn(
                        cadrs_ui::OpenedHeader::new(
                            "vs-insert-other",
                            o.name.clone(),
                            o.version_label(),
                        )
                        .graph_open(s.browse.graph)
                        .build(&t),
                    );
                    if o.version.is_none() {
                        linked::spawn_no_version(p, &t, Owner::VariableStudio);
                    } else if let Some(g) = graph {
                        p.spawn(g.build(&t));
                    } else {
                        source_list(p, &t, &vv);
                    }
                }
            }
        }
    });
    world.flush();
    *last = Some(v);
}

/// The studios to pick from, then the picked one's variables with checkboxes.
fn source_list(p: &mut ChildSpawnerCommands, t: &Theme, v: &InsertView) {
    if v.sources.is_empty() {
        p.spawn((
            Name::new("vs-insert-none"),
            t.text(
                if v.tab == 0 {
                    "This document has no other Variable Studio. Create one with + ▸ Create Variable Studio."
                } else {
                    "This version has no Variable Studio."
                },
                t.font_sm,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
            Node { margin: UiRect::all(Val::Px(8.0)), max_width: Val::Px(420.0), ..default() },
        ))
        .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
        return;
    }
    p.spawn(Node {
        flex_direction: FlexDirection::Column,
        max_height: Val::Px(320.0),
        overflow: Overflow::scroll_y(),
        ..default()
    })
    .with_children(|l| {
        for (i, s) in v.sources.iter().enumerate() {
            let picked = v.source == Some(s.id);
            let subtitle = match &s.blocked {
                Some(why) => why.clone(),
                None => format!(
                    "{} {}",
                    s.vars.len(),
                    if s.vars.len() == 1 {
                        "variable"
                    } else {
                        "variables"
                    }
                ),
            };
            l.spawn(
                cadrs_ui::DocumentRow::new(
                    format!("vs-insert-studio-{i}"),
                    s.name.clone(),
                    subtitle,
                )
                .icon("variables")
                .selected(picked)
                .muted(s.blocked.is_some())
                .build(t),
            );
            if picked {
                l.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(Val::Px(28.0), Val::Px(4.0), Val::Px(2.0), Val::Px(6.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                })
                .with_children(|c| {
                    c.spawn((
                        PickVar(None),
                        Checkbox::new("vs-insert-whole")
                            .label("Whole table")
                            .checked(v.only.is_empty())
                            .build(t),
                    ));
                    for (n, value) in &s.vars {
                        c.spawn((
                            PickVar(Some(n.clone())),
                            Checkbox::new(format!("vs-insert-var-{n}"))
                                .label(format!("#{n}  =  {value}"))
                                .checked(v.only.is_empty() || v.only.contains(n))
                                .build(t),
                        ));
                    }
                });
            }
        }
    });
}

/// A new search only rebuilds the browser's list, so the field keeps its focus.
fn sync_insert_list(world: &mut World, mut last: Local<Option<(Browse, u64)>>) {
    let Some(s) = world.get_resource::<InsertSession>() else {
        *last = None;
        return;
    };
    if s.tab != 1 || s.browse.opened.is_some() {
        *last = None;
        return;
    }
    let browse = s.browse.clone();
    let generation = world.resource::<linked::LibrarySnapshot>().generation;
    if last
        .as_ref()
        .is_some_and(|(b, g)| b == &browse && *g == generation)
    {
        return;
    }
    let refresh = last.as_ref().is_some_and(|(b, g)| {
        b.location == browse.location && (b.search != browse.search || *g != generation)
    });
    *last = Some((browse.clone(), generation));
    if !refresh {
        return;
    }
    let mut qn = world.query::<(Entity, &Name, &ChildOf)>();
    let Some((node, parent)) = qn
        .iter(world)
        .find(|(_, n, _)| n.as_str() == "vs-insert-other-list")
        .map(|(e, _, p)| (e, p.parent()))
    else {
        return;
    };
    let list = linked::browser_list(world, &browse);
    let thumbs = linked::browser_thumbs(world, &list.rows);
    let theme = world.resource::<Theme>().clone();
    world.entity_mut(node).despawn();
    world.commands().entity(parent).with_children(|p| {
        linked::spawn_browser_list(p, &theme, Owner::VariableStudio, &browse, &list, &thumbs)
    });
    world.flush();
}

/// Scripted scenarios: `variable-studio <verb> …` (see `crate::script`).
pub fn script(world: &mut World, arg: &str) {
    let mut words = arg.split_whitespace();
    match words.next() {
        Some("add") => {
            // add <name> <expr…>
            let Some(name) = words.next() else { return };
            let expr: Vec<&str> = words.collect();
            let expr = expr.join(" ");
            let name = name.to_string();
            edit(world, "Add variable", |s, units| {
                let mut r = studio::new_row(&name, units);
                if !expr.is_empty() {
                    r.var.expr = expr.clone();
                }
                s.variables.push(r);
                true
            });
        }
        Some("type") => {
            // type <name> <Length|Angle|Number|Any>
            let (Some(name), Some(k)) = (words.next(), words.next()) else {
                return;
            };
            let Some(t) = VariableType::ALL
                .into_iter()
                .find(|t| t.label().eq_ignore_ascii_case(k))
            else {
                return;
            };
            let name = name.to_string();
            edit(world, "Variable type", |s, _| {
                let Some(r) = s.variables.iter_mut().find(|r| r.var.name == name) else {
                    return false;
                };
                r.var.var_type = t;
                true
            });
        }
        Some("describe") => {
            let Some(name) = words.next() else { return };
            let text: Vec<&str> = words.collect();
            let (name, text) = (name.to_string(), text.join(" "));
            edit(world, "Edit variable description", |s, _| {
                let Some(r) = s.variables.iter_mut().find(|r| r.var.name == name) else {
                    return false;
                };
                r.var.description = text.clone();
                true
            });
        }
        Some("available") => {
            let on = words.next() != Some("off");
            edit(world, "Availability", |s, _| {
                s.available = on;
                true
            });
        }
        Some("insert") => {
            // insert <tab name>: a studio of this document, whole.
            let name: Vec<&str> = words.collect();
            let name = name.join(" ");
            let src = world
                .resource::<ActiveDocument>()
                .doc
                .elements
                .iter()
                .find(|e| e.name == name && vs::studio_of(e).is_some())
                .map(|e| e.id);
            if let Some(src) = src {
                edit(world, "Insert Variable Studio", |s, _| {
                    s.inserted
                        .push(InsertedTable::live(src, name.clone(), Vec::new()));
                    true
                });
            }
        }
        Some("open-insert") => open_insert(world),
        Some("collapse") => {
            if let Some((id, _)) = active_studio(world) {
                world
                    .resource_mut::<VsUi>()
                    .collapsed
                    .insert((id, TableKey::Own));
            }
        }
        _ => warn!("variable-studio: unknown script command {arg:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_core::assembly::mate::{MateKind, OffsetSlot};
    use cadrs_core::samples::step_stool as ss;

    /// The step stool with a studio whose `#lift` raises the Cross Bar through its mate's Z
    /// offset.
    fn stool() -> (App, ElementId) {
        let mut doc = ss::document().unwrap();
        let mut studio = cadrs_core::document::Element::variable_studio("Dimensions");
        let s = studio.id;
        let mut lift = studio::new_row("lift", &doc.units);
        lift.var.expr = "0 mm".into();
        let cadrs_core::document::ElementKind::VariableStudio(v) = &mut studio.kind else {
            unreachable!()
        };
        v.variables.push(lift);
        doc.elements.push(studio);
        let asm = doc
            .element_mut(ss::ASSEMBLY)
            .unwrap()
            .assembly_model_mut()
            .unwrap();
        let mate = asm
            .mates
            .iter_mut()
            .find(|m| m.id == ss::FASTENED_1)
            .unwrap();
        let MateKind::Mate(m) = &mate.kind else {
            unreachable!()
        };
        let z = m.offset.unwrap().translation[2];
        mate.exprs = vec![(OffsetSlot::Z, format!("{z} mm + #lift"))];
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(ActiveDocument::new(doc))
            .init_resource::<crate::assembly::AssemblyParts>();
        (app, s)
    }

    fn poses(app: &App) -> Vec<(InstanceId, Pose)> {
        let doc = &app.world().resource::<ActiveDocument>().doc;
        let asm = doc.element(ss::ASSEMBLY).unwrap().assembly_model().unwrap();
        asm.instances.iter().map(|i| (i.id, i.pose)).collect()
    }

    /// How much further apart the Base and the Cross Bar are in `b` than in `a` (the solver may
    /// move either).
    fn apart(a: &[(InstanceId, Pose)], b: &[(InstanceId, Pose)]) -> f64 {
        let at =
            |p: &[(InstanceId, Pose)], i| p.iter().find(|(id, _)| *id == i).unwrap().1.translation;
        let d = |p: &[(InstanceId, Pose)]| {
            let (x, y) = (at(p, ss::BASE), at(p, ss::CROSS));
            [y[0] - x[0], y[1] - x[1], y[2] - x[2]]
        };
        let (d0, d1) = (d(a), d(b));
        ((d1[0] - d0[0]).powi(2) + (d1[1] - d0[1]).powi(2) + (d1[2] - d0[2]).powi(2)).sqrt()
    }

    /// Sets the studio's `#lift`.
    fn lift(app: &mut App, s: ElementId, expr: &str) {
        let doc = &app.world().resource::<ActiveDocument>().doc;
        let mut st = vs::studio_of(doc.element(s).unwrap()).unwrap().clone();
        st.variables[0].var.expr = expr.into();
        assert!(set_studio(app.world_mut(), s, st, &format!("Lift {expr}")));
    }

    /// Runs the background solve until it is done, or waits for the document to come back.
    fn settle(app: &mut App) {
        let start = web_time::Instant::now();
        loop {
            assert!(start.elapsed().as_secs() < 120, "the solve never settled");
            finish_studio_solve(app.world_mut());
            let Some(solve) = app.world().get_resource::<StudioSolve>() else {
                return;
            };
            let h = &app.world().resource::<ActiveDocument>().history;
            let parked = match solve.stage {
                Stage::Solved { checked, .. } => checked == Some(h.revision()),
                Stage::Builds => solve.edit.is_none() && h.redo_len() > 0,
                Stage::Solving { .. } => false,
            };
            if parked {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn doc(app: &mut App) -> Mut<'_, ActiveDocument> {
        app.world_mut().resource_mut::<ActiveDocument>()
    }

    /// A studio edit commits at once; the assembly following it is solved on a task, and its
    /// placements join the edit's undo step.
    #[test]
    fn an_assembly_is_solved_off_the_ui_thread_in_the_edits_step() {
        let (mut app, s) = stool();
        let before = poses(&app);
        let undo = doc(&mut app).history.undo_len();
        lift(&mut app, s, "25 mm");
        assert!(app.world().contains_resource::<StudioSolve>());
        settle(&mut app);
        assert!(!app.world().contains_resource::<StudioSolve>());
        let shift = apart(&before, &poses(&app));
        assert!((shift - 25.0).abs() < 1e-6, "moved {shift} mm apart");
        assert_eq!(doc(&mut app).history.undo_len(), undo + 1, "one undo step");
        doc(&mut app).undo();
        assert_eq!(poses(&app), before);
    }

    /// Undone before the solve is done: nothing is placed on the document the undo left; the
    /// redo brings the edit back and its placements follow (a step of their own).
    #[test]
    fn an_undo_waits_for_the_redo() {
        let (mut app, s) = stool();
        let before = poses(&app);
        let undo = doc(&mut app).history.undo_len();
        lift(&mut app, s, "25 mm");
        doc(&mut app).undo();
        settle(&mut app);
        assert_eq!(poses(&app), before);
        assert_eq!(doc(&mut app).history.redo_len(), 1, "the redo is kept");
        assert!(app.world().contains_resource::<StudioSolve>());
        doc(&mut app).redo();
        settle(&mut app);
        assert!(!app.world().contains_resource::<StudioSolve>());
        let shift = apart(&before, &poses(&app));
        assert!((shift - 25.0).abs() < 1e-6, "moved {shift} mm apart");
        assert_eq!(doc(&mut app).history.undo_len(), undo + 2);
    }

    /// An edit while the last one's solve runs: that one finishes first, so each edit undoes
    /// with its own placements.
    #[test]
    fn each_edit_keeps_its_own_placements() {
        let (mut app, s) = stool();
        let before = poses(&app);
        let undo = doc(&mut app).history.undo_len();
        lift(&mut app, s, "10 mm");
        lift(&mut app, s, "30 mm");
        settle(&mut app);
        assert_eq!(doc(&mut app).history.undo_len(), undo + 2);
        assert!((apart(&before, &poses(&app)) - 30.0).abs() < 1e-6);
        doc(&mut app).undo();
        assert!((apart(&before, &poses(&app)) - 10.0).abs() < 1e-6);
    }

    /// Another edit before the solve is done that leaves the assembly and its parts as they
    /// were: the placements still apply, as a step of their own after it.
    #[test]
    fn placements_after_an_unrelated_edit_are_their_own_step() {
        let (mut app, s) = stool();
        let before = poses(&app);
        let undo = doc(&mut app).history.undo_len();
        lift(&mut app, s, "25 mm");
        let rename = cadrs_core::commands::RenameElement {
            id: s,
            name: "Sizes".into(),
        };
        doc(&mut app).execute(&rename).unwrap();
        settle(&mut app);
        assert!((apart(&before, &poses(&app)) - 25.0).abs() < 1e-6);
        {
            let mut d = doc(&mut app);
            assert_eq!(d.history.undo_len(), undo + 3);
            assert_eq!(d.history.undo_label(), Some("Lift 25 mm (placements)"));
            d.undo();
        }
        assert_eq!(poses(&app), before, "the placements undo on their own");
    }

    /// At the history's step limit (the oldest step goes with each new one), the placements
    /// still join the edit's step.
    #[test]
    fn placements_join_the_edit_at_the_step_limit() {
        let (mut app, s) = stool();
        doc(&mut app).history = cadrs_core::History::new(3);
        for n in ["A", "B", "C", "D"] {
            let rename = cadrs_core::commands::RenameElement {
                id: s,
                name: n.into(),
            };
            doc(&mut app).execute(&rename).unwrap();
        }
        assert_eq!(doc(&mut app).history.undo_len(), 3);
        let before = poses(&app);
        lift(&mut app, s, "25 mm");
        settle(&mut app);
        assert!((apart(&before, &poses(&app)) - 25.0).abs() < 1e-6);
        {
            let mut d = doc(&mut app);
            assert_eq!(d.history.undo_label(), Some("Lift 25 mm"));
            d.undo();
        }
        assert_eq!(
            poses(&app),
            before,
            "one step undoes the edit and its placements"
        );
    }

    /// An edit while a solve waits for an undo to be redone: the waiting solve is dropped (not
    /// placed onto the undone state), and the new edit's own solve places its result.
    #[test]
    fn an_edit_after_an_undo_drops_the_waiting_solve() {
        let (mut app, s) = stool();
        let before = poses(&app);
        let undo = doc(&mut app).history.undo_len();
        lift(&mut app, s, "25 mm");
        doc(&mut app).undo();
        settle(&mut app);
        assert_eq!(poses(&app), before);
        lift(&mut app, s, "5 mm");
        settle(&mut app);
        assert!((apart(&before, &poses(&app)) - 5.0).abs() < 1e-6);
        let d = doc(&mut app);
        assert_eq!(d.history.undo_len(), undo + 1);
        assert_eq!(d.history.redo_len(), 0);
    }
}

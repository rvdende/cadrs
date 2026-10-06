//! Data tables, modeled on gpui-component's `Table` (`table/`): [`Column`]s with fixed or
//! flexible widths and a [`ColumnSort`] state, a [`TableHeader`] whose sortable cells cycle the
//! sort when clicked, and [`TableRow`]s with hover highlight.
//!
//! - Clicking a sortable header cell triggers [`TableSortChange`] (bubbling) with the new sort;
//!   with [`TableHeader::double_click_sort`] it takes a double click instead (Onshape's BOM,
//!   A20.3). With [`TableHeader::menus`] every header cell is a [`ContextMenuTarget`] carrying
//!   its [`TableHeaderCell`], for a column menu (Remove column, Move left / right).
//!   The owner re-sorts its data and rebuilds the rows; the table does not own the data, like
//!   gpui-component's `TableDelegate::perform_sort`.
//! - Clicking a row with the primary button triggers `Activate` on the row. Rows are also
//!   [`ContextMenuTarget`]s, so a right-click triggers [`crate::ContextMenuRequested`]. A row
//!   given [`crate::DoubleClickable`] also triggers [`crate::DoubleClick`] on a double click
//!   (P3E.1: the documents list selects on a click and opens on a double click, like
//!   gpui-component's `Table` `on_click` with `click_count`).
//! - Header labels that don't fit are cut with "…" ([`crate::ellipsis`]) and show the whole
//!   label in a tooltip; a header's sort arrow has its own fixed slot, so the label is cut
//!   first and the arrow never.
//! - With [`TableHeader::resizable`] the divider at the right of each header cell is a resize
//!   handle (gpui-component's resizable columns): dragging it resizes the column live (every
//!   cell of that column under the same [`TableRoot`], or under the header's parent) and
//!   triggers [`TableColumnResize`] when released; double-clicking it asks for the column to
//!   fit its content (`width: None`). The owner keeps the width for its next rebuild.
//! - [`Column::grow`] shares the free width between flexible columns by weight;
//!   [`Column::shaded`] fills a column's cells (a row-number column); [`TableHeader::grid`] and
//!   [`TableRow::grid`] draw grid lines between the cells (gpui-component's bordered table).
//! - A [`TableBody`] (the rows' scroll container, a sibling of the header) scrolls both ways
//!   with the wheel (Shift+wheel: sideways); the header follows its horizontal scroll, so a
//!   wide table scrolls sideways inside a fixed-width panel.

use std::borrow::Cow;

use bevy::ecs::spawn::SpawnWith;
use bevy::input::mouse::MouseScrollUnit;
use bevy::picking::hover::Hovered;
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as WidgetButton};

use crate::ellipsis::Ellipsis;
use crate::icon::icon;
use crate::menu::ContextMenuTarget;
use crate::style::{InheritFg, InitState, StateColors, VisualState, Visuals};
use crate::theme::Theme;

pub struct TablePlugin;

impl Plugin for TablePlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_header_activate)
            .add_observer(on_header_double_click)
            .add_observer(on_row_click)
            .add_observer(on_resize_start)
            .add_observer(on_resize_drag)
            .add_observer(on_resize_end)
            .add_observer(on_resize_click)
            .add_observer(on_resize_double_click)
            .add_observer(on_body_scroll)
            .add_systems(Update, (sync_header_scroll, resize_handle_look));
    }
}

/// The sort state of a column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColumnSort {
    /// Sortable but not the sort column.
    #[default]
    Default,
    Ascending,
    Descending,
}

/// One column of a table.
#[derive(Debug, Clone)]
pub struct Column {
    pub key: Cow<'static, str>,
    pub label: String,
    /// Fixed width in px; `None` takes the remaining space.
    pub width: Option<f32>,
    pub sortable: bool,
    pub sort: ColumnSort,
    /// The sort applied when an unsorted column is clicked.
    pub first_sort: ColumnSort,
    /// The narrowest a resize can make it (px).
    pub min_width: f32,
    /// A flexible column's share of the free width (default 1).
    pub grow: f32,
    /// The cells' fill (header and body), if any.
    pub background: Option<Color>,
}

impl Column {
    pub fn new(key: impl Into<Cow<'static, str>>, label: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            width: None,
            sortable: false,
            sort: ColumnSort::Default,
            first_sort: ColumnSort::Ascending,
            min_width: 32.0,
            grow: 1.0,
            background: None,
        }
    }

    /// A flexible column (no fixed width) taking `weight` shares of the free width.
    pub fn grow(mut self, weight: f32) -> Self {
        self.width = None;
        self.grow = weight;
        self
    }

    /// Fills the column's cells (Onshape's shaded "#" column).
    pub fn shaded(mut self, c: Color) -> Self {
        self.background = Some(c);
        self
    }

    /// The narrowest a resize can make it (default 32 px).
    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w;
        self
    }

    pub fn width(mut self, w: f32) -> Self {
        self.width = Some(w);
        self
    }

    pub fn sortable(mut self) -> Self {
        self.sortable = true;
        self
    }

    pub fn sort(mut self, s: ColumnSort) -> Self {
        self.sort = s;
        self
    }

    /// Which direction the first click sorts in (dates usually start newest first).
    pub fn first_sort(mut self, s: ColumnSort) -> Self {
        self.first_sort = s;
        self
    }

    /// The layout node for a cell in this column.
    pub fn cell_node(&self, theme: &Theme) -> Node {
        let mut n = Node {
            height: Val::Percent(100.0),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(theme.space[4])),
            column_gap: Val::Px(theme.space[4]),
            overflow: Overflow::clip(),
            ..default()
        };
        match self.width {
            Some(w) => {
                n.width = Val::Px(w);
                n.flex_shrink = 0.0;
            }
            None => {
                n.flex_grow = self.grow;
                n.flex_basis = Val::Px(0.0);
                n.min_width = Val::Px(0.0);
            }
        }
        n
    }

    /// A cell's node with a grid line on its right (`grid`), and its fill.
    fn cell_bundle(&self, theme: &Theme, grid: bool) -> (Node, BackgroundColor, BorderColor) {
        let mut n = self.cell_node(theme);
        if grid {
            n.border = UiRect::right(Val::Px(1.0));
        }
        (n, BackgroundColor(self.background.unwrap_or(Color::NONE)), BorderColor::all(theme.row_separator))
    }
}

/// The sort of a table changed because a header cell was clicked. Bubbles from the cell.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct TableSortChange {
    pub entity: Entity,
    pub column: String,
    pub sort: ColumnSort,
}

/// A header cell (sortable, or with a menu).
#[derive(Component, Debug, Clone)]
pub struct TableHeaderCell {
    pub column: String,
    pub sort: ColumnSort,
    pub first_sort: ColumnSort,
    /// Sorts on a click (`false`: on a double click, or not at all).
    pub click_sorts: bool,
    pub sortable: bool,
}

impl TableHeaderCell {
    /// The sort a click (or double click) goes to.
    pub fn next_sort(&self) -> ColumnSort {
        match self.sort {
            ColumnSort::Default => self.first_sort,
            ColumnSort::Ascending => ColumnSort::Descending,
            ColumnSort::Descending => ColumnSort::Ascending,
        }
    }
}

/// Builder for a table's header row. Cells are named `<name>-<column key>`.
pub struct TableHeader {
    name: Cow<'static, str>,
    columns: Vec<Column>,
    height: f32,
    double_click_sort: bool,
    menus: bool,
    resizable: bool,
    grid: bool,
}

impl TableHeader {
    pub fn new(name: impl Into<Cow<'static, str>>, columns: Vec<Column>) -> Self {
        Self {
            name: name.into(),
            columns,
            height: 30.0,
            double_click_sort: false,
            menus: false,
            resizable: false,
            grid: false,
        }
    }

    /// Grid lines between the cells (and a top border), for a bordered table.
    pub fn grid(mut self) -> Self {
        self.grid = true;
        self
    }

    /// The dividers between header cells can be dragged to resize the columns (see the module
    /// docs). Cells are resized live; [`TableColumnResize`] reports the width on release.
    pub fn resizable(mut self) -> Self {
        self.resizable = true;
        self
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = h;
        self
    }

    /// Sortable columns sort on a double click of their header, not a click.
    pub fn double_click_sort(mut self) -> Self {
        self.double_click_sort = true;
        self
    }

    /// Every header cell can be right-clicked ([`crate::ContextMenuRequested`] on the cell,
    /// which carries its [`TableHeaderCell`]).
    pub fn menus(mut self) -> Self {
        self.menus = true;
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let TableHeader {
            name,
            columns,
            height,
            double_click_sort,
            menus,
            resizable,
            grid,
        } = self;
        let prefix = name.to_string();
        (
            Name::new(name.into_owned()),
            TableHeaderRow,
            Node {
                height: Val::Px(height),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                border: if grid { UiRect::new(Val::ZERO, Val::ZERO, Val::Px(1.0), Val::Px(1.0)) } else { UiRect::bottom(Val::Px(2.0)) },
                // Scrolled sideways with its [`TableBody`] (not by the wheel itself).
                overflow: Overflow {
                    x: OverflowAxis::Scroll,
                    y: OverflowAxis::Clip,
                },
                ..default()
            },
            ScrollPosition::default(),
            BorderColor::all(if grid { theme.row_separator } else { Color::srgb_u8(0xd0, 0xd0, 0xd0) }),
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let n = columns.len();
                for (i, col) in columns.into_iter().enumerate() {
                    let fg = theme.foreground;
                    let font = theme.font(theme.font_base, FontWeight::BOLD);
                    let muted = theme.muted_foreground;
                    let sep = theme.border_strong;
                    let mut cell = p.spawn((
                        Name::new(format!("{prefix}-{}", col.key)),
                        TableColumnCell {
                            column: col.key.to_string(),
                        },
                        col.cell_bundle(&theme, grid),
                    ));
                    if grid {
                        // A bordered table centres its headers over its centred cells.
                        cell.entry::<Node>().and_modify(|mut n| n.justify_content = JustifyContent::Center);
                    }
                    if col.sortable || menus {
                        cell.insert(TableHeaderCell {
                            column: col.key.to_string(),
                            sort: col.sort,
                            first_sort: col.first_sort,
                            click_sorts: col.sortable && !double_click_sort,
                            sortable: col.sortable,
                        });
                    }
                    if menus {
                        cell.insert(ContextMenuTarget);
                    }
                    if col.sortable && double_click_sort {
                        cell.insert(crate::inline_edit::DoubleClickable);
                    }
                    if col.sortable && !double_click_sort {
                        cell.insert((
                            WidgetButton,
                            Hovered::default(),
                            Visuals {
                                background: StateColors::all(Color::NONE),
                                border: StateColors::all(Color::NONE),
                                foreground: StateColors::all(fg),
                                focus_ring: theme.focus_ring,
                            },
                        ));
                    }
                    let arrow = match col.sort {
                        ColumnSort::Ascending => Some("arrow-up"),
                        ColumnSort::Descending => Some("arrow-down"),
                        ColumnSort::Default => None,
                    };
                    let key = col.key.to_string();
                    let min = col.min_width;
                    let accent = theme.primary;
                    cell.with_children(|c| {
                        // The label is cut ("…", the whole of it in a tooltip) before the arrow.
                        c.spawn((
                            Name::new(format!("{prefix}-{key}-label")),
                            Text::new(col.label.clone()),
                            font,
                            TextColor(fg),
                            TextLayout::no_wrap(),
                            InheritFg,
                            Ellipsis::default().with_tooltip(),
                            Ellipsis::node(),
                            Pickable::IGNORE,
                        ));
                        if let Some(a) = arrow {
                            // A fixed slot: it never shrinks.
                            c.spawn((
                                Name::new(format!("{prefix}-{key}-sort")),
                                Node {
                                    width: Val::Px(14.0),
                                    height: Val::Px(14.0),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_child((icon(a, 14.0, muted), Pickable::IGNORE));
                        }
                        // The column divider (Onshape draws a short grey line between headers);
                        // in a resizable header it is the resize handle. A grid has its own lines.
                        let idle = if i + 1 < n && !grid { sep } else { Color::NONE };
                        let line = (
                            ResizeLine,
                            Node {
                                position_type: PositionType::Absolute,
                                right: Val::Px(0.0),
                                top: Val::Percent(20.0),
                                height: Val::Percent(60.0),
                                width: Val::Px(2.0),
                                ..default()
                            },
                            BackgroundColor(idle),
                            Pickable::IGNORE,
                        );
                        if resizable {
                            c.spawn((
                                Name::new(format!("{prefix}-{key}-resize")),
                                TableResizeHandle {
                                    column: key.clone(),
                                    min,
                                    start: 0.0,
                                    dragging: false,
                                    idle,
                                    active: accent,
                                },
                                crate::inline_edit::DoubleClickable,
                                Hovered::default(),
                                Node {
                                    position_type: PositionType::Absolute,
                                    right: Val::Px(0.0),
                                    top: Val::Px(0.0),
                                    bottom: Val::Px(0.0),
                                    width: Val::Px(8.0),
                                    ..default()
                                },
                            ))
                            .with_child(line);
                        } else if i + 1 < n && !grid {
                            c.spawn(line);
                        }
                    });
                }
            })),
        )
    }
}

type SpawnFn = Box<dyn FnOnce(&mut ChildSpawner) + Send + Sync>;

/// A table body row.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TableRowMarker;

/// Builder for a table row: one cell per column, each filled by a closure.
pub struct TableRow {
    name: Cow<'static, str>,
    columns: Vec<Column>,
    cells: Vec<SpawnFn>,
    height: Option<f32>,
    selected: bool,
    force: Option<VisualState>,
    grid: bool,
}

impl TableRow {
    /// `columns` gives the cell widths (usually the same list as the header's).
    pub fn new(name: impl Into<Cow<'static, str>>, columns: &[Column]) -> Self {
        Self {
            name: name.into(),
            columns: columns.to_vec(),
            cells: Vec::new(),
            height: None,
            selected: false,
            force: None,
            grid: false,
        }
    }

    /// Grid lines between the cells, for a bordered table.
    pub fn grid(mut self) -> Self {
        self.grid = true;
        self
    }

    /// Adds the next cell's content.
    pub fn cell(mut self, f: impl FnOnce(&mut ChildSpawner) + Send + Sync + 'static) -> Self {
        self.cells.push(Box::new(f));
        self
    }

    /// Adds a plain text cell.
    pub fn text_cell(self, theme: &Theme, text: impl Into<String>) -> Self {
        let bundle = (
            theme.text(
                text.into(),
                theme.font_base,
                bevy::text::FontWeight::MEDIUM,
                theme.foreground,
            ),
            InheritFg,
            Pickable::IGNORE,
        );
        self.cell(move |p| {
            p.spawn(bundle);
        })
    }

    pub fn height(mut self, h: f32) -> Self {
        self.height = Some(h);
        self
    }

    pub fn selected(mut self, s: bool) -> Self {
        self.selected = s;
        self
    }

    pub fn force_state(mut self, s: VisualState) -> Self {
        self.force = Some(s);
        self
    }

    pub fn build(self, theme: &Theme) -> impl Bundle {
        let theme = theme.clone();
        let TableRow {
            name,
            columns,
            cells,
            height,
            selected,
            force,
            grid,
        } = self;
        let fg = theme.foreground;
        let total = total_width(&columns);
        (
            Name::new(name.into_owned()),
            TableRowMarker,
            ContextMenuTarget,
            Node {
                height: Val::Px(height.unwrap_or(theme.list_row_height)),
                // As wide as its fixed columns, so a wide table scrolls sideways in its body.
                min_width: Val::Px(total),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            Hovered::default(),
            Visuals {
                background: StateColors::new(
                    Color::NONE,
                    theme.list_hover,
                    theme.list_active,
                    Color::NONE,
                )
                .with_selected(theme.list_selected),
                border: StateColors::all(theme.row_separator),
                foreground: StateColors::new(fg, fg, fg, theme.disabled_foreground),
                focus_ring: theme.focus_ring,
            },
            InitState {
                disabled: false,
                selected,
                force,
            },
            Children::spawn(SpawnWith(move |p: &mut ChildSpawner| {
                let mut cells = cells.into_iter();
                for col in &columns {
                    let mut cell = p.spawn((
                        TableColumnCell {
                            column: col.key.to_string(),
                        },
                        col.cell_bundle(&theme, grid),
                        Pickable::IGNORE,
                    ));
                    if let Some(f) = cells.next() {
                        cell.with_children(|c| f(c));
                    }
                }
            })),
        )
    }
}

/// The sum of the fixed column widths.
pub fn total_width(columns: &[Column]) -> f32 {
    columns.iter().filter_map(|c| c.width).sum()
}

/// A table's header row.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TableHeaderRow;

/// A header or body cell of a column (resized together).
#[derive(Component, Debug, Clone)]
pub struct TableColumnCell {
    pub column: String,
}

/// Marks the node holding a table's header and body; a column resize reaches the cells under
/// it. Without one, the header's parent is used.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct TableRoot;

/// The rows' scroll container (`Overflow::scroll()`), next to the header: the wheel scrolls it
/// (Shift+wheel sideways) and the header follows its horizontal scroll.
#[derive(Component, Debug, Clone, Copy, Default)]
#[require(ScrollPosition)]
pub struct TableBody;

/// The drag handle on a header cell's right edge (a resizable header).
#[derive(Component, Debug, Clone)]
pub struct TableResizeHandle {
    pub column: String,
    min: f32,
    start: f32,
    dragging: bool,
    idle: Color,
    active: Color,
}

/// The visible line of a divider.
#[derive(Component, Debug, Clone, Copy)]
struct ResizeLine;

/// A column was resized by dragging its header divider (on release), or its divider was
/// double-clicked (`width: None`: fit the content). Bubbles from the header cell.
#[derive(EntityEvent, Clone, Debug)]
#[entity_event(propagate, auto_propagate)]
pub struct TableColumnResize {
    pub entity: Entity,
    pub column: String,
    pub width: Option<f32>,
}

fn px_width(n: &Node) -> f32 {
    if let Val::Px(w) = n.width { w } else { 0.0 }
}

fn on_resize_start(
    mut ev: On<Pointer<DragStart>>,
    mut q: Query<(&mut TableResizeHandle, &ChildOf)>,
    q_node: Query<&Node>,
) {
    let Ok((mut h, parent)) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    h.start = q_node.get(parent.parent()).map(px_width).unwrap_or(0.0);
    h.dragging = true;
}

/// The node a column resize reaches: the nearest [`TableRoot`] above the header row, or the
/// header row's parent.
fn table_root(header: Entity, q_parent: &Query<&ChildOf>, q_root: &Query<(), With<TableRoot>>) -> Option<Entity> {
    let mut e = header;
    for _ in 0..16 {
        if q_root.contains(e) {
            return Some(e);
        }
        match q_parent.get(e) {
            Ok(p) => e = p.parent(),
            Err(_) => break,
        }
    }
    q_parent.get(header).ok().map(|p| p.parent())
}

#[allow(clippy::type_complexity)]
fn on_resize_drag(
    mut ev: On<Pointer<Drag>>,
    q: Query<(&TableResizeHandle, &ChildOf)>,
    q_parent: Query<&ChildOf>,
    q_root: Query<(), With<TableRoot>>,
    q_children: Query<&Children>,
    mut q_cells: Query<(&TableColumnCell, &mut Node), Without<TableRowMarker>>,
    mut q_rows: Query<&mut Node, (With<TableRowMarker>, Without<TableColumnCell>)>,
) {
    let Ok((h, parent)) = q.get(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !h.dragging {
        return;
    }
    let width = (h.start + ev.distance.x).max(h.min).round();
    let cell = parent.parent();
    let Ok(header) = q_parent.get(cell).map(|p| p.parent()) else {
        return;
    };
    let Some(root) = table_root(header, &q_parent, &q_root) else {
        return;
    };
    for e in q_children.iter_descendants(root) {
        if let Ok((c, mut n)) = q_cells.get_mut(e)
            && c.column == h.column
            && n.width != Val::Px(width)
        {
            n.width = Val::Px(width);
        }
    }
    // Every row is as wide as the header's columns.
    let total: f32 = q_children
        .get(header)
        .map(|cs| cs.iter().filter_map(|c| q_cells.get(c).ok()).map(|(_, n)| px_width(n)).sum())
        .unwrap_or(0.0);
    for e in q_children.iter_descendants(root) {
        if let Ok(mut n) = q_rows.get_mut(e) {
            n.min_width = Val::Px(total);
        }
    }
}

fn on_resize_end(
    mut ev: On<Pointer<DragEnd>>,
    mut q: Query<(&mut TableResizeHandle, &ChildOf)>,
    q_node: Query<&Node>,
    mut commands: Commands,
) {
    let Ok((mut h, parent)) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    if !h.dragging {
        return;
    }
    h.dragging = false;
    let cell = parent.parent();
    let width = q_node.get(cell).map(px_width).unwrap_or(h.start);
    commands.trigger(TableColumnResize {
        entity: cell,
        column: h.column.clone(),
        width: Some(width),
    });
}

/// A click on a divider is not a click on its header (no sort, no menu).
fn on_resize_click(mut ev: On<Pointer<Click>>, q: Query<(), With<TableResizeHandle>>) {
    if q.contains(ev.entity) {
        ev.propagate(false);
    }
}

fn on_resize_double_click(
    ev: On<crate::inline_edit::DoubleClick>,
    q: Query<(&TableResizeHandle, &ChildOf)>,
    mut commands: Commands,
) {
    let Ok((h, parent)) = q.get(ev.entity) else {
        return;
    };
    commands.trigger(TableColumnResize {
        entity: parent.parent(),
        column: h.column.clone(),
        width: None,
    });
}

/// The divider shows in the accent colour, full height, while hovered or dragged.
#[allow(clippy::type_complexity)]
fn resize_handle_look(
    q: Query<(&TableResizeHandle, &Hovered, &Children), Or<(Changed<Hovered>, Changed<TableResizeHandle>)>>,
    mut q_line: Query<(&mut Node, &mut BackgroundColor), With<ResizeLine>>,
) {
    for (h, hovered, children) in &q {
        let on = hovered.get() || h.dragging;
        for c in children.iter() {
            if let Ok((mut n, mut bg)) = q_line.get_mut(c) {
                n.top = Val::Percent(if on { 0.0 } else { 20.0 });
                n.height = Val::Percent(if on { 100.0 } else { 60.0 });
                bg.0 = if on { h.active } else { h.idle };
            }
        }
    }
}

/// The wheel over a table body: vertical, and sideways with Shift (or a trackpad's x).
fn on_body_scroll(
    mut ev: On<Pointer<Scroll>>,
    mut q: Query<(&ComputedNode, &mut ScrollPosition), With<TableBody>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
) {
    let Ok((node, mut pos)) = q.get_mut(ev.entity) else {
        return;
    };
    ev.propagate(false);
    let k = match ev.unit {
        MouseScrollUnit::Line => MouseScrollUnit::SCROLL_UNIT_CONVERSION_FACTOR,
        MouseScrollUnit::Pixel => 1.0,
    };
    let mut d = Vec2::new(ev.x, ev.y) * k;
    if keys.is_some_and(|k| k.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight])) {
        d = Vec2::new(d.y, d.x);
    }
    let s = node.inverse_scale_factor();
    let max = ((node.content_size() - node.size()) * s).max(Vec2::ZERO);
    pos.x = (pos.x - d.x).clamp(0.0, max.x);
    pos.y = (pos.y - d.y).clamp(0.0, max.y);
}

/// A header follows the horizontal scroll of its sibling body.
#[allow(clippy::type_complexity)]
fn sync_header_scroll(
    q_body: Query<(&ScrollPosition, &ChildOf), With<TableBody>>,
    q_children: Query<&Children>,
    mut q_header: Query<&mut ScrollPosition, (With<TableHeaderRow>, Without<TableBody>)>,
) {
    for (pos, parent) in &q_body {
        let Ok(children) = q_children.get(parent.parent()) else {
            continue;
        };
        for c in children.iter() {
            if let Ok(mut h) = q_header.get_mut(c)
                && h.x != pos.x
            {
                h.x = pos.x;
            }
        }
    }
}

fn on_header_activate(
    ev: On<Activate>,
    q: Query<&TableHeaderCell>,
    mut commands: Commands,
) {
    let Ok(cell) = q.get(ev.entity) else {
        return;
    };
    if !cell.click_sorts {
        return;
    }
    commands.trigger(TableSortChange {
        entity: ev.entity,
        column: cell.column.clone(),
        sort: cell.next_sort(),
    });
}

fn on_header_double_click(
    ev: On<crate::inline_edit::DoubleClick>,
    q: Query<&TableHeaderCell>,
    mut commands: Commands,
) {
    let Ok(cell) = q.get(ev.entity) else {
        return;
    };
    if !cell.sortable || cell.click_sorts {
        return;
    }
    commands.trigger(TableSortChange {
        entity: ev.entity,
        column: cell.column.clone(),
        sort: cell.next_sort(),
    });
}

fn on_row_click(
    mut click: On<Pointer<Click>>,
    q: Query<Has<InteractionDisabled>, With<TableRowMarker>>,
    mut commands: Commands,
) {
    let Ok(disabled) = q.get(click.entity) else {
        return;
    };
    if click.button != PointerButton::Primary {
        return;
    }
    click.propagate(false);
    if !disabled {
        commands.trigger(Activate {
            entity: click.entity,
        });
    }
}

//! The **configuration table** of a configuration input: one UI, shown both by the node view's
//! input nodes (Input Configuration, Checkbox, Value) and by the Configuration panel
//! ([`spawn_table`]).
//!
//! - **The heading**: "name", "type", then a configuration's name each, a column per
//!   configuration. Clicked, it is shown in the Part Studio (its ✓, its whole column tinted);
//!   double-clicked, renamed; "default" on a List's default one. Its menu: **Show**, **Rename**,
//!   **Set as default**, **Duplicate**, **Move left**, **Move right**, **Delete**. [+] adds a
//!   configuration to a List (a copy of the last).
//! - **A Value's or Checkbox's own `#name`**: a Value's type and its value in the configuration
//!   shown; a Checkbox's 0 and 1.
//! - **A row per output** (a configured parameter, a column of the input's table): its `#name`
//!   (renamed wherever it is read; a part property's column, its label), its type (a free
//!   output's to pick), its value in each configuration (an expression, a bare number taking
//!   the unit; a checkbox for a switch), ×. Its menu: **Move up**, **Move down**, **Remove**.
//! - **+ Output** (a free output: a variable of its own) and the input's removal.
//!
//! A value a feature can't take ([`crate::configuration_ui::reveal_problem`]) is outlined red.
//! The controls carry a [`ConfigRole`], an ancestor the Part Studio ([`ConfigElement`]). Names
//! start with the host's prefix (`node`, `configuration`): `<prefix>-config-<input>-heading`,
//! `-show-<k>`, `-add-row`, `-add-output`, `-remove`; `<prefix>-output-<name>` (a row), `-name`,
//! `-type`, `-<k>` (a cell), `-remove`.

use std::collections::HashMap;

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui::UiGlobalTransform;
use cadrs_core::configuration::{self as cfg, ConfigInput, Configuration, InputKind};
use cadrs_core::ids::{ConfigInputId, ConfigRowId};
use cadrs_core::{ElementId, Feature};
use cadrs_sketch::units::Units;
use cadrs_ui::i18n::tr;
use cadrs_ui::inline_edit::InlineEditHide;
use cadrs_ui::menu::ContextMenuAnchor;
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, DoubleClickable, Select, SelectChange};

use crate::configuration_ui::{ProblemAt, SHOWN_CHECK};
use crate::{ActiveDocument, AppState};

pub struct ConfigTablePlugin;

impl Plugin for ConfigTablePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ColumnScroll>()
            .add_systems(
                Update,
                (reveal_problem_columns, link_columns)
                    .chain()
                    .run_if(in_state(AppState::Document)),
            )
            .add_observer(on_config_button)
            .add_observer(on_config_double_click)
            .add_observer(on_config_renamed)
            .add_observer(on_config_select)
            .add_observer(on_config_text)
            .add_observer(on_config_checkbox)
            .add_observer(on_context_menu)
            .add_observer(on_menu_action);
    }
}

/// What a control of a configuration table is for.
#[derive(Component, Debug, Clone, PartialEq)]
pub(crate) enum ConfigRole {
    /// A node's title: double-clicked, its input renamed.
    Title(ConfigInputId),
    /// A Value's kind and value.
    Kind(ConfigInputId),
    Value(ConfigInputId),
    /// A configuration's heading (its index): clicked, shown; double-clicked, renamed.
    Show(ConfigInputId, usize),
    /// [+]: a configuration (a List's) added, a copy of the last.
    AddRow(ConfigInputId),
    /// + Output: a row of its own, a variable the configurations set.
    AddOutput(ConfigInputId),
    /// An output's row (its menu).
    Output(ConfigInputId, usize),
    /// An output's `#name` (a column of the input's table; `None`: a Value's or Checkbox's own
    /// `#name`), its type (a free output's), its value in a configuration, its removal.
    OutputName(ConfigInputId, Option<usize>),
    OutputKind(ConfigInputId, usize),
    Cell(ConfigInputId, usize, usize),
    RemoveOutput(ConfigInputId, usize),
    RemoveInput(ConfigInputId),
}

/// The Part Studio the configuration table's controls under it change.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct ConfigElement(pub ElementId);

/// The table's columns: the name, the type, a configuration's, the last (a [+] heading, a
/// row's ×), and the gap between them.
const OUT_NAME_W: f32 = 112.0;
const OUT_TYPE_W: f32 = 78.0;
pub(crate) const OUT_CELL_W: f32 = 66.0;
const OUT_END_W: f32 = 22.0;
const OUT_GAP: f32 = 4.0;

/// An input's column widths, as wide as their longest text (an output's name, a value, a
/// configuration's name with its ✓ and "default") so nothing is cut: (name, value), at least
/// [`OUT_NAME_W`] and [`OUT_CELL_W`].
fn input_columns(input: &ConfigInput) -> (f32, f32) {
    // (About 7 px a character at the fields' size, and the field's padding.)
    let fit = |chars: usize, least: f32, most: f32| (chars as f32 * 7.0 + 22.0).clamp(least, most);
    let names = input
        .params
        .iter()
        .map(|p| p.name.chars().count())
        .chain(std::iter::once(input.name.chars().count()))
        .max()
        .unwrap_or(0);
    let values = input
        .params
        .iter()
        .flat_map(|p| &p.values)
        .filter_map(|v| v.as_expr())
        .map(|e| e.chars().count())
        .chain(input.row_names().iter().enumerate().map(|(k, n)| {
            let default = matches!(input.kind, InputKind::List { default, .. } if default == k);
            n.chars().count() + 2 + if default { 7 } else { 0 }
        }))
        .max()
        .unwrap_or(0);
    (
        fit(names, OUT_NAME_W, 220.0),
        fit(values, OUT_CELL_W, 160.0),
    )
}

/// How wide an input's table is with all its configurations in view.
pub(crate) fn table_width(input: &ConfigInput) -> f32 {
    table_width_showing(input, usize::MAX)
}

/// How wide an input's table is showing at most `most` configurations (the others scrolled
/// to).
pub(crate) fn table_width_showing(input: &ConfigInput, most: usize) -> f32 {
    let cells = match input.kind {
        InputKind::Value { .. } => 1,
        _ => input.row_count().min(most),
    } as f32;
    let (name_w, cell_w) = input_columns(input);
    name_w
        + OUT_TYPE_W
        + cells * cell_w
        + OUT_END_W
        + (cells + 3.0) * OUT_GAP
        + 2.0 * (COLUMNS_PAD + 1.0)
}

/// The configurations' columns' padding on their left and right (px), room for the shown
/// one's tint (inside a 1 px border).
const COLUMNS_PAD: f32 = 4.0;

/// The configurations' columns' background: opaque, darker than the panels' (which are glass,
/// so a translucent shade over them hardly shows).
fn columns_background(t: &Theme) -> Color {
    // (Light text: a dark theme.)
    let dark = t.foreground.to_srgba().red > 0.5;
    let k = if dark { 0.6 } else { 0.93 };
    let c = t.glass_panel.to_srgba();
    Color::srgb(c.red * k, c.green * k, c.blue * k)
}

/// The kind an output shows ("length", "switch"…).
fn kind_label(k: cfg::ColumnKind) -> String {
    tr(match k {
        cfg::ColumnKind::Length => "node-view-kind-length",
        cfg::ColumnKind::Angle => "node-view-kind-angle",
        cfg::ColumnKind::Number => "node-view-kind-number",
        cfg::ColumnKind::Boolean => "node-view-kind-switch",
        cfg::ColumnKind::Text => "node-view-kind-text",
    })
}

fn kind_key(k: cfg::ValueKind) -> &'static str {
    match k {
        cfg::ValueKind::Length => "node-view-kind-length",
        cfg::ValueKind::Angle => "node-view-kind-angle",
        cfg::ValueKind::Number => "node-view-kind-number",
    }
}

/// The kinds a free output can be.
const FREE_KINDS: [cfg::ColumnKind; 3] = [
    cfg::ColumnKind::Length,
    cfg::ColumnKind::Angle,
    cfg::ColumnKind::Number,
];

/// A row of the table.
fn row() -> Node {
    Node {
        height: Val::Px(28.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(OUT_GAP),
        flex_shrink: 0.0,
        ..default()
    }
}

/// What a table is built from.
pub(crate) struct TableSpec<'a> {
    /// The start of its names: `node`, `configuration`.
    pub prefix: &'a str,
    pub input: &'a ConfigInput,
    /// The input's value in the configuration shown.
    pub resolved: cfg::Resolved,
    /// The Part Studio's features (what a configured field measures).
    pub features: &'a [Feature],
    pub units: Units,
    /// The value a feature can't take, outlined red.
    pub problem: Option<ProblemAt>,
    /// Where its columns' sideways scroll is kept.
    pub link: ColumnsLink,
}

/// The name of output `j`'s row: `<prefix>-output-<name>` (a property's column, which has no
/// `#name`: `<prefix>-output-<input>-<j>`).
pub(crate) fn output_slug(input: &ConfigInput, j: usize) -> String {
    match input.params.get(j) {
        Some(p) if !p.name.is_empty() => p.name.clone(),
        _ => format!("{}-{}", input.name, j + 1),
    }
}

/// Spawns an input's configuration table under `b` (see the module's comment).
pub(crate) fn spawn_table(b: &mut ChildSpawner, t: &Theme, s: &TableSpec) {
    let (prefix, input) = (s.prefix, s.input);
    let id = input.id;
    let (name_w, cell_w) = input_columns(input);
    let shown = match s.resolved {
        cfg::Resolved::Row(i) => Some(i),
        cfg::Resolved::Checked(c) => Some(usize::from(c)),
        cfg::Resolved::Number(_) => None,
    };
    let default_row = match &input.kind {
        InputKind::List { default, .. } => Some(*default),
        _ => None,
    };
    let is_value = matches!(input.kind, InputKind::Value { .. });
    let configs = if is_value { 0 } else { input.row_count() };
    let cell_node = |w: f32| Node {
        width: Val::Px(w),
        flex_shrink: 0.0,
        height: Val::Px(22.0),
        align_items: AlignItems::Center,
        overflow: Overflow::clip(),
        ..default()
    };
    let label = |r: &mut ChildSpawner, text: String, w: f32, color: Color| {
        r.spawn((
            t.text(text, t.font_sm, FontWeight::MEDIUM, color),
            cell_node(w),
        ))
        .insert(TextLayout::no_wrap());
    };
    let red = |field: TextInput| field.border(t.danger).focus_border(2.0, t.danger);
    let config = format!("{prefix}-config-{}", input.name);
    // The rows' names: the outputs', and a Value's or Checkbox's own first.
    let own = input.defines_variable().then(|| input.name.clone());
    let outs: Vec<String> = (0..input.params.len())
        .map(|j| format!("{prefix}-output-{}", output_slug(input, j)))
        .collect();
    let own_row = own.as_ref().map(|n| format!("{prefix}-output-{n}"));
    // Three blocks side by side, their rows level: the outputs' names and types (fixed), the
    // configurations' columns (scrolled sideways, [`ColumnsLink`]), each row's × (fixed).
    b.spawn((
        Name::new(format!("{config}-table")),
        Node {
            flex_shrink: 0.0,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(OUT_GAP),
            ..default()
        },
    ))
    .with_children(|b| {
        // The names and types.
        b.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|b| {
            b.spawn((Name::new(format!("{config}-heading")), row()))
                .with_children(|r| {
                    label(r, tr("node-view-output-name"), name_w, t.muted_foreground);
                    label(
                        r,
                        tr("node-view-output-type"),
                        OUT_TYPE_W,
                        t.muted_foreground,
                    );
                    if is_value {
                        label(r, tr("node-view-output-value"), cell_w, t.muted_foreground);
                    }
                });
            if let Some(out) = &own_row {
                b.spawn((Name::new(out.clone()), row())).with_children(|r| {
                    r.spawn((
                        ConfigRole::OutputName(id, None),
                        TextInput::new(format!("{out}-name"))
                            .value(input.name.clone())
                            .width(Val::Px(name_w))
                            .height(22.0)
                            .select_all_on_focus()
                            .build(t),
                    ));
                    match &input.kind {
                        InputKind::Value { kind, .. } => {
                            let mut sel =
                                Select::new(format!("{out}-type")).width(Val::Px(OUT_TYPE_W));
                            for k in cfg::ValueKind::ALL {
                                sel = sel.option(tr(kind_key(k)), true);
                            }
                            let at = cfg::ValueKind::ALL
                                .iter()
                                .position(|k| k == kind)
                                .unwrap_or(0);
                            r.spawn((ConfigRole::Kind(id), sel.selected(at).build(t)));
                            let value = match s.resolved {
                                cfg::Resolved::Number(x) => x,
                                _ => 0.0,
                            };
                            let mut field = TextInput::new(format!("{out}-1"))
                                .value(s.units.with_unit(value, kind.quantity()))
                                .width(Val::Px(cell_w))
                                .height(22.0)
                                .select_all_on_focus();
                            if s.problem == Some(ProblemAt::Value(id)) {
                                field = red(field);
                            }
                            r.spawn((ConfigRole::Value(id), field.build(t)));
                        }
                        _ => label(
                            r,
                            kind_label(cfg::ColumnKind::Boolean),
                            OUT_TYPE_W,
                            t.muted_foreground,
                        ),
                    }
                });
            }
            for (j, p) in input.params.iter().enumerate() {
                let k = p.kind(s.features);
                let tip = p
                    .target
                    .label(s.features, &Default::default(), &|_| String::new());
                let out = &outs[j];
                b.spawn((
                    Name::new(out.clone()),
                    row(),
                    ConfigRole::Output(id, j),
                    ContextMenuTarget,
                ))
                .with_children(|r| {
                    if p.binds() {
                        r.spawn((
                            ConfigRole::OutputName(id, Some(j)),
                            TextInput::new(format!("{out}-name"))
                                .value(p.name.clone())
                                .width(Val::Px(name_w))
                                .height(22.0)
                                .select_all_on_focus()
                                .build(t),
                            Tooltip::new(tip.clone()),
                        ));
                    } else {
                        // A part property's column: no `#name`, its label.
                        r.spawn((
                            t.text(tip.clone(), t.font_sm, FontWeight::MEDIUM, t.foreground),
                            cell_node(name_w),
                            Tooltip::new(tip.clone()),
                        ))
                        .insert(TextLayout::no_wrap());
                    }
                    if matches!(p.target, cfg::ParamTarget::Free { .. }) {
                        let mut sel = Select::new(format!("{out}-type")).width(Val::Px(OUT_TYPE_W));
                        for f in FREE_KINDS {
                            sel = sel.option(kind_label(f), true);
                        }
                        let at = FREE_KINDS.iter().position(|f| *f == k).unwrap_or(0);
                        r.spawn((ConfigRole::OutputKind(id, j), sel.selected(at).build(t)));
                    } else {
                        label(r, kind_label(k), OUT_TYPE_W, t.muted_foreground);
                    }
                });
            }
        });
        // The configurations' columns, scrolled sideways under a slim scrollbar.
        let columns_w = (configs as f32 * (cell_w + OUT_GAP) - OUT_GAP).max(0.0);
        b.spawn(Node {
            flex_grow: 1.0,
            flex_shrink: 1.0,
            flex_basis: Val::Px(columns_w + 2.0 * (COLUMNS_PAD + 1.0)),
            min_width: Val::Px(0.0),
            ..default()
        })
        .with_children(|w| {
            let area = w
                .spawn((
                    Name::new(format!("{config}-columns")),
                    Node {
                        width: Val::Percent(100.0),
                        overflow: Overflow::scroll_x(),
                        padding: UiRect::new(
                            Val::Px(COLUMNS_PAD),
                            Val::Px(COLUMNS_PAD),
                            Val::ZERO,
                            Val::Px(8.0),
                        ),
                        border_radius: BorderRadius::all(Val::Px(6.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::all(t.separator),
                    // A shade darker than the rest of the table, so it reads as the part that
                    // scrolls, as wide as there is room for however few columns it has.
                    BackgroundColor(columns_background(t)),
                    s.link,
                    cadrs_ui::WheelScrollX,
                ))
                .with_children(|a| {
                    a.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        flex_shrink: 0.0,
                        min_width: Val::Px(columns_w),
                        ..default()
                    })
                    .with_children(|b| {
                        // (First, so under the headings and cells.)
                        if let Some(k) = shown {
                            b.spawn((
                                Name::new(format!("{config}-shown-column")),
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: Val::Px(k as f32 * (cell_w + OUT_GAP) - 2.0),
                                    width: Val::Px(cell_w + 4.0),
                                    top: Val::Px(0.0),
                                    bottom: Val::Px(0.0),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(t.list_selected),
                                Pickable::IGNORE,
                            ));
                        }
                        b.spawn((Name::new(format!("{config}-configurations")), row()))
                            .with_children(|r| {
                                if is_value {
                                    return;
                                }
                                for (k, n) in input.row_names().into_iter().enumerate() {
                                    heading(r, t, &config, id, k, n, shown, default_row, cell_w);
                                }
                            });
                        if own.is_some() {
                            b.spawn(row()).with_children(|r| {
                                if !is_value {
                                    for v in ["0", "1"] {
                                        label(r, v.into(), cell_w, t.muted_foreground);
                                    }
                                }
                            });
                        }
                        for (j, p) in input.params.iter().enumerate() {
                            let out = &outs[j];
                            b.spawn((
                                Name::new(format!("{out}-values")),
                                row(),
                                ConfigRole::Output(id, j),
                                ContextMenuTarget,
                            ))
                            .with_children(|r| {
                                for (row_at, v) in p.values.iter().enumerate() {
                                    let name = format!("{out}-{}", row_at + 1);
                                    match v {
                                        cfg::CellValue::Bool(on) => {
                                            r.spawn(cell_node(cell_w)).with_children(|c| {
                                                c.spawn((
                                                    ConfigRole::Cell(id, j, row_at),
                                                    cadrs_ui::Checkbox::new(name)
                                                        .checked(*on)
                                                        .build(t),
                                                ));
                                            });
                                        }
                                        v => {
                                            let mut field = TextInput::new(name)
                                                .value(v.as_expr().unwrap_or_default().to_string())
                                                .width(Val::Px(cell_w))
                                                .height(22.0)
                                                .select_all_on_focus();
                                            let at = ProblemAt::Cell {
                                                input: id,
                                                row: row_at,
                                                param: j,
                                            };
                                            if s.problem == Some(at) {
                                                field = red(field);
                                            }
                                            r.spawn((
                                                ConfigRole::Cell(id, j, row_at),
                                                field.build(t),
                                            ));
                                        }
                                    }
                                }
                            });
                        }
                    });
                })
                .id();
            w.spawn(cadrs_ui::horizontal_scrollbar(
                &format!("{config}-scrollbar"),
                area,
            ));
        });
        // [+] and each row's ×.
        b.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            width: Val::Px(OUT_END_W),
            ..default()
        })
        .with_children(|b| {
            b.spawn(row()).with_children(|r| {
                if matches!(input.kind, InputKind::List { .. }) {
                    r.spawn((
                        ConfigRole::AddRow(id),
                        IconButton::new(format!("{config}-add-row"), "plus")
                            .icon_size(14.0)
                            .tooltip(tr("node-view-add-row"))
                            .build(t),
                    ));
                }
            });
            if own.is_some() {
                b.spawn(row());
            }
            for (j, out) in outs.iter().enumerate() {
                b.spawn(row()).with_children(|r| {
                    r.spawn((
                        ConfigRole::RemoveOutput(id, j),
                        IconButton::new(format!("{out}-remove"), "close")
                            .icon_size(12.0)
                            .tooltip(tr("node-view-remove-output"))
                            .build(t),
                    ));
                });
            }
        });
    });
    // + Output (not a Value's: it is one), and the input's removal.
    b.spawn(Node {
        height: Val::Px(30.0),
        align_items: AlignItems::Center,
        justify_content: JustifyContent::SpaceBetween,
        flex_shrink: 0.0,
        ..default()
    })
    .with_children(|r| {
        if is_value {
            r.spawn(Node::default());
        } else {
            r.spawn((
                ConfigRole::AddOutput(id),
                cadrs_ui::Button::new(format!("{config}-add-output"))
                    .label(tr("node-view-add-output"))
                    .icon("plus")
                    .small()
                    .ghost()
                    .build(t),
            ));
        }
        r.spawn((
            ConfigRole::RemoveInput(id),
            IconButton::new(format!("{config}-remove"), "delete")
                .icon_size(14.0)
                .tooltip(tr("node-view-remove-input"))
                .build(t),
        ));
    });
}

/// Configuration `k`'s heading: its name (clicked: shown; double-clicked: renamed), a ✓ on the
/// one shown, "default" on a List's default.
#[allow(clippy::too_many_arguments)]
fn heading(
    r: &mut ChildSpawner,
    t: &Theme,
    config: &str,
    id: ConfigInputId,
    k: usize,
    name: String,
    shown: Option<usize>,
    default_row: Option<usize>,
    cell_w: f32,
) {
    let button = cadrs_ui::Button::new(format!("{config}-show-{}", k + 1))
        .label(name)
        .small()
        .ghost();
    let mut e = r.spawn((
        ConfigRole::Show(id, k),
        DoubleClickable,
        ContextMenuTarget,
        cadrs_ui::InlineEdit::default(),
        button.build(t),
        Tooltip::new(tr("node-view-show-configuration")),
    ));
    if let Some(mut n) = e.get_mut::<Node>() {
        n.width = Val::Px(cell_w);
        n.flex_shrink = 0.0;
        n.column_gap = Val::Px(3.0);
        n.padding = UiRect::horizontal(Val::Px(2.0));
        n.overflow = Overflow::clip();
    }
    let button = e.id();
    if shown == Some(k) {
        // The ✓ before the name (green whatever the button's hover).
        e.world_scope(|w| {
            let check = w
                .spawn((
                    cadrs_ui::icon::icon_in(
                        "check",
                        12.0,
                        SHOWN_CHECK,
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ),
                    InlineEditHide,
                    Pickable::IGNORE,
                ))
                .id();
            w.entity_mut(button).insert_children(0, &[check]);
        });
    }
    if default_row == Some(k) {
        e.with_children(|c| {
            c.spawn((
                t.text(
                    tr("config-default-tag"),
                    10.0,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                InlineEditHide,
                Pickable::IGNORE,
            ))
            .insert(TextLayout::no_wrap());
        });
    }
}

// ---------------------------------------------------------------------------------------------
// Sideways scrolling

/// The configurations' columns of a table (an input's, in the Configuration panel or the node
/// view), each scrolled sideways on its own. Where it is scrolled to is kept by this key
/// ([`ColumnScroll`]), so a table built again (after every edit) stays where it was.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ColumnsLink {
    pub element: ElementId,
    pub input: ConfigInputId,
    /// The panel's (the node view's otherwise).
    pub panel: bool,
}

/// How far each table's columns are scrolled (px), and what was last set on each area.
#[derive(Resource, Debug, Default)]
struct ColumnScroll {
    x: HashMap<ColumnsLink, f32>,
    set: HashMap<Entity, f32>,
}

/// Keeps each table's columns where they were scrolled to, across the table being built again.
fn link_columns(
    mut scroll: ResMut<ColumnScroll>,
    mut q: Query<(Entity, &ColumnsLink, &ComputedNode, &mut ScrollPosition)>,
) {
    let ColumnScroll { x, set } = &mut *scroll;
    // Scrolled by hand (the wheel, the scrollbar): kept.
    for (e, link, _, pos) in &q {
        if let Some(was) = set.get(&e)
            && (pos.x - was).abs() > 0.5
        {
            x.insert(*link, pos.x);
        }
    }
    let mut now = HashMap::new();
    for (e, link, node, mut pos) in &mut q {
        if node.size().x <= 0.0 {
            continue;
        }
        let s = node.inverse_scale_factor();
        let max = ((node.content_size().x - node.size().x) * s).max(0.0);
        let want = x.get(link).copied().unwrap_or(0.0).clamp(0.0, max);
        if (pos.x - want).abs() > 0.01 {
            pos.x = want;
        }
        now.insert(e, want);
    }
    *set = now;
}

/// Scrolls the columns holding a value a feature can't take so it is in view, for a few frames
/// after it is shown (the panel and the node view build their tables then).
#[allow(clippy::type_complexity)]
fn reveal_problem_columns(
    problem: Option<Res<crate::configuration_ui::ConfigProblem>>,
    mut frames: Local<u32>,
    mut scroll: ResMut<ColumnScroll>,
    q_roles: Query<(Entity, &ConfigRole, &ComputedNode, &UiGlobalTransform)>,
    q_parent: Query<&ChildOf>,
    q_area: Query<(
        &ColumnsLink,
        &ComputedNode,
        &UiGlobalTransform,
        &ScrollPosition,
    )>,
) {
    let Some(problem) = problem else {
        return;
    };
    if problem.is_changed() {
        *frames = 0;
    }
    if *frames > 30 {
        return;
    }
    *frames += 1;
    let at = problem.at;
    for (e, role, node, g) in &q_roles {
        let hit = match (role, at) {
            (ConfigRole::Cell(i, j, k), ProblemAt::Cell { input, row, param }) => {
                *i == input && *j == param && *k == row
            }
            _ => false,
        };
        if !hit || node.size().x <= 0.0 {
            continue;
        }
        let Some((link, area, area_g, pos)) =
            q_parent.iter_ancestors(e).find_map(|a| q_area.get(a).ok())
        else {
            continue;
        };
        let s = area.inverse_scale_factor();
        let cell = Rect::from_center_size(g.translation * s, node.size() * s);
        let shown = Rect::from_center_size(area_g.translation * s, area.size() * s);
        let margin = 8.0_f32.min(shown.width() / 4.0);
        let want = if cell.min.x < shown.min.x + margin {
            pos.x - (shown.min.x + margin - cell.min.x)
        } else if cell.max.x > shown.max.x - margin {
            pos.x + (cell.max.x - (shown.max.x - margin))
        } else {
            continue;
        };
        scroll.x.insert(*link, want.max(0.0));
    }
}

// ---------------------------------------------------------------------------------------------
// Edits

/// A control's role, and its Part Studio, for an entity in a table.
pub(crate) fn config_role(world: &World, e: Entity) -> Option<(ElementId, ConfigRole)> {
    let mut cur = e;
    let mut role = None;
    for _ in 0..16 {
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

/// Runs a command; an error shows as a toast, and the tables are built again as they were.
pub(crate) fn run(world: &mut World, cmd: &dyn cadrs_core::Command) {
    if let Some(mut d) = world.get_resource_mut::<ActiveDocument>()
        && let Err(e) = d.execute(cmd)
    {
        toast(world, e.to_string());
        world.resource_mut::<ActiveDocument>().set_changed();
    }
}

pub(crate) fn toast(world: &mut World, why: String) {
    let t = world.resource::<Theme>().clone();
    cadrs_ui::show_toast(&mut world.commands(), &t, why);
}

pub(crate) fn config_of(world: &World, element: ElementId) -> Option<(Configuration, Vec<String>)> {
    let el = world.resource::<ActiveDocument>().doc.element(element)?;
    Some((el.configuration.clone(), cfg::taken_names(el)))
}

/// The configuration with `f` applied, as one edit labelled `label` (`f` false: nothing to do).
pub(crate) fn edit_config(
    world: &mut World,
    element: ElementId,
    label: &str,
    f: impl FnOnce(&mut Configuration, &[String]) -> bool,
) {
    let Some((mut c, taken)) = config_of(world, element) else {
        return;
    };
    if !f(&mut c, &taken) {
        return;
    }
    run(
        world,
        &cfg::EditConfiguration {
            element,
            configuration: c,
            label: label.into(),
        },
    );
}

/// Shows `value` of `input` in the Part Studio.
pub(crate) fn show_value(
    world: &mut World,
    element: ElementId,
    input: ConfigInputId,
    value: cfg::InputValue,
) {
    let mut values = cfg::ConfigValues::default();
    values.set(input, value);
    run(world, &cfg::SetActiveConfiguration { element, values });
}

/// Shows configuration `k` of `input` (a List's row, a Checkbox's state).
fn show_configuration(world: &mut World, element: ElementId, input: ConfigInputId, k: usize) {
    let Some(value) = config_of(world, element).and_then(|(c, _)| match &c.input(input)?.kind {
        InputKind::List { rows, .. } => rows.get(k).map(|r| cfg::InputValue::Row(r.id)),
        InputKind::Checkbox { .. } => Some(cfg::InputValue::Checked(k == 1)),
        InputKind::Value { .. } => None,
    }) else {
        return;
    };
    show_value(world, element, input, value);
}

/// Opens a node's title to rename its input.
pub(crate) fn begin_title_edit(world: &mut World, e: Entity, input: ConfigInputId) {
    let Some(element) = config_role(world, e).map(|(el, _)| el) else {
        return;
    };
    let Some(name) =
        config_of(world, element).and_then(|(c, _)| Some(c.input(input)?.name.clone()))
    else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let mut opts = cadrs_ui::InlineEditOptions::new("node-config-title-edit");
    opts.width = Val::Px(140.0);
    opts.height = 22.0;
    opts.font_size = Some(theme.font_base);
    opts.weight = FontWeight::BOLD;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    cadrs_ui::begin_inline_edit(&mut commands, &theme, e, name, opts);
    world.flush();
}

/// A configuration heading's label (its button's text), for renaming it in place.
fn begin_heading_edit(world: &mut World, button: Entity, input: ConfigInputId, k: usize) {
    let Some(element) = config_role(world, button).map(|(el, _)| el) else {
        return;
    };
    let Some(name) =
        config_of(world, element).and_then(|(c, _)| c.input(input)?.row_names().get(k).cloned())
    else {
        return;
    };
    // (The button's text is its label, hidden while the field is shown.)
    let text = world
        .get::<Children>(button)
        .map(|c| c.to_vec())
        .unwrap_or_default()
        .into_iter()
        .find(|e| world.get::<Text>(*e).is_some());
    let Some(text) = text else { return };
    world.entity_mut(text).insert(cadrs_ui::InlineEditLabel);
    let width = world
        .get::<ComputedNode>(button)
        .map(|n| n.size().x * n.inverse_scale_factor())
        .filter(|w| *w > 0.0)
        .unwrap_or(OUT_CELL_W);
    let theme = world.resource::<Theme>().clone();
    let mut opts = cadrs_ui::InlineEditOptions::new("node-config-heading-edit");
    opts.width = Val::Px(width - 4.0);
    opts.height = 20.0;
    opts.font_size = Some(theme.font_sm);
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    cadrs_ui::begin_inline_edit(&mut commands, &theme, button, name, opts);
    commands.entity(button).insert(HeadingEdit(input, k));
    world.flush();
}

/// A configuration heading being renamed in place.
#[derive(Component, Debug, Clone, Copy)]
struct HeadingEdit(ConfigInputId, usize);

fn on_config_double_click(ev: On<cadrs_ui::DoubleClick>, mut commands: Commands) {
    let e = ev.entity;
    commands.queue(move |world: &mut World| {
        if let Some(ConfigRole::Show(input, k)) = world.get::<ConfigRole>(e).cloned() {
            begin_heading_edit(world, e, input, k);
            return;
        }
        // The title: its row is the field's owner.
        let owner = world.get::<ChildOf>(e).map(|c| c.parent());
        if let Some(owner) = owner
            && let Some(ConfigRole::Title(input)) = world.get::<ConfigRole>(owner).cloned()
        {
            begin_title_edit(world, owner, input);
        }
    });
}

/// A title or configuration heading renamed in place.
fn on_config_renamed(
    ev: On<cadrs_ui::InlineEditCommit>,
    q_role: Query<&ConfigRole>,
    q_heading: Query<&HeadingEdit>,
    mut commands: Commands,
) {
    let e = ev.entity;
    let name = ev.value.trim().to_string();
    let title = match q_role.get(e) {
        Ok(ConfigRole::Title(i)) => Some(*i),
        _ => None,
    };
    let heading = q_heading.get(e).ok().copied();
    if title.is_none() && heading.is_none() {
        return;
    }
    commands.queue(move |world: &mut World| {
        if heading.is_some()
            && let Ok(mut em) = world.get_entity_mut(e)
        {
            em.remove::<HeadingEdit>();
        }
        let Some((element, _)) = config_role(world, e) else {
            return;
        };
        if name.is_empty() {
            world.resource_mut::<ActiveDocument>().set_changed();
            return;
        }
        if let Some(input) = title {
            rename_input(world, element, input, name);
        } else if let Some(HeadingEdit(input, k)) = heading {
            edit_config(world, element, "Rename configuration", |c, _| {
                match c.input_mut(input).map(|i| &mut i.kind) {
                    Some(InputKind::List { rows, .. }) => match rows.get_mut(k) {
                        Some(r) if r.name != name => {
                            r.name = name;
                            true
                        }
                        _ => false,
                    },
                    _ => false,
                }
            });
        }
    });
}

/// Renames an input: a List's title, a Value's or Checkbox's `#name` (wherever it is read).
pub(crate) fn rename_input(
    world: &mut World,
    element: ElementId,
    input: ConfigInputId,
    to: String,
) {
    let Some(i) = config_of(world, element).and_then(|(c, _)| c.input(input).cloned()) else {
        return;
    };
    if i.name == to {
        return;
    }
    if i.defines_variable() {
        run(
            world,
            &cfg::RenameConfigName {
                element,
                from: i.name,
                to,
            },
        );
    } else {
        edit_config(world, element, "Rename configuration input", |c, _| {
            let Some(i) = c.input_mut(input) else {
                return false;
            };
            i.name = to;
            true
        });
    }
}

/// Adds a configuration to a List: after row `copy`, a copy of it ("M8 copy"), or at the end a
/// copy of the last.
fn add_row(world: &mut World, element: ElementId, input: ConfigInputId, copy: Option<usize>) {
    edit_config(world, element, "Add configuration", |c, _| {
        let Some(i) = c.input_mut(input) else {
            return false;
        };
        let InputKind::List { rows, .. } = &mut i.kind else {
            return false;
        };
        let taken: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        let free = |base: String| {
            std::iter::once(base.clone())
                .chain((2..).map(|n| format!("{base}{n}")))
                .find(|n| !taken.contains(n))
                .unwrap_or_default()
        };
        let (from, at, name) = match copy {
            Some(r) if r < rows.len() => (r, r + 1, free(format!("{} copy", rows[r].name))),
            _ => {
                let name = (rows.len() + 1..)
                    .map(|n| format!("config{n}"))
                    .find(|n| !taken.contains(n))
                    .unwrap_or_default();
                (rows.len().saturating_sub(1), rows.len(), name)
            }
        };
        rows.insert(
            at,
            cfg::ListRow {
                id: ConfigRowId::new(),
                name,
            },
        );
        for p in &mut i.params {
            let v = p
                .values
                .get(from)
                .cloned()
                .unwrap_or(cfg::CellValue::Expr(String::new()));
            p.values.insert(at.min(p.values.len()), v);
        }
        true
    });
}

/// Moves configuration `row` of a List by `by` (−1 left, +1 right), its values with it.
fn move_row(c: &mut Configuration, input: ConfigInputId, row: usize, by: isize) -> bool {
    let Some(i) = c.input_mut(input) else {
        return false;
    };
    let InputKind::List { rows, default } = &mut i.kind else {
        return false;
    };
    let to = row as isize + by;
    if to < 0 || to as usize >= rows.len() {
        return false;
    }
    let to = to as usize;
    rows.swap(row, to);
    for p in &mut i.params {
        if to < p.values.len() && row < p.values.len() {
            p.values.swap(row, to);
        }
    }
    if *default == row {
        *default = to;
    } else if *default == to {
        *default = row;
    }
    true
}

fn delete_row(c: &mut Configuration, input: ConfigInputId, row: usize) -> bool {
    let Some(i) = c.input_mut(input) else {
        return false;
    };
    let InputKind::List { rows, default } = &mut i.kind else {
        return false;
    };
    if rows.len() < 2 || row >= rows.len() {
        return false;
    }
    rows.remove(row);
    for p in &mut i.params {
        if row < p.values.len() {
            p.values.remove(row);
        }
    }
    if *default > row || *default >= rows.len() {
        *default = default.saturating_sub(1);
    }
    true
}

/// Moves output `param` by `by` (−1 up, +1 down).
fn move_output(c: &mut Configuration, input: ConfigInputId, param: usize, by: isize) -> bool {
    let Some(i) = c.input_mut(input) else {
        return false;
    };
    let to = param as isize + by;
    if to < 0 || to as usize >= i.params.len() {
        return false;
    }
    i.params.swap(param, to as usize);
    true
}

/// Takes output `j` out: a free one (its readers keep `#name`, which no longer resolves); a
/// configured field's or dimension's, unconfigured (it keeps the value it shows).
fn remove_output(world: &mut World, element: ElementId, input: ConfigInputId, j: usize) {
    let Some(target) = config_of(world, element)
        .and_then(|(c, _)| Some(c.input(input)?.params.get(j)?.target.clone()))
    else {
        return;
    };
    if matches!(target, cfg::ParamTarget::Free { .. }) {
        edit_config(
            world,
            element,
            "Remove configuration output",
            |c, _| match c.input_mut(input) {
                Some(i) if j < i.params.len() => {
                    i.params.remove(j);
                    true
                }
                _ => false,
            },
        );
    } else {
        run(world, &cfg::UnconfigureParameter { element, target });
    }
}

// ---------------------------------------------------------------------------------------------
// Observers

fn on_config_button(a: On<Activate>, mut commands: Commands) {
    let e = a.entity;
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        match role {
            ConfigRole::Show(input, k) => show_configuration(world, element, input, k),
            ConfigRole::AddRow(input) => add_row(world, element, input, None),
            ConfigRole::AddOutput(input) => {
                let Some((c, taken)) = config_of(world, element) else {
                    return;
                };
                let name = c.fresh_name("output", &taken);
                run(
                    world,
                    &cfg::ConfigureParameter {
                        element,
                        input,
                        target: cfg::ParamTarget::Free {
                            kind: cfg::ColumnKind::Length,
                        },
                        name: Some(name),
                    },
                );
            }
            ConfigRole::RemoveOutput(input, j) => remove_output(world, element, input, j),
            ConfigRole::RemoveInput(input) => {
                edit_config(world, element, "Remove configuration input", |c, _| {
                    let n = c.inputs.len();
                    c.inputs.retain(|i| i.id != input);
                    c.inputs.len() != n
                })
            }
            _ => {}
        }
    });
}

fn on_config_checkbox(ev: On<CheckboxChange>, mut commands: Commands) {
    let (e, checked) = (ev.entity, ev.checked);
    commands.queue(move |world: &mut World| {
        // A switch's value (a suppression) in a configuration.
        if let Some((element, ConfigRole::Cell(input, j, row))) = config_role(world, e) {
            edit_config(world, element, "Edit configuration table", |c, _| {
                let Some(v) = c
                    .input_mut(input)
                    .and_then(|i| i.params.get_mut(j))
                    .and_then(|p| p.values.get_mut(row))
                else {
                    return false;
                };
                *v = cfg::CellValue::Bool(checked);
                true
            })
        }
    });
}

fn on_config_select(ev: On<SelectChange>, mut commands: Commands) {
    let (e, index) = (ev.entity, ev.index);
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        match role {
            ConfigRole::Kind(input) => {
                let Some(k) = cfg::ValueKind::ALL.get(index).copied() else {
                    return;
                };
                edit_config(world, element, "Edit configuration input", |c, _| {
                    match c.input_mut(input).map(|i| &mut i.kind) {
                        Some(InputKind::Value { kind, .. }) if *kind != k => {
                            *kind = k;
                            true
                        }
                        _ => false,
                    }
                });
            }
            // A free output's type: a value that is a number and a unit kept as the number in
            // the new one ("3 mm" → "3 deg").
            ConfigRole::OutputKind(input, j) => {
                let Some(k) = FREE_KINDS.get(index).copied() else {
                    return;
                };
                edit_config(world, element, "Edit configuration output", |c, _| {
                    let Some(p) = c.input_mut(input).and_then(|i| i.params.get_mut(j)) else {
                        return false;
                    };
                    let cfg::ParamTarget::Free { kind } = &mut p.target else {
                        return false;
                    };
                    *kind = k;
                    for v in &mut p.values {
                        if let cfg::CellValue::Expr(e) = v {
                            let mut words = e.split_whitespace();
                            let number = words.next().and_then(|w| w.parse::<f64>().ok());
                            if let Some(x) = number
                                && words.count() <= 1
                            {
                                *e = cfg::plain(x, k.quantity());
                            }
                        }
                    }
                    true
                });
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
            // An output's `#name`, renamed wherever it is read.
            ConfigRole::OutputName(input, j) => {
                let Some(from) = config_of(world, element).and_then(|(c, _)| {
                    let i = c.input(input)?;
                    Some(match j {
                        Some(j) => i.params.get(j)?.name.clone(),
                        None => i.name.clone(),
                    })
                }) else {
                    return;
                };
                if from != value {
                    run(
                        world,
                        &cfg::RenameConfigName {
                            element,
                            from,
                            to: value,
                        },
                    );
                }
            }
            // A Value's value (the configuration shown).
            ConfigRole::Value(input) => {
                let Some(kind) =
                    config_of(world, element).and_then(|(c, _)| match c.input(input)?.kind {
                        InputKind::Value { kind, .. } => Some(kind),
                        _ => None,
                    })
                else {
                    return;
                };
                let units = world.resource::<ActiveDocument>().doc.units;
                match units.eval(&value, kind.quantity()) {
                    Ok(v) => show_value(world, element, input, cfg::InputValue::Number(v)),
                    Err(err) => {
                        toast(world, err.to_string());
                        world.resource_mut::<ActiveDocument>().set_changed();
                    }
                }
            }
            // An output's value in a configuration: a property's text as typed (empty: not
            // set), an expression with a bare number taking the unit.
            ConfigRole::Cell(input, j, row) => {
                let Some(el) = world.resource::<ActiveDocument>().doc.element(element) else {
                    return;
                };
                let features = el.features().to_vec();
                let units = world.resource::<ActiveDocument>().doc.units;
                edit_config(world, element, "Edit configuration table", |c, _| {
                    let Some(p) = c.input_mut(input).and_then(|i| i.params.get_mut(j)) else {
                        return false;
                    };
                    let kind = p.kind(&features);
                    let new = match kind {
                        cfg::ColumnKind::Text => value.clone(),
                        _ if value.is_empty() => return false,
                        cfg::ColumnKind::Length => {
                            cadrs_core::variables::VariableType::Length.with_unit(&value, &units)
                        }
                        cfg::ColumnKind::Angle => {
                            cadrs_core::variables::VariableType::Angle.with_unit(&value, &units)
                        }
                        _ => value.clone(),
                    };
                    let Some(v) = p.values.get_mut(row) else {
                        return false;
                    };
                    let new = cfg::CellValue::Expr(new);
                    if *v == new {
                        return false;
                    }
                    *v = new;
                    true
                });
                // (Shown again as it was when nothing changed: an empty value.)
                world.resource_mut::<ActiveDocument>().set_changed();
            }
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------------------------
// Menus

/// What an open menu of a table is for: a configuration's heading (the button, to rename it in
/// place), an output's row.
#[derive(Component, Debug, Clone, Copy)]
enum TableMenu {
    Configuration(ElementId, ConfigInputId, usize, Entity),
    Output(ElementId, ConfigInputId, usize),
}

fn on_context_menu(ev: On<ContextMenuRequested>, mut commands: Commands) {
    let (e, at) = (ev.entity, ev.position);
    commands.queue(move |world: &mut World| {
        let Some((element, role)) = config_role(world, e) else {
            return;
        };
        let Some((c, _)) = config_of(world, element) else {
            return;
        };
        let menu = Menu::new("config-table-menu")
            .min_width(180.0)
            .item_height(22.0);
        let (menu, what) = match role {
            ConfigRole::Show(input, k) => {
                let Some(i) = c.input(input) else { return };
                let n = i.row_count();
                let m = menu.item(MenuItem::new("config-table-show", tr("config-show")));
                let m = if matches!(i.kind, InputKind::List { .. }) {
                    m.item(MenuItem::new("config-table-rename", tr("common-rename")))
                        .item(MenuItem::new(
                            "config-table-default",
                            tr("config-set-default"),
                        ))
                        .item(MenuItem::new(
                            "config-table-duplicate",
                            tr("common-duplicate"),
                        ))
                        .item(
                            MenuItem::new("config-table-left", tr("config-move-left"))
                                .disabled(k == 0),
                        )
                        .item(
                            MenuItem::new("config-table-right", tr("config-move-right"))
                                .disabled(k + 1 >= n),
                        )
                        .separator()
                        .item(
                            MenuItem::new("config-table-delete", tr("common-delete"))
                                .icon("remove-circle")
                                .disabled(n < 2),
                        )
                } else {
                    m
                };
                (m, TableMenu::Configuration(element, input, k, e))
            }
            ConfigRole::Output(input, j) => {
                let Some(i) = c.input(input) else { return };
                let m = menu
                    .item(MenuItem::new("config-table-up", tr("config-move-up")).disabled(j == 0))
                    .item(
                        MenuItem::new("config-table-down", tr("config-move-down"))
                            .disabled(j + 1 >= i.params.len()),
                    )
                    .separator()
                    .item(
                        MenuItem::new("config-table-remove", tr("config-remove-column"))
                            .icon("remove-circle"),
                    );
                (m, TableMenu::Output(element, input, j))
            }
            _ => return,
        };
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
        commands
            .entity(anchor)
            .insert((what, DespawnOnExit(AppState::Document)));
        world.flush();
    });
}

fn on_menu_action(
    ev: On<MenuAction>,
    q: Query<&TableMenu, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(what) = q.get(ev.entity).copied() else {
        return;
    };
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| match (what, item.as_str()) {
        (TableMenu::Configuration(el, input, k, _), "config-table-show") => {
            show_configuration(world, el, input, k)
        }
        (TableMenu::Configuration(_, input, k, button), "config-table-rename") => {
            if world.get_entity(button).is_ok() {
                begin_heading_edit(world, button, input, k);
            }
        }
        (TableMenu::Configuration(el, input, k, _), "config-table-default") => {
            edit_config(world, el, "Set default configuration", |c, _| {
                match c.input_mut(input).map(|i| &mut i.kind) {
                    Some(InputKind::List { default, .. }) if *default != k => {
                        *default = k;
                        true
                    }
                    _ => false,
                }
            })
        }
        (TableMenu::Configuration(el, input, k, _), "config-table-duplicate") => {
            add_row(world, el, input, Some(k))
        }
        (TableMenu::Configuration(el, input, k, _), "config-table-left" | "config-table-right") => {
            let by = if item.ends_with("left") { -1 } else { 1 };
            edit_config(world, el, "Move configuration", |c, _| {
                move_row(c, input, k, by)
            })
        }
        (TableMenu::Configuration(el, input, k, _), "config-table-delete") => {
            edit_config(world, el, "Delete configuration", |c, _| {
                delete_row(c, input, k)
            })
        }
        (TableMenu::Output(el, input, j), "config-table-up" | "config-table-down") => {
            let by = if item.ends_with("up") { -1 } else { 1 };
            edit_config(world, el, "Move configured parameter", |c, _| {
                move_output(c, input, j, by)
            })
        }
        (TableMenu::Output(el, input, j), "config-table-remove") => {
            remove_output(world, el, input, j)
        }
        _ => {}
    });
}

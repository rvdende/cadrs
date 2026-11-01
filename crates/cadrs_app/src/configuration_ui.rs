//! **Configurations** in the app (Onshape's Configuration panel; the model is
//! [`cadrs_core::configuration`]).
//!
//! - **The Configuration panel** (the tab bar's `panel-configuration`, icon `configurations`;
//!   Part Studios only) docks beside the viewport. **Add input** adds a **List** or a
//!   **Checkbox**, its name open for editing. Each input has a section:
//!   - a **List**: its **configuration table**, one row per configuration (its name, edited
//!     in place; a check on the one the Part Studio shows; "default" on the default one) and a
//!     column per configured parameter, each cell an expression (Enter commits, one undo step)
//!     or, for suppression, a checkbox. A double-click on a row (or its menu's **Show**) makes
//!     it the Part Studio's configuration. **Add configuration** adds a row (a copy of the
//!     last). The row menu: **Show**, **Rename**, **Set as default**, **Duplicate**,
//!     **Move up**, **Move down**, **Delete**.
//!   - a **Checkbox**: its state (the Part Studio's configuration) and the same table with
//!     two rows, Unchecked and Checked.
//!
//!   Values typed in are Variable features (or a Variable Studio's): a List's column can set a
//!   Variable feature's value.
//!
//!   An input's title has a menu: **Rename**, **Move up**, **Move down**, **Delete**, and for a
//!   Checkbox **Checked by default**. A column's heading has **Move left**, **Move right** and
//!   **Remove** (the field keeps the value it shows).
//! - **Configure ▸** on a feature's menu ([`configure_entries`]): its fields, a sketch's
//!   dimensions or a Variable's value, each adding a column to a List or Checkbox (a ✓ on the
//!   configured ones: picking one again removes its column). Dynamic suppression ▸
//!   **Suppress by configuration** configures whether it is suppressed. With no List or
//!   Checkbox yet, a List named "Configuration" is made, with the row "Default".
//! - **Assembly instances**: the instance menu's **Configuration ▸** ([`instance_menu_item`])
//!   picks each input's value for the selected instances of the same Part Studio (a List's
//!   rows, a Checkbox). Instances inserted from a
//!   configured Part Studio take its configuration then.
//!
//! Names: the panel `configuration-panel` (`configuration-panel-close`), `configuration-add-input`
//! (its menu's `configuration-add-list`, `-checkbox`), each input `configuration-input-<k>`
//! (with `-checked` for a Checkbox, `-add-row` for a List, the columns `-col-<j>`, the rows
//! `-row-<r>` with their cells `-row-<r>-cell-<j>`), the name editor `configuration-name`.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::InstanceSource;
use cadrs_core::configuration::{
    self as cfg, CellValue, ConfigInput, ConfigValues, Configuration, EditConfiguration, InputKind,
    InputValue, ListRow, ParamTarget, SetActiveConfiguration, SetInstanceConfiguration,
    UnconfigureParameter,
};
use cadrs_core::document::Element;
use cadrs_core::variables::VariableType;
use cadrs_core::{ConfigInputId, ConfigRowId, FeatureId};
use cadrs_sketch::units::{Quantity, Units};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::menu::{ContextMenuAnchor, MenuEntry};
use cadrs_ui::prelude::*;
use cadrs_ui::{
    CheckboxChange, DoubleClick, DoubleClickable, InlineEditOptions, begin_inline_edit,
};

use crate::side_panel::{SidePanel, side_panel_header, side_panel_node};
use crate::viewport::{ActiveKind, ViewportArea};
use crate::{ActiveDocument, AppState};

pub struct ConfigurationPlugin;

impl Plugin for ConfigurationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_panel, sync_button)
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_observer(on_button)
        .add_observer(on_rename)
        .add_observer(on_text)
        .add_observer(on_check)
        .add_observer(on_double_click)
        .add_observer(on_context_menu)
        .add_observer(on_menu_action)
        .add_observer(on_insert_button)
        .add_observer(on_insert_menu)
        .add_observer(on_insert_check);
    }
}

#[derive(Component)]
struct ConfigurationPanel;

/// What the panel was built from.
#[derive(Component, Debug, Clone, PartialEq)]
struct PanelKey(String);

/// An input's title row.
#[derive(Component, Debug, Clone, Copy)]
struct InputTitle(ConfigInputId);

/// A row of an input's table: a List's configuration, a Checkbox's state (0, 1).
#[derive(Component, Debug, Clone, Copy)]
struct TableRow {
    input: ConfigInputId,
    row: usize,
}

/// A column's heading.
#[derive(Component, Debug, Clone, Copy)]
struct ColumnHead {
    input: ConfigInputId,
    param: usize,
}

/// A cell of an input's table.
#[derive(Component, Debug, Clone, Copy)]
struct Cell {
    input: ConfigInputId,
    row: usize,
    param: usize,
}

/// A Checkbox input's state (the Part Studio's configuration).
#[derive(Component, Debug, Clone, Copy)]
struct ActiveCheck(ConfigInputId);

/// A List's Add configuration button.
#[derive(Component, Debug, Clone, Copy)]
struct AddRow(ConfigInputId);

/// What an open menu of the panel is for.
#[derive(Component, Debug, Clone, Copy)]
enum PanelMenu {
    AddInput,
    Input(ConfigInputId),
    Row(ConfigInputId, usize),
    Column(ConfigInputId, usize),
}

/// A name to open for editing once the panel shows it (an input or row just added).
#[derive(Resource, Debug, Clone, Copy)]
enum PendingName {
    Input(ConfigInputId),
    Row(ConfigInputId, usize),
}

/// A column's width (px).
const COL_W: f32 = 84.0;
/// The row-name column's width (px).
const NAME_W: f32 = 120.0;

/// The active Part Studio, if it is one.
fn studio(world: &World) -> Option<(ElementId, Element)> {
    let d = world.get_resource::<ActiveDocument>()?;
    let el = d.active_element()?;
    matches!(el.kind, cadrs_core::ElementKind::PartStudio { .. }).then(|| (el.id, el.clone()))
}

fn units(world: &World) -> Units {
    world.resource::<crate::WorkspaceUnits>().0
}

/// Runs a command; an error shows as a toast. Whether it was applied.
fn run(world: &mut World, cmd: &dyn cadrs_core::Command) -> bool {
    match world.resource_mut::<ActiveDocument>().execute(cmd) {
        Ok(()) => true,
        Err(e) => {
            crate::linked::error_toast(world, e.to_string());
            false
        }
    }
}

/// Edits the active Part Studio's configuration as one undo step (`f` false: nothing to do).
fn edit(world: &mut World, label: &str, f: impl FnOnce(&mut Configuration, &Element) -> bool) {
    let Some((element, el)) = studio(world) else {
        return;
    };
    let mut c = el.configuration.clone();
    if !f(&mut c, &el) {
        return;
    }
    run(
        world,
        &EditConfiguration {
            element,
            configuration: c,
            label: label.into(),
        },
    );
}

/// Shows `values` (merged over the active ones) in the active Part Studio.
fn activate(world: &mut World, input: ConfigInputId, value: InputValue) {
    let Some((element, _)) = studio(world) else {
        return;
    };
    let mut values = ConfigValues::default();
    values.set(input, value);
    run(world, &SetActiveConfiguration { element, values });
}

/// Text typed in a field as an expression: a bare number takes the quantity's unit.
fn with_unit(text: &str, q: Option<Quantity>, units: &Units) -> String {
    let t = match q {
        Some(Quantity::Length) => VariableType::Length,
        Some(Quantity::Angle) => VariableType::Angle,
        _ => VariableType::Number,
    };
    t.with_unit(text, units)
}

// ---- The panel ---------------------------------------------------------------------------

fn sync_panel(
    doc: Option<Res<ActiveDocument>>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    q_panel: Query<Entity, With<ConfigurationPanel>>,
    mut commands: Commands,
) {
    let want = *open == SidePanel::Configuration
        && *kind == ActiveKind::PartStudio
        && doc.as_ref().is_some_and(|d| {
            d.active_element()
                .is_some_and(|e| matches!(e.kind, cadrs_core::ElementKind::PartStudio { .. }))
        });
    if !want {
        for e in &q_panel {
            commands.entity(e).try_despawn();
        }
        return;
    }
    if doc.as_ref().is_some_and(|d| d.is_changed())
        || open.is_changed()
        || kind.is_changed()
        || q_panel.is_empty()
    {
        commands.queue(rebuild);
    }
}

/// The tab bar's Configuration button shows in Part Studios only (set on every button when the
/// tab kind changes, and on one just spawned).
fn sync_button(
    kind: Res<ActiveKind>,
    mut q: Query<(Ref<Name>, &mut Node)>,
    q_new: Query<&Name, Added<Name>>,
) {
    let fresh = q_new.iter().any(|n| n.as_str() == "panel-configuration");
    if !kind.is_changed() && !fresh {
        return;
    }
    let d = if *kind == ActiveKind::PartStudio {
        Display::Flex
    } else {
        Display::None
    };
    for (n, mut node) in &mut q {
        if n.as_str() == "panel-configuration" && node.display != d {
            node.display = d;
        }
    }
}

/// What a cell shows.
fn cell_text(v: &CellValue) -> String {
    match v {
        CellValue::Expr(e) => e.clone(),
        CellValue::Bool(b) => b.to_string(),
    }
}

fn rebuild(world: &mut World) {
    let Some((_, el)) = studio(world) else {
        return;
    };
    let c = el.configuration.clone();
    let resolved = c.resolve(&c.active);
    let heads: Vec<Vec<String>> = c
        .inputs
        .iter()
        .map(|i| {
            i.params
                .iter()
                .map(|p| p.target.label(el.features()))
                .collect()
        })
        .collect();
    let key = PanelKey(format!("{c:?}{heads:?}"));
    let mut q = world.query_filtered::<(Entity, &PanelKey), With<ConfigurationPanel>>();
    let panels: Vec<(Entity, PanelKey)> = q.iter(world).map(|(e, k)| (e, k.clone())).collect();
    if panels.iter().any(|(_, k)| *k == key) {
        open_pending(world);
        return;
    }
    for (e, _) in panels {
        world.entity_mut(e).despawn();
    }
    let t = world.resource::<Theme>().clone();
    let widest = c.inputs.iter().map(|i| i.params.len()).max().unwrap_or(0);
    let width = (NAME_W + 60.0 + (COL_W + 4.0) * widest as f32).clamp(280.0, 760.0);
    let panel = world
        .spawn((
            Name::new("configuration-panel"),
            ConfigurationPanel,
            key,
            DespawnOnExit(AppState::Document),
            side_panel_node(&t),
        ))
        .id();
    world
        .entity_mut(panel)
        .entry::<Node>()
        .and_modify(move |mut n| {
            n.width = Val::Px(width);
            n.flex_direction = FlexDirection::Column;
        });
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        side_panel_header(p, &t, &tr("config-title"), "configuration-panel-close");
        p.spawn(Node {
            padding: UiRect::new(Val::Px(8.0), Val::Px(8.0), Val::Px(6.0), Val::Px(6.0)),
            ..default()
        })
        .with_children(|r| {
            r.spawn(
                cadrs_ui::Button::new("configuration-add-input")
                    .label(tr("config-add-input"))
                    .icon("plus")
                    .small()
                    .build(&t),
            )
            .insert(Tooltip::new(tr("config-add-input-tooltip")));
        });
        if c.inputs.is_empty() {
            p.spawn((
                Name::new("configuration-empty"),
                t.text(
                    tr("config-empty"),
                    11.5,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Node {
                    margin: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
            ));
            return;
        }
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll(),
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            ..default()
        })
        .with_children(|body| {
            for (k, (input, r)) in c.inputs.iter().zip(&resolved).enumerate() {
                section(body, &t, &el, k, input, *r, &heads[k]);
            }
        });
    });
    let mut q_area = world.query_filtered::<(Entity, &ChildOf), With<ViewportArea>>();
    let Some((area, parent)) = q_area.iter(world).next().map(|(e, c)| (e, c.parent())) else {
        return;
    };
    let at = world
        .get::<Children>(parent)
        .and_then(|c| c.iter().position(|e| e == area))
        .map_or(0, |i| i + 1);
    world.entity_mut(parent).insert_children(at, &[panel]);
    open_pending(world);
}

/// An input's section: its title, its value or values, and its table.
#[allow(clippy::too_many_arguments)]
fn section(
    p: &mut ChildSpawnerCommands,
    t: &Theme,
    el: &Element,
    k: usize,
    input: &ConfigInput,
    r: cfg::Resolved,
    heads: &[String],
) {
    let base = format!("configuration-input-{}", k + 1);
    let icon = match input.kind {
        InputKind::List { .. } => "list",
        InputKind::Checkbox { .. } => "check",
    };
    let mut title = p.spawn((
        TreeItem::new(base.clone(), input.name.clone())
            .icon(icon, 15.0)
            .icon_color(t.muted_foreground)
            .left(8.0)
            .weight(FontWeight::MEDIUM)
            .editable()
            .trailing(Some(input.kind.label().to_string()), t.muted_foreground)
            .build(t),
        InputTitle(input.id),
        ContextMenuTarget,
        Tooltip::new(tr_with("config-input-tooltip", &[("name", &input.name)])),
    ));
    title.entry::<Node>().and_modify(|mut n| {
        n.height = Val::Px(28.0);
        n.margin.top = Val::Px(6.0);
    });
    match &input.kind {
        InputKind::Checkbox { .. } => {
            let on = matches!(r, cfg::Resolved::Checked(true));
            p.spawn(Node {
                padding: UiRect::new(Val::Px(30.0), Val::Px(8.0), Val::ZERO, Val::ZERO),
                ..default()
            })
            .with_children(|r| {
                r.spawn((
                    Checkbox::new(format!("{base}-checked"))
                        .label(input.name.clone())
                        .checked(on)
                        .build(t),
                    ActiveCheck(input.id),
                ));
            });
        }
        InputKind::List { .. } => {}
    }
    // The configuration table.
    let active = match r {
        cfg::Resolved::Row(i) => Some(i),
        cfg::Resolved::Checked(c) => Some(usize::from(c)),
    };
    let default_row = match &input.kind {
        InputKind::List { default, .. } => Some(*default),
        _ => None,
    };
    let is_list = default_row.is_some();
    // The rows' width: the name column, the ✓, and the cells.
    let table_w = NAME_W + 60.0 + (COL_W + 4.0) * input.params.len() as f32;
    p.spawn((
        Name::new(format!("{base}-header")),
        Node {
            height: Val::Px(32.0),
            min_width: Val::Px(table_w),
            flex_shrink: 0.0,
            padding: UiRect::new(Val::Px(30.0), Val::Px(6.0), Val::ZERO, Val::ZERO),
            align_items: AlignItems::Center,
            column_gap: Val::Px(4.0),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(t.panel_border),
    ))
    .with_children(|h| {
        h.spawn((
            t.text(
                tr("config-configuration"),
                11.0,
                FontWeight::MEDIUM,
                t.muted_foreground,
            ),
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(NAME_W - 30.0),
                ..default()
            },
        ));
        for (j, label) in heads.iter().enumerate() {
            // Two lines, cut at the column's width: the feature, then the parameter.
            let (feature, param) = label.split_once(" · ").unwrap_or((label.as_str(), ""));
            h.spawn((
                Name::new(format!("{base}-col-{}", j + 1)),
                Node {
                    width: Val::Px(COL_W),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip(),
                    ..default()
                },
                Tooltip::new(label.clone()),
                ColumnHead {
                    input: input.id,
                    param: j,
                },
                ContextMenuTarget,
            ))
            .with_children(|c| {
                for (text, color) in [(feature, t.muted_foreground), (param, t.foreground)] {
                    c.spawn(t.text(text.to_string(), 10.5, FontWeight::MEDIUM, color))
                        .insert(TextLayout::no_wrap());
                }
            });
        }
    });
    for (row, name) in input.row_names().iter().enumerate() {
        let row_name = format!("{base}-row-{}", row + 1);
        let shown = active == Some(row);
        let mut tree = TreeItem::new(row_name.clone(), name.clone())
            .left(30.0)
            .selected(shown)
            .trailing(
                (default_row == Some(row)).then(|| tr("config-default-tag")),
                t.muted_foreground,
            );
        if is_list {
            tree = tree.editable();
        }
        let mut e = p.spawn((
            tree.build(t),
            TableRow {
                input: input.id,
                row,
            },
            ContextMenuTarget,
            DoubleClickable,
            Tooltip::new(if shown {
                tr_with("config-row-shown-tooltip", &[("name", name)])
            } else {
                tr_with("config-row-tooltip", &[("name", name)])
            }),
        ));
        e.entry::<Node>().and_modify(move |mut n| {
            // As wide as its name and cells, however narrow the panel (it then scrolls).
            n.min_width = Val::Px(table_w);
            n.flex_shrink = 0.0;
            n.height = Val::Px(26.0);
            n.column_gap = Val::Px(4.0);
            n.padding.right = Val::Px(6.0);
        });
        e.with_children(|r| {
            r.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if shown {
                r.spawn((
                    Name::new(format!("{row_name}-shown")),
                    cadrs_ui::icon::icon_in(
                        "check",
                        14.0,
                        Color::srgb_u8(0x2e, 0x9e, 0x44),
                        Node {
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ),
                ));
            }
            for (j, param) in input.params.iter().enumerate() {
                let name = format!("{row_name}-cell-{}", j + 1);
                let cell = Cell {
                    input: input.id,
                    row,
                    param: j,
                };
                match param.values.get(row) {
                    Some(CellValue::Bool(b)) => {
                        r.spawn(Node {
                            width: Val::Px(COL_W),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::Center,
                            ..default()
                        })
                        .with_children(|c| {
                            c.spawn((
                                Checkbox::new(name).checked(*b).height(22.0).build(t),
                                cell,
                                Tooltip::new(tr("config-suppressed-tooltip")),
                            ));
                        });
                    }
                    v => {
                        let mut f = r.spawn((
                            TextInput::new(name)
                                .value(v.map(cell_text).unwrap_or_default())
                                .height(22.0)
                                .width(Val::Px(COL_W))
                                .select_all_on_focus()
                                .build(t),
                            cell,
                            Tooltip::new(heads[j].clone()),
                        ));
                        f.entry::<Node>().and_modify(|mut n| n.flex_shrink = 0.0);
                    }
                }
            }
        });
    }
    if is_list {
        p.spawn(Node {
            padding: UiRect::new(Val::Px(26.0), Val::Px(8.0), Val::Px(4.0), Val::Px(2.0)),
            ..default()
        })
        .with_children(|r| {
            r.spawn(
                cadrs_ui::Button::new(format!("{base}-add-row"))
                    .label(tr("config-add-row"))
                    .icon("plus")
                    .small()
                    .ghost()
                    .build(t),
            )
            .insert(AddRow(input.id));
        });
    }
    if input.params.is_empty() {
        p.spawn((
            t.text(
                tr("config-no-params"),
                11.0,
                FontWeight::NORMAL,
                t.muted_foreground,
            ),
            Node {
                margin: UiRect::new(Val::Px(30.0), Val::Px(8.0), Val::Px(2.0), Val::Px(4.0)),
                ..default()
            },
        ));
    }
    let _ = el;
}

/// Opens the name of an input or row just added, once the panel shows it.
fn open_pending(world: &mut World) {
    let Some(p) = world.get_resource::<PendingName>().copied() else {
        return;
    };
    let found = match p {
        PendingName::Input(id) => {
            let mut q = world.query::<(Entity, &InputTitle)>();
            q.iter(world).find(|(_, x)| x.0 == id).map(|(e, _)| e)
        }
        PendingName::Row(id, row) => {
            let mut q = world.query::<(Entity, &TableRow)>();
            q.iter(world)
                .find(|(_, x)| x.input == id && x.row == row)
                .map(|(e, _)| e)
        }
    };
    let Some(e) = found else { return };
    world.remove_resource::<PendingName>();
    let name = studio(world)
        .and_then(|(_, el)| {
            let c = &el.configuration;
            match p {
                PendingName::Input(id) => c.input(id).map(|i| i.name.clone()),
                PendingName::Row(id, row) => {
                    c.input(id).and_then(|i| i.row_names().get(row).cloned())
                }
            }
        })
        .unwrap_or_default();
    begin_rename(world, e, name);
}

fn begin_rename(world: &mut World, e: Entity, name: String) {
    let theme = world.resource::<Theme>().clone();
    let mut opts = InlineEditOptions::new("configuration-name");
    opts.width = Val::Px(NAME_W);
    opts.height = 22.0;
    opts.font_size = Some(theme.font_sm);
    opts.weight = FontWeight::MEDIUM;
    opts.padding = Some(2.0);
    let mut commands = world.commands();
    begin_inline_edit(&mut commands, &theme, e, name, opts);
    world.flush();
}

// ---- Edits ---------------------------------------------------------------------------------

/// A name not yet taken: `base`, then `base2`, `base3`, ….
fn free_name(taken: &[String], base: &str) -> String {
    if !taken.iter().any(|n| n == base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}{n}"))
        .find(|n| !taken.contains(n))
        .unwrap_or_default()
}

/// Adds an input of kind `kind` ("list", "variable", "checkbox"), its name open for editing.
pub fn add_input(world: &mut World, kind: &str) -> Option<ConfigInputId> {
    let (_, el) = studio(world)?;
    let taken: Vec<String> = el
        .configuration
        .inputs
        .iter()
        .map(|i| i.name.clone())
        .collect();
    let input = match kind {
        "list" => ConfigInput::list(&free_name(&taken, "Configuration"), &["Default"]),
        _ => ConfigInput::checkbox(&free_name(&taken, "Checkbox"), false),
    };
    let id = input.id;
    let mut done = false;
    edit(world, "Add configuration input", |c, _| {
        c.inputs.push(input);
        done = true;
        true
    });
    if done {
        world.insert_resource(PendingName::Input(id));
    }
    Some(id)
}

fn add_row(world: &mut World, input: ConfigInputId, copy: Option<usize>) {
    let mut at = None;
    edit(world, "Add configuration", |c, _| {
        let Some(i) = c.input_mut(input) else {
            return false;
        };
        let InputKind::List { rows, .. } = &mut i.kind else {
            return false;
        };
        let taken: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        let from = copy.unwrap_or(rows.len().saturating_sub(1));
        let n = rows.len() + 1;
        let name = match copy {
            Some(r) => free_name(&taken, &format!("{} copy", rows[r].name)),
            None => free_name(&taken, &format!("Configuration {n}")),
        };
        let row = copy.map_or(rows.len(), |r| r + 1);
        rows.insert(
            row,
            ListRow {
                id: ConfigRowId::new(),
                name,
            },
        );
        for p in &mut i.params {
            let v = p
                .values
                .get(from)
                .cloned()
                .unwrap_or(CellValue::Expr(String::new()));
            p.values.insert(row, v);
        }
        at = Some(row);
        true
    });
    if let Some(row) = at {
        world.insert_resource(PendingName::Row(input, row));
    }
}

fn rename(world: &mut World, target: PendingName, name: String) {
    edit(world, "Rename", |c, _| match target {
        PendingName::Input(id) => match c.input_mut(id) {
            Some(i) if i.name != name => {
                i.name = name;
                true
            }
            _ => false,
        },
        PendingName::Row(id, row) => match c.input_mut(id).map(|i| &mut i.kind) {
            Some(InputKind::List { rows, .. }) => match rows.get_mut(row) {
                Some(r) if r.name != name => {
                    r.name = name;
                    true
                }
                _ => false,
            },
            _ => false,
        },
    });
}

/// Moves row `row` of a List by `by` (−1 up, +1 down), its values with it.
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

fn move_input(c: &mut Configuration, input: ConfigInputId, by: isize) -> bool {
    let Some(k) = c.inputs.iter().position(|i| i.id == input) else {
        return false;
    };
    let to = k as isize + by;
    if to < 0 || to as usize >= c.inputs.len() {
        return false;
    }
    c.inputs.swap(k, to as usize);
    true
}

fn move_column(c: &mut Configuration, input: ConfigInputId, param: usize, by: isize) -> bool {
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

// ---- Observers -----------------------------------------------------------------------------

fn on_button(a: On<Activate>, q: Query<&Name>, q_add: Query<&AddRow>, mut commands: Commands) {
    if let Ok(r) = q_add.get(a.entity) {
        let id = r.0;
        commands.queue(move |world: &mut World| add_row(world, id, None));
        return;
    }
    let Ok(n) = q.get(a.entity) else { return };
    if n.as_str() != "configuration-add-input" {
        return;
    }
    let e = a.entity;
    commands.queue(move |world: &mut World| {
        let at = below(world, e).unwrap_or_default();
        let menu = Menu::new("configuration-add-input-menu")
            .min_width(160.0)
            .item_height(22.0)
            .item(MenuItem::new("configuration-add-list", tr("config-list")).icon("list"))
            .item(MenuItem::new("configuration-add-checkbox", tr("config-checkbox")).icon("check"));
        open_menu_at(world, at, menu, PanelMenu::AddInput);
    });
}

/// Just under an entity's left edge, in logical pixels.
fn below(world: &World, e: Entity) -> Option<Vec2> {
    let r = world.get_entity(e).ok()?;
    let n = r.get::<ComputedNode>()?;
    let t = r.get::<bevy::ui::UiGlobalTransform>()?;
    let s = n.inverse_scale_factor();
    let size = n.size() * s;
    let c = t.translation * s;
    Some(Vec2::new(c.x - size.x / 2.0, c.y + size.y / 2.0))
}

fn open_menu_at(world: &mut World, at: Vec2, menu: Menu, what: PanelMenu) {
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
    commands
        .entity(anchor)
        .insert((what, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_rename(
    ev: On<InlineEditCommit>,
    q_title: Query<&InputTitle>,
    q_row: Query<&TableRow>,
    mut commands: Commands,
) {
    let target = if let Ok(t) = q_title.get(ev.entity) {
        PendingName::Input(t.0)
    } else if let Ok(r) = q_row.get(ev.entity) {
        PendingName::Row(r.input, r.row)
    } else {
        return;
    };
    let name = ev.value.trim().to_string();
    if !name.is_empty() {
        commands.queue(move |world: &mut World| rename(world, target, name));
    }
}

/// A cell typed in.
fn on_text(ev: On<TextSubmit>, q_cell: Query<&Cell>, mut commands: Commands) {
    let text = ev.value.trim().to_string();
    if let Ok(cell) = q_cell.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| {
            let units = units(world);
            edit(world, "Edit configuration table", |c, el| {
                let Some(p) = c
                    .input_mut(cell.input)
                    .and_then(|i| i.params.get_mut(cell.param))
                else {
                    return false;
                };
                let q = p.target.quantity(el.features());
                let Some(v) = p.values.get_mut(cell.row) else {
                    return false;
                };
                let new = CellValue::Expr(with_unit(&text, q, &units));
                if text.is_empty() || *v == new {
                    return false;
                }
                *v = new;
                true
            });
        });
    }
}

/// A suppression cell, or a Checkbox input's state.
fn on_check(
    ev: On<CheckboxChange>,
    q_cell: Query<&Cell>,
    q_active: Query<&ActiveCheck>,
    mut commands: Commands,
) {
    let checked = ev.checked;
    if let Ok(a) = q_active.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| activate(world, a.0, InputValue::Checked(checked)));
        return;
    }
    let Ok(cell) = q_cell.get(ev.entity).copied() else {
        return;
    };
    commands.queue(move |world: &mut World| {
        edit(world, "Edit configuration table", |c, _| {
            let Some(v) = c
                .input_mut(cell.input)
                .and_then(|i| i.params.get_mut(cell.param))
                .and_then(|p| p.values.get_mut(cell.row))
            else {
                return false;
            };
            *v = CellValue::Bool(checked);
            true
        })
    });
}

/// Shows a row's configuration in the Part Studio.
fn show_row(world: &mut World, input: ConfigInputId, row: usize) {
    let Some((_, el)) = studio(world) else { return };
    let Some(i) = el.configuration.input(input) else {
        return;
    };
    let v = match &i.kind {
        InputKind::List { rows, .. } => match rows.get(row) {
            Some(r) => InputValue::Row(r.id),
            None => return,
        },
        InputKind::Checkbox { .. } => InputValue::Checked(row == 1),
    };
    activate(world, input, v);
}

fn on_double_click(ev: On<DoubleClick>, q: Query<&TableRow>, mut commands: Commands) {
    if let Ok(r) = q.get(ev.entity).copied() {
        commands.queue(move |world: &mut World| show_row(world, r.input, r.row));
    }
}

fn on_context_menu(
    ev: On<ContextMenuRequested>,
    q_title: Query<&InputTitle>,
    q_row: Query<&TableRow>,
    q_col: Query<&ColumnHead>,
    mut commands: Commands,
) {
    let at = ev.position;
    let what = if let Ok(t) = q_title.get(ev.entity) {
        PanelMenu::Input(t.0)
    } else if let Ok(r) = q_row.get(ev.entity) {
        PanelMenu::Row(r.input, r.row)
    } else if let Ok(c) = q_col.get(ev.entity) {
        PanelMenu::Column(c.input, c.param)
    } else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let Some((_, el)) = studio(world) else { return };
        let c = &el.configuration;
        let menu = Menu::new("configuration-menu")
            .min_width(180.0)
            .item_height(22.0);
        let menu = match what {
            PanelMenu::Input(id) => {
                let Some(i) = c.input(id) else { return };
                let k = c.inputs.iter().position(|x| x.id == id).unwrap_or(0);
                let mut m = menu
                    .item(MenuItem::new(
                        "configuration-input-rename",
                        tr("common-rename"),
                    ))
                    .item(
                        MenuItem::new("configuration-input-up", tr("config-move-up"))
                            .disabled(k == 0),
                    )
                    .item(
                        MenuItem::new("configuration-input-down", tr("config-move-down"))
                            .disabled(k + 1 == c.inputs.len()),
                    );
                if let InputKind::Checkbox { default } = i.kind {
                    m = m.item(
                        MenuItem::new(
                            "configuration-input-default-checked",
                            tr("config-checked-by-default"),
                        )
                        .checked(default),
                    );
                }
                m.separator().item(
                    MenuItem::new("configuration-input-delete", tr("common-delete"))
                        .icon("remove-circle"),
                )
            }
            PanelMenu::Row(id, row) => {
                let Some(i) = c.input(id) else { return };
                let n = i.row_count();
                let m = menu.item(MenuItem::new("configuration-row-show", tr("config-show")));
                if !matches!(i.kind, InputKind::List { .. }) {
                    m
                } else {
                    m.item(MenuItem::new(
                        "configuration-row-rename",
                        tr("common-rename"),
                    ))
                    .item(MenuItem::new(
                        "configuration-row-default",
                        tr("config-set-default"),
                    ))
                    .item(MenuItem::new(
                        "configuration-row-duplicate",
                        tr("common-duplicate"),
                    ))
                    .item(
                        MenuItem::new("configuration-row-up", tr("config-move-up"))
                            .disabled(row == 0),
                    )
                    .item(
                        MenuItem::new("configuration-row-down", tr("config-move-down"))
                            .disabled(row + 1 >= n),
                    )
                    .separator()
                    .item(
                        MenuItem::new("configuration-row-delete", tr("common-delete"))
                            .icon("remove-circle")
                            .disabled(n < 2),
                    )
                }
            }
            PanelMenu::Column(id, param) => {
                let Some(i) = c.input(id) else { return };
                menu.item(
                    MenuItem::new("configuration-column-left", tr("config-move-left"))
                        .disabled(param == 0),
                )
                .item(
                    MenuItem::new("configuration-column-right", tr("config-move-right"))
                        .disabled(param + 1 >= i.params.len()),
                )
                .separator()
                .item(
                    MenuItem::new("configuration-column-remove", tr("config-remove-column"))
                        .icon("remove-circle"),
                )
            }
            PanelMenu::AddInput => return,
        };
        open_menu_at(world, at, menu, what);
    });
}

fn on_menu_action(
    ev: On<MenuAction>,
    q: Query<&PanelMenu, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(what) = q.get(ev.entity).copied() else {
        return;
    };
    let item = ev.item.clone();
    commands.queue(move |world: &mut World| menu_action(world, what, &item));
}

fn find_entity<C: Component>(world: &mut World, f: impl Fn(&C) -> bool) -> Option<Entity> {
    let mut q = world.query::<(Entity, &C)>();
    q.iter(world).find(|(_, c)| f(c)).map(|(e, _)| e)
}

fn menu_action(world: &mut World, what: PanelMenu, item: &str) {
    match (what, item) {
        (PanelMenu::AddInput, "configuration-add-list") => {
            add_input(world, "list");
        }
        (PanelMenu::AddInput, "configuration-add-checkbox") => {
            add_input(world, "checkbox");
        }
        (PanelMenu::Input(id), "configuration-input-rename") => {
            let name = studio(world)
                .and_then(|(_, el)| el.configuration.input(id).map(|i| i.name.clone()));
            if let (Some(e), Some(name)) = (find_entity::<InputTitle>(world, |t| t.0 == id), name) {
                begin_rename(world, e, name);
            }
        }
        (PanelMenu::Input(id), "configuration-input-up" | "configuration-input-down") => {
            let by = if item.ends_with("up") { -1 } else { 1 };
            edit(world, "Move configuration input", |c, _| {
                move_input(c, id, by)
            });
        }
        (PanelMenu::Input(id), "configuration-input-default-checked") => {
            edit(world, "Edit configuration input", |c, _| {
                match c.input_mut(id).map(|i| &mut i.kind) {
                    Some(InputKind::Checkbox { default }) => {
                        *default = !*default;
                        true
                    }
                    _ => false,
                }
            });
        }
        (PanelMenu::Input(id), "configuration-input-delete") => {
            edit(world, "Delete configuration input", |c, _| {
                let n = c.inputs.len();
                c.inputs.retain(|i| i.id != id);
                c.inputs.len() != n
            });
        }
        (PanelMenu::Row(id, row), "configuration-row-show") => show_row(world, id, row),
        (PanelMenu::Row(id, row), "configuration-row-rename") => {
            let name = studio(world).and_then(|(_, el)| {
                el.configuration
                    .input(id)
                    .and_then(|i| i.row_names().get(row).cloned())
            });
            if let (Some(e), Some(name)) = (
                find_entity::<TableRow>(world, |r| r.input == id && r.row == row),
                name,
            ) {
                begin_rename(world, e, name);
            }
        }
        (PanelMenu::Row(id, row), "configuration-row-default") => {
            edit(world, "Set default configuration", |c, _| {
                match c.input_mut(id).map(|i| &mut i.kind) {
                    Some(InputKind::List { default, .. }) if *default != row => {
                        *default = row;
                        true
                    }
                    _ => false,
                }
            });
        }
        (PanelMenu::Row(id, row), "configuration-row-duplicate") => add_row(world, id, Some(row)),
        (PanelMenu::Row(id, row), "configuration-row-up" | "configuration-row-down") => {
            let by = if item.ends_with("up") { -1 } else { 1 };
            edit(world, "Move configuration", |c, _| move_row(c, id, row, by));
        }
        (PanelMenu::Row(id, row), "configuration-row-delete") => {
            edit(world, "Delete configuration", |c, _| delete_row(c, id, row));
        }
        (
            PanelMenu::Column(id, param),
            "configuration-column-left" | "configuration-column-right",
        ) => {
            let by = if item.ends_with("left") { -1 } else { 1 };
            edit(world, "Move configured parameter", |c, _| {
                move_column(c, id, param, by)
            });
        }
        (PanelMenu::Column(id, param), "configuration-column-remove") => {
            let Some((element, el)) = studio(world) else {
                return;
            };
            let Some(target) = el
                .configuration
                .input(id)
                .and_then(|i| i.params.get(param))
                .map(|p| p.target.clone())
            else {
                return;
            };
            run(world, &UnconfigureParameter { element, target });
        }
        _ => {}
    }
}

// ---- Configure ▸ (the feature menu) --------------------------------------------------------

/// The inputs a parameter can be configured by (every List and Checkbox).
fn table_inputs(c: &Configuration) -> Vec<&ConfigInput> {
    c.inputs.iter().collect()
}

/// A parameter's label in the Configure menu ("Depth", "D3", "Value").
fn param_label(target: &ParamTarget, el: &Element) -> String {
    let full = target.label(el.features());
    full.split_once(" · ")
        .map_or(full.clone(), |(_, l)| l.to_string())
}

/// The feature menu's **Configure ▸** entries for `feature` (none when it has nothing to
/// configure): item `feature-configure-<p>` (or `feature-configure-<p>-<i>` with several
/// Lists and Checkboxes), `p` indexing [`cfg::configurable`] and `i` the table inputs.
pub fn configure_entries(el: &Element, feature: FeatureId) -> Option<MenuItem> {
    let f = el.feature(feature)?;
    let c = &el.configuration;
    let inputs = table_inputs(c);
    let entries: Vec<MenuEntry> = cfg::configurable(f)
        .iter()
        .enumerate()
        .filter(|(_, t)| !matches!(t, ParamTarget::Suppressed { .. }))
        .map(|(p, t)| {
            let label = param_label(t, el);
            let by = c.param(t).map(|(i, _)| i.id);
            if inputs.len() > 1 {
                MenuItem::new(format!("feature-configure-{p}"), label)
                    .checked(by.is_some())
                    .submenu(
                        inputs
                            .iter()
                            .enumerate()
                            .map(|(k, i)| {
                                MenuItem::new(format!("feature-configure-{p}-{k}"), i.name.clone())
                                    .checked(by == Some(i.id))
                                    .into()
                            })
                            .collect(),
                    )
                    .into()
            } else {
                MenuItem::new(format!("feature-configure-{p}"), label)
                    .checked(by.is_some())
                    .into()
            }
        })
        .collect();
    if entries.is_empty() {
        return None;
    }
    Some(
        MenuItem::new("feature-configure", tr("config-configure"))
            .icon("configurations")
            .submenu(entries),
    )
}

/// Dynamic suppression ▸ **Suppress by configuration**'s entry: a submenu of the Lists and
/// Checkboxes when there are several (`feature-suppress-by-configuration-<i>`).
pub fn suppress_entry(el: &Element, feature: FeatureId) -> MenuItem {
    let c = &el.configuration;
    let inputs = table_inputs(c);
    let target = ParamTarget::Suppressed { feature };
    let by = c.param(&target).map(|(i, _)| i.id);
    let item = MenuItem::new(
        "feature-suppress-by-configuration",
        tr("shell-doc-suppress-by-configuration"),
    )
    .checked(by.is_some());
    if inputs.len() > 1 {
        item.submenu(
            inputs
                .iter()
                .enumerate()
                .map(|(k, i)| {
                    MenuItem::new(
                        format!("feature-suppress-by-configuration-{k}"),
                        i.name.clone(),
                    )
                    .checked(by == Some(i.id))
                    .into()
                })
                .collect(),
        )
    } else {
        item
    }
}

/// A Configure ▸ item picked on `feature`'s menu (`item`: `feature-configure-…` or
/// `feature-suppress-by-configuration…`): configures the parameter (making a List first when
/// there is no List or Checkbox), or removes its column when it is configured already. Opens
/// the Configuration panel.
pub fn configure_action(world: &mut World, feature: FeatureId, item: &str) {
    let Some((element, el)) = studio(world) else {
        return;
    };
    let Some(f) = el.feature(feature) else { return };
    let (target, input) = if let Some(rest) = item.strip_prefix("feature-configure-") {
        let mut it = rest.split('-').map(|s| s.parse::<usize>().ok());
        let (Some(Some(p)), k) = (it.next(), it.next().flatten()) else {
            return;
        };
        let Some(t) = cfg::configurable(f).get(p).cloned() else {
            return;
        };
        (t, k)
    } else if let Some(rest) = item.strip_prefix("feature-suppress-by-configuration") {
        let k = rest.strip_prefix('-').and_then(|s| s.parse::<usize>().ok());
        (ParamTarget::Suppressed { feature }, k)
    } else {
        return;
    };
    // Configured already (by this input, or by any when none is named): removed.
    let named = input.and_then(|k| table_inputs(&el.configuration).get(k).map(|i| i.id));
    if let Some((by, _)) = el.configuration.param(&target)
        && named.is_none_or(|i| i == by.id)
    {
        run(world, &UnconfigureParameter { element, target });
        return;
    }
    // Else one step: off any other input, onto this one (a new List when there is none),
    // each row starting with the value it has now.
    let Some(now) = cfg::current(&target, el.features(), el.suppressed()) else {
        return;
    };
    let mut new_input = None;
    let mut done = false;
    edit(world, "Configure parameter", |c, _| {
        for i in &mut c.inputs {
            i.params.retain(|p| p.target != target);
        }
        let id = match named.or_else(|| table_inputs(c).first().map(|i| i.id)) {
            Some(id) => id,
            None => {
                let taken: Vec<String> = c.inputs.iter().map(|i| i.name.clone()).collect();
                let list = ConfigInput::list(&free_name(&taken, "Configuration"), &["Default"]);
                let id = list.id;
                c.inputs.push(list);
                new_input = Some(id);
                id
            }
        };
        let Some(i) = c.input_mut(id) else {
            return false;
        };
        let n = i.row_count();
        i.params.push(cfg::ConfiguredParam {
            target,
            values: vec![now; n],
        });
        done = true;
        true
    });
    if done {
        *world.resource_mut::<SidePanel>() = SidePanel::Configuration;
        if let Some(id) = new_input {
            world.insert_resource(PendingName::Input(id));
        }
    }
}

// ---- Assembly instances ---------------------------------------------------------------------

/// The configured Part Studio of the instances (all of the same one), and their values (the
/// first's).
fn instances_studio(
    world: &World,
    assembly: ElementId,
    instances: &[cadrs_core::assembly::InstanceId],
) -> Option<(Element, ConfigValues)> {
    let doc = &world.get_resource::<ActiveDocument>()?.doc;
    let a = doc.element(assembly)?.assembly_model()?;
    let mut studio: Option<ElementId> = None;
    let mut values = None;
    for id in instances {
        let i = a.instance(*id)?;
        let s = match &i.configuration {
            Some(c) => c.studio,
            None => i.source.element(),
        };
        if studio.is_some_and(|x| x != s) {
            return None;
        }
        studio = Some(s);
        if values.is_none() {
            values = Some(i.configuration.as_ref().map(|c| c.values.clone()));
        }
    }
    let el = doc.elements.iter().find(|e| Some(e.id) == studio)?;
    if el.configuration.is_empty() || !matches!(el.kind, cadrs_core::ElementKind::PartStudio { .. })
    {
        return None;
    }
    let values = values
        .flatten()
        .unwrap_or_else(|| el.configuration.active.clone());
    Some((el.clone(), values))
}

/// The instance menu's **Configuration ▸** for instances of a configured Part Studio: each
/// List's rows (`asm-config-<k>-<r>`, ✓ on the instances' one), each Checkbox
/// (`asm-config-<k>-toggle`).
pub fn instance_menu_item(
    world: &World,
    assembly: ElementId,
    instances: &[cadrs_core::assembly::InstanceId],
) -> Option<MenuItem> {
    let (el, values) = instances_studio(world, assembly, instances)?;
    let c = &el.configuration;
    let mut entries: Vec<MenuEntry> = Vec::new();
    for (k, (input, r)) in c.inputs.iter().zip(c.resolve(&values)).enumerate() {
        match (&input.kind, r) {
            (InputKind::List { rows, .. }, cfg::Resolved::Row(cur)) => {
                entries.push(
                    MenuItem::new(format!("asm-config-{k}"), input.name.clone())
                        .submenu(
                            rows.iter()
                                .enumerate()
                                .map(|(j, row)| {
                                    MenuItem::new(format!("asm-config-{k}-{j}"), row.name.clone())
                                        .checked(j == cur)
                                        .into()
                                })
                                .collect(),
                        )
                        .into(),
                );
            }
            (InputKind::Checkbox { .. }, cfg::Resolved::Checked(on)) => entries.push(
                MenuItem::new(format!("asm-config-{k}-toggle"), input.name.clone())
                    .checked(on)
                    .into(),
            ),
            _ => {}
        }
    }
    Some(
        MenuItem::new("asm-config", tr("config-instance-configuration"))
            .icon("configurations")
            .submenu(entries),
    )
}

/// Sets one input of the instances' configuration.
fn set_instance_value(
    world: &mut World,
    assembly: ElementId,
    instances: Vec<cadrs_core::assembly::InstanceId>,
    input: ConfigInputId,
    value: InputValue,
) {
    let Some((_, mut values)) = instances_studio(world, assembly, &instances) else {
        return;
    };
    values.set(input, value);
    run(
        world,
        &SetInstanceConfiguration {
            assembly,
            instances,
            values: Some(values),
        },
    );
}

/// An `asm-config-…` item of the instance menu picked.
pub fn instance_menu_action(
    world: &mut World,
    assembly: ElementId,
    instances: Vec<cadrs_core::assembly::InstanceId>,
    item: &str,
) {
    let Some(rest) = item.strip_prefix("asm-config-") else {
        return;
    };
    let Some((el, values)) = instances_studio(world, assembly, &instances) else {
        return;
    };
    let mut parts = rest.splitn(2, '-');
    let Some(k) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
        return;
    };
    let Some(input) = el.configuration.inputs.get(k) else {
        return;
    };
    match (parts.next(), &input.kind) {
        (Some("toggle"), InputKind::Checkbox { .. }) => {
            let on = matches!(
                input.resolve_value(values.get(input.id)),
                cfg::Resolved::Checked(true)
            );
            set_instance_value(
                world,
                assembly,
                instances,
                input.id,
                InputValue::Checked(!on),
            );
        }
        (Some(r), InputKind::List { rows, .. }) => {
            if let Some(row) = r.parse::<usize>().ok().and_then(|j| rows.get(j)) {
                set_instance_value(
                    world,
                    assembly,
                    instances,
                    input.id,
                    InputValue::Row(row.id),
                );
            }
        }
        _ => {}
    }
}

// ---- The Insert dialog ----------------------------------------------------------------------
//
// Picking an item of a configured Part Studio shows its configuration under the tree
// (`insert-configuration`): a button per List with the row chosen (its menu lists the rows,
// `insert-config-<k>`, items `insert-config-row-<j>`) and a checkbox per Checkbox
// (`insert-config-<k>-checked`). It starts at the studio's active configuration; what is chosen
// is kept per studio while the dialog is open. The ghost follows it (the studio built in that
// configuration) and each instance inserted takes it, as Onshape's Insert dialog does.

/// The Insert dialog's configuration section (filled while the picked item is configured).
#[derive(Component)]
pub(crate) struct InsertConfigPanel;

/// A List's button in the Insert dialog: the studio and the input.
#[derive(Component, Debug, Clone, Copy)]
struct InsertConfigButton(ElementId, ConfigInputId);

/// A Checkbox in the Insert dialog.
#[derive(Component, Debug, Clone, Copy)]
struct InsertConfigCheck(ElementId, ConfigInputId);

/// An open menu of a List's rows in the Insert dialog.
#[derive(Component, Debug, Clone, Copy)]
struct InsertConfigMenu(ElementId, ConfigInputId);

/// The configured Part Studio a source comes from (a tab of this document with a
/// configuration; not a linked copy or a board).
fn configured_studio(world: &World, source: &InstanceSource) -> Option<Element> {
    let element = match source {
        InstanceSource::Part { element, .. }
        | InstanceSource::Studio {
            element,
            board: None,
        } => *element,
        _ => return None,
    };
    let doc = &world.get_resource::<ActiveDocument>()?.doc;
    doc.elements
        .iter()
        .find(|e| e.id == element && !e.configuration.is_empty())
        .cloned()
}

/// The configuration chosen in the Insert dialog for `studio` (its active one until another
/// is picked).
fn insert_values(world: &World, studio: &Element) -> ConfigValues {
    world
        .get_resource::<crate::assembly::insert::InsertSession>()
        .and_then(|s| {
            s.configs
                .iter()
                .find(|(e, _)| *e == studio.id)
                .map(|(_, v)| v.clone())
        })
        .unwrap_or_else(|| studio.configuration.active.clone())
}

/// The configuration an instance of `source` inserted from the dialog takes.
pub(crate) fn insert_configuration(
    world: &World,
    source: &InstanceSource,
) -> Option<cadrs_core::configuration::InstanceConfiguration> {
    let studio = configured_studio(world, source)?;
    let values = studio
        .configuration
        .complete(&insert_values(world, &studio));
    Some(cadrs_core::configuration::InstanceConfiguration {
        studio: studio.id,
        values,
    })
}

/// `source` as a ghost shows it: from the studio built in the chosen configuration (made for
/// the preview, [`crate::assembly::AssemblyParts::extra`], until an instance uses it).
pub(crate) fn ghost_source(world: &mut World, source: InstanceSource) -> InstanceSource {
    let Some(c) = insert_configuration(world, &source) else {
        return source;
    };
    let id = c.copy_id();
    let exists = world.resource::<ActiveDocument>().doc.element(id).is_some()
        || world
            .resource::<crate::assembly::AssemblyParts>()
            .extra
            .iter()
            .any(|e| e.id == id);
    if !exists {
        let doc = &world.resource::<ActiveDocument>().doc;
        let Some(studio) = doc.elements.iter().find(|e| e.id == c.studio) else {
            return source;
        };
        let el = cfg::generate(studio, &c.values, id, &doc.units);
        world
            .resource_mut::<crate::assembly::AssemblyParts>()
            .extra
            .push(el);
    }
    let mut s = source;
    match &mut s {
        InstanceSource::Part { element, .. } | InstanceSource::Studio { element, .. } => {
            *element = id
        }
        InstanceSource::Assembly { .. } => {}
    }
    s
}

/// Fills the Insert dialog's configuration section for the picked item.
pub(crate) fn sync_insert_panel(world: &mut World, mut key: Local<String>) {
    let Some(s) = world
        .get_resource::<crate::assembly::insert::InsertSession>()
        .cloned()
    else {
        key.clear();
        return;
    };
    let mut q = world.query_filtered::<Entity, With<InsertConfigPanel>>();
    let Some(panel) = q.iter(world).next() else {
        return;
    };
    let studio = s
        .armed
        .as_ref()
        .filter(|i| i.link.is_none() && !s.standard)
        .and_then(|i| i.sources.first())
        .and_then(|src| configured_studio(world, src));
    let values = studio.as_ref().map(|e| insert_values(world, e));
    let now = format!(
        "{:?}{:?}",
        studio.as_ref().map(|e| (e.id, &e.configuration.inputs)),
        values
    );
    if *key == now {
        return;
    }
    *key = now;
    world.entity_mut(panel).despawn_related::<Children>();
    let (Some(studio), Some(values)) = (studio, values) else {
        world
            .entity_mut(panel)
            .entry::<Node>()
            .and_modify(|mut n| n.display = Display::None);
        return;
    };
    world
        .entity_mut(panel)
        .entry::<Node>()
        .and_modify(|mut n| n.display = Display::Flex);
    let t = world.resource::<Theme>().clone();
    let c = studio.configuration.clone();
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        p.spawn(t.text(
            tr_with("config-insert-title", &[("name", &studio.name)]),
            11.5,
            FontWeight::BOLD,
            t.foreground,
        ));
        for (k, input) in c.inputs.iter().enumerate() {
            let r = input.resolve_value(values.get(input.id));
            p.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                min_height: Val::Px(26.0),
                ..default()
            })
            .with_children(|row| match (&input.kind, r) {
                (InputKind::List { rows, .. }, cfg::Resolved::Row(i)) => {
                    row.spawn((
                        t.text(
                            input.name.clone(),
                            11.5,
                            FontWeight::NORMAL,
                            t.muted_foreground,
                        ),
                        Node {
                            width: Val::Px(90.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    row.spawn(
                        cadrs_ui::Button::new(format!("insert-config-{}", k + 1))
                            .label(rows[i].name.clone())
                            .icon("chevron-down")
                            .small()
                            .build(&t),
                    )
                    .insert(InsertConfigButton(studio.id, input.id));
                }
                (InputKind::Checkbox { .. }, cfg::Resolved::Checked(on)) => {
                    row.spawn((
                        Checkbox::new(format!("insert-config-{}-checked", k + 1))
                            .label(input.name.clone())
                            .checked(on)
                            .build(&t),
                        InsertConfigCheck(studio.id, input.id),
                    ));
                }
                _ => {}
            });
        }
    });
    world.flush();
}

/// Sets one input of the configuration the Insert dialog inserts `studio` in.
fn set_insert_value(world: &mut World, studio: ElementId, input: ConfigInputId, v: InputValue) {
    let Some(el) = world
        .resource::<ActiveDocument>()
        .doc
        .elements
        .iter()
        .find(|e| e.id == studio)
        .cloned()
    else {
        return;
    };
    let mut values = insert_values(world, &el);
    values.set(input, v);
    let Some(mut s) = world.get_resource_mut::<crate::assembly::insert::InsertSession>() else {
        return;
    };
    s.configs.retain(|(e, _)| *e != studio);
    s.configs.push((studio, values));
}

fn on_insert_button(a: On<Activate>, q: Query<&InsertConfigButton>, mut commands: Commands) {
    let Ok(b) = q.get(a.entity).copied() else {
        return;
    };
    let e = a.entity;
    commands.queue(move |world: &mut World| {
        let Some(el) = world
            .resource::<ActiveDocument>()
            .doc
            .elements
            .iter()
            .find(|x| x.id == b.0)
            .cloned()
        else {
            return;
        };
        let Some(input) = el.configuration.input(b.1) else {
            return;
        };
        let InputKind::List { rows, .. } = &input.kind else {
            return;
        };
        let cur = input.row_of(insert_values(world, &el).get(b.1));
        let mut menu = Menu::new("insert-config-menu")
            .min_width(140.0)
            .item_height(22.0);
        for (j, r) in rows.iter().enumerate() {
            menu = menu.item(
                MenuItem::new(format!("insert-config-row-{j}"), r.name.clone()).checked(j == cur),
            );
        }
        let at = below(world, e).unwrap_or_default();
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        let anchor = open_context_menu(&mut commands, at, menu.build(&theme));
        commands.entity(anchor).insert((
            InsertConfigMenu(b.0, b.1),
            DespawnOnExit(AppState::Document),
        ));
        world.flush();
    });
}

fn on_insert_menu(
    ev: On<MenuAction>,
    q: Query<&InsertConfigMenu, With<ContextMenuAnchor>>,
    mut commands: Commands,
) {
    let Ok(m) = q.get(ev.entity).copied() else {
        return;
    };
    let Some(j) = ev
        .item
        .strip_prefix("insert-config-row-")
        .and_then(|s| s.parse::<usize>().ok())
    else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let row = world
            .resource::<ActiveDocument>()
            .doc
            .elements
            .iter()
            .find(|e| e.id == m.0)
            .and_then(|e| match &e.configuration.input(m.1)?.kind {
                InputKind::List { rows, .. } => rows.get(j).map(|r| r.id),
                _ => None,
            });
        if let Some(row) = row {
            set_insert_value(world, m.0, m.1, InputValue::Row(row));
        }
    });
}

fn on_insert_check(ev: On<CheckboxChange>, q: Query<&InsertConfigCheck>, mut commands: Commands) {
    let Ok(c) = q.get(ev.entity).copied() else {
        return;
    };
    let on = ev.checked;
    commands
        .queue(move |world: &mut World| set_insert_value(world, c.0, c.1, InputValue::Checked(on)));
}

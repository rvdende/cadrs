//! **Configurations** in the app (Onshape's Configuration panel; the model is
//! [`cadrs_core::configuration`]).
//!
//! - **The Configuration panel** (the tab bar's `panel-configuration`, icon `configurations`;
//!   Part Studios only) docks beside the viewport. **Add input** adds a **List** or a
//!   **Checkbox**, its name open for editing. Each input has its title (a menu: **Rename**,
//!   **Move up**, **Move down**, **Delete**, and for a Checkbox **Checked by default**) and its
//!   configuration table, the same one the node view's input nodes show
//!   ([`crate::config_table`]): a column per configuration (click one to show it: its ✓ and
//!   its column tinted), a row per configured parameter, + Output.
//!
//!   Values typed in are Variable features (or a Variable Studio's): a List's column can set a
//!   Variable feature's value.
//! - **Configure ▸** on a feature's menu ([`configure_entries`]): its fields, a sketch's
//!   dimensions or a Variable's value, each adding a column to a List or Checkbox (a ✓ on the
//!   configured ones: picking one again removes its column). Dynamic suppression ▸
//!   **Suppress by configuration** configures whether it is suppressed. With no List or
//!   Checkbox yet, a List named "Configuration" is made, with the row "Default".
//! - **Configure ▸** on a part's menu in the Parts list ([`configure_property_entries`]): its
//!   Name, Part number, Description, Revision, Vendor, Category and custom properties (not
//!   Mass, Unit of measure, Material or Appearance), each a text column "Part 1 · Part number"
//!   (an empty cell: not set). The Properties dialog then edits the configuration shown.
//! - **Assembly instances**: the instance menu's **Configuration ▸** ([`instance_menu_item`])
//!   picks each input's value for the selected instances of the same Part Studio (a List's
//!   rows, a Checkbox). Instances inserted from a
//!   configured Part Studio take its configuration then.
//! - **A value a feature can't take** ([`reveal_problem`]): a sketch dimension typed as
//!   `#testcircle` while that output is 0 mm in the configuration shown opens the panel and the
//!   node view at its cell, outlined red in both, the panel's focused to be typed in.
//!
//! Names: the panel `configuration-panel` (`configuration-panel-close`), `configuration-add-input`
//! (its menu's `configuration-add-list`, `-checkbox`), the scrolled tables
//! `configuration-scroll`, each input's title `configuration-input-<k>`, its table's controls
//! `configuration-config-<input>-…` and `configuration-output-<name>-…`
//! ([`crate::config_table`]), the name editor `configuration-name`.

use bevy::input_focus::{FocusCause, InputFocus};
use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_core::ElementId;
use cadrs_core::assembly::InstanceSource;
use cadrs_core::configuration::{
    self as cfg, ConfigInput, ConfigValues, Configuration, EditConfiguration, InputKind,
    InputValue, ParamTarget, SetInstanceConfiguration, UnconfigureParameter,
};
use cadrs_core::document::Element;
use cadrs_core::{ConfigInputId, FeatureId};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::menu::{ContextMenuAnchor, MenuEntry};
use cadrs_ui::prelude::*;
use cadrs_ui::{CheckboxChange, InlineEditOptions, TextInputField, begin_inline_edit};

use crate::config_table::{self, ConfigElement, ConfigRole};
use crate::side_panel::{SidePanel, side_panel_header, side_panel_node};
use crate::viewport::ActiveKind;
use crate::{ActiveDocument, AppState};

pub struct ConfigurationPlugin;

impl Plugin for ConfigurationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_panel, sync_button, reveal_problem_cell)
                .chain()
                .after(crate::parts::PartsSet)
                .run_if(in_state(AppState::Document)),
        )
        .init_resource::<PanelWidth>()
        .add_observer(on_panel_resized)
        .add_observer(on_button)
        .add_observer(on_rename)
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

/// What an open menu of the panel is for.
#[derive(Component, Debug, Clone, Copy)]
enum PanelMenu {
    AddInput,
    Input(ConfigInputId),
}

/// An input whose name is to open for editing once the panel shows it (one just added).
#[derive(Resource, Debug, Clone, Copy)]
struct PendingName(ConfigInputId);

/// A configuration value a feature can't take (a sketch dimension typed as `#testcircle` while
/// that output is 0 mm in the configuration shown), set by [`reveal_problem`]: its field is
/// outlined red in the panel and the node view until the dimension's editor closes.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct ConfigProblem {
    pub element: ElementId,
    pub at: ProblemAt,
    /// The panel's field is still to be scrolled to and focused.
    focus: bool,
}

/// Where a [`ConfigProblem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProblemAt {
    /// Output `param`'s value in configuration `row` of a List or Checkbox.
    Cell {
        input: ConfigInputId,
        row: usize,
        param: usize,
    },
    /// A Value input's value.
    Value(ConfigInputId),
}

/// Where the problem is in `element`, if there is one.
pub fn problem_in(world: &World, element: ElementId) -> Option<ProblemAt> {
    world
        .get_resource::<ConfigProblem>()
        .filter(|p| p.element == element)
        .map(|p| p.at)
}

/// Whether `name` is a variable of the active Part Studio's configuration (an output's, a
/// Value's or a Checkbox's).
pub fn is_config_variable(world: &World, name: &str) -> bool {
    studio(world).is_some_and(|(_, el)| el.configuration.names().iter().any(|n| n == name))
}

/// Shows where `#name`, a configuration's variable, has the value a feature can't take: the
/// Configuration panel and the node view open, its cell in the configuration shown (or the
/// Value's field) outlined red in both, the panel's scrolled to and focused to be typed in.
/// Whether `name` is an output's or a Value's.
pub fn reveal_problem(world: &mut World, name: &str) -> bool {
    let Some((element, el)) = studio(world) else {
        return false;
    };
    let c = &el.configuration;
    let resolved = c.resolve(&c.active);
    let at = if let Some((input, _)) = c.column_named(name) {
        let (Some(k), Some(param)) = (
            c.inputs.iter().position(|i| i.id == input.id),
            input.params.iter().position(|p| p.name == name),
        ) else {
            return false;
        };
        let row = match resolved.get(k) {
            Some(cfg::Resolved::Row(i)) => *i,
            Some(cfg::Resolved::Checked(b)) => usize::from(*b),
            _ => return false,
        };
        ProblemAt::Cell {
            input: input.id,
            row,
            param,
        }
    } else if let Some(i) = c
        .input_named(name)
        .filter(|i| matches!(i.kind, InputKind::Value { .. }))
    {
        ProblemAt::Value(i.id)
    } else {
        return false;
    };
    world.insert_resource(ConfigProblem {
        element,
        at,
        focus: true,
    });
    if let Some(mut open) = world.get_resource_mut::<SidePanel>() {
        *open = SidePanel::Configuration;
    }
    if let Some(mut view) = world.get_resource_mut::<crate::node_view::NodeView>() {
        view.set(element, true);
    }
    true
}

/// Takes away the problem's red outline (its editor closed).
pub fn clear_problem(world: &mut World) {
    world.remove_resource::<ConfigProblem>();
}

/// The ✓ on the configuration shown.
pub(crate) const SHOWN_CHECK: Color = Color::srgb_u8(0x2e, 0x9e, 0x44);

/// The panel's left edge, dragged to make it wider or narrower.
const PANEL_EDGE: &str = "configuration-panel-edge";

/// The panel's width as its left edge was last dragged to (kept for the session); `None`: as
/// wide as its widest table.
#[derive(Resource, Debug, Clone, Copy, Default)]
struct PanelWidth(Option<f32>);

fn on_panel_resized(
    ev: On<cadrs_ui::SplitterMoved>,
    q: Query<&Name>,
    mut width: ResMut<PanelWidth>,
) {
    if q.get(ev.entity).is_ok_and(|n| n.as_str() == PANEL_EDGE) {
        width.0 = Some(ev.height);
    }
}

/// The left inset of an input's title and table (px).
const INSET: f32 = 10.0;

/// The active Part Studio, if it is one.
fn studio(world: &World) -> Option<(ElementId, Element)> {
    let d = world.get_resource::<ActiveDocument>()?;
    let el = d.active_element()?;
    matches!(el.kind, cadrs_core::ElementKind::PartStudio { .. }).then(|| (el.id, el.clone()))
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

// ---- The panel ---------------------------------------------------------------------------

fn sync_panel(
    doc: Option<Res<ActiveDocument>>,
    open: Res<SidePanel>,
    kind: Res<ActiveKind>,
    problem: Option<Res<ConfigProblem>>,
    mut last_problem: Local<Option<ProblemAt>>,
    q_panel: Query<Entity, With<ConfigurationPanel>>,
    mut commands: Commands,
) {
    let now = problem.as_ref().map(|p| p.at);
    let problem_changed = *last_problem != now;
    *last_problem = now;
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
        || problem_changed
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

/// A column's heading ("Extrude 1 · Depth", "Part 1 · Part number"): parts named as the Parts
/// list shows them.
pub(crate) fn target_label(world: &World, el: &Element, target: &ParamTarget) -> String {
    let settings = world
        .get_resource::<ActiveDocument>()
        .map(|d| d.doc.properties.clone())
        .unwrap_or_default();
    let cache = world.get_resource::<crate::parts::PartCache>();
    let name = |part: cadrs_core::PartId| {
        cache
            .and_then(|c| c.part_name(part))
            .map(str::to_string)
            .or_else(|| el.part_prop(part).and_then(|p| p.name.clone()))
            .unwrap_or_else(|| tr("view-part-fallback-name"))
    };
    target.label(el.features(), &settings, &name)
}

fn rebuild(world: &mut World) {
    let Some((element, el)) = studio(world) else {
        return;
    };
    let c = el.configuration.clone();
    let resolved = c.resolve(&c.active);
    let units = world.resource::<ActiveDocument>().doc.units;
    let problem = problem_in(world, element);
    let theme_key = format!("{:?}", world.resource::<Theme>().background);
    // (An output's kind and label follow the feature it configures.)
    let outputs: Vec<(cfg::ColumnKind, String)> = c
        .inputs
        .iter()
        .flat_map(|i| &i.params)
        .map(|p| {
            let label = p
                .target
                .label(el.features(), &Default::default(), &|_| String::new());
            (p.kind(el.features()), label)
        })
        .collect();
    let key = PanelKey(format!("{c:?}{units:?}{problem:?}{theme_key}{outputs:?}"));
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
    let widest = c
        .inputs
        .iter()
        .map(config_table::table_width)
        .fold(0.0, f32::max);
    let width = world
        .resource::<PanelWidth>()
        .0
        .unwrap_or_else(|| (widest + 2.0 * INSET + 16.0).clamp(280.0, 760.0));
    let panel = world
        .spawn((
            Name::new("configuration-panel"),
            ConfigurationPanel,
            ConfigElement(element),
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
            // (Not clipped: its left edge's grip reaches into the gap beside it. The tables
            // scroll and clip themselves.)
            n.overflow = Overflow::visible();
        });
    let mut body = None;
    let mut commands = world.commands();
    commands.entity(panel).with_children(|p| {
        p.spawn(cadrs_ui::left_edge(
            PANEL_EDGE,
            panel,
            crate::document::PANEL_GAP + 1.0,
            320.0,
            240.0,
        ));
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
        body = Some(
            p.spawn((
                Name::new("configuration-scroll"),
                Node {
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll(),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    padding: UiRect::new(Val::Px(INSET), Val::Px(INSET), Val::ZERO, Val::Px(8.0)),
                    ..default()
                },
            ))
            .id(),
        );
    });
    world.flush();
    // The inputs, each its title and its table (built in the world, as the node view's are).
    if let Some(body) = body {
        let features = el.features().to_vec();
        world.entity_mut(body).with_children(|b| {
            for (k, (input, r)) in c.inputs.iter().zip(&resolved).enumerate() {
                title(b, &t, k, input);
                config_table::spawn_table(
                    b,
                    &t,
                    &config_table::TableSpec {
                        prefix: "configuration",
                        input,
                        resolved: *r,
                        features: &features,
                        units,
                        problem,
                        link: config_table::ColumnsLink {
                            element,
                            input: input.id,
                            panel: true,
                        },
                    },
                );
            }
        });
    }
    let mut q_area =
        world.query_filtered::<(Entity, &ChildOf), With<crate::side_panel::SideDock>>();
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

/// An input's title: its name (renamed in place), its kind, its menu.
fn title(b: &mut ChildSpawner, t: &Theme, k: usize, input: &ConfigInput) {
    let icon = match input.kind {
        InputKind::List { .. } => "list",
        InputKind::Checkbox { .. } => "check",
        InputKind::Value { .. } => "variables",
    };
    let mut title = b.spawn((
        TreeItem::new(format!("configuration-input-{}", k + 1), input.name.clone())
            .icon(icon, 15.0)
            .icon_color(t.muted_foreground)
            .left(0.0)
            .weight(FontWeight::MEDIUM)
            .editable()
            .trailing(Some(input.kind.label().to_string()), t.muted_foreground)
            .build(t),
        InputTitle(input.id),
        ContextMenuTarget,
        Tooltip::new(tr_with("config-input-tooltip", &[("name", &input.name)])),
    ));
    if let Some(mut n) = title.get_mut::<Node>() {
        n.height = Val::Px(28.0);
        n.margin.top = Val::Px(if k == 0 { 2.0 } else { 10.0 });
        n.flex_shrink = 0.0;
    }
}

/// Puts the problem's field in the panel in view and gives it the focus, once the panel shows
/// it (given up after a second).
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn reveal_problem_cell(
    problem: Option<ResMut<ConfigProblem>>,
    mut frames: Local<u32>,
    q_panel: Query<Entity, With<ConfigurationPanel>>,
    q_roles: Query<(
        Entity,
        &ConfigRole,
        &Children,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
    )>,
    q_parent: Query<&ChildOf>,
    q_field: Query<(), With<TextInputField>>,
    mut q_scroll: Query<(
        &Name,
        &ComputedNode,
        &bevy::ui::UiGlobalTransform,
        &mut ScrollPosition,
    )>,
    mut focus: ResMut<InputFocus>,
) {
    let Some(mut problem) = problem.filter(|p| p.focus) else {
        *frames = 0;
        return;
    };
    *frames += 1;
    if *frames > 60 {
        problem.focus = false;
        return;
    }
    let Ok(panel) = q_panel.single() else {
        return;
    };
    let at = problem.at;
    // (The node view has the same field: the panel's.)
    let found = q_roles
        .iter()
        .filter(|(_, r, ..)| match (r, at) {
            (ConfigRole::Cell(i, j, k), ProblemAt::Cell { input, row, param }) => {
                *i == input && *j == param && *k == row
            }
            (ConfigRole::Value(i), ProblemAt::Value(v)) => *i == v,
            _ => false,
        })
        .find(|(e, ..)| q_parent.iter_ancestors(*e).any(|a| a == panel));
    let Some((_, _, kids, node, at)) = found else {
        return;
    };
    if node.size().x <= 0.0 {
        return;
    }
    if let Some((_, view, view_at, mut pos)) = q_scroll
        .iter_mut()
        .find(|(n, ..)| n.as_str() == "configuration-scroll")
    {
        let s = view.inverse_scale_factor();
        let rect = |n: &ComputedNode, g: &bevy::ui::UiGlobalTransform| {
            Rect::from_center_size(g.translation * s, n.size() * s)
        };
        let (cell, shown) = (rect(node, at), rect(view, view_at));
        let max = ((view.content_size() - view.size()) * s).max(Vec2::ZERO);
        // (Room above it for its configuration's heading, and as little scrolled sideways as
        // will show it, so the outputs' names stay in view where they can.)
        let (before, after) = (
            Vec2::new(16.0, 48.0).min(shown.size() / 3.0),
            Vec2::new(16.0, 16.0),
        );
        let mut want = pos.0;
        for axis in 0..2 {
            let (lo, hi) = (
                shown.min[axis] + before[axis],
                shown.max[axis] - after[axis],
            );
            if cell.min[axis] < lo {
                want[axis] -= lo - cell.min[axis];
            } else if cell.max[axis] > hi {
                want[axis] += cell.max[axis] - hi;
            }
            want[axis] = want[axis].clamp(0.0, max[axis]);
        }
        if want != pos.0 {
            pos.0 = want;
        }
    }
    if let Some(field) = kids.iter().find(|k| q_field.contains(*k)) {
        focus.set(field, FocusCause::Navigated);
    }
    problem.focus = false;
}

/// Opens the name of an input just added, once the panel shows it.
fn open_pending(world: &mut World) {
    let Some(PendingName(id)) = world.get_resource::<PendingName>().copied() else {
        return;
    };
    let Some(e) = find_entity::<InputTitle>(world, |t| t.0 == id) else {
        return;
    };
    world.remove_resource::<PendingName>();
    let name = studio(world)
        .and_then(|(_, el)| el.configuration.input(id).map(|i| i.name.clone()))
        .unwrap_or_default();
    begin_rename(world, e, name);
}

fn begin_rename(world: &mut World, e: Entity, name: String) {
    let theme = world.resource::<Theme>().clone();
    let mut opts = InlineEditOptions::new("configuration-name");
    opts.width = Val::Px(160.0);
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
        world.insert_resource(PendingName(id));
    }
    Some(id)
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

// ---- Observers -----------------------------------------------------------------------------

fn on_button(a: On<Activate>, q: Query<&Name>, mut commands: Commands) {
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

/// An input's title renamed in place (a Value's or Checkbox's `#name` wherever it is read).
fn on_rename(ev: On<InlineEditCommit>, q_title: Query<&InputTitle>, mut commands: Commands) {
    let Ok(t) = q_title.get(ev.entity).copied() else {
        return;
    };
    let name = ev.value.trim().to_string();
    if name.is_empty() {
        return;
    }
    commands.queue(move |world: &mut World| {
        if let Some((element, _)) = studio(world) {
            config_table::rename_input(world, element, t.0, name);
        }
    });
}

fn on_context_menu(
    ev: On<ContextMenuRequested>,
    q_title: Query<&InputTitle>,
    mut commands: Commands,
) {
    let at = ev.position;
    let Ok(t) = q_title.get(ev.entity).copied() else {
        return;
    };
    let id = t.0;
    commands.queue(move |world: &mut World| {
        let Some((_, el)) = studio(world) else { return };
        let c = &el.configuration;
        let Some(i) = c.input(id) else { return };
        let k = c.inputs.iter().position(|x| x.id == id).unwrap_or(0);
        let mut m = Menu::new("configuration-menu")
            .min_width(180.0)
            .item_height(22.0)
            .item(MenuItem::new(
                "configuration-input-rename",
                tr("common-rename"),
            ))
            .item(MenuItem::new("configuration-input-up", tr("config-move-up")).disabled(k == 0))
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
        let m = m.separator().item(
            MenuItem::new("configuration-input-delete", tr("common-delete")).icon("remove-circle"),
        );
        open_menu_at(world, at, m, PanelMenu::Input(id));
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
    let full = target.label(el.features(), &Default::default(), &|_| String::new());
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
    configure_target(world, element, &el, target, input);
}

/// Configures `target` by the `input`th table input (any when `None`; a new List when there is
/// none), or removes its column when it is configured already; opens the Configuration panel.
fn configure_target(
    world: &mut World,
    element: ElementId,
    el: &Element,
    target: ParamTarget,
    input: Option<usize>,
) {
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
    let Some(now) = cfg::current(&target, el.features(), el.suppressed(), el.part_props()) else {
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
        // (Named after the parameter by the edit: `#Extrude_1_Depth`.)
        i.params.push(cfg::ConfiguredParam {
            target,
            values: vec![now; n],
            name: String::new(),
        });
        done = true;
        true
    });
    if done {
        *world.resource_mut::<SidePanel>() = SidePanel::Configuration;
        if let Some(id) = new_input {
            world.insert_resource(PendingName(id));
        }
    }
}

// ---- Configure ▸ (the part menu) -----------------------------------------------------------

/// The part menu's **Configure ▸** entries for `part`'s properties (Name, Part number, …,
/// custom ones; not Mass, Unit of measure, Material or Appearance): item `part-configure-<p>`
/// (or `part-configure-<p>-<i>` with several Lists and Checkboxes), `p` indexing
/// [`cfg::configurable_properties`] and `i` the table inputs. A ✓ on the configured ones.
pub fn configure_property_entries(
    el: &Element,
    settings: &cadrs_core::properties::PropertySettings,
    part: cadrs_core::PartId,
) -> MenuItem {
    let c = &el.configuration;
    let inputs = table_inputs(c);
    let entries: Vec<MenuEntry> = cfg::configurable_properties(part, settings)
        .iter()
        .enumerate()
        .map(|(p, t)| {
            let ParamTarget::Property { key, .. } = t else {
                unreachable!("configurable_properties gives properties")
            };
            let label = key.label(settings);
            let by = c.param(t).map(|(i, _)| i.id);
            let item = MenuItem::new(format!("part-configure-{p}"), label).checked(by.is_some());
            if inputs.len() > 1 {
                item.submenu(
                    inputs
                        .iter()
                        .enumerate()
                        .map(|(k, i)| {
                            MenuItem::new(format!("part-configure-{p}-{k}"), i.name.clone())
                                .checked(by == Some(i.id))
                                .into()
                        })
                        .collect(),
                )
                .into()
            } else {
                item.into()
            }
        })
        .collect();
    MenuItem::new("part-configure", tr("config-configure"))
        .icon("configurations")
        .submenu(entries)
}

/// A part menu's Configure ▸ item (`part-configure-<p>[-<i>]`): configures the property (or
/// removes its column), as [`configure_action`] does for a feature's parameter.
pub fn configure_property_action(world: &mut World, part: cadrs_core::PartId, item: &str) {
    let Some((element, el)) = studio(world) else {
        return;
    };
    let Some(rest) = item.strip_prefix("part-configure-") else {
        return;
    };
    let mut it = rest.split('-').map(|s| s.parse::<usize>().ok());
    let (Some(Some(p)), k) = (it.next(), it.next().flatten()) else {
        return;
    };
    let settings = world.resource::<ActiveDocument>().doc.properties.clone();
    let Some(target) = cfg::configurable_properties(part, &settings)
        .get(p)
        .cloned()
    else {
        return;
    };
    configure_target(world, element, &el, target, k);
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

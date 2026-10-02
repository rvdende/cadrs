//! The toolbar's **Search tools** box (P3.9, PS2.3): clicking it or pressing Alt+C opens a
//! field over it with a list of the tools whose names match what is typed (a
//! [`cadrs_ui::CommandPalette`]). The tools are the toolbar's own buttons, whatever the toolbar
//! shows (the Part Studio's features, or the sketch tools while sketching), plus the pattern
//! menu's three patterns and (P3.11) every variant in a sketch tool's ▾ menu (Midpoint line,
//! Tangent arc, the constraints, …), each launched directly; tools cadrs doesn't have yet are
//! listed greyed. ↑/↓ move the
//! highlight, Enter or a click launches the tool (as its toolbar button would), Esc or a click
//! elsewhere closes the list.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::EditableText;
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, Button as WidgetButton};
use cadrs_ui::prelude::*;
use cadrs_ui::{CommandPalette, CommandPaletteClose, CommandPaletteResults, TextInputField, palette_row};

use crate::AppState;

pub struct SearchToolsPlugin;

impl Plugin for SearchToolsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SearchState>()
            .init_resource::<SearchClosedFrame>()
            .add_systems(Update, (open_on_key, update_results).chain().run_if(in_state(AppState::Document)))
            .add_systems(OnExit(AppState::Document), |mut s: ResMut<SearchState>| *s = SearchState::default())
            .add_observer(on_box_click)
            .add_observer(on_close)
            .add_observer(on_submit)
            .add_observer(on_cancel)
            .add_observer(on_row)
            .add_observer(on_arrow_keys);
    }
}

const PALETTE: &str = "search-tools-palette";

/// The frame the search closed in (P3.11): the Enter or Esc that closed it isn't also the
/// sketch's (Enter would accept the sketch the chosen tool is for).
#[derive(Resource, Debug, Default, Clone, Copy)]
pub struct SearchClosedFrame(pub Option<u32>);

/// A tool the search can launch.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolEntry {
    /// Its id in the list (the toolbar button's name).
    pub id: String,
    pub label: String,
    pub icon: String,
    pub shortcut: Option<String>,
    pub enabled: bool,
    pub launch: Launch,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Launch {
    /// Activate this toolbar button.
    Button(Entity),
    Pattern(cadrs_core::pattern::PatternKind),
    /// P3I.4: a sheet metal tool of the Sheet metal model's ▾.
    Applied(crate::applied::AppliedKind),
    /// A variant in a sketch tool button's ▾ menu (P3.11): made the button's and the active
    /// tool, as the menu does.
    SketchVariant(Entity, crate::sketch::SketchTool),
}

/// The open search: the palette entity, the tools, the query the rows were built for, the
/// matches and the highlighted one.
#[derive(Resource, Debug, Default)]
struct SearchState {
    palette: Option<Entity>,
    tools: Vec<ToolEntry>,
    query: Option<String>,
    matches: Vec<usize>,
    highlight: usize,
}

/// The tools whose name matches `query` (case-insensitive; words in any order), the enabled
/// ones first, in toolbar order; all of them for an empty query.
pub fn matching(tools: &[ToolEntry], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    let mut hits: Vec<usize> = tools
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            let l = t.label.to_lowercase();
            words.iter().all(|w| l.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect();
    // Names starting with the query first, then the enabled ones.
    let first = words.first().cloned().unwrap_or_default();
    hits.sort_by_key(|i| (!tools[*i].label.to_lowercase().starts_with(&first), !tools[*i].enabled));
    hits
}

/// The toolbar's buttons as tools.
fn gather(world: &mut World) -> Vec<ToolEntry> {
    let mut q_names = world.query::<(Entity, &Name)>();
    let Some(toolbar) = q_names.iter(world).find(|(_, n)| n.as_str() == "toolbar").map(|(e, _)| e) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut stack = vec![toolbar];
    let mut order = Vec::new();
    while let Some(e) = stack.pop() {
        order.push(e);
        if let Some(children) = world.get::<Children>(e) {
            stack.extend(children.iter().rev());
        }
    }
    for e in order {
        let (Some(name), Some(tip)) = (world.get::<Name>(e), world.get::<Tooltip>(e)) else { continue };
        if world.get::<WidgetButton>(e).is_none() {
            continue;
        }
        let id = name.as_str().to_string();
        let label = tip.text.clone();
        let shortcut = tip.shortcut.clone();
        let enabled = world.get::<InteractionDisabled>(e).is_none();
        let icon = world
            .get::<Children>(e)
            .and_then(|c| c.iter().find_map(|x| world.get::<cadrs_ui::Icon>(x).map(|i| i.name.to_string())))
            .unwrap_or_else(|| "search".into());
        if id == "pattern" {
            // The pattern button opens a menu: its three patterns are tools of their own.
            use cadrs_core::pattern::PatternKind as K;
            for (k, label, icon) in [
                (K::Linear, "Linear pattern", "linear-pattern"),
                (K::Circular, "Circular pattern", "circular-pattern"),
                (K::Curve, "Curve pattern", "spline"),
            ] {
                out.push(ToolEntry {
                    id: label.to_lowercase().replace(' ', "-"),
                    label: label.into(),
                    icon: icon.into(),
                    shortcut: None,
                    enabled,
                    launch: Launch::Pattern(k),
                });
            }
            continue;
        }
        // A sketch tool with a ▾ (Line, Rectangle, Circle, …): each of its variants is a tool,
        // launched directly rather than opening the menu (P3.11, P3.9 judge).
        if let Some(b) = world.get::<crate::sketch::SketchToolButton>(e).copied() {
            let variants = crate::sketch::button_variants(b);
            if !variants.is_empty() {
                for (vid, vlabel, vicon, key, tool) in variants {
                    out.push(ToolEntry {
                        id: vid,
                        label: vlabel,
                        icon: vicon,
                        shortcut: key,
                        enabled: enabled && tool.is_some_and(|t| t.implemented()),
                        launch: match tool {
                            Some(t) => Launch::SketchVariant(e, t),
                            None => Launch::Button(e),
                        },
                    });
                }
                continue;
            }
        }
        // P3I.2 (X1): the sheet metal tools of the Sheet metal model button's ▾ (greyed until
        // they are built).
        let sheet_metal = id == "sheet-metal-model";
        out.push(ToolEntry { id, label, icon, shortcut, enabled, launch: Launch::Button(e) });
        if sheet_metal {
            for (tid, tlabel, ticon) in crate::sheetmetal_ui::OTHER_TOOLS {
                let launch = match crate::sheetmetal_features_ui::SmTool::of_name(tid) {
                    Some(t) => Launch::Applied(crate::applied::AppliedKind::SmFeature(t)),
                    None => Launch::Button(e),
                };
                out.push(ToolEntry { id: tid.into(), label: tlabel.into(), icon: ticon.into(), shortcut: None, enabled: enabled && crate::sheetmetal_features_ui::built(tid), launch });
            }
        }
    }
    out
}

/// Opens the search (or focuses it again).
pub fn open(world: &mut World) {
    if let Some(p) = world.resource::<SearchState>().palette
        && world.get_entity(p).is_ok()
    {
        return;
    }
    let mut q = world.query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>();
    let Some((left, top, width)) = q.iter(world).find(|(n, ..)| n.as_str() == "search-tools").map(|(_, node, t)| {
        let s = node.inverse_scale_factor();
        let size = node.size() * s;
        let c = t.translation * s;
        (c.x - size.x / 2.0, c.y - size.y / 2.0, size.x)
    }) else {
        return;
    };
    // As wide as the box, or wider to the left (the box is at the toolbar's right end).
    let w = width.max(250.0);
    let tools = gather(world);
    let theme = world.resource::<Theme>().clone();
    let palette = world
        .spawn((
            CommandPalette::new(PALETTE).placeholder("Search tools…").at(left + width - w, top, w).build(&theme),
            DespawnOnExit(AppState::Document),
        ))
        .id();
    let mut s = world.resource_mut::<SearchState>();
    *s = SearchState { palette: Some(palette), tools, query: None, matches: Vec::new(), highlight: 0 };
}

fn close(world: &mut World) {
    let frame = world.resource::<bevy::diagnostic::FrameCount>().0;
    world.resource_mut::<SearchClosedFrame>().0 = Some(frame);
    let p = world.resource_mut::<SearchState>().palette.take();
    if let Some(p) = p
        && let Ok(e) = world.get_entity_mut(p)
    {
        e.despawn();
    }
    world.resource_mut::<InputFocus>().clear();
}

fn launch(world: &mut World, i: usize) {
    let Some(tool) = world.resource::<SearchState>().tools.get(i).cloned() else { return };
    if !tool.enabled {
        return;
    }
    close(world);
    match tool.launch {
        Launch::Button(e) => {
            if world.get_entity(e).is_ok() {
                world.trigger(Activate { entity: e });
            }
        }
        Launch::Pattern(k) => crate::applied::begin(world, crate::applied::AppliedKind::Pattern(k)),
        Launch::Applied(k) => crate::applied::begin(world, k),
        Launch::SketchVariant(e, t) => {
            if world.get_entity(e).is_ok() {
                crate::sketch::choose_variant(world, e, t);
            }
        }
    }
}

fn on_box_click(click: On<Pointer<Click>>, q: Query<&Name>, mut commands: Commands) {
    if click.button == PointerButton::Primary && q.get(click.entity).is_ok_and(|n| n.as_str() == "search-tools") {
        commands.queue(open);
    }
}

/// Alt+C opens the search, unless a text field has focus.
fn open_on_key(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<cadrs_ui::DialogRoot>>,
    mut commands: Commands,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let alt = keys.any_pressed([KeyCode::AltLeft, KeyCode::AltRight]);
    for k in keys_in.read() {
        if k.state == ButtonState::Pressed && k.key_code == KeyCode::KeyC && alt && !typing && q_dialogs.is_empty() {
            commands.queue(open);
        }
    }
}

/// Rebuilds the rows when the text changes.
fn update_results(
    mut state: ResMut<SearchState>,
    q_text: Query<(&Name, &EditableText)>,
    mut q_results: Query<(Entity, &mut Node), With<CommandPaletteResults>>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    if state.palette.is_none() {
        return;
    }
    let field = format!("{PALETTE}-input-field");
    let Some(text) = q_text.iter().find(|(n, _)| n.as_str() == field).map(|(_, t)| t.value().to_string()) else {
        return;
    };
    let Some((results, mut node)) = q_results.iter_mut().next() else { return };
    if state.query.as_deref() == Some(text.as_str()) && !state.is_changed() {
        return;
    }
    if state.query.as_deref() != Some(text.as_str()) {
        state.matches = if text.trim().is_empty() { Vec::new() } else { matching(&state.tools, &text) };
        state.highlight = 0;
        state.query = Some(text);
    }
    let shown: Vec<usize> = state.matches.iter().copied().take(12).collect();
    node.display = if shown.is_empty() { Display::None } else { Display::Flex };
    commands.entity(results).despawn_children();
    let t = theme.clone();
    let rows: Vec<(ToolEntry, bool)> =
        shown.iter().enumerate().map(|(k, i)| (state.tools[*i].clone(), k == state.highlight)).collect();
    commands.entity(results).with_children(|r| {
        for (tool, lit) in rows {
            r.spawn((
                palette_row(&t, PALETTE, &tool.id, &tool.label, &tool.icon, tool.shortcut.as_deref(), !tool.enabled, lit),
                SearchRow,
            ));
        }
        if state.matches.is_empty() {
            r.spawn((t.text("No tools found", t.font_sm, FontWeight::NORMAL, t.muted_foreground), Pickable::IGNORE));
        }
    });
}

#[derive(Component)]
struct SearchRow;

fn on_row(a: On<Activate>, q: Query<&Name, With<SearchRow>>, state: Res<SearchState>, mut commands: Commands) {
    let Ok(name) = q.get(a.entity) else { return };
    let prefix = format!("{PALETTE}-item-");
    let Some(id) = name.as_str().strip_prefix(&prefix) else { return };
    if let Some(i) = state.tools.iter().position(|t| t.id == id) {
        commands.queue(move |world: &mut World| launch(world, i));
    }
}

fn is_palette_field(world_names: &Query<&Name>, e: Entity) -> bool {
    world_names.get(e).is_ok_and(|n| n.as_str() == format!("{PALETTE}-input-field"))
}

fn on_submit(ev: On<cadrs_ui::TextSubmit>, q: Query<&Name>, state: Res<SearchState>, mut commands: Commands) {
    if ev.entity != ev.original_event_target() || !is_palette_field(&q, ev.entity) {
        return;
    }
    if let Some(i) = state.matches.get(state.highlight).copied() {
        commands.queue(move |world: &mut World| launch(world, i));
    }
}

fn on_cancel(ev: On<cadrs_ui::TextCancel>, q: Query<&Name>, mut commands: Commands) {
    if ev.entity == ev.original_event_target() && is_palette_field(&q, ev.entity) {
        commands.queue(close);
    }
}

fn on_close(_ev: On<CommandPaletteClose>, mut commands: Commands) {
    commands.queue(close);
}

/// ↑ and ↓ move the highlight.
fn on_arrow_keys(ev: On<FocusedInput<KeyboardInput>>, q: Query<&Name>, mut state: ResMut<SearchState>) {
    if ev.input.state != ButtonState::Pressed || !is_palette_field(&q, ev.focused_entity) {
        return;
    }
    let n = state.matches.len().min(12);
    if n == 0 {
        return;
    }
    match ev.input.key_code {
        KeyCode::ArrowDown => state.highlight = (state.highlight + 1) % n,
        KeyCode::ArrowUp => state.highlight = (state.highlight + n - 1) % n,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(label: &str, enabled: bool) -> ToolEntry {
        ToolEntry {
            id: label.to_lowercase(),
            label: label.into(),
            icon: "x".into(),
            shortcut: None,
            enabled,
            launch: Launch::Pattern(cadrs_core::pattern::PatternKind::Linear),
        }
    }

    #[test]
    fn search_finds_tools_by_name() {
        let tools = vec![tool("Sketch", true), tool("Extrude", true), tool("Fillet", true), tool("Draft", false), tool("Linear pattern", true), tool("Circular pattern", true)];
        let labels = |q: &str| matching(&tools, q).into_iter().map(|i| tools[i].label.clone()).collect::<Vec<_>>();
        assert_eq!(labels("ext"), ["Extrude"]);
        assert_eq!(labels("FIL"), ["Fillet"]);
        assert_eq!(labels("pattern"), ["Linear pattern", "Circular pattern"]);
        assert_eq!(labels("pattern circ"), ["Circular pattern"]);
        // A name starting with the text comes first; missing tools after the others.
        assert_eq!(labels("d"), ["Draft", "Extrude"]);
        assert!(labels("zzz").is_empty());
    }
}

//! The keyboard shortcuts dialog (Shift+/ or Help ▾ › Keyboard shortcuts), laid out like
//! Onshape's (`reference/onshape/shortcuts/keyboard-shortcut-dialog-03.png`): a 20 px title,
//! a "Search shortcuts" box in the title row, tabs General | Part Studio | Assembly | 3D view |
//! Sketch | Drawing, one row per shortcut with right-aligned keycaps and the action (a 🔒 marks bindings
//! that cannot be changed), and a legend at the bottom. Shortcuts cadrs does not have yet are
//! listed struck through, like Onshape's disabled ones. [`SHORTCUTS`] is also what the tests
//! check the key handlers against.

use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::InputFocus;
use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight, Strikethrough, StrikethroughColor};
use bevy::ui_widgets::ScrollArea;
use cadrs_ui::input::TextInputField;
use cadrs_ui::prelude::*;
use cadrs_ui::{DialogRoot, TabStrip, TabStripState};

pub struct ShortcutsPlugin;

impl Plugin for ShortcutsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (open_on_key, sync_rows));
    }
}

/// The dialog's tabs.
pub const TABS: [&str; 6] = ["General", "Part Studio", "Assembly", "3D view", "Sketch", "Drawing"];

/// Cannot be rebound (shown with a lock, like Onshape's "Onshape controlled").
pub const LOCKED: u8 = 1;
/// Not in cadrs yet (shown struck through, like Onshape's "Disabled").
pub const OFF: u8 = 2;

/// (tab index, keys, action, flags). Keys: `+` joins a chord, ` / ` separates alternatives.
pub const SHORTCUTS: &[(usize, &str, &str, u8)] = &[
    (0, "Shift+Enter", "Accept & repeat command", LOCKED),
    (0, "Enter", "Accept command", LOCKED),
    (0, "Escape", "Cancel", LOCKED),
    (0, "Space", "Clear selection", LOCKED),
    (0, "Ctrl+C", "Copy", LOCKED | OFF),
    (0, "Shift+C", "Curve/surface analysis", OFF),
    (0, "Delete / Backspace", "Delete selection", LOCKED),
    (0, "Shift+D", "Dihedral analysis", OFF),
    (0, "Shift+/", "Keyboard shortcuts", 0),
    (0, "[", "Measure", 0),
    (0, "Ctrl+V", "Paste", LOCKED | OFF),
    (0, "Ctrl+Y / Ctrl+Shift+Z", "Redo", LOCKED),
    (0, "`", "Select other", OFF),
    (0, "Alt+C", "Search tools", 0),
    (0, "S", "Shortcut toolbar", OFF),
    (0, "Alt+T", "Tab manager", OFF),
    (0, "Ctrl+Space", "Cycle recent tabs", OFF),
    (0, "Ctrl+Z", "Undo", LOCKED),
    (1, "Delete", "Delete selected features", LOCKED),
    (1, "Shift+E", "Extrude", 0),
    (1, "Shift+F", "Fillet", 0),
    (1, "Shift+W", "Revolve", 0),
    (1, "Shift+H", "Show or hide all sketches", OFF),
    (1, "Shift+S", "Sketch", 0),
    (1, "K", "Show or hide mate connectors", 0),
    (1, "Ctrl+M", "Mate connector", 0),
    (2, "M", "Fastened mate", 0),
    (2, "I", "Insert parts and assemblies", 0),
    (2, "Y", "Hide the instance under the pointer", 0),
    (2, "Shift+Y", "Show all instances", 0),
    (2, "Shift+S", "Snap mode", OFF),
    (2, "Ctrl+M", "Mate connector", 0),
    (2, "Shift+N", "Rename the selected instance", OFF),
    (2, "J", "Show or hide mates", 0),
    (2, "K", "Show or hide mate connectors", 0),
    (2, "H", "Toggle show-mates mode", 0),
    (3, "Shift+1", "Front view", 0),
    (3, "Shift+2", "Back view", 0),
    (3, "Shift+3", "Left view", 0),
    (3, "Shift+4", "Right view", 0),
    (3, "Shift+5", "Top view", 0),
    (3, "Shift+6", "Bottom view", 0),
    (3, "Shift+7", "Isometric view", 0),
    (3, "N", "Normal to", 0),
    (3, "F", "Zoom to fit", 0),
    (3, "W", "Zoom to window", OFF),
    (3, "Z", "Zoom out", 0),
    (3, "Shift+Z", "Zoom in", 0),
    (3, "← / → / ↑ / ↓", "Rotate 15°", 0),
    (3, "Shift+←", "Rotate 90° (any arrow)", 0),
    (3, "Ctrl+←", "Rotate 5° (any arrow)", 0),
    (3, "Ctrl+Shift+←", "Pan (any arrow)", 0),
    (3, "P", "Show or hide planes", 0),
    (3, "Shift+P", "Hide construction geometry", OFF),
    (3, "Shift+I", "Isolate", OFF),
    (3, "Shift+T", "Transparent", OFF),
    (3, "Shift+X", "Section view", OFF),
    (3, "Shift+V", "Named views", OFF),
    (3, "Shift+R", "Render in high quality", OFF),
    (4, "L", "Line", 0),
    (4, "Shift+A", "Switch between line and tangent arc", 0),
    (4, "G", "Corner rectangle", 0),
    (4, "R", "Center point rectangle", 0),
    (4, "C", "Center point circle", 0),
    (4, "A", "3 point arc", 0),
    (4, "Shift+S", "Point", 0),
    (4, "D", "Dimension", 0),
    (4, "Q", "Construction", 0),
    (4, "Ctrl+A", "Select all", 0),
    (4, "U", "Use", 0),
    (4, "O", "Offset", 0),
    (4, "M", "Trim", 0),
    (4, "X", "Extend", 0),
    (4, "Shift+F", "Sketch fillet", 0),
    (4, "I", "Coincident", 0),
    (4, "Shift+O", "Concentric", 0),
    (4, "B", "Parallel", 0),
    (4, "T", "Tangent", 0),
    (4, "H", "Horizontal", 0),
    (4, "V", "Vertical", 0),
    (4, "Shift+L", "Perpendicular", 0),
    (4, "E", "Equal", 0),
    (4, "Shift+M", "Midpoint", 0),
    (4, "Shift+K", "Normal", 0),
    (4, "Shift+G", "Pierce", 0),
    (4, "Shift+Q", "Symmetric", 0),
    (4, "Shift+J", "Fix", 0),
    (4, "Shift+U", "Curvature", OFF),
    // Drawings (X12, P3C.1): the keys of tools still to come are struck through.
    (5, "F", "Zoom to fit the sheet", 0),
    (5, "Ctrl+S", "Sheets flyout", 0),
    (5, "Escape", "End the current tool", LOCKED),
    (5, "D", "Dimension", 0),
    (5, "Shift+R", "Radial dimension", 0),
    (5, "Shift+D", "Diameter dimension", 0),
    (5, "N", "Note", OFF),
    (5, "Ctrl+Q", "Update from this workspace", 0),
    (5, "Tab / Shift+Tab", "Next / previous table cell", OFF),
    (4, "Shift", "Hold to turn off inferencing", LOCKED),
];

/// The dialog root.
#[derive(Component)]
struct ShortcutsDialog;

/// The container the rows are rebuilt into.
#[derive(Component)]
struct ShortcutRows;

/// The shortcuts matching a search (in any tab), or a tab's shortcuts.
pub fn matching(tab: usize, query: &str) -> Vec<(usize, &'static str, &'static str, u8)> {
    let q = query.trim().to_lowercase();
    SHORTCUTS
        .iter()
        .filter(|(t, keys, action, _)| {
            if q.is_empty() {
                *t == tab
            } else {
                action.to_lowercase().contains(&q) || keys.to_lowercase().contains(&q)
            }
        })
        .copied()
        .collect()
}

/// Rows the list shows before it scrolls.
const VISIBLE_ROWS: f32 = 12.0;
const ROW_HEIGHT: f32 = 38.0;
/// The keycap column's width (keycaps are right-aligned in it).
const KEY_COLUMN: f32 = 430.0;

/// Opens the dialog (does nothing if it is open).
pub fn open_shortcuts(world: &mut World) {
    let mut q = world.query_filtered::<(), With<ShortcutsDialog>>();
    if q.iter(world).next().is_some() {
        return;
    }
    let theme = world.resource::<Theme>().clone();
    let (th, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    world.spawn((
        ShortcutsDialog,
        Dialog::new("shortcuts-dialog")
            .title("Keyboard shortcuts")
            .title_font(20.0, FontWeight::NORMAL)
            .header_height(52.0)
            .width(994.0)
            .header(move |h| {
                let t = &th;
                h.spawn((
                    Name::new("shortcuts-customize"),
                    t.text("Customize keyboard shortcuts", 12.0, FontWeight::SEMIBOLD, t.link),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(200.0),
                        top: Val::Px(21.0),
                        ..default()
                    },
                    Tooltip::new("Customizing shortcuts is not available in cadrs yet"),
                ));
                h.spawn(
                    TextInput::new("shortcuts-search")
                        .placeholder("Search shortcuts")
                        .width(Val::Px(194.0))
                        .height(34.0)
                        .build(t),
                )
                .entry::<Node>()
                .and_modify(|mut n| n.margin = UiRect::right(Val::Px(16.0)));
            })
            .body(move |b| {
                let t = &tb;
                b.spawn(Node {
                    justify_content: JustifyContent::Center,
                    margin: UiRect::top(Val::Px(-6.0)),
                    ..default()
                })
                .with_children(|r| {
                    let mut strip = TabStrip::new("shortcuts-tab");
                    for tab in TABS {
                        strip = strip.tab(tab);
                    }
                    r.spawn(strip.build(t));
                });
                // The list sizes to its rows, up to VISIBLE_ROWS, then scrolls.
                b.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|w| {
                    let list = w
                        .spawn((
                            Name::new("shortcuts-list"),
                            ShortcutRows,
                            ScrollArea,
                            Node {
                                max_height: Val::Px(VISIBLE_ROWS * ROW_HEIGHT),
                                flex_direction: FlexDirection::Column,
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .id();
                    w.spawn(cadrs_ui::vertical_scrollbar(t, "shortcuts-scrollbar", list))
                        .entry::<Node>()
                        .and_modify(|mut n| n.right = Val::Px(170.0));
                });
            })
            .footer(move |f| legend(f, &tf))
            .build(&theme),
    ));
}

/// The legend under the list, in a bordered box: what the keycap styles and the lock mean.
fn legend(f: &mut ChildSpawner, t: &Theme) {
    f.spawn((
        Name::new("shortcuts-legend"),
        Node {
            flex_grow: 1.0,
            height: Val::Px(40.0),
            padding: UiRect::horizontal(Val::Px(8.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(4.0)),
            ..default()
        },
        BorderColor::all(Color::srgb_u8(0x88, 0x88, 0x88)),
    ))
    .with_children(|l| {
        l.spawn(dialog_keycap(t, "example".into(), false));
        l.spawn(legend_text(t, "Default"));
        l.spawn(dialog_keycap(t, "example".into(), true));
        l.spawn(legend_text(t, "Not available in cadrs yet"));
        l.spawn(lock(t));
        l.spawn(legend_text(t, "Cannot be changed"));
    });
}

fn legend_text(t: &Theme, s: &str) -> impl Bundle {
    (
        t.text(s, 14.0, FontWeight::NORMAL, t.foreground),
        Node {
            margin: UiRect::right(Val::Px(18.0)),
            ..default()
        },
    )
}

fn lock(t: &Theme) -> impl Bundle {
    cadrs_ui::icon::icon("lock-filled", 16.0, t.foreground)
}

/// Shift+/ opens the dialog (not while typing).
fn open_on_key(
    mut keys_in: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    focus: Res<InputFocus>,
    q_fields: Query<(), With<TextInputField>>,
    q_dialogs: Query<(), With<DialogRoot>>,
    mut commands: Commands,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for k in keys_in.read() {
        if k.state == ButtonState::Pressed
            && k.key_code == KeyCode::Slash
            && shift
            && !ctrl
            && !typing
            && q_dialogs.is_empty()
        {
            commands.queue(open_shortcuts);
        }
    }
}

/// Rebuilds the rows when the tab or the search changes.
#[allow(clippy::type_complexity)]
fn sync_rows(
    q_rows: Query<(Entity, Ref<ShortcutRows>)>,
    q_tabs: Query<(&Name, &TabStripState)>,
    q_search: Query<(&Name, &EditableText), With<TextInputField>>,
    q_tab_buttons: Query<(Entity, &Name, Has<cadrs_ui::Selected>)>,
    theme: Res<Theme>,
    mut last: Local<Option<(usize, String)>>,
    mut commands: Commands,
) {
    let Some((container, marker)) = q_rows.iter().next() else {
        *last = None;
        return;
    };
    let tab = q_tabs
        .iter()
        .find(|(n, _)| n.as_str() == "shortcuts-tab")
        .map_or(0, |(_, s)| s.selected);
    let query = q_search
        .iter()
        .find(|(n, _)| n.as_str() == "shortcuts-search-field")
        .map(|(_, t)| t.value().to_string())
        .unwrap_or_default();
    let key = (tab, query);
    if !marker.is_added() && last.as_ref() == Some(&key) {
        return;
    }
    // While searching, results come from every tab: no tab is underlined (Onshape clears the
    // tab selection and groups the results by tab).
    let searching = !key.1.trim().is_empty();
    for (e, name, selected) in &q_tab_buttons {
        let Some(i) = name
            .as_str()
            .strip_prefix("shortcuts-tab-")
            .and_then(|n| n.parse::<usize>().ok())
        else {
            continue;
        };
        let want = !searching && i == key.0;
        if want && !selected {
            commands.entity(e).insert(cadrs_ui::Selected);
        } else if !want && selected {
            commands.entity(e).remove::<cadrs_ui::Selected>();
        }
    }
    let rows = matching(key.0, &key.1);
    *last = Some(key);
    let t = theme.clone();
    commands.entity(container).despawn_children();
    commands.entity(container).with_children(|c| {
        if rows.is_empty() {
            c.spawn((
                Name::new("shortcuts-empty"),
                t.text("No shortcuts found", t.font_base, FontWeight::NORMAL, t.muted_foreground),
                Node {
                    align_self: AlignSelf::Center,
                    margin: UiRect::vertical(Val::Px(24.0)),
                    ..default()
                },
            ));
        }
        let mut group = None;
        for (i, (tab, keys, action, flags)) in rows.into_iter().enumerate() {
            if searching && group != Some(tab) {
                group = Some(tab);
                c.spawn((
                    Name::new(format!("shortcut-group-{tab}")),
                    t.text(TABS[tab], 14.0, FontWeight::SEMIBOLD, t.foreground),
                    Node {
                        height: Val::Px(30.0),
                        flex_shrink: 0.0,
                        margin: UiRect::left(Val::Px(KEY_COLUMN - 120.0)),
                        align_items: AlignItems::Center,
                        ..default()
                    },
                ));
            }
            let off = flags & OFF != 0;
            c.spawn((
                Name::new(format!("shortcut-row-{i}")),
                Node {
                    height: Val::Px(ROW_HEIGHT),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(13.0),
                    ..default()
                },
            ))
            .with_children(|r| {
                // Keycaps, right-aligned in the left column.
                r.spawn(Node {
                    width: Val::Px(KEY_COLUMN),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|k| {
                    for (a, alt) in keys.split(" / ").enumerate() {
                        if a > 0 {
                            k.spawn(t.text("/", 16.0, FontWeight::NORMAL, t.foreground));
                        }
                        for cap in cadrs_ui::tooltip::keycaps(alt) {
                            k.spawn(dialog_keycap(&t, cap, off));
                        }
                    }
                });
                r.spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(4.0),
                    ..default()
                })
                .with_children(|a| {
                    a.spawn(t.text(action, 14.0, FontWeight::NORMAL, t.foreground));
                    if flags & LOCKED != 0 {
                        a.spawn(lock(&t));
                    }
                });
            });
        }
    });
}

/// A keycap as the dialog draws them: 14 px text in a 1 px grey box; struck through and grey
/// when `off`.
fn dialog_keycap(t: &Theme, key: String, off: bool) -> impl Bundle {
    let color = if off {
        Color::srgb_u8(0xa8, 0xa8, 0xa8)
    } else {
        t.foreground
    };
    (
        Node {
            height: Val::Px(26.0),
            min_width: Val::Px(20.0),
            padding: UiRect::horizontal(Val::Px(6.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        BorderColor::all(Color::srgb_u8(0xa0, 0xa0, 0xa0)),
        BackgroundColor(t.background),
        Children::spawn(bevy::ecs::spawn::SpawnWith({
            let text = t.text(key, 14.0, FontWeight::NORMAL, color);
            move |p: &mut ChildSpawner| {
                let mut e = p.spawn(text);
                if off {
                    e.insert((Strikethrough, StrikethroughColor(color)));
                }
            }
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_and_search() {
        for tab in 0..TABS.len() {
            assert!(matching(tab, "").iter().all(|s| s.0 == tab));
            assert!(matching(tab, "").len() >= 6, "tab {tab}");
        }
        let zoom = matching(0, "zoom");
        assert!(zoom.iter().any(|s| s.2 == "Zoom to fit"));
        assert!(matching(0, "no such thing").is_empty());
    }

    #[test]
    fn sketch_tab_matches_the_tool_keys() {
        use crate::sketch::SketchTool;
        let key = |k: &str| match k {
            "L" => KeyCode::KeyL,
            "G" => KeyCode::KeyG,
            "R" => KeyCode::KeyR,
            "C" => KeyCode::KeyC,
            "A" => KeyCode::KeyA,
            "D" => KeyCode::KeyD,
            "I" => KeyCode::KeyI,
            "O" => KeyCode::KeyO,
            "B" => KeyCode::KeyB,
            "T" => KeyCode::KeyT,
            "H" => KeyCode::KeyH,
            "V" => KeyCode::KeyV,
            "E" => KeyCode::KeyE,
            "M" => KeyCode::KeyM,
            "J" => KeyCode::KeyJ,
            "S" => KeyCode::KeyS,
            "U" => KeyCode::KeyU,
            "F" => KeyCode::KeyF,
            _ => KeyCode::F24,
        };
        for (_, keys, action, _) in matching(4, "") {
            let (shift, k) = match keys.strip_prefix("Shift+") {
                Some(k) => (true, k),
                None => (false, keys),
            };
            if let Some(tool) = SketchTool::from_key(key(k), shift) {
                assert_eq!(tool.label().to_lowercase(), action.to_lowercase(), "{keys}");
            }
        }
    }
}

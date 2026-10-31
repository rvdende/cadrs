//! The keyboard shortcuts dialog (Shift+/ or Settings ▾ › Keyboard shortcuts), laid out like
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
use cadrs_ui::ShortcutKey;
use cadrs_ui::i18n::{Translated, tr};
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
/// Their names' catalog keys ([`tr`]).
pub const TABS: [&str; 6] = [
    "view-shortcuts-tab-general",
    "common-part-studio",
    "common-assembly",
    "view-shortcuts-tab-3d-view",
    "common-sketch",
    "common-drawing",
];

/// Cannot be rebound (shown with a lock, like Onshape's "Onshape controlled").
pub const LOCKED: u8 = 1;
/// Not in cadrs yet (shown struck through, like Onshape's "Disabled").
pub const OFF: u8 = 2;

/// (tab index, keys, action (its catalog key, [`tr`]), flags). Keys: `+` joins a chord, ` / `
/// separates alternatives.
pub const SHORTCUTS: &[(usize, &str, &str, u8)] = &[
    (0, "Shift+Enter", "view-shortcut-accept-repeat", LOCKED),
    (0, "Enter", "view-shortcut-accept", LOCKED),
    (0, "Escape", "common-cancel", LOCKED),
    (0, "Space", "common-clear-selection", LOCKED),
    (0, "Ctrl+= / Ctrl+-", "view-shortcut-ui-scale", 0),
    (0, "Ctrl+C", "common-copy", LOCKED | OFF),
    (0, "Shift+C", "view-shortcut-curve-analysis", OFF),
    (
        0,
        "Delete / Backspace",
        "view-shortcut-delete-selection",
        LOCKED,
    ),
    (0, "Shift+D", "view-shortcut-dihedral-analysis", OFF),
    (0, "Shift+/", "view-shortcuts-title", 0),
    (0, "[", "view-shortcut-measure", 0),
    (0, "Ctrl+V", "common-paste", LOCKED | OFF),
    (0, "Ctrl+Y / Ctrl+Shift+Z", "common-redo", LOCKED),
    (0, "`", "view-shortcut-select-other", OFF),
    (0, "Alt+C", "view-shortcut-search-tools", 0),
    (0, "Alt+click", "view-shortcut-look-here", LOCKED),
    (0, "S", "view-shortcut-shortcut-toolbar", OFF),
    (0, "Alt+T", "view-shortcut-tab-manager", OFF),
    (0, "Ctrl+Space", "view-shortcut-cycle-tabs", OFF),
    (0, "Ctrl+Z", "common-undo", LOCKED),
    (1, "Delete", "view-shortcut-delete-features", LOCKED),
    (1, "Shift+E", "common-extrude", 0),
    (1, "Shift+F", "view-shortcut-fillet", 0),
    (1, "Shift+W", "view-shortcut-revolve", 0),
    (1, "Shift+H", "view-shortcut-toggle-sketches", OFF),
    (1, "Shift+S", "common-sketch", 0),
    (1, "K", "view-shortcut-toggle-mate-connectors", 0),
    (1, "Ctrl+M", "common-mate-connector", 0),
    (2, "M", "view-shortcut-fastened-mate", 0),
    (2, "I", "view-shortcut-insert-parts", 0),
    (2, "Y", "view-shortcut-hide-instance", 0),
    (2, "Shift+Y", "view-shortcut-show-all-instances", 0),
    (2, "Shift+S", "view-shortcut-snap-mode", OFF),
    (2, "Ctrl+M", "common-mate-connector", 0),
    (2, "Shift+N", "view-shortcut-rename-instance", OFF),
    (2, "J", "view-shortcut-toggle-mates", 0),
    (2, "K", "view-shortcut-toggle-mate-connectors", 0),
    (2, "H", "view-shortcut-show-mates-mode", 0),
    (3, "Shift+1", "view-shortcut-front", 0),
    (3, "Shift+2", "view-shortcut-back", 0),
    (3, "Shift+3", "view-shortcut-left", 0),
    (3, "Shift+4", "view-shortcut-right", 0),
    (3, "Shift+5", "view-shortcut-top", 0),
    (3, "Shift+6", "view-shortcut-bottom", 0),
    (3, "Shift+7", "view-shortcut-isometric", 0),
    (3, "Numpad 7", "view-shortcut-top-or-bottom", 0),
    (3, "Numpad 1", "view-shortcut-front-or-back", 0),
    (3, "Numpad 3", "view-shortcut-right-or-left", 0),
    (3, "Numpad 9", "view-shortcut-opposite", 0),
    (3, "Numpad 8 / 2 / 4 / 6", "view-shortcut-rotate-numpad", 0),
    (3, "Numpad 5", "view-shortcut-perspective", 0),
    (3, "N", "view-shortcut-normal-to", 0),
    (3, "F", "common-zoom-to-fit", 0),
    (3, "Alt+F / Numpad .", "common-zoom-to-selection", 0),
    (3, "W / A / S / D", "view-shortcut-walk", 0),
    (3, "Q / E", "view-shortcut-walk-down-up", 0),
    (3, "Escape", "view-shortcut-leave-walk", 0),
    (3, "W", "view-shortcut-zoom-window", OFF),
    (3, "Z", "view-shortcut-zoom-out", 0),
    (3, "Shift+Z", "view-shortcut-zoom-in", 0),
    (3, "← / → / ↑ / ↓", "view-shortcut-rotate-15", 0),
    (3, "Shift+←", "view-shortcut-rotate-90", 0),
    (3, "Ctrl+←", "view-shortcut-rotate-5", 0),
    (3, "Ctrl+Shift+←", "view-shortcut-pan", 0),
    (3, "P", "view-shortcut-toggle-planes", 0),
    (3, "F12", "view-shortcut-toggle-frame-rate", 0),
    (3, "Shift+P", "view-shortcut-hide-construction", OFF),
    (3, "Shift+I", "view-shortcut-isolate", OFF),
    (3, "Shift+T", "view-shortcut-transparent", OFF),
    (3, "Shift+X", "view-shortcut-section-view", OFF),
    (3, "Shift+V", "view-shortcut-named-views", OFF),
    (3, "Shift+R", "view-shortcut-render-hq", OFF),
    (4, "L", "sketch-tool-line", 0),
    (4, "Shift+A", "sketch-shortcut-line-tangent-arc", 0),
    (4, "G", "sketch-tool-corner-rectangle", 0),
    (4, "R", "sketch-tool-center-rectangle", 0),
    (4, "C", "sketch-tool-center-circle", 0),
    (4, "A", "sketch-tool-3-point-arc", 0),
    (4, "Shift+S", "sketch-tool-point", 0),
    (4, "D", "common-dimension", 0),
    (4, "Q", "sketch-construction", 0),
    (4, "Ctrl+A", "sketch-select-all", 0),
    (4, "U", "sketch-tool-use", 0),
    (4, "O", "common-offset", 0),
    (4, "M", "sketch-tool-trim", 0),
    (4, "X", "sketch-tool-extend", 0),
    (4, "Shift+F", "sketch-tool-fillet", 0),
    (4, "I", "sketch-constraint-coincident", 0),
    (4, "Shift+O", "sketch-constraint-concentric", 0),
    (4, "B", "sketch-constraint-parallel", 0),
    (4, "T", "sketch-constraint-tangent", 0),
    (4, "H", "sketch-constraint-horizontal", 0),
    (4, "V", "sketch-constraint-vertical", 0),
    (4, "Shift+L", "sketch-constraint-perpendicular", 0),
    (4, "E", "sketch-constraint-equal", 0),
    (4, "Shift+M", "sketch-constraint-midpoint", 0),
    (4, "Shift+K", "sketch-constraint-normal", 0),
    (4, "Shift+G", "sketch-constraint-pierce", 0),
    (4, "Shift+Q", "sketch-constraint-symmetric", 0),
    (4, "Shift+J", "sketch-constraint-fix", 0),
    (4, "Shift+U", "sketch-constraint-curvature", OFF),
    // Drawings (X12, P3C.1): the keys of tools still to come are struck through.
    (5, "F", "view-shortcut-zoom-sheet", 0),
    (5, "Ctrl+S", "view-shortcut-sheets-flyout", 0),
    (5, "Escape", "view-shortcut-end-tool", LOCKED),
    (5, "D", "common-dimension", 0),
    (5, "Shift+R", "view-shortcut-radial-dimension", 0),
    (5, "Shift+D", "view-shortcut-diameter-dimension", 0),
    (5, "N", "view-shortcut-note", OFF),
    (5, "Ctrl+Q", "view-shortcut-update-workspace", 0),
    (5, "Tab / Shift+Tab", "view-shortcut-next-cell", OFF),
    (4, "Shift", "view-shortcut-no-inferencing", LOCKED),
];

/// The dialog root.
#[derive(Component)]
struct ShortcutsDialog;

/// The container the rows are rebuilt into.
#[derive(Component)]
struct ShortcutRows;

/// The shortcuts matching a search (in any tab), or a tab's shortcuts, with their actions in
/// the current language.
pub fn matching(tab: usize, query: &str) -> Vec<(usize, &'static str, String, u8)> {
    let q = query.trim().to_lowercase();
    SHORTCUTS
        .iter()
        .map(|(t, keys, action, flags)| (*t, *keys, tr(action), *flags))
        .filter(|(t, keys, action, _)| {
            if q.is_empty() {
                *t == tab
            } else {
                action.to_lowercase().contains(&q) || keys.to_lowercase().contains(&q)
            }
        })
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
            .title(tr("view-shortcuts-title"))
            .title_font(20.0, FontWeight::NORMAL)
            .header_height(52.0)
            .width(994.0)
            .header(move |h| {
                let t = &th;
                h.spawn((
                    Name::new("shortcuts-customize"),
                    t.text(
                        tr("view-shortcuts-customize"),
                        12.0,
                        FontWeight::SEMIBOLD,
                        t.link,
                    ),
                    Translated("view-shortcuts-customize"),
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(200.0),
                        top: Val::Px(21.0),
                        ..default()
                    },
                    Tooltip::new(tr("view-shortcuts-customize-tooltip")),
                ));
                h.spawn(
                    TextInput::new("shortcuts-search")
                        .placeholder(tr("view-shortcuts-search"))
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
                        strip = strip.tab(tr(tab));
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
        l.spawn(dialog_keycap(t, tr("view-shortcuts-legend-example"), false));
        l.spawn(legend_text(t, "view-shortcuts-legend-default"));
        l.spawn(dialog_keycap(t, tr("view-shortcuts-legend-example"), true));
        l.spawn(legend_text(t, "view-shortcuts-legend-off"));
        l.spawn(lock(t));
        l.spawn(legend_text(t, "view-shortcuts-legend-locked"));
    });
}

/// A legend's text: the text of `key`.
fn legend_text(t: &Theme, key: &'static str) -> impl Bundle {
    (
        t.text(tr(key), 14.0, FontWeight::NORMAL, t.foreground),
        Translated(key),
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
    q_fields: Query<(), With<cadrs_ui::KeyboardField>>,
    q_dialogs: Query<(), With<DialogRoot>>,
    mut commands: Commands,
) {
    let typing = focus.get().is_some_and(|e| q_fields.contains(e));
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for k in keys_in.read() {
        if k.state == ButtonState::Pressed
            && k.shortcut_code() == KeyCode::Slash
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
                t.text(
                    tr("view-shortcuts-none-found"),
                    t.font_base,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
                Translated("view-shortcuts-none-found"),
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
                    t.text(tr(TABS[tab]), 14.0, FontWeight::SEMIBOLD, t.foreground),
                    Translated(TABS[tab]),
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

    /// Issue #32: handlers match `shortcut_code()`, which follows the keyboard layout, never the
    /// physical `key_code` (on AZERTY the key labelled Z is `KeyW`).
    #[test]
    fn handlers_match_layout_aware_keys() {
        // Spelled in two parts so this test does not find itself.
        const NEEDLE: &str = concat!(".key", "_code");
        fn scan(dir: &std::path::Path, hits: &mut Vec<String>) {
            for entry in cadrs_fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    scan(&path, hits);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && !path.ends_with("shortcut_key.rs")
                {
                    let text = cadrs_fs::read_to_string(&path).unwrap();
                    for (i, line) in text.lines().enumerate() {
                        if line.contains(NEEDLE) {
                            hits.push(format!("{}:{}", path.display(), i + 1));
                        }
                    }
                }
            }
        }
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let mut hits = Vec::new();
        scan(&root.join("src"), &mut hits);
        scan(&root.join("../cadrs_ui/src"), &mut hits);
        assert!(
            hits.is_empty(),
            "use ShortcutKey::shortcut_code() instead of {NEEDLE}: {hits:#?}"
        );
    }

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

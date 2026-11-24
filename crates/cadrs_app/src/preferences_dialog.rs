//! The Preferences dialog (the app menu's **Preferences…**): the sections in a column on the left
//! (Interface; Input › Navigation; Design › Assembly; Connect › Collaboration, AI assistants), the
//! chosen section's settings on the right under folding headings, a search in the title bar
//! that lists the settings matching it from every section, and a footer with **Restore
//! Defaults** (the section's) on the left and **Cancel**, **Apply** and **OK** on the right.
//!
//! The dialog edits a draft ([`PrefsDialog`]). What can be seen at once (the color scheme, the UI
//! scale, the grid, the origin, the frame stats, vertical sync, the mouse) is previewed as it is
//! chosen; the rest (the language, the MCP server, public documents, the seed peers) waits for
//! Apply or OK. Apply keeps the draft (saved in `preferences.ron`) and leaves the dialog open;
//! OK keeps it and closes; Cancel, ✕ and Escape put back what was kept when the dialog opened,
//! or at the last Apply.

use std::collections::HashSet;

use bevy::prelude::*;
use bevy::text::{EditableText, FontWeight};
use bevy::ui::InteractionDisabled;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::preferences::{
    ColorScheme, FrameStatsMode, GridStyle, Language, MousePreset, OriginStyle, Preferences,
    UI_SCALES, ViewAction,
};
use cadrs_ui::i18n::{tr, tr_with};
use cadrs_ui::{
    Button, Collapsible, CollapsibleToggled, Dialog, DialogClose, NumberField, NumberFieldCommit,
    NumberFieldEdit, NumberFieldState, Segmented, SegmentedSelect, Select, SelectChange,
    SelectState, SelectableText, SideNav, SideNavSelect, Switch, SwitchChange, TextInput, Theme,
    form_row,
};

use crate::preferences_ui::{LocalPreferences, from_ui, save, scheme_override, to_ui};
// The MCP server's port and endpoint (the AI assistants section). A page serves none and has
// no `mcp_button` (nor `cadrs_mcp`): its section isn't offered, so it never asks.
#[cfg(not(target_arch = "wasm32"))]
use crate::mcp_button::{endpoint as mcp_endpoint, parse_port as parse_mcp_port};
#[cfg(target_arch = "wasm32")]
fn parse_mcp_port(_: &str) -> Option<u16> {
    None
}
#[cfg(target_arch = "wasm32")]
fn mcp_endpoint(_: u16) -> String {
    String::new()
}
use crate::{AppState, DocumentStore};

pub(crate) fn plugin(app: &mut App) {
    app.add_observer(on_select)
        .add_observer(on_segment)
        .add_observer(on_switch)
        .add_observer(on_group_toggled)
        .add_observer(on_close)
        .add_observer(on_snap_commit)
        .add_systems(Update, follow_texts);
}

/// The width of the labels' column.
const LABEL_W: f32 = 170.0;
/// A note under a row: as wide as the page's rows.
const NOTE_W: f32 = 470.0;
/// The width of a select or a number.
const CONTROL_W: f32 = 240.0;

/// A section of the dialog, in the order the column lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Interface,
    Navigation,
    Assembly,
    Collaboration,
    Assistants,
}

impl Section {
    const ALL: [Section; 5] = [
        Section::Interface,
        Section::Navigation,
        Section::Assembly,
        Section::Collaboration,
        Section::Assistants,
    ];

    /// Its row's name: `pref-section-<slug>`.
    pub fn slug(self) -> &'static str {
        match self {
            Section::Interface => "interface",
            Section::Navigation => "navigation",
            Section::Assembly => "assembly",
            Section::Collaboration => "collaboration",
            Section::Assistants => "assistants",
        }
    }

    fn title(self) -> String {
        tr(match self {
            Section::Interface => "preferences-interface",
            Section::Navigation => "preferences-navigation",
            Section::Assembly => "preferences-assembly",
            Section::Collaboration => "shell-prefs-collab-heading",
            Section::Assistants => "shell-prefs-mcp-heading",
        })
    }

    fn about(self) -> String {
        tr(match self {
            Section::Interface => "preferences-interface-about",
            Section::Navigation => "preferences-navigation-about",
            Section::Assembly => "preferences-assembly-about",
            Section::Collaboration => "preferences-collab-about",
            Section::Assistants => "preferences-assistants-about",
        })
    }

    fn icon(self) -> &'static str {
        match self {
            Section::Interface => "interface",
            Section::Navigation => "mouse",
            Section::Assembly => "assembly",
            Section::Collaboration => "network",
            Section::Assistants => "assistant",
        }
    }

    /// The heading above it in the column, where a group starts.
    fn group(self) -> Option<&'static str> {
        match self {
            Section::Navigation => Some("preferences-group-input"),
            Section::Assembly => Some("preferences-group-design"),
            Section::Collaboration => Some("preferences-group-connect"),
            _ => None,
        }
    }

    /// A page can't serve an MCP server.
    fn offered(self) -> bool {
        self != Section::Assistants || cfg!(not(target_arch = "wasm32"))
    }
}

/// The dialog's root: the draft and what Cancel goes back to.
#[derive(Component, Debug, Clone)]
pub struct PrefsDialog {
    pub section: Section,
    pub draft: Preferences,
    /// The preferences when the dialog opened, or at the last Apply.
    kept: Preferences,
    /// The fields' texts: seed peers, the MCP port, the rotation snap and its tolerance.
    seeds: String,
    kept_seeds: String,
    port: String,
    snap: String,
    tolerance: String,
    /// What the search field holds.
    query: String,
    /// The headings folded (by their catalog key).
    folded: HashSet<String>,
}

impl PrefsDialog {
    fn new(p: Preferences, seeds: String) -> Self {
        Self {
            section: Section::Interface,
            draft: p,
            kept: p,
            kept_seeds: seeds.clone(),
            seeds,
            port: p.mcp.port.to_string(),
            snap: degrees(p.rotation_snap_degrees()),
            tolerance: degrees(p.rotation_snap_tolerance_degrees()),
            query: String::new(),
            folded: HashSet::new(),
        }
    }

    /// Whether there is anything for Apply to keep.
    fn dirty(&self) -> bool {
        self.draft != self.kept
            || self.seeds != self.kept_seeds
            || self.port != self.kept.mcp.port.to_string()
            || parse_rotation_snap(&self.snap) != Some(self.kept.rotation_snap_degrees())
            || parse_rotation_snap(&self.tolerance)
                != Some(self.kept.rotation_snap_tolerance_degrees())
    }

    /// Restore Defaults: the section's settings as they are for a new user.
    fn restore_section(&mut self) {
        let d = Preferences::default();
        let p = &mut self.draft;
        match self.section {
            Section::Interface => {
                p.color_scheme = d.color_scheme;
                p.ui_scale = d.ui_scale;
                p.language = d.language;
                p.grid = d.grid;
                p.origin = d.origin;
                p.frame_stats = d.frame_stats;
                p.vsync = d.vsync;
                p.profile_cpu = d.profile_cpu;
            }
            Section::Navigation => {
                p.mouse = d.mouse;
                p.keep_camera_up = d.keep_camera_up;
                p.sketch_view_normal = d.sketch_view_normal;
            }
            Section::Assembly => {
                self.snap = degrees(d.rotation_snap_degrees);
                self.tolerance = degrees(d.rotation_snap_tolerance_degrees);
            }
            Section::Collaboration => {
                p.autoconnect_p2p = d.autoconnect_p2p;
                p.public_documents = d.public_documents;
                self.seeds.clear();
            }
            Section::Assistants => {
                p.mcp = d.mcp;
                self.port = d.mcp.port.to_string();
            }
        }
    }
}

fn degrees(v: f64) -> String {
    format!("{v} deg")
}

/// The search field.
#[derive(Component)]
struct PrefsSearch;

/// The column of sections, made again as the section changes.
#[derive(Component)]
struct PrefsSidebar;

/// The settings of the section (or the search's), made again as they change.
#[derive(Component)]
struct PrefsPage;

/// A gesture row's text (its action).
#[derive(Component)]
struct GestureText(ViewAction);

/// The view actions: the row's name, its label's catalog key.
const ACTIONS: [(ViewAction, &str, &str); 4] = [
    (
        ViewAction::Rotate,
        "pref-mouse-rotate",
        "shell-prefs-rotate",
    ),
    (
        ViewAction::RotateTurntable,
        "pref-mouse-turntable",
        "shell-prefs-rotate-turntable",
    ),
    (ViewAction::Pan, "pref-mouse-pan", "shell-prefs-pan"),
    (ViewAction::Zoom, "pref-mouse-zoom", "shell-prefs-zoom"),
];

pub(crate) fn gesture_text(preset: MousePreset, action: ViewAction) -> String {
    let g = preset.gestures(action);
    if g.is_empty() {
        "—".into()
    } else {
        g.join(", ")
    }
}

// ---------------------------------------------------------------------------------------------
// Opening

/// Opens the Preferences dialog with the preferences in force (once: a second asks nothing).
pub fn open_preferences(world: &mut World) {
    open_preferences_at(world, Section::Interface);
}

/// Opens the Preferences dialog on a section (the AI assistants button's Preferences…).
pub fn open_preferences_at(world: &mut World, section: Section) {
    if world
        .query_filtered::<(), With<PrefsDialog>>()
        .iter(world)
        .next()
        .is_some()
    {
        return;
    }
    let prefs = world.resource::<LocalPreferences>().0;
    let seeds = world
        .get_resource::<crate::collab::public::Seeds>()
        .map(|s| s.0.join(", "))
        .unwrap_or_default();
    let state = *world.resource::<State<AppState>>().get();
    let theme = world.resource::<Theme>().clone();
    let (ts, tb, tf) = (theme.clone(), theme.clone(), theme.clone());
    world.commands().spawn((
        Dialog::new("preferences-dialog")
            .title(tr("preferences"))
            .width(800.0)
            .height(620.0)
            .header(move |h| {
                h.spawn((
                    TextInput::new("pref-search")
                        .placeholder(tr("preferences-search"))
                        .leading_icon("search")
                        .cleanable()
                        .width(Val::Px(240.0))
                        .build(&ts),
                    PrefsSearch,
                ))
                .entry::<Node>()
                .and_modify(|mut n| n.margin = UiRect::right(Val::Px(ts.space[4])));
            })
            .sidebar(200.0, move |s| {
                s.spawn((
                    Name::new("pref-sections"),
                    PrefsSidebar,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    },
                ));
            })
            .body(move |b| {
                b.spawn((
                    Name::new("pref-page"),
                    PrefsPage,
                    Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Percent(100.0),
                        // Clear of the scrollbar.
                        padding: UiRect::right(Val::Px(12.0)),
                        ..default()
                    },
                ));
            })
            .footer(move |f| footer(f, &tf))
            .build(&tb),
        PrefsDialog {
            section,
            ..PrefsDialog::new(prefs, seeds)
        },
        DespawnOnExit(state),
    ));
    world.flush();
    rebuild(world);
}

fn footer(f: &mut ChildSpawner, t: &Theme) {
    f.spawn((
        Button::new("pref-restore-defaults")
            .label(tr("preferences-restore-defaults"))
            .tooltip(tr("preferences-restore-defaults-tooltip"))
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| {
                edit(w, PrefsDialog::restore_section);
                rebuild(w);
            });
        }),
    ));
    f.spawn(Node {
        flex_grow: 1.0,
        ..default()
    });
    f.spawn((
        Button::new("pref-cancel")
            .label(tr("common-cancel"))
            .build(t),
        observe(
            |_: On<Activate>, q: Query<Entity, With<PrefsDialog>>, mut commands: Commands| {
                for e in &q {
                    commands.trigger(DialogClose { entity: e });
                }
            },
        ),
    ));
    f.spawn((
        Button::new("pref-apply")
            .label(tr("common-apply"))
            .disabled(true)
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(|w: &mut World| {
                apply(w);
            });
        }),
    ));
    f.spawn((
        Button::new("pref-ok")
            .label(tr("common-ok"))
            .primary()
            .build(t),
        observe(|_: On<Activate>, mut commands: Commands| commands.queue(ok)),
    ));
}

// ---------------------------------------------------------------------------------------------
// The pages

/// A row of a page.
struct Row {
    /// The control's name (the row is `<name>-row`).
    name: &'static str,
    label: String,
    /// More words the search finds it by.
    keywords: &'static str,
    control: Control,
    /// A note under it.
    note: Option<String>,
}

type SpawnBox = Box<dyn FnOnce(&mut ChildSpawner, &Theme) + Send + Sync>;

enum Control {
    /// Options as (label, key); the chosen one.
    Select(Vec<(String, Option<&'static str>)>, usize),
    Segmented(Vec<String>, usize),
    Switch(String, bool),
    /// A number field with its label (it lays out its own row).
    Number(String),
    /// Anything else, after the label.
    Custom(SpawnBox),
}

impl Row {
    fn new(name: &'static str, label: String, keywords: &'static str, control: Control) -> Self {
        Self {
            name,
            label,
            keywords,
            control,
            note: None,
        }
    }

    fn note(mut self, key: &str) -> Self {
        self.note = Some(tr(key));
        self
    }

    fn matches(&self, query: &str) -> bool {
        let q = query.to_lowercase();
        self.label.to_lowercase().contains(&q)
            || self.keywords.contains(&q)
            || self
                .note
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&q))
    }
}

/// A heading of a page and its rows; the heading's catalog key names it.
struct Group {
    key: &'static str,
    rows: Vec<Row>,
}

/// What the pages show that isn't a preference.
struct Facts {
    cadrs_id: Option<String>,
}

fn options<T: Copy + PartialEq>(all: &[T], label: impl Fn(T) -> String, now: T) -> Control {
    Control::Select(
        all.iter().map(|v| (label(*v), None)).collect(),
        all.iter().position(|v| *v == now).unwrap_or(0),
    )
}

fn groups(section: Section, s: &PrefsDialog, facts: &Facts) -> Vec<Group> {
    let p = &s.draft;
    match section {
        Section::Interface => {
            let scheme = [ColorScheme::System, ColorScheme::Light, ColorScheme::Dark];
            let scales = Control::Select(
                UI_SCALES
                    .iter()
                    .map(|&v| {
                        let label = if v == 100 {
                            tr("shell-prefs-ui-scale-default")
                        } else {
                            format!("{v}%")
                        };
                        (label, None)
                    })
                    .collect(),
                // A size between the presets (an older cadrs's) shows as the nearest below.
                UI_SCALES
                    .iter()
                    .rposition(|&v| v <= p.ui_scale)
                    .unwrap_or(0),
            );
            // Issue #178: each language by its own name, known by its code
            // (`pref-language-option-fr`). One not offered shows as English, as it reads.
            let offered: Vec<_> = cadrs_ui::i18n::offered().collect();
            let lang_at = offered
                .iter()
                .position(|(c, _)| *c == p.language.code())
                .or_else(|| offered.iter().position(|(c, _)| *c == "en"))
                .unwrap_or(0);
            let languages = Control::Select(
                offered
                    .iter()
                    .map(|(code, name)| (name.to_string(), Some(*code)))
                    .collect(),
                lang_at,
            );
            let mut display = vec![Row::new(
                "pref-frame-stats",
                tr("preferences-frame-stats"),
                "fps frame rate panel performance f12",
                options(FrameStatsMode::ALL, frame_stats_label, p.frame_stats),
            )];
            // A page presents as the browser does, and has no system times to show.
            if cfg!(not(target_arch = "wasm32")) {
                display.push(
                    Row::new(
                        "pref-vsync",
                        tr("preferences-display"),
                        "vsync vertical sync refresh",
                        Control::Switch(tr("shell-prefs-vsync"), p.vsync),
                    )
                    .note("shell-prefs-vsync-note"),
                );
                display.push(
                    Row::new(
                        "pref-profile-cpu",
                        tr("preferences-profiling"),
                        "time systems cpu profile",
                        Control::Switch(tr("shell-prefs-time-systems"), p.profile_cpu),
                    )
                    .note("shell-prefs-time-systems-note"),
                );
            }
            vec![
                Group {
                    key: "preferences-group-appearance",
                    rows: vec![
                        Row::new(
                            "pref-color-scheme",
                            tr("preferences-color-scheme"),
                            "theme dark light mode",
                            Control::Segmented(
                                scheme.iter().map(|c| scheme_label(*c)).collect(),
                                scheme
                                    .iter()
                                    .position(|c| *c == p.color_scheme)
                                    .unwrap_or(0),
                            ),
                        ),
                        Row::new(
                            "pref-ui-scale",
                            tr("preferences-ui-scale"),
                            "size zoom",
                            scales,
                        ),
                        Row::new(
                            "pref-language",
                            tr("preferences-language"),
                            "language",
                            languages,
                        )
                        .note("preferences-language-note"),
                    ],
                },
                Group {
                    key: "preferences-group-grid",
                    rows: vec![
                        Row::new(
                            "pref-grid",
                            tr("preferences-grid"),
                            "grid lines crossed ground",
                            options(GridStyle::ALL, grid_label, p.grid),
                        ),
                        Row::new(
                            "pref-origin",
                            tr("preferences-origin"),
                            "origin axes dot triad",
                            options(OriginStyle::ALL, origin_label, p.origin),
                        ),
                    ],
                },
                Group {
                    key: "preferences-group-display",
                    rows: display,
                },
            ]
        }
        Section::Navigation => {
            let preset = p.mouse;
            let mut mouse = vec![Row::new(
                "pref-mouse-preset",
                tr("preferences-mouse-controls"),
                "mouse rotate pan zoom onshape solidworks inventor creo",
                options(MousePreset::ALL.as_slice(), |m| m.label().into(), preset),
            )];
            for (action, name, label) in ACTIONS {
                mouse.push(Row::new(
                    name,
                    tr(label),
                    "mouse gesture",
                    Control::Custom(Box::new(move |r, t| {
                        r.spawn((
                            Name::new(name),
                            GestureText(action),
                            t.text(
                                gesture_text(preset, action),
                                t.font_base,
                                FontWeight::NORMAL,
                                t.muted_foreground,
                            ),
                        ));
                    })),
                ));
            }
            vec![
                Group {
                    key: "preferences-mouse-heading",
                    rows: mouse,
                },
                Group {
                    key: "preferences-group-view",
                    rows: vec![
                        Row::new(
                            "pref-keep-camera-up",
                            tr("preferences-camera"),
                            "keep camera up roll turntable",
                            Control::Switch(tr("view-menu-keep-up"), p.keep_camera_up),
                        )
                        .note("preferences-keep-up-note"),
                        Row::new(
                            "pref-sketch-view-normal",
                            tr("preferences-sketch-view"),
                            "sketch view normal to plane auto rotate align",
                            Control::Switch(
                                tr("preferences-sketch-view-normal"),
                                p.sketch_view_normal,
                            ),
                        )
                        .note("preferences-sketch-view-normal-note"),
                    ],
                },
            ]
        }
        Section::Assembly => vec![Group {
            key: "shell-prefs-assembly-heading",
            rows: vec![
                Row::new(
                    "pref-rotation-snap",
                    tr("shell-prefs-rotation-snap"),
                    "rotation snap increment angle",
                    Control::Number(s.snap.clone()),
                ),
                Row::new(
                    "pref-rotation-snap-tolerance",
                    tr("shell-prefs-snap-tolerance"),
                    "rotation snap tolerance angle",
                    Control::Number(s.tolerance.clone()),
                )
                .note("shell-prefs-snap-hint"),
            ],
        }],
        Section::Collaboration => {
            let id = facts.cadrs_id.clone();
            let seeds = s.seeds.clone();
            vec![
                Group {
                    key: "preferences-group-network",
                    rows: vec![
                        Row::new(
                            "pref-cadrs-id",
                            tr("shell-share-cadrs-id"),
                            "id identity peer",
                            Control::Custom(Box::new(move |r, t| match id {
                                Some(id) => {
                                    r.spawn(
                                        SelectableText::new("pref-cadrs-id")
                                            .value(id)
                                            .font(t.font_sm, FontWeight::NORMAL)
                                            .copy_button()
                                            .build(t),
                                    );
                                }
                                None => {
                                    r.spawn((
                                        Name::new("pref-cadrs-id"),
                                        t.text(
                                            tr("shell-prefs-id-while-online"),
                                            t.font_base,
                                            FontWeight::NORMAL,
                                            t.muted_foreground,
                                        ),
                                    ));
                                }
                            })),
                        ),
                        Row::new(
                            "pref-p2p-autoconnect",
                            tr("shell-prefs-p2p"),
                            "p2p network online connect start",
                            Control::Switch(tr("shell-network-autoconnect"), p.autoconnect_p2p),
                        )
                        .note("shell-prefs-collab-note"),
                    ],
                },
                Group {
                    key: "shell-prefs-public",
                    rows: vec![
                        Row::new(
                            "pref-public-documents",
                            tr("shell-prefs-public"),
                            "public documents publish",
                            Control::Switch(tr("shell-prefs-public-switch"), p.public_documents),
                        ),
                        Row::new(
                            "pref-public-seeds",
                            tr("shell-prefs-public-seeds"),
                            "seed peers public",
                            Control::Custom(Box::new(move |r, t| {
                                r.spawn(
                                    TextInput::new("pref-public-seeds")
                                        .placeholder(tr("shell-prefs-public-seeds-placeholder"))
                                        .value(seeds)
                                        .width(Val::Px(300.0))
                                        .build(t),
                                );
                            })),
                        ),
                    ],
                },
            ]
        }
        Section::Assistants => {
            let port = s.port.clone();
            let endpoint = parse_mcp_port(&port).map(mcp_endpoint).unwrap_or_default();
            vec![Group {
                key: "shell-prefs-mcp-server",
                rows: vec![
                    Row::new(
                        "pref-mcp-enabled",
                        tr("shell-prefs-mcp-server"),
                        "mcp ai assistant claude cursor server",
                        Control::Switch(tr("shell-prefs-enabled"), p.mcp.enabled),
                    ),
                    Row::new(
                        "pref-mcp-endpoint",
                        tr("shell-prefs-endpoint"),
                        "mcp endpoint url",
                        Control::Custom(Box::new(move |r, t| {
                            r.spawn((
                                Name::new("pref-mcp-endpoint"),
                                t.text(
                                    endpoint,
                                    t.font_base,
                                    FontWeight::NORMAL,
                                    t.muted_foreground,
                                ),
                            ));
                        })),
                    ),
                    Row::new(
                        "pref-mcp-port",
                        tr("shell-prefs-mcp-port"),
                        "mcp port",
                        Control::Custom(Box::new(move |r, t| mcp_port(r, t, port))),
                    )
                    .note("shell-prefs-mcp-note"),
                ],
            }]
        }
    }
}

/// The MCP port, and Reset to put the default back in the field.
fn mcp_port(r: &mut ChildSpawner, t: &Theme, port: String) {
    let default = cadrs_core::preferences::DEFAULT_MCP_PORT.to_string();
    r.spawn(
        TextInput::new("pref-mcp-port")
            .value(port)
            .width(Val::Px(90.0))
            .build(t),
    );
    r.spawn((
        Button::new("pref-mcp-port-reset")
            .label(tr("shell-prefs-mcp-port-reset"))
            .ghost()
            .small()
            .tooltip(tr_with(
                "shell-prefs-mcp-port-reset-tooltip",
                &[("port", &default)],
            ))
            .build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            let default = default.clone();
            commands.queue(move |w: &mut World| {
                crate::pcb::dialogs::set_text(w, "pref-mcp-port-field", &default);
            });
        }),
    ));
}

fn scheme_label(c: ColorScheme) -> String {
    tr(match c {
        ColorScheme::System => "preferences-scheme-system",
        ColorScheme::Light => "ui-color-scheme-light",
        ColorScheme::Dark => "ui-color-scheme-dark",
    })
}

fn grid_label(g: GridStyle) -> String {
    tr(match g {
        GridStyle::Off => "preferences-grid-off",
        GridStyle::Crossed => "preferences-grid-crossed",
        GridStyle::Lines => "preferences-grid-lines",
    })
}

fn origin_label(o: OriginStyle) -> String {
    tr(match o {
        OriginStyle::Hidden => "preferences-origin-hidden",
        OriginStyle::Dot => "preferences-origin-dot",
        OriginStyle::Axes => "preferences-origin-axes",
    })
}

fn frame_stats_label(m: FrameStatsMode) -> String {
    tr(match m {
        FrameStatsMode::Off => "preferences-frame-stats-off",
        FrameStatsMode::Fps => "preferences-frame-stats-fps",
        FrameStatsMode::Details => "preferences-frame-stats-details",
    })
}

/// Makes the column and the page again from the draft: the section's settings, or with a
/// search, the settings that match it from every section.
fn rebuild(world: &mut World) {
    let mut q = world.query::<(Entity, &PrefsDialog)>();
    let Some((dialog, s)) = q.iter(world).next().map(|(e, s)| (e, s.clone())) else {
        return;
    };
    let theme = world.resource::<Theme>().clone();
    let facts = Facts {
        cadrs_id: cadrs_net::cadrs_id(world.get_resource::<cadrs_net::bevy_iroh::Iroh>()),
    };
    let sidebar = world
        .query_filtered::<Entity, With<PrefsSidebar>>()
        .iter(world)
        .next();
    let page = world
        .query_filtered::<Entity, With<PrefsPage>>()
        .iter(world)
        .next();
    let query = s.query.trim().to_string();
    if let Some(sidebar) = sidebar {
        world.entity_mut(sidebar).despawn_children();
        let offered: Vec<Section> = Section::ALL.into_iter().filter(|s| s.offered()).collect();
        let mut nav = SideNav::new("pref-section").selected(
            // None while a search's results are up.
            offered
                .iter()
                .position(|section| *section == s.section)
                .filter(|_| query.is_empty()),
        );
        for section in &offered {
            if let Some(group) = section.group() {
                nav = nav.group(tr(group));
            }
            nav = nav.item(section.slug(), section.icon(), section.title());
        }
        world.commands().entity(sidebar).with_child((
            nav.build(&theme),
            observe(|ev: On<SideNavSelect>, mut commands: Commands| {
                let Some(section) = Section::ALL.into_iter().find(|s| s.slug() == ev.key) else {
                    return;
                };
                commands.queue(move |w: &mut World| show_section(w, section));
            }),
        ));
    }
    let Some(page) = page else {
        world.flush();
        return;
    };
    world.entity_mut(page).despawn_children();
    // The page's headings: the section's, or each matching section's under its name.
    let mut shown: Vec<(String, &'static str, Vec<Row>)> = Vec::new();
    if query.is_empty() {
        for g in groups(s.section, &s, &facts) {
            shown.push((tr(g.key), g.key, g.rows));
        }
    } else {
        for section in Section::ALL.into_iter().filter(|s| s.offered()) {
            for g in groups(section, &s, &facts) {
                let rows: Vec<Row> = g.rows.into_iter().filter(|r| r.matches(&query)).collect();
                if !rows.is_empty() {
                    shown.push((format!("{} › {}", section.title(), tr(g.key)), g.key, rows));
                }
            }
        }
    }
    let t = theme.clone();
    let folded = s.folded.clone();
    let mut commands = world.commands();
    commands.entity(page).with_children(|p| {
        let (title, about) = if query.is_empty() {
            (s.section.title(), s.section.about())
        } else {
            (
                tr("preferences-search-results"),
                tr_with("preferences-search-about", &[("text", &query)]),
            )
        };
        p.spawn((
            Name::new("pref-page-title"),
            t.text(title, t.font_xl, FontWeight::SEMIBOLD, t.foreground),
        ));
        p.spawn((
            Name::new("pref-page-about"),
            t.text(about, t.font_base, FontWeight::NORMAL, t.muted_foreground),
            Node {
                margin: UiRect::new(Val::ZERO, Val::ZERO, Val::Px(2.0), Val::Px(14.0)),
                ..default()
            },
        ));
        if shown.is_empty() {
            p.spawn((
                Name::new("pref-no-match"),
                t.text(
                    tr("preferences-no-match"),
                    t.font_base,
                    FontWeight::NORMAL,
                    t.muted_foreground,
                ),
            ));
        }
        for (heading, key, rows) in shown {
            let tc = t.clone();
            p.spawn(
                Collapsible::new(format!("pref-group-{key}"), heading)
                    .heading()
                    .open(!folded.contains(key))
                    .content(move |c| {
                        for row in rows {
                            spawn_row(c, &tc, row);
                        }
                    })
                    .build(&t),
            );
        }
    });
    world.flush();
    sync_apply(world, dialog);
}

fn spawn_row(c: &mut ChildSpawner, t: &Theme, row: Row) {
    let Row {
        name,
        label,
        control,
        note,
        ..
    } = row;
    match control {
        Control::Number(text) => {
            // As wide as a select beside its label.
            c.spawn(Node {
                width: Val::Px(LABEL_W + 12.0 + CONTROL_W),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_child(
                NumberField::new(name, label)
                    .text(text)
                    .label_width(LABEL_W + 12.0)
                    .label_size(t.font_base)
                    .build(t),
            );
        }
        control => {
            let tc = t.clone();
            c.spawn(form_row(t, format!("{name}-row"), label, LABEL_W))
                .with_children(move |r| match control {
                    Control::Select(options, at) => {
                        let mut select = Select::new(name);
                        for (label, key) in options {
                            select = match key {
                                Some(k) => select.keyed_option(k, label, true),
                                None => select.option(label, true),
                            };
                        }
                        // (A select grows to fill its row.)
                        r.spawn(Node {
                            width: Val::Px(CONTROL_W),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_child(select.selected(at).build(&tc));
                    }
                    Control::Segmented(options, at) => {
                        let mut seg = Segmented::new(name).size(24.0, 12.0);
                        for o in options {
                            seg = seg.option(o, None, "");
                        }
                        r.spawn(seg.selected(at).build(&tc));
                    }
                    Control::Switch(label, on) => {
                        r.spawn(Switch::new(name).label(label).on(on).build(&tc));
                    }
                    Control::Custom(f) => f(r, &tc),
                    Control::Number(_) => {}
                });
        }
    }
    if let Some(note) = note {
        c.spawn((
            Name::new(format!("{name}-note")),
            t.text(note, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
            Node {
                max_width: Val::Px(NOTE_W),
                margin: UiRect::new(Val::Px(LABEL_W + 12.0), Val::ZERO, Val::ZERO, Val::Px(6.0)),
                ..default()
            },
        ))
        // Wrapped (the theme's text doesn't wrap).
        .insert(TextLayout::default());
    }
}

// ---------------------------------------------------------------------------------------------
// Editing

/// Changes the draft, previews what can be seen at once, and enables Apply when there is
/// something for it to keep.
fn edit(world: &mut World, f: impl FnOnce(&mut PrefsDialog)) {
    let mut q = world.query::<(Entity, &mut PrefsDialog)>();
    let Some((dialog, mut s)) = q.iter_mut(world).next() else {
        return;
    };
    f(&mut s);
    let draft = s.draft;
    let kept = s.kept;
    preview(world, &draft, &kept);
    sync_apply(world, dialog);
}

/// The settings that are seen at once (the preview), as in `draft`; the others as in `kept`.
fn previewed(base: Preferences, draft: &Preferences, kept: &Preferences) -> Preferences {
    Preferences {
        color_scheme: draft.color_scheme,
        ui_scale: draft.ui_scale,
        grid: draft.grid,
        origin: draft.origin,
        frame_stats: draft.frame_stats,
        vsync: draft.vsync,
        mouse: draft.mouse,
        keep_camera_up: draft.keep_camera_up,
        sketch_view_normal: kept.sketch_view_normal,
        language: kept.language,
        profile_cpu: kept.profile_cpu,
        autoconnect_p2p: kept.autoconnect_p2p,
        public_documents: kept.public_documents,
        mcp: kept.mcp,
        rotation_snap_degrees: kept.rotation_snap_degrees,
        rotation_snap_tolerance_degrees: kept.rotation_snap_tolerance_degrees,
        // Not the dialog's: what's new, the last slicer.
        ..base
    }
}

/// Every setting the dialog has, as in `p`.
fn kept_all(base: Preferences, p: &Preferences) -> Preferences {
    previewed(base, p, p)
}

fn preview(world: &mut World, draft: &Preferences, kept: &Preferences) {
    let now = world.resource::<LocalPreferences>().0;
    let want = previewed(now, draft, kept);
    set_in_force(world, now, want);
}

/// Puts `want` in force (and the color scheme with it), unless it already is.
fn set_in_force(world: &mut World, now: Preferences, want: Preferences) {
    if want != now {
        world.resource_mut::<LocalPreferences>().0 = want;
    }
    // The scheme the window is drawn in follows (`save_color_scheme` then sees nothing new).
    if scheme_override().is_none()
        && let Some(mut a) = world.get_resource_mut::<cadrs_ui::Appearance>()
        && from_ui(a.preference) != want.color_scheme
    {
        a.preference = to_ui(want.color_scheme);
    }
}

fn sync_apply(world: &mut World, dialog: Entity) {
    let dirty = world.get::<PrefsDialog>(dialog).is_some_and(|s| s.dirty());
    let mut q = world.query::<(Entity, &Name)>();
    let Some(apply) = q
        .iter(world)
        .find(|(_, n)| n.as_str() == "pref-apply")
        .map(|(e, _)| e)
    else {
        return;
    };
    let disabled = world.get::<InteractionDisabled>(apply).is_some();
    if dirty && disabled {
        world.entity_mut(apply).remove::<InteractionDisabled>();
    } else if !dirty && !disabled {
        world.entity_mut(apply).insert(InteractionDisabled);
    }
}

/// A select of the dialog was changed.
fn on_select(
    ev: On<SelectChange>,
    q: Query<(&Name, &SelectState)>,
    mut q_text: Query<(&GestureText, &mut Text)>,
    mut commands: Commands,
) {
    let Ok((name, state)) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    let key = state.keys.get(i).cloned().flatten();
    let change: Box<dyn FnOnce(&mut PrefsDialog) + Send + Sync> = match name.as_str() {
        "pref-ui-scale" => match UI_SCALES.get(i) {
            Some(&v) => Box::new(move |s| s.draft.ui_scale = v),
            None => return,
        },
        "pref-language" => match key.as_deref().and_then(Language::from_code) {
            Some(l) => Box::new(move |s| s.draft.language = l),
            None => return,
        },
        "pref-grid" => match GridStyle::ALL.get(i) {
            Some(&g) => Box::new(move |s| s.draft.grid = g),
            None => return,
        },
        "pref-origin" => match OriginStyle::ALL.get(i) {
            Some(&o) => Box::new(move |s| s.draft.origin = o),
            None => return,
        },
        "pref-frame-stats" => match FrameStatsMode::ALL.get(i) {
            Some(&m) => Box::new(move |s| s.draft.frame_stats = m),
            None => return,
        },
        "pref-mouse-preset" => match MousePreset::ALL.get(i) {
            Some(&m) => {
                // Its gestures follow.
                for (g, mut t) in &mut q_text {
                    t.0 = gesture_text(m, g.0);
                }
                Box::new(move |s| s.draft.mouse = m)
            }
            None => return,
        },
        _ => return,
    };
    commands.queue(move |w: &mut World| edit(w, change));
}

fn on_segment(ev: On<SegmentedSelect>, q: Query<&Name>, mut commands: Commands) {
    if q.get(ev.entity).map(Name::as_str) != Ok("pref-color-scheme") {
        return;
    }
    let scheme = match ev.index {
        1 => ColorScheme::Light,
        2 => ColorScheme::Dark,
        _ => ColorScheme::System,
    };
    commands.queue(move |w: &mut World| edit(w, |s| s.draft.color_scheme = scheme));
}

fn on_switch(ev: On<SwitchChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let on = ev.on;
    let change: fn(&mut PrefsDialog, bool) = match name.as_str() {
        "pref-vsync" => |s, on| s.draft.vsync = on,
        "pref-profile-cpu" => |s, on| s.draft.profile_cpu = on,
        "pref-keep-camera-up" => |s, on| s.draft.keep_camera_up = on,
        "pref-sketch-view-normal" => |s, on| s.draft.sketch_view_normal = on,
        "pref-p2p-autoconnect" => |s, on| s.draft.autoconnect_p2p = on,
        "pref-public-documents" => |s, on| s.draft.public_documents = on,
        "pref-mcp-enabled" => |s, on| s.draft.mcp.enabled = on,
        _ => return,
    };
    commands.queue(move |w: &mut World| edit(w, |s| change(s, on)));
}

/// A heading folded or unfolded stays so as the page is made again.
fn on_group_toggled(
    ev: On<CollapsibleToggled>,
    q: Query<&Name>,
    mut dialogs: Query<&mut PrefsDialog>,
) {
    let Some(key) = q
        .get(ev.entity)
        .ok()
        .and_then(|n| n.as_str().strip_prefix("pref-group-"))
    else {
        return;
    };
    for mut s in &mut dialogs {
        if ev.open {
            s.folded.remove(key);
        } else {
            s.folded.insert(key.to_string());
        }
    }
}

/// The texts typed: the search (the page follows), the seed peers, the MCP port (its endpoint
/// follows) and the rotation snap's fields.
fn follow_texts(
    fields: Query<(&Name, &EditableText), Changed<EditableText>>,
    numbers: Query<(&NumberFieldEdit, &EditableText), Changed<EditableText>>,
    names: Query<&Name>,
    dialogs: Query<&PrefsDialog>,
    mut labels: Query<(&Name, &mut Text)>,
    mut commands: Commands,
) {
    let Ok(s) = dialogs.single() else {
        return;
    };
    for (name, text) in &fields {
        let value = text.value().to_string();
        match name.as_str() {
            "pref-search-field" if value != s.query => {
                commands.queue(move |w: &mut World| {
                    edit(w, |s| s.query = value);
                    rebuild(w);
                });
            }
            "pref-public-seeds-field" if value != s.seeds => {
                commands.queue(move |w: &mut World| edit(w, |s| s.seeds = value));
            }
            "pref-mcp-port-field" if value != s.port => {
                if let Some(port) = parse_mcp_port(&value) {
                    let url = mcp_endpoint(port);
                    for (name, mut text) in &mut labels {
                        if name.as_str() == "pref-mcp-endpoint" && text.0 != url {
                            text.0 = url.clone();
                        }
                    }
                }
                commands.queue(move |w: &mut World| edit(w, |s| s.port = value));
            }
            _ => {}
        }
    }
    for (edit_of, text) in &numbers {
        let value = text.value().to_string();
        match names.get(edit_of.0).map(Name::as_str) {
            Ok("pref-rotation-snap") if value != s.snap => {
                commands.queue(move |w: &mut World| edit(w, |s| s.snap = value));
            }
            Ok("pref-rotation-snap-tolerance") if value != s.tolerance => {
                commands.queue(move |w: &mut World| edit(w, |s| s.tolerance = value));
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Apply, OK, Cancel

/// Apply: keeps the draft (saved), unless a field holds what it can't take, which shows its
/// section with the field in error. True when it was kept.
pub fn apply(world: &mut World) -> bool {
    let mut q = world.query::<(Entity, &PrefsDialog)>();
    let Some((dialog, s)) = q.iter(world).next().map(|(e, s)| (e, s.clone())) else {
        return false;
    };
    let snap = parse_rotation_snap(&s.snap);
    let tolerance = parse_rotation_snap(&s.tolerance).filter(|v| *v <= 90.0);
    if snap.is_none() || tolerance.is_none() {
        show_section(world, Section::Assembly);
        for (name, bad) in [
            ("pref-rotation-snap", snap.is_none()),
            ("pref-rotation-snap-tolerance", tolerance.is_none()),
        ] {
            let mut q = world.query::<(&Name, &mut NumberFieldState)>();
            if let Some((_, mut state)) = q.iter_mut(world).find(|(n, _)| n.as_str() == name) {
                state.error = bad;
            }
        }
        return false;
    }
    let port = if cfg!(target_arch = "wasm32") {
        s.draft.mcp.port
    } else {
        match parse_mcp_port(&s.port) {
            Some(p) => p,
            None => {
                show_section(world, Section::Assistants);
                crate::linked::toast(
                    world,
                    tr_with("shell-mcp-port-invalid", &[("text", &s.port)]),
                );
                return false;
            }
        }
    };
    let mut draft = s.draft;
    draft.rotation_snap_degrees = snap.unwrap_or_default();
    draft.rotation_snap_tolerance_degrees = tolerance.unwrap_or_default();
    draft.mcp.port = port;
    let now = world.resource::<LocalPreferences>().0;
    set_in_force(world, now, kept_all(now, &draft));
    save(world);
    let seeds = crate::collab::public::Seeds::parse(&s.seeds);
    if world.get_resource::<crate::collab::public::Seeds>() != Some(&seeds) {
        if let Some(store) = world.get_resource::<DocumentStore>()
            && let Err(e) = seeds.save(store.0.root())
        {
            warn!("cannot keep the seed peers: {e}");
        }
        world.insert_resource(seeds);
    }
    if let Some(mut s) = world.get_mut::<PrefsDialog>(dialog) {
        s.draft = draft;
        s.kept = draft;
        s.kept_seeds = s.seeds.clone();
        s.port = port.to_string();
        s.snap = degrees(draft.rotation_snap_degrees);
        s.tolerance = degrees(draft.rotation_snap_tolerance_degrees);
    }
    sync_apply(world, dialog);
    true
}

/// OK: Apply, then close.
pub fn ok(world: &mut World) {
    if !apply(world) {
        return;
    }
    let mut q = world.query_filtered::<Entity, With<PrefsDialog>>();
    if let Some(dialog) = q.iter(world).next() {
        world.trigger(DialogClose { entity: dialog });
    }
}

fn show_section(world: &mut World, section: Section) {
    edit(world, |s| {
        s.section = section;
        s.query.clear();
    });
    crate::pcb::dialogs::set_text(world, "pref-search-field", "");
    rebuild(world);
}

/// Closing (Cancel, ✕, Escape, or OK after Apply): what was kept is put back in force.
fn on_close(ev: On<DialogClose>, q: Query<&PrefsDialog>, mut commands: Commands) {
    let Ok(s) = q.get(ev.entity) else {
        return;
    };
    let kept = s.kept;
    commands.queue(move |w: &mut World| {
        let now = w.resource::<LocalPreferences>().0;
        set_in_force(w, now, kept_all(now, &kept));
    });
}

/// A rotation snap field shows what was typed, in error when it can't be kept.
fn on_snap_commit(ev: On<NumberFieldCommit>, mut fields: Query<(&Name, &mut NumberFieldState)>) {
    let Ok((name, mut state)) = fields.get_mut(ev.entity) else {
        return;
    };
    let tolerance = match name.as_str() {
        "pref-rotation-snap" => false,
        "pref-rotation-snap-tolerance" => true,
        _ => return,
    };
    state.text = ev.text.clone();
    state.error = parse_rotation_snap(&ev.text)
        .filter(|v| !tolerance || *v <= 90.0)
        .is_none();
}

pub(crate) fn parse_rotation_snap(text: &str) -> Option<f64> {
    let value: f64 = text
        .trim()
        .trim_end_matches("deg")
        .trim_end_matches('°')
        .trim()
        .parse()
        .ok()?;
    (value.is_finite() && (0.0..=180.0).contains(&value)).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use cadrs_ui::SwitchState;

    fn world() -> World {
        let mut world = World::new();
        world.init_resource::<LocalPreferences>();
        world.init_resource::<Theme>();
        world.insert_resource(State::new(AppState::Document));
        world.add_observer(on_close);
        world
    }

    /// Cancel (as ✕ and Escape): the dialog closes.
    fn cancel(world: &mut World) {
        let e = world
            .query_filtered::<Entity, With<PrefsDialog>>()
            .single(world)
            .unwrap();
        world.trigger(DialogClose { entity: e });
        world.flush();
        world.entity_mut(e).despawn();
    }

    fn named<C: Component>(world: &mut World, name: &str) -> Option<Entity> {
        let mut q = world.query_filtered::<(Entity, &Name), With<C>>();
        q.iter(world)
            .find(|(_, n)| n.as_str() == name)
            .map(|(e, _)| e)
    }

    fn dialog(world: &mut World) -> PrefsDialog {
        let mut q = world.query::<&PrefsDialog>();
        q.iter(world).next().unwrap().clone()
    }

    fn show(world: &mut World, section: Section) {
        edit(world, |s| s.section = section);
        rebuild(world);
    }

    /// The dialog opens on Interface with its settings; the other sections' come with theirs.
    #[test]
    fn the_sections_show_their_settings() {
        let mut world = world();
        open_preferences(&mut world);
        // Once.
        open_preferences(&mut world);
        assert_eq!(
            world
                .query_filtered::<(), With<PrefsDialog>>()
                .iter(&world)
                .count(),
            1
        );
        for name in [
            "pref-ui-scale",
            "pref-grid",
            "pref-origin",
            "pref-frame-stats",
        ] {
            assert!(named::<SelectState>(&mut world, name).is_some(), "{name}");
        }
        assert!(named::<SelectState>(&mut world, "pref-mouse-preset").is_none());
        show(&mut world, Section::Navigation);
        assert!(named::<SelectState>(&mut world, "pref-mouse-preset").is_some());
        assert!(named::<SelectState>(&mut world, "pref-grid").is_none());
        show(&mut world, Section::Assembly);
        let mut fields = world.query::<(&Name, &NumberFieldState)>();
        let (_, snap) = fields
            .iter(&world)
            .find(|(name, _)| name.as_str() == "pref-rotation-snap")
            .unwrap();
        assert_eq!(snap.text, "45 deg");
        let mut texts = world.query::<(&Name, &Text)>();
        let (_, hint) = texts
            .iter(&world)
            .find(|(name, _)| name.as_str() == "pref-rotation-snap-tolerance-note")
            .unwrap();
        assert!(hint.0.contains("Shift"));
    }

    /// What can be seen is previewed as it is chosen; Cancel puts it back, Apply keeps it.
    #[test]
    fn preview_cancel_and_apply() {
        let mut world = world();
        open_preferences(&mut world);
        edit(&mut world, |s| {
            s.draft.grid = GridStyle::Off;
            s.draft.language = Language::German;
        });
        let now = world.resource::<LocalPreferences>().0;
        assert_eq!(now.grid, GridStyle::Off, "previewed");
        assert_eq!(now.language, Language::English, "not until Apply");
        assert!(dialog(&mut world).dirty());
        cancel(&mut world);
        assert_eq!(
            world.resource::<LocalPreferences>().0.grid,
            GridStyle::Lines
        );

        open_preferences(&mut world);
        edit(&mut world, |s| s.draft.frame_stats = FrameStatsMode::Fps);
        assert!(apply(&mut world));
        assert!(!dialog(&mut world).dirty());
        cancel(&mut world);
        assert_eq!(
            world.resource::<LocalPreferences>().0.frame_stats,
            FrameStatsMode::Fps
        );
    }

    /// Restore Defaults resets the section shown, not the others.
    #[test]
    fn restore_defaults_resets_the_section() {
        let mut world = world();
        world.resource_mut::<LocalPreferences>().0.grid = GridStyle::Off;
        world.resource_mut::<LocalPreferences>().0.mouse = MousePreset::Creo;
        open_preferences(&mut world);
        edit(&mut world, PrefsDialog::restore_section);
        let s = dialog(&mut world);
        assert_eq!(s.draft.grid, GridStyle::Lines);
        assert_eq!(s.draft.mouse, MousePreset::Creo);
    }

    /// The search lists the settings that match from every section.
    #[test]
    fn search_finds_settings_in_every_section() {
        let mut world = world();
        open_preferences(&mut world);
        edit(&mut world, |s| s.query = "snap".into());
        rebuild(&mut world);
        assert!(named::<NumberFieldState>(&mut world, "pref-rotation-snap").is_some());
        assert!(named::<SelectState>(&mut world, "pref-grid").is_none());
        edit(&mut world, |s| s.query = "zzz".into());
        rebuild(&mut world);
        let mut texts = world.query::<&Name>();
        assert!(texts.iter(&world).any(|n| n.as_str() == "pref-no-match"));
    }

    /// Rotation snap fields are validated before anything is kept.
    #[test]
    fn rotation_snap_is_validated_before_saving() {
        assert_eq!(parse_rotation_snap("22.5 deg"), Some(22.5));
        assert_eq!(parse_rotation_snap("0°"), Some(0.0));
        assert_eq!(parse_rotation_snap("180"), Some(180.0));
        for text in ["-1", "181", "NaN", "inf", "", "abc"] {
            assert_eq!(parse_rotation_snap(text), None);
        }
        let mut world = world();
        open_preferences(&mut world);
        edit(&mut world, |s| {
            s.snap = "22.5".into();
            s.tolerance = "15".into();
        });
        assert!(apply(&mut world));
        let p = world.resource::<LocalPreferences>().0;
        assert_eq!(p.rotation_snap_degrees(), 22.5);
        assert_eq!(p.rotation_snap_tolerance_degrees(), 15.0);
        edit(&mut world, |s| {
            s.snap = "60".into();
            s.tolerance = "91".into();
        });
        assert!(!apply(&mut world));
        let p = world.resource::<LocalPreferences>().0;
        assert_eq!(p.rotation_snap_degrees(), 22.5, "nothing kept");
        assert_eq!(dialog(&mut world).section, Section::Assembly);
        let e = named::<NumberFieldState>(&mut world, "pref-rotation-snap-tolerance").unwrap();
        assert!(world.get::<NumberFieldState>(e).unwrap().error);
    }

    /// Vertical sync: the switch shows it, and a change is previewed.
    #[test]
    fn vsync_switch_follows_and_previews() {
        let mut world = world();
        open_preferences(&mut world);
        let e = named::<SwitchState>(&mut world, "pref-vsync").unwrap();
        assert!(world.get::<SwitchState>(e).unwrap().on);
        edit(&mut world, |s| s.draft.vsync = false);
        assert!(!world.resource::<LocalPreferences>().0.vsync);
    }

    #[test]
    fn gesture_rows_read_like_the_table() {
        assert_eq!(
            gesture_text(MousePreset::Onshape, ViewAction::Rotate),
            "Right drag"
        );
        assert_eq!(
            gesture_text(MousePreset::Onshape, ViewAction::RotateTurntable),
            "Alt+Right drag"
        );
        assert_eq!(
            gesture_text(MousePreset::SolidWorks, ViewAction::RotateTurntable),
            "—"
        );
        assert_eq!(
            gesture_text(MousePreset::Onshape, ViewAction::Pan),
            "Middle drag, Ctrl+Right drag"
        );
        assert_eq!(
            gesture_text(MousePreset::SolidWorks, ViewAction::Zoom),
            "Scroll wheel, Shift+Middle drag"
        );
    }
}

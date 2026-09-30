//! The Create Drawing dialog (D1.3–D1.5, X2; `lesson-create-drawing-dialog.png`), opened from
//! the tab bar's "+" → Create Drawing… or a tab's "Create Drawing of X…".
//!
//! - **Existing templates**: a source list on the left (Built-in, This document, My templates,
//!   Recently used; shared and company libraries are out of scope), All / ANSI / ISO filter
//!   tabs, and a Template / Document / Owner table. Click a row to pick it; double-click is OK.
//! - **Custom template**: standard, size, orientation, units and projection (first or third
//!   angle). OK creates the drawing and keeps the template in "My templates" (a local file next
//!   to the documents, `drawing-templates.ron`).
//! - **Options**: Four views / No views. Four views (P3C.2) needs a referenced part or Part
//!   Studio ("Create Drawing of X…"): OK then places Front, the top and side views of the
//!   template's projection and a shaded isometric view at a scale that fits the sheet. From the
//!   "+" menu there is no reference, so it is unavailable, as in the course's screenshot.
//!
//! OK adds the drawing tab right of the active tab as one undoable step and opens it. With No
//! views, Insert view starts at once (D1.7).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_core::commands::{EditDrawing, InsertElement};
use cadrs_core::{Element, ElementId, ElementKind};
use cadrs_drawing::template::builtin_templates;
use cadrs_drawing::{
    Drawing, DrawingOp, DrawingUnits, ObjectRef, Orientation, Projection, SheetSize, Standard,
    Template,
};
use cadrs_ui::prelude::*;
use cadrs_ui::Button;
use cadrs_ui::{
    Column, DialogClose, DoubleClickable, Select, SelectChange, SelectState, TabStrip,
    TabStripSelect, form_row,
};
use serde::{Deserialize, Serialize};

use crate::{ActiveDocument, AppClock, AppState, DocumentStore, UserProfile};

pub struct CreateDrawingPlugin;

impl Plugin for CreateDrawingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, sync_body.run_if(in_state(AppState::Document)))
            .add_observer(on_tab_select)
            .add_observer(on_select_change);
    }
}

/// The template sources of the left list.
pub const SOURCES: [(&str, &str, &str); 4] = [
    ("template-source-builtin", "Built-in", "books"),
    ("template-source-document", "This document", "details"),
    ("template-source-mine", "My templates", "user"),
    ("template-source-recent", "Recently used", "clock"),
];

/// Locally kept templates: custom ones ("My templates") and the recently used names.
#[derive(Resource, Debug, Default, Clone, Serialize, Deserialize)]
pub struct TemplateLibrary {
    #[serde(default)]
    pub custom: Vec<Template>,
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(skip)]
    loaded: bool,
}

const LIBRARY_FILE: &str = "drawing-templates.ron";

impl TemplateLibrary {
    fn ensure_loaded(&mut self, store: &DocumentStore) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let path = store.0.root().join(LIBRARY_FILE);
        if let Ok(text) = std::fs::read_to_string(&path) {
            match ron::from_str::<TemplateLibrary>(&text) {
                Ok(lib) => {
                    self.custom = lib.custom;
                    self.recent = lib.recent;
                }
                Err(e) => warn!("cannot read {}: {e}", path.display()),
            }
        }
    }

    fn save(&self, store: &DocumentStore) {
        let path = store.0.root().join(LIBRARY_FILE);
        let _ = std::fs::create_dir_all(store.0.root());
        match ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            Ok(text) => {
                if let Err(e) = std::fs::write(&path, text) {
                    warn!("cannot save {}: {e}", path.display());
                }
            }
            Err(e) => warn!("cannot save the template library: {e}"),
        }
    }

    /// A template by name: built-in or custom.
    pub fn find(&self, name: &str) -> Option<Template> {
        cadrs_drawing::template::builtin(name)
            .or_else(|| self.custom.iter().find(|t| t.name == name).cloned())
    }
}

/// The dialog's state; the body is rebuilt when it changes.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct CreateDrawingState {
    pub name: String,
    pub reference: Option<ObjectRef>,
    /// 0 Existing templates, 1 Custom template.
    pub tab: usize,
    pub source: usize,
    /// 0 All, 1 ANSI, 2 ISO.
    pub filter: usize,
    pub selected: Option<String>,
    pub custom: CustomChoice,
    /// Template names of drawings in this document (the "This document" source).
    pub in_document: Vec<String>,
    /// The Four views option (else No views).
    pub four_views: bool,
}

/// The Custom template tab's settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CustomChoice {
    pub standard: Standard,
    pub size: SheetSize,
    pub orientation: Orientation,
    pub units: DrawingUnits,
    pub projection: Projection,
}

impl Default for CustomChoice {
    fn default() -> Self {
        Self {
            standard: Standard::Iso,
            size: SheetSize::IsoA3,
            orientation: Orientation::Landscape,
            units: DrawingUnits::Millimeter,
            projection: Projection::First,
        }
    }
}

impl CustomChoice {
    pub fn template(&self) -> Template {
        Template::custom(self.size, self.orientation, self.units, self.projection)
    }
}

/// The body container, and the state it was last built from.
#[derive(Component, Default)]
struct CreateDrawingBody(Option<CreateDrawingState>);

/// Opens the dialog. `reference` is the part or assembly the drawing is of (Create Drawing of
/// X…), or `None` from the "+" menu.
pub fn open_create_drawing(world: &mut World, reference: Option<ObjectRef>) {
    let Some(doc) = world.get_resource::<ActiveDocument>() else {
        return;
    };
    let name = doc.doc.next_element_name("Drawing");
    // Opened on a part (an instance's or the Parts list's menu): the title names it.
    let of = reference.and_then(|r| r.part.map(|(f, index)| (r.element, f, index))).map(|(e, f, index)| {
        let owner = cadrs_core::properties::PropertyOwner::Part {
            element: cadrs_core::ElementId(e),
            part: cadrs_core::ids::PartId { feature: cadrs_core::FeatureId(f), index },
        };
        cadrs_core::properties::text(&doc.doc, owner, cadrs_core::properties::PropertyKey::Name, None)
    });
    let title = match of.filter(|n| !n.trim().is_empty()) {
        Some(part) => format!("Create Drawing: {name} of {part}"),
        None => format!("Create Drawing: {name}"),
    };
    let mut in_document: Vec<String> = Vec::new();
    for el in &doc.doc.elements {
        if let ElementKind::Drawing(d) = &el.kind
            && !in_document.contains(&d.template.name)
        {
            in_document.push(d.template.name.clone());
        }
    }
    {
        let store = world.resource::<DocumentStore>().clone();
        world.resource_mut::<TemplateLibrary>().ensure_loaded(&store);
    }
    let state = CreateDrawingState {
        name: name.clone(),
        reference,
        tab: 0,
        source: 0,
        filter: 0,
        selected: None,
        custom: CustomChoice::default(),
        in_document,
        four_views: false,
    };
    let theme = world.resource::<Theme>().clone();
    let tf = theme.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("create-drawing-dialog")
            .title(title)
            .width(720.0)
            .body(move |b| {
                b.spawn((
                    Name::new("create-drawing-body"),
                    CreateDrawingBody(None),
                    Node {
                        flex_direction: FlexDirection::Column,
                        height: Val::Px(430.0),
                        ..default()
                    },
                ));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("create-drawing-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(accept);
                    }),
                ));
                f.spawn((
                    Button::new("create-drawing-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<CreateDrawingState>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        state,
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

/// The templates the Existing tab lists for the current source and filter.
pub fn listed_templates(state: &CreateDrawingState, lib: &TemplateLibrary) -> Vec<Template> {
    let all: Vec<Template> = match state.source {
        0 => builtin_templates(),
        1 => state.in_document.iter().filter_map(|n| lib.find(n)).collect(),
        2 => lib.custom.clone(),
        _ => lib.recent.iter().filter_map(|n| lib.find(n)).collect(),
    };
    all.into_iter()
        .filter(|t| match state.filter {
            1 => t.standard_kind() == Standard::Ansi,
            2 => t.standard_kind() == Standard::Iso,
            _ => true,
        })
        .collect()
}

fn update_state(world: &mut World, f: impl FnOnce(&mut CreateDrawingState)) {
    let mut q = world.query::<&mut CreateDrawingState>();
    if let Some(mut s) = q.iter_mut(world).next() {
        f(&mut s);
    }
}

fn sync_body(
    q_state: Query<&CreateDrawingState>,
    mut q_body: Query<(Entity, &mut CreateDrawingBody)>,
    lib: Res<TemplateLibrary>,
    theme: Res<Theme>,
    mut commands: Commands,
) {
    let Ok(state) = q_state.single() else {
        return;
    };
    let Ok((body, mut built)) = q_body.single_mut() else {
        return;
    };
    if built.0.as_ref() == Some(state) {
        return;
    }
    built.0 = Some(state.clone());
    commands.entity(body).despawn_children();
    let t = theme.clone();
    let state = state.clone();
    let templates = listed_templates(&state, &lib);
    commands.entity(body).with_children(|b| {
        b.spawn(
            TabStrip::new("create-drawing-tabs")
                .tab("Existing templates")
                .tab("Custom template")
                .selected(state.tab)
                .build(&t),
        );
        b.spawn((
            Name::new("create-drawing-main"),
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                margin: UiRect::top(Val::Px(8.0)),
                ..default()
            },
        ))
        .with_children(|m| {
            if state.tab == 0 {
                existing_tab(m, &t, &state, templates);
            } else {
                custom_tab(m, &t, &state);
            }
        });
        options_row(b, &t, &state);
    });
}

fn existing_tab(m: &mut ChildSpawnerCommands, t: &Theme, state: &CreateDrawingState, templates: Vec<Template>) {
    // Source list.
    m.spawn((
        Name::new("template-sources"),
        Node {
            width: Val::Px(170.0),
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Column,
            border: UiRect::right(Val::Px(1.0)),
            padding: UiRect::right(Val::Px(6.0)),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|s| {
        for (i, (name, label, icon)) in SOURCES.iter().enumerate() {
            s.spawn((
                ListItem::new(*name)
                    .icon(*icon)
                    .label(*label)
                    .height(28.0)
                    .weight(if i == state.source { FontWeight::SEMIBOLD } else { FontWeight::NORMAL })
                    .selection_indicator()
                    .selected(i == state.source)
                    .build(t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |w: &mut World| {
                        update_state(w, |s| {
                            s.source = i;
                            s.selected = None;
                        })
                    });
                }),
            ));
        }
    });
    // Filter tabs and the table.
    m.spawn((
        Name::new("template-list"),
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::left(Val::Px(12.0)),
            min_width: Val::Px(0.0),
            ..default()
        },
    ))
    .with_children(|l| {
        l.spawn(
            TabStrip::new("template-filter")
                .tab("All")
                .tab("ANSI")
                .tab("ISO")
                .selected(state.filter)
                .build(t),
        )
        .entry::<Node>()
        .and_modify(|mut n| n.margin = UiRect::bottom(Val::Px(6.0)));
        let columns = vec![
            Column::new("template", "Template").width(210.0),
            Column::new("document", "Document"),
            Column::new("owner", "Owner").width(80.0),
        ];
        l.spawn(TableHeader::new("template-table-header", columns.clone()).build(t));
        // The rows scroll under the header, with a slim scrollbar at the right.
        l.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            ..default()
        })
        .with_children(|wrap| {
        let list = wrap.spawn((
            Name::new("template-table-rows"),
            bevy::ui_widgets::ScrollArea,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
                padding: UiRect::right(Val::Px(14.0)),
                ..default()
            },
        ))
        .with_children(|rows| {
            if templates.is_empty() {
                rows.spawn((
                    Name::new("template-table-empty"),
                    t.text("No templates here yet", t.font_base, FontWeight::NORMAL, t.muted_foreground),
                    Node {
                        margin: UiRect::all(Val::Px(12.0)),
                        ..default()
                    },
                ));
            }
            for tpl in templates {
                let selected = state.selected.as_deref() == Some(tpl.name.as_str());
                let name = tpl.name.clone();
                // The picked row: a solid blue bar with bold text (lesson-create-drawing-dialog.png),
                // clearly unlike the grey hover.
                let weight = if selected { FontWeight::BOLD } else { FontWeight::MEDIUM };
                let mut row = TableRow::new(format!("template-row-{}", tpl.name), &columns)
                    .height(26.0)
                    .selected(selected);
                for text in [tpl.name.clone(), tpl.document_label(), tpl.owner_label().to_string()] {
                    let cell = (
                        t.text(text, t.font_base, weight, t.foreground),
                        cadrs_ui::InheritFg,
                        Pickable::IGNORE,
                    );
                    row = row.cell(move |p| {
                        p.spawn(cell);
                    });
                }
                let mut visuals = cadrs_ui::Visuals {
                    background: cadrs_ui::StateColors::new(Color::NONE, t.list_hover, t.list_active, Color::NONE)
                        .with_selected(selected_row_color()),
                    border: cadrs_ui::StateColors::all(t.row_separator),
                    foreground: cadrs_ui::StateColors::all(t.foreground),
                    focus_ring: t.focus_ring,
                };
                visuals.foreground = visuals.foreground.with_selected(t.foreground);
                rows.spawn((
                    row.build(t),
                    DoubleClickable,
                    observe(move |_: On<Activate>, mut commands: Commands| {
                        let n = name.clone();
                        commands.queue(move |w: &mut World| update_state(w, |s| s.selected = Some(n)));
                    }),
                    observe(|_: On<DoubleClick>, mut commands: Commands| {
                        commands.queue(accept);
                    }),
                )).insert(visuals);
            }
        })
        .id();
        wrap.spawn(cadrs_ui::vertical_scrollbar(t, "template-table-scrollbar", list));
        });
    });
}

/// The picked template row (Onshape's saturated selection blue).
fn selected_row_color() -> Color {
    Color::srgb_u8(0xb3, 0xd4, 0xf2)
}

fn custom_tab(m: &mut ChildSpawnerCommands, t: &Theme, state: &CreateDrawingState) {
    let c = state.custom;
    m.spawn((
        Name::new("custom-template-form"),
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::left(Val::Px(8.0)),
            row_gap: Val::Px(2.0),
            ..default()
        },
    ))
    .with_children(|f| {
        let lw = 150.0;
        let select = |name: &'static str, options: Vec<String>, selected: usize| {
            let mut s = Select::new(name).width(Val::Px(240.0));
            for o in options {
                s = s.option(o, true);
            }
            s.selected(selected)
        };
        let std_i = Standard::ALL.iter().position(|s| *s == c.standard).unwrap_or(0);
        f.spawn(form_row(t, "custom-standard-row", "Standard", lw)).with_child(
            select("custom-standard", Standard::ALL.iter().map(|s| s.label().to_string()).collect(), std_i)
                .build(t),
        );
        let sizes = c.standard.sizes();
        let size_i = sizes.iter().position(|s| *s == c.size).unwrap_or(0);
        f.spawn(form_row(t, "custom-size-row", "Size", lw)).with_child(
            select("custom-size", sizes.iter().map(|s| s.long_label()).collect(), size_i).build(t),
        );
        let o_i = Orientation::ALL.iter().position(|o| *o == c.orientation).unwrap_or(0);
        f.spawn(form_row(t, "custom-orientation-row", "Orientation", lw)).with_child(
            select(
                "custom-orientation",
                Orientation::ALL.iter().map(|o| o.label().to_string()).collect(),
                o_i,
            )
            .build(t),
        );
        let u_i = DrawingUnits::ALL.iter().position(|u| *u == c.units).unwrap_or(0);
        f.spawn(form_row(t, "custom-units-row", "Units", lw)).with_child(
            select("custom-units", DrawingUnits::ALL.iter().map(|u| u.label().to_string()).collect(), u_i)
                .build(t),
        );
        let p_i = Projection::ALL.iter().position(|p| *p == c.projection).unwrap_or(0);
        f.spawn(form_row(t, "custom-projection-row", "Projection", lw)).with_child(
            select(
                "custom-projection",
                Projection::ALL.iter().map(|p| p.label().to_string()).collect(),
                p_i,
            )
            .build(t),
        );
        f.spawn((
            Name::new("custom-template-name"),
            t.text(
                format!("Template: {}", c.template().name),
                t.font_base,
                FontWeight::MEDIUM,
                t.foreground,
            ),
            Node {
                margin: UiRect::top(Val::Px(10.0)),
                ..default()
            },
        ));
        f.spawn(t.text(
            "OK creates the drawing and keeps this template in My templates.",
            t.font_sm,
            FontWeight::NORMAL,
            t.muted_foreground,
        ));
    });
}

/// "Options": the Four views / No views tiles.
fn options_row(b: &mut ChildSpawnerCommands, t: &Theme, state: &CreateDrawingState) {
    let can_four = state.reference.is_some();
    b.spawn((
        Node {
            flex_shrink: 0.0,
            margin: UiRect::top(Val::Px(8.0)),
            padding: UiRect::top(Val::Px(6.0)),
            border: UiRect::top(Val::Px(1.0)),
            flex_direction: FlexDirection::Column,
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|o| {
        o.spawn(t.text("Options", t.font_base, FontWeight::SEMIBOLD, t.foreground));
        o.spawn(Node {
            justify_content: JustifyContent::Center,
            column_gap: Val::Px(8.0),
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        })
        .with_children(|r| {
            for (name, label, icon_name, selected, enabled) in [
                ("template-option-four-views", "Four views", "apps", state.four_views, can_four),
                ("template-option-no-views", "No views", "file", !state.four_views, true),
            ] {
                let four = name == "template-option-four-views";
                let fg = if enabled { t.foreground } else { t.disabled_foreground };
                r.spawn((
                    Name::new(name),
                    Node {
                        width: Val::Px(92.0),
                        height: Val::Px(62.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        row_gap: Val::Px(4.0),
                        border: UiRect::bottom(Val::Px(if selected { 2.0 } else { 0.0 })),
                        ..default()
                    },
                    BackgroundColor(if selected {
                        Color::srgb_u8(0xdd, 0xe9, 0xf5)
                    } else {
                        Color::srgb_u8(0xf0, 0xf0, 0xf0)
                    }),
                    BorderColor::all(t.primary),
                    Tooltip::new(match (four, enabled) {
                        (false, _) => "Start with an empty sheet and insert views yourself",
                        (true, true) => "Front, top, side and isometric views of the referenced object",
                        (true, false) => "Four views needs a referenced part: use Create Drawing of… on its tab",
                    }),
                    Interaction::default(),
                    observe(move |_: On<Pointer<Click>>, mut commands: Commands| {
                        if enabled {
                            commands.queue(move |w: &mut World| update_state(w, |s| s.four_views = four));
                        }
                    }),
                ))
                .with_children(|tile| {
                    tile.spawn((icon(icon_name, 24.0, fg), Pickable::IGNORE));
                    tile.spawn((t.text(label, t.font_sm, FontWeight::MEDIUM, fg), Pickable::IGNORE));
                });
            }
        });
    });
}

fn on_tab_select(ev: On<TabStripSelect>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    match name.as_str() {
        "create-drawing-tabs" => commands.queue(move |w: &mut World| update_state(w, |s| s.tab = i)),
        "template-filter" => commands.queue(move |w: &mut World| {
            update_state(w, |s| {
                s.filter = i;
                s.selected = None;
            })
        }),
        _ => {}
    }
}

fn on_select_change(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    let name = name.as_str().to_string();
    if !name.starts_with("custom-") {
        return;
    }
    commands.queue(move |w: &mut World| {
        update_state(w, |s| {
            let c = &mut s.custom;
            match name.as_str() {
                "custom-standard" => {
                    if let Some(std) = Standard::ALL.get(i).copied()
                        && std != c.standard
                    {
                        c.standard = std;
                        c.size = std.sizes()[0];
                        c.projection = std.default_projection();
                        c.units = match std {
                            Standard::Ansi => DrawingUnits::Inch,
                            Standard::Iso => DrawingUnits::Millimeter,
                        };
                    }
                }
                "custom-size" => {
                    if let Some(sz) = c.standard.sizes().get(i) {
                        c.size = *sz;
                    }
                }
                "custom-orientation" => c.orientation = Orientation::ALL[i.min(1)],
                "custom-units" => c.units = DrawingUnits::ALL[i.min(1)],
                "custom-projection" => c.projection = Projection::ALL[i.min(1)],
                _ => {}
            }
        })
    });
}

/// The date a drawing is drawn on, `YYYY-MM-DD`.
fn today(clock: &AppClock) -> String {
    chrono::DateTime::from_timestamp(clock.now() + clock.utc_offset, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// OK: creates the drawing from the chosen template.
fn accept(world: &mut World) {
    let mut q = world.query::<(Entity, &CreateDrawingState)>();
    let Some((dialog, state)) = q.iter(world).next().map(|(e, s)| (e, s.clone())) else {
        return;
    };
    let store = world.resource::<DocumentStore>().clone();
    let template = if state.tab == 1 {
        Some(state.custom.template())
    } else {
        let lib = world.resource::<TemplateLibrary>();
        state.selected.as_deref().and_then(|n| lib.find(n))
    };
    let Some(template) = template else {
        let theme = world.resource::<Theme>().clone();
        let mut commands = world.commands();
        cadrs_ui::show_toast(&mut commands, &theme, "Select a template first");
        world.flush();
        return;
    };
    {
        let mut lib = world.resource_mut::<TemplateLibrary>();
        if template.source == cadrs_drawing::TemplateSource::Custom
            && !lib.custom.iter().any(|t| t.name == template.name)
        {
            lib.custom.push(template.clone());
        }
        lib.recent.retain(|n| *n != template.name);
        lib.recent.insert(0, template.name.clone());
        lib.recent.truncate(8);
        lib.save(&store);
    }
    let mut drawing = Drawing::from_template(&template, state.reference);
    drawing.title.drawn_by = Some(world.resource::<UserProfile>().display_name.clone());
    drawing.title.drawn_date = Some(today(world.resource::<AppClock>()));
    let four = state.four_views && state.reference.is_some();
    if four
        && let Some(r) = state.reference
        && let Some(doc) = world.get_resource::<ActiveDocument>()
    {
        let mut views = four_views_of(&doc.doc, &drawing, r);
        // P3C.6: the drawing shows the studio as it is now, until it is updated.
        if let Some(src) = cadrs_core::drawing_source::live_source(&doc.doc, cadrs_core::ElementId(r.element)) {
            for v in &mut views {
                v.source_hash = src.hash_of(v.reference.part);
            }
            drawing.sources.push(src);
        }
        if let Some(sheet) = drawing.sheets.first_mut() {
            if let Some(f) = views.first() {
                sheet.scale = f.scale;
            }
            sheet.views = views;
        }
    }
    let element = Element::drawing(state.name.clone(), drawing);
    let id = element.id;
    if let Some(mut doc) = world.get_resource_mut::<ActiveDocument>() {
        let after = doc.active;
        match doc.execute(&InsertElement {
            element,
            after,
            label: "Create Drawing".into(),
        }) {
            Ok(()) => doc.set_active(id),
            Err(e) => warn!("cannot create the drawing: {e}"),
        }
    }
    world.trigger(DialogClose { entity: dialog });
    // D1.7: with No views, Insert view starts at once.
    if !four {
        world.flush();
        super::view_tools::open_insert_view(world);
    }
}

/// The Four views of `r` on the first sheet of `d` (above the title block).
fn four_views_of(doc: &cadrs_core::Document, d: &Drawing, r: ObjectRef) -> Vec<cadrs_drawing::View> {
    let Some(sheet) = d.sheets.first() else {
        return Vec::new();
    };
    // An assembly's (P3C.5): its occurrences' meshes where they are placed.
    let parts: Vec<cadrs_core::Part> = if cadrs_core::drawing_assembly::is_assembly(doc, cadrs_core::ElementId(r.element)) {
        let Some(state) = cadrs_core::drawing_assembly::AssemblyState::of(doc, cadrs_core::ElementId(r.element)) else {
            return Vec::new();
        };
        let builds = cadrs_core::drawing_assembly::builds(&state);
        state
            .occurrences
            .iter()
            .filter(|o| !o.hidden)
            .filter_map(|o| {
                let p = builds.get(&o.element)?.part(o.part)?;
                let mut q = p.clone();
                q.solid = std::sync::Arc::new(cadrs_core::assembly::transform_solid(&p.solid, &o.pose));
                Some(q)
            })
            .collect()
    } else {
        let Some(studio) = super::views::studio(doc, &r) else {
            return Vec::new();
        };
        let build = cadrs_core::rebuild::build(studio.features);
        cadrs_core::views::view_parts(&build.parts, super::views::part_of(&r)).into_iter().cloned().collect()
    };
    let parts: Vec<&cadrs_core::Part> = parts.iter().collect();
    let frame = cadrs_drawing::standard::frame(sheet.format);
    let tb = cadrs_drawing::title_block::placement(frame.inner, sheet.format.size);
    let area = cadrs_drawing::standard::Rect::new(frame.inner.min[0], tb.max[1], frame.inner.max[0], frame.inner.max[1]);
    cadrs_drawing::view::four_views(
        r,
        d.projection,
        area,
        12.0,
        d.style.hidden_lines,
        d.style.tangent_edges,
        |f| cadrs_core::views::mesh_bounds(&parts, &f.view_frame()),
    )
}

// ---------------------------------------------------------------------------------------------
// Update properties from a template

#[derive(Component)]
struct UpdateTemplateDialog(Vec<String>);

/// "Update properties from a template…" in the Drawing properties panel.
pub fn open_update_from_template(world: &mut World) {
    let Some(current) = world
        .get_resource::<ActiveDocument>()
        .and_then(|d| super::active_drawing(d).map(|(_, dr)| dr.template.name.clone()))
    else {
        return;
    };
    {
        let store = world.resource::<DocumentStore>().clone();
        world.resource_mut::<TemplateLibrary>().ensure_loaded(&store);
    }
    let lib = world.resource::<TemplateLibrary>().clone();
    let mut names: Vec<String> = builtin_templates().into_iter().map(|t| t.name).collect();
    names.extend(lib.custom.iter().map(|t| t.name.clone()));
    let selected = names.iter().position(|n| *n == current).unwrap_or(0);
    let theme = world.resource::<Theme>().clone();
    let (tb, tf) = (theme.clone(), theme.clone());
    let list = names.clone();
    let mut commands = world.commands();
    commands.spawn((
        Dialog::new("update-template-dialog")
            .title("Update properties from a template")
            .width(420.0)
            .body(move |b| {
                let t = &tb;
                let mut s = Select::new("update-template-select").width(Val::Px(240.0));
                for n in &list {
                    s = s.option(n.clone(), true);
                }
                b.spawn(form_row(t, "update-template-row", "Template", 90.0))
                    .with_child(s.selected(selected).build(t));
                b.spawn((
                    t.text(
                        "Replaces the drawing properties, units and projection with the template's.",
                        t.font_sm,
                        FontWeight::NORMAL,
                        t.muted_foreground,
                    ),
                    Node {
                        max_width: Val::Px(380.0),
                        ..default()
                    },
                ))
                .insert(TextLayout::new(bevy::text::Justify::Left, bevy::text::LineBreak::WordBoundary));
            })
            .footer(move |f| {
                let t = &tf;
                f.spawn((
                    Button::new("update-template-ok").label("OK").primary().build(t),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(apply_update_from_template);
                    }),
                ));
                f.spawn((
                    Button::new("update-template-cancel").label("Cancel").build(t),
                    observe(|_: On<Activate>, q: Query<Entity, With<UpdateTemplateDialog>>, mut commands: Commands| {
                        for e in &q {
                            commands.trigger(DialogClose { entity: e });
                        }
                    }),
                ));
            })
            .build(&theme),
        UpdateTemplateDialog(names),
        DespawnOnExit(AppState::Document),
    ));
    world.flush();
}

fn apply_update_from_template(world: &mut World) {
    let mut q = world.query::<(Entity, &UpdateTemplateDialog)>();
    let Some((dialog, names)) = q.iter(world).next().map(|(e, d)| (e, d.0.clone())) else {
        return;
    };
    let mut qs = world.query::<(&Name, &SelectState)>();
    let i = qs
        .iter(world)
        .find(|(n, _)| n.as_str() == "update-template-select")
        .map(|(_, s)| s.selected)
        .unwrap_or(0);
    let template = names
        .get(i)
        .and_then(|n| world.resource::<TemplateLibrary>().find(n));
    if let Some(template) = template
        && let Some(mut doc) = world.get_resource_mut::<ActiveDocument>()
        && let Some(element) = super::active_drawing(&doc).map(|(id, _)| id)
        && let Err(e) = doc.execute(&EditDrawing {
            element,
            op: DrawingOp::UpdateFromTemplate(template),
        })
    {
        warn!("cannot update from the template: {e}");
    }
    world.trigger(DialogClose { entity: dialog });
}

/// "Create Drawing of X…": a reference to element `id`.
pub fn reference_to(id: ElementId) -> ObjectRef {
    ObjectRef {
        element: id.0,
        part: None,
    }
}

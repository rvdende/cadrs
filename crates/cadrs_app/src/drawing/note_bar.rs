//! The floating cards of notes and tables (P3C.4):
//!
//! - **Note toolbar** (D9.3, D9.4), above the note or cell being typed: bold, italic,
//!   underline, strikethrough; left, centre and right alignment; bulleted and numbered lists; the
//!   text height (drawing units); the symbol menu (Ø ° ± ⌴ ⌵ ↧ and more); **Insert sheet
//!   reference property** and **Insert drawing property**, each a small card to pick the
//!   property, its text format and, for dates, the date format, with a preview; ✓ and ✗.
//! - **Cell toolbar** (D10.3), above a table with a selected cell: insert rows above and below
//!   and columns left and right, remove the row or column, merge and unmerge, bold, italic,
//!   underline and alignment for the selected cells.
//! - **Table dialog** (D10.1): rows, columns, title row, header row and the fixed corner; the
//!   Table tool places the table with a click on the sheet.
//! - **Table properties** (D10.4): the fixed corner of a placed table.

use bevy::prelude::*;
use bevy::text::FontWeight;
use cadrs_drawing::rich::{Attr, DrawingProp, Field, HAlign, ListKind, Prop, RefProp, TextCase};
use cadrs_drawing::style::DateFormat;
use cadrs_drawing::table::{Corner, Table, TableId};
use cadrs_drawing::{DrawingOp, DrawingUnits};
use cadrs_ui::glyphs::{Glyph, glyph};
use cadrs_ui::prelude::*;
use cadrs_ui::{Button, CheckboxChange, Select, SelectChange};

use super::annotations::{AnnTool, AnnotationUi};
use super::notes::{self, EditTarget, Item, NoteScene, NotesUi, field_sources, find_table};
use super::view_tools::edit_drawing;
use super::{DrawingUi, active_drawing, current_view, sheet_area, sheet_to_screen};
use crate::viewport::{ActiveKind, ViewportRect};
use crate::{ActiveDocument, AppState};

pub struct NoteBarPlugin;

impl Plugin for NoteBarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (sync_note_bar, sync_cell_bar, sync_toggles, read_table_dialog)
                .chain()
                .after(super::annotations::AnnotationDrawSet)
                .run_if(in_state(AppState::Document)),
        )
        .add_observer(on_select)
        .add_observer(on_check)
        .add_observer(on_submit)
        .add_observer(on_symbol);
    }
}

pub(crate) fn card_bundle(t: &Theme, name: &str, at: Vec2) -> impl Bundle {
    (
        Name::new(name.to_string()),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x.max(4.0)),
            top: Val::Px(at.y.max(4.0)),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(t.background),
        BorderColor::all(Color::srgb_u8(0xd8, 0xd8, 0xd8)),
        BoxShadow::new(Color::srgba(0.0, 0.0, 0.0, 0.3), Val::Px(1.0), Val::Px(2.0), Val::Px(0.0), Val::Px(6.0)),
        GlobalZIndex(cadrs_ui::z::DIALOG - 12),
        DespawnOnExit(AppState::Document),
    )
}

/// A card's title row: the title, then ✓ (when `accept`) and ✗.
pub(crate) fn header(
    c: &mut ChildSpawnerCommands,
    t: &Theme,
    title: &str,
    prefix: &str,
    accept: Option<fn(&mut World)>,
    cancel: fn(&mut World),
) {
    c.spawn((
        Node {
            height: Val::Px(28.0),
            padding: UiRect::horizontal(Val::Px(6.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(2.0),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(t.separator),
    ))
    .with_children(|h| {
        h.spawn((
            t.text(title, t.font_base, FontWeight::BOLD, t.foreground),
            Node {
                flex_grow: 1.0,
                margin: UiRect::right(Val::Px(12.0)),
                ..default()
            },
        ));
        if let Some(f) = accept {
            let visuals = cadrs_ui::Visuals {
                background: cadrs_ui::StateColors::new(t.accept, t.accept.darker(0.04), t.accept.darker(0.08), t.accept_disabled),
                border: cadrs_ui::StateColors::all(Color::NONE),
                foreground: cadrs_ui::StateColors::all(Color::WHITE),
                focus_ring: t.focus_ring,
            };
            h.spawn((
                Button::new(format!("{prefix}-ok")).icon("check-bold").icon_size(14.0).tooltip("Accept").build(t),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(f);
                }),
            ))
            .insert((
                visuals,
                Node {
                    width: Val::Px(22.0),
                    height: Val::Px(20.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
            ));
        }
        h.spawn((
            Button::new(format!("{prefix}-cancel")).icon("x-bold").icon_size(14.0).ghost().tooltip("Cancel").build(t),
            observe(move |_: On<Activate>, mut commands: Commands| {
                commands.queue(cancel);
            }),
        ))
        .insert(TextColor(t.cancel));
    });
}

/// A small square toolbar button with an icon or a glyph, running `f` when clicked.
fn tool(
    r: &mut ChildSpawnerCommands,
    t: &Theme,
    name: &str,
    icon: Result<&'static str, Glyph>,
    tip: &str,
    f: impl Fn(&mut World) + Send + Sync + 'static,
) {
    let f = std::sync::Arc::new(f);
    let b = Button::new(name.to_string()).ghost().tooltip(tip);
    let mut e = r.spawn((
        b.build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            let f = f.clone();
            commands.queue(move |w: &mut World| f(w));
        }),
    ));
    e.insert(Node {
        width: Val::Px(26.0),
        height: Val::Px(26.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        flex_shrink: 0.0,
        ..default()
    });
    // Every enabled tool in the one ink colour, icon and glyph alike (P3C.7: B and I read
    // greyer than the drawn glyphs, as if disabled).
    let fg = t.foreground;
    match icon {
        Ok(i) => {
            e.with_children(|b| {
                b.spawn((cadrs_ui::icon(i, 16.0, fg), Pickable::IGNORE));
            });
        }
        Err(g) => {
            let th = t.clone();
            e.with_children(|b| glyph(b, &th, g, fg));
        }
    }
}

/// A tool button that is disabled (greyed, its tooltip saying why) when `enabled` is false.
fn tool_if(
    r: &mut ChildSpawnerCommands,
    t: &Theme,
    name: &str,
    icon: Result<&'static str, Glyph>,
    tip: &str,
    enabled: bool,
    f: impl Fn(&mut World) + Send + Sync + 'static,
) {
    if enabled {
        tool(r, t, name, icon, tip, f);
        return;
    }
    let why = if name == "cell-merge" { "select two or more cells" } else { "select a merged cell" };
    let b = Button::new(name.to_string()).ghost().disabled(true).tooltip(format!("{tip}: {why}"));
    let mut e = r.spawn(b.build(t));
    if let Ok(i) = icon {
        let fg = t.disabled_foreground;
        e.with_children(|b| {
            b.spawn((cadrs_ui::icon(i, 16.0, fg), Pickable::IGNORE));
        });
    }
    e.insert(Node {
        width: Val::Px(26.0),
        height: Val::Px(26.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        border_radius: BorderRadius::all(Val::Px(3.0)),
        flex_shrink: 0.0,
        ..default()
    });
    if let Err(g) = icon {
        let fg = t.disabled_foreground;
        let th = t.clone();
        e.with_children(|b| glyph(b, &th, g, fg));
    }
}

fn sep(r: &mut ChildSpawnerCommands, t: &Theme) {
    r.spawn((
        Node {
            width: Val::Px(1.0),
            height: Val::Px(18.0),
            margin: UiRect::horizontal(Val::Px(3.0)),
            ..default()
        },
        BackgroundColor(t.separator),
    ));
}

/// The screen point of a sheet point.
fn screen_of(world: &World, p: [f64; 2]) -> Option<Vec2> {
    let doc = world.get_resource::<ActiveDocument>()?;
    let ui = world.resource::<DrawingUi>();
    let (_, sv) = current_view(doc, ui)?;
    let area = sheet_area(world.resource::<ViewportRect>(), ui);
    Some(sheet_to_screen(sv, area, Vec2::new(p[0] as f32, p[1] as f32)))
}

fn drawing_units(world: &World) -> DrawingUnits {
    world
        .get_resource::<ActiveDocument>()
        .and_then(|d| active_drawing(d).map(|(_, d)| d.units))
        .unwrap_or(DrawingUnits::Millimeter)
}

fn unit_mm(u: DrawingUnits) -> f64 {
    match u {
        DrawingUnits::Inch => 25.4,
        DrawingUnits::Millimeter => 1.0,
    }
}

// ---------------------------------------------------------------------------------------------
// The note toolbar

#[derive(Component, Clone, PartialEq)]
struct NoteBar {
    target: EditTarget,
    at: Vec2,
}

/// Where the note toolbar goes: above the ruler over the text being edited.
fn note_bar_at(world: &World) -> Option<Vec2> {
    let scene = world.resource::<NoteScene>();
    let f = scene.edit_frame?;
    let ui = world.resource::<NotesUi>();
    let h = ui.edit.as_ref()?.note.as_ref().map_or(0.0, |n| n.height);
    let top = f.iter().fold(f64::MIN, |a, p| a.max(p[1]));
    let left = f.iter().fold(f64::MAX, |a, p| a.min(p[0]));
    let p = screen_of(world, [left, top + 2.2 * h])?;
    Some(clear_of_zone_labels(world, (p + Vec2::new(-4.0, -70.0)).round(), NOTE_BAR_SIZE))
}

/// The note and cell toolbars' sizes (px), for keeping them off the zone labels.
const NOTE_BAR_SIZE: Vec2 = Vec2::new(450.0, 64.0);
const CELL_BAR_SIZE: Vec2 = Vec2::new(412.0, 38.0);

/// A toolbar at `at` (its top-left, px) of `size`, moved sideways as little as it takes so it
/// doesn't cover a zone label of the sheet's border (P3C.4's delta: the toolbars covered the
/// "2" and "1" above the sheet).
fn clear_of_zone_labels(world: &World, at: Vec2, size: Vec2) -> Vec2 {
    let labels: Vec<Vec2> = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let ui = world.resource::<DrawingUi>();
        let (id, d) = active_drawing(doc)?;
        let sheet = d.sheets.get(ui.sheet_index(id, d))?;
        if !sheet.border || !sheet.zones {
            return Some(Vec::new());
        }
        let f = cadrs_drawing::standard::frame(sheet.format);
        let bands = [(f.outer.max[1] + f.inner.max[1]) / 2.0, (f.outer.min[1] + f.inner.min[1]) / 2.0];
        let mut out = Vec::new();
        for z in &f.columns {
            for y in bands {
                out.push(screen_of(world, [(z.from + z.to) / 2.0, y])?);
            }
        }
        Some(out)
    })()
    .unwrap_or_default();
    let (hw, hh) = (12.0, 11.0);
    let covers = |x: f32| labels.iter().any(|l| l.x + hw > x && l.x - hw < x + size.x && l.y + hh > at.y && l.y - hh < at.y + size.y);
    if !covers(at.x) {
        return at;
    }
    let right = world.resource::<ViewportRect>().0.max.x;
    let mut best: Option<f32> = None;
    for l in &labels {
        for x in [l.x - hw - 4.0 - size.x, l.x + hw + 4.0] {
            if x >= 4.0 && x + size.x <= right - 4.0 && !covers(x) && best.is_none_or(|b| (x - at.x).abs() < (b - at.x).abs()) {
                best = Some(x);
            }
        }
    }
    Vec2::new(best.unwrap_or(at.x).round(), at.y)
}

fn sync_note_bar(world: &mut World) {
    let kind = *world.resource::<ActiveKind>();
    let want = (kind == ActiveKind::Drawing)
        .then(|| {
            let ui = world.resource::<NotesUi>();
            let e = ui.edit.as_ref().filter(|e| e.toolbar)?;
            Some(NoteBar { target: e.target, at: note_bar_at(world)? })
        })
        .flatten();
    let mut q = world.query::<(Entity, &NoteBar)>();
    let have: Vec<(Entity, NoteBar)> = q.iter(world).map(|(e, b)| (e, b.clone())).collect();
    match (&want, have.as_slice()) {
        (Some(w), [(e, h)]) if w.target == h.target => {
            if w.at != h.at {
                if let Some(mut n) = world.get_mut::<Node>(*e) {
                    n.left = Val::Px(w.at.x.max(4.0));
                    n.top = Val::Px(w.at.y.max(4.0));
                }
                if let Some(mut b) = world.get_mut::<NoteBar>(*e) {
                    b.at = w.at;
                }
            }
            return;
        }
        (None, []) => return,
        _ => {}
    }
    for (e, _) in have {
        world.entity_mut(e).despawn();
    }
    close_property_card(world);
    let Some(w) = want else {
        return;
    };
    let t = world.resource::<Theme>().clone();
    let units = drawing_units(world);
    let height = {
        let ui = world.resource::<NotesUi>();
        let e = ui.edit.as_ref();
        let default_h = e.and_then(|e| e.note.as_ref().map(|n| n.height)).unwrap_or_else(|| {
            let doc = world.get_resource::<ActiveDocument>();
            doc.and_then(|d| active_drawing(d).map(|(_, d)| d.style.table_text_height)).unwrap_or(3.0)
        });
        e.and_then(|e| e.editor.height()).unwrap_or(default_h)
    };
    let title = match w.target {
        EditTarget::Note { .. } => "Note",
        EditTarget::Cell { .. } => "Cell",
    };
    let mut commands = world.commands();
    commands.spawn((card_bundle(&t, "note-bar", w.at), w.clone())).with_children(|c| {
        header(c, &t, title, "note", Some(notes::commit_edit), notes::cancel_edit);
        c.spawn(Node {
            padding: UiRect::all(Val::Px(4.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|r| {
            let toggle = |a: Attr| move |w: &mut World| notes::with_editor(w, |e| e.toggle(a));
            tool(r, &t, "note-bold", Ok("bold"), "Bold (Ctrl+B)", toggle(Attr::Bold));
            tool(r, &t, "note-italic", Ok("italic"), "Italic (Ctrl+I)", toggle(Attr::Italic));
            tool(r, &t, "note-underline", Err(Glyph::Underline), "Underline (Ctrl+U)", toggle(Attr::Underline));
            tool(r, &t, "note-strike", Err(Glyph::Strike), "Strikethrough", toggle(Attr::Strike));
            sep(r, &t);
            let align = |a: HAlign| move |w: &mut World| notes::with_editor(w, |e| e.set_align(a));
            tool(r, &t, "note-align-left", Err(Glyph::AlignLeft), "Align left", align(HAlign::Left));
            tool(r, &t, "note-align-center", Err(Glyph::AlignCenter), "Center", align(HAlign::Center));
            tool(r, &t, "note-align-right", Err(Glyph::AlignRight), "Align right", align(HAlign::Right));
            sep(r, &t);
            let list = |l: ListKind| move |w: &mut World| notes::with_editor(w, |e| e.toggle_list(l));
            tool(r, &t, "note-bullets", Err(Glyph::Bullets), "Bulleted list", list(ListKind::Bullet));
            tool(r, &t, "note-numbered", Err(Glyph::Numbered), "Numbered list", list(ListKind::Numbered));
            sep(r, &t);
            r.spawn(cadrs_ui::icon("text", 14.0, t.muted_foreground));
            r.spawn(
                TextInput::new("note-height")
                    .value(format_len(height / unit_mm(units)))
                    .width(Val::Px(50.0))
                    .height(24.0)
                    .select_all_on_focus()
                    .build(&t),
            )
            .insert(cadrs_ui::Tooltip::new(match units {
                DrawingUnits::Inch => "Text height (in)",
                DrawingUnits::Millimeter => "Text height (mm)",
            }));
            sep(r, &t);
            r.spawn((
                Button::new("note-symbol").label("Ø").dropdown_caret().ghost().tooltip("Insert symbol").build(&t),
                observe(|a: On<Activate>, mut commands: Commands| {
                    let b = a.entity;
                    commands.queue(move |w: &mut World| open_symbol_menu(w, b));
                }),
            ))
            .insert(Node {
                height: Val::Px(26.0),
                padding: UiRect::horizontal(Val::Px(5.0)),
                align_items: AlignItems::Center,
                column_gap: Val::Px(2.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            });
            sep(r, &t);
            tool(r, &t, "note-insert-reference", Ok("part"), "Insert sheet reference property", |w| open_property_card(w, false));
            tool(r, &t, "note-insert-drawing", Ok("file"), "Insert drawing property", |w| open_property_card(w, true));
        });
    });
    world.flush();
}

fn format_len(v: f64) -> String {
    let s = format!("{v:.3}");
    let s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    if s.starts_with("0.") { s[1..].to_string() } else { s }
}

/// Shows the toolbars' toggles (bold, alignment, …) as the text under the caret has them.
fn sync_toggles(ui: Res<NotesUi>, q: Query<(Entity, &Name, Has<cadrs_ui::Selected>)>, mut commands: Commands) {
    let Some(e) = ui.edit.as_ref() else {
        return;
    };
    let ed = &e.editor;
    let para = ed.para_style();
    for (ent, name, selected) in &q {
        let on = match name.as_str() {
            "note-bold" => ed.has(Attr::Bold),
            "note-italic" => ed.has(Attr::Italic),
            "note-underline" => ed.has(Attr::Underline),
            "note-strike" => ed.has(Attr::Strike),
            "note-align-left" => para.align == HAlign::Left,
            "note-align-center" => para.align == HAlign::Center,
            "note-align-right" => para.align == HAlign::Right,
            "note-bullets" => para.list == ListKind::Bullet,
            "note-numbered" => para.list == ListKind::Numbered,
            _ => continue,
        };
        if on && !selected {
            commands.entity(ent).try_insert(cadrs_ui::Selected);
        } else if !on && selected {
            commands.entity(ent).try_remove::<cadrs_ui::Selected>();
        }
    }
}

/// The symbol menu's items: (name, symbol, label).
pub const SYMBOLS: [(&str, char, &str); 12] = [
    ("diameter", 'Ø', "Diameter"),
    ("degree", '°', "Degree"),
    ("plus-minus", '±', "Plus/minus"),
    ("counterbore", '⌴', "Counterbore"),
    ("countersink", '⌵', "Countersink"),
    ("depth", '↧', "Depth"),
    ("times", '×', "Times"),
    ("approx", '≈', "Approximately"),
    ("le", '≤', "Less than or equal"),
    ("ge", '≥', "Greater than or equal"),
    ("delta", 'Δ', "Delta"),
    ("pi", 'π', "Pi"),
];

#[derive(Component)]
struct SymbolMenu;

fn open_symbol_menu(world: &mut World, button: Entity) {
    let theme = world.resource::<Theme>().clone();
    let mut menu = Menu::new("note-symbol-menu").min_width(190.0);
    for (name, c, label) in SYMBOLS {
        menu = menu.item(MenuItem::new(format!("note-symbol-{name}"), format!("{c}    {label}")));
    }
    let mut commands = world.commands();
    let anchor = cadrs_ui::menu::open_menu(&mut commands, button, menu.build(&theme));
    commands.entity(anchor).insert((SymbolMenu, DespawnOnExit(AppState::Document)));
    world.flush();
}

fn on_symbol(ev: On<MenuAction>, q: Query<(), With<SymbolMenu>>, mut commands: Commands) {
    if !q.contains(ev.entity) {
        return;
    }
    let Some(c) = SYMBOLS.iter().find(|(n, _, _)| ev.item == format!("note-symbol-{n}")).map(|s| s.1) else {
        return;
    };
    commands.queue(move |w: &mut World| notes::with_editor(w, |e| e.insert_str(&c.to_string())));
}

// ---------------------------------------------------------------------------------------------
// Property fields (D9.4)

#[derive(Component, Clone, Copy, PartialEq)]
struct PropertyCard {
    drawing: bool,
    prop: usize,
    case: usize,
    /// 0: the drawing's; then Iso, Us, European.
    date: usize,
}

impl PropertyCard {
    fn field(&self) -> Field {
        let prop = if self.drawing {
            Prop::Drawing(DrawingProp::ALL[self.prop.min(DrawingProp::ALL.len() - 1)])
        } else {
            Prop::Reference(RefProp::ALL[self.prop.min(RefProp::ALL.len() - 1)])
        };
        Field {
            prop,
            case: TextCase::ALL[self.case.min(3)],
            date: match self.date {
                1 => Some(DateFormat::Iso),
                2 => Some(DateFormat::Us),
                3 => Some(DateFormat::European),
                _ => None,
            },
        }
    }

    fn is_date(&self) -> bool {
        self.field().prop == Prop::Drawing(DrawingProp::Date)
    }
}

fn close_property_card(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<PropertyCard>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn open_property_card(world: &mut World, drawing: bool) {
    let state = PropertyCard { drawing, prop: 0, case: 0, date: 0 };
    spawn_property_card(world, state);
}

fn spawn_property_card(world: &mut World, s: PropertyCard) {
    close_property_card(world);
    let at = {
        let mut q = world.query::<(&Name, &ComputedNode, &bevy::ui::UiGlobalTransform)>();
        let button = if s.drawing { "note-insert-drawing" } else { "note-insert-reference" };
        q.iter(world).find(|(n, _, _)| n.as_str() == button).map(|(_, n, t)| {
            let k = n.inverse_scale_factor();
            Vec2::new((t.translation.x - n.size().x / 2.0) * k, (t.translation.y + n.size().y / 2.0) * k + 4.0)
        })
    }
    .unwrap_or(Vec2::new(400.0, 200.0));
    let preview = field_sources(world)
        .map(|(r, d)| cadrs_drawing::rich::resolve_field(&s.field(), &cadrs_drawing::rich::FieldContext { reference: &r, drawing: &d }))
        .unwrap_or_default();
    let t = world.resource::<Theme>().clone();
    let title = if s.drawing { "Insert drawing property" } else { "Insert sheet reference property" };
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&t, "note-prop-card", at), s))
        .insert(GlobalZIndex(cadrs_ui::z::DIALOG - 8))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            // Wide enough for the title, ✓ and ✗ (the ✗ stood outside the card).
            width: Val::Px(272.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        })
        .with_children(|c| {
            header(c, &t, title, "note-prop", Some(insert_property), close_property_card);
            c.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|b| {
                let row = |b: &mut ChildSpawnerCommands, label: &str, sel: Select| {
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text(label, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                            Node {
                                width: Val::Px(62.0),
                                ..default()
                            },
                        ));
                        r.spawn(sel.build(&t));
                    });
                };
                let mut props = Select::new("note-prop-property").width(Val::Px(160.0));
                if s.drawing {
                    for p in DrawingProp::ALL {
                        props = props.option(p.label(), true);
                    }
                } else {
                    for p in RefProp::ALL {
                        props = props.option(p.label(), true);
                    }
                }
                row(b, "Property", props.selected(s.prop));
                let mut case = Select::new("note-prop-format").width(Val::Px(160.0));
                for c in TextCase::ALL {
                    case = case.option(c.label(), true);
                }
                row(b, "Format", case.selected(s.case));
                if s.is_date() {
                    let mut date = Select::new("note-prop-date").width(Val::Px(160.0));
                    for o in ["Drawing default", "YYYY-MM-DD", "MM/DD/YYYY", "DD.MM.YYYY"] {
                        date = date.option(o, true);
                    }
                    row(b, "Date", date.selected(s.date));
                }
                // The preview wraps inside the card (P3C.4's delta: a long value ran out of it).
                b.spawn((
                    Name::new("note-prop-preview"),
                    Node {
                        width: Val::Percent(100.0),
                        padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        column_gap: Val::Px(6.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb_u8(0xf4, 0xf6, 0xf9)),
                    BorderColor::all(Color::srgb_u8(0xe0, 0xe4, 0xea)),
                ))
                .with_children(|p| {
                    p.spawn((t.text("Shows", t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { flex_shrink: 0.0, ..default() }));
                    p.spawn((
                        Name::new("note-prop-preview-value"),
                        Text::new(preview.clone()),
                        t.font(t.font_sm, FontWeight::MEDIUM),
                        TextColor(t.foreground),
                        // Wraps (theme texts don't).
                        TextLayout::default(),
                        Node { flex_grow: 1.0, flex_shrink: 1.0, min_width: Val::Px(0.0), ..default() },
                    ));
                });
            });
        });
    world.flush();
}

fn insert_property(world: &mut World) {
    let mut q = world.query::<&PropertyCard>();
    let Some(s) = q.iter(world).next().copied() else {
        return;
    };
    close_property_card(world);
    notes::with_editor(world, |e| e.insert_field(s.field()));
}

fn on_select(ev: On<SelectChange>, q: Query<&Name>, mut commands: Commands) {
    let Ok(name) = q.get(ev.entity) else {
        return;
    };
    let i = ev.index;
    let which = match name.as_str() {
        "note-prop-property" => 0,
        "note-prop-format" => 1,
        "note-prop-date" => 2,
        _ => return,
    };
    commands.queue(move |w: &mut World| {
        let mut q = w.query::<&PropertyCard>();
        let Some(mut s) = q.iter(w).next().copied() else {
            return;
        };
        match which {
            0 => s.prop = i,
            1 => s.case = i,
            _ => s.date = i,
        }
        spawn_property_card(w, s);
    });
}

/// Enter in the text height field.
fn on_submit(ev: On<TextSubmit>, q: Query<(&Name, Option<&ChildOf>)>, q_names: Query<&Name>, mut commands: Commands) {
    let Ok((name, parent)) = q.get(ev.entity) else {
        return;
    };
    let frame = parent.and_then(|p| q_names.get(p.parent()).ok()).map(|n| n.as_str().to_string());
    let name = frame.unwrap_or_else(|| name.as_str().trim_end_matches("-field").to_string());
    if name != "note-height" {
        return;
    }
    let Ok(v) = ev.value.trim().parse::<f64>() else {
        return;
    };
    commands.queue(move |w: &mut World| {
        let mm = v * unit_mm(drawing_units(w));
        if !(0.3..=100.0).contains(&mm) {
            return;
        }
        let mut ui = w.resource_mut::<NotesUi>();
        let Some(e) = ui.edit.as_mut() else {
            return;
        };
        // Selected text takes the height; with none, an empty note its default.
        if e.editor.selection().is_empty()
            && e.editor.is_empty()
            && let Some(n) = e.note.as_mut()
        {
            n.height = mm;
            e.editor.set_height(None);
        } else {
            e.editor.set_height(Some(mm));
        }
        notes::with_editor(w, |_| {});
    });
}

// ---------------------------------------------------------------------------------------------
// The cell toolbar (D10.3)

#[derive(Component, Clone, PartialEq)]
struct CellBar {
    table: TableId,
    at: Vec2,
    /// Merge applies (more than one cell selected) and Unmerge applies (a merged cell in the
    /// selection): otherwise their buttons are disabled.
    merge: bool,
    unmerge: bool,
}

fn sync_cell_bar(world: &mut World) {
    let kind = *world.resource::<ActiveKind>();
    let want = (kind == ActiveKind::Drawing)
        .then(|| {
            let ui = world.resource::<NotesUi>();
            if ui.edit.as_ref().is_some_and(|e| e.toolbar) || ui.drag.is_some_and(|d| d.moving) {
                return None;
            }
            let sel = ui.cells?;
            let scene = world.resource::<NoteScene>();
            let g = scene.items.iter().find_map(|(i, g)| match (i, g) {
                (Item::Table(t), notes::ItemGraphics::Table(g)) if *t == sel.table => Some(g),
                _ => None,
            })?;
            let p = screen_of(world, [g.min[0], g.max[1]])?;
            let (_, t, ((r0, c0), (r1, c1))) = selected_table(world)?;
            let one = t.merge_at(r0, c0).is_some_and(|m| m.contains(r1, c1)) || (r0, c0) == (r1, c1);
            let unmerge = (r0..=r1).any(|r| (c0..=c1).any(|c| t.merge_at(r, c).is_some()));
            let at = clear_of_zone_labels(world, (p + Vec2::new(0.0, -44.0)).round(), CELL_BAR_SIZE);
            Some(CellBar { table: sel.table, at, merge: !one, unmerge })
        })
        .flatten();
    let mut q = world.query::<(Entity, &CellBar)>();
    let have: Vec<(Entity, CellBar)> = q.iter(world).map(|(e, b)| (e, b.clone())).collect();
    if have.len() == 1 && want.as_ref() == Some(&have[0].1) {
        return;
    }
    if want.is_none() && have.is_empty() {
        return;
    }
    for (e, _) in have {
        world.entity_mut(e).despawn();
    }
    let Some(w) = want else {
        return;
    };
    let t = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    commands.spawn((card_bundle(&t, "cell-bar", w.at), w.clone())).with_children(|c| {
        c.spawn(Node {
            padding: UiRect::all(Val::Px(4.0)),
            align_items: AlignItems::Center,
            column_gap: Val::Px(1.0),
            ..default()
        })
        .with_children(|r| {
            tool(r, &t, "cell-row-above", Err(Glyph::RowAbove), "Insert row above", |w| {
                table_op(w, "Insert row", |t, (a, _)| t.insert_row(a.0))
            });
            tool(r, &t, "cell-row-below", Err(Glyph::RowBelow), "Insert row below", |w| {
                table_op(w, "Insert row", |t, (_, b)| t.insert_row(b.0 + t.span(b.0, b.1).0))
            });
            tool(r, &t, "cell-col-left", Err(Glyph::ColumnLeft), "Insert column left", |w| {
                table_op(w, "Insert column", |t, (a, _)| t.insert_col(a.1))
            });
            tool(r, &t, "cell-col-right", Err(Glyph::ColumnRight), "Insert column right", |w| {
                table_op(w, "Insert column", |t, (_, b)| t.insert_col(b.1 + t.span(b.0, b.1).1))
            });
            tool(r, &t, "cell-row-delete", Err(Glyph::DeleteRow), "Delete row", |w| {
                table_op(w, "Delete row", |t, (a, _)| t.remove_row(a.0))
            });
            tool(r, &t, "cell-col-delete", Err(Glyph::DeleteColumn), "Delete column", |w| {
                table_op(w, "Delete column", |t, (a, _)| t.remove_col(a.1))
            });
            sep(r, &t);
            tool_if(r, &t, "cell-merge", Err(Glyph::Merge), "Merge cells", w.merge, |w| table_op(w, "Merge cells", |t, (a, b)| t.merge(a, b)));
            tool_if(r, &t, "cell-unmerge", Err(Glyph::Unmerge), "Unmerge cell", w.unmerge, |w| {
                table_op(w, "Unmerge cell", |t, (a, _)| t.unmerge(a))
            });
            sep(r, &t);
            tool(r, &t, "cell-bold", Ok("bold"), "Bold", |w| format_cells(w, Some(Attr::Bold), None));
            tool(r, &t, "cell-italic", Ok("italic"), "Italic", |w| format_cells(w, Some(Attr::Italic), None));
            tool(r, &t, "cell-underline", Err(Glyph::Underline), "Underline", |w| format_cells(w, Some(Attr::Underline), None));
            sep(r, &t);
            tool(r, &t, "cell-align-left", Err(Glyph::AlignLeft), "Align left", |w| format_cells(w, None, Some(HAlign::Left)));
            tool(r, &t, "cell-align-center", Err(Glyph::AlignCenter), "Center", |w| format_cells(w, None, Some(HAlign::Center)));
            tool(r, &t, "cell-align-right", Err(Glyph::AlignRight), "Align right", |w| format_cells(w, None, Some(HAlign::Right)));
        });
    });
    world.flush();
}

/// The selected table, its sheet and the selected range.
type CellRange = ((usize, usize), (usize, usize));

fn selected_table(world: &World) -> Option<(cadrs_drawing::SheetId, Table, CellRange)> {
    let ui = world.resource::<NotesUi>();
    let sel = ui.cells?;
    let doc = world.get_resource::<ActiveDocument>()?;
    let dui = world.resource::<DrawingUi>();
    let (id, d) = active_drawing(doc)?;
    let index = dui.sheet_index(id, d);
    let t = find_table(d, index, sel.table)?.clone();
    Some((d.sheets[index].id, t, sel.range()))
}

/// A table edit from the cell toolbar.
fn table_op(world: &mut World, label: &str, f: impl FnOnce(&Table, ((usize, usize), (usize, usize))) -> Result<Table, String>) {
    notes::commit_edit(world);
    let Some((sheet, t, range)) = selected_table(world) else {
        return;
    };
    match f(&t, range) {
        Ok(m) => {
            let (r, c) = (m.n_rows(), m.n_cols());
            let m = notes::fit(world, m);
            if edit_drawing(world, DrawingOp::SetTable { sheet, table: m, label: label.into() }) {
                let mut ui = world.resource_mut::<NotesUi>();
                if let Some(s) = ui.cells.as_mut() {
                    // Keep the selection on the table.
                    let clamp = |x: (usize, usize)| (x.0.min(r - 1), x.1.min(c - 1));
                    s.anchor = clamp(s.anchor);
                    s.head = clamp(s.head);
                    if label == "Merge cells" || label == "Unmerge cell" {
                        s.head = s.anchor.min(s.head);
                        s.anchor = s.head;
                    }
                }
            }
        }
        Err(e) => {
            warn!("table edit refused: {e}");
        }
    }
}

/// Bold, italic or underline on or off, or an alignment, for the selected cells.
fn format_cells(world: &mut World, attr: Option<Attr>, align: Option<HAlign>) {
    notes::commit_edit(world);
    let Some((sheet, t, ((r0, c0), (r1, c1)))) = selected_table(world) else {
        return;
    };
    let mut m = t.clone();
    let cells: Vec<(usize, usize)> = (r0..=r1).flat_map(|r| (c0..=c1).map(move |c| (r, c))).filter(|(r, c)| !t.covered(*r, *c)).collect();
    if let Some(a) = attr {
        let all = cells.iter().all(|(r, c)| {
            t.cells[*r][*c].paragraphs.iter().flat_map(|p| &p.spans).all(|s| s.style.get(a))
                && !t.cells[*r][*c].is_empty()
        });
        for (r, c) in &cells {
            m.cells[*r][*c].restyle(|s| s.set(a, !all), |_| {});
        }
    }
    if let Some(al) = align {
        for (r, c) in &cells {
            m.cells[*r][*c].restyle(|_| {}, |p| p.align = al);
        }
    }
    if m != t {
        let m = notes::fit(world, m);
        edit_drawing(world, DrawingOp::SetTable { sheet, table: m, label: "Format cells".into() });
    }
}

// ---------------------------------------------------------------------------------------------
// The Table dialog (D10.1) and Table properties (D10.4)

#[derive(Component)]
struct TableDialog;

/// A fixed-corner picker's choice.
#[derive(Component, Clone, Copy)]
struct CornerChoice(Corner);

/// The fixed-corner buttons: a small grid with the corner marked.
pub(crate) fn corner_buttons(c: &mut ChildSpawnerCommands, t: &Theme, prefix: &str, selected: Corner, on: fn(&mut World, Corner)) {
    c.spawn((t.text("Select fixed corner:", t.font_sm, FontWeight::NORMAL, t.muted_foreground),));
    c.spawn(Node {
        column_gap: Val::Px(8.0),
        margin: UiRect::top(Val::Px(2.0)),
        ..default()
    })
    .with_children(|r| {
        for corner in Corner::ALL {
            let name = format!("{prefix}-{}", corner.label().to_lowercase().replace(' ', "-"));
            let fg = t.foreground;
            let accent = t.primary;
            r.spawn((
                Button::new(name).ghost().selected(corner == selected).tooltip(corner.label()).build(t),
                CornerChoice(corner),
                observe(move |_: On<Activate>, mut commands: Commands| {
                    commands.queue(move |w: &mut World| on(w, corner));
                }),
            ))
            .insert(Node {
                width: Val::Px(30.0),
                height: Val::Px(28.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            })
            .with_children(|b| {
                b.spawn((
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(16.0),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|g| {
                    let bar = |x: f32, y: f32, w: f32, h: f32, c: Color| {
                        (
                            Node {
                                position_type: PositionType::Absolute,
                                left: Val::Px(x),
                                top: Val::Px(y),
                                width: Val::Px(w),
                                height: Val::Px(h),
                                ..default()
                            },
                            BackgroundColor(c),
                            Pickable::IGNORE,
                        )
                    };
                    for k in 0..4 {
                        g.spawn(bar(0.0, k as f32 * 5.0, 18.0, 1.0, fg));
                    }
                    for k in 0..3 {
                        g.spawn(bar(k as f32 * 8.5, 0.0, 1.0, 16.0, fg));
                    }
                    let (x, y) = match corner {
                        Corner::TopLeft => (-2.0, -2.0),
                        Corner::TopRight => (15.0, -2.0),
                        Corner::BottomLeft => (-2.0, 13.0),
                        Corner::BottomRight => (15.0, 13.0),
                    };
                    g.spawn(bar(x, y, 5.0, 5.0, accent));
                });
            });
        }
    });
}

pub(crate) fn refresh_corner_buttons(world: &mut World, prefix: &str, c: Corner) {
    let mut q = world.query::<(Entity, &Name, &CornerChoice)>();
    let items: Vec<(Entity, bool)> = q
        .iter(world)
        .filter(|(_, n, _)| n.as_str().starts_with(prefix))
        .map(|(e, _, k)| (e, k.0 == c))
        .collect();
    for (e, on) in items {
        if on {
            world.entity_mut(e).insert(cadrs_ui::Selected);
        } else {
            world.entity_mut(e).remove::<cadrs_ui::Selected>();
        }
    }
}

/// The Table tool (toolbar): the Table dialog; a click on the sheet then places the table.
pub fn open_table_dialog(world: &mut World) {
    close_table_dialog(world);
    notes::commit_edit(world);
    world.resource_mut::<NotesUi>().clear_selection();
    super::annotations::start_tool(world, AnnTool::Table);
    if world.resource::<AnnotationUi>().tool != AnnTool::Table {
        return;
    }
    let spec = world.resource::<NotesUi>().table_spec;
    let area = {
        let ui = world.resource::<DrawingUi>();
        sheet_area(world.resource::<ViewportRect>(), ui)
    };
    let t = world.resource::<Theme>().clone();
    let at = Vec2::new(area.max.x - 250.0, area.min.y + 14.0);
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&t, "table-dialog", at), TableDialog))
        .insert(Node {
            position_type: PositionType::Absolute,
            left: Val::Px(at.x),
            top: Val::Px(at.y),
            width: Val::Px(210.0),
            flex_direction: FlexDirection::Column,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        })
        .with_children(|c| {
            header(c, &t, "Table", "table-dialog", None, cancel_table_tool);
            c.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|b| {
                for (name, label, v) in [("table-rows", "Rows", spec.rows), ("table-cols", "Columns", spec.cols)] {
                    b.spawn(Node {
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|r| {
                        r.spawn((
                            t.text(label, t.font_sm, FontWeight::NORMAL, t.muted_foreground),
                            Node {
                                width: Val::Px(70.0),
                                ..default()
                            },
                        ));
                        r.spawn(TextInput::new(name).value(v.to_string()).width(Val::Px(60.0)).select_all_on_focus().build(&t));
                    });
                }
                b.spawn(Checkbox::new("table-title-row").label("Title row").checked(spec.title).build(&t));
                b.spawn(Checkbox::new("table-header-row").label("Header row").checked(spec.header).build(&t));
                b.spawn(Checkbox::new("table-revision").label("Revision table").checked(spec.revision).build(&t));
                corner_buttons(b, &t, "table-corner", spec.fixed, |w, c| {
                    w.resource_mut::<NotesUi>().table_spec.fixed = c;
                    refresh_corner_buttons(w, "table-corner", c);
                });
            });
        });
    world.flush();
}

pub fn close_table_dialog(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<TableDialog>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn cancel_table_tool(world: &mut World) {
    close_table_dialog(world);
    if world.resource::<AnnotationUi>().tool == AnnTool::Table {
        world.resource_mut::<AnnotationUi>().tool = AnnTool::None;
    }
}

/// Reads the Table dialog's row and column counts as they are typed; the dialog closes when the
/// tool ends.
fn read_table_dialog(
    ann: Res<AnnotationUi>,
    q_dialog: Query<Entity, With<TableDialog>>,
    q_fields: Query<(&Name, &bevy::text::EditableText)>,
    mut ui: ResMut<NotesUi>,
    mut commands: Commands,
) {
    if q_dialog.is_empty() {
        return;
    }
    if ann.tool != AnnTool::Table {
        for e in &q_dialog {
            commands.entity(e).despawn();
        }
        return;
    }
    for (n, t) in &q_fields {
        let v = t.value().to_string().trim().parse::<usize>().ok().filter(|v| (1..=50).contains(v));
        match (n.as_str(), v) {
            ("table-rows-field", Some(v)) if ui.table_spec.rows != v => ui.table_spec.rows = v,
            ("table-cols-field", Some(v)) if ui.table_spec.cols != v => ui.table_spec.cols = v,
            _ => {}
        }
    }
}

fn on_check(ev: On<CheckboxChange>, q: Query<&Name>, mut ui: ResMut<NotesUi>) {
    match q.get(ev.entity).map(|n| n.as_str()) {
        Ok("table-title-row") => ui.table_spec.title = ev.checked,
        Ok("table-header-row") => ui.table_spec.header = ev.checked,
        Ok("table-revision") => ui.table_spec.revision = ev.checked,
        _ => {}
    }
}

#[derive(Component, Clone, Copy)]
struct TablePropsDialog {
    table: TableId,
    corner: Corner,
}

/// Right-click → Table properties… (D10.4): the fixed corner.
pub fn open_table_properties(world: &mut World, id: TableId) {
    close_table_properties(world);
    let t = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let dui = world.resource::<DrawingUi>();
        let (eid, d) = active_drawing(doc)?;
        find_table(d, dui.sheet_index(eid, d), id).cloned()
    })();
    let Some(table) = t else {
        return;
    };
    let (_, hi) = table.rect();
    let at = screen_of(world, [hi[0], hi[1]]).map(|p| p + Vec2::new(16.0, 0.0)).unwrap_or(Vec2::new(500.0, 200.0));
    let theme = world.resource::<Theme>().clone();
    let mut commands = world.commands();
    commands
        .spawn((card_bundle(&theme, "table-properties", at), TablePropsDialog { table: id, corner: table.fixed }))
        .with_children(|c| {
            header(c, &theme, "Table properties", "table-properties", Some(apply_table_properties), close_table_properties);
            c.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(8.0)),
                row_gap: Val::Px(4.0),
                ..default()
            })
            .with_children(|b| {
                corner_buttons(b, &theme, "table-props-corner", table.fixed, |w, c| {
                    let mut q = w.query::<&mut TablePropsDialog>();
                    for mut d in q.iter_mut(w) {
                        d.corner = c;
                    }
                    refresh_corner_buttons(w, "table-props-corner", c);
                });
            });
        });
    world.flush();
}

fn close_table_properties(world: &mut World) {
    let mut q = world.query_filtered::<Entity, With<TablePropsDialog>>();
    let all: Vec<Entity> = q.iter(world).collect();
    for e in all {
        world.entity_mut(e).despawn();
    }
}

fn apply_table_properties(world: &mut World) {
    let mut q = world.query::<&TablePropsDialog>();
    let Some(d) = q.iter(world).next().copied() else {
        return;
    };
    close_table_properties(world);
    let Some((sheet, t)) = (|| {
        let doc = world.get_resource::<ActiveDocument>()?;
        let dui = world.resource::<DrawingUi>();
        let (eid, dr) = active_drawing(doc)?;
        let i = dui.sheet_index(eid, dr);
        Some((dr.sheets[i].id, find_table(dr, i, d.table)?.clone()))
    })() else {
        return;
    };
    if t.fixed != d.corner {
        edit_drawing(world, DrawingOp::SetTable { sheet, table: t.set_fixed(d.corner), label: "Change fixed corner".into() });
    }
    world.resource_mut::<NotesUi>().selected = vec![Item::Table(d.table)];
}

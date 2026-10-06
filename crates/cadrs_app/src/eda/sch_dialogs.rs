//! The Schematic view's dialogs (GS3, GS7, GS9–GS12): the label's name, a symbol's fields (E),
//! page settings, footprint assignment, ERC, BOM.
//!
//! Names: `eda-label-dialog` (`eda-label-text`, `eda-label-ok`), `eda-props-dialog`
//! (`eda-props-<field>`, `eda-props-ok`), `eda-page-dialog` (`eda-page-paper`, `eda-page-title`,
//! `eda-page-date`, `eda-page-rev`, `eda-page-company`, `eda-page-ok`), `eda-assign-dialog`
//! (`eda-assign-sym-<ref>` rows, `eda-assign-fp-<slug>` candidates, the `eda-assign-by-symbol`,
//! `eda-assign-by-pins` checkboxes, `eda-assign-filter`, `eda-assign-ok`), `eda-erc-dialog`
//! (`eda-erc-run`, `eda-erc-list`, `eda-erc-summary`), `eda-bom-dialog` (`eda-bom-preview`,
//! `eda-bom-export`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_eda::assign::{Filters, candidates};
use cadrs_eda::sch_edit as se;
use cadrs_eda::symbol::fields;
use cadrs_ui::checkbox::CheckboxState;
use cadrs_ui::dialog_fields::{Select, SelectState};
use cadrs_ui::prelude::*;
use cadrs_ui::{Checkbox, Dialog};
use uuid::Uuid;

use super::schematic_tools::{Tool, set_tool};
use super::ui;
use crate::AppState;

#[derive(Component)]
pub struct EdaDialog;

fn close_all(w: &mut World) {
    let mut q = w.query_filtered::<Entity, With<EdaDialog>>();
    let es: Vec<Entity> = q.iter(w).collect();
    for e in es {
        w.entity_mut(e).despawn();
    }
}

/// A labelled row: the label, then `f` spawns the field.
fn row(p: &mut ChildSpawner, t: &Theme, label: &str, f: impl FnOnce(&mut ChildSpawner)) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), height: Val::Px(34.0), ..default() }).with_children(|r| {
        r.spawn((t.text(label, t.font_base, FontWeight::MEDIUM, t.muted_foreground), Node { width: Val::Px(110.0), ..default() }));
        f(r);
    });
}

fn text_row(p: &mut ChildSpawner, t: &Theme, label: &str, name: &'static str, value: &str) {
    let value = value.to_string();
    row(p, t, label, |r| {
        r.spawn(TextInput::new(name).value(value).width(Val::Px(300.0)).height(28.0).build(t));
    });
}

/// OK / Cancel; OK runs `ok`.
fn ok_cancel(f: &mut ChildSpawner, t: &Theme, ok_name: &'static str, ok: fn(&mut World)) {
    f.spawn((
        cadrs_ui::Button::new(ok_name).label("OK").primary().build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(ok);
        }),
    ));
    f.spawn((
        cadrs_ui::Button::new(format!("{ok_name}-cancel")).label("Cancel").build(t),
        observe(|_: On<Activate>, mut commands: Commands| {
            commands.queue(close_all);
        }),
    ));
}

fn spawn_dialog(w: &mut World, d: Dialog) {
    let theme = w.resource::<Theme>().clone();
    w.spawn((d.build(&theme), EdaDialog, DespawnOnExit(AppState::Document)));
}

// ---------------------------------------------------------------------------------------------
// Label

pub fn open_label(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-label-dialog")
            .title("Net label")
            .width(440.0)
            .body(move |b| text_row(b, &t, "Label", "eda-label-text", ""))
            .footer(move |f| ok_cancel(f, &tf, "eda-label-ok", accept_label)),
    );
}

fn accept_label(w: &mut World) {
    let text = ui::text_value(w, "eda-label-text").trim().to_string();
    close_all(w);
    if !text.is_empty() {
        set_tool(w, Tool::Label(text));
    }
}


/// Ctrl+L: a global label's name (it joins every sheet's net of that name).
pub fn open_global_label(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-glabel-dialog")
            .title("Global label")
            .width(440.0)
            .body(move |b| text_row(b, &t, "Label", "eda-glabel-text", ""))
            .footer(move |f| ok_cancel(f, &tf, "eda-glabel-ok", accept_global_label)),
    );
}

fn accept_global_label(w: &mut World) {
    let text = ui::text_value(w, "eda-glabel-text").trim().to_string();
    close_all(w);
    if !text.is_empty() {
        set_tool(w, Tool::GlobalLabel(text));
    }
}

/// T: a text note's words.
pub fn open_text(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-text-dialog")
            .title("Text")
            .width(440.0)
            .body(move |b| text_row(b, &t, "Text", "eda-text-text", ""))
            .footer(move |f| ok_cancel(f, &tf, "eda-text-ok", accept_text)),
    );
}

fn accept_text(w: &mut World) {
    let text = ui::text_value(w, "eda-text-text").trim().to_string();
    close_all(w);
    if !text.is_empty() {
        set_tool(w, Tool::Text(text));
    }
}

/// Ctrl+F: a symbol by its reference or value; found, it is selected and centred.
pub fn open_find(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-find-dialog")
            .title("Find")
            .width(440.0)
            .body(move |b| text_row(b, &t, "Reference or value", "eda-find-text", ""))
            .footer(move |f| ok_cancel(f, &tf, "eda-find-ok", accept_find)),
    );
}

fn accept_find(w: &mut World) {
    let text = ui::text_value(w, "eda-find-text").trim().to_lowercase();
    close_all(w);
    let Some((_, _, d)) = ui::current(w) else { return };
    let sh = &d.schematic.sheets[0];
    let hit = sh
        .symbols
        .iter()
        .find(|s| s.reference().to_lowercase() == text)
        .or_else(|| sh.symbols.iter().find(|s| s.value().to_lowercase().contains(&text)))
        .map(|s| (s.id, s.placement.at));
    match hit {
        Some((id, at)) => {
            w.resource_mut::<super::schematic_tools::SchState>().selection = vec![se::SchItem::Symbol(id)];
            let key = w.resource::<super::Eda2d>().0;
            if let Some(k) = key
                && let Some(v) = w.resource_mut::<super::EdaUi>().views.get_mut(&k)
            {
                v.0.center = [at.x as f64, at.y as f64];
                v.1 = false;
            }
        }
        None => ui::toast(w, &format!("Nothing matches \u{201c}{text}\u{201d}")),
    }
}
// ---------------------------------------------------------------------------------------------
// Symbol properties (E)

#[derive(Resource, Default)]
struct PropsTarget(Option<Uuid>);

const PROP_FIELDS: [(&str, &str); 4] = [(fields::REFERENCE, "eda-props-reference"), (fields::VALUE, "eda-props-value"), (fields::FOOTPRINT, "eda-props-footprint"), (fields::DATASHEET, "eda-props-datasheet")];

pub fn open_properties(w: &mut World, symbol: Uuid) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let Some(s) = d.schematic.sheets.iter().flat_map(|x| &x.symbols).find(|x| x.id == symbol).cloned() else { return };
    w.insert_resource(PropsTarget(Some(symbol)));
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    let title = format!("Symbol properties: {}", s.reference());
    spawn_dialog(
        w,
        Dialog::new("eda-props-dialog")
            .title(title)
            .width(480.0)
            .body(move |b| {
                for (name, field) in PROP_FIELDS {
                    text_row(b, &t, name, field, s.field(name).map_or("", |f| f.value()));
                }
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-props-ok", accept_properties)),
    );
}

fn accept_properties(w: &mut World) {
    let Some(id) = w.get_resource::<PropsTarget>().and_then(|t| t.0) else { return };
    let values: Vec<(&str, String)> = PROP_FIELDS.iter().map(|(n, f)| (*n, ui::text_value(w, f))).collect();
    close_all(w);
    ui::commit(w, "Edit symbol properties", |d| {
        for (name, v) in &values {
            let cur = d.schematic.sheets.iter().flat_map(|s| &s.symbols).find(|s| s.id == id).and_then(|s| s.field(name)).map(|f| f.value().to_string());
            if cur.as_deref() != Some(v.as_str()) && !(cur.is_none() && v.is_empty()) {
                se::set_field(&mut d.schematic, id, name, v);
            }
        }
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Page settings (GS3)

const PAPERS: [&str; 12] = ["A5", "A4", "A3", "A2", "A1", "A0", "A", "B", "C", "D", "E", "USLegal"];

pub fn open_page(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let sh = d.schematic.sheets[0].clone();
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-page-dialog")
            .title("Page settings")
            .width(480.0)
            .body(move |b| {
                let selected = PAPERS.iter().position(|p| *p == sh.paper.name).unwrap_or(1);
                row(b, &t, "Paper size", |r| {
                    let mut s = Select::new("eda-page-paper").bordered().width(Val::Px(160.0));
                    for p in PAPERS {
                        s = s.option(p, true);
                    }
                    r.spawn(s.selected(selected).build(&t));
                });
                let tb = &sh.title_block;
                text_row(b, &t, "Title", "eda-page-title", &tb.title);
                text_row(b, &t, "Issue date", "eda-page-date", &tb.date);
                text_row(b, &t, "Revision", "eda-page-rev", &tb.revision);
                text_row(b, &t, "Company", "eda-page-company", &tb.company);
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-page-ok", accept_page)),
    );
}

fn select_index(w: &mut World, name: &str) -> Option<usize> {
    let mut q = w.query::<(&Name, &SelectState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected)
}

fn accept_page(w: &mut World) {
    let paper = select_index(w, "eda-page-paper").and_then(|i| PAPERS.get(i)).and_then(|p| se::paper(p));
    let tb = cadrs_eda::schematic::TitleBlock {
        title: ui::text_value(w, "eda-page-title"),
        date: ui::text_value(w, "eda-page-date"),
        revision: ui::text_value(w, "eda-page-rev"),
        company: ui::text_value(w, "eda-page-company"),
        comments: vec![],
    };
    close_all(w);
    ui::commit(w, "Page settings", |d| {
        let p = paper.unwrap_or_else(|| d.schematic.sheets[0].paper.clone());
        let comments = d.schematic.sheets[0].title_block.comments.clone();
        se::set_page(&mut d.schematic, 0, p, cadrs_eda::schematic::TitleBlock { comments, ..tb });
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Assign footprints (GS10)

#[derive(Resource, Default, Clone, PartialEq)]
pub struct AssignState {
    pub symbol: Option<Uuid>,
    shown: Option<(Option<Uuid>, bool, bool, String, u64)>,
}

#[derive(Component)]
struct AssignSymbols;

#[derive(Component)]
struct AssignCandidates;

#[derive(Component, Clone, Copy)]
struct AssignSymbolRow(Uuid);

#[derive(Component, Clone)]
struct AssignFootprintRow(String);

pub fn open_assign(w: &mut World) {
    w.insert_resource(AssignState::default());
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-assign-dialog")
            .title("Assign footprints")
            .width(860.0)
            .body(move |b| {
                b.spawn(Node { column_gap: Val::Px(14.0), align_items: AlignItems::Center, ..default() }).with_children(|r| {
                    r.spawn(Checkbox::new("eda-assign-by-symbol").label("Symbol's filters").checked(true).build(&t));
                    r.spawn(Checkbox::new("eda-assign-by-pins").label("Pin count").checked(true).build(&t));
                    r.spawn(TextInput::new("eda-assign-filter").placeholder("Filter footprints").width(Val::Px(260.0)).height(28.0).build(&t));
                });
                b.spawn(Node { column_gap: Val::Px(10.0), height: Val::Px(360.0), margin: UiRect::top(Val::Px(8.0)), ..default() }).with_children(|r| {
                    r.spawn((Name::new("eda-assign-symbols"), AssignSymbols, Node { width: Val::Px(330.0), flex_shrink: 0.0, flex_direction: FlexDirection::Column, overflow: Overflow { x: OverflowAxis::Clip, y: OverflowAxis::Scroll }, ..default() }));
                    r.spawn((Name::new("eda-assign-candidates"), AssignCandidates, Node { flex_grow: 1.0, flex_direction: FlexDirection::Column, overflow: Overflow { x: OverflowAxis::Clip, y: OverflowAxis::Scroll }, ..default() }));
                });
            })
            .footer(move |f| {
                f.spawn((
                    cadrs_ui::Button::new("eda-assign-ok").label("Close").primary().build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_all);
                    }),
                ));
            }),
    );
}

fn checkbox(w: &mut World, name: &str) -> bool {
    let mut q = w.query::<(&Name, &CheckboxState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).is_some_and(|(_, s)| s.checked)
}

/// Rebuilds both lists when the selection, the filters or the design change.
pub fn refresh_assign(w: &mut World) {
    let mut q = w.query_filtered::<Entity, With<AssignSymbols>>();
    let Some(syms) = q.iter(w).next() else { return };
    let mut q2 = w.query_filtered::<Entity, With<AssignCandidates>>();
    let Some(cands) = q2.iter(w).next() else { return };
    let by_symbol = checkbox(w, "eda-assign-by-symbol");
    let by_pins = checkbox(w, "eda-assign-by-pins");
    let text = ui::text_value(w, "eda-assign-filter");
    let undo = w.resource::<crate::ActiveDocument>().history.undo_len() as u64;
    let state = w.resource::<AssignState>().clone();
    let key = (state.symbol, by_symbol, by_pins, text.clone(), undo);
    if state.shown.as_ref() == Some(&key) {
        return;
    }
    w.resource_mut::<AssignState>().shown = Some(key);
    let Some((_, _, d)) = ui::current(w) else { return };
    let lib = ui::libraries(w);
    let theme = w.resource::<Theme>().clone();
    let mut parts: Vec<_> = d.schematic.sheets.iter().flat_map(|s| &s.symbols).filter(|s| !s.reference().starts_with('#')).cloned().collect();
    parts.sort_by_key(|s| cadrs_eda::connectivity::natural(s.reference()));
    let chosen = state.symbol.or(parts.first().map(|s| s.id));
    let found: Vec<(String, String)> = chosen
        .and_then(|id| parts.iter().find(|s| s.id == id))
        .map(|s| {
            candidates(&lib, &d.schematic, s, &Filters { symbol_filters: by_symbol, pin_count: by_pins, library: None, text: text.clone() })
                .into_iter()
                .take(200)
                .map(|f| (f.id.clone(), f.description.clone()))
                .collect()
        })
        .unwrap_or_default();
    let mut commands = w.commands();
    commands.entity(syms).despawn_children();
    commands.entity(syms).with_children(|l| {
        for s in &parts {
            let label = format!("{} – {} : {}", s.reference(), s.value(), if s.footprint().is_empty() { "—" } else { s.footprint() });
            l.spawn((ListItem::new(format!("eda-assign-sym-{}", s.reference())).label(label).height(24.0).selected(Some(s.id) == chosen).build(&theme), AssignSymbolRow(s.id)));
        }
    });
    commands.entity(cands).despawn_children();
    commands.entity(cands).with_children(|l| {
        for (id, desc) in found {
            l.spawn((
                ListItem::new(format!("eda-assign-fp-{}", crate::pcb::slug(&id))).label(id.clone()).detail(desc).height(24.0).build(&theme),
                AssignFootprintRow(id),
                cadrs_ui::DoubleClickable,
            ));
        }
    });
    w.flush();
}

fn on_assign_row(a: On<Activate>, q_sym: Query<&AssignSymbolRow>, mut state: Option<ResMut<AssignState>>) {
    if let (Ok(r), Some(state)) = (q_sym.get(a.entity), state.as_mut()) {
        state.symbol = Some(r.0);
    }
}

/// Double-click (or click) a footprint: it is assigned to the chosen symbol.
fn on_assign_footprint(a: On<cadrs_ui::DoubleClick>, q: Query<&AssignFootprintRow>, mut commands: Commands) {
    let Ok(r) = q.get(a.entity) else { return };
    let fp = r.0.clone();
    commands.queue(move |w: &mut World| {
        let Some((_, _, d)) = ui::current(w) else { return };
        let mut parts: Vec<_> = d.schematic.sheets.iter().flat_map(|s| &s.symbols).filter(|s| !s.reference().starts_with('#')).cloned().collect();
        parts.sort_by_key(|s| cadrs_eda::connectivity::natural(s.reference()));
        let chosen = w.resource::<AssignState>().symbol.or(parts.first().map(|s| s.id));
        let Some(id) = chosen else { return };
        let Some(r) = parts.iter().find(|s| s.id == id).map(|s| s.reference().to_string()) else { return };
        ui::commit(w, "Assign footprint", |d| {
            cadrs_eda::assign::assign(&mut d.schematic, &r, &fp);
            Ok(())
        });
        // On to the next symbol without a footprint.
        let next = ui::current(w).and_then(|(_, _, d)| {
            let mut v: Vec<_> = d.schematic.sheets.iter().flat_map(|s| &s.symbols).filter(|s| !s.reference().starts_with('#') && s.footprint().is_empty()).cloned().collect();
            v.sort_by_key(|s| cadrs_eda::connectivity::natural(s.reference()));
            v.first().map(|s| s.id)
        });
        if next.is_some() {
            w.resource_mut::<AssignState>().symbol = next;
        }
    });
}

// ---------------------------------------------------------------------------------------------
// ERC (GS11)

#[derive(Component)]
struct ErcList;

pub fn open_erc(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-erc-dialog")
            .title("Electrical rules checker")
            .width(620.0)
            .body(move |b| {
                b.spawn((Name::new("eda-erc-summary"), t.text("", t.font_base, FontWeight::MEDIUM, t.foreground)));
                b.spawn((Name::new("eda-erc-list"), ErcList, Node { flex_direction: FlexDirection::Column, height: Val::Px(300.0), overflow: Overflow::scroll_y(), margin: UiRect::top(Val::Px(8.0)), ..default() }));
            })
            .footer(move |f| {
                f.spawn((
                    cadrs_ui::Button::new("eda-erc-run").label("Run ERC").primary().build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(run_erc);
                    }),
                ));
                f.spawn((
                    cadrs_ui::Button::new("eda-erc-close").label("Close").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_all);
                    }),
                ));
            }),
    );
    run_erc(w);
}

fn run_erc(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let vs = cadrs_eda::erc::check(&d.schematic);
    let errors = vs.iter().filter(|v| v.severity() == cadrs_eda::erc::Severity::Error).count();
    let warnings = vs.len() - errors;
    let summary = if vs.is_empty() { "No violations".to_string() } else { format!("Violations ({}): {errors} errors, {warnings} warnings", vs.len()) };
    ui::set_label(w, "eda-erc-summary", &summary);
    let mut q = w.query_filtered::<Entity, With<ErcList>>();
    let Some(list) = q.iter(w).next() else { return };
    let t = w.resource::<Theme>().clone();
    let mut commands = w.commands();
    commands.entity(list).despawn_children();
    commands.entity(list).with_children(|l| {
        for (i, v) in vs.iter().enumerate() {
            let sev = if v.severity() == cadrs_eda::erc::Severity::Error { "Error" } else { "Warning" };
            l.spawn((Name::new(format!("eda-erc-{i}")), t.text(format!("{sev}: {}", v.rule.message()), t.font_base, FontWeight::SEMIBOLD, t.foreground)));
            for item in &v.items {
                l.spawn((t.text(format!("    {item}"), t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::left(Val::Px(14.0)), ..default() }));
            }
        }
    });
    w.flush();
}

// ---------------------------------------------------------------------------------------------
// BOM (GS12)

pub fn open_bom(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let rows = cadrs_eda::bom::rows(&d.schematic);
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-bom-dialog")
            .title("Bill of materials")
            .width(760.0)
            .body(move |b| {
                b.spawn((Name::new("eda-bom-preview"), Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), ..default() })).with_children(|l| {
                    let head = ["Reference", "Value", "Footprint", "Qty", "DNP"];
                    let widths = [120.0, 90.0, 430.0, 40.0, 40.0];
                    let line = |l: &mut ChildSpawner, cells: [String; 5], bold: bool| {
                        l.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                            for (c, w) in cells.into_iter().zip(widths) {
                                r.spawn((t.text(c, t.font_sm, if bold { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, t.foreground), Node { width: Val::Px(w), ..default() }));
                            }
                        });
                    };
                    line(l, head.map(String::from), true);
                    for r in &rows {
                        line(l, [r.references.join(", "), r.value.clone(), r.footprint.clone(), r.qty().to_string(), if r.dnp { "DNP".into() } else { String::new() }], false);
                    }
                });
            })
            .footer(move |f| {
                f.spawn((
                    cadrs_ui::Button::new("eda-bom-export").label("Export…").primary().build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(|w: &mut World| {
                            let theme = w.resource::<Theme>().clone();
                            let dir = std::env::current_dir().unwrap_or_default();
                            let mut c = w.commands();
                            cadrs_ui::file_picker::open_folder_picker(&mut c, &theme, "eda-bom-folder", "Export the BOM to", "eda-bom", dir);
                            w.flush();
                        });
                    }),
                ));
                f.spawn((
                    cadrs_ui::Button::new("eda-bom-close").label("Close").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_all);
                    }),
                ));
            }),
    );
}

/// Writes the BOM CSV into `dir` as `<board>.csv`; returns the path.
pub fn export_bom(w: &mut World, dir: &std::path::Path) -> Option<std::path::PathBuf> {
    let (el, b, d) = ui::current(w)?;
    let name = w.resource::<crate::ActiveDocument>().doc.element(el)?.pcb()?.board(b)?.name().to_string();
    let path = dir.join(format!("{}.csv", cadrs_idf::safe_file_name(&name)));
    match std::fs::write(&path, cadrs_eda::bom::csv(&d.schematic)) {
        Ok(()) => {
            ui::toast(w, &format!("Wrote {}", path.display()));
            Some(path)
        }
        Err(e) => {
            ui::toast(w, &format!("Couldn't write {}: {e}", path.display()));
            None
        }
    }
}

pub fn on_folder_picked(mut msgs: MessageReader<cadrs_ui::file_picker::FilePicked>, mut commands: Commands) {
    for m in msgs.read() {
        if m.tag == "eda-bom" {
            let p = m.path.clone();
            commands.queue(move |w: &mut World| {
                export_bom(w, &p);
            });
        }
    }
}

pub fn register(app: &mut App) {
    app.add_systems(Update, (refresh_assign, on_folder_picked).run_if(in_state(AppState::Document)))
        .add_observer(on_assign_row)
        .add_observer(on_assign_footprint);
}


// ---------------------------------------------------------------------------------------------
// Plot: the sheet as SVG or PDF, the netlist as CSV

/// The sheet as a vector PDF page at its paper size (lines and fills; text as strokes).
pub fn schematic_pdf(d: &cadrs_eda::Design, title: &str) -> Vec<u8> {
    use cadrs_drawing::export::{Item, Layer, Page, Pen, Shape};
    use cadrs_eda::units::to_mm;
    let list = cadrs_eda::render::schematic(&d.schematic, 0, &cadrs_eda::render::SchematicTheme::default(), &cadrs_eda::render::Highlight::default());
    let paper = &d.schematic.sheets[0].paper;
    let mut page = Page { name: title.into(), width: to_mm(paper.size.w), height: to_mm(paper.size.h), items: vec![] };
    let mm2 = |p: &cadrs_eda::units::Pt| [to_mm(p.x), to_mm(p.y)];
    for a in &list.areas {
        // The paper's own colour stays the paper's.
        if a.z == -10 {
            continue;
        }
        for t in a.tris.chunks_exact(3) {
            let points = t.iter().map(|q| [q[0] / 1e6, q[1] / 1e6]).collect();
            page.items.push(Item::Fill { points, color: [a.color[0], a.color[1], a.color[2]], layer: Layer::Visible });
        }
    }
    for l in &list.lines {
        let mut pen = Pen::new(to_mm(l.width).max(0.15), Layer::Visible);
        pen.color = [l.color[0], l.color[1], l.color[2]];
        page.items.push(Item::Stroke(Shape::Polyline { points: l.pts.iter().map(mm2).collect(), closed: false }, pen));
    }
    cadrs_drawing::pdf::write_pdf(&[page], &cadrs_drawing::pdf::PdfOptions::default())
}

pub fn open_plot(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-splot-dialog")
            .title("Plot schematic")
            .width(560.0)
            .body(move |p| {
                text_row(p, &t, "Output directory", "eda-splot-dir", "plots/");
                p.spawn((Name::new("eda-splot-log"), t.text("", t.font_sm, FontWeight::NORMAL, t.muted_foreground), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
            })
            .footer(move |f| {
                for (name, label, kind) in [("eda-splot-svg", "SVG", "svg"), ("eda-splot-pdf", "PDF", "pdf"), ("eda-splot-netlist", "Netlist (CSV)", "net")] {
                    f.spawn((
                        cadrs_ui::Button::new(name).label(label).build(&tf),
                        observe(move |_: On<Activate>, mut commands: Commands| {
                            commands.queue(move |w: &mut World| run_plot(w, kind));
                        }),
                    ));
                }
                f.spawn((
                    cadrs_ui::Button::new("eda-splot-close").label("Close").build(&tf),
                    observe(|_: On<Activate>, mut commands: Commands| {
                        commands.queue(close_all);
                    }),
                ));
            }),
    );
}

/// Writes the sheet as SVG or PDF, or the netlist, into `dir`; returns the file written.
pub fn plot_schematic_to(w: &mut World, dir: &std::path::Path, kind: &str) -> Result<std::path::PathBuf, String> {
    let (el, b, d) = ui::current(w).ok_or("No board")?;
    let name = w.resource::<crate::ActiveDocument>().doc.element(el).and_then(|e| e.pcb()).and_then(|s| s.board(b)).map(|x| cadrs_idf::safe_file_name(x.name())).unwrap_or_else(|| "schematic".into());
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (file, bytes): (String, Vec<u8>) = match kind {
        "svg" => (format!("{name}.svg"), cadrs_eda::render::to_svg(&cadrs_eda::render::schematic(&d.schematic, 0, &cadrs_eda::render::SchematicTheme::default(), &cadrs_eda::render::Highlight::default())).into_bytes()),
        "pdf" => (format!("{name}.pdf"), schematic_pdf(&d, &name)),
        _ => (format!("{name}-netlist.csv"), cadrs_eda::connectivity::netlist_csv(&d.schematic).into_bytes()),
    };
    let path = dir.join(&file);
    std::fs::write(&path, bytes).map_err(|e| format!("{file}: {e}"))?;
    Ok(path)
}

fn run_plot(w: &mut World, kind: &str) {
    let dir = ui::text_value(w, "eda-splot-dir");
    let dir = std::path::PathBuf::from(if dir.trim().is_empty() { "plots/".into() } else { dir });
    let msg = match plot_schematic_to(w, &dir, kind) {
        Ok(p) => format!("Wrote {}", p.display()),
        Err(e) => format!("Error: {e}"),
    };
    ui::set_label(w, "eda-splot-log", &msg);
}

// ---------------------------------------------------------------------------------------------
// Symbol fields table: every placed symbol's reference, value, footprint, LCSC part and DNP

#[derive(Resource, Default)]
struct FieldsTable(Vec<Uuid>);

pub fn open_fields_table(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let mut rows: Vec<(Uuid, String, String, String, String, bool)> = d
        .schematic
        .sheets
        .iter()
        .flat_map(|s| &s.symbols)
        .filter(|s| !s.reference().starts_with('#'))
        .map(|s| {
            let lcsc = ["LCSC", "LCSC Part", "LCSC Part #"].iter().find_map(|n| s.field(n).map(|f| f.value().to_string())).unwrap_or_default();
            (s.id, s.reference().to_string(), s.value().to_string(), s.footprint().to_string(), lcsc, s.dnp)
        })
        .collect();
    rows.sort_by_key(|r| cadrs_eda::connectivity::natural(&r.1));
    w.insert_resource(FieldsTable(rows.iter().map(|r| r.0).collect()));
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-fields-table")
            .title("Symbol fields")
            .width(900.0)
            .body(move |p| {
                p.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|h| {
                    for (label, wd) in [("Reference", 80.0), ("Value", 170.0), ("Footprint", 380.0), ("LCSC part", 110.0), ("DNP", 40.0)] {
                        h.spawn((t.text(label, t.font_sm, FontWeight::MEDIUM, t.muted_foreground), Node { width: Val::Px(wd), ..default() }));
                    }
                });
                p.spawn(Node { flex_direction: FlexDirection::Column, row_gap: Val::Px(2.0), max_height: Val::Px(420.0), overflow: Overflow::scroll_y(), ..default() }).with_children(|l| {
                    for (i, (_, r, v, fp, lcsc, dnp)) in rows.into_iter().enumerate() {
                        l.spawn(Node { column_gap: Val::Px(6.0), align_items: AlignItems::Center, ..default() }).with_children(|row| {
                            row.spawn(TextInput::new(format!("eda-fields-{i}-reference")).value(r).width(Val::Px(80.0)).height(24.0).build(&t));
                            row.spawn(TextInput::new(format!("eda-fields-{i}-value")).value(v).width(Val::Px(170.0)).height(24.0).build(&t));
                            row.spawn(TextInput::new(format!("eda-fields-{i}-footprint")).value(fp).width(Val::Px(380.0)).height(24.0).build(&t));
                            row.spawn(TextInput::new(format!("eda-fields-{i}-lcsc")).value(lcsc).width(Val::Px(110.0)).height(24.0).build(&t));
                            row.spawn(Checkbox::new(format!("eda-fields-{i}-dnp")).checked(dnp).build(&t));
                        });
                    }
                });
            })
            .footer(move |f| ok_cancel(f, &tf, "eda-fields-ok", accept_fields_table)),
    );
}

fn accept_fields_table(w: &mut World) {
    let ids = w.get_resource::<FieldsTable>().map(|t| t.0.clone()).unwrap_or_default();
    let rows: Vec<(Uuid, String, String, String, String, bool)> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let field = |w: &mut World, n: &str| ui::text_value(w, &format!("eda-fields-{i}-{n}")).trim().to_string();
            let dnp = {
                let name = format!("eda-fields-{i}-dnp");
                let mut q = w.query::<(&Name, &CheckboxState)>();
                q.iter(w).find(|(n, _)| n.as_str() == name).is_some_and(|(_, s)| s.checked)
            };
            (*id, field(w, "reference"), field(w, "value"), field(w, "footprint"), field(w, "lcsc"), dnp)
        })
        .collect();
    close_all(w);
    w.remove_resource::<FieldsTable>();
    ui::commit(w, "Edit symbol fields", |d| {
        for (id, r, v, fp, lcsc, dnp) in &rows {
            let s = &mut d.schematic;
            se::set_field(s, *id, fields::REFERENCE, r);
            se::set_field(s, *id, fields::VALUE, v);
            se::set_field(s, *id, fields::FOOTPRINT, fp);
            let has = s.sheets.iter().flat_map(|x| &x.symbols).find(|x| x.id == *id).and_then(|x| ["LCSC", "LCSC Part", "LCSC Part #"].into_iter().find(|n| x.field(n).is_some()));
            if !lcsc.is_empty() || has.is_some() {
                se::set_field(s, *id, has.unwrap_or("LCSC Part"), lcsc);
            }
            if let Some(x) = s.sheets.iter_mut().flat_map(|x| &mut x.symbols).find(|x| x.id == *id) {
                x.dnp = *dnp;
            }
        }
        Ok(())
    });
}

//! The Layout view's dialogs (GS13, GS14, GS18, GS19, GS21): Update PCB from schematic, Board
//! setup, zone properties, DRC, Plot.
//!
//! Names: `eda-update-dialog` (`eda-update-replace`, `eda-update-delete`, `eda-update-run`,
//! `eda-update-log`), `eda-setup-dialog` (`eda-setup-copper`, `eda-setup-<rule>`,
//! `eda-setup-track`, `eda-setup-clearance`, `eda-setup-via`, `eda-setup-via-drill`,
//! `eda-setup-ok`), `eda-zone-dialog` (`eda-zone-net`, `eda-zone-layer`, `eda-zone-ok`),
//! `eda-drc-dialog` (`eda-drc-refill`, `eda-drc-run`, `eda-drc-summary`, `eda-drc-list`),
//! `eda-plot-dialog` (`eda-plot-dir`, `eda-plot-run`, `eda-plot-drill`, `eda-plot-log`).

use bevy::prelude::*;
use bevy::text::FontWeight;
use bevy::ui_widgets::{Activate, observe};
use cadrs_eda::expr::{Unit, eval};
use cadrs_eda::layer::Layer;
use cadrs_eda::units::{Nm, to_mm};
use cadrs_ui::checkbox::CheckboxState;
use cadrs_ui::dialog_fields::{Select, SelectState};
use cadrs_ui::prelude::*;
use cadrs_ui::{Checkbox, Dialog};

use super::layout_tools::{LayoutState, Tool, set_tool};
use super::ui;
use crate::AppState;

#[derive(Component)]
pub struct LayoutDialog;

fn close_all(w: &mut World) {
    let mut q = w.query_filtered::<Entity, With<LayoutDialog>>();
    let es: Vec<Entity> = q.iter(w).collect();
    for e in es {
        w.entity_mut(e).despawn();
    }
}

fn spawn_dialog(w: &mut World, d: Dialog) {
    let theme = w.resource::<Theme>().clone();
    w.spawn((d.build(&theme), LayoutDialog, DespawnOnExit(AppState::Document)));
}

fn row(p: &mut ChildSpawner, t: &Theme, label: &str, f: impl FnOnce(&mut ChildSpawner)) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), height: Val::Px(32.0), ..default() }).with_children(|r| {
        r.spawn((t.text(label, t.font_base, FontWeight::MEDIUM, t.muted_foreground), Node { width: Val::Px(190.0), ..default() }));
        f(r);
    });
}

fn mm_row(p: &mut ChildSpawner, t: &Theme, label: &str, name: &'static str, v: Nm) {
    row(p, t, label, |r| {
        r.spawn(TextInput::new(name).value(format!("{}", to_mm(v))).width(Val::Px(120.0)).height(26.0).build(t));
        r.spawn(t.text("mm", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
    });
}

fn button(f: &mut ChildSpawner, t: &Theme, name: &'static str, label: &str, primary: bool, run: fn(&mut World)) {
    let mut b = cadrs_ui::Button::new(name).label(label);
    if primary {
        b = b.primary();
    }
    f.spawn((
        b.build(t),
        observe(move |_: On<Activate>, mut commands: Commands| {
            commands.queue(run);
        }),
    ));
}

fn checkbox(w: &mut World, name: &str) -> bool {
    let mut q = w.query::<(&Name, &CheckboxState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).is_some_and(|(_, s)| s.checked)
}

fn select_index(w: &mut World, name: &str) -> Option<usize> {
    let mut q = w.query::<(&Name, &SelectState)>();
    q.iter(w).find(|(n, _)| n.as_str() == name).map(|(_, s)| s.selected)
}

fn list_lines(w: &mut World, list: &str, lines: Vec<(String, bool)>) {
    let mut q = w.query::<(Entity, &Name)>();
    let Some(e) = q.iter(w).find(|(_, n)| n.as_str() == list).map(|(e, _)| e) else { return };
    let t = w.resource::<Theme>().clone();
    let mut commands = w.commands();
    commands.entity(e).despawn_children();
    commands.entity(e).with_children(|l| {
        for (i, (s, bold)) in lines.into_iter().enumerate() {
            l.spawn((Name::new(format!("{list}-{i}")), t.text(s, t.font_sm, if bold { FontWeight::SEMIBOLD } else { FontWeight::NORMAL }, t.foreground)));
        }
    });
    w.flush();
}

fn list_node(name: &'static str, h: f32) -> impl Bundle {
    (Name::new(name), Node { flex_direction: FlexDirection::Column, height: Val::Px(h), overflow: Overflow::scroll_y(), margin: UiRect::top(Val::Px(6.0)), ..default() })
}

// ---------------------------------------------------------------------------------------------
// Update PCB from schematic (GS14)

pub fn open_update(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-update-dialog")
            .title("Update PCB from schematic")
            .width(640.0)
            .body(move |b| {
                b.spawn(Checkbox::new("eda-update-relink").label("Re-link footprints to symbols by reference").checked(false).build(&t));
                b.spawn(Checkbox::new("eda-update-replace").label("Replace footprints with those the symbols name").checked(true).build(&t));
                b.spawn(Checkbox::new("eda-update-delete").label("Delete footprints with no symbol").checked(false).build(&t));
                b.spawn(Checkbox::new("eda-update-fields").label("Update footprint fields from symbols").checked(true).build(&t));
                b.spawn(list_node("eda-update-log", 220.0));
            })
            .footer(move |f| {
                button(f, &tf, "eda-update-run", "Update PCB", true, run_update);
                button(f, &tf, "eda-update-close", "Close", false, close_all);
            }),
    );
}

fn run_update(w: &mut World) {
    let lib = ui::libraries(w);
    // New footprints go to the middle of the view.
    let at = {
        let ui_state = w.resource::<super::EdaUi>();
        let key = w.resource::<super::Eda2d>().0;
        key.and_then(|k| ui_state.views.get(&k)).map(|v| v.0.center).map_or(cadrs_eda::units::Pt::ZERO, |c| cadrs_eda::units::Pt::new(c[0] as i64, c[1] as i64))
    };
    let opts = cadrs_eda::forward::Options {
        relink_by_reference: checkbox(w, "eda-update-relink"),
        replace_footprints: checkbox(w, "eda-update-replace"),
        delete_unused: checkbox(w, "eda-update-delete"),
        update_fields: checkbox(w, "eda-update-fields"),
        place_at: at,
    };
    let mut report = None;
    ui::commit(w, "Update PCB from schematic", |d| {
        report = Some(cadrs_eda::forward::update_pcb(d, &lib, &opts));
        Ok(())
    });
    if let Some(r) = report {
        let mut lines: Vec<(String, bool)> = r.messages.into_iter().map(|m| (m, false)).collect();
        lines.extend(r.errors.into_iter().map(|e| (format!("Error: {e}"), true)));
        list_lines(w, "eda-update-log", lines);
    }
}

// ---------------------------------------------------------------------------------------------
// Board setup (GS13)

const RULE_FIELDS: [(&str, &str); 8] = [
    ("Minimum clearance", "eda-setup-min-clearance"),
    ("Minimum track width", "eda-setup-min-track"),
    ("Minimum annular width", "eda-setup-annular"),
    ("Minimum via diameter", "eda-setup-min-via"),
    ("Copper to hole clearance", "eda-setup-hole-clearance"),
    ("Copper to edge clearance", "eda-setup-edge"),
    ("Minimum drill size", "eda-setup-drill"),
    ("Hole to hole clearance", "eda-setup-hole-hole"),
];

pub fn open_setup(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let b = d.board.clone();
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-setup-dialog")
            .title("Board setup")
            .width(560.0)
            .body(move |p| {
                p.spawn(t.text("Physical stackup", t.font_base, FontWeight::SEMIBOLD, t.foreground));
                row(p, &t, "Copper layers", |r| {
                    let s = Select::new("eda-setup-copper").bordered().width(Val::Px(120.0)).option("2", true).option("4", true).option("6", true);
                    r.spawn(s.selected(((b.copper_layers.max(2) / 2) - 1) as usize).build(&t));
                });
                mm_row(p, &t, "Board thickness", "eda-setup-thickness", b.thickness);
                p.spawn((t.text("Constraints", t.font_base, FontWeight::SEMIBOLD, t.foreground), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
                let r = &b.rules;
                let values = [r.min_clearance, r.min_track_width, r.min_annular_ring, r.min_via_diameter, r.hole_clearance, r.copper_edge_clearance, r.min_drill, r.hole_to_hole];
                for ((label, name), v) in RULE_FIELDS.iter().zip(values) {
                    mm_row(p, &t, label, name, v);
                }
                let c = r.class_of("").clone();
                p.spawn((t.text("Net class Default", t.font_base, FontWeight::SEMIBOLD, t.foreground), Node { margin: UiRect::top(Val::Px(8.0)), ..default() }));
                mm_row(p, &t, "Clearance", "eda-setup-clearance", c.clearance);
                mm_row(p, &t, "Track width", "eda-setup-track", c.track_width);
                mm_row(p, &t, "Via size", "eda-setup-via", c.via_diameter);
                mm_row(p, &t, "Via hole", "eda-setup-via-drill", c.via_drill);
            })
            .footer(move |f| {
                button(f, &tf, "eda-setup-ok", "OK", true, accept_setup);
                button(f, &tf, "eda-setup-cancel", "Cancel", false, close_all);
            }),
    );
}

fn read_mm(w: &mut World, name: &str) -> Result<Nm, String> {
    let s = ui::text_value(w, name);
    eval(&s, Unit::Mm).map_err(|e| format!("{s}: {e}"))
}

fn accept_setup(w: &mut World) {
    let copper = select_index(w, "eda-setup-copper").map(|i| (i as u8 + 1) * 2);
    let mut vals = vec![];
    for (_, name) in RULE_FIELDS {
        vals.push(read_mm(w, name));
    }
    let thickness = read_mm(w, "eda-setup-thickness");
    let class = ["eda-setup-clearance", "eda-setup-track", "eda-setup-via", "eda-setup-via-drill"].map(|n| read_mm(w, n));
    close_all(w);
    ui::commit(w, "Board setup", |d| {
        let b = &mut d.board;
        if let Some(c) = copper {
            b.copper_layers = c;
        }
        b.thickness = thickness?;
        b.stackup = cadrs_eda::board::Stackup::standard(b.copper_layers, b.thickness);
        let r = &mut b.rules;
        let v: Vec<Nm> = vals.into_iter().collect::<Result<_, _>>()?;
        (r.min_clearance, r.min_track_width, r.min_annular_ring, r.min_via_diameter, r.hole_clearance, r.copper_edge_clearance, r.min_drill, r.hole_to_hole) = (v[0], v[1], v[2], v[3], v[4], v[5], v[6], v[7]);
        let [cl, tw, vd, vh] = class;
        let c = r.class_mut("Default").ok_or("No Default net class")?;
        (c.clearance, c.track_width, c.via_diameter, c.via_drill) = (cl?, tw?, vd?, vh?);
        Ok(())
    });
}

// ---------------------------------------------------------------------------------------------
// Zone properties (GS18)

#[derive(Resource, Default)]
struct ZoneNets(Vec<String>);

pub fn open_zone(w: &mut World) {
    let Some((_, _, d)) = ui::current(w) else { return };
    let mut nets = d.board.nets.clone();
    if let Some(i) = nets.iter().position(|n| n == "GND") {
        nets.swap(0, i);
    }
    w.insert_resource(ZoneNets(nets.clone()));
    let active = w.resource::<LayoutState>().active;
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-zone-dialog")
            .title("Copper zone properties")
            .width(440.0)
            .body(move |p| {
                row(p, &t, "Net", |r| {
                    let mut s = Select::new("eda-zone-net").bordered().width(Val::Px(200.0));
                    for n in &nets {
                        s = s.option(n.clone(), true);
                    }
                    r.spawn(s.build(&t));
                });
                row(p, &t, "Layer", |r| {
                    let s = Select::new("eda-zone-layer").bordered().width(Val::Px(200.0)).option("Top copper", true).option("Bottom copper", true);
                    r.spawn(s.selected(if active == Layer::BottomCopper { 1 } else { 0 }).build(&t));
                });
            })
            .footer(move |f| {
                button(f, &tf, "eda-zone-ok", "OK", true, accept_zone);
                button(f, &tf, "eda-zone-cancel", "Cancel", false, close_all);
            }),
    );
}

fn accept_zone(w: &mut World) {
    let nets = w.resource::<ZoneNets>().0.clone();
    let net = select_index(w, "eda-zone-net").and_then(|i| nets.get(i).cloned()).unwrap_or_default();
    let layer = if select_index(w, "eda-zone-layer") == Some(1) { Layer::BottomCopper } else { Layer::TopCopper };
    close_all(w);
    w.resource_mut::<LayoutState>().active = layer;
    set_tool(w, Tool::Zone { net, layer, pts: vec![] });
}

// ---------------------------------------------------------------------------------------------
// DRC (GS19)

pub fn open_drc(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-drc-dialog")
            .title("Design rules checker")
            .width(680.0)
            .body(move |p| {
                p.spawn(Checkbox::new("eda-drc-refill").label("Refill all zones before performing DRC").checked(true).build(&t));
                p.spawn((Name::new("eda-drc-summary"), t.text("", t.font_base, FontWeight::MEDIUM, t.foreground), Node { margin: UiRect::top(Val::Px(6.0)), ..default() }));
                p.spawn(list_node("eda-drc-list", 300.0));
            })
            .footer(move |f| {
                button(f, &tf, "eda-drc-run", "Run DRC", true, run_drc);
                button(f, &tf, "eda-drc-close", "Close", false, close_all);
            }),
    );
}

fn run_drc(w: &mut World) {
    if checkbox(w, "eda-drc-refill") {
        ui::commit(w, "Fill zones", |d| {
            cadrs_eda::zone::fill_all(&mut d.board);
            Ok(())
        });
    }
    let Some((_, _, d)) = ui::current(w) else { return };
    let rep = cadrs_eda::drc::check(&d.board);
    let errors = rep.errors().count();
    let warnings = rep.violations.len() - errors;
    let summary = format!("Violations ({}): {errors} errors, {warnings} warnings. Unconnected items: {}", rep.violations.len(), rep.unconnected.len());
    ui::set_label(w, "eda-drc-summary", &summary);
    let mut lines = vec![];
    for v in &rep.violations {
        lines.push((format!("{}: {}", if v.rule.is_warning() { "Warning" } else { "Error" }, v.message), true));
        lines.extend(v.items.iter().map(|i| (format!("    {i}"), false)));
    }
    for a in &rep.unconnected {
        lines.push((format!("Unconnected: {} ({:.2}, {:.2}) – ({:.2}, {:.2})", a.net, to_mm(a.a.x), to_mm(a.a.y), to_mm(a.b.x), to_mm(a.b.y)), false));
    }
    list_lines(w, "eda-drc-list", lines);
}

// ---------------------------------------------------------------------------------------------
// Plot (GS21)

pub fn open_plot(w: &mut World) {
    let t = w.resource::<Theme>().clone();
    let tf = t.clone();
    spawn_dialog(
        w,
        Dialog::new("eda-plot-dialog")
            .title("Plot")
            .width(600.0)
            .body(move |p| {
                row(p, &t, "Output directory", |r| {
                    r.spawn(TextInput::new("eda-plot-dir").value("fab/").width(Val::Px(320.0)).height(26.0).build(&t));
                });
                p.spawn(t.text("Gerber X2: copper, board outline, solder mask, silkscreen and paste layers, with a job file. Drill files: Excellon, plated and not. Assembly: JLCPCB's BOM and placement (CPL) files. STEP: the board with its parts' 3D models.", t.font_sm, FontWeight::NORMAL, t.muted_foreground));
                p.spawn(list_node("eda-plot-log", 180.0));
            })
            .footer(move |f| {
                button(f, &tf, "eda-plot-step", "STEP", false, run_step);
                button(f, &tf, "eda-plot-assembly", "JLCPCB BOM and placement", false, run_assembly);
                button(f, &tf, "eda-plot-drill", "Generate drill files", false, run_drill);
                button(f, &tf, "eda-plot-run", "Plot", true, run_plot);
                button(f, &tf, "eda-plot-close", "Close", false, close_all);
            }),
    );
}

fn board_name(w: &World) -> String {
    let Some((el, b, _)) = w.resource::<super::Eda2d>().board() else { return "board".into() };
    w.resource::<crate::ActiveDocument>().doc.element(el).and_then(|e| e.pcb()).and_then(|s| s.board(b)).map(|b| cadrs_idf::safe_file_name(b.name())).unwrap_or_else(|| "board".into())
}

/// Writes the Gerbers and the job file into `dir`; returns what was written.
pub fn plot_to(w: &mut World, dir: &std::path::Path) -> Result<Vec<String>, String> {
    let Some((_, _, d)) = ui::current(w) else { return Err("No board".into()) };
    let name = board_name(w);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = vec![];
    for (file, text) in cadrs_eda::fab::gerbers(&d.board, &name) {
        std::fs::write(dir.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
        out.push(format!("Plotted {file}"));
    }
    let job = format!("{name}-job.gbrjob");
    std::fs::write(dir.join(&job), cadrs_eda::fab::job_file(&d.board, &name)).map_err(|e| e.to_string())?;
    out.push(format!("Created Gerber job file {job}"));
    Ok(out)
}

/// Writes the plated and non-plated Excellon files into `dir`.
pub fn drill_to(w: &mut World, dir: &std::path::Path) -> Result<Vec<String>, String> {
    let Some((_, _, d)) = ui::current(w) else { return Err("No board".into()) };
    let name = board_name(w);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = vec![];
    for (plated, suffix) in [(true, "PTH"), (false, "NPTH")] {
        let file = format!("{name}-{suffix}.drl");
        std::fs::write(dir.join(&file), cadrs_eda::fab::excellon(&d.board, plated)).map_err(|e| format!("{file}: {e}"))?;
        out.push(format!("Created {file}"));
    }
    Ok(out)
}

fn plot_dir(w: &mut World) -> std::path::PathBuf {
    let s = ui::text_value(w, "eda-plot-dir");
    std::path::PathBuf::from(if s.trim().is_empty() { "fab/".into() } else { s })
}

fn run_plot(w: &mut World) {
    let dir = plot_dir(w);
    let lines = match plot_to(w, &dir) {
        Ok(v) => v.into_iter().map(|l| (l, false)).collect(),
        Err(e) => vec![(format!("Error: {e}"), true)],
    };
    list_lines(w, "eda-plot-log", lines);
}

fn run_drill(w: &mut World) {
    let dir = plot_dir(w);
    let lines = match drill_to(w, &dir) {
        Ok(v) => v.into_iter().map(|l| (l, false)).collect(),
        Err(e) => vec![(format!("Error: {e}"), true)],
    };
    list_lines(w, "eda-plot-log", lines);
}


/// Writes JLCPCB's assembly files into `dir`: `<board>-bom.csv` (from the schematic) and
/// `<board>-cpl.csv` (the placement of the surface-mount parts).
pub fn assembly_to(w: &mut World, dir: &std::path::Path) -> Result<Vec<String>, String> {
    let Some((_, _, d)) = ui::current(w) else { return Err("No board".into()) };
    let name = board_name(w);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut out = vec![];
    for (file, text) in [(format!("{name}-bom.csv"), cadrs_eda::bom::jlcpcb_csv(&d.schematic)), (format!("{name}-cpl.csv"), cadrs_eda::fab::jlcpcb_cpl(&d.board))] {
        std::fs::write(dir.join(&file), text).map_err(|e| format!("{file}: {e}"))?;
        out.push(format!("Created {file}"));
    }
    Ok(out)
}

fn run_assembly(w: &mut World) {
    let dir = plot_dir(w);
    let lines = match assembly_to(w, &dir) {
        Ok(v) => v.into_iter().map(|l| (l, false)).collect(),
        Err(e) => vec![(format!("Error: {e}"), true)],
    };
    list_lines(w, "eda-plot-log", lines);
}

/// Writes `<board>.step` into `dir`: the board and its parts' 3D models.
pub fn step_to(w: &mut World, dir: &std::path::Path) -> Result<Vec<String>, String> {
    let Some((el, b, d)) = ui::current(w) else { return Err("No board".into()) };
    let pcb = w.resource::<crate::ActiveDocument>().doc.element(el).and_then(|e| e.pcb()).and_then(|s| s.board(b)).map(|x| x.board.clone()).ok_or("No board")?;
    let name = board_name(w);
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let (bytes, warnings) = cadrs_pcb::step::native_board_step(&pcb, &d)?;
    let file = format!("{name}.step");
    std::fs::write(dir.join(&file), bytes).map_err(|e| format!("{file}: {e}"))?;
    let mut out = vec![format!("Created {file}")];
    out.extend(warnings.into_iter().map(|w| format!("Warning: {w}")));
    Ok(out)
}

fn run_step(w: &mut World) {
    let dir = plot_dir(w);
    let lines = match step_to(w, &dir) {
        Ok(v) => v.into_iter().map(|l| { let warn = l.starts_with("Warning"); (l, warn) }).collect(),
        Err(e) => vec![(format!("Error: {e}"), true)],
    };
    list_lines(w, "eda-plot-log", lines);
}
pub fn register(_app: &mut App) {}

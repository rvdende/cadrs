//! The setup sheet: what the operator needs at the machine to run a setup, as a page to print
//! beside its program (the Post dialog writes it next to the NC file): the machine, the stock
//! and how big to cut it, where the work coordinate system is, the workholding, the tools to
//! load with their stick-out and holders, and each operation with its tool, speeds, depths and
//! time.

use std::fmt::Write as _;

use cadrs_cam::tool::fmt_mm;
use cadrs_cam::{Dialect, Frame, Step};

use super::program::{SetupRun, stock_in, stock_shape};
use super::{CamStudio, Job, Origin, Role, Setup, StockKind};
use crate::parts::Part;

/// The setup sheet of `run` (a setup's operations), posted as `file`.
pub fn setup_sheet(
    cam: &CamStudio,
    job: &Job,
    setup: &Setup,
    run: &SetupRun,
    parts: &[Part],
    file: &str,
) -> String {
    let mut h = String::new();
    let title = format!("{} · {}", job.name, setup.name);
    let _ = write!(
        h,
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\">\
         <title>{}</title><style>{STYLE}</style></head><body>\n",
        esc(&format!("Setup sheet: {title}"))
    );
    let _ = writeln!(
        h,
        "<h1>{}</h1><p class=\"sub\">Program O{:04} · {}</p>",
        esc(&title),
        setup.program_number,
        esc(file)
    );
    // Machine.
    h.push_str("<h2>Machine</h2><table>");
    match &job.machine {
        Some(m) => {
            row(&mut h, "Machine", &m.title());
            row(&mut h, "Control", m.dialect.label());
            row(
                &mut h,
                "Travel",
                &format!(
                    "X {} · Y {} · Z {} mm",
                    fmt_mm(m.travel[0]),
                    fmt_mm(m.travel[1]),
                    fmt_mm(m.travel[2])
                ),
            );
            if let Some(ix) = m.indexing {
                row(
                    &mut h,
                    "Indexing",
                    &format!("3+2, tilting up to {}°", fmt_mm(ix.max_tilt)),
                );
            }
        }
        None => row(&mut h, "Machine", "none"),
    }
    h.push_str("</table>");
    // Stock and work coordinate system.
    h.push_str("<h2>Stock and work coordinates</h2><table>");
    if let Some(shape) = stock_shape(cam, job, parts) {
        let b = stock_in(&shape, &run.frame).bounds;
        let s = b.size();
        let kind = match job.stock.kind {
            StockKind::Component(id) => cam.component(id).map_or("User defined".to_string(), |c| {
                format!("User defined ({})", c.name)
            }),
            k => k.label().to_string(),
        };
        row(&mut h, "Stock", &kind);
        row(
            &mut h,
            "Size",
            &format!(
                "{} × {} × {} mm (X × Y × Z in the setup)",
                fmt_mm(s[0]),
                fmt_mm(s[1]),
                fmt_mm(s[2])
            ),
        );
        row(
            &mut h,
            "Stock from the WCS",
            &format!(
                "X {} to {} · Y {} to {} · Z {} to {}",
                fmt_mm(b.min[0]),
                fmt_mm(b.max[0]),
                fmt_mm(b.min[1]),
                fmt_mm(b.max[1]),
                fmt_mm(b.min[2]),
                fmt_mm(b.max[2])
            ),
        );
    }
    row(
        &mut h,
        "Datum",
        &cadrs_cam::post::datum_code(setup.wcs.datum.unwrap_or(1)),
    );
    row(&mut h, "WCS origin", &origin_words(&setup.wcs.origin));
    // Set these too: an offset left at machine zero crashes the tool.
    for line in super::program::offset_lines(setup, job, run) {
        let (code, rest) = line.split_once(": ").unwrap_or(("Offset", &line));
        row(&mut h, code, rest);
    }
    row(
        &mut h,
        "On the table",
        &match setup.mating {
            super::Mating::Automatic => "The stock's bottom centred on the table".to_string(),
            super::Mating::Manual(o) => format!(
                "The stock's bottom centre at X {} Y {} Z {} from the table's centre",
                fmt_mm(o[0]),
                fmt_mm(o[1]),
                fmt_mm(o[2])
            ),
        },
    );
    row(&mut h, "Tool axis (Z)", &axis_words(&run.frame));
    let holding: Vec<&str> = cam
        .components
        .iter()
        .filter(|c| c.role == Role::Workholding)
        .map(|c| c.name.as_str())
        .collect();
    row(
        &mut h,
        "Workholding",
        &if holding.is_empty() {
            "none modelled".to_string()
        } else {
            holding.join(", ")
        },
    );
    h.push_str("</table>");
    // Tools, in the order they're first used.
    h.push_str(
        "<h2>Tools</h2><table class=\"grid\"><tr><th>T</th><th>Tool</th><th>Ø</th>\
         <th>Flutes</th><th>Stick-out</th><th>Holder</th><th>Used by</th></tr>",
    );
    let mut seen: Vec<u32> = Vec::new();
    for o in &run.ops {
        if seen.contains(&o.tool_number) {
            continue;
        }
        seen.push(o.tool_number);
        let t = o.tool;
        let users: Vec<&str> = run
            .ops
            .iter()
            .filter(|x| x.tool_number == o.tool_number)
            .map(|x| x.name)
            .collect();
        let _ = write!(
            h,
            "<tr><td>T{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
            o.tool_number,
            esc(&t.name),
            fmt_mm(t.diameter),
            if t.is_beam() {
                "—".into()
            } else {
                format!("{} mm", fmt_mm(t.cutting_length))
            },
            if t.is_beam() {
                "—".into()
            } else {
                format!("{} mm", fmt_mm(t.overall_length))
            },
            esc(t.holder.as_ref().map_or("—", |p| p.name.as_str())),
            esc(&users.join(", "))
        );
    }
    h.push_str("</table>");
    // Operations.
    h.push_str(
        "<h2>Operations</h2><table class=\"grid\"><tr><th>#</th><th>Operation</th>\
         <th>Pattern</th><th>T</th><th>Speed</th><th>Feed</th><th>Depths</th>\
         <th>Left</th><th>Z from the WCS</th><th>Time</th></tr>",
    );
    let rapid = job.machine.as_ref().map_or(5_000.0, |m| m.rapid_rate);
    let mut total = 0.0;
    let mut notes: Vec<String> = match stock_shape(cam, job, parts) {
        Some(shape) => {
            super::program::travel_warnings(job, setup, run, &stock_in(&shape, &run.frame))
        }
        None => Vec::new(),
    };
    for (i, o) in run.ops.iter().enumerate() {
        let params = cam
            .operation(o.id)
            .and_then(|(_, _, op)| op.toolpath())
            .map(|t| t.params.clone());
        let start = o.toolpath.start().unwrap_or([0.0; 3]);
        let stats = o.toolpath.stats(start, rapid);
        total += stats.minutes;
        let b = o.toolpath.bounds();
        // As posted: the speed the toolpath runs at (a machine's limit slows it, and the
        // feeds with it).
        let rpm = o.toolpath.steps.iter().find_map(|s| match s {
            cadrs_cam::Step::Spindle { rpm, .. } => Some(*rpm),
            _ => None,
        });
        let (pattern, speed, feed, depths, left) = match &params {
            Some(p) => (
                p.pattern.label().to_string(),
                if o.tool.is_beam() {
                    format!("{} %", fmt_mm(p.beam.power))
                } else {
                    format!("{} rpm", rpm.unwrap_or(p.speeds.spindle_rpm).round())
                },
                format!(
                    "{} mm/min",
                    (p.speeds.feed * rpm.map_or(1.0, |r| r / p.speeds.spindle_rpm.max(1e-9)))
                        .round()
                ),
                format!("↓ {} · ↔ {}", fmt_mm(p.step_down), fmt_mm(p.step_over)),
                format!("{} / {}", fmt_mm(p.wall_stock), fmt_mm(p.floor_stock)),
            ),
            None => Default::default(),
        };
        let _ = write!(
            h,
            "<tr><td>{}</td><td>{}{}</td><td>{}</td><td>T{}</td><td>{}</td><td>{}</td>\
             <td>{}</td><td>{}</td><td>{} to {}</td><td>{}</td></tr>",
            i + 1,
            esc(o.name),
            if o.tilted { " <em>(3+2)</em>" } else { "" },
            esc(&pattern),
            o.tool_number,
            speed,
            feed,
            depths,
            left,
            fmt_mm(b.min[2]),
            fmt_mm(b.max[2]),
            minutes(stats.minutes)
        );
        for w in &o.toolpath.warnings {
            notes.push(format!("{}: {w}", o.name));
        }
        if o.posting.stop_before {
            notes.push(format!("{}: the program stops before it (M0)", o.name));
        }
        if o.toolpath
            .steps
            .iter()
            .any(|s| matches!(s, Step::Compensation(Some(_))))
        {
            let d = job.machine.as_ref().map_or(Dialect::Fanuc, |m| m.dialect);
            notes.push(format!(
                "{}: {}",
                o.name,
                cadrs_cam::post::compensation_note(d, o.tool_number)
            ));
        }
        if !o.posting.comment.is_empty() {
            notes.push(format!("{}: {}", o.name, o.posting.comment));
        }
    }
    let _ = write!(
        h,
        "<tr class=\"total\"><td></td><td colspan=\"8\">{} operation{}, {} tool{}</td><td>{}</td></tr></table>",
        run.ops.len(),
        if run.ops.len() == 1 { "" } else { "s" },
        seen.len(),
        if seen.len() == 1 { "" } else { "s" },
        minutes(total)
    );
    if !notes.is_empty() {
        h.push_str("<h2>Notes</h2><ul>");
        for n in notes {
            let _ = write!(h, "<li>{}</li>", esc(&n));
        }
        h.push_str("</ul>");
    }
    h.push_str("<p class=\"sub\">Depths: step down ↓ and step over ↔ (mm); left: wall / floor stock (mm); times are estimates from the machine's rapid rate and the feeds.</p></body></html>\n");
    h
}

const STYLE: &str = "body{font-family:Inter,system-ui,sans-serif;font-size:13px;color:#1d2330;\
margin:24px;max-width:1000px}h1{font-size:20px;margin:0}h2{font-size:15px;margin:22px 0 6px}\
.sub{color:#5b6475;margin:4px 0}table{border-collapse:collapse;width:100%}\
td,th{padding:4px 8px;text-align:left;vertical-align:top}table:not(.grid) td:first-child{\
color:#5b6475;width:180px}.grid th{background:#eef1f5;font-weight:600}.grid td,.grid th{\
border:1px solid #d5dae2}.total td{font-weight:600}@media print{body{margin:0}}";

fn row(h: &mut String, k: &str, v: &str) {
    let _ = write!(h, "<tr><td>{}</td><td>{}</td></tr>", esc(k), esc(v));
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn minutes(m: f64) -> String {
    let s = (m * 60.0).round() as i64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// "The stock's box: top, centre" and the like.
fn origin_words(o: &Origin) -> String {
    let bp = |p: &[i8; 3], what: &str| {
        let x = ["left (−X)", "centre", "right (+X)"][(p[0] + 1).clamp(0, 2) as usize];
        let y = ["front (−Y)", "centre", "back (+Y)"][(p[1] + 1).clamp(0, 2) as usize];
        let z = ["bottom", "middle", "top"][(p[2] + 1).clamp(0, 2) as usize];
        format!("{what}: {z}, {x}, {y}")
    };
    match o {
        Origin::World => "The model's origin".into(),
        Origin::ComponentBox(p) => bp(p, "The parts' box"),
        Origin::StockBox(p) => bp(p, "The stock's box"),
        Origin::Manual(p) => format!(
            "At ({}, {}, {}) in the model",
            fmt_mm(p[0]),
            fmt_mm(p[1]),
            fmt_mm(p[2])
        ),
    }
}

fn axis_words(f: &Frame) -> String {
    let named = [
        ([0.0, 0.0, 1.0], "the model's +Z"),
        ([0.0, 0.0, -1.0], "the model's −Z"),
        ([1.0, 0.0, 0.0], "the model's +X"),
        ([-1.0, 0.0, 0.0], "the model's −X"),
        ([0.0, 1.0, 0.0], "the model's +Y"),
        ([0.0, -1.0, 0.0], "the model's −Y"),
    ];
    for (d, n) in named {
        if cadrs_cam::geom::dot(d, f.z) > 1.0 - 1e-9 {
            return n.into();
        }
    }
    format!("({:.3}, {:.3}, {:.3})", f.z[0], f.z[1], f.z[2])
}

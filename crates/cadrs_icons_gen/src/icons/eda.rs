//! Electronics design: the schematic editor and the PCB layout editor of a PCB Studio.
//!
//! Three kinds, chosen as in the rest of the set:
//! - Sketch for the drawing and placement tools (wire, bus, labels, pins, tracks, vias, zones,
//!   board outline): flat geometry at 1.5 with the points the user picks as ACCENT dots (DOT), as in
//!   sketch.rs. Where the placed object is itself a mark (a junction dot, a no-connect cross, a bus
//!   entry) that mark is the accent instead.
//! - Line for the documents, library objects and commands (schematic, symbol, footprint, rule
//!   checks, annotate, layers, outputs...): ink at 1.75 plus one accent modifier, as in ui_b.rs.
//!   Plain objects (symbol, footprint) keep the accent on one small detail or none.
//! - Solid for board-3d, the 3D board view, drawn like the document types in assembly.rs.
//!
//! Shared vocabulary:
//! - Schematic wires are thin strokes; a bus is a thick (BUS) stroke.
//! - PCB tracks are thick (TRACK) round-capped strokes; pads are filled ink in line icons and SOFT
//!   regions with a TOP drill hole in sketch icons.
//! - The op-amp triangle stands for a schematic symbol, the zigzag for a resistor.

use crate::common::*;
use Kind::{Line, Sketch, Solid};

pub fn icons() -> Vec<Def> {
    vec![
        def("schematic", Line, "Schematic", schematic),
        def("symbol", Line, "Symbol", symbol),
        def("add-symbol", Line, "Add symbol", add_symbol),
        def("add-power", Line, "Add power symbol", add_power),
        def("wire", Sketch, "Wire", wire),
        def("bus", Sketch, "Bus", bus),
        def("bus-entry", Sketch, "Bus entry", bus_entry),
        def("junction", Sketch, "Junction", junction),
        def("no-connect", Sketch, "No connection flag", no_connect),
        def("pin", Sketch, "Pin", pin),
        def("net-label", Sketch, "Net label", net_label),
        def("global-label", Sketch, "Global label", global_label),
        def(
            "hierarchical-label",
            Sketch,
            "Hierarchical label",
            hierarchical_label,
        ),
        def(
            "hierarchical-sheet",
            Sketch,
            "Hierarchical sheet",
            hierarchical_sheet,
        ),
        def("annotate", Line, "Annotate", annotate),
        def("erc", Line, "Electrical rules check", erc),
        def(
            "assign-footprints",
            Line,
            "Assign footprints",
            assign_footprints,
        ),
        def("update-pcb", Line, "Update PCB from schematic", update_pcb),
        def("footprint", Line, "Footprint", footprint),
        def("pad", Sketch, "Pad", pad),
        def("route-track", Sketch, "Route tracks", route_track),
        def("via", Sketch, "Via", via),
        def("copper-zone", Sketch, "Copper zone", copper_zone),
        def("keepout-zone", Sketch, "Keepout zone", keepout_zone),
        def("fill-zones", Line, "Fill zones", fill_zones),
        def("board-outline", Sketch, "Board outline", board_outline),
        def("ratsnest", Line, "Ratsnest", ratsnest),
        def("drc", Line, "Design rules check", drc),
        def("layers", Line, "Layers", layers),
        def("flip-side", Line, "Flip to other side", flip_side),
        def("highlight-net", Line, "Highlight net", highlight_net),
        def(
            "select-connected",
            Line,
            "Select connected",
            select_connected,
        ),
        def(
            "plot-gerbers",
            Line,
            "Plot fabrication outputs",
            plot_gerbers,
        ),
        def("drill-file", Line, "Drill file", drill_file),
        def("board-3d", Solid, "3D board view", board_3d),
    ]
}

// ---------------------------------------------------------------------------------------------
// Sketch helpers (as in sketch.rs; kept separate so each module reads on its own)

const DOT: f64 = 1.45;
const CON: &str = r#"stroke-width="1" stroke-dasharray="1.2 2.6""#; // dash(1)
const ACC: &str = r##"stroke="#2563eb""##;
const FILL: &str = SOFT;
const REGION: &str = r#"fill-opacity="0.45""#;
const MUTED: &str = r#"stroke-opacity="0.5""#;
const BUS: f64 = 3.2; // schematic bus stroke width
const TRACK: f64 = 2.6; // PCB track stroke width in sketch icons

fn dot(x: f64, y: f64) -> String {
    circle(x, y, DOT, ACCENT, r#"stroke="none""#)
}

fn dots(ps: &[P]) -> Vec<String> {
    ps.iter().map(|&(x, y)| dot(x, y)).collect()
}

/// The part of segment a-b inside the convex polygon, or None.
fn clip_seg(a: P, b: P, convex: &[P]) -> Option<(P, P)> {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let area = psum(pairs(convex).map(|(p, q)| p.0 * q.1 - q.0 * p.1));
    let sgn = if area > 0.0 { 1.0 } else { -1.0 };
    for (p, q) in pairs(convex) {
        // Inside is to the left of p->q for a positive (clockwise on screen) polygon.
        let (ex, ey) = (q.0 - p.0, q.1 - p.1);
        let num = sgn * (ex * (a.1 - p.1) - ey * (a.0 - p.0));
        let den = sgn * (ex * dy - ey * dx);
        if den.abs() < 1e-12 {
            if num < 0.0 {
                return None;
            }
            continue;
        }
        let t = -num / den;
        if den > 0.0 {
            t0 = t0.max(t);
        } else {
            t1 = t1.min(t);
        }
        if t0 >= t1 {
            return None;
        }
    }
    Some((
        (a.0 + t0 * dx, a.1 + t0 * dy),
        (a.0 + t1 * dx, a.1 + t1 * dy),
    ))
}

/// 45-degree hatch lines across the convex polygon.
fn hatch(convex: &[P]) -> Vec<String> {
    let (step, extra) = (3.2, r#"stroke-width="1""#);
    let mut out = Vec::new();
    for k in -30..60 {
        let c = k as f64 * step;
        if let Some((s0, s1)) = clip_seg((c - 30.0, 30.0), (c + 30.0, -30.0), convex)
            && hypot(s1.0 - s0.0, s1.1 - s0.1) > 0.8
        {
            out.push(seg(s0, s1, extra));
        }
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Line helpers (as in ui_b.rs)

const R: f64 = 2.5;

/// An element in ACCENT: the stroke, and the fill where it was INK.
fn acc(el: String) -> String {
    el.replace(&format!(r#"fill="{INK}""#), &format!(r#"fill="{ACCENT}""#))
        .replacen("/>", &format!(r#" stroke="{ACCENT}"/>"#), 1)
}

/// Elements in ACCENT.
fn acc_all(els: Vec<String>) -> Vec<String> {
    els.into_iter().map(acc).collect()
}

fn plus(cx: f64, cy: f64) -> String {
    let arm = 2.5;
    path(
        &format!(
            "M{} {} H{} M{} {} V{}",
            f(cx - arm),
            f(cy),
            f(cx + arm),
            f(cx),
            f(cy - arm),
            f(cy + arm)
        ),
        "",
        "",
    )
}

/// A straight arrow with an open head, `head` along and 2*head across.
fn open_arrow(x0: f64, y0: f64, x1: f64, y1: f64, head: f64) -> Vec<String> {
    let (dx, dy) = (x1 - x0, y1 - y0);
    let l = hypot(dx, dy);
    let (ux, uy) = (dx / l, dy / l);
    let (bx, by) = (x1 - ux * head, y1 - uy * head);
    let (nx, ny) = (-uy * head, ux * head);
    els![
        line(x0, y0, x1, y1, ""),
        polyline(&[(bx + nx, by + ny), (x1, y1), (bx - nx, by - ny)], "")
    ]
}

fn head_at(tip: P, frm: P, s: f64) -> String {
    let (dx, dy) = (tip.0 - frm.0, tip.1 - frm.1);
    let l = or1(hypot(dx, dy));
    let (ux, uy) = (dx / l, dy / l);
    let (nx, ny) = (-uy, ux);
    polyline(
        &[
            (tip.0 - s * ux + s * nx, tip.1 - s * uy + s * ny),
            tip,
            (tip.0 - s * ux - s * nx, tip.1 - s * uy - s * ny),
        ],
        "",
    )
}

fn d(ps: &[P], close: bool) -> String {
    let body = ps
        .iter()
        .map(|&(x, y)| format!("{} {}", f(x), f(y)))
        .collect::<Vec<_>>()
        .join(" L");
    format!("M{body}{}", if close { " Z" } else { "" })
}

/// An op-amp triangle, `h` tall with its left side at x0 from y0, and its three pins.
fn opamp(x0: f64, y0: f64, h: f64, pins: f64) -> Vec<String> {
    let w = h * 0.87;
    let cy = y0 + h / 2.0;
    let yi = (y0 + h * 0.27, y0 + h * 0.73);
    els![
        poly(&[(x0, y0), (x0 + w, cy), (x0, y0 + h)], "", ""),
        path(
            &format!(
                "M{} {} H{} M{} {} H{} M{} {} H{}",
                f(x0 - pins),
                f(yi.0),
                f(x0),
                f(x0 - pins),
                f(yi.1),
                f(x0),
                f(x0 + w),
                f(cy),
                f(x0 + w + pins)
            ),
            "",
            ""
        ),
    ]
}

/// A check mark with its lowest point at (x, y).
fn check(x: f64, y: f64) -> String {
    polyline(&[(x - 3.5, y - 3.5), (x, y), (x + 7.0, y - 7.0)], "")
}

fn doc() -> Vec<String> {
    let (x0, x1, y0, y1, fold) = (5.0, 19.0, 3.0, 21.0, 5.0);
    let (fx, fy) = (x1 - fold, y0 + fold);
    let d = format!(
        "M{} {} H{} A2.5 2.5 0 0 0 {} {} V{} A2.5 2.5 0 0 0 {} {} H{} A2.5 2.5 0 0 0 {} {} V{} Z",
        f(fx),
        f(y0),
        f(x0 + R),
        f(x0),
        f(y0 + R),
        f(y1 - R),
        f(x0 + R),
        f(y1),
        f(x1 - R),
        f(x1),
        f(y1 - R),
        f(fy)
    );
    let fold_d = format!(
        "M{} {} V{} A1.5 1.5 0 0 0 {} {} H{}",
        f(fx),
        f(y0),
        f(fy - 1.5),
        f(fx + 1.5),
        f(fy),
        f(x1)
    );
    els![path(&d, "", ""), path(&fold_d, "", "")]
}

/// A filled SMD pad centred on (x, y) (ink, drawn with the root stroke).
fn smd(x: f64, y: f64, w: f64, h: f64) -> String {
    rect(
        x - w / 2.0,
        y - h / 2.0,
        w,
        h,
        0.4,
        INK,
        r#"stroke-width="1""#,
    )
}

// =============================================================================================
// Schematic

fn schematic() -> Vec<String> {
    // A drawing sheet with its title block (lower right) and a resistor drawn on it (accent).
    let zig = [
        (6.0, 10.0),
        (8.0, 10.0),
        (9.0, 7.5),
        (11.0, 12.5),
        (13.0, 7.5),
        (15.0, 12.5),
        (16.0, 10.0),
        (18.0, 10.0),
    ];
    els![
        rect(3.0, 4.0, 18.0, 16.0, R, "", ""),
        path("M13 20 V16 H21", "", ""),
        acc(polyline(&zig, ""))
    ]
}

fn symbol() -> Vec<String> {
    // A library symbol: an op-amp with its two inputs and output.
    els![
        opamp(6.5, 4.0, 16.0, 3.5),
        path("M8.5 9.75 H11 M8.5 16.25 H11 M9.75 15 V17.5", "", "")
    ]
}

fn add_symbol() -> Vec<String> {
    els![opamp(5.0, 3.0, 12.5, 2.75), acc(plus(18.0, 18.0))]
}

fn add_power() -> Vec<String> {
    // A ground symbol: a wire into three shrinking bars.
    els![
        path(
            "M9.5 3.5 V11.5 M3 11.5 H16 M5.75 15.25 H13.25 M8 19 H11",
            "",
            ""
        ),
        acc(plus(18.5, 5.5))
    ]
}

fn wire() -> Vec<String> {
    let (a, b, c, d) = ((3.5, 17.5), (11.0, 17.5), (11.0, 6.5), (20.5, 6.5));
    els![polyline(&[a, b, c, d], ""), dots(&[a, d])]
}

fn bus() -> Vec<String> {
    let (a, b, c) = ((3.5, 7.0), (17.0, 7.0), (17.0, 20.0));
    els![
        polyline(&[a, b, c], &format!(r#"stroke-width="{}""#, f(BUS))),
        dots(&[a, c])
    ]
}

fn bus_entry() -> Vec<String> {
    // A bus (thick, context) and the 45-degree entry (accent) that joins a wire to it.
    els![
        line(
            6.0,
            3.5,
            6.0,
            20.5,
            &format!(r#"stroke-width="{}" {MUTED}"#, f(BUS))
        ),
        line(12.0, 12.0, 20.5, 12.0, ""),
        line(6.0, 18.0, 12.0, 12.0, &format!(r#"{ACC} stroke-width="2""#)),
    ]
}

fn junction() -> Vec<String> {
    els![
        line(3.5, 12.0, 20.5, 12.0, ""),
        line(12.0, 12.0, 12.0, 20.5, ""),
        line(12.0, 3.5, 12.0, 12.0, ""),
        circle(12.0, 12.0, 2.6, ACCENT, r#"stroke="none""#),
    ]
}

fn no_connect() -> Vec<String> {
    // A pin leaving a symbol's body (left), its free end crossed out (accent).
    let (c, a) = ((15.5, 12.0), 3.25);
    els![
        path("M3.5 4.5 H8.5 V19.5 H3.5", FILL, REGION),
        line(8.5, 12.0, c.0, c.1, ""),
        path(
            &format!(
                "M{} {} L{} {} M{} {} L{} {}",
                f(c.0 - a),
                f(c.1 - a),
                f(c.0 + a),
                f(c.1 + a),
                f(c.0 + a),
                f(c.1 - a),
                f(c.0 - a),
                f(c.1 + a)
            ),
            "",
            &format!(r#"{ACC} stroke-width="2""#)
        ),
    ]
}

fn pin() -> Vec<String> {
    // A pin on the edge of a symbol's body; its free end is the connection point.
    els![
        path("M20.5 4.5 H13 V19.5 H20.5", FILL, REGION),
        line(4.0, 12.0, 13.0, 12.0, r#"stroke-width="2""#),
        dot(4.0, 12.0)
    ]
}

fn net_label() -> Vec<String> {
    // A name (an "A") written on a wire, anchored at its lower left.
    els![
        line(3.5, 16.5, 20.5, 16.5, ""),
        polyline(&[(6.5, 13.5), (10.0, 4.5), (13.5, 13.5)], ""),
        line(7.9, 10.4, 12.1, 10.4, ""),
        dot(6.5, 16.5),
    ]
}

fn global_label() -> Vec<String> {
    // A wire ending in a pointed tag: the net name, shared across every sheet.
    let tag = [
        (9.0, 12.0),
        (12.5, 7.5),
        (20.5, 7.5),
        (20.5, 16.5),
        (12.5, 16.5),
    ];
    els![
        line(3.5, 12.0, 9.0, 12.0, ""),
        poly(&tag, FILL, REGION),
        dot(9.0, 12.0)
    ]
}

fn hierarchical_label() -> Vec<String> {
    // A wire ending in an arrow-shaped port inside a sheet (dashed), leading up to its parent.
    let port = [
        (9.0, 8.5),
        (14.0, 8.5),
        (17.5, 12.0),
        (14.0, 15.5),
        (9.0, 15.5),
    ];
    els![
        rect(1.75, 3.5, 18.0, 17.0, 0.0, "", CON),
        line(4.0, 12.0, 9.0, 12.0, ""),
        poly(&port, FILL, REGION),
        dot(9.0, 12.0),
    ]
}

fn hierarchical_sheet() -> Vec<String> {
    // A sheet symbol placed by two corners, with sheet pins on its left and right edges.
    let (x0, y0, x1, y1) = (6.0, 4.0, 18.0, 20.0);
    els![
        rect(x0, y0, x1 - x0, y1 - y0, 0.0, FILL, REGION),
        path(
            &format!(
                "M{} 9 H{} M{} 15 H{} M{} 12 H{}",
                f(x0 - 3.0),
                f(x0),
                f(x0 - 3.0),
                f(x0),
                f(x1),
                f(x1 + 3.0)
            ),
            "",
            ""
        ),
        rect(9.0, 8.0, 6.0, 8.0, 0.0, "", r#"stroke-width="1""#),
        dots(&[(x0, y0), (x1, y1)]),
    ]
}

fn annotate() -> Vec<String> {
    // A resistor and its reference designator, R and a number (accent): annotation numbers it.
    els![
        path("M3 17 H6.5 M17.5 17 H21", "", ""),
        rect(6.5, 14.25, 11.0, 5.5, 1.0, "", ""),
        path(
            "M6.5 11 V3.5 H9.75 A2.1 2.1 0 0 1 9.75 7.7 H6.5 M9.6 7.7 L12 11",
            "",
            ""
        ),
        acc(polyline(&[(15.0, 5.25), (17.25, 3.5), (17.25, 11.0)], "")),
    ]
}

fn erc() -> Vec<String> {
    let zig = [
        (3.0, 7.5),
        (5.5, 7.5),
        (6.75, 4.5),
        (9.25, 10.5),
        (11.75, 4.5),
        (14.25, 10.5),
        (16.75, 4.5),
        (18.0, 7.5),
        (21.0, 7.5),
    ];
    els![polyline(&zig, ""), acc(check(10.5, 19.5))]
}

fn assign_footprints() -> Vec<String> {
    // A resistor symbol (top) becomes its footprint, two pads (bottom): the arrow (accent).
    let zig = [
        (3.0, 5.5),
        (5.5, 5.5),
        (6.75, 3.0),
        (9.25, 8.0),
        (11.75, 3.0),
        (14.25, 8.0),
        (16.75, 3.0),
        (18.0, 5.5),
        (21.0, 5.5),
    ];
    els![
        polyline(&zig, ""),
        acc_all(open_arrow(12.0, 10.0, 12.0, 15.5, 2.5)),
        smd(6.0, 18.5, 4.0, 3.5),
        smd(18.0, 18.5, 4.0, 3.5),
    ]
}

fn update_pcb() -> Vec<String> {
    // The board (as in `board`) with the schematic's changes arriving from the left (accent).
    let d = "M8 9.5 V7.5 A2.5 2.5 0 0 1 10.5 5 H18.5 A2.5 2.5 0 0 1 21 7.5 V16.5 \
             A2.5 2.5 0 0 1 18.5 19 H10.5 A2.5 2.5 0 0 1 8 16.5 V14.5";
    els![
        path(d, "", ""),
        circle(17.5, 8.5, 1.0, "", ""),
        circle(17.5, 15.5, 1.0, "", ""),
        acc_all(open_arrow(2.5, 12.0, 14.0, 12.0, 3.0)),
    ]
}

// =============================================================================================
// Layout

fn footprint() -> Vec<String> {
    // A footprint: the package outline between two rows of pads, with the pin 1 mark (accent).
    let pads: Vec<String> = [4.5, 19.5]
        .iter()
        .flat_map(|&x| [6.0, 12.0, 18.0].map(|y| smd(x, y, 3.5, 2.5)))
        .collect();
    els![
        rect(8.5, 3.5, 7.0, 17.0, 1.25, "", ""),
        pads,
        acc(circle(11.25, 6.25, 0.5, INK, ""))
    ]
}

fn pad() -> Vec<String> {
    // A through-hole pad placed by its centre: copper (region) around a drill hole.
    els![
        rect(4.5, 4.5, 15.0, 15.0, 3.5, FILL, REGION),
        circle(12.0, 12.0, 3.6, TOP, ""),
        dot(12.0, 12.0)
    ]
}

fn route_track() -> Vec<String> {
    // A track leaving a pad with a 45-degree bend; picked points at the start and the end.
    let (pad_c, b, c) = ((5.0, 18.0), (10.5, 12.5), (19.5, 12.5));
    els![
        circle(pad_c.0, pad_c.1, 2.9, FILL, ""),
        polyline(&[pad_c, b, c], &format!(r#"stroke-width="{}""#, f(TRACK))),
        circle(pad_c.0, pad_c.1, 1.0, TOP, r#"stroke="none""#),
        dots(&[b, c]),
    ]
}

fn via() -> Vec<String> {
    // A via joins a track on this layer (solid) to one on another (faded); placed by its centre.
    let w = format!(r#"stroke-width="{}""#, f(TRACK));
    els![
        polyline(&[(3.5, 16.5), (7.5, 16.5), (12.0, 12.0)], &w),
        polyline(
            &[(12.0, 12.0), (16.5, 7.5), (20.5, 7.5)],
            &format!("{w} {MUTED}")
        ),
        circle(12.0, 12.0, 4.2, FILL, ""),
        circle(12.0, 12.0, 2.0, TOP, ""),
        dot(12.0, 12.0),
    ]
}

const ZONE: [P; 4] = [(4.0, 4.5), (19.0, 3.5), (20.5, 17.5), (6.0, 20.5)];

fn copper_zone() -> Vec<String> {
    // A filled zone drawn by its corners, poured around a pad with a clearance gap.
    els![
        poly(&ZONE, MID, r#"fill-opacity="0.7""#),
        circle(13.0, 12.5, 2.8, TOP, r#"stroke="none""#),
        circle(13.0, 12.5, 1.2, INK, r#"stroke="none""#),
        dots(&ZONE),
    ]
}

fn keepout_zone() -> Vec<String> {
    // A hatched zone drawn by its corners: nothing may be placed or routed inside it.
    els![hatch(&ZONE), poly(&ZONE, "", ""), dots(&ZONE)]
}

fn fill_zones() -> Vec<String> {
    // A zone outline poured with copper (accent), the copper keeping clear of a pad.
    let outline = [(3.0, 4.0), (21.0, 4.0), (21.0, 20.0), (3.0, 20.0)];
    let inset = d(
        &[(6.25, 7.25), (17.75, 7.25), (17.75, 16.75), (6.25, 16.75)],
        true,
    );
    let hole = "M8.75 12 A3.25 3.25 0 1 0 15.25 12 A3.25 3.25 0 1 0 8.75 12 Z";
    els![
        path(&d(&outline, true), "", r#"stroke-dasharray="3 2.6""#),
        path(
            &format!("{inset} {hole}"),
            ACCENT,
            r#"fill-rule="evenodd" stroke="none""#
        ),
        circle(12.0, 12.0, 1.25, INK, r#"stroke="none""#),
    ]
}

fn board_outline() -> Vec<String> {
    // The board's edge drawn as a closed outline (a chamfered corner), with two mounting holes.
    let pts = [
        (3.5, 5.0),
        (20.5, 5.0),
        (20.5, 14.0),
        (15.0, 19.5),
        (3.5, 19.5),
    ];
    els![
        poly(&pts, FILL, REGION),
        circle(7.0, 8.5, 1.3, TOP, r#"stroke-width="1""#),
        circle(7.0, 16.0, 1.3, TOP, r#"stroke-width="1""#),
        dots(&[pts[0], pts[2], pts[3]]),
    ]
}

fn ratsnest() -> Vec<String> {
    // Unrouted connections: straight accent lines between the pads of two parts.
    let (a, b, c, d) = ((5.0, 6.0), (5.0, 18.0), (19.0, 6.0), (19.0, 18.0));
    let pads: Vec<String> = [a, b, c, d]
        .iter()
        .map(|&(x, y)| rect(x - 2.0, y - 2.0, 4.0, 4.0, 0.75, INK, r#"stroke-width="1""#))
        .collect();
    let wires = path(
        "M7.5 6.75 L16.5 16.75 M7.5 18 H16.5",
        "",
        &format!(r#"stroke="{ACCENT}" stroke-width="1.25""#),
    );
    els![wires, pads]
}

fn drc() -> Vec<String> {
    // Two tracks side by side (the clearance between them is what the rules check) and a check.
    els![
        path("M3 4.5 H10 L13 7.5 H21 M3 10 H8 L11 13 H21", "", ""),
        acc(check(10.5, 20.0))
    ]
}

fn layers() -> Vec<String> {
    // A stack of board layers; the active one on top (accent).
    els![
        polyline(&[(3.0, 12.5), (12.0, 17.0), (21.0, 12.5)], ""),
        polyline(&[(3.0, 16.5), (12.0, 21.0), (21.0, 16.5)], ""),
        acc(poly(
            &[(12.0, 3.5), (21.0, 8.0), (12.0, 12.5), (3.0, 8.0)],
            "",
            ""
        )),
    ]
}

fn flip_side() -> Vec<String> {
    // The board edge-on, a part on top, and the arrow round the board's edge that puts it on the
    // bottom (faded).
    let ring = arc_points_step(15.5, 12.0, 5.5, 5.5, -90.0, 90.0, 10.0);
    let n = ring.len();
    els![
        line(3.0, 12.0, 15.5, 12.0, r#"stroke-width="2.5""#),
        rect(5.0, 5.75, 7.0, 3.5, 0.75, "", ""),
        rect(5.0, 14.75, 7.0, 3.5, 0.75, "", r#"stroke-opacity="0.45""#),
        acc_all(els![
            polyline(&ring, ""),
            head_at(ring[n - 1], ring[n - 2], 2.25)
        ]),
    ]
}

fn highlight_net() -> Vec<String> {
    // Three nets; the highlighted one (accent) stands out while the others fade.
    let faded = r#"stroke-opacity="0.4""#;
    els![
        path("M3 5 H21", "", faded),
        path("M3 19 H21", "", faded),
        acc(path("M3 12 H8 L11 9 H21", "", ""))
    ]
}

fn select_connected() -> Vec<String> {
    // A click (the pointer) on a track selects everything connected to it (accent).
    let tip = (6.0, 8.5);
    let cursor = [
        (0.0, 0.0),
        (0.0, 10.5),
        (2.7, 7.9),
        (4.6, 11.4),
        (6.4, 10.5),
        (4.5, 7.1),
        (8.2, 7.1),
    ];
    els![
        acc(path("M3 5 H13.5 L17.5 9 V20.5", "", "")),
        poly(
            &cursor.map(|(x, y)| (tip.0 + x, tip.1 + y)),
            "",
            r#"stroke-width="1.5""#
        ),
    ]
}

fn plot_gerbers() -> Vec<String> {
    // A fabrication file: a document with copper artwork on it, a track between two pads (accent).
    els![
        doc(),
        acc_all(els![
            path("M8.75 15.75 L12.25 12.25 H13.5", "", ""),
            circle(8.25, 17.0, 1.4, "", ""),
            circle(15.0, 12.25, 1.4, "", "")
        ]),
    ]
}

fn drill_file() -> Vec<String> {
    // A drill file: a document listing holes (accent rings).
    let holes: Vec<String> = [(9.25, 12.0), (14.75, 12.0), (9.25, 17.0), (14.75, 17.0)]
        .iter()
        .map(|&(x, y)| circle(x, y, 1.4, "", ""))
        .collect();
    els![doc(), acc_all(holes)]
}

fn board_3d() -> Vec<String> {
    // A populated board in 3D (a chip and a capacitor on a thin board) and the orbit arrow
    // around it (accent).
    let o = (12.0, 9.0);
    let p = projector(o);
    let t = 1.2;
    let (rx, ry) = (7.6 * 1.5f64.sqrt(), 7.6 * 0.5f64.sqrt());
    let (cx, cy) = p(5.5, 5.5, 0.0);
    let ring = arc_points(cx, cy + 1.4, rx, ry, 160.0, 20.0);
    els![
        iso_box_with(o, 0.0, 11.0, 0.0, 11.0, 0.0, t, SOFT, MID, SHADE, ""),
        iso_box(o, 1.5, 6.5, 4.5, 9.5, t, t + 1.8),
        cylinder(o, 8.0, 3.0, t, t + 4.5, 1.7),
        curved_arrow(&ring, 2.8, 2.6, ACCENT, ""),
    ]
}

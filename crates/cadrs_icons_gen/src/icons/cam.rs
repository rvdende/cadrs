//! CAM: the machining workflow of a CAM Studio (jobs, machines, setups, tools, toolpaths, work
//! planes) for mills, routers, lasers, plasma tables and waterjets.
//!
//! Three kinds, chosen as in the rest of the set:
//! - Solid for the things on the machine: the machines themselves, the cutters and beams, the
//!   stock, the vise, a setup (a part in the vise), a job (stock with a pocket being cut), a work
//!   plane (a plane with its axes) and Insert part. Neutral faces with INK outlines; the ACCENT is
//!   the cutting part (a flute, a beam, a jet, the pocket's floor, the Z axis, the plus).
//! - Sketch for the paths: a toolpath (a zigzag raster), a cut path (a contour with its lead-in and
//!   pierce) and a drill cycle (a hole with its pecks), flat at 1.5 with the tool's position as an
//!   ACCENT dot.
//! - Line for the commands: regenerate, preview and post, ink at 1.75 with one accent modifier.

use crate::common::*;
use Kind::{Line, Sketch, Solid};

pub fn icons() -> Vec<Def> {
    vec![
        def(
            "cam-insert",
            Solid,
            "Insert part, workholding and stock",
            cam_insert,
        ),
        def("cam-job", Solid, "Machining job", cam_job),
        def("cam-machine", Solid, "Milling machine", cam_machine),
        def("cam-router", Solid, "CNC router", cam_router),
        def("cam-laser", Solid, "Laser cutter", cam_laser),
        def("cam-plasma", Solid, "Plasma cutter", cam_plasma),
        def("cam-waterjet", Solid, "Waterjet", cam_waterjet),
        def("cam-setup", Solid, "Setup", cam_setup),
        def("cam-workholding", Solid, "Workholding", cam_workholding),
        def("cam-stock", Solid, "Stock", cam_stock),
        def("cam-tool", Solid, "Cutting tool", cam_tool),
        def("cam-drill", Solid, "Drill", cam_drill),
        def("cam-work-plane", Solid, "Work plane", cam_work_plane),
        def("cam-toolpath", Sketch, "Toolpath", cam_toolpath),
        def("cam-cut-path", Sketch, "Cut path", cam_cut_path),
        def("cam-drill-cycle", Sketch, "Drill cycle", cam_drill_cycle),
        def("cam-face", Sketch, "Face toolpath", cam_face),
        def("cam-pocket", Sketch, "Rough (pocket) toolpath", cam_pocket),
        def("cam-adaptive", Sketch, "Adaptive clearing", cam_adaptive),
        def("cam-profile", Sketch, "Profile toolpath", cam_profile),
        def("cam-chamfer", Sketch, "Chamfer toolpath", cam_chamfer),
        def("cam-engrave", Sketch, "Engrave toolpath", cam_engrave),
        def("cam-parallel", Sketch, "Parallel finishing", cam_parallel),
        def(
            "cam-waterline",
            Sketch,
            "Constant Z (waterline) finishing",
            cam_waterline,
        ),
        def("cam-scallop", Sketch, "Scallop finishing", cam_scallop),
        def("cam-spiral", Sketch, "Spiral finishing", cam_spiral),
        def("cam-pencil", Sketch, "Pencil finishing", cam_pencil),
        def("cam-bore", Sketch, "Helical bore", cam_bore),
        def("cam-thread", Sketch, "Thread milling", cam_thread),
        def(
            "cam-flatlands",
            Sketch,
            "Flatlands (flat floors)",
            cam_flatlands,
        ),
        def("cam-etch", Sketch, "Etch (marking along edges)", cam_etch),
        def("cam-fill", Sketch, "Fill (hatching a face)", cam_fill),
        def(
            "cam-regenerate",
            Line,
            "Regenerate toolpaths",
            cam_regenerate,
        ),
        def("cam-preview", Line, "Preview operation", cam_preview),
        def("cam-post", Line, "Post to G-code", cam_post),
    ]
}

/// An ACCENT stroke of width `w`.
fn acc(w: f64) -> String {
    format!(r#"stroke="{ACCENT}" stroke-width="{}""#, f(w))
}

/// Recolours a line-icon element in ACCENT.
fn acc_el(el: String) -> String {
    el.replace(&format!(r#"fill="{INK}""#), &format!(r#"fill="{ACCENT}""#))
        .replacen("/>", &format!(r#" stroke="{ACCENT}"/>"#), 1)
}

/// An accent dot (the tool's position on a path).
fn dot(x: f64, y: f64) -> String {
    circle(x, y, 1.6, ACCENT, r#"stroke="none""#)
}

/// An upright end mill at (cx, cy) from z0 (its tip) to z1, radius r: the shank plain, the
/// flutes with ACCENT helix strokes.
fn end_mill(o: P, cx: f64, cy: f64, z0: f64, z1: f64, r: f64, flutes: f64) -> Vec<String> {
    let p = projector(o);
    let rx = r * 1.5f64.sqrt();
    let (bx, _) = p(cx, cy, z0);
    let zf = z0 + flutes;
    let mut out = vec![cylinder(o, cx, cy, z0, z1, r)];
    // Helix strokes across the flutes, down to the right.
    let n = 3;
    for k in 0..n {
        let z = z0 + 0.6 + (flutes - 1.2) * (k as f64 + 0.5) / n as f64;
        let (_, y) = p(cx, cy, z);
        out.push(line(
            bx - rx * 0.92,
            y - 1.1,
            bx + rx * 0.92,
            y + 1.1,
            &acc(1.1),
        ));
    }
    let (_, yf) = p(cx, cy, zf);
    out.push(ellipse(bx, yf, rx, r * 0.5f64.sqrt(), "", r#"fill="none""#));
    out
}

fn cam_insert() -> Vec<String> {
    // A part (a block with a hole) and a plus: bring it into the studio.
    let o = (10.0, 11.5);
    let (cx, cy) = (3.0, 3.5);
    let top = iso_circle(o, cx, cy, 5.0, 1.4, 24);
    els![
        iso_box(o, 0.0, 7.0, 0.0, 6.5, 0.0, 5.0),
        poly(&top, SHADE, ""),
        line(18.0, 3.2, 18.0, 9.8, &acc(1.9)),
        line(14.7, 6.5, 21.3, 6.5, &acc(1.9)),
    ]
}

fn cam_job() -> Vec<String> {
    // The stock with a pocket being cut: its floor is what the job makes.
    let o = (12.0, 8.6);
    let p = projector(o);
    let (s, h, d) = (9.0, 5.0, 2.2);
    let (a, b) = (2.0, 7.0);
    let z = h - d;
    let rim = [p(a, a, h), p(b, a, h), p(b, b, h), p(a, b, h)];
    els![
        iso_box(o, 0.0, s, 0.0, s, 0.0, h),
        poly(
            &clip(&[p(a, a, h), p(b, a, h), p(b, a, z), p(a, a, z)], &rim),
            SHADE,
            ""
        ),
        poly(
            &clip(&[p(a, a, h), p(a, b, h), p(a, b, z), p(a, a, z)], &rim),
            MID,
            ""
        ),
        poly(
            &clip(&[p(a, a, z), p(b, a, z), p(b, b, z), p(a, b, z)], &rim),
            ACCENT,
            ""
        ),
    ]
}

fn cam_machine() -> Vec<String> {
    // A vertical mill: base and table, the column at the back, the head reaching over the table
    // with its spindle and the cutter (accent) down to the work.
    let o = (11.0, 7.0);
    els![
        iso_box(o, 0.0, 10.0, 0.0, 8.0, 0.0, 2.0),
        iso_box(o, 0.0, 3.0, 0.0, 8.0, 2.0, 13.0),
        iso_box(o, 3.0, 8.5, 2.5, 5.5, 9.5, 12.5),
        cylinder(o, 6.8, 4.0, 6.4, 9.5, 1.2),
        cylinder_with(o, 6.8, 4.0, 3.2, 6.4, 0.55, ACCENT, ACCENT, ""),
        iso_box(o, 4.5, 9.5, 1.5, 6.5, 2.0, 3.0),
    ]
}

fn cam_router() -> Vec<String> {
    // A gantry router: the bed, the gantry's two legs and beam, the spindle with its bit (accent).
    let o = (12.0, 5.6);
    els![
        iso_box(o, 0.0, 11.0, 0.0, 9.0, 0.0, 1.6),
        iso_box(o, 4.0, 5.6, 0.0, 1.4, 1.6, 9.5),
        iso_box(o, 4.0, 5.6, 1.4, 9.0, 8.0, 9.5),
        iso_box(o, 4.0, 5.6, 7.6, 9.0, 1.6, 8.0),
        iso_box(o, 5.6, 7.6, 3.6, 5.6, 6.0, 10.5),
        cylinder_with(o, 6.6, 4.6, 2.6, 6.0, 0.5, ACCENT, ACCENT, ""),
    ]
}

/// A sheet on the table (a thin slab) for the cutting machines.
fn sheet(o: P) -> String {
    iso_box(o, 0.0, 11.0, 0.0, 9.0, 0.0, 1.2)
}

fn cam_laser() -> Vec<String> {
    // A laser head over a sheet, its beam (accent) down to the kerf it cuts.
    let o = (12.0, 9.0);
    let p = projector(o);
    let (bx, by) = p(5.5, 4.5, 1.2);
    let (hx, hy) = p(5.5, 4.5, 6.0);
    let kerf = [p(1.5, 4.5, 1.2), p(5.5, 4.5, 1.2)];
    els![
        sheet(o),
        seg(kerf[0], kerf[1], &acc(1.3)),
        iso_box(o, 3.8, 7.2, 2.8, 6.2, 6.0, 10.5),
        line(hx, hy, bx, by, &acc(1.5)),
        circle(bx, by, 1.2, ACCENT, r#"stroke="none""#),
    ]
}

fn cam_plasma() -> Vec<String> {
    // A plasma torch over a sheet, its arc (an accent zigzag) down to the work.
    let o = (12.0, 9.0);
    let p = projector(o);
    let (bx, by) = p(5.5, 4.5, 1.2);
    let (tx, ty) = p(5.5, 4.5, 6.2);
    let n = 4;
    let zig: Vec<P> = (0..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let dx = if k == 0 || k == n {
                0.0
            } else if k % 2 == 1 {
                0.9
            } else {
                -0.9
            };
            (tx + (bx - tx) * t + dx, ty + (by - ty) * t)
        })
        .collect();
    els![
        sheet(o),
        polyline(&zig, &acc(1.3)),
        cylinder(o, 5.5, 4.5, 6.2, 12.0, 1.6),
        circle(bx, by, 1.1, ACCENT, r#"stroke="none""#),
    ]
}

fn cam_waterjet() -> Vec<String> {
    // A waterjet nozzle over a sheet, its jet (accent, dashed) down to the work.
    let o = (12.0, 9.0);
    let p = projector(o);
    let (bx, by) = p(5.5, 4.5, 1.2);
    let (tx, ty) = p(5.5, 4.5, 6.4);
    els![
        sheet(o),
        line(
            tx,
            ty,
            bx,
            by,
            &format!(r#"stroke="{ACCENT}" {}"#, dash_with(1.4, 1.2, 0.8))
        ),
        cylinder(o, 5.5, 4.5, 8.4, 12.0, 2.0),
        cylinder(o, 5.5, 4.5, 6.4, 8.4, 0.9),
    ]
}

/// A machine vise's two jaws and base, open by `gap` along x.
fn vise(o: P, gap: f64) -> Vec<String> {
    let w = 2.4;
    els![
        iso_box(o, 0.0, 11.0, 0.0, 6.0, 0.0, 1.6),
        iso_box(o, 0.0, w, 0.0, 6.0, 1.6, 5.4),
        iso_box(o, w + gap, w + gap + w, 0.0, 6.0, 1.6, 5.4),
    ]
}

fn cam_setup() -> Vec<String> {
    // A part held in the vise, its work coordinate system's Z (accent) up from its top.
    let o = (11.6, 9.4);
    let p = projector(o);
    let (zx, zy) = p(5.8, 3.0, 6.6);
    els![
        vise(o, 6.8),
        iso_box(o, 2.4, 9.2, 0.6, 5.4, 1.6, 6.6),
        arrow(
            zx,
            zy,
            zx,
            zy - 6.6,
            2.4,
            2.4,
            ACCENT,
            r#"stroke-width="1.5""#
        ),
    ]
}

fn cam_workholding() -> Vec<String> {
    // A machine vise, its jaws' gripping faces in accent.
    let o = (11.6, 9.4);
    let p = projector(o);
    let face = [
        p(2.4, 0.0, 5.4),
        p(2.4, 6.0, 5.4),
        p(2.4, 6.0, 1.6),
        p(2.4, 0.0, 1.6),
    ];
    els![vise(o, 5.2), poly(&face, ACCENT, "")]
}

fn cam_stock() -> Vec<String> {
    // A block of raw material with the part to come out of it dashed in accent on its top.
    let o = (12.0, 7.4);
    let p = projector(o);
    let h = 6.5;
    let part = [
        p(2.0, 2.0, h),
        p(7.0, 2.0, h),
        p(7.0, 7.0, h),
        p(2.0, 7.0, h),
    ];
    els![
        iso_box(o, 0.0, 9.0, 0.0, 9.0, 0.0, h),
        poly(
            &part,
            "",
            &format!(r#"fill="none" stroke="{ACCENT}" {}"#, dash(1.2))
        ),
    ]
}

fn cam_tool() -> Vec<String> {
    let o = (12.0, 18.6);
    end_mill(o, 0.0, 0.0, 0.0, 15.5, 2.9, 8.0)
}

fn cam_drill() -> Vec<String> {
    // A twist drill: the shank, the fluted body (accent helix) and its point.
    let o = (12.0, 16.4);
    let p = projector(o);
    let r = 2.6;
    let rx = r * 1.5f64.sqrt();
    let (bx, by) = p(0.0, 0.0, 0.0);
    let tip = (bx, by + 3.0);
    let mut v = end_mill(o, 0.0, 0.0, 0.0, 13.5, r, 7.0);
    v.insert(0, poly(&[(bx - rx, by), tip, (bx + rx, by)], MID, ""));
    v
}

fn cam_work_plane() -> Vec<String> {
    // A plane with its axes: X and Y in ink along it, Z (the tool axis) up in accent.
    let o = (12.0, 10.4);
    let p = projector(o);
    let plane = [
        p(-5.0, -5.0, 0.0),
        p(5.0, -5.0, 0.0),
        p(5.0, 5.0, 0.0),
        p(-5.0, 5.0, 0.0),
    ];
    let c = p(0.0, 0.0, 0.0);
    let x = p(4.6, 0.0, 0.0);
    let y = p(0.0, 4.6, 0.0);
    let z = p(0.0, 0.0, 7.6);
    els![
        poly(&plane, TOP, ""),
        arrow(c.0, c.1, x.0, x.1, 2.2, 2.2, INK, r#"stroke-width="1.2""#),
        arrow(c.0, c.1, y.0, y.1, 2.2, 2.2, INK, r#"stroke-width="1.2""#),
        arrow(
            c.0,
            c.1,
            z.0,
            z.1,
            2.4,
            2.4,
            ACCENT,
            r#"stroke-width="1.5""#
        ),
    ]
}

fn cam_toolpath() -> Vec<String> {
    // A pocket's boundary and the zigzag passes clearing it; the tool's position in accent.
    let (x0, x1, y0, y1) = (3.0, 21.0, 4.0, 20.0);
    let rows = [7.0, 10.3, 13.6, 16.9];
    let mut zig: Vec<P> = Vec::new();
    for (k, y) in rows.iter().enumerate() {
        let (a, b) = if k % 2 == 0 { (6.0, 18.0) } else { (18.0, 6.0) };
        zig.push((a, *y));
        zig.push((b, *y));
    }
    let end = *zig.last().unwrap();
    els![
        rect(x0, y0, x1 - x0, y1 - y0, 2.0, "", ""),
        polyline(&zig, ""),
        dot(end.0, end.1),
    ]
}

fn cam_cut_path() -> Vec<String> {
    // A part's contour cut by a beam: the lead-in arc from the pierce (accent) onto the contour.
    let c = (13.0, 12.5);
    let (w, h) = (15.0, 13.0);
    let lead = arc_points(8.5, 9.2, 3.4, 3.4, 180.0, 270.0);
    let start = lead[0];
    els![
        rect(c.0 - w / 2.0, c.1 - h / 2.0, w, h, 3.0, "", ""),
        polyline(&lead, &acc(1.5)),
        dot(start.0, start.1),
    ]
}

fn cam_drill_cycle() -> Vec<String> {
    // A hole in section and the pecks going into it: down, back out, deeper (accent).
    let pecks: Vec<P> = vec![
        (12.0, 2.5),
        (12.0, 9.5),
        (12.0, 6.5),
        (12.0, 13.5),
        (12.0, 10.5),
        (12.0, 17.5),
    ];
    let peck_line: Vec<String> = pecks
        .windows(2)
        .enumerate()
        .map(|(k, w)| {
            let dx = if k % 2 == 0 { 0.0 } else { 1.4 };
            line(w[0].0 + dx, w[0].1, w[1].0 + dx, w[1].1, &acc(1.5))
        })
        .collect();
    els![
        polyline(
            &[
                (3.0, 6.0),
                (8.0, 6.0),
                (8.0, 20.0),
                (16.0, 20.0),
                (16.0, 6.0),
                (21.0, 6.0)
            ],
            ""
        ),
        peck_line,
        dot(12.0, 17.5),
    ]
}

fn cam_face() -> Vec<String> {
    // The stock's top and passes across it that run off both sides.
    let rows = [7.5, 11.5, 15.5];
    let mut zig: Vec<P> = Vec::new();
    for (k, y) in rows.iter().enumerate() {
        let (a, b) = if k % 2 == 0 { (2.0, 22.0) } else { (22.0, 2.0) };
        zig.push((a, *y));
        zig.push((b, *y));
    }
    els![
        rect(5.0, 5.0, 14.0, 13.0, 1.0, "", &dash(1.2)),
        polyline(&zig, ""),
        dot(2.0, 15.5),
    ]
}

fn cam_pocket() -> Vec<String> {
    // A pocket's wall and the rings clearing it, from the middle out.
    els![
        rect(3.0, 4.0, 18.0, 16.0, 3.0, "", ""),
        rect(6.5, 7.5, 11.0, 9.0, 2.0, "", ""),
        rect(10.0, 11.0, 4.0, 2.0, 1.0, "", ""),
        dot(17.5, 12.0),
    ]
}

fn cam_adaptive() -> Vec<String> {
    // A slot cleared by little loops that each take the same bite (constant engagement), the
    // last one still cutting.
    let loops: Vec<P> = (0..=90)
        .map(|k| {
            let t = k as f64 / 90.0;
            let a = t * std::f64::consts::TAU * 4.0;
            (5.0 + 13.0 * t + 3.2 * a.cos(), 12.0 + 3.6 * a.sin())
        })
        .collect();
    let end = *loops.last().unwrap();
    els![
        line(2.0, 6.5, 22.0, 6.5, ""),
        line(2.0, 17.5, 22.0, 17.5, ""),
        polyline(&loops, &acc(1.3)),
        dot(end.0, end.1),
    ]
}

fn cam_profile() -> Vec<String> {
    // A part's outline and the cutter's path round it a radius out, led in on an arc.
    let lead = arc_points(4.5, 15.5, 3.0, 3.0, 90.0, 180.0);
    let start = lead[0];
    els![
        rect(7.0, 6.5, 12.0, 11.0, 1.0, SOFT, ""),
        rect(4.5, 4.0, 17.0, 16.0, 3.5, "", &acc(1.5)),
        polyline(&lead, ""),
        dot(start.0, start.1),
    ]
}

fn cam_chamfer() -> Vec<String> {
    // A corner in section with its edge broken: the chamfer the cone cuts, in accent.
    els![
        polyline(&[(3.0, 8.0), (12.0, 8.0), (18.0, 14.0), (18.0, 21.0)], ""),
        line(12.0, 8.0, 18.0, 14.0, &acc(2.2)),
        dot(17.0, 9.0),
    ]
}

fn cam_engrave() -> Vec<String> {
    // A letter's stroke engraved into a face.
    let s = sample_cubic((5.0, 17.0), (5.0, 3.0), (19.0, 21.0), (19.0, 7.0), 24);
    let end = *s.last().unwrap();
    els![
        rect(2.5, 2.5, 19.0, 19.0, 2.0, "", &dash(1.2)),
        polyline(&s, &acc(1.6)),
        dot(end.0, end.1),
    ]
}

fn cam_parallel() -> Vec<String> {
    // A dome seen from above, diagonal raster passes laid over it.
    let mut out = vec![circle(12.0, 12.0, 8.5, SOFT, "")];
    let mut end = (0.0, 0.0);
    for k in -3i32..=3 {
        // Lines along (1, −1), a step apart across the dome.
        let o = k as f64 * 2.4 / std::f64::consts::SQRT_2;
        let l = (8.5f64.powi(2) - (o * std::f64::consts::SQRT_2).powi(2))
            .max(0.0)
            .sqrt()
            / std::f64::consts::SQRT_2;
        let (a, b) = ((12.0 + o - l, 12.0 + o + l), (12.0 + o + l, 12.0 + o - l));
        out.push(line(a.0, a.1, b.0, b.1, ""));
        end = b;
    }
    els![out, dot(end.0, end.1)]
}

fn cam_waterline() -> Vec<String> {
    // A boss from the side with level passes round it, one a level.
    els![
        polyline(&[(5.0, 21.0), (8.0, 5.0), (16.0, 5.0), (19.0, 21.0)], ""),
        ellipse(12.0, 9.5, 5.6, 1.6, "", r#"fill="none""#),
        ellipse(12.0, 14.0, 6.4, 1.8, "", r#"fill="none""#),
        ellipse(12.0, 18.5, 7.2, 2.0, "", &acc(1.5)),
        dot(4.8, 18.5),
    ]
}

fn cam_scallop() -> Vec<String> {
    // Rings an even step apart across a free-form face.
    let blob = |s: f64| -> Vec<P> {
        (0..=48)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / 48.0;
                let r = s * (1.0 + 0.18 * (3.0 * a).sin());
                (12.0 + r * a.cos(), 12.0 + r * a.sin())
            })
            .collect()
    };
    els![
        polyline(&blob(8.5), ""),
        polyline(&blob(5.5), ""),
        polyline(&blob(2.5), &acc(1.5)),
        dot(14.5, 12.0),
    ]
}

fn cam_spiral() -> Vec<String> {
    // One spiral out from the middle.
    let s: Vec<P> = (0..=120)
        .map(|k| {
            let a = k as f64 / 120.0 * std::f64::consts::TAU * 3.2;
            let r = 0.4 + 9.0 * k as f64 / 120.0;
            (12.0 + r * a.cos(), 12.0 + r * a.sin())
        })
        .collect();
    let end = *s.last().unwrap();
    els![polyline(&s, ""), dot(end.0, end.1)]
}

fn cam_pencil() -> Vec<String> {
    // Two faces meeting in a concave corner, the pass along it in accent.
    els![
        poly(
            &[(3.0, 20.0), (10.0, 13.0), (21.0, 13.0), (14.0, 20.0)],
            SOFT,
            ""
        ),
        poly(
            &[(10.0, 13.0), (10.0, 3.0), (21.0, 3.0), (21.0, 13.0)],
            MID,
            ""
        ),
        line(10.0, 13.0, 21.0, 13.0, &acc(2.0)),
        dot(21.0, 13.0),
    ]
}

fn cam_bore() -> Vec<String> {
    // A hole in section and the helix going down it.
    let mut helix: Vec<P> = Vec::new();
    for k in 0..=64 {
        let t = k as f64 / 64.0;
        let a = t * std::f64::consts::TAU * 3.0;
        helix.push((12.0 + 5.0 * a.cos(), 6.0 + 12.0 * t + 1.2 * a.sin()));
    }
    let end = *helix.last().unwrap();
    els![
        polyline(&[(2.5, 4.0), (5.5, 4.0), (5.5, 21.0)], ""),
        polyline(&[(21.5, 4.0), (18.5, 4.0), (18.5, 21.0)], ""),
        polyline(&helix, &acc(1.3)),
        dot(end.0, end.1),
    ]
}

fn cam_thread() -> Vec<String> {
    // A tapped hole in section: the thread's teeth down both walls, the thread mill (a stem and
    // its one tooth) cutting them, the tooth in accent.
    let teeth = |x: f64, dir: f64| -> Vec<P> {
        (0..=10)
            .map(|k| {
                (
                    x + if k % 2 == 0 { 0.0 } else { dir * 2.2 },
                    4.0 + 1.7 * k as f64,
                )
            })
            .collect()
    };
    els![
        polyline(&teeth(5.5, 1.0), ""),
        polyline(&teeth(18.5, -1.0), ""),
        line(12.0, 2.5, 12.0, 14.9, ""),
        poly(
            &[(12.0, 11.5), (16.2, 13.2), (12.0, 14.9)],
            ACCENT,
            r#"stroke="none""#
        ),
    ]
}

fn cam_flatlands() -> Vec<String> {
    // A step in section: only its flat floors get the passes (accent), the wall none.
    els![
        polyline(&[(2.0, 9.0), (11.0, 9.0), (11.0, 17.0), (22.0, 17.0)], ""),
        line(3.0, 6.5, 9.5, 6.5, &acc(1.8)),
        line(12.5, 14.5, 21.0, 14.5, &acc(1.8)),
        dot(21.0, 14.5),
    ]
}

fn cam_etch() -> Vec<String> {
    // A part's outline marked by the beam, dashed (on the line, no kerf).
    els![
        rect(4.0, 5.0, 16.0, 14.0, 3.0, "", &dash(1.6)),
        circle(12.0, 12.0, 2.6, "", ""),
        dot(4.0, 12.0),
    ]
}

fn cam_fill() -> Vec<String> {
    // A face hatched by the beam, line after line.
    let mut out = vec![circle(12.0, 12.0, 8.5, "", "")];
    for k in 0..6 {
        let y = 6.5 + 2.2 * k as f64;
        let dy = (y - 12.0f64).abs();
        let half = (8.5f64.powi(2) - dy * dy).max(0.0).sqrt() - 1.2;
        out.push(line(12.0 - half, y, 12.0 + half, y, &acc(1.2)));
    }
    els![out, dot(12.0 - 6.0, 17.5)]
}

fn cam_regenerate() -> Vec<String> {
    // Two arrows round a loop (calculate again) around a short zigzag path in accent.
    let zig = [(9.0, 14.0), (11.0, 10.0), (13.0, 14.0), (15.0, 10.0)];
    els![
        path("M5 12 A7 7 0 0 1 17.5 7.6", "", ""),
        polyline(&[(17.8, 4.2), (17.8, 7.9), (14.2, 7.9)], ""),
        path("M19 12 A7 7 0 0 1 6.5 16.4", "", ""),
        polyline(&[(6.2, 19.8), (6.2, 16.1), (9.8, 16.1)], ""),
        acc_el(polyline(&zig, "")),
    ]
}

fn cam_preview() -> Vec<String> {
    // A play button over a toolpath's line (accent): play the program back.
    els![
        poly(
            &[(9.0, 4.5), (18.5, 10.0), (9.0, 15.5)],
            "",
            r#"stroke-linejoin="round""#
        ),
        acc_el(polyline(
            &[
                (3.5, 19.5),
                (8.0, 19.5),
                (8.0, 17.0),
                (16.0, 17.0),
                (16.0, 19.5),
                (20.5, 19.5)
            ],
            ""
        )),
    ]
}

fn cam_post() -> Vec<String> {
    // A program (a page of code lines) with an arrow out of it: post the toolpaths.
    els![
        path(
            "M13 3 H6.5 A2.5 2.5 0 0 0 4 5.5 V18.5 A2.5 2.5 0 0 0 6.5 21 H15.5 A2.5 2.5 0 0 0 18 18.5 V14",
            "",
            ""
        ),
        line(7.5, 8.0, 12.5, 8.0, ""),
        line(7.5, 12.0, 11.0, 12.0, ""),
        line(7.5, 16.0, 13.5, 16.0, ""),
        acc_el(line(13.5, 9.5, 21.0, 2.5, "")),
        acc_el(polyline(&[(16.0, 2.5), (21.0, 2.5), (21.0, 7.5)], "")),
    ]
}

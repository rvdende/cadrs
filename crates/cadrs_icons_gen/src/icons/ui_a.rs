//! Interface navigation, action and status icons (kind Line).
//!
//! All strokes use the root width (1.75) so the app can raise the weight by rewriting it. Filled
//! shapes with knocked-out marks use stroke="none" and are drawn 0.875 larger than their outline
//! counterparts, so the knock-outs never close up when the root stroke gets heavier.

use crate::common::*;
use Kind::Line;
use std::f64::consts::PI;

pub fn icons() -> Vec<Def> {
    vec![
        def("undo", Line, "Undo", undo),
        def("redo", Line, "Redo", redo),
        def("chevron-down", Line, "Chevron down", chevron_down),
        def("chevron-up", Line, "Chevron up", chevron_up),
        def("chevron-left", Line, "Chevron left", chevron_left),
        def("chevron-right", Line, "Chevron right", chevron_right),
        def("caret-down", Line, "Caret down", caret_down),
        def("caret-right", Line, "Caret right", caret_right),
        def(
            "caret-up-filled",
            Line,
            "Caret up (filled)",
            caret_up_filled,
        ),
        def(
            "caret-down-filled",
            Line,
            "Caret down (filled)",
            caret_down_filled,
        ),
        def(
            "caret-left-filled",
            Line,
            "Caret left (filled)",
            caret_left_filled,
        ),
        def(
            "caret-right-filled",
            Line,
            "Caret right (filled)",
            caret_right_filled,
        ),
        def("close", Line, "Close", close),
        def("check", Line, "Check", check),
        def("plus", Line, "Add", plus),
        def("delete", Line, "Delete", delete),
        def("remove-circle", Line, "Remove", remove_circle),
        def("search", Line, "Search", search),
        def("filter", Line, "Filter", filter),
        def("sort-descending", Line, "Sort descending", sort_descending),
        def("reorder", Line, "Reorder", reorder),
        def("drag-handle", Line, "Drag handle", drag_handle),
        def("cursor", Line, "Cursor", cursor),
        def("arrow-up", Line, "Arrow up", arrow_up),
        def("arrow-down", Line, "Arrow down", arrow_down),
        def("arrow-up-right", Line, "Arrow up-right", arrow_up_right),
        def(
            "arrow-down-right",
            Line,
            "Arrow down-right",
            arrow_down_right,
        ),
        def("menu", Line, "Menu", menu),
        def("more-horizontal", Line, "More", more_horizontal),
        def("open-external", Line, "Open", open_external),
        def("link", Line, "Link", link),
        def("copy", Line, "Copy", copy),
        def("edit", Line, "Edit", edit),
        def("restore", Line, "Restore", restore),
        def("history", Line, "History", history),
        def("stopwatch", Line, "Regeneration time", stopwatch),
        def("pause", Line, "Pause", pause),
        def("stop", Line, "Stop", stop),
        def("play", Line, "Play", play),
        def("skip-back", Line, "Skip back", skip_back),
        def("skip-forward", Line, "Skip forward", skip_forward),
        def("repeat", Line, "Repeat", repeat),
        def("volume", Line, "Volume", volume),
        def("volume-mute", Line, "Mute", volume_mute),
        def("music", Line, "Music", music),
        def("visible", Line, "Visible", visible),
        def("hidden", Line, "Hidden", hidden),
        def("lock-filled", Line, "Locked", lock_filled),
        def("lock-open", Line, "Unlocked", lock_open),
        def("info", Line, "Info", info),
        def("info-filled", Line, "Info (filled)", info_filled),
        def("help", Line, "Help", help),
        def("help-filled", Line, "Help (filled)", help_filled),
        def("error-filled", Line, "Error", error_filled),
        def("warning-filled", Line, "Warning", warning_filled),
        def("in-context", Line, "In context (primary)", in_context),
        def(
            "in-context-secondary",
            Line,
            "In context (secondary)",
            in_context_secondary,
        ),
        def("home", Line, "Home", home),
        def("notifications", Line, "Notifications", notifications),
        def("settings", Line, "Settings", settings),
        def("share", Line, "Share", share),
        def("public", Line, "Public", public),
        def("cloud-upload", Line, "Upload to cloud", cloud_upload),
        def("upload", Line, "Upload", upload),
        def("download", Line, "Download", download),
        def("network", Line, "Network", network),
        def("network-rx", Line, "Network receiving", network_rx),
        def("network-tx", Line, "Network sending", network_tx),
        def(
            "network-rx-tx",
            Line,
            "Network receiving and sending",
            network_rx_tx,
        ),
        def("network-error", Line, "Network error", network_error),
        def(
            "network-disconnected",
            Line,
            "Network disconnected",
            network_disconnected,
        ),
        def("theme-light", Line, "Light theme", theme_light),
        def("theme-dark", Line, "Dark theme", theme_dark),
        def("theme-system", Line, "System theme", theme_system),
        def("flip-direction", Line, "Flip direction", flip_direction),
        def(
            "flip-direction-up",
            Line,
            "Flip direction (flipped)",
            flip_direction_up,
        ),
    ]
}

const HEAD: f64 = 5.0; // open arrowhead: each leg runs 5 back along the shaft and 5 out to the side
const R_OUT: f64 = 9.875; // a filled circle as large as a stroked r 9 circle

// ---------------------------------------------------------------------------------------------
// Private helpers

/// An open 45-degree arrowhead at `tip`, pointing away from `frm`.
fn head(tip: P, frm: P, s: f64) -> String {
    let (dx, dy) = (tip.0 - frm.0, tip.1 - frm.1);
    let l = or1(hypot(dx, dy));
    let (ux, uy) = (dx / l, dy / l);
    let (nx, ny) = (-uy, ux);
    let a = (tip.0 - s * ux + s * nx, tip.1 - s * uy + s * ny);
    let b = (tip.0 - s * ux - s * nx, tip.1 - s * uy - s * ny);
    polyline(&[a, tip, b], "")
}

fn open_arrow(x0: f64, y0: f64, x1: f64, y1: f64, s: f64) -> Vec<String> {
    els![line(x0, y0, x1, y1, ""), head((x1, y1), (x0, y0), s)]
}

/// The element in ACCENT: stroke, and fill where it was INK.
fn acc(el: String) -> String {
    let el = el.replace(&format!(r#"fill="{INK}""#), &format!(r#"fill="{ACCENT}""#));
    format!(r#"{} stroke="{ACCENT}"/>"#, &el[..el.len() - 2])
}

fn acc_all(els: Vec<String>) -> Vec<String> {
    els.into_iter().map(acc).collect()
}

fn d(ps: &[P], close: bool) -> String {
    let body = ps
        .iter()
        .map(|&(x, y)| format!("{} {}", f(x), f(y)))
        .collect::<Vec<_>>()
        .join(" L");
    format!("M{body}{}", if close { " Z" } else { "" })
}

fn circle_d(cx: f64, cy: f64, r: f64) -> String {
    format!(
        "M{} {} A{} {} 0 1 0 {} {} A{} {} 0 1 0 {} {} Z",
        f(cx - r),
        f(cy),
        f(r),
        f(r),
        f(cx + r),
        f(cy),
        f(r),
        f(r),
        f(cx - r),
        f(cy)
    )
}

/// The outline of polyline `ps` stroked at width w with round caps, as a closed point list.
fn stroke_outline(ps: &[P], w: f64) -> Vec<P> {
    let h = w / 2.0;
    let (left, right) = (offset_polyline(ps, h), offset_polyline(ps, -h));

    // a semicircle at p, bulging away from q
    let cap = |p: P, q: P| {
        let a = (p.1 - q.1).atan2(p.0 - q.0);
        (-6..=6)
            .rev()
            .map(|k| (k * 15) as f64)
            .map(|t| (p.0 + h * (a + rad(t)).cos(), p.1 + h * (a + rad(t)).sin()))
            .collect::<Vec<P>>()
    };

    let n = ps.len();
    let c_end = cap(ps[n - 1], ps[n - 2]);
    let c_start = cap(ps[0], ps[1]);
    cat(&[
        &left,
        &c_end[1..c_end.len() - 1],
        &rev(&right),
        &c_start[1..c_start.len() - 1],
    ])
}

fn stem(x: f64, y0: f64, y1: f64) -> Vec<P> {
    stroke_outline(&[(x, y0), (x, y1)], 1.9)
}

/// Convex polygon `ps` with every corner rounded to radius r.
fn rounded(ps: &[P], r: f64) -> Vec<P> {
    let n = 6;
    let mut out = Vec::new();
    let k = ps.len();
    for i in 0..k {
        let (p0, p1, p2) = (ps[(i + k - 1) % k], ps[i], ps[(i + 1) % k]);
        let v1 = (p0.0 - p1.0, p0.1 - p1.1);
        let v2 = (p2.0 - p1.0, p2.1 - p1.1);
        let (l1, l2) = (hypot(v1.0, v1.1), hypot(v2.0, v2.1));
        let (u1, u2) = ((v1.0 / l1, v1.1 / l1), (v2.0 / l2, v2.1 / l2));
        let ang = (u1.0 * u2.0 + u1.1 * u2.1).clamp(-1.0, 1.0).acos();
        let t = r / (ang / 2.0).tan();
        let a = (p1.0 + u1.0 * t, p1.1 + u1.1 * t);
        let b = (p1.0 + u2.0 * t, p1.1 + u2.1 * t);
        let bis = (u1.0 + u2.0, u1.1 + u2.1);
        let bl = hypot(bis.0, bis.1);
        let dc = r / (ang / 2.0).sin();
        let c = (p1.0 + bis.0 / bl * dc, p1.1 + bis.1 / bl * dc);
        let a0 = (a.1 - c.1).atan2(a.0 - c.0);
        let mut a1 = (b.1 - c.1).atan2(b.0 - c.0);
        while a1 - a0 > PI {
            a1 -= 2.0 * PI;
        }
        while a0 - a1 > PI {
            a1 += 2.0 * PI;
        }
        out.extend((0..=n).map(|j| {
            let t = a0 + (a1 - a0) * j as f64 / n as f64;
            (c.0 + r * t.cos(), c.1 + r * t.sin())
        }));
    }
    out
}

/// One solid shape with holes (path data), no stroke.
fn knockout(outer_d: &str, holes: &[String]) -> String {
    let parts: Vec<&str> = std::iter::once(outer_d)
        .chain(holes.iter().map(String::as_str))
        .collect();
    path(
        &parts.join(" "),
        INK,
        r#"fill-rule="evenodd" stroke="none""#,
    )
}

/// Polylines for `ps` with the points within `gap` of segment a-b removed.
fn gapped(ps: &[P], a: P, b: P, gap: f64, closed: bool) -> Vec<String> {
    let dist = |p: P| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        hypot(p.0 - a.0 - t * dx, p.1 - a.1 - t * dy)
    };

    let (mut runs, mut cur): (Vec<Vec<P>>, Vec<P>) = (Vec::new(), Vec::new());
    for &p in ps {
        if dist(p) > gap {
            cur.push(p);
        } else if !cur.is_empty() {
            runs.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        if closed && !runs.is_empty() && dist(ps[0]) > gap {
            cur.extend_from_slice(&runs[0]);
            runs[0] = cur;
        } else {
            runs.push(cur);
        }
    }
    runs.iter()
        .filter(|r| r.len() > 1)
        .map(|r| polyline(r, ""))
        .collect()
}

/// `ps` with points added along every edge at most `step` apart, so `gapped` can cut straight
/// edges. A closed list gets points along its closing edge too, without repeating the first.
fn dense(ps: &[P], step: f64, closed: bool) -> Vec<P> {
    let n = ps.len();
    let edges = if closed { n } else { n - 1 };
    let mut out = Vec::new();
    for i in 0..edges {
        let (a, b) = (ps[i], ps[(i + 1) % n]);
        let k = ((hypot(b.0 - a.0, b.1 - a.1) / step).ceil() as usize).max(1);
        out.extend((0..k).map(|j| {
            let t = j as f64 / k as f64;
            (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t)
        }));
    }
    if !closed {
        out.push(ps[n - 1]);
    }
    out
}

fn eye_pts(n: usize) -> Vec<P> {
    let top = sample_cubic((2.75, 12.0), (6.0, 5.75), (18.0, 5.75), (21.25, 12.0), n);
    let bot = sample_cubic((21.25, 12.0), (18.0, 18.25), (6.0, 18.25), (2.75, 12.0), n);
    cat(&[&top, &bot[1..]])
}

/// The hook of the "?" from its upper-left end to the bottom of the stem.
fn question_pts() -> Vec<P> {
    let hook = arc_points_step(12.0, 9.5, 2.75, 2.75, 180.0, 395.0, 5.0);
    let stem = sample_cubic((14.1, 11.3), (12.6, 12.1), (12.0, 12.6), (12.0, 13.9), 8);
    cat(&[&hook, &stem[1..]])
}

fn trash_body(lid_accent: bool) -> Vec<String> {
    let lid = els![
        line(4.0, 7.0, 20.0, 7.0, ""),
        path(
            "M9 7 V5 A1.25 1.25 0 0 1 10.25 3.75 H13.75 A1.25 1.25 0 0 1 15 5 V7",
            "",
            ""
        ),
    ];
    let body = path(
        "M6 7 L6.85 18.2 A2 2 0 0 0 8.85 20.25 H15.15 A2 2 0 0 0 17.15 18.2 L18 7",
        "",
        "",
    );
    els![body, if lid_accent { acc_all(lid) } else { lid }]
}

// ---------------------------------------------------------------------------------------------
// History

fn undo() -> Vec<String> {
    els![
        path("M4 9.5 H15 A4.75 4.75 0 0 1 15 19 H11", "", ""),
        acc(head((4.0, 9.5), (10.0, 9.5), 4.5))
    ]
}

fn redo() -> Vec<String> {
    els![
        path("M20 9.5 H9 A4.75 4.75 0 0 0 9 19 H13", "", ""),
        acc(head((20.0, 9.5), (14.0, 9.5), 4.5))
    ]
}

// ---------------------------------------------------------------------------------------------
// Chevrons and carets

fn chevron_down() -> Vec<String> {
    els![polyline(&[(8.0, 10.0), (12.0, 14.0), (16.0, 10.0)], "")]
}

fn chevron_up() -> Vec<String> {
    els![polyline(&[(8.0, 14.0), (12.0, 10.0), (16.0, 14.0)], "")]
}

fn chevron_left() -> Vec<String> {
    els![polyline(&[(14.0, 8.0), (10.0, 12.0), (14.0, 16.0)], "")]
}

fn chevron_right() -> Vec<String> {
    els![polyline(&[(10.0, 8.0), (14.0, 12.0), (10.0, 16.0)], "")]
}

// Carets: a triangle 8 wide and 4.5 deep (pointing down around the origin), centred on 12,12.
const CARET: [P; 3] = [(-4.0, -2.0), (4.0, -2.0), (0.0, 2.5)];

fn caret(rot: f64, fill: &str) -> Vec<String> {
    let (c, s) = (
        rad(rot).cos().round_ties_even(),
        rad(rot).sin().round_ties_even(),
    );
    els![poly(
        &CARET.map(|(x, y)| (12.0 + x * c - y * s, 12.0 + x * s + y * c)),
        fill,
        ""
    )]
}

fn caret_down() -> Vec<String> {
    caret(0.0, "")
}

fn caret_right() -> Vec<String> {
    caret(-90.0, "")
}

fn caret_up_filled() -> Vec<String> {
    caret(180.0, INK)
}

fn caret_down_filled() -> Vec<String> {
    caret(0.0, INK)
}

fn caret_left_filled() -> Vec<String> {
    caret(90.0, INK)
}

fn caret_right_filled() -> Vec<String> {
    caret(-90.0, INK)
}

// ---------------------------------------------------------------------------------------------
// Actions

fn close() -> Vec<String> {
    els![
        line(6.0, 6.0, 18.0, 18.0, ""),
        line(18.0, 6.0, 6.0, 18.0, "")
    ]
}

fn check() -> Vec<String> {
    els![polyline(&[(4.5, 12.5), (9.5, 17.5), (19.5, 6.5)], "")]
}

fn plus() -> Vec<String> {
    els![
        line(12.0, 5.0, 12.0, 19.0, ""),
        line(5.0, 12.0, 19.0, 12.0, "")
    ]
}

fn delete() -> Vec<String> {
    els![
        trash_body(true),
        line(10.0, 11.0, 10.0, 16.25, ""),
        line(14.0, 11.0, 14.0, 16.25, "")
    ]
}

fn remove_circle() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        acc_all(els![
            line(9.0, 9.0, 15.0, 15.0, ""),
            line(15.0, 9.0, 9.0, 15.0, "")
        ])
    ]
}

fn search() -> Vec<String> {
    els![
        circle(10.5, 10.5, 6.5, "", ""),
        acc(line(15.25, 15.25, 20.0, 20.0, ""))
    ]
}

fn filter() -> Vec<String> {
    els![
        path("M10 11.75 L4 4.5 H20 L14 11.75", "", ""),
        acc(path("M10 11.75 V20 L14 18 V11.75", "", ""))
    ]
}

fn sort_descending() -> Vec<String> {
    els![
        line(4.0, 6.0, 12.0, 6.0, ""),
        line(4.0, 12.0, 10.0, 12.0, ""),
        line(4.0, 18.0, 8.0, 18.0, ""),
        acc_all(open_arrow(17.0, 5.0, 17.0, 19.0, 3.5)),
    ]
}

fn reorder() -> Vec<String> {
    // Up and down arrows side by side: toggles reordering a list. A control, so ink only.
    els![
        open_arrow(8.5, 19.0, 8.5, 5.0, 3.5),
        open_arrow(15.5, 5.0, 15.5, 19.0, 3.5)
    ]
}

fn cursor() -> Vec<String> {
    // A filled arrow pointer, its tip (the hotspot) at 5,2.5: a collaborator's pointer, which
    // apps tint in that person's colour. A single mark, so ink only.
    let arrow = [
        (5.0, 2.5),
        (17.8, 14.4),
        (12.3, 14.4),
        (15.2, 20.4),
        (12.2, 21.7),
        (9.3, 15.6),
        (5.0, 19.6),
    ];
    els![path(
        &d(&rounded(&arrow, 0.9), true),
        INK,
        r#"stroke="none""#
    )]
}

fn drag_handle() -> Vec<String> {
    // Two columns of three dots: grab here to drag. Filled dots, so they stay solid at 12 px.
    [9.0, 15.0]
        .iter()
        .flat_map(|&x| [6.0, 12.0, 18.0].map(|y| circle(x, y, 1.6, INK, r#"stroke="none""#)))
        .collect()
}

fn arrow_up() -> Vec<String> {
    open_arrow(12.0, 19.0, 12.0, 5.0, HEAD)
}

fn arrow_down() -> Vec<String> {
    open_arrow(12.0, 5.0, 12.0, 19.0, HEAD)
}

fn arrow_up_right() -> Vec<String> {
    open_arrow(6.5, 17.5, 17.0, 7.0, HEAD * 0.5f64.sqrt())
}

fn arrow_down_right() -> Vec<String> {
    open_arrow(6.5, 6.5, 17.0, 17.0, HEAD * 0.5f64.sqrt())
}

fn menu() -> Vec<String> {
    els![
        line(4.0, 6.0, 20.0, 6.0, ""),
        line(4.0, 12.0, 20.0, 12.0, ""),
        line(4.0, 18.0, 20.0, 18.0, "")
    ]
}

fn more_horizontal() -> Vec<String> {
    // The overflow menu's three dots (a single mark: INK only).
    [5.0, 12.0, 19.0]
        .iter()
        .map(|&x| circle(x, 12.0, 1.6, INK, r#"stroke-width="0.9""#))
        .collect()
}

fn open_external() -> Vec<String> {
    els![
        path(
            "M10.5 4 H6.5 A2.5 2.5 0 0 0 4 6.5 V17.5 A2.5 2.5 0 0 0 6.5 20 H17.5 A2.5 2.5 0 0 0 20 17.5 V13.5",
            "",
            ""
        ),
        acc_all(open_arrow(
            11.0,
            13.0,
            20.0,
            4.0,
            HEAD * 0.5f64.sqrt() + 0.5
        )),
    ]
}

fn link() -> Vec<String> {
    // Two open U-shaped links on the diagonal, joined by a short bar.
    let half = |cx: f64, cy: f64, s: f64| {
        let (r, leg) = (3.6, 3.2);
        let (ux, uy) = (s * 0.5f64.sqrt(), -s * 0.5f64.sqrt()); // outward along the diagonal
        let (nx, ny) = (-uy, ux);
        let arc: Vec<P> = (-90..91)
            .step_by(6)
            .map(|a| rad(a as f64))
            .map(|t| {
                (
                    cx + r * (t.cos() * ux + t.sin() * nx),
                    cy + r * (t.cos() * uy + t.sin() * ny),
                )
            })
            .collect();
        let a0 = (arc[0].0 - leg * ux, arc[0].1 - leg * uy);
        let last = arc[arc.len() - 1];
        let a1 = (last.0 - leg * ux, last.1 - leg * uy);
        polyline(&cat(&[&[a0], &arc, &[a1]]), "")
    };

    els![
        acc(half(15.75, 8.25, 1.0)),
        half(8.25, 15.75, -1.0),
        line(9.75, 14.25, 14.25, 9.75, "")
    ]
}

fn copy() -> Vec<String> {
    els![
        rect(8.5, 8.5, 11.5, 11.5, 2.5, "", ""),
        acc(path(
            "M15.5 8.5 V6.5 A2.5 2.5 0 0 0 13 4 H6.5 A2.5 2.5 0 0 0 4 6.5 V13 A2.5 2.5 0 0 0 6.5 15.5 H8.5",
            "",
            ""
        )),
    ]
}

fn edit() -> Vec<String> {
    // A pencil on the diagonal: pointed tip at the lower left, rounded end at the upper right.
    let (ux, uy) = (0.5f64.sqrt(), -(0.5f64.sqrt())); // along the pencil, toward the end
    let (nx, ny) = (-uy, ux);
    let (tip, w, tip_len) = ((4.0, 20.0), 2.4, 4.2);
    let end = (17.6, 6.4);
    let sh = (tip.0 + tip_len * ux, tip.1 + tip_len * uy);
    let cap: Vec<P> = (-9..=9)
        .rev()
        .map(|k| rad((k * 10) as f64))
        .map(|t| {
            (
                end.0 + w * (t.cos() * ux + t.sin() * nx),
                end.1 + w * (t.cos() * uy + t.sin() * ny),
            )
        })
        .collect();
    let (s1, s2) = (
        (sh.0 + w * nx, sh.1 + w * ny),
        (sh.0 - w * nx, sh.1 - w * ny),
    );
    let body = cat(&[&[s1], &cap, &[s2]]);
    let band = (end.0 - 3.2 * ux, end.1 - 3.2 * uy);
    els![
        poly(&body, "", ""),
        acc(polyline(&[s1, tip, s2], "")),
        line(
            band.0 + w * nx,
            band.1 + w * ny,
            band.0 - w * nx,
            band.1 - w * ny,
            ""
        ),
    ]
}

fn restore() -> Vec<String> {
    els![
        trash_body(false),
        acc_all(open_arrow(12.0, 17.0, 12.0, 10.5, 2.5))
    ]
}

fn history() -> Vec<String> {
    let ring = arc_points_step(12.0, 12.0, 9.0, 9.0, 215.0, 505.0, 5.0);
    let (tip, nxt) = (ring[0], ring[1]);
    els![
        polyline(&ring, ""),
        head(tip, nxt, 3.5),
        acc(polyline(&[(12.0, 7.5), (12.0, 12.0), (15.0, 14.0)], ""))
    ]
}

fn stopwatch() -> Vec<String> {
    els![
        circle(12.0, 13.25, 7.75, "", ""),
        line(10.0, 3.0, 14.0, 3.0, ""),
        line(12.0, 3.0, 12.0, 5.5, ""),
        line(18.0, 6.75, 19.5, 5.25, ""),
        acc(line(12.0, 13.25, 12.0, 9.25, "")),
    ]
}

fn pause() -> Vec<String> {
    els![
        rect(6.5, 5.0, 3.5, 14.0, 1.25, "", ""),
        rect(14.0, 5.0, 3.5, 14.0, 1.25, "", "")
    ]
}

fn stop() -> Vec<String> {
    // A filled square, as a media player's stop button (the same height as Pause).
    els![rect(6.0, 6.0, 12.0, 12.0, 1.5, INK, "")]
}

fn play() -> Vec<String> {
    // A filled triangle pointing right, as a media player's play button: as tall as Stop, and
    // nudged right of the box centre so its mass sits in the middle.
    els![poly(&[(7.5, 5.5), (18.75, 12.0), (7.5, 18.5)], INK, "")]
}

fn skip_back() -> Vec<String> {
    // Play's triangle turned to face left, against a bar: the bar is the start of the track.
    els![
        line(6.0, 5.5, 6.0, 18.5, ""),
        poly(&[(18.0, 5.5), (9.5, 12.0), (18.0, 18.5)], INK, "")
    ]
}

fn skip_forward() -> Vec<String> {
    els![
        line(18.0, 5.5, 18.0, 18.5, ""),
        poly(&[(6.0, 5.5), (14.5, 12.0), (6.0, 18.5)], INK, "")
    ]
}

fn repeat() -> Vec<String> {
    // Two arrows chasing each other round a loop, each turning a rounded corner.
    els![
        path("M4 13 V10.5 A3.5 3.5 0 0 1 7.5 7 H19", "", ""),
        path("M20 11 V13.5 A3.5 3.5 0 0 1 16.5 17 H5", "", ""),
        acc(head((19.0, 7.0), (14.0, 7.0), 3.0)),
        acc(head((5.0, 17.0), (10.0, 17.0), 3.0)),
    ]
}

fn speaker() -> String {
    poly(
        &[
            (3.5, 9.5),
            (7.5, 9.5),
            (12.5, 5.0),
            (12.5, 19.0),
            (7.5, 14.5),
            (3.5, 14.5),
        ],
        "",
        "",
    )
}

fn volume() -> Vec<String> {
    els![
        speaker(),
        acc(path("M16 9.5 A3.5 3.5 0 0 1 16 14.5", "", "")),
        acc(path("M18.75 6.75 A7.5 7.5 0 0 1 18.75 17.25", "", "")),
    ]
}

fn volume_mute() -> Vec<String> {
    els![
        speaker(),
        acc(line(16.0, 9.5, 21.0, 14.5, "")),
        acc(line(21.0, 9.5, 16.0, 14.5, ""))
    ]
}

fn music() -> Vec<String> {
    // Two beamed quavers: the note heads are filled, the beam carries the accent.
    els![
        line(9.5, 17.5, 9.5, 6.5, ""),
        line(19.5, 15.5, 19.5, 4.5, ""),
        circle(7.25, 17.5, 2.25, INK, ""),
        circle(17.25, 15.5, 2.25, INK, ""),
        acc(line(9.5, 6.5, 19.5, 4.5, "")),
    ]
}

fn visible() -> Vec<String> {
    els![
        path(&d(&eye_pts(32), true), "", ""),
        acc(circle(12.0, 12.0, 3.0, "", ""))
    ]
}

fn hidden() -> Vec<String> {
    let (a, b) = ((4.0, 4.0), (20.0, 20.0));
    let pupil = arc_points_step(12.0, 12.0, 3.0, 3.0, 0.0, 360.0, 4.0);
    els![
        gapped(&eye_pts(64), a, b, 2.2, true),
        gapped(&pupil, a, b, 2.2, true),
        acc(seg(a, b, ""))
    ]
}

fn lock(shackle: &str) -> Vec<String> {
    let body = rounded(&[(4.1, 10.1), (19.9, 10.1), (19.9, 21.4), (4.1, 21.4)], 3.0);
    let keyhole = stroke_outline(&[(12.0, 14.5), (12.0, 17.0)], 2.5);
    els![
        acc(path(shackle, "", "")),
        knockout(&d(&body, true), &[d(&keyhole, true)])
    ]
}

fn lock_filled() -> Vec<String> {
    lock("M8 10.5 V8 A4 4 0 0 1 16 8 V10.5")
}

fn lock_open() -> Vec<String> {
    // lock-filled with the shackle raised and swung free on the right: its left leg still runs
    // into the body, its right leg stops short of it.
    lock("M8 10.5 V6.5 A4 4 0 0 1 16 6.5 V7.5")
}

// ---------------------------------------------------------------------------------------------
// Status

fn info() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        acc_all(els![
            line(12.0, 8.0, 12.0, 8.01, ""),
            line(12.0, 11.25, 12.0, 16.5, "")
        ])
    ]
}

fn info_filled() -> Vec<String> {
    els![knockout(
        &circle_d(12.0, 12.0, R_OUT),
        &[circle_d(12.0, 8.0, 1.15), d(&stem(12.0, 11.25, 16.5), true)]
    )]
}

fn help() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        acc_all(els![
            polyline(&question_pts(), ""),
            line(12.0, 17.0, 12.0, 17.01, "")
        ])
    ]
}

fn help_filled() -> Vec<String> {
    els![knockout(
        &circle_d(12.0, 12.0, R_OUT),
        &[
            d(&stroke_outline(&question_pts(), 1.9), true),
            circle_d(12.0, 17.0, 1.15)
        ]
    )]
}

fn error_filled() -> Vec<String> {
    els![knockout(
        &circle_d(12.0, 12.0, R_OUT),
        &[
            d(&stem(12.0, 7.25, 12.75), true),
            circle_d(12.0, 16.5, 1.15)
        ]
    )]
}

fn warning_filled() -> Vec<String> {
    let tri = rounded(&[(12.0, 2.4), (22.2, 20.6), (1.8, 20.6)], 2.4);
    els![knockout(
        &d(&tri, true),
        &[
            d(&stem(12.0, 9.5, 14.25), true),
            circle_d(12.0, 17.25, 1.15)
        ]
    )]
}

// A block arrow pointing left: a feature or instance defined in the context of an assembly.
const IN_CONTEXT: [P; 7] = [
    (3.5, 12.0),
    (11.0, 4.5),
    (11.0, 9.0),
    (20.5, 9.0),
    (20.5, 15.0),
    (11.0, 15.0),
    (11.0, 19.5),
];

fn in_context() -> Vec<String> {
    els![poly(&IN_CONTEXT, INK, "")]
}

fn in_context_secondary() -> Vec<String> {
    els![poly(&IN_CONTEXT, "", r#"stroke-dasharray="2.2 1.6""#)]
}

// ---------------------------------------------------------------------------------------------
// Places and sharing

fn home() -> Vec<String> {
    els![
        polyline(&[(3.5, 11.0), (12.0, 3.75), (20.5, 11.0)], ""),
        path(
            "M6 9.25 V18 A2 2 0 0 0 8 20 H16 A2 2 0 0 0 18 18 V9.25",
            "",
            ""
        ),
        acc(path(
            "M10 20 V15.5 A1 1 0 0 1 11 14.5 H13 A1 1 0 0 1 14 15.5 V20",
            "",
            ""
        )),
    ]
}

fn notifications() -> Vec<String> {
    els![
        path("M6.5 16.5 V10.5 A5.5 5.5 0 0 1 17.5 10.5 V16.5", "", ""),
        line(4.5, 16.5, 19.5, 16.5, ""),
        line(12.0, 3.0, 12.0, 5.0, ""),
        acc(path("M10 19.25 A2 2 0 0 0 14 19.25", "", "")),
    ]
}

fn settings() -> Vec<String> {
    // Eight-toothed gear with rounded teeth; the hub (the part you "adjust") is accent.
    let mut pts = Vec::new();
    for k in 0..8 {
        let c = (k * 45) as f64;
        for (r, da) in [
            (6.9, -22.5 + 7.0),
            (9.0, -9.5),
            (9.0, 9.5),
            (6.9, 22.5 - 7.0),
        ] {
            let a = rad(c + da);
            pts.push((12.0 + r * a.cos(), 12.0 + r * a.sin()));
        }
    }
    els![
        path(&d(&pts, true), "", ""),
        acc(circle(12.0, 12.0, 2.9, "", ""))
    ]
}

fn share() -> Vec<String> {
    let nodes = [(6.0, 12.0), (17.5, 5.75), (17.5, 18.25)];
    let r = 2.5;
    let mut out = els![acc(circle(nodes[0].0, nodes[0].1, r, "", ""))];
    out.extend(nodes[1..].iter().map(|&(x, y)| circle(x, y, r, "", "")));
    for &(x1, y1) in &nodes[1..] {
        let (x0, y0) = nodes[0];
        let l = hypot(x1 - x0, y1 - y0);
        let (ux, uy) = ((x1 - x0) / l, (y1 - y0) / l);
        let g = r + 1.1;
        out.push(line(x0 + ux * g, y0 + uy * g, x1 - ux * g, y1 - uy * g, ""));
    }
    out
}

fn public() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        ellipse(12.0, 12.0, 3.75, 9.0, "", ""),
        acc(line(3.0, 12.0, 21.0, 12.0, ""))
    ]
}

fn cloud_upload() -> Vec<String> {
    els![
        path(
            "M9 18.5 H7 A4.25 4.25 0 0 1 6.35 10.05 A6 6 0 0 1 17.75 9.1 A4.75 4.75 0 0 1 17 18.5 H15",
            "",
            ""
        ),
        acc_all(open_arrow(12.0, 20.5, 12.0, 12.0, 3.0)),
    ]
}

const TRAY: &str = "M4 15 V17.5 A2.5 2.5 0 0 0 6.5 20 H17.5 A2.5 2.5 0 0 0 20 17.5 V15";

fn upload() -> Vec<String> {
    els![
        path(TRAY, "", ""),
        acc_all(open_arrow(12.0, 15.5, 12.0, 4.0, HEAD))
    ]
}

fn download() -> Vec<String> {
    // The upload tray with the arrow turned round: down into the tray.
    els![
        path(TRAY, "", ""),
        acc_all(open_arrow(12.0, 4.0, 12.0, 15.5, HEAD))
    ]
}

// ---------------------------------------------------------------------------------------------
// Flip direction: a bold diagonal arrow with a filled head (an allowed exception; it sits in a
// small ghost button). The two icons are exact 180-degree rotations about 12,12.

fn flip(sign: f64) -> Vec<String> {
    // the down-left arrow; sign -1 rotates it 180 degrees about 12,12
    let p = |(x, y): P| (12.0 + sign * (x - 12.0), 12.0 + sign * (y - 12.0));

    let (tip, tail) = ((4.5, 19.5), (18.5, 5.5));
    let (ux, uy) = (-(0.5f64.sqrt()), 0.5f64.sqrt());
    let (nx, ny) = (-uy, ux);
    let (hl, hw) = (6.5, 4.0); // head length along the shaft, half width
    let base = (tip.0 - hl * ux, tip.1 - hl * uy);
    let head = [
        tip,
        (base.0 + hw * nx, base.1 + hw * ny),
        (base.0 - hw * nx, base.1 - hw * ny),
    ];
    let shaft_end = (base.0 + ux, base.1 + uy);
    els![
        seg(p(tail), p(shaft_end), ""),
        acc(poly(&head.map(p), INK, ""))
    ]
}

fn flip_direction() -> Vec<String> {
    flip(1.0)
}

fn flip_direction_up() -> Vec<String> {
    flip(-1.0)
}

fn theme_light() -> Vec<String> {
    // A sun: a solid disc and eight rays.
    let rays: Vec<String> = (0..8)
        .map(|i| {
            let a = f64::from(i) * PI / 4.0;
            let (c, s) = (a.cos(), a.sin());
            format!(
                "M{} {} L{} {}",
                f(12.0 + 6.75 * c),
                f(12.0 + 6.75 * s),
                f(12.0 + 9.0 * c),
                f(12.0 + 9.0 * s)
            )
        })
        .collect();
    els![
        circle(12.0, 12.0, 4.25, INK, r#"stroke="none""#),
        path(&rays.join(" "), "", "")
    ]
}

fn theme_dark() -> Vec<String> {
    // A solid crescent moon.
    els![path(
        "M20.6 14.6 A8.9 8.9 0 1 1 9.4 3.4 A7 7 0 0 0 20.6 14.6 Z",
        INK,
        r#"stroke="none""#
    )]
}

fn theme_system() -> Vec<String> {
    // A monitor whose screen is half light, half dark: follow the system.
    els![
        rect(2.5, 3.5, 19.0, 13.0, 2.5, "", ""),
        path(
            "M12 6 H17.5 A1.5 1.5 0 0 1 19 7.5 V12.5 A1.5 1.5 0 0 1 17.5 14 H12 Z",
            INK,
            r#"stroke="none""#
        ),
        path("M12 16.5 V20.5 M8 20.5 H16", "", ""),
    ]
}

// ---------------------------------------------------------------------------------------------
// Network activity: an up arrow for what is sent and a down arrow for what is received. An arrow
// lights up while traffic flows its way: accent, with its head filled, so it stands out even
// when the app tints the whole icon one colour. The arrows never move, so an app can swap the
// icons as the traffic changes.

const TX_X: f64 = 8.0; // the up arrow's centre line
const RX_X: f64 = 16.0; // the down arrow's

/// The arrow from `tail` to `tip` on centre line x: open while idle, filled while lit.
fn traffic(x: f64, tail: f64, tip: f64, lit: bool) -> Vec<String> {
    if !lit {
        return open_arrow(x, tail, x, tip, 4.0);
    }
    let s = (tip - tail).signum();
    let base = tip - 6.0 * s;
    let head = [(x, tip + 0.6 * s), (x + 4.4, base), (x - 4.4, base)];
    acc_all(els![
        line(x, tail, x, base, ""),
        poly(&rounded(&head, 0.6), INK, "")
    ])
}

fn arrows(tx: bool, rx: bool) -> Vec<String> {
    els![traffic(TX_X, 20.0, 4.0, tx), traffic(RX_X, 4.0, 20.0, rx)]
}

fn network() -> Vec<String> {
    arrows(false, false)
}

fn network_rx() -> Vec<String> {
    arrows(false, true)
}

fn network_tx() -> Vec<String> {
    arrows(true, false)
}

fn network_rx_tx() -> Vec<String> {
    arrows(true, true)
}

fn network_error() -> Vec<String> {
    // An exclamation mark in the free corner beside the down arrow's tail.
    let x = 20.25;
    els![
        arrows(false, false),
        acc_all(els![line(x, 4.0, x, 9.5, ""), line(x, 13.0, x, 13.01, "")])
    ]
}

fn network_disconnected() -> Vec<String> {
    // The idle arrows struck through, broken where the slash runs over them. It runs from bottom
    // left to top right so that it crosses each shaft once and misses both heads.
    let (a, b) = ((4.0, 20.0), (20.0, 4.0));
    let mut out = Vec::new();
    for (x, tail, tip) in [(TX_X, 20.0_f64, 4.0), (RX_X, 4.0, 20.0)] {
        let s = (tip - tail).signum();
        let head = [(x - 4.0, tip - 4.0 * s), (x, tip), (x + 4.0, tip - 4.0 * s)];
        out.extend(gapped(
            &dense(&[(x, tail), (x, tip)], 0.2, false),
            a,
            b,
            2.2,
            false,
        ));
        out.extend(gapped(&dense(&head, 0.2, false), a, b, 2.2, false));
    }
    out.push(acc(seg(a, b, "")));
    out
}

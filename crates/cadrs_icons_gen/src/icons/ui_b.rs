//! Interface objects, documents, panels and people (kind Line).
//!
//! Shared shapes (keep in step with ui_a):
//! - Documents: 14 x 18 (x 5..19, y 3..21), corner radius 2.5, folded top-right corner of 5.
//! - Folders: 18 x 15 (x 3..21, y 4.5..19.5), corner radius 2.5, tab on the left.
//! - People: a round head over a domed shoulder curve (`person`).
//! - Plus signs: arms of 2.5 (5 across). Arrowheads: open, 5 wide.
//! - ACCENT marks the modifier (plus signs, arrows, magnifiers, the heads of people, the first
//!   text line of a document), one element or group per icon. Plain objects stay INK only.

use crate::common::*;
use Kind::Line;
use std::f64::consts::PI;

pub fn icons() -> Vec<Def> {
    vec![
        def("file", Line, "File", file),
        def("file-new", Line, "New file", file_new),
        def("sync-document", Line, "Sync document", sync_document),
        def("board", Line, "Board", board),
        def("create-assembly", Line, "Create assembly", create_assembly),
        def("file-import", Line, "Import", file_import),
        def("file-export", Line, "Export", file_export),
        def("details", Line, "Details", details),
        def("folder", Line, "Folder", folder),
        def("folder-new", Line, "New folder", folder_new),
        def("move-to-folder", Line, "Move to folder", move_to_folder),
        def("apps", Line, "Apps", apps),
        def("custom-table", Line, "Custom tables", custom_table),
        def("column-add", Line, "Add column", column_add),
        def("configurations", Line, "Configurations", configurations),
        def("variables", Line, "Variables", variables),
        def("appearance", Line, "Appearances", appearance),
        def("properties", Line, "Properties", properties),
        def("comments", Line, "Comments", comments),
        def("chat", Line, "Chat", chat),
        def("find", Line, "Find in document", find),
        def("list", Line, "List view", list_view),
        def("list-details", Line, "List with details", list_details),
        def("tab-manager", Line, "Tab manager", tab_manager),
        def("branches", Line, "Branches", branches),
        def("versions", Line, "Versions", versions),
        def("likes", Line, "Likes", likes),
        def("heart", Line, "Heart", heart),
        def("star", Line, "Star", star),
        def("user", Line, "User", user),
        def("users", Line, "Teams", users),
        def("shop", Line, "Shop", shop),
        def("device-add", Line, "Link a device", device_add),
        def("owned-by-me", Line, "Owned by me", owned_by_me),
        def("shared-with-me", Line, "Shared with me", shared_with_me),
        def("screenshot", Line, "Camera", screenshot),
        def("code", Line, "Code", code),
        def("chip", Line, "Chip", chip),
        def("tool", Line, "Tool", tool),
        def("books", Line, "Books", books),
        def("idea", Line, "Explore", idea),
        def("keyboard", Line, "Keyboard shortcuts", keyboard),
        def("tag", Line, "Label", tag),
        def("tag-new", Line, "Add label", tag_new),
        def("receipt", Line, "Quote", receipt),
        def("location", Line, "Location", location),
        def("map-pin", Line, "Find on a map", map_pin),
        def("qr-code", Line, "QR code", qr_code),
        def("note", Line, "Note", note),
        def("bold", Line, "Bold", bold),
        def("italic", Line, "Italic", italic),
        def(
            "flip-horizontal",
            Line,
            "Flip left to right",
            flip_horizontal,
        ),
        def("ruler", Line, "Measure a distance", ruler),
        def("clock", Line, "Clock", clock),
    ]
}

const R: f64 = 2.5; // corner radius shared by documents, folders, frames

/// Recolours an element in ACCENT: the stroke, and the fill of filled dots. Accent the modifier,
/// not the object, and only one element or group per icon.
fn acc(el: String) -> String {
    el.replace(&format!(r#"fill="{INK}""#), &format!(r#"fill="{ACCENT}""#))
        .replacen("/>", &format!(r#" stroke="{ACCENT}"/>"#), 1)
}

fn acc_all(els: Vec<String>) -> Vec<String> {
    els.into_iter().map(acc).collect()
}

/// An arc command to (x, y) with radius r.
fn a(x: f64, y: f64, r: f64, sweep: u8) -> String {
    format!("A{} {} 0 0 {sweep} {} {}", f(r), f(r), f(x), f(y))
}

// ---------------------------------------------------------------------------------------------
// Documents

/// Where a document's outline is cut for an arrow to pass through it.
enum Gap {
    None,
    Left(f64, f64),
    Right(f64, f64),
}

/// The document outline, optionally with a gap in the left or right edge (for arrows that
/// pass through it), plus the fold line.
fn doc_with(x0: f64, x1: f64, y0: f64, y1: f64, gap: Gap) -> Vec<String> {
    let fold = 5.0;
    let (fx, fy) = (x1 - fold, y0 + fold);
    let left_down = format!(
        "V{} {} H{} {}",
        f(y1 - R),
        a(x0 + R, y1, R, 0),
        f(x1 - R),
        a(x1, y1 - R, R, 0)
    );
    let top = format!(
        "L{} {} H{} {}",
        f(fx),
        f(y0),
        f(x0 + R),
        a(x0, y0 + R, R, 0)
    );
    let d = match gap {
        Gap::Left(g0, g1) => format!(
            "M{} {} {left_down} V{} {top} V{}",
            f(x0),
            f(g1),
            f(fy),
            f(g0)
        ),
        Gap::Right(g0, g1) => format!(
            "M{} {} V{} {top} {left_down} V{}",
            f(x1),
            f(g0),
            f(fy),
            f(g1)
        ),
        Gap::None => format!(
            "M{} {} H{} {} {left_down} V{} Z",
            f(fx),
            f(y0),
            f(x0 + R),
            a(x0, y0 + R, R, 0),
            f(fy)
        ),
    };
    let fold_d = format!(
        "M{} {} V{} {} H{}",
        f(fx),
        f(y0),
        f(fy - 1.5),
        a(fx + 1.5, fy, 1.5, 0),
        f(x1)
    );
    els![path(&d, "", ""), path(&fold_d, "", "")]
}

fn doc() -> Vec<String> {
    doc_with(5.0, 19.0, 3.0, 21.0, Gap::None)
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

/// A straight arrow with an open head, 2.5 along and 5 across.
fn open_arrow(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<String> {
    let head = 2.5;
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

fn file() -> Vec<String> {
    doc()
}

fn file_new() -> Vec<String> {
    els![doc(), acc(plus(12.0, 14.0))]
}

fn sync_document() -> Vec<String> {
    // A document with a two-arrow sync ring over its lower right corner (the document edge is
    // cut back where the ring passes).
    let ring_a = arc_points_step(15.5, 15.5, 5.0, 5.0, 200.0, 340.0, 10.0);
    let ring_b = arc_points_step(15.5, 15.5, 5.0, 5.0, 20.0, 160.0, 10.0);
    let (na, nb) = (ring_a.len(), ring_b.len());
    els![
        doc_with(4.0, 17.0, 2.5, 20.5, Gap::Right(9.25, 20.5)),
        acc_all(els![
            polyline(&ring_a, ""),
            head_at(ring_a[na - 1], ring_a[na - 2]),
            polyline(&ring_b, ""),
            head_at(ring_b[nb - 1], ring_b[nb - 2]),
        ]),
    ]
}

fn head_at(tip: P, frm: P) -> String {
    let s = 2.25;
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

fn board() -> Vec<String> {
    // A printed circuit board: a rounded outline with a mounting hole in two corners and a chip
    // footprint with traces (the accent).
    els![
        rect(3.0, 5.0, 18.0, 14.0, R, "", ""),
        circle(6.0, 8.0, 1.0, "", ""),
        circle(18.0, 16.0, 1.0, "", ""),
        acc(path(
            "M9.5 10 H14.5 V14 H9.5 Z M14.5 12 H18 M6 16 H9.5",
            "",
            ""
        )),
    ]
}

fn create_assembly() -> Vec<String> {
    // Two stacked parts on a base plate, with a plus: make an assembly.
    els![
        path("M3 17.5 L12 21 L21 17.5", "", ""),
        path(
            "M5 12.5 L10 10.5 L15 12.5 L10 14.5 Z M5 12.5 V15 L10 17 L15 15 V12.5 M10 14.5 V17",
            "",
            ""
        ),
        acc(plus(17.5, 6.5)),
    ]
}

fn file_import() -> Vec<String> {
    els![
        doc_with(7.0, 21.0, 3.0, 21.0, Gap::Left(10.5, 17.5)),
        acc_all(open_arrow(2.5, 14.0, 13.0, 14.0))
    ]
}

fn file_export() -> Vec<String> {
    els![
        doc_with(3.0, 17.0, 3.0, 21.0, Gap::Right(10.5, 17.5)),
        acc_all(open_arrow(9.5, 14.0, 21.5, 14.0))
    ]
}

fn details() -> Vec<String> {
    els![
        doc(),
        acc(path("M8.5 9 H11", "", "")),
        path("M8.5 13 H15.5 M8.5 17 H15.5", "", "")
    ]
}

// ---------------------------------------------------------------------------------------------
// Folders

fn folder_shape() -> Vec<String> {
    let (x0, x1, y0, y1) = (3.0, 21.0, 4.5, 19.5);
    let tab = 7.0; // width of the tab's flat top, from the left corner
    let d = format!(
        "M{} {} V{} {} H{} L{} {} H{} {} V{} {} H{} {} Z",
        f(x0),
        f(y1 - R),
        f(y0 + R),
        a(x0 + R, y0, R, 1),
        f(x0 + tab),
        f(x0 + tab + 2.0),
        f(y0 + 2.5),
        f(x1 - R),
        a(x1, y0 + 2.5 + R, R, 1),
        f(y1 - R),
        a(x1 - R, y1, R, 1),
        f(x0 + R),
        a(x0, y1 - R, R, 1)
    );
    els![path(&d, "", "")]
}

fn folder() -> Vec<String> {
    folder_shape()
}

fn folder_new() -> Vec<String> {
    els![folder_shape(), acc(plus(12.0, 13.25))]
}

fn move_to_folder() -> Vec<String> {
    els![folder_shape(), acc_all(open_arrow(8.0, 13.25, 16.0, 13.25))]
}

// ---------------------------------------------------------------------------------------------
// Panels

fn apps() -> Vec<String> {
    let (s, a, b) = (6.75, 3.5, 13.75);
    let mut tiles: Vec<String> = [a, b]
        .iter()
        .flat_map(|&y| [a, b].map(|x| rect(x, y, s, s, 1.75, "", "")))
        .collect();
    tiles[1] = acc(tiles[1].clone()); // top right
    tiles
}

fn custom_table() -> Vec<String> {
    // The accent goes underneath, so the ink frame covers its round caps.
    els![
        acc(line(3.0, 9.5, 21.0, 9.5, "")),
        rect(3.0, 4.0, 18.0, 16.0, R, "", ""),
        path("M3 14.75 H21 M9.5 9.5 V20", "", "")
    ]
}

fn column_add() -> Vec<String> {
    // A table with a header row, and a plus where the new column goes (right).
    els![
        rect(3.0, 4.0, 11.0, 16.0, R, "", ""),
        path("M3 9.5 H14 M8.5 4 V20", "", ""),
        acc(plus(18.5, 12.0))
    ]
}

fn configurations() -> Vec<String> {
    let mut out = Vec::new();
    for (y, k) in [(6.0, 15.0), (12.0, 8.0), (18.0, 13.0)] {
        out.push(path(
            &format!("M3 {} H{} M{} {} H21", f(y), f(k - 2.25), f(k + 2.25), f(y)),
            "",
            "",
        ));
        out.push(acc(circle(k, y, 2.25, "", "")));
    }
    out
}

fn variables() -> Vec<String> {
    els![
        path(
            "M14 4.6 C12.4 3.7 10.6 4.3 10.2 6.3 L8 17.7 C7.6 19.7 5.8 20.3 4.2 19.4",
            "",
            ""
        ),
        path("M6.5 9.5 H13", "", ""),
        acc(path("M14.5 12.5 L20 19 M20 12.5 L14.5 19", "", "")),
    ]
}

fn appearance() -> Vec<String> {
    let d = "M12 21 A9 9 0 0 1 12 3 A9 8.5 0 0 1 21 11.5 A4.25 4.25 0 0 1 16.75 15.75 H15 \
             A1.75 1.75 0 0 0 13.6 18.55 L13.85 18.9 A1.35 1.35 0 0 1 12 21 Z";
    let dots: Vec<String> = [(7.5, 12.25), (8.5, 8.0), (12.25, 6.5), (16.0, 8.25)]
        .iter()
        .map(|&(x, y)| acc(circle(x, y, 0.45, INK, "")))
        .collect();
    els![path(d, "", ""), dots]
}

fn properties() -> Vec<String> {
    els![
        rect(3.5, 3.5, 17.0, 17.0, R, "", ""),
        acc(path("M7.5 8.5 H9.5 M12.5 8.5 H16.5", "", "")),
        path(
            "M7.5 12 H9.5 M12.5 12 H16.5 M7.5 15.5 H9.5 M12.5 15.5 H16.5",
            "",
            ""
        ),
    ]
}

fn comments() -> Vec<String> {
    let d = format!(
        "M7.5 17 V20.5 L11.75 17 H18.5 {} V6.5 {} H5.5 {} V14.5 {} Z",
        a(21.0, 14.5, R, 0),
        a(18.5, 4.0, R, 0),
        a(3.0, 6.5, R, 0),
        a(5.5, 17.0, R, 0)
    );
    els![
        path(&d, "", ""),
        acc(path("M7.5 9 H16.5", "", "")),
        path("M7.5 12.5 H13", "", "")
    ]
}

fn find() -> Vec<String> {
    let corners = "M3 7.5 V5.5 A2.5 2.5 0 0 1 5.5 3 H7.5 M16.5 3 H18.5 A2.5 2.5 0 0 1 21 5.5 V7.5 \
                   M21 16.5 V18.5 A2.5 2.5 0 0 1 18.5 21 H16.5 M7.5 21 H5.5 A2.5 2.5 0 0 1 3 18.5 V16.5";
    els![
        path(corners, "", ""),
        acc_all(els![
            circle(11.25, 11.25, 3.75, "", ""),
            line(14.0, 14.0, 16.5, 16.5, "")
        ])
    ]
}

fn list_view() -> Vec<String> {
    els![
        path("M9 6 H21 M9 12 H21 M9 18 H21", "", ""),
        acc(path("M4 6 h.01 M4 12 h.01 M4 18 h.01", "", ""))
    ]
}

fn list_details() -> Vec<String> {
    els![
        acc(rect(3.0, 4.0, 6.5, 6.5, 1.5, "", "")),
        acc(rect(3.0, 13.5, 6.5, 6.5, 1.5, "", "")),
        path("M13 5.5 H21 M13 9 H17.5 M13 15 H21 M13 18.5 H17.5", "", ""),
    ]
}

fn tab_manager() -> Vec<String> {
    els![
        path("M3 5.5 H21 M3 11.5 H10 M3 17.5 H8.5", "", ""),
        acc_all(els![
            circle(15.5, 14.5, 3.5, "", ""),
            line(18.1, 17.1, 20.5, 19.5, "")
        ]),
    ]
}

// ---------------------------------------------------------------------------------------------
// Versioning and social

fn branches() -> Vec<String> {
    els![
        circle(7.0, 5.5, 2.25, "", ""),
        circle(7.0, 18.5, 2.25, "", ""),
        acc(circle(17.0, 5.5, 2.25, "", "")),
        line(7.0, 7.75, 7.0, 16.25, ""),
        path(
            "M17 7.75 V8.5 C17 12.5 14 14 11.5 14 C9 14 7 14.8 7 16.25",
            "",
            ""
        ),
    ]
}

fn versions() -> Vec<String> {
    els![
        rect(10.0, 3.5, 11.0, 17.0, R, "", ""),
        acc(path("M6.5 6 V18 M3 8.5 V15.5", "", ""))
    ]
}

fn heart() -> Vec<String> {
    // A solid heart (sponsor, favourite).
    let d = "M12 20.4 C7.1 17 3.25 13.8 3.25 9.5 C3.25 6.7 5.4 4.55 8.1 4.55 C9.65 4.55 11.05 5.3 12 6.55 \
             C12.95 5.3 14.35 4.55 15.9 4.55 C18.6 4.55 20.75 6.7 20.75 9.5 C20.75 13.8 16.9 17 12 20.4 Z";
    els![path(d, INK, r#"stroke="none""#)]
}

fn star() -> Vec<String> {
    // A solid five-pointed star (stars, favourites), its points rounded by the root stroke.
    let (cx, cy) = (12.0, 12.6);
    let points: Vec<(f64, f64)> = (0..10)
        .map(|i| {
            let r = if i % 2 == 0 { 8.6 } else { 3.7 };
            let a = -PI / 2.0 + f64::from(i) * PI / 5.0;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect();
    els![poly(&points, INK, "")]
}

fn likes() -> Vec<String> {
    let hand = "M7.5 10.5 L10.6 4.3 A2.3 2.3 0 0 1 14.9 5.7 L14.2 10 H18.3 A2.2 2.2 0 0 1 20.45 12.7 \
                L19.1 18.3 A2.2 2.2 0 0 1 16.95 20 H7.5 Z";
    let cuff = "M7.5 10.5 H5 A2 2 0 0 0 3 12.5 V18 A2 2 0 0 0 5 20 H7.5";
    els![path(hand, "", ""), acc(path(cuff, "", ""))]
}

/// A head over a domed shoulder curve. The shoulders are a half-ellipse-ish curve from
/// (cx - half_w, bottom) up to `top` and back down.
fn person(cx: f64, head_cy: f64, head_r: f64, half_w: f64, top: f64, bottom: f64) -> Vec<String> {
    let k = 0.55;
    let h = bottom - top;
    let d = format!(
        "M{} {} C{} {} {} {} {} {} C{} {} {} {} {} {}",
        f(cx - half_w),
        f(bottom),
        f(cx - half_w),
        f(bottom - h * k * 1.3),
        f(cx - half_w * k),
        f(top),
        f(cx),
        f(top),
        f(cx + half_w * k),
        f(top),
        f(cx + half_w),
        f(bottom - h * k * 1.3),
        f(cx + half_w),
        f(bottom)
    );
    els![acc(circle(cx, head_cy, head_r, "", "")), path(&d, "", "")]
}

fn user() -> Vec<String> {
    person(12.0, 7.5, 4.0, 7.5, 14.75, 20.5)
}

fn users() -> Vec<String> {
    let back_head = "M15.5 3.9 A3.6 3.6 0 0 1 15.5 10.9";
    let back_body = "M18.5 14.9 C20.2 15.7 21 17.8 21 20.5";
    els![
        person(9.0, 7.4, 3.5, 6.0, 14.5, 20.5),
        path(back_head, "", ""),
        path(back_body, "", "")
    ]
}

fn owned_by_me() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        acc(circle(12.0, 10.0, 3.25, "", "")),
        path(
            "M6.4 19 C7.5 16.8 9.6 15.5 12 15.5 C14.4 15.5 16.5 16.8 17.6 19",
            "",
            ""
        ),
    ]
}

fn shared_with_me() -> Vec<String> {
    els![
        rect(3.5, 3.5, 17.0, 17.0, 3.0, "", ""),
        acc(circle(12.0, 10.0, 3.25, "", "")),
        path(
            "M6.6 20.5 C7.1 17.3 9.3 15.5 12 15.5 C14.7 15.5 16.9 17.3 17.4 20.5",
            "",
            ""
        ),
    ]
}

// ---------------------------------------------------------------------------------------------
// Objects

fn screenshot() -> Vec<String> {
    let d = format!(
        "M3 9.5 {} H7.25 L8.9 4.5 H15.1 L16.75 7 H18.5 {} V17.5 {} H5.5 {} Z",
        a(5.5, 7.0, R, 1),
        a(21.0, 9.5, R, 1),
        a(18.5, 20.0, R, 1),
        a(3.0, 17.5, R, 1)
    );
    els![path(&d, "", ""), acc(circle(12.0, 13.25, 3.5, "", ""))]
}

fn code() -> Vec<String> {
    els![
        polyline(&[(7.5, 7.5), (3.0, 12.0), (7.5, 16.5)], ""),
        polyline(&[(16.5, 7.5), (21.0, 12.0), (16.5, 16.5)], ""),
        acc(line(13.5, 5.0, 10.5, 19.0, "")),
    ]
}

fn chip() -> Vec<String> {
    let pins: Vec<String> = [9.5, 14.5]
        .iter()
        .map(|&t| {
            format!(
                "M{} 3 V5.5 M{} 18.5 V21 M3 {} H5.5 M18.5 {} H21",
                f(t),
                f(t),
                f(t),
                f(t)
            )
        })
        .collect();
    els![
        rect(5.5, 5.5, 13.0, 13.0, R, "", ""),
        acc(rect(9.5, 9.5, 5.0, 5.0, 1.0, "", "")),
        path(&pins.join(" "), "", "")
    ]
}

fn tool() -> Vec<String> {
    // Built along +x (head at the origin, handle towards +x, jaw towards -x), then rotated so
    // the handle runs down-left.
    let (rh, h, s, depth, l) = (5.25f64, 1.7f64, 1.9f64, 0.9, 12.4);
    let ang = rad(135.0);
    let (cx, cy) = (14.9, 9.1);
    let xr = (rh * rh - h * h).sqrt();
    let xs = (rh * rh - s * s).sqrt();
    let a_h = deg(h.atan2(xr));
    let a_s = 180.0 - deg(s.atan2(xs));
    let mut ps = vec![(xr, -h), (l, -h)];
    ps.extend(
        (15..180)
            .step_by(15)
            .map(|t| (l + h * rad(t as f64).sin(), -h * rad(t as f64).cos())),
    );
    ps.extend([(l, h), (xr, h)]);
    ps.extend(
        (1..16)
            .map(|i| a_h + (a_s - a_h) * i as f64 / 16.0)
            .map(|t| (rh * rad(t).cos(), rh * rad(t).sin())),
    );
    ps.extend([(-xs, s), (-depth, s), (-depth, -s), (-xs, -s)]);
    ps.extend(
        (1..16)
            .map(|i| a_s - (a_s - a_h) * i as f64 / 16.0)
            .map(|t| (rh * rad(-t).cos(), rh * rad(-t).sin())),
    );
    let (ca, sa) = (ang.cos(), ang.sin());
    let t = |q: &[P]| {
        q.iter()
            .map(|&(x, y)| (cx + x * ca - y * sa, cy + x * sa + y * ca))
            .collect::<Vec<P>>()
    };
    // The jaw slot is traced again in ACCENT on top of the outline.
    let jaw = [(-xs, s), (-depth, s), (-depth, -s), (-xs, -s)];
    els![poly(&t(&ps), "", ""), acc(polyline(&t(&jaw), ""))]
}

fn books() -> Vec<String> {
    els![
        rect(3.5, 4.0, 4.5, 16.0, 1.25, "", ""),
        rect(8.0, 7.0, 4.5, 13.0, 1.25, "", ""),
        acc(rect(
            13.6,
            4.6,
            4.5,
            15.5,
            1.25,
            "",
            r#"transform="rotate(-14 15.85 20.1)""#
        )),
        path("M3.5 8 H8 M8 10.5 H12.5", "", ""),
    ]
}

fn idea() -> Vec<String> {
    els![
        path(
            "M9.5 16 C9.5 13.9 6 12.6 6 9.25 A6 6 0 0 1 18 9.25 C18 12.6 14.5 13.9 14.5 16 Z",
            "",
            ""
        ),
        acc(path("M9.5 18.75 H14.5 M10.75 21 H13.25", "", "")),
    ]
}

fn keyboard() -> Vec<String> {
    let keys = [6.5, 10.17, 13.83, 17.5]
        .iter()
        .map(|&x| format!("M{} 10 h.01", f(x)))
        .collect::<Vec<_>>()
        .join(" ");
    els![
        rect(2.5, 6.0, 19.0, 12.0, R, "", ""),
        path(&format!("{keys} M6.5 14 h.01 M17.5 14 h.01"), "", ""),
        acc(line(9.5, 14.0, 14.5, 14.0, "")),
    ]
}

/// The label outline and its hole, scaled by s and moved by (ox, oy).
fn tag_shape(s: f64, ox: f64, oy: f64) -> (String, String) {
    let p = |x: f64, y: f64| format!("{} {}", f(ox + x * s), f(oy + y * s));
    let r = 2.5 * s;
    let d = format!(
        "M{} V{} {} L{} A{} {} 0 0 0 {} L{} A{} {} 0 0 0 {} L{} A{} {} 0 0 0 {} H{} A{} {} 0 0 0 {} Z",
        p(3.0, 5.5),
        f(oy + 11.0 * s),
        a(ox + 3.73 * s, oy + 12.77 * s, 2.5 * s, 0),
        p(11.23, 20.27),
        f(r),
        f(r),
        p(14.77, 20.27),
        p(20.27, 14.77),
        f(r),
        f(r),
        p(20.27, 11.23),
        p(12.77, 3.73),
        f(r),
        f(r),
        p(11.0, 3.0),
        f(ox + 5.5 * s),
        f(r),
        f(r),
        p(3.0, 5.5)
    );
    (
        path(&d, "", ""),
        circle(ox + 7.5 * s, oy + 7.5 * s, 0.6, INK, ""),
    )
}

fn tag() -> Vec<String> {
    let (body, hole) = tag_shape(1.0, 0.0, 0.0);
    els![body, acc(hole)]
}

fn tag_new() -> Vec<String> {
    let (body, hole) = tag_shape(0.74, 0.9, 0.9);
    els![body, hole, acc(plus(18.25, 18.25))]
}

fn location() -> Vec<String> {
    els![
        path(
            "M12 21 C12 21 19 15 19 9.75 A7 7 0 0 0 5 9.75 C5 15 12 21 12 21 Z",
            "",
            ""
        ),
        acc(circle(12.0, 9.75, 2.5, "", ""))
    ]
}

fn note() -> Vec<String> {
    let d = format!(
        "M14.5 20.5 H6 {} V6 {} H18 {} V14.5 Z",
        a(3.5, 18.0, R, 1),
        a(6.0, 3.5, R, 1),
        a(20.5, 6.0, R, 1)
    );
    els![
        acc(path("M14.5 20.5 V16.5 A2 2 0 0 1 16.5 14.5 H20.5", "", "")),
        path(&d, "", ""),
        path("M7.5 8.5 H16.5 M7.5 12 H12.5", "", "")
    ]
}

// ---------------------------------------------------------------------------------------------
// Text and measurement

fn bold() -> Vec<String> {
    els![path(
        "M7 4.5 H12.75 A3.75 3.75 0 0 1 12.75 12 H7 Z M7 12 H13.75 A3.75 3.75 0 0 1 13.75 19.5 H7 Z",
        "",
        ""
    )]
}

fn italic() -> Vec<String> {
    els![path("M10 4.5 H19 M5 19.5 H14 M14.5 4.5 L9.5 19.5", "", "")]
}

fn flip_horizontal() -> Vec<String> {
    els![
        acc(line(
            12.0,
            3.0,
            12.0,
            21.0,
            r#"stroke-dasharray="0.01 3.6""#
        )),
        poly(&[(3.0, 18.0), (8.75, 6.0), (8.75, 18.0)], "", ""),
        poly(
            &[(21.0, 18.0), (15.25, 6.0), (15.25, 18.0)],
            "",
            r#"stroke-opacity="0.45""#
        ),
    ]
}

fn ruler() -> Vec<String> {
    let ticks = "M-6 -3.5 V-0.5 M-3 -3.5 V-1.5 M0 -3.5 V-0.5 M3 -3.5 V-1.5 M6 -3.5 V-0.5";
    els![format!(
        r#"<g transform="translate(12 12) rotate(-45)">{}{}</g>"#,
        rect(-9.5, -3.5, 19.0, 7.0, 2.0, "", ""),
        acc(path(ticks, "", ""))
    )]
}

fn clock() -> Vec<String> {
    els![
        circle(12.0, 12.0, 9.0, "", ""),
        acc(polyline(&[(12.0, 7.0), (12.0, 12.0), (15.25, 14.0)], ""))
    ]
}

// ---------------------------------------------------------------------------------------------
// Shops and people between them (the social side: organizations, quotes, chat)

fn shop() -> Vec<String> {
    // A storefront: the awning with its scallops over the building; the door is the accent.
    let scallops = format!(
        "M3 8.5 {} {} {} {}",
        a(7.5, 8.5, 2.25, 0),
        a(12.0, 8.5, 2.25, 0),
        a(16.5, 8.5, 2.25, 0),
        a(21.0, 8.5, 2.25, 0)
    );
    els![
        path("M3 8.5 L5 4 H19 L21 8.5", "", ""),
        path(&scallops, "", ""),
        path("M5 11 V20.5 H19 V11", "", ""),
        acc(path("M10 20.5 V15 H14 V20.5", "", ""))
    ]
}

fn receipt() -> Vec<String> {
    // A slip with a torn bottom edge; its first line (the total) is the accent.
    els![
        path(
            "M6 3 H18 V21 L16 19.5 L14 21 L12 19.5 L10 21 L8 19.5 L6 21 Z",
            "",
            ""
        ),
        acc(path("M9 7.5 H15", "", "")),
        path("M9 11 H15", "", ""),
        path("M9 14.5 H12.5", "", "")
    ]
}

fn chat() -> Vec<String> {
    // Two speech bubbles: the one behind (open where the front one covers it) and the front
    // one, an answer, in accent.
    let back = format!(
        "M9.5 12.5 H8 L5 15 V12.5 {} V6 {} H12 {} V8.5",
        a(3.0, 10.0, R, 1),
        a(5.5, 3.5, R, 1),
        a(14.5, 6.0, R, 1)
    );
    let front = format!(
        "M12 10 H18.5 {} V15 {} H19 V20 L15.5 17.5 H12 {} V12.5 {} Z",
        a(21.0, 12.5, R, 1),
        a(18.5, 17.5, R, 1),
        a(9.5, 15.0, R, 1),
        a(12.0, 10.0, R, 1)
    );
    els![path(&back, "", ""), acc(path(&front, "", ""))]
}

fn map_pin() -> Vec<String> {
    // A map folded in three, with a pin on it: the pin is the accent.
    els![
        path(
            "M3 7 L8.5 5 L15.5 7 L21 5 V17 L15.5 19 L8.5 17 L3 19 Z",
            "",
            ""
        ),
        path("M8.5 5 V9.5 M8.5 15 V17 M15.5 7 V9.5 M15.5 15 V19", "", ""),
        acc(path(
            "M12 16 C12 16 15 13.25 15 10.75 A3 3 0 0 0 9 10.75 C9 13.25 12 16 12 16 Z",
            "",
            ""
        ))
    ]
}

fn qr_code() -> Vec<String> {
    // Three finder squares and a scatter of modules: a plain object, ink only.
    let finder = |x: f64, y: f64| {
        vec![
            rect(x, y, 6.5, 6.5, 1.0, "", ""),
            rect(x + 2.25, y + 2.25, 2.0, 2.0, 0.0, INK, r#"stroke="none""#),
        ]
    };
    els![
        finder(3.0, 3.0),
        finder(14.5, 3.0),
        finder(3.0, 14.5),
        rect(14.5, 14.5, 2.5, 2.5, 0.0, INK, r#"stroke="none""#),
        rect(18.5, 18.5, 2.5, 2.5, 0.0, INK, r#"stroke="none""#),
        rect(18.5, 14.5, 2.5, 2.5, 0.0, INK, r#"stroke="none""#),
        rect(14.5, 18.5, 2.5, 2.5, 0.0, INK, r#"stroke="none""#),
        rect(10.75, 10.75, 2.5, 2.5, 0.0, INK, r#"stroke="none""#)
    ]
}

fn device_add() -> Vec<String> {
    // A phone (its speaker slot at the top), and the plus of adding it: the plus is the accent.
    els![
        rect(4.0, 3.0, 9.5, 18.0, R, "", ""),
        path("M7.5 6 H10", "", ""),
        acc(plus(18.25, 12.0))
    ]
}

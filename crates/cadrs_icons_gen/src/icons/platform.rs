//! Operating systems, devices and code hosting, for download buttons and platform lists (kind Line).
//!
//! Solid marks: filled INK shapes, with details such as eyes and the beak cut out with
//! `fill-rule="evenodd"` (as the filled glyphs in ui_a), so they tint as one colour. Simplified
//! marks drawn on the grid, not the vendors' artwork: each is one plain object, so all stay INK
//! only.

use crate::common::*;
use Kind::Line;

pub fn icons() -> Vec<Def> {
    vec![
        def("platform-windows", Line, "Windows", windows),
        def("platform-macos", Line, "macOS", macos),
        def("platform-linux", Line, "Linux", linux),
        def("platform-android", Line, "Android", android),
        def("platform-ios", Line, "iPhone", ios),
        def("github", Line, "GitHub", github),
    ]
}

const SOLID: &str = r#"stroke="none""#;
const SOLID_CUT: &str = r#"fill-rule="evenodd" stroke="none""#;

/// An ellipse as a closed subpath, for cutting holes with evenodd.
fn oval(cx: f64, cy: f64, rx: f64, ry: f64) -> String {
    let (left, right) = (f(cx - rx), f(cx + rx));
    let (rx, ry, cy) = (f(rx), f(ry), f(cy));
    format!("M{left} {cy} A{rx} {ry} 0 1 0 {right} {cy} A{rx} {ry} 0 1 0 {left} {cy} Z")
}

/// A rounded rectangle as a closed subpath.
fn rounded(x: f64, y: f64, w: f64, h: f64, r: f64) -> String {
    let (x0, x1, y0, y1) = (f(x), f(x + w), f(y), f(y + h));
    let (xa, xb, ya, yb) = (f(x + r), f(x + w - r), f(y + r), f(y + h - r));
    let r = f(r);
    format!(
        "M{xa} {y0} H{xb} A{r} {r} 0 0 1 {x1} {ya} V{yb} A{r} {r} 0 0 1 {xb} {y1} \
         H{xa} A{r} {r} 0 0 1 {x0} {yb} V{ya} A{r} {r} 0 0 1 {xa} {y0} Z"
    )
}

fn windows() -> Vec<String> {
    // Four panes with a gap between them.
    let (lo, hi, s) = (3.5, 12.75, 7.75);
    [(lo, lo), (hi, lo), (lo, hi), (hi, hi)]
        .iter()
        .map(|&(x, y)| rect(x, y, s, s, 0.75, INK, SOLID))
        .collect()
}

fn macos() -> Vec<String> {
    // An apple with a bite out of its right side, and a leaf.
    let body = "M12 8.3 C10.6 7.2 8.2 6.9 6.5 8.2 C4.6 9.7 4.3 12.6 5 15 C5.7 17.5 7.3 20.4 9 20.5 \
                C10.2 20.6 10.8 19.9 12 19.9 C13.2 19.9 13.8 20.6 15 20.5 C16.6 20.4 18 18.1 18.9 15.9 \
                C17.3 15.1 16.4 13.6 16.5 12 C16.6 10.6 17.4 9.7 18.3 9.2 C17.2 7.7 15.5 7.2 14.3 7.5 \
                C13.4 7.7 12.7 8 12 8.3 Z";
    let leaf = "M12.2 6.4 C12.3 4.7 13.4 3.4 15.2 3.1 C15.1 4.8 14 6.1 12.2 6.4 Z";
    els![path(body, INK, SOLID), path(leaf, INK, SOLID)]
}

fn linux() -> Vec<String> {
    // A penguin: a solid body with the belly, eyes and beak cut out, standing on its feet.
    let body = "M12 3 C9.2 3 8.3 5.3 8.3 7.6 C8.3 9.4 7.2 10.6 6.4 12.4 C5.5 14.5 5.6 17.2 7.4 18.7 \
                C8.7 19.8 10.3 20 12 20 C13.7 20 15.3 19.8 16.6 18.7 C18.4 17.2 18.5 14.5 17.6 12.4 \
                C16.8 10.6 15.7 9.4 15.7 7.6 C15.7 5.3 14.8 3 12 3 Z";
    let cuts = [
        oval(12.0, 14.7, 3.2, 4.0),
        oval(10.7, 6.4, 0.85, 1.0),
        oval(13.3, 6.4, 0.85, 1.0),
        "M10.9 8.5 H13.1 L12 10 Z".to_owned(),
    ];
    els![
        path(&format!("{body} {}", cuts.join(" ")), INK, SOLID_CUT),
        path(&oval(8.4, 20.3, 2.6, 1.3), INK, SOLID),
        path(&oval(15.6, 20.3, 2.6, 1.3), INK, SOLID),
    ]
}

fn android() -> Vec<String> {
    // The robot's head: a solid dome with the eyes cut out, and two antennae.
    let (cx, cy, r) = (12.0, 17.5, 8.5);
    let dome = format!(
        "M{} {} A{} {} 0 0 1 {} {} Z",
        f(cx - r),
        f(cy),
        f(r),
        f(r),
        f(cx + r),
        f(cy)
    );
    let eyes = format!(
        "{} {}",
        oval(9.1, 14.0, 1.0, 1.0),
        oval(14.9, 14.0, 1.0, 1.0)
    );
    // The antennae leave the dome at 60 degrees from the vertical.
    let (ax, ay) = (r * 0.5, r * 0.866);
    let antennae = format!(
        "M{} {} L{} {} M{} {} L{} {}",
        f(cx - ax),
        f(cy - ay + 0.8),
        f(cx - ax - 1.3),
        f(cy - ay - 2.0),
        f(cx + ax),
        f(cy - ay + 0.8),
        f(cx + ax + 1.3),
        f(cy - ay - 2.0)
    );
    els![
        path(&format!("{dome} {eyes}"), INK, SOLID_CUT),
        path(&antennae, "", "")
    ]
}

fn ios() -> Vec<String> {
    // A solid phone with the camera pill cut out at the top of its screen.
    let phone = format!(
        "{} {}",
        rounded(6.0, 2.0, 12.0, 20.0, 2.75),
        rounded(10.0, 4.25, 4.0, 1.5, 0.75)
    );
    els![path(&phone, INK, SOLID_CUT)]
}

fn github() -> Vec<String> {
    // A disc with a cat cut out of it: head and ears, neck down to the rim, and a curled tail.
    let disc = oval(12.0, 12.0, 9.5, 9.5);
    let cat = "M10 21.2 V18.2 C7.4 17.7 6.2 16.2 6.2 13.3 C6.2 12 6.6 11 7.3 10.2 C7.1 9.4 7.1 8.3 7.5 7.2 \
               C8.6 7.2 9.6 7.8 10.3 8.3 C11.4 8 12.6 8 13.7 8.3 C14.4 7.8 15.4 7.2 16.5 7.2 \
               C16.9 8.3 16.9 9.4 16.7 10.2 C17.4 11 17.8 12 17.8 13.3 C17.8 16.2 16.6 17.7 14 18.2 V21.2 Z";
    let tail = "M10 20.2 C8.4 20.6 7.3 19.9 6.6 18.6 L7.4 18.2 C7.9 19.1 8.7 19.6 10 19.3 Z";
    els![path(&format!("{disc} {cat} {tail}"), INK, SOLID_CUT)]
}

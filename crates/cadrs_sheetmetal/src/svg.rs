//! An SVG picture of a flat pattern, for previews and for checking shapes by eye: the outline in
//! grey (holes white), relief cuts outlined, tangent lines thin, centrelines dashed (up bends
//! dark, down bends orange, with their names), and colliding pieces in red.

use std::fmt::Write as _;

use crate::flat::{FlatError, FlatPart, FlatPattern, PieceSource};
use crate::poly::{P2, Polygon};

fn path(poly: &Polygon, flip: impl Fn(P2) -> (f64, f64)) -> String {
    let mut d = String::new();
    for l in std::iter::once(&poly.outer).chain(poly.holes.iter()) {
        for (i, p) in l.iter().enumerate() {
            let (x, y) = flip(*p);
            let _ = write!(d, "{}{x:.4},{y:.4} ", if i == 0 { "M" } else { "L" });
        }
        d.push_str("Z ");
    }
    d
}

/// The flat pattern as an SVG document `px` wide, with `title` above it.
pub fn flat_svg(flat: &FlatPattern, title: &str, px: f64) -> String {
    let bounds = flat.parts.iter().filter_map(FlatPart::bounds).reduce(|(a, b), (c, d)| {
        (P2::new(a.x.min(c.x), a.y.min(c.y)), P2::new(b.x.max(d.x), b.y.max(d.y)))
    });
    let (lo, hi) = bounds.unwrap_or((P2::origin(), P2::new(1.0, 1.0)));
    let margin = 0.08 * (hi - lo).norm().max(1.0);
    let (w, h) = (hi.x - lo.x + 2.0 * margin, hi.y - lo.y + 2.0 * margin);
    let scale = px / w;
    let title_h = 28.0;
    let flip = |p: P2| ((p.x - lo.x + margin) * scale, title_h + (hi.y - p.y + margin) * scale);
    let colliding: Vec<PieceSource> = flat
        .errors
        .iter()
        .filter_map(|e| match e {
            FlatError::Collision { a, b, .. } => Some([*a, *b]),
            _ => None,
        })
        .flatten()
        .collect();
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{:.0}" height="{:.0}" font-family="Inter, sans-serif">"#,
        px,
        h * scale + title_h
    );
    let _ = writeln!(s, r#"<rect width="100%" height="100%" fill="white"/>"#);
    let status = if flat.is_ok() {
        String::new()
    } else {
        format!(" — {}", flat.errors[0].message())
    };
    let _ = writeln!(s, r#"<text x="8" y="19" font-size="14" fill="{}">{title}{status}</text>"#, if flat.is_ok() { "#222" } else { "#c62828" });
    for part in &flat.parts {
        for o in &part.outline {
            let _ = writeln!(s, r##"<path d="{}" fill="#c9cdd2" fill-rule="evenodd" stroke="#333" stroke-width="1"/>"##, path(o, flip));
        }
        for c in &part.cuts {
            for shape in &c.shapes {
                let _ = writeln!(s, r##"<path d="{}" fill="none" stroke="#7a7a7a" stroke-width="0.6" stroke-dasharray="2,2"/>"##, path(shape, flip));
            }
            if let Some(slit) = c.slit {
                let ((x1, y1), (x2, y2)) = (flip(slit.a), flip(slit.b));
                let _ = writeln!(s, r##"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="#333" stroke-width="1"/>"##);
            }
        }
        for pc in part.pieces.iter().filter(|pc| colliding.contains(&pc.source)) {
            for c in &pc.cut {
                let _ = writeln!(s, r##"<path d="{}" fill="#e53935" fill-opacity="0.2" stroke="#c62828" stroke-width="0.8"/>"##, path(c, flip));
            }
        }
        for e in &flat.errors {
            if let FlatError::Collision { region, .. } = e {
                for r in region {
                    let _ = writeln!(s, r##"<path d="{}" fill="#c62828" fill-opacity="0.75" stroke="none"/>"##, path(r, flip));
                }
            }
        }
        for b in &part.bends {
            let colour = if b.up { "#263238" } else { "#e65100" };
            for t in &b.tangent_visible {
                let ((x1, y1), (x2, y2)) = (flip(t.a), flip(t.b));
                let _ = writeln!(s, r##"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="#555" stroke-width="0.5"/>"##);
            }
            for c in &b.center_visible {
                let ((x1, y1), (x2, y2)) = (flip(c.a), flip(c.b));
                let _ = writeln!(
                    s,
                    r#"<line x1="{x1:.2}" y1="{y1:.2}" x2="{x2:.2}" y2="{y2:.2}" stroke="{colour}" stroke-width="1" stroke-dasharray="6,3"/>"#
                );
            }
            let ((x1, y1), (x2, y2)) = (flip(b.center.a), flip(b.center.b));
            let (mx, my) = ((x1 + x2) / 2.0, (y1 + y2) / 2.0);
            let _ = writeln!(
                s,
                r#"<text x="{mx:.1}" y="{my:.1}" font-size="11" fill="{colour}" text-anchor="middle" dy="-4">{} {}</text>"#,
                b.name,
                if b.up { "UP" } else { "DOWN" }
            );
        }
    }
    s.push_str("</svg>\n");
    s
}

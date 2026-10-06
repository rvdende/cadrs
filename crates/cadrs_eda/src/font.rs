//! The stroke font: text as polylines, for silkscreen, fabrication outputs, text extents and
//! the canvases. Glyphs are the Roman Simplex Hershey font (`fonts/romans.jhf`, see
//! `fonts/NOTICE`), ASCII only; other characters draw as `?`.
//!
//! A glyph's cap height is 21 font units (from `F` to `[` in the font's letter coordinates),
//! scaled so capitals are [`TextStyle::size`]'s height tall.

use crate::graphics::{HAlign, Text, TextStyle, VAlign};
use crate::units::{Nm, Pt};
use std::sync::OnceLock;

const DATA: &str = include_str!("fonts/romans.jhf");
/// Font units from the top of a capital to the baseline.
const CAP: f64 = 21.0;
/// Font units from the cap line to the font's origin row ('R').
const TOP: f64 = -12.0;
/// Line pitch, in cap heights.
const LINE_PITCH: f64 = 1.62;

struct Glyph {
    left: f64,
    right: f64,
    /// Strokes in font units, Y down.
    strokes: Vec<Vec<[f64; 2]>>,
}

fn glyphs() -> &'static Vec<Glyph> {
    static G: OnceLock<Vec<Glyph>> = OnceLock::new();
    G.get_or_init(|| {
        DATA.lines()
            .filter(|l| l.len() >= 10)
            .map(|l| {
                let b = l.as_bytes();
                let v = |c: u8| c as f64 - b'R' as f64;
                let body = &b[8..];
                let (left, right) = (v(body[0]), v(body[1]));
                let mut strokes = vec![vec![]];
                for pair in body[2..].chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    if pair == b" R" {
                        strokes.push(vec![]);
                    } else {
                        strokes.last_mut().unwrap().push([v(pair[0]), v(pair[1])]);
                    }
                }
                strokes.retain(|s: &Vec<[f64; 2]>| !s.is_empty());
                Glyph { left, right, strokes }
            })
            .collect()
    })
}

fn glyph(c: char) -> &'static Glyph {
    let g = glyphs();
    let i = (c as usize).wrapping_sub(32);
    g.get(i).filter(|_| (c as u32) < 127).unwrap_or(&g[('?' as usize) - 32])
}

/// The stroke width for a style: its own, else a ninth of the height (an eighth when bold).
pub fn stroke_width(style: &TextStyle) -> Nm {
    style.thickness.unwrap_or(if style.bold { style.size.h / 6 } else { style.size.h / 9 })
}

/// Width of one line of text in nanometres (advance widths, no stroke).
pub fn line_width(line: &str, style: &TextStyle) -> f64 {
    let k = style.size.w as f64 / CAP;
    line.chars().map(|c| {
        let g = glyph(c);
        (g.right - g.left) * k
    }).sum()
}

/// The text's strokes in its own frame: anchor at the origin, baseline along +X, Y up,
/// aligned by the style. Multi-line text stacks downwards.
pub fn local_strokes(text: &str, style: &TextStyle) -> Vec<Vec<[f64; 2]>> {
    let (kx, ky) = (style.size.w as f64 / CAP, style.size.h as f64 / CAP);
    let slant = if style.italic { 0.2 } else { 0.0 };
    let lines: Vec<&str> = text.split('\n').collect();
    let pitch = style.size.h as f64 * style.line_spacing.unwrap_or(1.0) * LINE_PITCH;
    let block = pitch * (lines.len() - 1) as f64;
    let h = style.size.h as f64;
    // The first line's baseline relative to the anchor.
    let first_base = match style.v_align {
        VAlign::Top => -h,
        VAlign::Center => -h / 2.0 + block / 2.0,
        VAlign::Bottom => block,
    };
    let mut out = vec![];
    for (i, line) in lines.iter().enumerate() {
        let w = line_width(line, style);
        let mut x = match style.h_align {
            HAlign::Left => 0.0,
            HAlign::Center => -w / 2.0,
            HAlign::Right => -w,
        };
        let base = first_base - pitch * i as f64;
        for c in line.chars() {
            let g = glyph(c);
            for s in &g.strokes {
                out.push(
                    s.iter()
                        .map(|p| {
                            // Font Y down from the cap line; ours up from the baseline.
                            let up = (CAP + TOP - p[1]) * ky;
                            [x + (p[0] - g.left) * kx + up * slant, base + up]
                        })
                        .collect(),
                );
            }
            x += (g.right - g.left) * kx;
        }
    }
    out
}

/// A text item's strokes on the page or board, drawn readable ([`Text::drawn`]) and mirrored
/// when its style says so.
pub fn strokes(t: &Text) -> Vec<Vec<Pt>> {
    let (angle, h, v) = t.drawn();
    let mut st = t.style.clone();
    st.h_align = h;
    st.v_align = v;
    let (s, c) = angle.to_radians().sin_cos();
    local_strokes(&t.text, &st)
        .into_iter()
        .map(|line| {
            line.into_iter()
                .map(|[x, y]| {
                    let x = if st.mirrored { -x } else { x };
                    Pt::new(t.at.x + (x * c - y * s).round() as Nm, t.at.y + (x * s + y * c).round() as Nm)
                })
                .collect()
        })
        .collect()
}

/// The box a text item covers on the page or board (its strokes, plus half the stroke width).
pub fn bounds(t: &Text) -> Option<crate::units::Bounds> {
    let half = stroke_width(&t.style) / 2;
    let mut b: Option<crate::units::Bounds> = None;
    for s in strokes(t) {
        for p in s {
            b = Some(crate::units::Bounds::union(b, crate::units::Bounds::of(p)));
        }
    }
    b.map(|b| b.grow(half))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Size, mm};

    fn style() -> TextStyle {
        TextStyle { size: Size::mm(1.0, 1.0), h_align: HAlign::Left, v_align: VAlign::Bottom, ..Default::default() }
    }

    #[test]
    fn capitals_are_the_text_height() {
        let s = local_strokes("E", &style());
        let ys: Vec<f64> = s.iter().flatten().map(|p| p[1]).collect();
        let (lo, hi) = (ys.iter().cloned().fold(f64::MAX, f64::min), ys.iter().cloned().fold(f64::MIN, f64::max));
        assert!(lo.abs() < 1.0, "baseline {lo}");
        assert!((hi - mm(1.0) as f64).abs() < 1.0, "cap {hi}");
    }

    #[test]
    fn alignment_and_angle() {
        let mut st = style();
        let w = line_width("R1", &st);
        assert!(w > mm(1.0) as f64 && w < mm(2.0) as f64, "{w}");
        st.h_align = HAlign::Right;
        let xs: Vec<f64> = local_strokes("R1", &st).iter().flatten().map(|p| p[0]).collect();
        assert!(xs.iter().all(|x| *x <= 1.0));
        // Turned a quarter: the text runs up from its anchor.
        let t = Text { text: "led".into(), at: Pt::ZERO, angle: 90.0, style: style(), visible: true };
        let b = bounds(&t).unwrap();
        assert!(b.size().h > b.size().w);
        assert!(b.min.y > -mm(0.2));
        // Pointing down reads upwards too (readable), sitting on the same side.
        let t2 = Text { angle: 270.0, ..t.clone() };
        assert!(bounds(&t2).unwrap().max.y < mm(0.2));
    }

    #[test]
    fn every_ascii_glyph_parses() {
        assert_eq!(glyphs().len(), 95);
        for c in ' '..='~' {
            let g = glyph(c);
            assert!(g.right > g.left, "{c:?}");
        }
        assert!(glyph(' ').strokes.is_empty());
        assert!(!glyph('Ω').strokes.is_empty());
    }
}

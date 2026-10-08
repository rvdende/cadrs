//! Pieces shared by the schematic and board readers: coordinates, strokes, fills and text
//! effects.

use crate::sexpr::Sexp;
use cadrs_eda::graphics::{Color, Fill, HAlign, LineStyle, Stroke, TextStyle, VAlign};
use cadrs_eda::units::{Pt, Size, mm};

/// KiCad's Y axis points down on sheets and boards (but up in symbol libraries). A `YAxis`
/// maps a KiCad point into cadrs's Y-up coordinates: `y' = origin + sign * y`.
#[derive(Clone, Copy, Debug)]
pub struct YAxis {
    pub sign: f64,
    pub origin: f64,
}

impl YAxis {
    /// Y already up (symbol libraries).
    pub const UP: YAxis = YAxis { sign: 1.0, origin: 0.0 };
    /// Y down about 0 (boards, footprints).
    pub const DOWN: YAxis = YAxis { sign: -1.0, origin: 0.0 };

    /// Y down from the top of a page `height` mm tall. The origin is the page height rounded up
    /// to the 50 mil schematic grid, so KiCad's grid (from the page's top-left corner) lands on
    /// cadrs's (from the bottom-left): an imported sheet can be edited without its symbols and
    /// wires snapping off each other. (A4's 210 mm becomes 210.82 mm: the drawing sits 0.82 mm
    /// higher on the page.)
    pub fn page(height: f64) -> YAxis {
        let grid = cadrs_eda::units::to_mm(cadrs_eda::units::SCHEMATIC_GRID);
        YAxis { sign: -1.0, origin: (height / grid - 1e-9).ceil() * grid }
    }

    pub fn pt(&self, x: f64, y: f64) -> Pt {
        Pt::mm(x, self.origin + self.sign * y)
    }

    /// `(at x y …)`, `(xy x y)`, `(start x y)`: the first two arguments as a point.
    pub fn of(&self, s: &Sexp) -> Pt {
        self.pt(s.f64_arg(0).unwrap_or(0.0), s.f64_arg(1).unwrap_or(0.0))
    }

    /// A child `(name x y)` as a point.
    pub fn child(&self, s: &Sexp, name: &str) -> Option<Pt> {
        s.find(name).map(|c| self.of(c))
    }

    /// A vector (no origin).
    pub fn vec(&self, x: f64, y: f64) -> Pt {
        Pt::mm(x, self.sign * y)
    }
}

/// `(at x y angle)`'s angle (0 when missing).
pub fn at_angle(s: &Sexp) -> f64 {
    s.find("at").and_then(|a| a.f64_arg(2)).unwrap_or(0.0)
}

/// `(pts (xy …) …)` as points. Arcs inside (`(arc (start)(mid)(end))`, board polygons) are
/// flattened.
pub fn pts(s: &Sexp, y: YAxis) -> Vec<Pt> {
    let Some(p) = s.find("pts") else { return vec![] };
    let mut out = vec![];
    for c in p.children() {
        match c.head() {
            "xy" => out.push(y.of(c)),
            "arc" => {
                let (a, m, b) = (y.child(c, "start"), y.child(c, "mid"), y.child(c, "end"));
                if let (Some(a), Some(m), Some(b)) = (a, m, b) {
                    let flat = cadrs_eda::geom::arc_points(a, m, b, mm(0.005));
                    out.extend(flat.iter().map(|q| Pt::new(q[0].round() as i64, q[1].round() as i64)));
                }
            }
            _ => {}
        }
    }
    out.dedup();
    out
}

pub fn color(s: &Sexp) -> Option<Color> {
    let c = s.find("color")?;
    let v: Vec<f64> = (0..4).filter_map(|i| c.f64_arg(i)).collect();
    if v.len() < 3 || v.iter().all(|x| *x == 0.0) {
        // (color 0 0 0 0) means "default".
        return None;
    }
    let a = v.get(3).copied().unwrap_or(1.0);
    Some(Color { r: v[0] as u8, g: v[1] as u8, b: v[2] as u8, a: (a * 255.0).round() as u8 })
}

pub fn stroke(s: &Sexp) -> Stroke {
    // Old board files: `(width w)` beside the shape instead of a stroke.
    let Some(st) = s.find("stroke") else {
        return Stroke::width(s.get_f64("width").map(mm).unwrap_or(0));
    };
    let style = match st.get("type") {
        Some("solid") => LineStyle::Solid,
        Some("dash") => LineStyle::Dash,
        Some("dot") => LineStyle::Dot,
        Some("dash_dot") => LineStyle::DashDot,
        Some("dash_dot_dot") => LineStyle::DashDotDot,
        _ => LineStyle::Default,
    };
    Stroke { width: st.get_f64("width").map(mm).unwrap_or(0), style, color: color(st) }
}

/// A shape's fill: `(fill (type none|outline|background|color) (color …))` (symbols),
/// `(fill solid|none|yes|no)` (boards).
pub fn fill(s: &Sexp) -> Fill {
    let Some(f) = s.find("fill") else { return Fill::None };
    match f.get("type").or_else(|| f.str_arg(0)) {
        Some("outline" | "solid" | "yes") => Fill::Outline,
        Some("background") => Fill::Background,
        Some("color") => color(f).map_or(Fill::Outline, Fill::Color),
        _ => Fill::None,
    }
}

/// `(effects (font (size h w) (thickness t) bold italic (face "…")) (justify …) (hide yes))`
/// as a style and whether it is visible. `hidden` is the parent's own `(hide yes)` too.
pub fn effects(parent: &Sexp) -> (TextStyle, bool) {
    let mut st = TextStyle::default();
    let mut visible = parent.flag("hide") != Some(true);
    if let Some(e) = parent.find("effects") {
        if let Some(f) = e.find("font") {
            if let Some(sz) = f.find("size") {
                // Height first.
                let h = sz.f64_arg(0).unwrap_or(1.27);
                st.size = Size::mm(sz.f64_arg(1).unwrap_or(h), h);
            }
            st.thickness = f.get_f64("thickness").map(mm);
            st.bold = f.flag("bold") == Some(true);
            st.italic = f.flag("italic") == Some(true);
            st.font = f.get("face").map(str::to_string);
            st.color = color(f);
            st.line_spacing = f.get_f64("line_spacing");
        }
        if let Some(j) = e.find("justify") {
            if j.has_atom("left") {
                st.h_align = HAlign::Left;
            } else if j.has_atom("right") {
                st.h_align = HAlign::Right;
            }
            if j.has_atom("top") {
                st.v_align = VAlign::Top;
            } else if j.has_atom("bottom") {
                st.v_align = VAlign::Bottom;
            }
            st.mirrored = j.has_atom("mirror");
        }
        if e.flag("hide") == Some(true) {
            visible = false;
        }
    }
    (st, visible)
}

/// `(uuid "…")` (or the old `(tstamp …)`), or a new one.
pub fn uuid(s: &Sexp) -> uuid::Uuid {
    s.get("uuid").or_else(|| s.get("tstamp")).and_then(|u| uuid::Uuid::parse_str(u).ok()).unwrap_or_else(uuid::Uuid::new_v4)
}

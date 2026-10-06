//! Shared graphics: strokes, fills, shapes and text, used by symbols, schematics, footprints
//! and boards.

use crate::units::{Nm, Pt, Size, mm};
use serde::{Deserialize, Serialize};

/// An RGBA colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LineStyle {
    /// Whatever the item's kind uses (solid for most, dashed for some).
    #[default]
    Default,
    Solid,
    Dash,
    Dot,
    DashDot,
    DashDotDot,
}

/// How a shape's outline is drawn. A width of 0 means the default width of its kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Stroke {
    pub width: Nm,
    pub style: LineStyle,
    pub color: Option<Color>,
}

impl Stroke {
    pub fn width(width: Nm) -> Stroke {
        Stroke { width, ..Default::default() }
    }
}

/// How a closed shape's inside is drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Fill {
    #[default]
    None,
    /// The outline's colour (a filled body, a solid pad).
    Outline,
    /// The symbol body background colour.
    Background,
    Color(Color),
}

/// The geometry of a shape.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Geom {
    Line { a: Pt, b: Pt },
    /// Joined segments; `closed` joins the last point to the first.
    Polyline { pts: Vec<Pt>, closed: bool },
    /// Corners `a` and `b`, axis-aligned.
    Rect { a: Pt, b: Pt },
    Circle { center: Pt, radius: Nm },
    /// Through `start`, `mid` and `end`.
    Arc { start: Pt, mid: Pt, end: Pt },
    /// A cubic Bézier.
    Bezier { pts: [Pt; 4] },
}

impl Geom {
    /// Moved by `d`.
    pub fn translated(&self, d: Pt) -> Geom {
        self.map(|p| p + d)
    }

    /// Every point passed through `f` (a rigid motion or a mirror; a circle's radius is kept).
    /// A rectangle stays a rectangle only under quarter turns: callers that rotate by other
    /// angles convert it to a polygon first ([`Geom::rect_as_polyline`]).
    pub fn map(&self, f: impl Fn(Pt) -> Pt) -> Geom {
        match self {
            Geom::Line { a, b } => Geom::Line { a: f(*a), b: f(*b) },
            Geom::Polyline { pts, closed } => Geom::Polyline { pts: pts.iter().map(|p| f(*p)).collect(), closed: *closed },
            Geom::Rect { a, b } => Geom::Rect { a: f(*a), b: f(*b) },
            Geom::Circle { center, radius } => Geom::Circle { center: f(*center), radius: *radius },
            Geom::Arc { start, mid, end } => Geom::Arc { start: f(*start), mid: f(*mid), end: f(*end) },
            Geom::Bezier { pts } => Geom::Bezier { pts: pts.map(&f) },
        }
    }

    /// A rectangle as its closed polygon (other shapes unchanged).
    pub fn rect_as_polyline(&self) -> Geom {
        match self {
            Geom::Rect { a, b } => Geom::Polyline { pts: vec![*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)], closed: true },
            g => g.clone(),
        }
    }

    /// The points that bound it (corners, ends; a circle's or arc's box corners).
    pub fn extent(&self) -> Vec<Pt> {
        match self {
            Geom::Line { a, b } | Geom::Rect { a, b } => vec![*a, *b],
            Geom::Polyline { pts, .. } => pts.clone(),
            Geom::Circle { center, radius } => {
                vec![*center - Pt::new(*radius, *radius), *center + Pt::new(*radius, *radius)]
            }
            Geom::Arc { start, mid, end } => {
                crate::geom::arc_points(*start, *mid, *end, mm(0.01)).into_iter().map(|[x, y]| Pt::new(x as Nm, y as Nm)).collect()
            }
            Geom::Bezier { pts } => pts.to_vec(),
        }
    }
}

/// A drawn shape: geometry, outline and fill.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Shape {
    pub geom: Geom,
    pub stroke: Stroke,
    pub fill: Fill,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HAlign {
    Left,
    #[default]
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum VAlign {
    Top,
    #[default]
    Center,
    Bottom,
}

/// How text looks. `size` is a glyph's width and height.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub size: Size,
    /// Stroke thickness; `None` is the default for the size.
    pub thickness: Option<Nm>,
    pub bold: bool,
    pub italic: bool,
    pub h_align: HAlign,
    pub v_align: VAlign,
    /// Mirrored (text on the bottom side reads mirrored from the top).
    pub mirrored: bool,
    /// A font name; `None` is the default stroke font.
    pub font: Option<String>,
    pub color: Option<Color>,
    pub line_spacing: Option<f64>,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            size: Size::new(crate::units::SCHEMATIC_GRID, crate::units::SCHEMATIC_GRID),
            thickness: None,
            bold: false,
            italic: false,
            h_align: HAlign::Center,
            v_align: VAlign::Center,
            mirrored: false,
            font: None,
            color: None,
            line_spacing: None,
        }
    }
}

/// A piece of text placed at a point, its baseline running in direction `angle` (degrees
/// counter-clockwise). Text is drawn readable: see [`readable`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Text {
    pub text: String,
    pub at: Pt,
    pub angle: f64,
    pub style: TextStyle,
    pub visible: bool,
}

impl Text {
    pub fn new(text: impl Into<String>, at: Pt) -> Text {
        Text { text: text.into(), at, angle: 0.0, style: TextStyle::default(), visible: true }
    }

    /// How it is drawn: [`readable`] applied to its angle and alignment.
    pub fn drawn(&self) -> (f64, HAlign, VAlign) {
        readable(self.angle, self.style.h_align, self.style.v_align)
    }
}

/// Schematic-style readable text: text whose direction would read right-to-left or downwards
/// (an angle in (90°, 270°]) is drawn half a turn around, with both alignments swapped so it
/// still sits on the same side of its anchor. Returns the drawn angle and alignments.
pub fn readable(angle: f64, h: HAlign, v: VAlign) -> (f64, HAlign, VAlign) {
    let a = crate::units::normalize_deg(angle);
    if a > 90.0 && a <= 270.0 {
        let h = match h {
            HAlign::Left => HAlign::Right,
            HAlign::Right => HAlign::Left,
            c => c,
        };
        let v = match v {
            VAlign::Top => VAlign::Bottom,
            VAlign::Bottom => VAlign::Top,
            c => c,
        };
        (a - 180.0, h, v)
    } else {
        (a, h, v)
    }
}

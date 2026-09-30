//! Sketch text (S16): a string laid out in a font, filling a construction box.
//!
//! Following Onshape's help (`reference/onshape/t5/t5.md`): the text tool draws a box whose lower
//! edge is the text's baseline and whose height runs from the baseline to the top of the
//! capitals, so the text fills it. The box is four construction lines (a rectangle, its lower
//! edge Horizontal) and its width is tied to its height by the string's aspect ratio
//! ([`crate::ConstraintOf::TextAspect`]), so only one of width and height can be dimensioned.
//!
//! The glyph outlines come from the embedded Inter fonts (`assets/fonts`, OFL), flattened into
//! closed polylines. Italic is a synthetic 12° slant; bold uses the next heavier Inter face.
//! Mirroring flips the text inside its box. The outlines take part in the region finder like
//! curves, so text regions can be selected and extruded.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::{CurveId, PointId, Sketch, TextId, Vec2};

/// The fonts the text dialog offers: the Inter faces cadrs ships.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextFont {
    #[default]
    Inter,
    InterMedium,
    InterSemiBold,
    InterBlack,
}

impl TextFont {
    pub const ALL: [TextFont; 4] = [
        TextFont::Inter,
        TextFont::InterMedium,
        TextFont::InterSemiBold,
        TextFont::InterBlack,
    ];

    /// The name shown in the font dropdown.
    pub fn label(self) -> &'static str {
        match self {
            TextFont::Inter => "Inter",
            TextFont::InterMedium => "Inter Medium",
            TextFont::InterSemiBold => "Inter SemiBold",
            TextFont::InterBlack => "Inter Black",
        }
    }

    /// The font file for this face, bold or not.
    fn data(self, bold: bool) -> &'static [u8] {
        static REGULAR: &[u8] = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
        static MEDIUM: &[u8] = include_bytes!("../../../assets/fonts/Inter-Medium.ttf");
        static SEMIBOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-SemiBold.ttf");
        static BOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-Bold.ttf");
        static EXTRABOLD: &[u8] = include_bytes!("../../../assets/fonts/Inter-ExtraBold.ttf");
        static BLACK: &[u8] = include_bytes!("../../../assets/fonts/Inter-Black.ttf");
        match (self, bold) {
            (TextFont::Inter, false) => REGULAR,
            (TextFont::Inter, true) => BOLD,
            (TextFont::InterMedium, false) => MEDIUM,
            (TextFont::InterMedium, true) => EXTRABOLD,
            (TextFont::InterSemiBold, false) => SEMIBOLD,
            (TextFont::InterSemiBold, true) => EXTRABOLD,
            (TextFont::InterBlack, _) => BLACK,
        }
    }
}

/// The embedded Inter face closest to `weight` (400 Regular, 500 Medium, 600 SemiBold, 700 Bold,
/// 800 ExtraBold, 900 Black), or Inter Italic (regular weight, a Latin subset) when `italic` and
/// the weight is under 650. For measuring text the app renders with those faces (drawing notes,
/// P3C.4).
pub fn inter_data(weight: u16, italic: bool) -> &'static [u8] {
    static ITALIC: &[u8] = include_bytes!("../../../assets/fonts/Inter-Italic.ttf");
    match (weight, italic) {
        (0..=649, true) => ITALIC,
        (0..=449, _) => TextFont::Inter.data(false),
        (450..=549, _) => TextFont::InterMedium.data(false),
        (550..=649, _) => TextFont::InterSemiBold.data(false),
        (650..=749, _) => TextFont::Inter.data(true),
        (750..=849, _) => TextFont::InterMedium.data(true),
        _ => TextFont::InterBlack.data(false),
    }
}

/// What the text dialog sets.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct TextStyle {
    pub text: String,
    #[serde(default)]
    pub font: TextFont,
    #[serde(default)]
    pub bold: bool,
    #[serde(default)]
    pub italic: bool,
    /// Flipped left to right, about the box's vertical centre line.
    #[serde(default)]
    pub mirror_h: bool,
    /// Flipped upside down, about the box's horizontal centre line.
    #[serde(default)]
    pub mirror_v: bool,
}

/// Onshape's limit on a text box's characters.
pub const MAX_CHARS: usize = 250;

impl TextStyle {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            ..Self::default()
        }
    }
}

/// A text entity: its style and its box (corners from the lower left, counter-clockwise, and
/// the four construction lines: bottom, right, top, left).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SketchText {
    pub style: TextStyle,
    pub corners: [PointId; 4],
    pub lines: [CurveId; 4],
}

/// A laid-out string in box units: the baseline is y = 0, the capitals' top y = 1, and the
/// advance ends at x = `width`.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// The box's width over its height.
    pub width: f64,
    /// Closed outlines (first point not repeated).
    pub contours: Vec<Vec<Vec2>>,
}

/// The slant of synthetic italics.
const ITALIC_SLANT: f64 = 0.2126; // tan 12°
/// Segments per quadratic and cubic Bézier piece.
const QUAD_STEPS: usize = 6;
const CUBIC_STEPS: usize = 8;

thread_local! {
    static CACHE: RefCell<HashMap<TextStyle, Rc<Layout>>> = RefCell::new(HashMap::new());
}

/// Lays out a style's text (cached).
pub fn layout(style: &TextStyle) -> Rc<Layout> {
    if let Some(l) = CACHE.with(|c| c.borrow().get(style).cloned()) {
        return l;
    }
    let l = Rc::new(layout_uncached(style));
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 64 {
            c.clear();
        }
        c.insert(style.clone(), l.clone());
    });
    l
}

struct Flatten {
    contours: Vec<Vec<Vec2>>,
    current: Vec<Vec2>,
    last: Vec2,
    scale: f64,
    x0: f64,
}

impl Flatten {
    fn p(&self, x: f32, y: f32) -> Vec2 {
        Vec2::new(self.x0 + x as f64 * self.scale, y as f64 * self.scale)
    }
    fn push(&mut self, p: Vec2) {
        if self.current.last().is_none_or(|q| q.distance(p) > 1e-9) {
            self.current.push(p);
        }
        self.last = p;
    }
    fn finish(&mut self) {
        let mut c = std::mem::take(&mut self.current);
        if c.len() > 1 && c[0].distance(c[c.len() - 1]) < 1e-9 {
            c.pop();
        }
        if c.len() >= 3 {
            self.contours.push(c);
        }
    }
}

impl ttf_parser::OutlineBuilder for Flatten {
    fn move_to(&mut self, x: f32, y: f32) {
        self.finish();
        let p = self.p(x, y);
        self.push(p);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.p(x, y);
        self.push(p);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (p0, p1, p2) = (self.last, self.p(x1, y1), self.p(x, y));
        for i in 1..=QUAD_STEPS {
            let t = i as f64 / QUAD_STEPS as f64;
            let u = 1.0 - t;
            self.push(p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t));
        }
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (p0, p1, p2, p3) = (self.last, self.p(x1, y1), self.p(x2, y2), self.p(x, y));
        for i in 1..=CUBIC_STEPS {
            let t = i as f64 / CUBIC_STEPS as f64;
            let u = 1.0 - t;
            self.push(
                p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t),
            );
        }
    }
    fn close(&mut self) {
        self.finish();
    }
}

fn layout_uncached(style: &TextStyle) -> Layout {
    let Ok(face) = ttf_parser::Face::parse(style.font.data(style.bold), 0) else {
        return Layout {
            width: 1.0,
            contours: Vec::new(),
        };
    };
    let cap = face
        .capital_height()
        .filter(|c| *c > 0)
        .map_or(face.units_per_em() as f64 * 0.727, |c| c as f64);
    let scale = 1.0 / cap;
    let space = face
        .glyph_index(' ')
        .and_then(|g| face.glyph_hor_advance(g))
        .unwrap_or(face.units_per_em() / 4) as f64;
    let mut f = Flatten {
        contours: Vec::new(),
        current: Vec::new(),
        last: Vec2::ZERO,
        scale,
        x0: 0.0,
    };
    let mut x = 0.0;
    // Only the first line goes in the box.
    for ch in style.text.lines().next().unwrap_or("").chars() {
        let Some(g) = face.glyph_index(ch) else {
            x += space;
            continue;
        };
        f.x0 = x * scale;
        face.outline_glyph(g, &mut f);
        f.finish();
        x += face.glyph_hor_advance(g).map_or(space, |a| a as f64);
    }
    let width = if x > 0.0 { x * scale } else { 1.0 };
    let mut contours = f.contours;
    for c in &mut contours {
        for p in c.iter_mut() {
            if style.italic {
                p.x += p.y * ITALIC_SLANT;
            }
            if style.mirror_h {
                p.x = width - p.x;
            }
            if style.mirror_v {
                p.y = 1.0 - p.y;
            }
        }
    }
    Layout { width, contours }
}

/// The box's width over its height for a style.
pub fn aspect(style: &TextStyle) -> f64 {
    layout(style).width
}

/// The corners of a text box from its lower-left corner `origin`, its baseline direction
/// `dir` (unit) and its height: lower left, lower right, upper right, upper left.
pub fn box_corners(style: &TextStyle, origin: Vec2, dir: Vec2, height: f64) -> [Vec2; 4] {
    let w = aspect(style) * height;
    let up = dir.perp();
    [
        origin,
        origin + dir * w,
        origin + dir * w + up * height,
        origin + up * height,
    ]
}

/// A text's outlines in sketch coordinates, fitted to its box as it is now.
pub fn outlines(s: &Sketch, id: TextId) -> Vec<Vec<Vec2>> {
    let Some(t) = s.texts.get(id) else {
        return Vec::new();
    };
    let [a, b, _, d] = t.corners.map(|p| s.pos(p));
    let l = layout(&t.style);
    let ex = (b - a) / l.width.max(1e-9);
    let ey = d - a;
    l.contours
        .iter()
        .map(|c| c.iter().map(|p| a + ex * p.x + ey * p.y).collect())
        .collect()
}

/// The text a box line or corner belongs to.
pub fn text_of_curve(s: &Sketch, c: CurveId) -> Option<TextId> {
    s.texts.iter().find(|(_, t)| t.lines.contains(&c)).map(|(k, _)| k)
}

pub fn text_of_point(s: &Sketch, p: PointId) -> Option<TextId> {
    s.texts.iter().find(|(_, t)| t.corners.contains(&p)).map(|(k, _)| k)
}

/// The synthetic curve ids of a text's outline pieces: contour `c`, run `r` (the outline is
/// split into runs at sharp corners so an extrusion gets flat sides there and smooth ones
/// along curves).
pub fn piece_id(text: TextId, contour: usize, run: usize) -> CurveId {
    use slotmap::Key;
    let t = (text.data().as_ffi() & 0xFFF) as u32;
    crate::synthetic_curve(2 + t, ((contour as u32 & 0x3FFF) << 12) | (run as u32 & 0xFFF))
}

/// Sharper turns than this (radians) start a new run.
const CORNER: f64 = 0.6;

/// For each segment of a closed contour (from point `i` to `i + 1`), which run it is in.
pub fn contour_runs(c: &[Vec2]) -> Vec<usize> {
    let n = c.len();
    let dir = |i: usize| (c[(i + 1) % n] - c[i]).normalize();
    let corner = |i: usize| {
        let (a, b) = (dir((i + n - 1) % n), dir(i));
        a.cross(b).atan2(a.dot(b)).abs() > CORNER
    };
    let start = (0..n).find(|&i| corner(i));
    let Some(start) = start else {
        return vec![0; n];
    };
    let mut out = vec![0; n];
    let mut run = 0;
    for k in 0..n {
        let i = (start + k) % n;
        if k > 0 && corner(i) {
            run += 1;
        }
        out[i] = run;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraint::{ConstraintOf, PointRef};
    use crate::geom::polygon_area;

    #[test]
    fn layout_fills_the_box() {
        let l = layout(&TextStyle::new("HI"));
        assert!(l.width > 1.0 && l.width < 3.0, "{}", l.width);
        let (mut lo, mut hi) = (f64::MAX, f64::MIN);
        let (mut xl, mut xh) = (f64::MAX, f64::MIN);
        for c in &l.contours {
            for p in c {
                lo = lo.min(p.y);
                hi = hi.max(p.y);
                xl = xl.min(p.x);
                xh = xh.max(p.x);
            }
        }
        // Capitals run from the baseline to the box's top.
        assert!(lo.abs() < 1e-6 && (hi - 1.0).abs() < 1e-6, "{lo} {hi}");
        assert!(xl >= 0.0 && xh <= l.width + 1e-9);
        // H and I: 3 closed outlines... H is one contour, I is one.
        assert_eq!(l.contours.len(), 2);
    }

    #[test]
    fn aspect_follows_the_string_and_style() {
        let one = aspect(&TextStyle::new("A"));
        let two = aspect(&TextStyle::new("AA"));
        assert!((two - 2.0 * one).abs() < 1e-9);
        let bold = aspect(&TextStyle {
            bold: true,
            ..TextStyle::new("A")
        });
        assert!(bold > one);
        // The box corners have the aspect.
        let c = box_corners(&TextStyle::new("AA"), Vec2::ZERO, Vec2::new(1.0, 0.0), 10.0);
        assert!((c[1].x - 10.0 * two).abs() < 1e-9 && (c[3].y - 10.0).abs() < 1e-9);
    }

    #[test]
    fn mirroring_flips_inside_the_box() {
        let plain = layout(&TextStyle::new("L"));
        let h = layout(&TextStyle {
            mirror_h: true,
            ..TextStyle::new("L")
        });
        let v = layout(&TextStyle {
            mirror_v: true,
            ..TextStyle::new("L")
        });
        let w = plain.width;
        for (a, b) in plain.contours[0].iter().zip(&h.contours[0]) {
            assert!((a.x - (w - b.x)).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9);
        }
        for (a, b) in plain.contours[0].iter().zip(&v.contours[0]) {
            assert!((a.y - (1.0 - b.y)).abs() < 1e-9 && (a.x - b.x).abs() < 1e-9);
        }
    }

    fn text_sketch(s: &str) -> (Sketch, crate::TextId) {
        let mut sk = Sketch::new();
        crate::SketchOp::AddText {
            origin: Vec2::new(0.0, 0.0),
            dir: Vec2::new(1.0, 0.0),
            height: 10.0,
            style: TextStyle::new(s),
        }
        .apply(&mut sk)
        .unwrap();
        let id = sk.texts.keys().next().unwrap();
        (sk, id)
    }

    #[test]
    fn text_regions_have_area() {
        let (s, _) = text_sketch("HI");
        let r = crate::region::regions(&s);
        // One region per letter (the box is construction).
        assert_eq!(r.len(), 2, "{}", r.len());
        assert!(r.iter().all(|r| r.area() > 1.0));
        // "O" makes the letter (with a hole) and its counter.
        let (s, _) = text_sketch("O");
        let r = crate::region::regions(&s);
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|r| r.holes.len() == 1));
    }

    #[test]
    fn the_box_keeps_the_aspect_and_one_dimension_defines_it() {
        use crate::solve;
        let (mut s, id) = text_sketch("AB");
        let k = aspect(&s.texts[id].style);
        let t = s.texts[id].clone();
        let [a, b, _, d] = t.corners.map(|p| s.pos(p));
        assert!((a.distance(b) - 10.0 * k).abs() < 1e-6);
        assert!((a.distance(d) - 10.0).abs() < 1e-6);
        // Fix the corner and dimension the height: fully defined.
        crate::SketchOp::AddConstraint {
            constraints: vec![ConstraintOf::FixPoint(PointRef::Point(t.corners[0]))],
            label: "Add fix",
        }
        .apply(&mut s)
        .unwrap();
        crate::SketchOp::SetDimension {
            dimension: crate::Dimension::new(
                crate::DimensionKind::Aligned { a: t.corners[0], b: t.corners[3] },
                20.0,
                -5.0,
            ),
            moves: vec![],
            radii: vec![],
        }
        .apply(&mut s)
        .unwrap();
        let [a, b, _, d] = t.corners.map(|p| s.pos(p));
        assert!((a.distance(d) - 20.0).abs() < 1e-6);
        assert!((a.distance(b) - 20.0 * k).abs() < 1e-6);
        assert!(solve::analyze(&s).fully_constrained());
        // Without its Horizontal the box can turn.
        let h = s
            .constraints
            .iter()
            .find(|(_, c)| matches!(c, ConstraintOf::Horizontal(_)))
            .unwrap()
            .0;
        crate::SketchOp::Delete {
            curves: vec![],
            points: vec![],
            dimensions: vec![],
            constraints: vec![h],
        }
        .apply(&mut s)
        .unwrap();
        assert_eq!(solve::analyze(&s).dof, 1);
        // Edit text: the box keeps its corner and height, and its width follows.
        crate::SketchOp::EditText {
            id,
            style: TextStyle { bold: true, ..TextStyle::new("ABC") },
        }
        .apply(&mut s)
        .unwrap();
        let k3 = aspect(&s.texts[id].style);
        let [a, b, _, d] = t.corners.map(|p| s.pos(p));
        assert!((a.distance(d) - 20.0).abs() < 1e-6 && (a.distance(b) - 20.0 * k3).abs() < 1e-6);
        // Deleting a box line deletes the text.
        crate::SketchOp::Delete {
            curves: vec![t.lines[0]],
            points: vec![],
            dimensions: vec![],
            constraints: vec![],
        }
        .apply(&mut s)
        .unwrap();
        assert!(s.texts.is_empty() && s.curves.is_empty());
        assert!(!s.constraints.values().any(|c| matches!(c, ConstraintOf::TextAspect(_))));
    }

    #[test]
    fn texts_are_saved() {
        let (s, _) = text_sketch("Hi");
        let text = ron::to_string(&s).unwrap();
        let back: Sketch = ron::from_str(&text).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn outlines_are_closed_loops_with_area() {
        let l = layout(&TextStyle::new("O"));
        // O: an outer and an inner loop.
        assert_eq!(l.contours.len(), 2);
        let areas: Vec<f64> = l.contours.iter().map(|c| polygon_area(c).abs()).collect();
        assert!(areas.iter().all(|a| *a > 0.01), "{areas:?}");
        let runs = contour_runs(&l.contours[0]);
        assert_eq!(runs.len(), l.contours[0].len());
    }
}

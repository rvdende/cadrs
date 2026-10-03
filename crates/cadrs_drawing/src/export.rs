//! A sheet as a printable page (P3C.7, D2.10, X13): every line, arc, fill, text and image of a
//! sheet in sheet millimetres, which the PDF ([`crate::pdf`]), DXF ([`crate::dxf`]) and raster
//! ([`crate::raster`]) writers turn into files.
//!
//! [`sheet_page`] collects, back to front: the border and title block, shaded views, images,
//! the views' lines (visible, hidden dashed, tangent, phantom, shown sketches: straight edges as
//! lines, circular edges as arcs and circles, the rest as polylines), the annotations
//! (centerlines, dimensions, callouts: thin strokes, filled arrowheads, text), notes, tables and
//! the sheet sketch (lines, splines, inserted DXF blocks). Selection colours are not exported:
//! everything is ink, except dangling annotations (red, P3C.6) in colour output.

use std::collections::HashMap;

use crate::annotation::{ViewModel, annotation_graphics};
use crate::graphics::{Align, Weight};
use crate::note::note_graphics;
use crate::rich::{self, CharStyle, DrawingContext, FieldContext};
use crate::sheet_sketch::{self as sk, Entity, ItemKind};
use crate::table::table_graphics;
use crate::view::{LineKind, View, ViewId, view_lines};
use crate::{Drawing, ReferenceProps};
use cadrs_kernel::ProjCurve;

pub type P2 = [f64; 2];
pub type Rgb = [u8; 3];

/// Sheet ink.
pub const INK: Rgb = [0x1a, 0x1a, 0x1a];
/// Annotations whose references are gone (P3C.6).
pub const DANGLING: Rgb = [0xe0, 0x3c, 0x1f];

/// What a stroke belongs to (DXF layers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    Border,
    Visible,
    Hidden,
    Tangent,
    Phantom,
    Sketch,
    Annotation,
    Note,
    Table,
    SheetSketch,
    Import,
    Shaded,
    Image,
    /// Section hatching, thread marks, break lines and cutting lines (P3C.8).
    Hatch,
    /// P3I.6 (SM15): a sheet metal flat pattern's outer outline, its cut-outs, its tear
    /// reliefs' slits, its up and down bend centrelines, its bend tangent lines and the
    /// sketches on it.
    FlatOutline,
    FlatCutout,
    FlatSlit,
    BendUp,
    BendDown,
    BendTangent,
    FlatSketch,
}

impl Layer {
    pub const ALL: [Layer; 21] = [
        Layer::Border,
        Layer::Visible,
        Layer::Hidden,
        Layer::Tangent,
        Layer::Phantom,
        Layer::Sketch,
        Layer::Annotation,
        Layer::Note,
        Layer::Table,
        Layer::SheetSketch,
        Layer::Import,
        Layer::Shaded,
        Layer::Image,
        Layer::Hatch,
        Layer::FlatOutline,
        Layer::FlatCutout,
        Layer::FlatSlit,
        Layer::BendUp,
        Layer::BendDown,
        Layer::BendTangent,
        Layer::FlatSketch,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Layer::Border => "BORDER",
            Layer::Visible => "VISIBLE",
            Layer::Hidden => "HIDDEN",
            Layer::Tangent => "TANGENT",
            Layer::Phantom => "PHANTOM",
            Layer::Sketch => "VIEW_SKETCH",
            Layer::Annotation => "ANNOTATION",
            Layer::Note => "NOTES",
            Layer::Table => "TABLES",
            Layer::SheetSketch => "SHEET_SKETCH",
            Layer::Import => "IMPORT",
            Layer::Shaded => "SHADED",
            Layer::Image => "IMAGES",
            Layer::Hatch => "HATCH",
            Layer::FlatOutline => "OUTLINE",
            Layer::FlatCutout => "CUTOUTS",
            Layer::FlatSlit => "TEAR_SLITS",
            Layer::BendUp => "BEND_UP",
            Layer::BendDown => "BEND_DOWN",
            Layer::BendTangent => "BEND_TANGENT",
            Layer::FlatSketch => "FLAT_SKETCH",
        }
    }

    /// The DXF linetype of the layer's strokes.
    pub fn linetype(self) -> &'static str {
        match self {
            Layer::Hidden => "HIDDEN",
            Layer::Phantom => "PHANTOM",
            // Bend centrelines (the flat export, P3I.6, and flat pattern views, P3I.7): CENTER,
            // up and down told apart by their layers' colours.
            Layer::BendUp | Layer::BendDown => "CENTER",
            _ => "CONTINUOUS",
        }
    }

    /// P3I.6: a flat pattern layer (written to a DXF's layer table only when used, so other
    /// drawings' files stay as they were).
    pub fn is_flat(self) -> bool {
        matches!(self, Layer::FlatOutline | Layer::FlatCutout | Layer::FlatSlit | Layer::BendUp | Layer::BendDown | Layer::BendTangent | Layer::FlatSketch)
    }

    /// The layer's AutoCAD colour index (7: black/white).
    pub fn aci(self) -> i32 {
        match self {
            Layer::FlatCutout => 5,
            Layer::FlatSlit => 6,
            Layer::BendUp => 3,
            Layer::BendDown => 1,
            Layer::BendTangent => 8,
            Layer::FlatSketch => 4,
            _ => 7,
        }
    }
}

/// How a stroke is drawn.
#[derive(Debug, Clone, PartialEq)]
pub struct Pen {
    /// Paper width (mm).
    pub width: f64,
    pub color: Rgb,
    /// Dash pattern on paper (mm: dash, gap, …), `None` for continuous.
    pub dash: Option<Vec<f64>>,
    pub layer: Layer,
}

impl Pen {
    pub fn new(width: f64, layer: Layer) -> Self {
        Self { width, color: INK, dash: None, layer }
    }
}

/// A stroked shape.
#[derive(Debug, Clone, PartialEq)]
pub enum Shape {
    Line { a: P2, b: P2 },
    Polyline { points: Vec<P2>, closed: bool },
    /// Counter-clockwise from `start` to `end` (degrees).
    Arc { center: P2, radius: f64, start: f64, end: f64 },
    Circle { center: P2, radius: f64 },
    /// A cubic B-spline (DXF SPLINE); `points` is its polyline for PDF and raster.
    Spline { knots: Vec<f64>, control: Vec<P2>, fit: Vec<P2>, points: Vec<P2> },
}

impl Shape {
    /// The shape as a polyline.
    pub fn polyline(&self) -> Vec<P2> {
        match self {
            Shape::Line { a, b } => vec![*a, *b],
            Shape::Polyline { points, closed } => {
                let mut p = points.clone();
                if *closed && let Some(f) = points.first() {
                    p.push(*f);
                }
                p
            }
            Shape::Arc { center, radius, start, end } => sk::arc_polyline(*center, *radius, *start, *end),
            Shape::Circle { center, radius } => sk::arc_polyline(*center, *radius, 0.0, 360.0),
            Shape::Spline { points, .. } => points.clone(),
        }
    }
}

/// A text: `pos` is the left end of its baseline.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    pub pos: P2,
    /// Cap height (mm).
    pub height: f64,
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    /// Degrees counter-clockwise about `pos`.
    pub rotation: f64,
    pub color: Rgb,
    pub layer: Layer,
}

/// A picture on the page.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    /// Bottom-left corner and size (mm).
    pub at: P2,
    pub width: f64,
    pub height: f64,
    /// The file's bytes (PNG or JPEG).
    pub data: std::sync::Arc<Vec<u8>>,
}

/// One thing on a page.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    /// A character drawn as vector strokes (the counterbore ⌴, depth ↧, countersink ⌵, arc ⌒
    /// and GD&T symbols, P3C.8): the strokes are separate items; a PDF puts the character here
    /// as ActualText so it extracts as text, other formats ignore it.
    Symbol { pos: P2, height: f64, ch: char },
    Stroke(Shape, Pen),
    /// A filled polygon.
    Fill { points: Vec<P2>, color: Rgb, layer: Layer },
    Text(Text),
    Image(Picture),
}

/// One sheet as a page.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub name: String,
    /// Paper size (mm).
    pub width: f64,
    pub height: f64,
    pub items: Vec<Item>,
}

impl Page {
    fn stroke(&mut self, s: Shape, p: Pen) {
        self.items.push(Item::Stroke(s, p));
    }

    fn polyline(&mut self, pts: Vec<P2>, p: Pen) {
        if pts.len() < 2 {
            return;
        }
        if pts.len() == 2 {
            self.stroke(Shape::Line { a: pts[0], b: pts[1] }, p);
        } else {
            self.stroke(Shape::Polyline { points: pts, closed: false }, p);
        }
    }

    fn fill(&mut self, points: Vec<P2>, color: Rgb, layer: Layer) {
        self.items.push(Item::Fill { points, color, layer });
    }

    /// The count of each kind of stroke: (lines, arcs, circles, others).
    pub fn counts(&self) -> (usize, usize, usize, usize) {
        let mut c = (0, 0, 0, 0);
        for it in &self.items {
            match it {
                Item::Stroke(Shape::Line { .. }, _) => c.0 += 1,
                Item::Stroke(Shape::Arc { .. }, _) => c.1 += 1,
                Item::Stroke(Shape::Circle { .. }, _) => c.2 += 1,
                Item::Stroke(..) => c.3 += 1,
                _ => {}
            }
        }
        c
    }

    /// Every text's string, for tests.
    pub fn strings(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|i| match i {
                Item::Text(t) => Some(t.text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// A shaded view's triangle (view 2D coordinates, linear RGB), back to front.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShadedTri {
    pub points: [P2; 3],
    pub colors: [[f32; 3]; 3],
}

/// What a view needs to be drawn: its projection with the model data annotations read, its
/// shaded triangles and the sketches it shows (sheet mm polylines).
pub struct ViewInput<'a> {
    pub model: &'a dyn ViewModel,
    pub shaded: Vec<ShadedTri>,
    pub sketches: Vec<Vec<P2>>,
}

/// Everything a sheet's page reads besides the drawing.
pub struct PageContext<'a> {
    /// The sheet's referenced part or assembly (the title block, fields).
    pub reference: &'a ReferenceProps,
    /// What fields read.
    pub fields: &'a DrawingContext,
    pub views: HashMap<ViewId, ViewInput<'a>>,
}

fn linear_to_srgb(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}

/// The page of sheet `index` of `d`.
pub fn sheet_page(d: &Drawing, index: usize, ctx: &PageContext) -> Page {
    let Some(sheet) = d.sheets.get(index) else {
        return Page::default();
    };
    let (w, h) = sheet.size_mm();
    let mut page = Page { name: sheet.name.clone(), width: w, height: h, items: Vec::new() };
    // Border and title block.
    let g = crate::sheet_graphics(d, index, ctx.reference);
    for l in &g.lines {
        page.stroke(Shape::Line { a: l.a, b: l.b }, Pen::new(l.weight.mm(), Layer::Border));
    }
    for c in &g.circles {
        page.stroke(Shape::Circle { center: c.center, radius: c.radius }, Pen::new(c.weight.mm(), Layer::Border));
    }
    for t in &g.texts {
        let style = CharStyle { bold: t.bold, ..CharStyle::default() };
        let width = rich::width(&t.text, &style) * t.height;
        let em = t.height / CAP_HEIGHT;
        let (x, y) = match t.align {
            Align::Center => (t.pos[0] - width / 2.0, t.pos[1] - t.height / 2.0),
            Align::CenterLeft => (t.pos[0], t.pos[1] - t.height / 2.0),
            Align::TopLeft => (t.pos[0], t.pos[1] - ASCENT * em),
            Align::BottomRight => (t.pos[0] - width, t.pos[1] + DESCENT * em),
        };
        page.items.push(Item::Text(Text {
            pos: [x, y],
            height: t.height,
            text: t.text.clone(),
            bold: t.bold,
            italic: false,
            rotation: 0.0,
            color: INK,
            layer: Layer::Border,
        }));
    }
    let views: Vec<&View> = sheet.views.iter().collect();
    // Shaded views first (under every line).
    for v in &views {
        let Some(input) = ctx.views.get(&v.id) else { continue };
        if !v.shaded {
            continue;
        }
        for t in &input.shaded {
            let c = t.colors.iter().fold([0.0f32; 3], |a, c| [a[0] + c[0] / 3.0, a[1] + c[1] / 3.0, a[2] + c[2] / 3.0]);
            let color = [linear_to_srgb(c[0]), linear_to_srgb(c[1]), linear_to_srgb(c[2])];
            page.fill(t.points.iter().map(|p| v.to_sheet(*p)).collect(), color, Layer::Shaded);
        }
    }
    // Images.
    for it in &sheet.sketch {
        if let ItemKind::Image(img) = &it.kind {
            page.items.push(Item::Image(Picture {
                at: img.at,
                width: img.width,
                height: img.height,
                data: std::sync::Arc::new(img.data.clone()),
            }));
        }
    }
    // View lines.
    for v in &views {
        let Some(input) = ctx.views.get(&v.id) else { continue };
        let hlr = input.model.projection();
        for l in view_lines(v, hlr) {
            if v.shaded && l.kind == LineKind::Hidden {
                continue;
            }
            let layer = match l.kind {
                LineKind::Visible => Layer::Visible,
                LineKind::Hidden => Layer::Hidden,
                LineKind::Tangent => Layer::Tangent,
                LineKind::Phantom => Layer::Phantom,
                LineKind::Sketch => Layer::Sketch,
            };
            let pen = Pen { width: l.kind.weight().mm(), color: INK, dash: l.kind.pattern().map(|p| p.to_vec()), layer };
            let straight = l.points.len() == 2;
            // A clipped or broken view's curves are drawn as the pieces shown (P3C.8).
            let arc = if straight || v.clipped() { None } else { hlr.edges.get(l.edge).and_then(|e| sheet_arc(v, &e.curve)) };
            match arc {
                Some(s) => page.stroke(s, pen),
                None => page.polyline(l.points, pen),
            }
        }
        for s in &input.sketches {
            page.polyline(s.clone(), Pen { width: Weight::Thin.mm(), color: INK, dash: None, layer: Layer::Sketch });
        }
        // A flat pattern's bend lines, up and down each with its own pen (P3I.7).
        if let Some(flat) = input.model.flat() {
            for l in crate::flat_view::bend_lines(v, flat) {
                let layer = if l.up { Layer::BendUp } else { Layer::BendDown };
                let pen = Pen { width: l.style.weight, color: l.style.color, dash: Some(crate::flat_view::BEND_PATTERN.to_vec()), layer };
                page.polyline(l.points, pen);
            }
            // Its centermarks (SM16.3).
            for l in crate::flat_view::centermarks(&d.style, v, flat) {
                page.polyline(l, Pen::new(Weight::Thin.mm(), Layer::Annotation));
            }
        }
        // Hatching, threads, breaks, cutting lines and labels (P3C.8).
        let dec = crate::view_kinds::view_decor(&d.style, &sheet.views, v, Some(input.model), &crate::view_kinds::label_avoid(sheet));
        for l in &dec.thin {
            page.polyline(l.clone(), Pen::new(Weight::Thin.mm(), Layer::Hatch));
        }
        for l in &dec.medium {
            page.polyline(l.clone(), Pen::new(Weight::Medium.mm(), Layer::Hatch));
        }
        for t in &dec.fills {
            page.fill(t.to_vec(), INK, Layer::Hatch);
        }
        for t in &dec.texts {
            page.items.push(Item::Text(Text {
                pos: [t.pos[0], t.pos[1] - t.height / 2.0],
                height: t.height,
                text: t.text.clone(),
                bold: false,
                italic: false,
                rotation: 0.0,
                color: INK,
                layer: Layer::Hatch,
            }));
        }
    }
    // Annotations.
    let ann_pen = |color: Rgb| Pen { width: ANNOTATION_WIDTH, color, dash: None, layer: Layer::Annotation };
    for v in &views {
        let Some(input) = ctx.views.get(&v.id) else { continue };
        let model = crate::assembly::SheetModel::new(d, sheet, v, input.model);
        for a in &v.annotations {
            let Some(gr) = annotation_graphics(&d.style, v, &model, a) else { continue };
            let color = if gr.dangling { DANGLING } else { INK };
            for s in &gr.strokes {
                page.polyline(s.clone(), ann_pen(color));
            }
            for t in &gr.fills {
                page.fill(t.to_vec(), color, Layer::Annotation);
            }
            for (pos, height, ch) in &gr.symbols {
                page.items.push(Item::Symbol { pos: [pos[0], pos[1] - height / 2.0], height: *height, ch: *ch });
            }
            for t in &gr.texts {
                page.items.push(Item::Text(Text {
                    pos: [t.pos[0], t.pos[1] - t.height / 2.0],
                    height: t.height,
                    text: t.text.clone(),
                    bold: false,
                    italic: false,
                    rotation: 0.0,
                    color,
                    layer: Layer::Annotation,
                }));
            }
        }
    }
    // A flat pattern's bend notes (P3I.7).
    for v in &views {
        let Some(flat) = ctx.views.get(&v.id).and_then(|i| i.model.flat()) else { continue };
        for n in crate::flat_view::bend_notes(&d.style, v, flat) {
            for s in &n.strokes {
                page.polyline(s.clone(), ann_pen(INK));
            }
            for t in &n.fills {
                page.fill(t.to_vec(), INK, Layer::Annotation);
            }
            page.items.push(Item::Text(rotated_text(n.text.pos, n.text.height, &n.text.text, false, false, n.rotation, Layer::Annotation)));
        }
    }
    // Notes and tables.
    let fctx = FieldContext { reference: ctx.reference, drawing: ctx.fields };
    let lookup = |id: ViewId| -> Option<(&View, &dyn ViewModel)> {
        let v = sheet.views.iter().find(|v| v.id == id)?;
        ctx.views.get(&id).map(|i| (v, i.model))
    };
    for n in &sheet.notes {
        let g = note_graphics(n, &fctx, &lookup, d.style.dim_arrow_length, false);
        for (i, s) in g.strokes.iter().enumerate() {
            let color = if g.dangling_strokes.contains(&i) { DANGLING } else { INK };
            page.polyline(s.clone(), Pen { layer: Layer::Note, ..ann_pen(color) });
        }
        for (i, t) in g.fills.iter().enumerate() {
            let color = if g.dangling_fills.contains(&i) { DANGLING } else { INK };
            page.fill(t.to_vec(), color, Layer::Note);
        }
        for t in &g.texts {
            page.items.push(Item::Text(rotated_text(t.pos, t.piece.height, &t.piece.text, t.piece.bold, t.piece.italic, t.rotation, Layer::Note)));
        }
    }
    for t in &sheet.tables {
        let g = table_graphics(t, &fctx, false, None);
        for (a, b) in &g.thin {
            page.stroke(Shape::Line { a: *a, b: *b }, Pen::new(Weight::Thin.mm(), Layer::Table));
        }
        for (a, b) in &g.outline {
            page.stroke(Shape::Line { a: *a, b: *b }, Pen::new(Weight::Medium.mm(), Layer::Table));
        }
        for s in &g.strokes {
            page.polyline(s.clone(), Pen::new(ANNOTATION_WIDTH, Layer::Table));
        }
        for x in &g.texts {
            page.items.push(Item::Text(rotated_text(x.pos, x.piece.height, &x.piece.text, x.piece.bold, x.piece.italic, x.rotation, Layer::Table)));
        }
    }
    // The sheet sketch.
    for it in &sheet.sketch {
        match &it.kind {
            ItemKind::Line { a, b } => page.stroke(Shape::Line { a: *a, b: *b }, Pen::new(Weight::Medium.mm(), Layer::SheetSketch)),
            ItemKind::Spline { points } => {
                let (knots, control) = sk::spline_nurbs(points);
                page.stroke(
                    Shape::Spline { knots, control, fit: points.clone(), points: sk::spline_polyline(points, 24) },
                    Pen::new(Weight::Medium.mm(), Layer::SheetSketch),
                );
            }
            ItemKind::Block(b) => block_items(&mut page, b),
            ItemKind::Image(_) => {}
        }
    }
    page
}

/// Paper width of annotation lines (mm).
pub const ANNOTATION_WIDTH: f64 = 0.25;
/// Inter's cap height, ascender and descender as fractions of the font size.
pub const CAP_HEIGHT: f64 = 0.727;
pub const ASCENT: f64 = 0.969;
pub const DESCENT: f64 = 0.242;

/// A text whose anchor is the left end of its capitals' middle, turned `rotation` degrees.
fn rotated_text(mid: P2, height: f64, text: &str, bold: bool, italic: bool, rotation: f64, layer: Layer) -> Text {
    let (s, c) = rotation.to_radians().sin_cos();
    let d = -height / 2.0;
    Text { pos: [mid[0] - s * d, mid[1] + c * d], height, text: text.into(), bold, italic, rotation, color: INK, layer }
}

/// A projected circle or arc on the sheet, if the curve is one.
fn sheet_arc(v: &View, curve: &ProjCurve) -> Option<Shape> {
    let ProjCurve::Arc { center, radius, start, mid, end, full } = curve else {
        return None;
    };
    let c = v.to_sheet([center.x, center.y]);
    let r = radius * v.scale.factor();
    if *full {
        return Some(Shape::Circle { center: c, radius: r });
    }
    let ang = |p: &nalgebra::Point2<f64>| {
        let q = v.to_sheet([p.x, p.y]);
        (q[1] - c[1]).atan2(q[0] - c[0]).to_degrees()
    };
    let (a0, am, a1) = (ang(start), ang(mid), ang(end));
    // Counter-clockwise from a0 to a1 when the middle lies on that sweep, else the other way.
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(360.0);
    let (s, e) = if ccw(a0, am) <= ccw(a0, a1) { (a0, a1) } else { (a1, a0) };
    Some(Shape::Arc { center: c, radius: r, start: s.rem_euclid(360.0), end: e.rem_euclid(360.0) })
}

/// The entities of an inserted DXF, on the sheet.
fn block_items(page: &mut Page, b: &sk::Block) {
    let m = |p: P2| sk::block_to_sheet(b, p);
    let pen = || Pen::new(Weight::Thin.mm(), Layer::Import);
    for e in &b.entities {
        match e {
            Entity::Line { a, b: e } => page.stroke(Shape::Line { a: m(*a), b: m(*e) }, pen()),
            Entity::Arc { center, radius, start, end } => {
                page.stroke(Shape::Arc { center: m(*center), radius: radius * b.scale, start: *start, end: *end }, pen())
            }
            Entity::Circle { center, radius } => page.stroke(Shape::Circle { center: m(*center), radius: radius * b.scale }, pen()),
            Entity::Polyline { points, bulges, closed } => {
                let pl: Vec<P2> = sk::bulge_polyline(points, bulges, *closed).into_iter().map(m).collect();
                page.polyline(pl, pen());
            }
            Entity::Spline { degree, knots, control, weights, fit } => {
                let pts: Vec<P2> = sk::bspline_polyline(*degree, knots, control, weights, 16).into_iter().map(m).collect();
                if *degree == 3 && weights.is_empty() {
                    page.stroke(
                        Shape::Spline { knots: knots.clone(), control: control.iter().map(|p| m(*p)).collect(), fit: fit.iter().map(|p| m(*p)).collect(), points: pts },
                        pen(),
                    );
                } else {
                    page.polyline(pts, pen());
                }
            }
            Entity::Text { at, height, text, rotation } => page.items.push(Item::Text(Text {
                pos: m(*at),
                height: height * b.scale,
                text: text.clone(),
                bold: false,
                italic: false,
                rotation: *rotation,
                color: INK,
                layer: Layer::Import,
            })),
            Entity::Solid { points } => page.fill(points.iter().map(|p| m(*p)).collect(), INK, Layer::Import),
        }
    }
}

/// Black and white: every colour to black, fills of shaded views and images to grey.
pub fn to_black_and_white(page: &Page) -> Page {
    let mut p = page.clone();
    let grey = |c: Rgb| {
        let l = (0.299 * c[0] as f64 + 0.587 * c[1] as f64 + 0.114 * c[2] as f64).round() as u8;
        [l, l, l]
    };
    for it in &mut p.items {
        match it {
            Item::Stroke(_, pen) => pen.color = [0, 0, 0],
            Item::Fill { color, layer, .. } => *color = if *layer == Layer::Shaded { grey(*color) } else { [0, 0, 0] },
            Item::Text(t) => t.color = [0, 0, 0],
            Item::Image(_) | Item::Symbol { .. } => {}
        }
    }
    p
}

/// Decodes an image file (PNG or JPEG) to 8-bit RGBA.
pub fn decode_image(data: &[u8]) -> Result<image::RgbaImage, String> {
    image::load_from_memory(data).map(|i| i.to_rgba8()).map_err(|e| e.to_string())
}

/// Encoded-image sizes: (width, height) in pixels, if it decodes.
pub fn image_size(data: &[u8]) -> Option<(u32, u32)> {
    let r = image::ImageReader::new(std::io::Cursor::new(data)).with_guessed_format().ok()?;
    r.into_dimensions().ok()
}

// ---------------------------------------------------------------------------------------------
// Files

/// An export format (D2.10 Export…).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Format {
    Pdf,
    Dxf,
    Dwg,
    /// An AutoCAD template: the sheet written as DWG through the converter, named `.dwt` (a
    /// DWT is a DWG file whose extension marks it as a template).
    Dwt,
    Png,
    Jpeg,
}

impl Format {
    pub const ALL: [Format; 6] = [Format::Pdf, Format::Dxf, Format::Dwg, Format::Dwt, Format::Png, Format::Jpeg];

    /// Written through the external DWG converter.
    pub fn needs_converter(self) -> bool {
        matches!(self, Format::Dwg | Format::Dwt)
    }

    pub fn label(self) -> &'static str {
        match self {
            Format::Pdf => "PDF",
            Format::Dxf => "DXF",
            Format::Dwg => "DWG",
            Format::Dwt => "DWT",
            Format::Png => "PNG",
            Format::Jpeg => "JPEG",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Pdf => "pdf",
            Format::Dxf => "dxf",
            Format::Dwg => "dwg",
            Format::Dwt => "dwt",
            Format::Png => "png",
            Format::Jpeg => "jpg",
        }
    }
}

/// How to export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    pub format: Format,
    /// PDF and images: colour, or black and white.
    pub color: bool,
    /// PNG and JPEG: dots per inch.
    pub dpi: f64,
    /// DXF and DWG: the AutoCAD version.
    pub dxf_version: crate::dxf::DxfVersion,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self { format: Format::Pdf, color: true, dpi: 300.0, dxf_version: crate::dxf::DxfVersion::R2013 }
    }
}

/// A name for a file: characters files can't have become `_`.
pub fn file_safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if "/\\:*?\"<>|".contains(c) || c.is_control() { '_' } else { c }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "Drawing".into() } else { s }
}

/// Writes `pages` into `dir` as `base.<ext>`: PDF in one file (a page per sheet); DXF, DWG and
/// images one file per sheet (`base - <sheet>.<ext>` when there are several). The paths
/// written.
pub fn write_files(pages: &[Page], opts: &ExportOptions, dir: &std::path::Path, base: &str) -> Result<Vec<std::path::PathBuf>, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let base = file_safe(base);
    let ext = opts.format.extension();
    let path_for = |page: &Page| {
        if pages.len() > 1 {
            dir.join(format!("{base} - {}.{ext}", file_safe(&page.name)))
        } else {
            dir.join(format!("{base}.{ext}"))
        }
    };
    let write = |p: &std::path::Path, data: &[u8]| std::fs::write(p, data).map_err(|e| format!("{}: {e}", p.display()));
    let mut out = Vec::new();
    match opts.format {
        Format::Pdf => {
            let p = dir.join(format!("{base}.pdf"));
            write(&p, &crate::pdf::write_pdf(pages, &crate::pdf::PdfOptions { color: opts.color }))?;
            out.push(p);
        }
        Format::Dxf => {
            for page in pages {
                let p = path_for(page);
                write(&p, crate::dxf::write_dxf_version(page, opts.dxf_version).as_bytes())?;
                out.push(p);
            }
        }
        Format::Dwg | Format::Dwt => {
            let conv = crate::dwg::find_converter().filter(|c| c.can_write()).ok_or(crate::dwg::INSTALL_HINT)?;
            for page in pages {
                let p = path_for(page);
                crate::dwg::dxf_to_dwg(&conv, &crate::dxf::write_dxf_version(page, opts.dxf_version), &p, opts.dxf_version)?;
                out.push(p);
            }
        }
        Format::Png | Format::Jpeg => {
            for page in pages {
                let pm = crate::raster::render(page, opts.dpi, opts.color)?;
                let data = if opts.format == Format::Png { crate::raster::png(&pm)? } else { crate::raster::jpeg(&pm)? };
                let p = path_for(page);
                write(&p, &data)?;
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// An imported DXF (or DWG) file as a block: its entities in mm with the bottom-left of their
/// extent at the block's origin, placed at `at`.
pub fn block_from_file(path: &std::path::Path, at: P2) -> Result<sk::Block, String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("drawing").to_string();
    let is_dwg = path.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("dwg"));
    let text = if is_dwg {
        let conv = crate::dwg::find_converter().filter(|c| c.can_read()).ok_or(crate::dwg::INSTALL_HINT)?;
        crate::dwg::dwg_to_dxf(&conv, path)?
    } else {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        String::from_utf8_lossy(&bytes).into_owned()
    };
    let d = crate::dxf::read_dxf(&text)?;
    block_from_dxf(&name, &d, at)
}

/// A read DXF as a block (see [`block_from_file`]).
pub fn block_from_dxf(name: &str, d: &crate::dxf::DxfDrawing, at: P2) -> Result<sk::Block, String> {
    if d.entities.is_empty() {
        return Err(format!("{name} has nothing to insert"));
    }
    let k = d.unit_mm;
    let scaled: Vec<Entity> = d.entities.iter().map(|e| scale_entity(e, k)).collect();
    let (lo, _) = sk::entities_bounds(&scaled).unwrap_or(([0.0, 0.0], [0.0, 0.0]));
    let entities = scaled.iter().map(|e| translate_entity(e, [-lo[0], -lo[1]])).collect();
    Ok(sk::Block { name: name.to_string(), at, scale: 1.0, entities })
}

fn map_entity(e: &Entity, f: &dyn Fn(P2) -> P2, k: f64) -> Entity {
    match e {
        Entity::Line { a, b } => Entity::Line { a: f(*a), b: f(*b) },
        Entity::Arc { center, radius, start, end } => Entity::Arc { center: f(*center), radius: radius * k, start: *start, end: *end },
        Entity::Circle { center, radius } => Entity::Circle { center: f(*center), radius: radius * k },
        Entity::Polyline { points, bulges, closed } => Entity::Polyline { points: points.iter().map(|p| f(*p)).collect(), bulges: bulges.clone(), closed: *closed },
        Entity::Spline { degree, knots, control, weights, fit } => Entity::Spline {
            degree: *degree,
            knots: knots.clone(),
            control: control.iter().map(|p| f(*p)).collect(),
            weights: weights.clone(),
            fit: fit.iter().map(|p| f(*p)).collect(),
        },
        Entity::Text { at, height, text, rotation } => Entity::Text { at: f(*at), height: height * k, text: text.clone(), rotation: *rotation },
        Entity::Solid { points } => Entity::Solid { points: points.iter().map(|p| f(*p)).collect() },
    }
}

fn scale_entity(e: &Entity, k: f64) -> Entity {
    if k == 1.0 {
        return e.clone();
    }
    map_entity(e, &|p| [p[0] * k, p[1] * k], k)
}

fn translate_entity(e: &Entity, d: P2) -> Entity {
    map_entity(e, &|p| [p[0] + d[0], p[1] + d[1]], 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sheet_sketch::{ItemKind, SketchItem, image_item};
    use crate::{Drawing, DrawingOp, SheetId, template};

    fn drawing() -> Drawing {
        let mut d = Drawing::from_template(&template::builtin("ANSI_A_INCH.dwt").unwrap(), None);
        d.apply(&DrawingOp::InsertSheet { id: SheetId::new(), after: None }).unwrap();
        d.apply(&DrawingOp::RenameSheet { id: d.sheets[1].id, name: "Detail/B".into() }).unwrap();
        d
    }

    fn page(d: &Drawing, i: usize) -> Page {
        let r = ReferenceProps::default();
        let f = DrawingContext::default();
        sheet_page(d, i, &PageContext { reference: &r, fields: &f, views: HashMap::new() })
    }

    #[test]
    fn options_default_and_names() {
        let o = ExportOptions::default();
        assert_eq!((o.format, o.color, o.dpi, o.dxf_version), (Format::Pdf, true, 300.0, crate::dxf::DxfVersion::R2013));
        assert_eq!(file_safe("A/B: c?"), "A_B_ c_");
        assert_eq!(file_safe("  .. "), "Drawing");
        assert_eq!(Format::ALL.map(|f| f.extension()), ["pdf", "dxf", "dwg", "dwt", "png", "jpg"]);
    }

    #[test]
    fn sheets_become_pages_and_files() {
        let d = drawing();
        let pages = [page(&d, 0), page(&d, 1)];
        assert_eq!((pages[0].name.as_str(), pages[1].name.as_str()), ("Sheet1", "Detail/B"));
        assert!((pages[0].width - 279.4).abs() < 1e-9 && (pages[0].height - 215.9).abs() < 1e-9);
        let dir = std::env::temp_dir().join(format!("cadrs-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        // Every sheet: a PDF holds them all; DXF writes a file per sheet, named for it.
        let pdf = write_files(&pages, &ExportOptions::default(), &dir, "Part: 1").unwrap();
        assert_eq!(pdf, vec![dir.join("Part_ 1.pdf")]);
        let dxf = write_files(&pages, &ExportOptions { format: Format::Dxf, ..Default::default() }, &dir, "Part").unwrap();
        assert_eq!(dxf, vec![dir.join("Part - Sheet1.dxf"), dir.join("Part - Detail_B.dxf")]);
        // The current sheet only: one file with the plain name.
        let one = write_files(&pages[1..], &ExportOptions { format: Format::Dxf, ..Default::default() }, &dir, "Part").unwrap();
        assert_eq!(one, vec![dir.join("Part.dxf")]);
        let r2000 = std::fs::read_to_string(&one[0]).unwrap();
        assert!(r2000.contains("AC1027"));
        let old = write_files(&pages[1..], &ExportOptions { format: Format::Dxf, dxf_version: crate::dxf::DxfVersion::R2000, ..Default::default() }, &dir, "Old").unwrap();
        assert!(std::fs::read_to_string(&old[0]).unwrap().contains("AC1015"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_inserted_image_hides_the_dash_under_it() {
        let mut d = drawing();
        let dashes = |p: &Page| p.strings().iter().filter(|s| **s == "--").count();
        let before = dashes(&page(&d, 0));
        // The Company cell's "--" (x 160–202, y 13–21 on ANSI A).
        let img = image_item("logo.png", vec![1, 2, 3], (200, 100), [170.0, 13.5], 20.0);
        let sheet = d.sheets[0].id;
        d.apply(&DrawingOp::AddSketchItems { sheet, items: vec![SketchItem::new(ItemKind::Image(img))] }).unwrap();
        assert_eq!(dashes(&page(&d, 0)), before - 1);
    }
}

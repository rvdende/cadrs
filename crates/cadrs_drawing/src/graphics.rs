//! A sheet as plain 2D primitives (lines, circles, text) in sheet millimetres, for the app to
//! draw and, later, for PDF and DXF export (P3C.7).

use crate::standard::{Rect, Standard, frame};
use crate::title_block::{self, ReferenceProps, TitleContext};
use crate::{Drawing, Sheet};

/// Line weights (ISO 128 widths in mm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Weight {
    /// 0.25 mm: zone ticks, title-block cells.
    Thin,
    /// 0.35 mm: the border, title-block outline.
    Medium,
    /// 0.7 mm: the drawing frame.
    Thick,
    /// 0.18 mm chain line: centre lines.
    Center,
}

impl Weight {
    pub fn mm(self) -> f64 {
        match self {
            Weight::Thin => 0.25,
            Weight::Medium => 0.35,
            Weight::Thick => 0.7,
            Weight::Center => 0.18,
        }
    }
}

/// How text sits on its anchor point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    TopLeft,
    #[default]
    Center,
    BottomRight,
    CenterLeft,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GLine {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub weight: Weight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GCircle {
    pub center: [f64; 2],
    pub radius: f64,
    pub weight: Weight,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GText {
    pub pos: [f64; 2],
    /// Cap height in mm.
    pub height: f64,
    pub text: String,
    pub align: Align,
    pub bold: bool,
}

impl GText {
    pub fn new(pos: [f64; 2], height: f64, text: impl Into<String>) -> Self {
        Self {
            pos,
            height,
            text: text.into(),
            align: Align::Center,
            bold: false,
        }
    }

    pub fn align(mut self, a: Align) -> Self {
        self.align = a;
        self
    }

    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }
}

/// A list of primitives.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Graphics {
    pub lines: Vec<GLine>,
    pub circles: Vec<GCircle>,
    pub texts: Vec<GText>,
}

impl Graphics {
    pub fn line(&mut self, a: [f64; 2], b: [f64; 2], weight: Weight) {
        self.lines.push(GLine { a, b, weight });
    }

    pub fn rect(&mut self, r: Rect, weight: Weight) {
        let (x0, y0, x1, y1) = (r.min[0], r.min[1], r.max[0], r.max[1]);
        self.line([x0, y0], [x1, y0], weight);
        self.line([x1, y0], [x1, y1], weight);
        self.line([x1, y1], [x0, y1], weight);
        self.line([x0, y1], [x0, y0], weight);
    }

    pub fn circle(&mut self, center: [f64; 2], radius: f64, weight: Weight) {
        self.circles.push(GCircle {
            center,
            radius,
            weight,
        });
    }

    pub fn text(&mut self, t: GText) {
        self.texts.push(t);
    }

    /// The text of every label, for tests.
    pub fn strings(&self) -> Vec<&str> {
        self.texts.iter().map(|t| t.text.as_str()).collect()
    }
}

/// The border, zones and title block of sheet `index` of `drawing`, with the title block
/// filled from `reference` (the sheet's referenced part or assembly).
pub fn sheet_graphics(drawing: &Drawing, index: usize, reference: &ReferenceProps) -> Graphics {
    let mut g = Graphics::default();
    let Some(sheet) = drawing.sheets.get(index) else {
        return g;
    };
    border(&mut g, sheet);
    if sheet.title_block {
        let f = frame(sheet.format);
        let rect = title_block::placement(f.inner, sheet.format.size);
        let ctx = TitleContext {
            reference,
            props: &drawing.title,
            size: sheet.format.size,
            scale: sheet.scale,
            projection: drawing.projection,
            sheet_index: index,
            sheet_count: drawing.sheets.len(),
        };
        title_block::draw(&mut g, rect, &ctx, drawing.units);
        // An empty field's "--" under an inserted logo or image is not drawn (the item fills
        // the field, P3C.7).
        let covers: Vec<([f64; 2], [f64; 2])> = sheet
            .sketch
            .iter()
            .filter(|i| matches!(i.kind, crate::sheet_sketch::ItemKind::Block(_) | crate::sheet_sketch::ItemKind::Image(_)))
            .filter_map(crate::sheet_sketch::item_bounds)
            .collect();
        g.texts.retain(|t| {
            t.text != "--" || !covers.iter().any(|(lo, hi)| t.pos[0] >= lo[0] && t.pos[0] <= hi[0] && t.pos[1] >= lo[1] - t.height && t.pos[1] <= hi[1] + t.height)
        });
    }
    g
}

fn border(g: &mut Graphics, sheet: &Sheet) {
    if !sheet.border {
        return;
    }
    let f = frame(sheet.format);
    let std = sheet.format.size.standard();
    g.rect(f.outer, Weight::Medium);
    g.rect(f.inner, Weight::Thick);
    if sheet.zones {
        let label_h = match std {
            Standard::Ansi => 3.0,
            Standard::Iso => 3.5,
        };
        // Ticks at the zone boundaries across the band, and labels centred in each band: in
        // the space between the thin border line and the thick frame line (so each label sits
        // midway between the lines' inner edges, not their centres).
        let (o, i) = (f.outer, f.inner);
        let (ho, hi) = (Weight::Medium.mm() / 2.0, Weight::Thick.mm() / 2.0);
        let mid = |outer: f64, inner: f64| {
            let s = (inner - outer).signum();
            ((outer + s * ho) + (inner - s * hi)) / 2.0
        };
        for (k, z) in f.columns.iter().enumerate() {
            if k > 0 {
                g.line([z.from, o.min[1]], [z.from, i.min[1]], Weight::Thin);
                g.line([z.from, i.max[1]], [z.from, o.max[1]], Weight::Thin);
            }
            let cx = (z.from + z.to) / 2.0;
            g.text(GText::new([cx, mid(o.min[1], i.min[1])], label_h, z.label.clone()));
            g.text(GText::new([cx, mid(o.max[1], i.max[1])], label_h, z.label.clone()));
        }
        for (k, z) in f.rows.iter().enumerate() {
            if k > 0 {
                g.line([o.min[0], z.from], [i.min[0], z.from], Weight::Thin);
                g.line([i.max[0], z.from], [o.max[0], z.from], Weight::Thin);
            }
            let cy = (z.from + z.to) / 2.0;
            g.text(GText::new([mid(o.min[0], i.min[0]), cy], label_h, z.label.clone()));
            g.text(GText::new([mid(o.max[0], i.max[0]), cy], label_h, z.label.clone()));
        }
    }
    if std == Standard::Iso {
        // ISO 5457 centring marks: from the trimmed edge to 5 mm inside the frame at the
        // middle of each side.
        let (w, h) = sheet.format.size_mm();
        let i = f.inner;
        let (mx, my) = (w / 2.0, h / 2.0);
        g.line([mx, 0.0], [mx, i.min[1] + 5.0], Weight::Thick);
        g.line([mx, h], [mx, i.max[1] - 5.0], Weight::Thick);
        g.line([0.0, my], [i.min[0] + 5.0, my], Weight::Thick);
        g.line([w, my], [i.max[0] - 5.0, my], Weight::Thick);
    }
}

//! Raster export (P3C.7, D2.10 Export… → PNG or JPEG): a page drawn with tiny-skia at a chosen
//! resolution, on white. Text is drawn from Inter's outlines, so it matches the PDF.

use tiny_skia::{Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, StrokeDash, Transform};

use crate::export::{CAP_HEIGHT, Item, Page, Rgb, Shape};

/// A page drawn at `dpi` dots per inch (colour, or black and white).
pub fn render(page: &Page, dpi: f64, color: bool) -> Result<Pixmap, String> {
    let page = if color { page.clone() } else { crate::export::to_black_and_white(page) };
    let s = dpi / 25.4;
    let (w, h) = ((page.width * s).round().max(1.0) as u32, (page.height * s).round().max(1.0) as u32);
    if (w as u64) * (h as u64) > 400_000_000 {
        return Err(format!("{w} × {h} pixels is too large"));
    }
    let mut pm = Pixmap::new(w, h).ok_or("cannot make the image")?;
    pm.fill(Color::WHITE);
    let hh = h as f32;
    let to = Transform::from_row(s as f32, 0.0, 0.0, -(s as f32), 0.0, hh);
    let paint_of = |c: Rgb| {
        let mut p = Paint::default();
        p.set_color_rgba8(c[0], c[1], c[2], 255);
        p.anti_alias = true;
        p
    };
    for it in &page.items {
        match it {
            Item::Stroke(shape, pen) => {
                let pts = shape.polyline();
                let mut pb = PathBuilder::new();
                let Some(f) = pts.first() else { continue };
                pb.move_to(f[0] as f32, f[1] as f32);
                for p in &pts[1..] {
                    pb.line_to(p[0] as f32, p[1] as f32);
                }
                if matches!(shape, Shape::Circle { .. }) {
                    pb.close();
                }
                let Some(path) = pb.finish() else { continue };
                let mut st = Stroke { width: pen.width.max(1.0 / s) as f32, line_cap: LineCap::Round, line_join: LineJoin::Round, ..Stroke::default() };
                if let Some(d) = &pen.dash {
                    st.dash = StrokeDash::new(d.iter().map(|x| *x as f32).collect(), 0.0);
                    st.line_cap = LineCap::Butt;
                }
                pm.stroke_path(&path, &paint_of(pen.color), &st, to, None);
            }
            Item::Fill { points, color, layer } => {
                let mut pb = PathBuilder::new();
                let Some(f) = points.first() else { continue };
                pb.move_to(f[0] as f32, f[1] as f32);
                for p in &points[1..] {
                    pb.line_to(p[0] as f32, p[1] as f32);
                }
                pb.close();
                let Some(path) = pb.finish() else { continue };
                let paint = paint_of(*color);
                pm.fill_path(&path, &paint, FillRule::Winding, to, None);
                if *layer == crate::export::Layer::Shaded {
                    // Hides the seams between triangles.
                    let st = Stroke { width: (0.6 / s) as f32, ..Stroke::default() };
                    pm.stroke_path(&path, &paint, &st, to, None);
                }
            }
            Item::Text(t) => {
                if let Some(path) = text_path(t) {
                    pm.fill_path(&path, &paint_of(t.color), FillRule::Winding, to, None);
                }
            }
            Item::Symbol { .. } => {}
            Item::Image(pic) => {
                let Ok(img) = crate::export::decode_image(&pic.data) else { continue };
                let (iw, ih) = img.dimensions();
                let mut data = img.into_raw();
                for px in data.chunks_mut(4) {
                    let a = px[3] as u16;
                    for c in &mut px[..3] {
                        *c = ((*c as u16 * a + 127) / 255) as u8;
                    }
                }
                let Some(size) = tiny_skia::IntSize::from_wh(iw, ih) else { continue };
                let Some(src) = Pixmap::from_vec(data, size) else { continue };
                let t = Transform::from_row(
                    (s * pic.width / iw as f64) as f32,
                    0.0,
                    0.0,
                    (s * pic.height / ih as f64) as f32,
                    (s * pic.at[0]) as f32,
                    hh - (s * (pic.at[1] + pic.height)) as f32,
                );
                let paint = PixmapPaint { quality: tiny_skia::FilterQuality::Bicubic, ..PixmapPaint::default() };
                pm.draw_pixmap(0, 0, src.as_ref(), &paint, t, None);
            }
        }
    }
    Ok(pm)
}

struct Outline {
    pb: PathBuilder,
    m: [f64; 6],
}

impl Outline {
    fn p(&self, x: f32, y: f32) -> (f32, f32) {
        let (x, y) = (x as f64, y as f64);
        let m = &self.m;
        ((m[0] * x + m[2] * y + m[4]) as f32, (m[1] * x + m[3] * y + m[5]) as f32)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        self.pb.move_to(a, b);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let (a, b) = self.p(x, y);
        self.pb.line_to(a, b);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        let (a, b) = self.p(x1, y1);
        let (c, d) = self.p(x, y);
        self.pb.quad_to(a, b, c, d);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let (a, b) = self.p(x1, y1);
        let (c, d) = self.p(x2, y2);
        let (e, f) = self.p(x, y);
        self.pb.cubic_to(a, b, c, d, e, f);
    }
    fn close(&mut self) {
        self.pb.close();
    }
}

/// A text's glyph outlines on the sheet (mm).
fn text_path(t: &crate::export::Text) -> Option<tiny_skia::Path> {
    let style = crate::rich::CharStyle { bold: t.bold, italic: t.italic, ..crate::rich::CharStyle::default() };
    let (w, it) = crate::rich::face(&style);
    let face = ttf_parser::Face::parse(cadrs_sketch::text::inter_data(w, it && !t.bold), 0).ok()?;
    let em = t.height / CAP_HEIGHT;
    let k = em / face.units_per_em() as f64;
    let (s, c) = t.rotation.to_radians().sin_cos();
    let mut o = Outline { pb: PathBuilder::new(), m: [0.0; 6] };
    let mut x = 0.0;
    for ch in t.text.chars() {
        let Some(g) = face.glyph_index(ch) else { continue };
        // Glyph units → sheet: scale, then the text's rotation about its baseline start.
        let (gx, gy) = (x, 0.0);
        o.m = [k * c, k * s, -k * s, k * c, t.pos[0] + c * gx - s * gy, t.pos[1] + s * gx + c * gy];
        face.outline_glyph(g, &mut o);
        x += face.glyph_hor_advance(g).unwrap_or(0) as f64 * k;
    }
    o.pb.finish()
}

/// PNG bytes of a rendered page.
pub fn png(pm: &Pixmap) -> Result<Vec<u8>, String> {
    pm.encode_png().map_err(|e| e.to_string())
}

/// JPEG bytes of a rendered page (quality 92).
pub fn jpeg(pm: &Pixmap) -> Result<Vec<u8>, String> {
    let rgb: Vec<u8> = pm.data().chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 92);
    image::ImageEncoder::write_image(enc, &rgb, pm.width(), pm.height(), image::ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Layer, Pen, Text};

    #[test]
    fn renders_lines_and_text() {
        let mut page = Page { name: "S".into(), width: 50.0, height: 25.0, items: Vec::new() };
        page.items.push(Item::Stroke(Shape::Line { a: [0.0, 5.0], b: [50.0, 5.0] }, Pen::new(0.5, Layer::Visible)));
        page.items.push(Item::Text(Text { pos: [5.0, 10.0], height: 5.0, text: "Ø1".into(), bold: false, italic: false, rotation: 0.0, color: [0, 0, 0], layer: Layer::Note }));
        let pm = render(&page, 100.0, true).unwrap();
        assert_eq!((pm.width(), pm.height()), (197, 98));
        // The line is dark at y = 5 mm from the bottom.
        let y = (98.0 - 5.0 * 100.0 / 25.4) as u32;
        assert!(pm.pixel(100, y).unwrap().red() < 80);
        // Some text ink above it.
        let dark = (0..pm.width()).flat_map(|x| (0..y - 3).map(move |y| (x, y))).filter(|(x, y)| pm.pixel(*x, *y).unwrap().red() < 128).count();
        assert!(dark > 20, "{dark}");
        assert!(jpeg(&pm).unwrap().starts_with(&[0xff, 0xd8]));
        assert!(png(&pm).unwrap().starts_with(b"\x89PNG"));
    }
}

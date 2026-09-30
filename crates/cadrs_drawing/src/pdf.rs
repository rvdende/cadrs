//! A vector PDF writer for drawing pages (P3C.7, D2.10 Export…): one page per sheet at its true
//! paper size, lines, arcs, dashes and fills as PDF paths, text as real text in the embedded
//! Inter faces (TrueType, Identity-H, with a ToUnicode map so text can be searched and copied:
//! `Ø`, `±` and `°` come out as themselves), images as image XObjects.
//!
//! The content of each page is drawn in sheet millimetres (a `cm` of 72/25.4 at the start).
//! Streams are Flate-compressed. The fonts are embedded whole (Inter is OFL-licensed; see
//! `assets/fonts/OFL.txt`).

use std::collections::{BTreeMap, HashMap};
use std::fmt::Write as _;
use std::io::Write as _;

use crate::export::{Item, Page, Pen, Rgb, Shape};
use crate::rich::{self, CharStyle};

/// Points per millimetre.
pub const PT_PER_MM: f64 = 72.0 / 25.4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfOptions {
    /// Colour, or black and white (every line and text black, shaded views grey).
    pub color: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self { color: true }
    }
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = e.write_all(data);
    e.finish().unwrap_or_default()
}

/// A number for a content stream (4 decimals, trailing zeros dropped).
fn n(v: f64) -> String {
    if !v.is_finite() {
        return "0".into();
    }
    let s = format!("{v:.4}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" || s.is_empty() { "0".into() } else { s.to_string() }
}

fn rgb(c: Rgb) -> String {
    format!("{} {} {}", n(c[0] as f64 / 255.0), n(c[1] as f64 / 255.0), n(c[2] as f64 / 255.0))
}

/// A face used on the pages: (weight, italic) as `rich::face` gives them.
type FaceKey = (u16, bool);

struct FontUse {
    data: &'static [u8],
    glyphs: BTreeMap<u16, char>,
}

fn face_key(bold: bool, italic: bool) -> FaceKey {
    let style = CharStyle { bold, italic, ..CharStyle::default() };
    let (w, it) = rich::face(&style);
    (w, it && !bold)
}

struct Writer {
    out: Vec<u8>,
    offsets: Vec<usize>,
}

impl Writer {
    fn new() -> Self {
        let mut out = Vec::new();
        out.extend_from_slice(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n");
        Self { out, offsets: Vec::new() }
    }

    /// Reserves an object number.
    fn reserve(&mut self) -> usize {
        self.offsets.push(0);
        self.offsets.len()
    }

    fn object(&mut self, id: usize, body: &str) {
        self.offsets[id - 1] = self.out.len();
        let _ = write!(self.out, "{id} 0 obj\n{body}\nendobj\n");
    }

    fn stream(&mut self, id: usize, dict: &str, data: &[u8]) {
        self.offsets[id - 1] = self.out.len();
        let _ = write!(self.out, "{id} 0 obj\n<< {dict} /Length {} >>\nstream\n", data.len());
        self.out.extend_from_slice(data);
        self.out.extend_from_slice(b"\nendstream\nendobj\n");
    }

    fn finish(mut self, root: usize) -> Vec<u8> {
        let xref = self.out.len();
        let _ = write!(self.out, "xref\n0 {}\n0000000000 65535 f \n", self.offsets.len() + 1);
        for o in &self.offsets {
            let _ = writeln!(self.out, "{o:010} 00000 n ");
        }
        let _ = write!(
            self.out,
            "trailer\n<< /Size {} /Root {root} 0 R /Info << /Producer (cadrs) /Creator (cadrs drawing export) >> >>\nstartxref\n{xref}\n%%EOF\n",
            self.offsets.len() + 1
        );
        self.out
    }
}

/// Appends the path of `s` to a content stream.
fn path(c: &mut String, s: &Shape) {
    let mv = |c: &mut String, p: [f64; 2]| {
        let _ = writeln!(c, "{} {} m", n(p[0]), n(p[1]));
    };
    let ln = |c: &mut String, p: [f64; 2]| {
        let _ = writeln!(c, "{} {} l", n(p[0]), n(p[1]));
    };
    match s {
        Shape::Line { a, b } => {
            mv(c, *a);
            ln(c, *b);
        }
        Shape::Polyline { points, closed } => {
            if let Some(f) = points.first() {
                mv(c, *f);
                for p in &points[1..] {
                    ln(c, *p);
                }
                if *closed {
                    c.push_str("h\n");
                }
            }
        }
        Shape::Arc { center, radius, start, end } => {
            let mut sweep = (end - start).rem_euclid(360.0);
            if sweep < 1e-9 {
                sweep = 360.0;
            }
            arc_path(c, *center, *radius, *start, sweep, true);
        }
        Shape::Circle { center, radius } => {
            arc_path(c, *center, *radius, 0.0, 360.0, true);
            c.push_str("h\n");
        }
        Shape::Spline { knots, control, points, .. } => {
            // Our splines are joined cubic Béziers (interior knots of multiplicity 3): drawn as
            // curves; any other B-spline as its polyline.
            let segs = control.len().saturating_sub(1) / 3;
            let bezier = control.len() >= 4
                && control.len() % 3 == 1
                && knots.len() == control.len() + 4
                && (0..segs.saturating_sub(1)).all(|i| knots[4 + 3 * i] == knots[5 + 3 * i] && knots[5 + 3 * i] == knots[6 + 3 * i]);
            if bezier {
                mv(c, control[0]);
                for k in control[1..].chunks(3) {
                    let _ = writeln!(c, "{} {} {} {} {} {} c", n(k[0][0]), n(k[0][1]), n(k[1][0]), n(k[1][1]), n(k[2][0]), n(k[2][1]));
                }
            } else if let Some(f) = points.first() {
                mv(c, *f);
                for p in &points[1..] {
                    ln(c, *p);
                }
            }
        }
    }
}

/// Cubic Bézier pieces (at most 90° each) of an arc.
fn arc_path(c: &mut String, center: [f64; 2], r: f64, start: f64, sweep: f64, move_first: bool) {
    let pieces = (sweep / 90.0).ceil().max(1.0) as usize;
    let step = sweep.to_radians() / pieces as f64;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |a: f64| [center[0] + r * a.cos(), center[1] + r * a.sin()];
    let mut a = start.to_radians();
    if move_first {
        let p = at(a);
        let _ = writeln!(c, "{} {} m", n(p[0]), n(p[1]));
    }
    for _ in 0..pieces {
        let b = a + step;
        let (p0, p3) = (at(a), at(b));
        let c1 = [p0[0] - k * r * a.sin(), p0[1] + k * r * a.cos()];
        let c2 = [p3[0] + k * r * b.sin(), p3[1] - k * r * b.cos()];
        let _ = writeln!(c, "{} {} {} {} {} {} c", n(c1[0]), n(c1[1]), n(c2[0]), n(c2[1]), n(p3[0]), n(p3[1]));
        a = b;
    }
}

fn pen_state(c: &mut String, pen: &Pen, last: &mut Option<(String, String, String)>) {
    let w = n(pen.width);
    let col = rgb(pen.color);
    let dash = match &pen.dash {
        Some(d) => format!("[{}] 0 d", d.iter().map(|x| n(*x)).collect::<Vec<_>>().join(" ")),
        None => "[] 0 d".into(),
    };
    let want = (w, col, dash);
    if last.as_ref() != Some(&want) {
        let _ = writeln!(c, "{} w {} RG {}", want.0, want.1, want.2);
        *last = Some(want);
    }
}

/// Writes `pages` as a PDF.
pub fn write_pdf(pages: &[Page], opts: &PdfOptions) -> Vec<u8> {
    let pages: Vec<Page> = if opts.color { pages.to_vec() } else { pages.iter().map(crate::export::to_black_and_white).collect() };
    // The faces and glyphs used.
    let mut fonts: BTreeMap<FaceKey, FontUse> = BTreeMap::new();
    let mut faces: HashMap<FaceKey, Option<ttf_parser::Face<'static>>> = HashMap::new();
    for p in &pages {
        for it in &p.items {
            // A symbol's ActualText rides on an invisible space of Inter Medium.
            if let Item::Symbol { .. } = it {
                let key = face_key(false, false);
                let data = cadrs_sketch::text::inter_data(key.0, key.1);
                if let Some(face) = faces.entry(key).or_insert_with(|| ttf_parser::Face::parse(data, 0).ok()).as_ref() {
                    let u = fonts.entry(key).or_insert_with(|| FontUse { data, glyphs: BTreeMap::new() });
                    if let Some(g) = face.glyph_index(' ') {
                        u.glyphs.entry(g.0).or_insert(' ');
                    }
                }
            }
            if let Item::Text(t) = it {
                let key = face_key(t.bold, t.italic);
                let data = cadrs_sketch::text::inter_data(key.0, key.1);
                let Some(face) = faces.entry(key).or_insert_with(|| ttf_parser::Face::parse(data, 0).ok()).as_ref() else {
                    continue;
                };
                let u = fonts.entry(key).or_insert_with(|| FontUse { data, glyphs: BTreeMap::new() });
                for ch in t.text.chars() {
                    if let Some(g) = face.glyph_index(ch) {
                        u.glyphs.entry(g.0).or_insert(ch);
                    }
                }
            }
        }
    }
    let mut w = Writer::new();
    let catalog = w.reserve();
    let pages_id = w.reserve();
    // Fonts.
    let mut font_ids: HashMap<FaceKey, (usize, String)> = HashMap::new();
    for (i, (key, u)) in fonts.iter().enumerate() {
        let Some(face) = faces[key].as_ref() else { continue };
        let upem = face.units_per_em() as f64;
        let scale = 1000.0 / upem;
        let name = match key {
            (w, true) if *w < 650 => "Inter-Italic",
            (w, _) if *w >= 750 => "Inter-ExtraBold",
            _ => "Inter-Medium",
        };
        let type0 = w.reserve();
        let cid = w.reserve();
        let desc = w.reserve();
        let file = w.reserve();
        let tounicode = w.reserve();
        let mut widths = String::new();
        for g in u.glyphs.keys() {
            let adv = face.glyph_hor_advance(ttf_parser::GlyphId(*g)).unwrap_or(0) as f64 * scale;
            let _ = write!(widths, "{g} [{}] ", n(adv));
        }
        w.object(
            type0,
            &format!("<< /Type /Font /Subtype /Type0 /BaseFont /{name} /Encoding /Identity-H /DescendantFonts [{cid} 0 R] /ToUnicode {tounicode} 0 R >>"),
        );
        w.object(
            cid,
            &format!(
                "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /{name} /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor {desc} 0 R /CIDToGIDMap /Identity /DW 600 /W [{widths}] >>"
            ),
        );
        let bb = face.global_bounding_box();
        let italic = if key.1 { -9.4 } else { 0.0 };
        let flags = if key.1 { 32 + 64 } else { 32 };
        w.object(
            desc,
            &format!(
                "<< /Type /FontDescriptor /FontName /{name} /Flags {flags} /FontBBox [{} {} {} {}] /ItalicAngle {} /Ascent {} /Descent {} /CapHeight {} /StemV {} /FontFile2 {file} 0 R >>",
                n(bb.x_min as f64 * scale),
                n(bb.y_min as f64 * scale),
                n(bb.x_max as f64 * scale),
                n(bb.y_max as f64 * scale),
                n(italic),
                n(face.ascender() as f64 * scale),
                n(face.descender() as f64 * scale),
                n(face.capital_height().unwrap_or(727) as f64 * scale),
                if key.0 >= 750 { 160 } else { 90 },
            ),
        );
        let packed = deflate(u.data);
        w.stream(file, &format!("/Filter /FlateDecode /Length1 {}", u.data.len()), &packed);
        let mut cmap = String::from(
            "/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n",
        );
        let entries: Vec<(&u16, &char)> = u.glyphs.iter().collect();
        for chunk in entries.chunks(100) {
            let _ = writeln!(cmap, "{} beginbfchar", chunk.len());
            for (g, ch) in chunk {
                let mut buf = [0u16; 2];
                let hex: String = ch.encode_utf16(&mut buf).iter().map(|u| format!("{u:04X}")).collect();
                let _ = writeln!(cmap, "<{:04X}> <{hex}>", g);
            }
            cmap.push_str("endbfchar\n");
        }
        cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
        w.stream(tounicode, "/Filter /FlateDecode", &deflate(cmap.as_bytes()));
        font_ids.insert(*key, (type0, format!("F{}", i + 1)));
    }
    let mut page_ids = Vec::new();
    for p in &pages {
        let page_id = w.reserve();
        let content_id = w.reserve();
        let mut images: Vec<(String, usize)> = Vec::new();
        let mut c = String::new();
        let _ = writeln!(c, "q\n{} 0 0 {} 0 0 cm\n1 J 1 j", n(PT_PER_MM), n(PT_PER_MM));
        let mut last_pen = None;
        for it in &p.items {
            match it {
                Item::Stroke(s, pen) => {
                    pen_state(&mut c, pen, &mut last_pen);
                    path(&mut c, s);
                    c.push_str("S\n");
                }
                Item::Fill { points, color, layer } => {
                    if points.len() < 3 {
                        continue;
                    }
                    let _ = writeln!(c, "{} rg", rgb(*color));
                    path(&mut c, &Shape::Polyline { points: points.clone(), closed: true });
                    if *layer == crate::export::Layer::Shaded {
                        // A hairline of the same colour hides the seams between triangles.
                        let _ = writeln!(c, "0.05 w {} RG [] 0 d B", rgb(*color));
                        last_pen = None;
                    } else {
                        c.push_str("f\n");
                    }
                }
                Item::Text(t) => {
                    let key = face_key(t.bold, t.italic);
                    let (Some((_, fname)), Some(Some(face))) = (font_ids.get(&key), faces.get(&key)) else { continue };
                    let hex: String = t
                        .text
                        .chars()
                        .filter_map(|ch| face.glyph_index(ch))
                        .map(|g| format!("{:04X}", g.0))
                        .collect();
                    if hex.is_empty() {
                        continue;
                    }
                    let em = t.height / crate::export::CAP_HEIGHT;
                    let (s, co) = t.rotation.to_radians().sin_cos();
                    let _ = writeln!(
                        c,
                        "BT /{fname} {} Tf {} rg {} {} {} {} {} {} Tm <{hex}> Tj ET",
                        n(em),
                        rgb(t.color),
                        n(co),
                        n(s),
                        n(-s),
                        n(co),
                        n(t.pos[0]),
                        n(t.pos[1])
                    );
                }
                Item::Symbol { pos, height, ch } => {
                    // The character as ActualText on an invisible space (render mode 3), so
                    // text extraction reads "⌴Ø.438" where the strokes draw the symbol.
                    let key = face_key(false, false);
                    let (Some((_, fname)), Some(Some(face))) = (font_ids.get(&key), faces.get(&key)) else { continue };
                    let Some(space) = face.glyph_index(' ') else { continue };
                    let mut utf16 = String::from("FEFF");
                    for u in ch.encode_utf16(&mut [0u16; 2]) {
                        let _ = write!(utf16, "{:04X}", u);
                    }
                    let em = height / crate::export::CAP_HEIGHT;
                    let _ = writeln!(
                        c,
                        "/Span <</ActualText <{utf16}>>> BDC BT 3 Tr /{fname} {} Tf 1 0 0 1 {} {} Tm <{:04X}> Tj 0 Tr ET EMC",
                        n(em),
                        n(pos[0]),
                        n(pos[1]),
                        space.0
                    );
                }
                Item::Image(pic) => {
                    let Ok(img) = crate::export::decode_image(&pic.data) else { continue };
                    let id = image_object(&mut w, &img, opts.color);
                    let name = format!("Im{}", images.len() + 1);
                    let _ = writeln!(c, "q {} 0 0 {} {} {} cm /{name} Do Q", n(pic.width), n(pic.height), n(pic.at[0]), n(pic.at[1]));
                    images.push((name, id));
                }
            }
        }
        c.push_str("Q\n");
        w.stream(content_id, "/Filter /FlateDecode", &deflate(c.as_bytes()));
        let mut res_fonts = String::new();
        for (id, name) in font_ids.values() {
            let _ = write!(res_fonts, "/{name} {id} 0 R ");
        }
        let mut res_images = String::new();
        for (name, id) in &images {
            let _ = write!(res_images, "/{name} {id} 0 R ");
        }
        w.object(
            page_id,
            &format!(
                "<< /Type /Page /Parent {pages_id} 0 R /MediaBox [0 0 {} {}] /Resources << /Font << {res_fonts}>> /XObject << {res_images}>> /ProcSet [/PDF /Text /ImageB /ImageC] >> /Contents {content_id} 0 R >>",
                n(p.width * PT_PER_MM),
                n(p.height * PT_PER_MM)
            ),
        );
        page_ids.push(page_id);
    }
    let kids: Vec<String> = page_ids.iter().map(|i| format!("{i} 0 R")).collect();
    w.object(pages_id, &format!("<< /Type /Pages /Kids [{}] /Count {} >>", kids.join(" "), page_ids.len()));
    w.object(catalog, &format!("<< /Type /Catalog /Pages {pages_id} 0 R >>"));
    w.finish(catalog)
}

/// An image XObject (with a soft mask when it has transparency); its object number.
fn image_object(w: &mut Writer, img: &image::RgbaImage, color: bool) -> usize {
    let (iw, ih) = img.dimensions();
    let id = w.reserve();
    let has_alpha = img.pixels().any(|p| p.0[3] < 255);
    let mask = has_alpha.then(|| {
        let m = w.reserve();
        let alpha: Vec<u8> = img.pixels().map(|p| p.0[3]).collect();
        w.stream(
            m,
            &format!("/Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode"),
            &deflate(&alpha),
        );
        m
    });
    let (space, samples): (&str, Vec<u8>) = if color {
        ("DeviceRGB", img.pixels().flat_map(|p| [p.0[0], p.0[1], p.0[2]]).collect())
    } else {
        ("DeviceGray", img.pixels().map(|p| (0.299 * p.0[0] as f64 + 0.587 * p.0[1] as f64 + 0.114 * p.0[2] as f64).round() as u8).collect())
    };
    let smask = mask.map(|m| format!(" /SMask {m} 0 R")).unwrap_or_default();
    w.stream(
        id,
        &format!("/Type /XObject /Subtype /Image /Width {iw} /Height {ih} /ColorSpace /{space} /BitsPerComponent 8 /Filter /FlateDecode{smask}"),
        &deflate(&samples),
    );
    id
}

/// The text of every `Tj` string of a PDF made by [`write_pdf`], decoded through its fonts'
/// ToUnicode maps, and the page sizes in points: a small reader for tests where pdftotext is
/// not installed.
pub fn read_back(pdf: &[u8]) -> (Vec<String>, Vec<(f64, f64)>) {
    let text = String::from_utf8_lossy(pdf);
    let mut sizes = Vec::new();
    for part in text.split("/MediaBox [").skip(1) {
        let nums: Vec<f64> = part.split(']').next().unwrap_or("").split_whitespace().filter_map(|x| x.parse().ok()).collect();
        if nums.len() == 4 {
            sizes.push((nums[2] - nums[0], nums[3] - nums[1]));
        }
    }
    // Inflate every stream.
    let mut streams: Vec<Vec<u8>> = Vec::new();
    let mut rest = pdf;
    while let Some(at) = find(rest, b">>\nstream\n") {
        let start = at + 10;
        let Some(end) = find(&rest[start..], b"\nendstream") else { break };
        let data = &rest[start..start + end];
        let mut d = flate2::read::ZlibDecoder::new(data);
        let mut out = Vec::new();
        if std::io::Read::read_to_end(&mut d, &mut out).is_ok() {
            streams.push(out);
        }
        rest = &rest[start + end..];
    }
    // Glyph → text, from every ToUnicode map (glyph ids are unique enough across faces here:
    // later maps don't overwrite earlier entries).
    let mut map: HashMap<u16, String> = HashMap::new();
    for s in &streams {
        let s = String::from_utf8_lossy(s);
        if !s.contains("beginbfchar") {
            continue;
        }
        for line in s.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 && parts[0].starts_with('<') && parts[1].starts_with('<') {
                let g = u16::from_str_radix(parts[0].trim_matches(|c| c == '<' || c == '>'), 16);
                let u = parts[1].trim_matches(|c| c == '<' || c == '>');
                let units: Vec<u16> = (0..u.len() / 4).filter_map(|i| u16::from_str_radix(&u[4 * i..4 * i + 4], 16).ok()).collect();
                if let Ok(g) = g {
                    map.entry(g).or_insert_with(|| String::from_utf16_lossy(&units));
                }
            }
        }
    }
    let mut strings = Vec::new();
    for s in &streams {
        let s = String::from_utf8_lossy(s);
        for part in s.split("Tm <").skip(1) {
            let hex = part.split('>').next().unwrap_or("");
            let text: String = (0..hex.len() / 4)
                .filter_map(|i| u16::from_str_radix(&hex[4 * i..4 * i + 4], 16).ok())
                .map(|g| map.get(&g).cloned().unwrap_or_default())
                .collect();
            strings.push(text);
        }
    }
    (strings, sizes)
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::{Layer, Text};

    #[test]
    fn a_page_with_text_reads_back() {
        let mut page = Page { name: "Sheet1".into(), width: 279.4, height: 215.9, items: Vec::new() };
        page.items.push(Item::Stroke(Shape::Circle { center: [50.0, 50.0], radius: 10.0 }, Pen::new(0.35, Layer::Visible)));
        page.items.push(Item::Stroke(
            Shape::Arc { center: [80.0, 50.0], radius: 10.0, start: 10.0, end: 200.0 },
            Pen { dash: Some(vec![2.0, 0.8]), ..Pen::new(0.25, Layer::Hidden) },
        ));
        for (i, s) in ["Ø4.750", "±0.1", "43.0°"].iter().enumerate() {
            page.items.push(Item::Text(Text {
                pos: [20.0, 100.0 + 10.0 * i as f64],
                height: 3.0,
                text: s.to_string(),
                bold: i == 1,
                italic: false,
                rotation: 0.0,
                color: [0, 0, 0],
                layer: Layer::Annotation,
            }));
        }
        let pdf = write_pdf(&[page.clone(), page], &PdfOptions::default());
        let (strings, sizes) = read_back(&pdf);
        assert!(strings.iter().any(|s| s == "Ø4.750"), "{strings:?}");
        assert!(strings.iter().any(|s| s == "±0.1"), "{strings:?}");
        assert!(strings.iter().any(|s| s == "43.0°"), "{strings:?}");
        assert_eq!(sizes.len(), 2);
        assert!((sizes[0].0 - 792.0).abs() < 1e-3 && (sizes[0].1 - 612.0).abs() < 1e-3, "{sizes:?}");
    }
}

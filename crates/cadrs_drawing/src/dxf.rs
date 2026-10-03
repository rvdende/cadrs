//! DXF (P3C.7, X13): an AutoCAD R2013 (AC1027) ASCII writer for drawing pages and a reader for
//! Insert DXF/DWG. Our own code, no library.
//!
//! **Writer** ([`write_dxf`]): one sheet per file, in millimetres (`$INSUNITS` 4) at paper size,
//! so the file measures like the printed sheet. Every item goes to model space on a layer named
//! for what it is (`BORDER`, `VISIBLE`, `HIDDEN`, `PHANTOM`, `ANNOTATION`, …, see
//! [`crate::export::Layer`]): straight lines as LINE, circular edges as ARC and CIRCLE, other
//! curves as LWPOLYLINE, sheet splines as SPLINE (with their fit points), texts as TEXT in the
//! `INTER` style, arrowheads as SOLID. Hidden and phantom lines use the `HIDDEN` and `PHANTOM`
//! linetypes (the dash patterns on paper, in mm). Shaded views and images are not written.
//!
//! **Reader** ([`read_dxf`]): LINE, ARC, CIRCLE, LWPOLYLINE (with bulges), POLYLINE/VERTEX,
//! SPLINE, ELLIPSE, TEXT, MTEXT, SOLID and TRACE, and INSERT and DIMENSION (their blocks,
//! transformed), from ENTITIES; units from `$INSUNITS`. Other entities are counted and skipped.

use std::collections::HashMap;
use std::fmt::Write as _;

use crate::export::{Item, Layer, Page, Pen, Shape};
use crate::sheet_sketch::{Entity, P2, arc_polyline, bspline_polyline};

// ---------------------------------------------------------------------------------------------
// Writer

struct Out {
    s: String,
    handle: u32,
    version: DxfVersion,
}

/// The DXF version written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DxfVersion {
    /// AutoCAD 2013 (AC1027), UTF-8 text.
    #[default]
    R2013,
    /// AutoCAD 2000 (AC1015), for older readers: non-ASCII text as `\U+XXXX`.
    R2000,
}

impl DxfVersion {
    pub const ALL: [DxfVersion; 2] = [DxfVersion::R2013, DxfVersion::R2000];

    pub fn label(self) -> &'static str {
        match self {
            DxfVersion::R2013 => "AutoCAD 2013 (R2013)",
            DxfVersion::R2000 => "AutoCAD 2000 (R2000)",
        }
    }

    pub fn acadver(self) -> &'static str {
        match self {
            DxfVersion::R2013 => "AC1027",
            DxfVersion::R2000 => "AC1015",
        }
    }
}

impl Out {
    fn pair(&mut self, code: i32, value: impl std::fmt::Display) {
        let _ = write!(self.s, "{code:>3}\n{value}\n");
    }

    fn num(&mut self, code: i32, v: f64) {
        // Rust's shortest round-trip form: the coordinates read back exactly.
        let v = if v == 0.0 { 0.0 } else { v };
        self.pair(code, v);
    }

    fn point(&mut self, code: i32, p: P2) {
        self.num(code, p[0]);
        self.num(code + 10, p[1]);
        self.num(code + 20, 0.0);
    }

    fn next(&mut self) -> String {
        self.handle += 1;
        format!("{:X}", self.handle)
    }
}

/// A line weight in hundredths of a mm (the nearest standard DXF weight).
fn lineweight(mm: f64) -> i32 {
    const W: [i32; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
    let want = mm * 100.0;
    *W.iter().min_by(|a, b| (**a as f64 - want).abs().total_cmp(&(**b as f64 - want).abs())).unwrap_or(&25)
}

/// (name, description, pattern: dash, gap, …) of the linetypes we write.
fn linetypes() -> [(&'static str, &'static str, Vec<f64>); 3] {
    let hidden = crate::view::LineKind::Hidden.pattern().unwrap_or(&[2.0, 0.8]).to_vec();
    let phantom = crate::view::LineKind::Phantom.pattern().unwrap_or(&[4.0, 0.8, 0.8, 0.8, 0.8, 0.8]).to_vec();
    let center = crate::flat_view::BEND_PATTERN.to_vec();
    [("HIDDEN", "Hidden __ __ __", hidden), ("PHANTOM", "Phantom ____ _ _ ____", phantom), ("CENTER", "Center ____ _ ____ _", center)]
}

/// The drawing's extents. A page of strokes only (a sketch or a face laid flat, P3F.2 judge: one
/// below or left of its origin) spans exactly its geometry, the smallest to the largest x and y;
/// a sheet (with text, fills, a border) spans its paper, (0, 0)–(width, height), and any
/// geometry beyond it.
pub fn extents(page: &Page) -> (P2, P2) {
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for it in &page.items {
        if let crate::export::Item::Stroke(s, _) = it {
            for p in s.polyline() {
                for k in 0..2 {
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
    }
    let only_strokes = page.items.iter().all(|i| matches!(i, crate::export::Item::Stroke(..)));
    match (lo[0].is_finite() && hi[0].is_finite(), only_strokes) {
        (true, true) => (lo, hi),
        (true, false) => ([lo[0].min(0.0), lo[1].min(0.0)], [hi[0].max(page.width), hi[1].max(page.height)]),
        (false, _) => ([0.0, 0.0], [page.width, page.height]),
    }
}

/// Writes a page as an R2013 ASCII DXF.
pub fn write_dxf(page: &Page) -> String {
    write_dxf_version(page, DxfVersion::R2013)
}

/// Writes a page as an ASCII DXF of `version`.
pub fn write_dxf_version(page: &Page, version: DxfVersion) -> String {
    let mut o = Out { s: String::new(), handle: 0x10, version };
    // Handles fixed up front for the owners.
    let h_vport_t = o.next();
    let h_ltype_t = o.next();
    let h_layer_t = o.next();
    let h_style_t = o.next();
    let h_view_t = o.next();
    let h_ucs_t = o.next();
    let h_appid_t = o.next();
    let h_dimstyle_t = o.next();
    let h_brec_t = o.next();
    let h_ms_rec = o.next();
    let h_ps_rec = o.next();
    let h_root_dict = o.next();
    let h_group_dict = o.next();

    // HEADER
    o.pair(0, "SECTION");
    o.pair(2, "HEADER");
    o.pair(9, "$ACADVER");
    o.pair(1, version.acadver());
    o.pair(9, "$DWGCODEPAGE");
    o.pair(3, "ANSI_1252");
    o.pair(9, "$INSBASE");
    o.point(10, [0.0, 0.0]);
    let (ext_lo, ext_hi) = extents(page);
    o.pair(9, "$EXTMIN");
    o.point(10, ext_lo);
    o.pair(9, "$EXTMAX");
    o.point(10, ext_hi);
    o.pair(9, "$LIMMIN");
    o.num(10, 0.0);
    o.num(20, 0.0);
    o.pair(9, "$LIMMAX");
    o.num(10, page.width);
    o.num(20, page.height);
    o.pair(9, "$LTSCALE");
    o.num(40, 1.0);
    o.pair(9, "$PSLTSCALE");
    o.pair(70, 0);
    o.pair(9, "$INSUNITS");
    o.pair(70, 4);
    o.pair(9, "$MEASUREMENT");
    o.pair(70, 1);
    o.pair(9, "$LUNITS");
    o.pair(70, 2);
    o.pair(9, "$LWDISPLAY");
    o.pair(290, 1);
    let handseed_at = o.s.len();
    o.pair(9, "$HANDSEED");
    o.pair(5, "FFFFF");
    o.pair(0, "ENDSEC");

    // CLASSES
    o.pair(0, "SECTION");
    o.pair(2, "CLASSES");
    o.pair(0, "ENDSEC");

    // TABLES
    o.pair(0, "SECTION");
    o.pair(2, "TABLES");
    let table = |o: &mut Out, name: &str, h: &str, n: usize| {
        o.pair(0, "TABLE");
        o.pair(2, name);
        o.pair(5, h);
        o.pair(330, 0);
        o.pair(100, "AcDbSymbolTable");
        o.pair(70, n);
    };
    let record = |o: &mut Out, kind: &str, owner: &str, sub: &str| {
        o.pair(0, kind);
        let h = o.next();
        o.pair(5, &h);
        o.pair(330, owner);
        o.pair(100, "AcDbSymbolTableRecord");
        o.pair(100, sub);
        h
    };
    // VPORT: the active viewport shows the sheet.
    table(&mut o, "VPORT", &h_vport_t, 1);
    record(&mut o, "VPORT", &h_vport_t, "AcDbViewportTableRecord");
    o.pair(2, "*Active");
    o.pair(70, 0);
    o.num(10, 0.0);
    o.num(20, 0.0);
    o.num(11, 1.0);
    o.num(21, 1.0);
    // The view centred on the drawing's extents.
    o.num(12, (ext_lo[0] + ext_hi[0]) / 2.0);
    o.num(22, (ext_lo[1] + ext_hi[1]) / 2.0);
    o.num(13, 0.0);
    o.num(23, 0.0);
    o.num(14, 10.0);
    o.num(24, 10.0);
    o.num(15, 10.0);
    o.num(25, 10.0);
    o.num(16, 0.0);
    o.num(26, 0.0);
    o.num(36, 1.0);
    o.num(17, 0.0);
    o.num(27, 0.0);
    o.num(37, 0.0);
    let (ext_w, ext_h) = ((ext_hi[0] - ext_lo[0]).max(1.0), (ext_hi[1] - ext_lo[1]).max(1.0));
    o.num(40, ext_h * 1.1);
    o.num(41, ext_w / ext_h);
    o.num(42, 50.0);
    o.num(43, 0.0);
    o.num(44, 0.0);
    o.num(50, 0.0);
    o.num(51, 0.0);
    o.pair(71, 0);
    o.pair(72, 100);
    o.pair(73, 1);
    o.pair(74, 3);
    o.pair(75, 0);
    o.pair(76, 0);
    o.pair(77, 0);
    o.pair(78, 0);
    o.pair(0, "ENDTAB");
    // LTYPE
    let lts = linetypes();
    table(&mut o, "LTYPE", &h_ltype_t, 3 + lts.len());
    for name in ["ByBlock", "ByLayer", "Continuous"] {
        record(&mut o, "LTYPE", &h_ltype_t, "AcDbLinetypeTableRecord");
        o.pair(2, name);
        o.pair(70, 0);
        o.pair(3, if name == "Continuous" { "Solid line" } else { "" });
        o.pair(72, 65);
        o.pair(73, 0);
        o.num(40, 0.0);
    }
    for (name, desc, pattern) in &lts {
        record(&mut o, "LTYPE", &h_ltype_t, "AcDbLinetypeTableRecord");
        o.pair(2, name);
        o.pair(70, 0);
        o.pair(3, desc);
        o.pair(72, 65);
        o.pair(73, pattern.len());
        o.num(40, pattern.iter().sum());
        for (i, d) in pattern.iter().enumerate() {
            o.num(49, if i % 2 == 0 { *d } else { -*d });
            o.pair(74, 0);
        }
    }
    o.pair(0, "ENDTAB");
    // LAYER
    // The flat pattern layers (P3I.6) only when the page uses them.
    let used = |l: Layer| page.items.iter().any(|it| matches!(it, Item::Stroke(_, pen) if pen.layer == l));
    let layers: Vec<Layer> = Layer::ALL.into_iter().filter(|l| !l.is_flat() || used(*l)).collect();
    table(&mut o, "LAYER", &h_layer_t, 1 + layers.len());
    let layer_rec = |o: &mut Out, name: &str, lt: &str, color: i32| {
        record(o, "LAYER", &h_layer_t, "AcDbLayerTableRecord");
        o.pair(2, name);
        o.pair(70, 0);
        o.pair(62, color);
        o.pair(6, if lt == "CONTINUOUS" { "Continuous" } else { lt });
        o.pair(370, -3);
    };
    layer_rec(&mut o, "0", "CONTINUOUS", 7);
    for l in layers {
        layer_rec(&mut o, l.name(), l.linetype(), l.aci());
    }
    o.pair(0, "ENDTAB");
    // STYLE: Standard, and INTER for our texts.
    table(&mut o, "STYLE", &h_style_t, 2);
    for (name, font) in [("Standard", "txt"), ("INTER", "Inter-Medium.ttf")] {
        record(&mut o, "STYLE", &h_style_t, "AcDbTextStyleTableRecord");
        o.pair(2, name);
        o.pair(70, 0);
        o.num(40, 0.0);
        o.num(41, 1.0);
        o.num(50, 0.0);
        o.pair(71, 0);
        o.num(42, 2.5);
        o.pair(3, font);
        o.pair(4, "");
    }
    o.pair(0, "ENDTAB");
    table(&mut o, "VIEW", &h_view_t, 0);
    o.pair(0, "ENDTAB");
    table(&mut o, "UCS", &h_ucs_t, 0);
    o.pair(0, "ENDTAB");
    table(&mut o, "APPID", &h_appid_t, 1);
    record(&mut o, "APPID", &h_appid_t, "AcDbRegAppTableRecord");
    o.pair(2, "ACAD");
    o.pair(70, 0);
    o.pair(0, "ENDTAB");
    // DIMSTYLE (its table has its own subclass).
    o.pair(0, "TABLE");
    o.pair(2, "DIMSTYLE");
    o.pair(5, &h_dimstyle_t);
    o.pair(330, 0);
    o.pair(100, "AcDbSymbolTable");
    o.pair(70, 1);
    o.pair(100, "AcDbDimStyleTable");
    o.pair(0, "DIMSTYLE");
    let h = o.next();
    o.pair(105, &h);
    o.pair(330, &h_dimstyle_t);
    o.pair(100, "AcDbSymbolTableRecord");
    o.pair(100, "AcDbDimStyleTableRecord");
    o.pair(2, "Standard");
    o.pair(70, 0);
    o.pair(0, "ENDTAB");
    // BLOCK_RECORD
    table(&mut o, "BLOCK_RECORD", &h_brec_t, 2);
    for (name, h) in [("*Model_Space", &h_ms_rec), ("*Paper_Space", &h_ps_rec)] {
        o.pair(0, "BLOCK_RECORD");
        o.pair(5, h);
        o.pair(330, &h_brec_t);
        o.pair(100, "AcDbSymbolTableRecord");
        o.pair(100, "AcDbBlockTableRecord");
        o.pair(2, name);
        o.pair(70, 0);
        if version == DxfVersion::R2013 {
            o.pair(280, 1);
            o.pair(281, 0);
        }
    }
    o.pair(0, "ENDTAB");
    o.pair(0, "ENDSEC");

    // BLOCKS
    o.pair(0, "SECTION");
    o.pair(2, "BLOCKS");
    for (name, rec, ps) in [("*Model_Space", &h_ms_rec, false), ("*Paper_Space", &h_ps_rec, true)] {
        o.pair(0, "BLOCK");
        let h = o.next();
        o.pair(5, &h);
        o.pair(330, rec);
        o.pair(100, "AcDbEntity");
        if ps {
            o.pair(67, 1);
        }
        o.pair(8, "0");
        o.pair(100, "AcDbBlockBegin");
        o.pair(2, name);
        o.pair(70, 0);
        o.point(10, [0.0, 0.0]);
        o.pair(3, name);
        o.pair(1, "");
        o.pair(0, "ENDBLK");
        let h = o.next();
        o.pair(5, &h);
        o.pair(330, rec);
        o.pair(100, "AcDbEntity");
        if ps {
            o.pair(67, 1);
        }
        o.pair(8, "0");
        o.pair(100, "AcDbBlockEnd");
    }
    o.pair(0, "ENDSEC");

    // ENTITIES
    o.pair(0, "SECTION");
    o.pair(2, "ENTITIES");
    for it in &page.items {
        write_item(&mut o, &h_ms_rec, it);
    }
    o.pair(0, "ENDSEC");

    // OBJECTS: the root dictionary with an empty group dictionary.
    o.pair(0, "SECTION");
    o.pair(2, "OBJECTS");
    o.pair(0, "DICTIONARY");
    o.pair(5, &h_root_dict);
    o.pair(330, 0);
    o.pair(100, "AcDbDictionary");
    o.pair(281, 1);
    o.pair(3, "ACAD_GROUP");
    o.pair(350, &h_group_dict);
    o.pair(0, "DICTIONARY");
    o.pair(5, &h_group_dict);
    o.pair(330, &h_root_dict);
    o.pair(100, "AcDbDictionary");
    o.pair(281, 1);
    o.pair(0, "ENDSEC");
    o.pair(0, "EOF");
    // The handle seed: past the last handle used.
    let seed = format!("{:X}", o.handle + 1);
    let old = "  9\n$HANDSEED\n  5\nFFFFF\n";
    let new = format!("  9\n$HANDSEED\n  5\n{seed}\n");
    o.s.replace_range(handseed_at..handseed_at + old.len(), &new);
    o.s
}

fn entity_head(o: &mut Out, owner: &str, kind: &str, layer: Layer, pen: Option<&Pen>, color: [u8; 3]) {
    o.pair(0, kind);
    let h = o.next();
    o.pair(5, &h);
    o.pair(330, owner);
    o.pair(100, "AcDbEntity");
    o.pair(8, layer.name());
    // The linetype on the entity as well as on its layer: some readers (ezdxf's renderer) only
    // resolve a layer's pattern late, and draw BYLAYER hidden lines solid.
    if layer.linetype() != "CONTINUOUS" {
        o.pair(6, layer.linetype());
    }
    if color == crate::export::DANGLING {
        o.pair(62, 1);
    } else if color != crate::export::INK && matches!(layer, Layer::BendUp | Layer::BendDown) {
        // A bend line's own colour (P3I.7), as a true colour.
        o.pair(420, ((color[0] as i32) << 16) | ((color[1] as i32) << 8) | color[2] as i32);
    }
    if let Some(p) = pen {
        o.pair(370, lineweight(p.width));
    }
}

fn write_item(o: &mut Out, owner: &str, it: &Item) {
    match it {
        Item::Stroke(s, pen) => match s {
            Shape::Line { a, b } => {
                entity_head(o, owner, "LINE", pen.layer, Some(pen), pen.color);
                o.pair(100, "AcDbLine");
                o.point(10, *a);
                o.point(11, *b);
            }
            Shape::Arc { center, radius, start, end } => {
                entity_head(o, owner, "ARC", pen.layer, Some(pen), pen.color);
                o.pair(100, "AcDbCircle");
                o.point(10, *center);
                o.num(40, *radius);
                o.pair(100, "AcDbArc");
                o.num(50, *start);
                o.num(51, *end);
            }
            Shape::Circle { center, radius } => {
                entity_head(o, owner, "CIRCLE", pen.layer, Some(pen), pen.color);
                o.pair(100, "AcDbCircle");
                o.point(10, *center);
                o.num(40, *radius);
            }
            Shape::Polyline { points, closed } => lwpolyline(o, owner, points, *closed, pen.layer, Some(pen), pen.color),
            Shape::Spline { knots, control, fit, .. } => {
                entity_head(o, owner, "SPLINE", pen.layer, Some(pen), pen.color);
                o.pair(100, "AcDbSpline");
                o.num(210, 0.0);
                o.num(220, 0.0);
                o.num(230, 1.0);
                o.pair(70, 8);
                o.pair(71, 3);
                o.pair(72, knots.len());
                o.pair(73, control.len());
                o.pair(74, fit.len());
                o.num(42, 1e-10);
                o.num(43, 1e-10);
                if !fit.is_empty() {
                    o.num(44, 1e-10);
                }
                for k in knots {
                    o.num(40, *k);
                }
                for p in control {
                    o.point(10, *p);
                }
                for p in fit {
                    o.point(11, *p);
                }
            }
        },
        Item::Fill { points, color, layer } => {
            if *layer == Layer::Shaded {
                return;
            }
            // Arrowheads and filled polygons: SOLIDs (triangles and quads; larger polygons as
            // a fan of triangles).
            if points.len() < 3 {
                return;
            }
            for k in 1..points.len() - 1 {
                if points.len() == 4 && k == 2 {
                    break;
                }
                let tri: Vec<P2> = if points.len() == 4 { points.clone() } else { vec![points[0], points[k], points[k + 1]] };
                entity_head(o, owner, "SOLID", *layer, None, *color);
                o.pair(100, "AcDbTrace");
                // SOLID's corners go 1, 2, 4, 3.
                let (p1, p2, p3, p4) = if tri.len() == 4 { (tri[0], tri[1], tri[3], tri[2]) } else { (tri[0], tri[1], tri[2], tri[2]) };
                o.point(10, p1);
                o.point(11, p2);
                o.point(12, p3);
                o.point(13, p4);
            }
        }
        Item::Text(t) => {
            entity_head(o, owner, "TEXT", t.layer, None, t.color);
            o.pair(100, "AcDbText");
            o.point(10, t.pos);
            o.num(40, t.height);
            let text = dxf_text(&t.text);
            let text = if o.version == DxfVersion::R2000 { ascii_escaped(&text) } else { text };
            o.pair(1, text);
            if t.rotation != 0.0 {
                o.num(50, t.rotation);
            }
            o.pair(7, "INTER");
            o.pair(100, "AcDbText");
        }
        Item::Image(_) | Item::Symbol { .. } => {}
    }
}

/// Non-ASCII characters as `\U+XXXX` (DXF before R2007 is not UTF-8).
fn ascii_escaped(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut buf = [0u16; 2];
            for u in c.encode_utf16(&mut buf) {
                out.push_str(&format!("\\U+{u:04X}"));
            }
        }
    }
    out
}

/// Text for a DXF string: no line breaks; `^` and control characters escaped.
/// A TEXT's content: one line, Ø ° ± as the `%%c` `%%d` `%%p` control codes, which every
/// reader draws with its own symbol at the right width (P3C.8: ezdxf's fallback font drew Ø
/// against the next digit).
fn dxf_text(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        match c {
            '\n' | '\r' => {}
            'Ø' | '⌀' => out.push_str("%%c"),
            '°' => out.push_str("%%d"),
            '±' => out.push_str("%%p"),
            c => out.push(c),
        }
    }
    out
}

fn lwpolyline(o: &mut Out, owner: &str, points: &[P2], closed: bool, layer: Layer, pen: Option<&Pen>, color: [u8; 3]) {
    entity_head(o, owner, "LWPOLYLINE", layer, pen, color);
    o.pair(100, "AcDbPolyline");
    o.pair(90, points.len());
    o.pair(70, if closed { 1 } else { 0 });
    o.num(43, 0.0);
    for p in points {
        o.num(10, p[0]);
        o.num(20, p[1]);
    }
}

// ---------------------------------------------------------------------------------------------
// Reader

/// A DXF file's entities (drawing units converted to mm) and what was skipped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DxfDrawing {
    pub entities: Vec<Entity>,
    /// The layer of each entity.
    pub layers: Vec<String>,
    /// Millimetres per drawing unit.
    pub unit_mm: f64,
    /// Entity types that were skipped, with their counts.
    pub skipped: Vec<(String, usize)>,
}

type Pairs = Vec<(i32, String)>;

fn pairs(text: &str) -> Result<Pairs, String> {
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(code) = lines.next() {
        let code = code.trim();
        if code.is_empty() {
            continue;
        }
        let code: i32 = code.parse().map_err(|_| format!("bad group code {code:?}"))?;
        let value = lines.next().ok_or("the file ends inside a group")?;
        let value = value.strip_suffix('\r').unwrap_or(value);
        out.push((code, value.trim_start().to_string()));
    }
    Ok(out)
}

/// One entity's groups.
#[derive(Debug, Clone, Default)]
struct Raw {
    kind: String,
    groups: Pairs,
}

impl Raw {
    fn f(&self, code: i32) -> Option<f64> {
        self.groups.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok())
    }
    fn fd(&self, code: i32, d: f64) -> f64 {
        self.f(code).unwrap_or(d)
    }
    fn i(&self, code: i32) -> Option<i64> {
        self.groups.iter().find(|(c, _)| *c == code).and_then(|(_, v)| v.trim().parse().ok())
    }
    fn s(&self, code: i32) -> Option<&str> {
        self.groups.iter().find(|(c, _)| *c == code).map(|(_, v)| v.as_str())
    }
    fn all(&self, code: i32) -> Vec<f64> {
        self.groups.iter().filter(|(c, _)| *c == code).filter_map(|(_, v)| v.trim().parse().ok()).collect()
    }
    fn p(&self, code: i32) -> P2 {
        [self.fd(code, 0.0), self.fd(code + 10, 0.0)]
    }
    /// The points given by codes `code`/`code+10`, in order.
    fn points(&self, code: i32) -> Vec<P2> {
        let mut out = Vec::new();
        let mut x = None;
        for (c, v) in &self.groups {
            if *c == code {
                x = v.trim().parse::<f64>().ok();
            } else if *c == code + 10
                && let Some(xv) = x.take()
            {
                out.push([xv, v.trim().parse().unwrap_or(0.0)]);
            }
        }
        out
    }
    fn layer(&self) -> String {
        self.s(8).unwrap_or("0").to_string()
    }
}

/// Splits a section's pairs into entities (each starting at a 0 group).
fn raws(p: &[(i32, String)]) -> Vec<Raw> {
    let mut out: Vec<Raw> = Vec::new();
    for (c, v) in p {
        if *c == 0 {
            out.push(Raw { kind: v.trim().to_string(), groups: Vec::new() });
        } else if let Some(r) = out.last_mut() {
            r.groups.push((*c, v.clone()));
        }
    }
    out
}

fn units_mm(code: i64) -> f64 {
    match code {
        1 => 25.4,
        2 => 304.8,
        3 => 1_609_344.0,
        4 => 1.0,
        5 => 10.0,
        6 => 1000.0,
        7 => 1_000_000.0,
        8 => 25.4e-6,
        9 => 25.4e-3,
        10 => 914.4,
        14 => 100.0,
        _ => 1.0,
    }
}

/// A 2D transform: `p → t + R(rot)·(s ⊙ p)`.
#[derive(Debug, Clone, Copy)]
struct Xf {
    t: P2,
    s: P2,
    rot: f64,
}

impl Xf {
    const ID: Xf = Xf { t: [0.0, 0.0], s: [1.0, 1.0], rot: 0.0 };

    fn apply(&self, p: P2) -> P2 {
        let q = [p[0] * self.s[0], p[1] * self.s[1]];
        let (s, c) = self.rot.to_radians().sin_cos();
        [self.t[0] + c * q[0] - s * q[1], self.t[1] + s * q[0] + c * q[1]]
    }

    fn uniform(&self) -> bool {
        (self.s[0] - self.s[1]).abs() < 1e-12 * self.s[0].abs().max(1.0)
    }

    fn then(&self, outer: &Xf) -> Xf {
        // outer ∘ self, when both are uniform or rotation-free; otherwise approximate by
        // composing the parts (non-uniform scales inside rotated inserts are rare).
        Xf {
            t: outer.apply(self.t),
            s: [self.s[0] * outer.s[0], self.s[1] * outer.s[1]],
            rot: self.rot + outer.rot,
        }
    }
}

struct Ctx<'a> {
    blocks: &'a HashMap<String, (P2, Vec<Raw>)>,
    out: &'a mut DxfDrawing,
    skipped: HashMap<String, usize>,
    depth: usize,
}

/// Reads a DXF (ASCII).
pub fn read_dxf(text: &str) -> Result<DxfDrawing, String> {
    let p = pairs(text)?;
    // Sections.
    let mut sections: HashMap<String, Pairs> = HashMap::new();
    let mut i = 0;
    while i < p.len() {
        if p[i].0 == 0 && p[i].1.trim() == "SECTION" && i + 1 < p.len() && p[i + 1].0 == 2 {
            let name = p[i + 1].1.trim().to_string();
            let mut j = i + 2;
            while j < p.len() && !(p[j].0 == 0 && p[j].1.trim() == "ENDSEC") {
                j += 1;
            }
            sections.insert(name, p[i + 2..j.min(p.len())].to_vec());
            i = j + 1;
        } else {
            i += 1;
        }
    }
    let entities = sections.get("ENTITIES").ok_or("no ENTITIES section: not a DXF drawing")?;
    let mut unit = 4;
    if let Some(h) = sections.get("HEADER")
        && let Some(k) = h.iter().position(|(c, v)| *c == 9 && v.trim() == "$INSUNITS")
        && let Some((_, v)) = h.get(k + 1)
    {
        unit = v.trim().parse().unwrap_or(4);
    }
    // Blocks by name (base point, entities).
    let mut blocks: HashMap<String, (P2, Vec<Raw>)> = HashMap::new();
    if let Some(b) = sections.get("BLOCKS") {
        let rs = raws(b);
        let mut cur: Option<(String, P2, Vec<Raw>)> = None;
        for r in rs {
            match r.kind.as_str() {
                "BLOCK" => cur = Some((r.s(2).unwrap_or("").to_string(), r.p(10), Vec::new())),
                "ENDBLK" => {
                    if let Some((n, base, es)) = cur.take() {
                        blocks.insert(n, (base, es));
                    }
                }
                _ => {
                    if let Some((_, _, es)) = cur.as_mut() {
                        es.push(r);
                    }
                }
            }
        }
    }
    let mut out = DxfDrawing { unit_mm: units_mm(unit), ..DxfDrawing::default() };
    let rs = raws(entities);
    let mut ctx = Ctx { blocks: &blocks, out: &mut out, skipped: HashMap::new(), depth: 0 };
    convert(&mut ctx, &rs, Xf::ID);
    let mut skipped: Vec<(String, usize)> = ctx.skipped.into_iter().collect();
    skipped.sort();
    out.skipped = skipped;
    Ok(out)
}

fn push(ctx: &mut Ctx, e: Entity, layer: &str) {
    ctx.out.entities.push(e);
    ctx.out.layers.push(layer.to_string());
}

fn convert(ctx: &mut Ctx, rs: &[Raw], xf: Xf) {
    let mut k = 0;
    while k < rs.len() {
        let r = &rs[k];
        k += 1;
        let layer = r.layer();
        // Paper-space entities (67 = 1) are not part of the drawing's model.
        if r.i(67) == Some(1) && ctx.depth == 0 {
            continue;
        }
        let m = |p: P2| xf.apply(p);
        match r.kind.as_str() {
            "LINE" => push(ctx, Entity::Line { a: m(r.p(10)), b: m(r.p(11)) }, &layer),
            "CIRCLE" | "ARC" => {
                let c = r.p(10);
                let rad = r.fd(40, 0.0);
                let (a0, a1) = if r.kind == "ARC" { (r.fd(50, 0.0), r.fd(51, 360.0)) } else { (0.0, 360.0) };
                if xf.uniform() && xf.s[0] > 0.0 {
                    let e = if r.kind == "ARC" {
                        Entity::Arc { center: m(c), radius: rad * xf.s[0], start: (a0 + xf.rot).rem_euclid(360.0), end: (a1 + xf.rot).rem_euclid(360.0) }
                    } else {
                        Entity::Circle { center: m(c), radius: rad * xf.s[0] }
                    };
                    push(ctx, e, &layer);
                } else {
                    let pts = arc_polyline(c, rad, a0, a1).into_iter().map(m).collect();
                    push(ctx, Entity::Polyline { points: pts, bulges: Vec::new(), closed: false }, &layer);
                }
            }
            "LWPOLYLINE" => {
                let closed = r.i(70).unwrap_or(0) & 1 == 1;
                // Vertices with their bulges (a 42 after a vertex's 10/20).
                let mut points = Vec::new();
                let mut bulges = Vec::new();
                let mut x = None;
                for (c, v) in &r.groups {
                    match c {
                        10 => x = v.trim().parse::<f64>().ok(),
                        20 => {
                            if let Some(xv) = x.take() {
                                points.push([xv, v.trim().parse().unwrap_or(0.0)]);
                                bulges.push(0.0);
                            }
                        }
                        42 => {
                            if let Some(b) = bulges.last_mut() {
                                *b = v.trim().parse().unwrap_or(0.0);
                            }
                        }
                        _ => {}
                    }
                }
                push_polyline(ctx, points, bulges, closed, xf, &layer);
            }
            "POLYLINE" => {
                let closed = r.i(70).unwrap_or(0) & 1 == 1;
                let mut points = Vec::new();
                let mut bulges = Vec::new();
                while k < rs.len() && rs[k].kind == "VERTEX" {
                    points.push(rs[k].p(10));
                    bulges.push(rs[k].fd(42, 0.0));
                    k += 1;
                }
                if k < rs.len() && rs[k].kind == "SEQEND" {
                    k += 1;
                }
                push_polyline(ctx, points, bulges, closed, xf, &layer);
            }
            "SPLINE" => {
                let degree = r.i(71).unwrap_or(3).max(1) as usize;
                let knots = r.all(40);
                let weights = r.all(41);
                let control: Vec<P2> = r.points(10).into_iter().map(m).collect();
                let fit: Vec<P2> = r.points(11).into_iter().map(m).collect();
                if control.len() >= 2 && knots.len() > control.len() + degree {
                    push(ctx, Entity::Spline { degree, knots, control, weights, fit }, &layer);
                } else if fit.len() >= 2 {
                    // Fit points only: our spline through them.
                    let (knots, control) = crate::sheet_sketch::spline_nurbs(&fit);
                    push(ctx, Entity::Spline { degree: 3, knots, control, weights: Vec::new(), fit }, &layer);
                } else {
                    *ctx.skipped.entry("SPLINE".into()).or_default() += 1;
                }
            }
            "ELLIPSE" => {
                let c = r.p(10);
                let major = r.p(11);
                let ratio = r.fd(40, 1.0);
                let (t0, t1) = (r.fd(41, 0.0), r.fd(42, std::f64::consts::TAU));
                let minor = [-major[1] * ratio, major[0] * ratio];
                let mut sweep = t1 - t0;
                if sweep <= 0.0 {
                    sweep += std::f64::consts::TAU;
                }
                let n = ((sweep.to_degrees() / 5.0).ceil() as usize).max(8);
                let pts: Vec<P2> = (0..=n)
                    .map(|i| {
                        let t = t0 + sweep * i as f64 / n as f64;
                        m([c[0] + major[0] * t.cos() + minor[0] * t.sin(), c[1] + major[1] * t.cos() + minor[1] * t.sin()])
                    })
                    .collect();
                push(ctx, Entity::Polyline { points: pts, bulges: Vec::new(), closed: false }, &layer);
            }
            "SOLID" | "TRACE" => {
                let (p1, p2, p3, p4) = (r.p(10), r.p(11), r.p(12), r.p(13));
                let mut pts = vec![m(p1), m(p2), m(p4), m(p3)];
                if (p3[0] - p4[0]).abs() < 1e-12 && (p3[1] - p4[1]).abs() < 1e-12 {
                    pts = vec![m(p1), m(p2), m(p3)];
                }
                push(ctx, Entity::Solid { points: pts }, &layer);
            }
            "TEXT" | "ATTRIB" => {
                let text = decode_text(r.s(1).unwrap_or(""));
                if text.trim().is_empty() {
                    continue;
                }
                let h = r.fd(40, 2.5);
                let rot = r.fd(50, 0.0);
                let (hj, vj) = (r.i(72).unwrap_or(0), r.i(73).unwrap_or(0));
                let mut at = r.p(10);
                if (hj != 0 || vj != 0) && r.f(11).is_some() {
                    // Aligned text: from its alignment point back to the baseline's left end.
                    let a = r.p(11);
                    let w = crate::annotation::text_width(&text) * h;
                    let dx = match hj {
                        1 | 4 => -w / 2.0,
                        2 => -w,
                        _ => 0.0,
                    };
                    let dy = match vj {
                        2 => -h / 2.0,
                        3 => -h,
                        _ => 0.0,
                    };
                    let (s, c) = rot.to_radians().sin_cos();
                    at = [a[0] + c * dx - s * dy, a[1] + s * dx + c * dy];
                }
                let e = text_entity(at, h, text, rot, xf);
                push(ctx, e, &layer);
            }
            "MTEXT" => {
                let mut raw = String::new();
                for (c, v) in &r.groups {
                    if *c == 3 || *c == 1 {
                        raw.push_str(v);
                    }
                }
                let lines = mtext_lines(&raw);
                let h = r.fd(40, 2.5);
                let rot = match (r.f(11), r.f(21)) {
                    (Some(dx), Some(dy)) if dx != 0.0 || dy != 0.0 => dy.atan2(dx).to_degrees(),
                    _ => r.f(50).unwrap_or(0.0),
                };
                let attach = r.i(71).unwrap_or(1);
                let origin = r.p(10);
                let step = h * 5.0 / 3.0;
                let n = lines.len().max(1) as f64;
                let total = h + step * (n - 1.0);
                // The first baseline, from the attachment's row.
                let y0 = match (attach - 1) / 3 {
                    0 => -h,
                    1 => total / 2.0 - h,
                    _ => total - h,
                };
                let (s, c) = rot.to_radians().sin_cos();
                for (i, line) in lines.iter().enumerate() {
                    if line.trim().is_empty() {
                        continue;
                    }
                    let w = crate::annotation::text_width(line) * h;
                    let dx = match (attach - 1) % 3 {
                        1 => -w / 2.0,
                        2 => -w,
                        _ => 0.0,
                    };
                    let dy = y0 - step * i as f64;
                    let at = [origin[0] + c * dx - s * dy, origin[1] + s * dx + c * dy];
                    let e = text_entity(at, h, line.clone(), rot, xf);
                    push(ctx, e, &layer);
                }
            }
            "INSERT" | "DIMENSION" => {
                let Some(name) = r.s(2).map(str::to_string) else { continue };
                let Some((base, es)) = ctx.blocks.get(&name) else {
                    *ctx.skipped.entry(format!("{} {name}", r.kind)).or_default() += 1;
                    continue;
                };
                if ctx.depth > 16 {
                    continue;
                }
                let inner = if r.kind == "INSERT" {
                    let at = r.p(10);
                    let s = [r.fd(41, 1.0), r.fd(42, 1.0)];
                    let rot = r.fd(50, 0.0);
                    let local = Xf { t: [0.0, 0.0], s, rot };
                    let b = local.apply(*base);
                    Xf { t: [at[0] - b[0], at[1] - b[1]], s, rot }.then(&xf)
                } else {
                    xf
                };
                let es = es.clone();
                ctx.depth += 1;
                convert(ctx, &es, inner);
                ctx.depth -= 1;
            }
            "VERTEX" | "SEQEND" | "ATTDEF" | "POINT" | "VIEWPORT" => {}
            other => *ctx.skipped.entry(other.to_string()).or_default() += 1,
        }
    }
}

fn text_entity(at: P2, h: f64, text: String, rot: f64, xf: Xf) -> Entity {
    let s = if xf.uniform() { xf.s[0].abs() } else { (xf.s[0].abs() * xf.s[1].abs()).sqrt() };
    Entity::Text { at: xf.apply(at), height: h * s, text, rotation: rot + xf.rot }
}

fn push_polyline(ctx: &mut Ctx, points: Vec<P2>, bulges: Vec<f64>, closed: bool, xf: Xf, layer: &str) {
    if points.len() < 2 {
        return;
    }
    if xf.uniform() && xf.s[0] > 0.0 {
        let pts = points.into_iter().map(|p| xf.apply(p)).collect();
        push(ctx, Entity::Polyline { points: pts, bulges: if bulges.iter().all(|b| *b == 0.0) { Vec::new() } else { bulges }, closed }, layer);
    } else {
        let flat = crate::sheet_sketch::bulge_polyline(&points, &bulges, closed);
        push(ctx, Entity::Polyline { points: flat.into_iter().map(|p| xf.apply(p)).collect(), bulges: Vec::new(), closed: false }, layer);
    }
}

/// `\U+00D8` escapes and `%%c`, `%%d`, `%%p` in TEXT.
fn decode_text(s: &str) -> String {
    let s = s.replace("%%c", "Ø").replace("%%C", "Ø").replace("%%d", "°").replace("%%D", "°").replace("%%p", "±").replace("%%P", "±");
    let mut out = String::new();
    let mut rest = s.as_str();
    while let Some(i) = rest.find("\\U+") {
        out.push_str(&rest[..i]);
        let hex = rest.get(i + 3..i + 7).unwrap_or("");
        match u32::from_str_radix(hex, 16).ok().and_then(char::from_u32) {
            Some(c) if hex.len() == 4 => {
                out.push(c);
                rest = &rest[i + 7..];
            }
            _ => {
                out.push_str("\\U+");
                rest = &rest[i + 3..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// MTEXT's lines with its formatting codes removed.
fn mtext_lines(raw: &str) -> Vec<String> {
    let s = decode_text(raw);
    let mut lines = vec![String::new()];
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' | '}' => {}
            '\\' => match chars.next() {
                Some('P') | Some('X') => lines.push(String::new()),
                Some('~') => lines.last_mut().unwrap().push(' '),
                Some('\\') => lines.last_mut().unwrap().push('\\'),
                Some('{') => lines.last_mut().unwrap().push('{'),
                Some('}') => lines.last_mut().unwrap().push('}'),
                Some('S') => {
                    // A stacked fraction: a/b → "a/b".
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                        lines.last_mut().unwrap().push(if d == '^' || d == '#' { '/' } else { d });
                    }
                }
                Some('L' | 'l' | 'O' | 'o' | 'K' | 'k') => {}
                Some(_) => {
                    // \f…; \H…; \C…; \A…; \T…; \Q…; \W…; \p…;
                    for d in chars.by_ref() {
                        if d == ';' {
                            break;
                        }
                    }
                }
                None => {}
            },
            _ => lines.last_mut().unwrap().push(c),
        }
    }
    lines
}

/// Counts of LINE, ARC and CIRCLE entities in a read drawing.
pub fn counts(d: &DxfDrawing) -> (usize, usize, usize) {
    let mut c = (0, 0, 0);
    for e in &d.entities {
        match e {
            Entity::Line { .. } => c.0 += 1,
            Entity::Arc { .. } => c.1 += 1,
            Entity::Circle { .. } => c.2 += 1,
            _ => {}
        }
    }
    c
}

/// A spline's polyline, for callers that draw read entities.
pub fn spline_points(e: &Entity) -> Vec<P2> {
    match e {
        Entity::Spline { degree, knots, control, weights, .. } => bspline_polyline(*degree, knots, control, weights, 16),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::export::Text;

    #[test]
    fn a_page_round_trips() {
        let mut page = Page { name: "S".into(), width: 279.4, height: 215.9, items: Vec::new() };
        let pen = |l| Pen::new(0.35, l);
        page.items.push(Item::Stroke(Shape::Line { a: [1.0 / 3.0, 2.0], b: [100.123456789, 50.5] }, pen(Layer::Visible)));
        page.items.push(Item::Stroke(Shape::Arc { center: [50.0, 60.0], radius: 7.25, start: 350.0, end: 20.0 }, pen(Layer::Hidden)));
        page.items.push(Item::Stroke(Shape::Circle { center: [10.0, 10.0], radius: 3.0 }, pen(Layer::Visible)));
        page.items.push(Item::Stroke(Shape::Polyline { points: vec![[0.0, 0.0], [5.0, 1.0], [7.0, 4.0]], closed: false }, pen(Layer::Tangent)));
        let pts = [[0.0, 0.0], [10.0, 5.0], [20.0, 0.0]];
        let (knots, control) = crate::sheet_sketch::spline_nurbs(&pts);
        page.items.push(Item::Stroke(Shape::Spline { knots, control, fit: pts.to_vec(), points: Vec::new() }, pen(Layer::SheetSketch)));
        page.items.push(Item::Fill { points: vec![[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]], color: [0, 0, 0], layer: Layer::Annotation });
        page.items.push(Item::Text(Text {
            pos: [5.0, 6.0],
            height: 3.0,
            text: "Ø4.750".into(),
            bold: false,
            italic: false,
            rotation: 90.0,
            color: [0, 0, 0],
            layer: Layer::Annotation,
        }));
        let text = write_dxf(&page);
        assert!(text.contains("AC1027"));
        let d = read_dxf(&text).unwrap();
        assert_eq!(counts(&d), (1, 1, 1));
        assert_eq!(d.unit_mm, 1.0);
        assert_eq!(d.entities[0], Entity::Line { a: [1.0 / 3.0, 2.0], b: [100.123456789, 50.5] });
        assert_eq!(d.entities[1], Entity::Arc { center: [50.0, 60.0], radius: 7.25, start: 350.0, end: 20.0 });
        assert!(matches!(&d.entities[4], Entity::Spline { degree: 3, fit, .. } if fit.len() == 3));
        assert!(matches!(&d.entities[5], Entity::Solid { points } if points.len() == 3));
        assert!(matches!(&d.entities[6], Entity::Text { text, rotation, .. } if text == "Ø4.750" && *rotation == 90.0));
        assert_eq!(d.layers[1], "HIDDEN");
        assert!(d.skipped.is_empty(), "{:?}", d.skipped);
        // R2000: ASCII with \U+ escapes, read back as the same text; hidden lines carry their
        // linetype on the entity too.
        let old = write_dxf_version(&page, DxfVersion::R2000);
        assert!(old.contains("AC1015") && old.contains("%%c4.750") && old.is_ascii());
        let d = read_dxf(&old).unwrap();
        assert!(matches!(&d.entities[6], Entity::Text { text, .. } if text == "Ø4.750"));
        assert!(text.contains("  8\nHIDDEN\n  6\nHIDDEN\n"));
    }

    #[test]
    fn reads_bulges_inserts_mtext_and_units() {
        let dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n1\n0\nENDSEC\n0\nSECTION\n2\nBLOCKS\n0\nBLOCK\n2\nB1\n10\n1.0\n20\n0.0\n0\nLINE\n8\n0\n10\n1.0\n20\n0.0\n11\n2.0\n21\n0.0\n0\nENDBLK\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLWPOLYLINE\n90\n2\n70\n0\n10\n0.0\n20\n0.0\n42\n1.0\n10\n2.0\n20\n0.0\n0\nINSERT\n2\nB1\n10\n10.0\n20\n5.0\n41\n2.0\n42\n2.0\n0\nMTEXT\n10\n0.0\n20\n10.0\n40\n0.2\n71\n1\n1\n{\\fArial|b0;AB}\\PCD\n0\nENDSEC\n0\nEOF\n";
        let d = read_dxf(dxf).unwrap();
        assert_eq!(d.unit_mm, 25.4);
        assert!(matches!(&d.entities[0], Entity::Polyline { bulges, .. } if bulges[0] == 1.0));
        // The block's base (1, 0) lands on the insertion point, scaled 2.
        assert_eq!(d.entities[1], Entity::Line { a: [10.0, 5.0], b: [12.0, 5.0] });
        let texts: Vec<&str> = d.entities.iter().filter_map(|e| if let Entity::Text { text, .. } = e { Some(text.as_str()) } else { None }).collect();
        assert_eq!(texts, ["AB", "CD"]);
    }
}

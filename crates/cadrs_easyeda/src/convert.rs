//! EasyEDA (Standard) part data → a cadrs symbol, footprint and 3D model placement.
//!
//! EasyEDA draws in 10 mil units (0.254 mm) with Y down, around the part's origin (`head.x`,
//! `head.y`); shapes are `~`-separated strings, pins `^^`-separated groups of them. cadrs is
//! mm with Y up, so every point is `((x - ox) · 0.254, −(y − oy) · 0.254)`.

use crate::path::{self, Seg, Sub};
use cadrs_eda::footprint::*;
use cadrs_eda::graphics::{Fill, Geom, HAlign, Shape, Stroke, Text, TextStyle};
use cadrs_eda::layer::{Layer, LayerSet};
use cadrs_eda::symbol::*;
use cadrs_eda::units::{Pt, Size, mm, to_mm};
use serde_json::Value;

/// mm per EasyEDA unit.
const U: f64 = 0.254;

/// Field names imported parts carry (the BOM's "LCSC Part" among them).
pub mod field_names {
    pub const LCSC: &str = "LCSC Part";
    pub const MANUFACTURER: &str = "Manufacturer";
    pub const MPN: &str = "Manufacturer Part";
    pub const JLCPCB_CLASS: &str = "JLCPCB Part Class";
    pub const SOURCE: &str = "Source";
}

/// A part's symbol and footprint, and where its 3D model goes once its extent is known.
#[derive(Clone, Debug)]
pub struct Converted {
    pub symbol: Symbol,
    pub footprint: Footprint,
    pub model: Option<ModelRef>,
    pub warnings: Vec<String>,
}

/// The footprint's 3D model as EasyEDA places it.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelRef {
    /// The model's id on EasyEDA's model server.
    pub uuid: String,
    pub name: String,
    /// The centre of the model's outline seen from above, in footprint mm.
    pub center: [f64; 2],
    /// Where the model's lowest point sits, mm above the board.
    pub z: f64,
    /// Degrees about X, Y, Z (EasyEDA's).
    pub rotation: [f64; 3],
}

fn num(s: &str) -> f64 {
    s.trim().parse().unwrap_or(0.0)
}

fn field<'a>(f: &[&'a str], i: usize) -> &'a str {
    f.get(i).copied().unwrap_or("")
}

/// EasyEDA units around an origin → cadrs mm.
#[derive(Clone, Copy)]
struct Frame {
    ox: f64,
    oy: f64,
}

impl Frame {
    fn of(head: &Value) -> Frame {
        let n = |v: &Value| v.as_f64().or_else(|| v.as_str().map(num)).unwrap_or(0.0);
        Frame { ox: n(&head["x"]), oy: n(&head["y"]) }
    }

    fn xy(&self, x: f64, y: f64) -> [f64; 2] {
        // Rounded to 0.1 µm: EasyEDA's mil-based numbers come with float noise.
        let r = |v: f64| (v * 1e4).round() / 1e4 + 0.0;
        [r((x - self.ox) * U), r(-(y - self.oy) * U)]
    }

    fn pt(&self, x: f64, y: f64) -> Pt {
        let [x, y] = self.xy(x, y);
        Pt::mm(x, y)
    }

    fn p(&self, q: [f64; 2]) -> Pt {
        self.pt(q[0], q[1])
    }
}

/// Space-separated `x y x y …` points.
fn points(s: &str) -> Vec<[f64; 2]> {
    let v: Vec<f64> = s.split([' ', ',']).filter(|t| !t.is_empty()).map(num).collect();
    v.as_chunks::<2>().0.to_vec()
}

/// A path's pieces as geometry: arcs as arcs, runs of lines as polylines.
fn sub_geoms(f: &Frame, sub: &Sub) -> Vec<Geom> {
    let mut out = vec![];
    let mut run: Vec<Pt> = vec![];
    let flush = |run: &mut Vec<Pt>, out: &mut Vec<Geom>| {
        if run.len() >= 2 {
            out.push(Geom::Polyline { pts: std::mem::take(run), closed: false });
        }
        run.clear();
    };
    if sub.closed && sub.segs.iter().all(|s| matches!(s, Seg::Line(..))) {
        let mut pts: Vec<Pt> = sub.points(1).into_iter().map(|q| f.p(q)).collect();
        if pts.len() > 1 && pts.first() == pts.last() {
            pts.pop();
        }
        return vec![Geom::Polyline { pts, closed: true }];
    }
    for s in &sub.segs {
        match *s {
            Seg::Line(a, b) => {
                if run.is_empty() {
                    run.push(f.p(a));
                }
                run.push(f.p(b));
            }
            Seg::Arc(a, m, b) => {
                flush(&mut run, &mut out);
                out.push(Geom::Arc { start: f.p(a), mid: f.p(m), end: f.p(b) });
            }
        }
    }
    flush(&mut run, &mut out);
    out
}

fn stroke(w: f64) -> Stroke {
    Stroke::width(mm(w * U))
}

fn fill_of(s: &str) -> Fill {
    match s.trim().to_ascii_lowercase().as_str() {
        "" | "none" | "transparent" => Fill::None,
        _ => Fill::Background,
    }
}

// ---------------------------------------------------------------------------------------------
// Symbol

fn pin_kind(e: &str) -> PinType {
    match e.trim() {
        "1" => PinType::Input,
        "2" => PinType::Output,
        "3" => PinType::Bidirectional,
        "4" => PinType::PowerIn,
        // EasyEDA leaves most pins "undefined": passive keeps ERC quiet about them.
        _ => PinType::Passive,
    }
}

/// `P~show~elec~spice~x~y~rot~id^^dot x~y^^path~color^^name…^^number…^^inverted…^^clock…`
fn symbol_pin(f: &Frame, s: &str) -> Option<Pin> {
    let parts: Vec<&str> = s.split("^^").collect();
    let set: Vec<&str> = parts.first()?.split('~').collect();
    let conn: Vec<&str> = parts.get(1)?.split('~').collect();
    let at_e = [num(field(&conn, 0)), num(field(&conn, 1))];
    let at = f.p(at_e);
    // The pin line runs from the connection point to the body: its far end.
    let line = parts.get(2).map(|p| p.split('~').next().unwrap_or("")).unwrap_or("");
    let ends: Vec<[f64; 2]> = path::parse(line).iter().flat_map(|s| s.points(1)).collect();
    let far = ends.into_iter().max_by(|a, b| {
        let d = |q: &[f64; 2]| (q[0] - at_e[0]).hypot(q[1] - at_e[1]);
        d(a).total_cmp(&d(b))
    });
    let (angle, length) = match far.map(|q| f.p(q) - at) {
        Some(d) if d.x != 0 || d.y != 0 => ((d.y as f64).atan2(d.x as f64).to_degrees().rem_euclid(360.0).round() % 360.0, (d.x as f64).hypot(d.y as f64).round() as i64),
        // No line: EasyEDA's rotation (0 = the pin points right, away from the body).
        _ => ((180.0 - num(field(&set, 6))).rem_euclid(360.0), mm(10.0 * U)),
    };
    let name: Vec<&str> = parts.get(3).map(|p| p.split('~').collect()).unwrap_or_default();
    let number: Vec<&str> = parts.get(4).map(|p| p.split('~').collect()).unwrap_or_default();
    let flag = |i: usize| parts.get(i).is_some_and(|p| p.split('~').next() == Some("1"));
    let shape = match (flag(5), flag(6)) {
        (true, true) => PinShape::InvertedClock,
        (true, false) => PinShape::Inverted,
        (false, true) => PinShape::Clock,
        _ => PinShape::Line,
    };
    let num_text = field(&number, 4).trim();
    let mut p = pin(if num_text.is_empty() { field(&set, 3) } else { num_text }, field(&name, 4).trim(), pin_kind(field(&set, 2)), (0.0, 0.0), angle, 0.0);
    p.at = at;
    p.length = length;
    p.shape = shape;
    p.visible = !field(&set, 1).is_empty();
    if p.name.is_empty() {
        p.name = "~".into();
    }
    Some(p)
}

fn graphic(item: SymbolItem) -> SymbolGraphic {
    SymbolGraphic { item, unit: 0, style: 0 }
}

fn shape_item(geom: Geom, w: f64, fill: Fill) -> SymbolGraphic {
    graphic(SymbolItem::Shape(Shape { geom, stroke: stroke(w), fill }))
}

/// The symbol's drawing, pins and the text marks for its reference and value.
fn symbol_shapes(f: &Frame, shapes: &[Value], s: &mut Symbol, warnings: &mut Vec<String>) -> (Option<Pt>, Option<Pt>) {
    let (mut ref_at, mut value_at) = (None, None);
    for v in shapes {
        let Some(str) = v.as_str() else { continue };
        if str.starts_with("P~") {
            match symbol_pin(f, str) {
                Some(p) => s.pins.push(p),
                None => warnings.push("a pin couldn't be read".into()),
            }
            continue;
        }
        let t: Vec<&str> = str.split('~').collect();
        let n = |i: usize| num(field(&t, i));
        match t[0] {
            "R" => {
                let (x, y, w, h) = (n(1), n(2), n(5), n(6));
                s.graphics.push(shape_item(Geom::Rect { a: f.pt(x, y), b: f.pt(x + w, y + h) }, n(8), fill_of(field(&t, 10))));
            }
            "E" | "C" => {
                let (cx, cy, rx, ry, (w, fill)) = if t[0] == "E" { (n(1), n(2), n(3), n(4), (n(6), field(&t, 8))) } else { (n(1), n(2), n(3), n(3), (n(5), field(&t, 7))) };
                if (rx - ry).abs() < 1e-6 {
                    s.graphics.push(shape_item(Geom::Circle { center: f.pt(cx, cy), radius: mm(rx * U) }, w, fill_of(fill)));
                } else {
                    let pts = (0..32).map(|i| i as f64 * std::f64::consts::TAU / 32.0).map(|a| f.pt(cx + rx * a.cos(), cy + ry * a.sin())).collect();
                    s.graphics.push(shape_item(Geom::Polyline { pts, closed: true }, w, fill_of(fill)));
                }
            }
            "PL" | "PG" => {
                let pts: Vec<Pt> = points(field(&t, 1)).into_iter().map(|q| f.p(q)).collect();
                if pts.len() >= 2 {
                    let closed = t[0] == "PG";
                    s.graphics.push(shape_item(Geom::Polyline { pts, closed }, n(3), if closed { fill_of(field(&t, 5)) } else { Fill::None }));
                }
            }
            "PT" | "A" => {
                // PT~path~stroke~width~style~fill; A~path~helper~stroke~width~style~fill.
                let (w, fill) = if t[0] == "PT" { (n(3), field(&t, 5)) } else { (n(4), field(&t, 6)) };
                for sub in path::parse(field(&t, 1)) {
                    let fill = if sub.closed { fill_of(fill) } else { Fill::None };
                    for g in sub_geoms(f, &sub) {
                        s.graphics.push(shape_item(g, w, fill));
                    }
                }
            }
            "T" => {
                // T~mark~x~y~rotation~color~font~size~weight~style~baseline~type~text~visible~anchor
                let at = f.pt(n(2), n(3));
                match field(&t, 1) {
                    "P" => ref_at = Some(at),
                    "N" => value_at = Some(at),
                    _ if field(&t, 13) != "0" && !field(&t, 12).trim().is_empty() => {
                        let h_align = match field(&t, 14) {
                            "start" => HAlign::Left,
                            "end" => HAlign::Right,
                            _ => HAlign::Center,
                        };
                        let text = Text { text: field(&t, 12).into(), at, angle: -n(4), style: TextStyle { h_align, ..Default::default() }, visible: true };
                        s.graphics.push(graphic(SymbolItem::Text(text)));
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    (ref_at, value_at)
}

/// The box around a symbol's pins and drawing (mm).
fn symbol_box(s: &Symbol) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    let mut add = |p: Pt| {
        let q = [to_mm(p.x), to_mm(p.y)];
        for i in 0..2 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    };
    for p in &s.pins {
        add(p.at);
        add(p.inner_end());
    }
    for g in &s.graphics {
        if let SymbolItem::Shape(sh) = &g.item {
            for p in geom_points(&sh.geom) {
                add(p);
            }
        }
    }
    if lo[0] > hi[0] {
        return ([0.0; 2], [0.0; 2]);
    }
    (lo, hi)
}

fn geom_points(g: &Geom) -> Vec<Pt> {
    match g {
        Geom::Line { a, b } | Geom::Rect { a, b } => vec![*a, *b],
        Geom::Polyline { pts, .. } => pts.clone(),
        Geom::Circle { center, radius } => vec![*center - Pt::new(*radius, *radius), *center + Pt::new(*radius, *radius)],
        Geom::Arc { start, mid, end } => vec![*start, *mid, *end],
        Geom::Bezier { pts } => pts.to_vec(),
    }
}

fn snap(v: f64, grid: f64) -> f64 {
    (v / grid).round() * grid
}

fn set_field(s: &mut Symbol, name: &str, value: &str) {
    match s.fields.iter_mut().find(|f| f.name == name) {
        Some(f) => f.text.text = value.into(),
        None => {
            let mut f = s.fields[s.fields.len() - 1].clone();
            f.name = name.into();
            f.text.text = value.into();
            f.text.visible = false;
            s.fields.push(f);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Footprint

fn fp_layer(id: &str) -> Option<Layer> {
    Some(match id.trim() {
        "1" => Layer::TopCopper,
        "2" => Layer::BottomCopper,
        "3" => Layer::TopSilk,
        "4" => Layer::BottomSilk,
        "5" => Layer::TopPaste,
        "6" => Layer::BottomPaste,
        "7" => Layer::TopMask,
        "8" => Layer::BottomMask,
        "10" => Layer::Outline,
        "12" => Layer::Drawings,
        "13" => Layer::TopFab,
        "14" => Layer::BottomFab,
        // The part's body shape and its polarity marks.
        "99" | "101" => Layer::TopFab,
        _ => return None,
    })
}

fn fp_shape(geom: Geom, layer: Layer, width: f64, fill: Fill) -> FpShape {
    FpShape { id: uuid::Uuid::new_v4(), shape: Shape { geom, stroke: Stroke::width(mm(width)), fill }, layer }
}

/// The four corners of a rotated `w` × `h` box at `c` (mm).
fn corners(c: [f64; 2], w: f64, h: f64, deg: f64) -> [[f64; 2]; 4] {
    let (s, co) = deg.to_radians().sin_cos();
    [[-w, -h], [w, -h], [w, h], [-w, h]].map(|[x, y]| [c[0] + (x * co - y * s) / 2.0, c[1] + (x * s + y * co) / 2.0])
}

/// `PAD~shape~x~y~w~h~layer~net~number~holeR~points~rot~id~holeLength~holePoints~plated~…`
fn pad(f: &Frame, t: &[&str]) -> Option<Pad> {
    let n = |i: usize| num(field(t, i));
    let at = f.pt(n(2), n(3));
    let (w, h) = (n(4) * U, n(5) * U);
    let angle = (-n(11)).rem_euclid(360.0);
    let hole_r = n(9) * U;
    let hole_len = n(13) * U;
    let layer = field(t, 6).trim();
    let mut shape = match field(t, 1) {
        "ELLIPSE" if (w - h).abs() < 1e-6 => PadShape::Circle,
        "ELLIPSE" | "OVAL" => PadShape::Oval,
        "RECT" => PadShape::Rect,
        "POLYGON" => {
            let pts: Vec<Pt> = points(field(t, 10)).into_iter().map(|q| f.p(q) - at).collect();
            PadShape::Custom { anchor_rect: false, shapes: vec![Shape { geom: Geom::Polyline { pts, closed: true }, stroke: Stroke::width(0), fill: Fill::Outline }] }
        }
        _ => return None,
    };
    let mut size = Size::mm(w, h);
    let mut angle = angle;
    if let PadShape::Custom { .. } = shape {
        // A custom pad's points are already turned; its anchor is a small circle inside.
        angle = 0.0;
        let d = w.min(h).max(0.1) / 2.0;
        size = Size::mm(d, d);
    }
    if shape == PadShape::Circle {
        size = Size::mm(w, w);
    }
    let drill = (hole_r > 0.0).then(|| mm(2.0 * hole_r));
    let mut p = new_pad(field(t, 8).trim(), PadShape::Circle, at, size, drill);
    std::mem::swap(&mut p.shape, &mut shape);
    p.angle = angle;
    match (layer, drill) {
        (_, Some(_)) => {
            if hole_len > 0.0 {
                // A slot along the pad's long side.
                let (sw, sh) = if w >= h { (hole_len, 2.0 * hole_r) } else { (2.0 * hole_r, hole_len) };
                p.drill = Some(Drill { size: Size::mm(sw, sh), offset: Pt::ZERO });
            }
            if field(t, 15).trim() == "N" {
                p.kind = PadKind::NonPlated;
                p.layers = LayerSet::of(&[Layer::TopMask, Layer::BottomMask]);
            }
        }
        ("2", None) => p.layers = LayerSet::of(&[Layer::BottomCopper, Layer::BottomMask, Layer::BottomPaste]),
        _ => {}
    }
    Some(p)
}

/// The footprint, its 3D model reference, and the box of its body (for the courtyard).
fn footprint_shapes(f: &Frame, shapes: &[Value], fp: &mut Footprint, warnings: &mut Vec<String>) -> (Option<ModelRef>, Option<Pt>, Option<Pt>) {
    let mut model = None;
    let (mut ref_at, mut value_at) = (None, None);
    for v in shapes {
        let Some(str) = v.as_str() else { continue };
        if let Some(json) = str.strip_prefix("SVGNODE~") {
            if let Ok(node) = serde_json::from_str::<Value>(json) {
                model = model_ref(f, &node).or(model);
            }
            continue;
        }
        let t: Vec<&str> = str.split('~').collect();
        let n = |i: usize| num(field(&t, i));
        match t[0] {
            "PAD" => match pad(f, &t) {
                Some(p) => fp.pads.push(p),
                None => warnings.push(format!("pad {} has a shape cadrs doesn't read ({})", field(&t, 8), field(&t, 1))),
            },
            "HOLE" => {
                let d = 2.0 * n(3) * U;
                let mut p = new_pad("", PadShape::Circle, f.pt(n(1), n(2)), Size::mm(d, d), Some(mm(d)));
                p.kind = PadKind::NonPlated;
                p.layers = LayerSet::of(&[Layer::TopMask, Layer::BottomMask]);
                fp.pads.push(p);
            }
            "VIA" => {
                let d = n(3) * U;
                fp.pads.push(new_pad("", PadShape::Circle, f.pt(n(1), n(2)), Size::mm(d, d), Some(mm(2.0 * n(5) * U))));
            }
            "TRACK" => {
                let Some(layer) = fp_layer(field(&t, 2)) else { continue };
                let pts: Vec<Pt> = points(field(&t, 4)).into_iter().map(|q| f.p(q)).collect();
                let geom = if pts.len() == 2 { Geom::Line { a: pts[0], b: pts[1] } } else { Geom::Polyline { pts, closed: false } };
                fp.shapes.push(fp_shape(geom, layer, n(1) * U, Fill::None));
            }
            "CIRCLE" => {
                let Some(layer) = fp_layer(field(&t, 5)) else { continue };
                fp.shapes.push(fp_shape(Geom::Circle { center: f.pt(n(1), n(2)), radius: mm(n(3) * U) }, layer, n(4) * U, Fill::None));
            }
            "ARC" => {
                let Some(layer) = fp_layer(field(&t, 2)) else { continue };
                for sub in path::parse(field(&t, 4)) {
                    for g in sub_geoms(f, &sub) {
                        fp.shapes.push(fp_shape(g, layer, n(1) * U, Fill::None));
                    }
                }
            }
            "RECT" => {
                // RECT~x~y~w~h~layer~id~locked~strokeWidth
                let Some(layer) = fp_layer(field(&t, 5)) else { continue };
                let (x, y) = (n(1), n(2));
                let width = if t.len() > 8 { n(8) * U } else { 0.1 };
                fp.shapes.push(fp_shape(Geom::Rect { a: f.pt(x, y), b: f.pt(x + n(3), y + n(4)) }, layer, width, Fill::None));
            }
            "SOLIDREGION" => {
                // SOLIDREGION~layer~net~path~type: the body outline on 99, copper on 1/2.
                let Some(layer) = fp_layer(field(&t, 1)) else { continue };
                let fill = if layer.is_copper() { Fill::Outline } else { Fill::None };
                for sub in path::parse(field(&t, 3)) {
                    let pts: Vec<Pt> = sub.points(8).into_iter().map(|q| f.p(q)).collect();
                    if pts.len() >= 3 {
                        fp.shapes.push(fp_shape(Geom::Polyline { pts, closed: true }, layer, if fill == Fill::None { 0.1 } else { 0.0 }, fill));
                    }
                }
            }
            "TEXT" => {
                // TEXT~type~x~y~strokeWidth~rotation~mirror~layer~net~fontSize~text~path~display
                let at = f.pt(n(2), n(3));
                match field(&t, 1) {
                    "P" => ref_at = Some(at),
                    "N" => value_at = Some(at),
                    _ => {
                        let Some(layer) = fp_layer(field(&t, 7)) else { continue };
                        if field(&t, 12) != "none" && !field(&t, 10).trim().is_empty() {
                            let size = (n(9) * U).max(0.5);
                            let mut text = fp_text(field(&t, 10), at, layer, size, (n(4) * U).max(0.1));
                            text.text.angle = -n(5);
                            fp.texts.push(text);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    (model, ref_at, value_at)
}

/// The 3D outline node: the model's id, and the centre of its outline.
fn model_ref(f: &Frame, node: &Value) -> Option<ModelRef> {
    let a = &node["attrs"];
    if a["c_etype"].as_str() != Some("outline3D") {
        return None;
    }
    let uuid = a["uuid"].as_str()?.to_string();
    let s = |k: &str| a[k].as_str().unwrap_or("");
    let nums = |k: &str| s(k).split(',').map(num).collect::<Vec<_>>();
    let origin = nums("c_origin");
    // The outline's own points (the model seen from above), else its stated origin.
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    for c in node["childNodes"].as_array().into_iter().flatten() {
        let d = c["attrs"]["points"].as_str().or_else(|| c["attrs"]["d"].as_str()).unwrap_or("");
        for sub in path::parse(&if c["attrs"]["points"].is_string() { format!("M {d}") } else { d.to_string() }) {
            for q in sub.points(4) {
                for i in 0..2 {
                    lo[i] = lo[i].min(q[i]);
                    hi[i] = hi[i].max(q[i]);
                }
            }
        }
    }
    let center = if lo[0] <= hi[0] { [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0] } else { [*origin.first()?, *origin.get(1)?] };
    let rot = nums("c_rotation");
    Some(ModelRef {
        uuid,
        name: s("title").to_string(),
        center: f.xy(center[0], center[1]),
        z: (num(s("z")) * U * 1e4).round() / 1e4 + 0.0,
        rotation: [rot.first().copied().unwrap_or(0.0), rot.get(1).copied().unwrap_or(0.0), rot.get(2).copied().unwrap_or(0.0)],
    })
}

/// The box (mm) around a footprint's pads and body shapes.
fn footprint_box(fp: &Footprint) -> Option<([f64; 2], [f64; 2])> {
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    let mut add = |q: [f64; 2]| {
        for i in 0..2 {
            lo[i] = lo[i].min(q[i]);
            hi[i] = hi[i].max(q[i]);
        }
    };
    for p in &fp.pads {
        let c = [to_mm(p.at.x), to_mm(p.at.y)];
        match &p.shape {
            PadShape::Custom { shapes, .. } => shapes.iter().flat_map(|s| geom_points(&s.geom)).for_each(|q| add([c[0] + to_mm(q.x), c[1] + to_mm(q.y)])),
            _ => corners(c, to_mm(p.size.w), to_mm(p.size.h), p.angle).into_iter().for_each(&mut add),
        }
    }
    for s in fp.shapes.iter().filter(|s| matches!(s.layer, Layer::TopFab | Layer::TopSilk)) {
        geom_points(&s.shape.geom).into_iter().for_each(|q| add([to_mm(q.x), to_mm(q.y)]));
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

// ---------------------------------------------------------------------------------------------

/// Extra facts about the part from the catalogue search, when there was one.
#[derive(Clone, Debug, Default)]
pub struct PartInfo {
    pub description: String,
    pub datasheet: String,
    pub category: String,
}

/// Converts a part's EasyEDA data (the component API's `result`) into a symbol and footprint
/// for the library `lib`.
pub fn convert(result: &Value, lib: &str, info: &PartInfo) -> Result<Converted, String> {
    let mut warnings = vec![];
    let sym_data = &result["dataStr"];
    let head = &sym_data["head"];
    let para = &head["c_para"];
    let p = |k: &str| latin(para[k].as_str().unwrap_or(""));
    let lcsc = result["lcsc"]["number"].as_str().map(str::to_string).unwrap_or_else(|| p("Supplier Part"));
    let fp_data = &result["packageDetail"]["dataStr"];
    if !fp_data.is_object() {
        return Err(format!("{lcsc}: EasyEDA has no footprint for this part"));
    }
    let fp_para = &fp_data["head"]["c_para"];
    let package = fp_para["package"].as_str().or_else(|| result["packageDetail"]["title"].as_str()).unwrap_or("").trim().to_string();
    let package = if package.is_empty() { format!("{lcsc}_footprint") } else { package };
    let name = {
        let n = p("name");
        let n = if n.is_empty() { result["title"].as_str().unwrap_or(&lcsc).trim().to_string() } else { n };
        n.replace(['/', '\\', ':'], "_")
    };
    let prefix = {
        let s = p("pre").trim_end_matches('?').to_string();
        if s.is_empty() { "U".to_string() } else { s }
    };

    // The symbol.
    let description = if info.description.is_empty() { format!("{} {} ({})", p("Manufacturer"), p("Manufacturer Part"), package).trim().to_string() } else { info.description.clone() };
    let mut s = new_symbol(&name, &prefix, &description);
    let f = Frame::of(head);
    let (ref_at, value_at) = symbol_shapes(&f, sym_data["shape"].as_array().map(Vec::as_slice).unwrap_or_default(), &mut s, &mut warnings);
    if s.pins.is_empty() {
        warnings.push("the symbol has no pins".into());
    }
    let (lo, hi) = symbol_box(&s);
    let g = 1.27;
    s.fields[0].text.at = ref_at.unwrap_or_else(|| Pt::mm(snap(lo[0], g), snap(hi[1], g) + g));
    s.fields[1].text.at = value_at.unwrap_or_else(|| Pt::mm(snap(lo[0], g), snap(lo[1], g) - g));
    s.show_pin_names = s.pins.iter().any(|p| p.name != "~");
    s.keywords = [p("Manufacturer Part"), info.category.clone(), lcsc.clone()].iter().filter(|w| !w.is_empty()).cloned().collect::<Vec<_>>().join(" ");
    set_field(&mut s, fields::FOOTPRINT, &format!("{lib}:{package}"));
    let datasheet = if info.datasheet.is_empty() { format!("https://www.lcsc.com/product-detail/{lcsc}.html") } else { info.datasheet.clone() };
    set_field(&mut s, fields::DATASHEET, &datasheet);
    set_field(&mut s, field_names::LCSC, &lcsc);
    for (k, v) in [(field_names::MANUFACTURER, p("Manufacturer")), (field_names::MPN, p("Manufacturer Part")), (field_names::JLCPCB_CLASS, p("JLCPCB Part Class"))] {
        if !v.is_empty() {
            set_field(&mut s, k, &v);
        }
    }
    set_field(&mut s, field_names::SOURCE, crate::SOURCE);

    // The footprint.
    let ff = Frame::of(&fp_data["head"]);
    let mut fp = new_footprint(&format!("{lib}:{package}"), &format!("{package} (LCSC {lcsc}). From the {}.", crate::SOURCE), MountKind::Smd, Pt::ZERO, Pt::ZERO, Pt::ZERO);
    fp.keywords = [package.clone(), lcsc.clone()].join(" ");
    let (model, fp_ref, fp_value) = footprint_shapes(&ff, fp_data["shape"].as_array().map(Vec::as_slice).unwrap_or_default(), &mut fp, &mut warnings);
    if fp.pads.iter().any(|p| p.kind == PadKind::ThroughHole) {
        fp.attrs.mount = MountKind::ThroughHole;
    }
    if fp.pads.is_empty() {
        warnings.push("the footprint has no pads".into());
    }
    // A courtyard round the pads and body, 0.25 mm out (EasyEDA footprints have none).
    if let Some((lo, hi)) = footprint_box(&fp) {
        let r = |v: f64, up: bool| if up { (v * 100.0).ceil() / 100.0 } else { (v * 100.0).floor() / 100.0 };
        let (a, b) = (Pt::mm(r(lo[0] - 0.25, false), r(lo[1] - 0.25, false)), Pt::mm(r(hi[0] + 0.25, true), r(hi[1] + 0.25, true)));
        fp.shapes.push(fp_shape(Geom::Rect { a, b }, Layer::TopCourtyard, 0.05, Fill::None));
        let center = Pt::mm(((lo[0] + hi[0]) / 2.0 * 100.0).round() / 100.0, ((lo[1] + hi[1]) / 2.0 * 100.0).round() / 100.0);
        fp.fields[0].text.text.at = fp_ref.unwrap_or(Pt::new(center.x, b.y + mm(1.0)));
        fp.fields[1].text.text.at = fp_value.unwrap_or(Pt::new(center.x, a.y - mm(1.0)));
        fp.texts[0].text.at = center;
    }
    // Pads take their symbol pin's name and type (as the board's netlist would).
    for pad in &mut fp.pads {
        if let Some(pin) = s.pins.iter().find(|p| p.number == pad.number) {
            pad.pin_function = if pin.name == "~" { String::new() } else { pin.name.clone() };
        }
    }
    Ok(Converted { symbol: s, footprint: fp, model, warnings })
}

/// Rotates `v` as the 3D view will (Z · Y · X, degrees).
fn rotate(v: [f64; 3], deg: [f64; 3]) -> [f64; 3] {
    let [rx, ry, rz] = deg.map(f64::to_radians);
    let x = |v: [f64; 3]| [v[0], v[1] * rx.cos() - v[2] * rx.sin(), v[1] * rx.sin() + v[2] * rx.cos()];
    let y = |v: [f64; 3]| [v[0] * ry.cos() + v[2] * ry.sin(), v[1], -v[0] * ry.sin() + v[2] * ry.cos()];
    let z = |v: [f64; 3]| [v[0] * rz.cos() - v[1] * rz.sin(), v[0] * rz.sin() + v[1] * rz.cos(), v[2]];
    z(y(x(v)))
}

/// The extent (min, max) of an OBJ model's vertices (mm), as turned by `rotation`.
pub fn obj_extent(obj: &str, rotation: [f64; 3]) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::MAX; 3];
    let mut hi = [f64::MIN; 3];
    for line in obj.lines() {
        let Some(rest) = line.strip_prefix("v ") else { continue };
        let c: Vec<f64> = rest.split_whitespace().take(3).map(num).collect();
        if c.len() < 3 {
            continue;
        }
        let v = rotate([c[0], c[1], c[2]], rotation);
        for i in 0..3 {
            lo[i] = lo[i].min(v[i]);
            hi[i] = hi[i].max(v[i]);
        }
    }
    (lo[0] <= hi[0]).then_some((lo, hi))
}

/// The footprint's 3D model, file `source`, placed as EasyEDA places it: turned, its outline
/// centred where EasyEDA's outline is, its lowest point at `z`. Without the model's OBJ (for
/// its extent) it sits with its origin there. With it, it also carries a plain box body of
/// that extent, shown when the file can't be.
pub fn place_model(m: &ModelRef, source: &str, obj: Option<&str>) -> Model3d {
    let r3 = |v: f64| (v * 1000.0).round() / 1000.0;
    let (offset, body) = match obj.and_then(|o| Some((obj_extent(o, m.rotation)?, obj_extent(o, [0.0; 3])?))) {
        Some(((lo, hi), (own_lo, own_hi))) => {
            let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0];
            let mut body = cadrs_eda::model3d::Body::default();
            // In the model's own frame: the view turns and moves it with the file.
            body.cuboid(own_lo, own_hi, cadrs_eda::model3d::colors::PLASTIC);
            ([r3(m.center[0] - c[0]), r3(m.center[1] - c[1]), r3(m.z - lo[2])], Some(body))
        }
        None => ([m.center[0], m.center[1], m.z], None),
    };
    // The 3D view turns by minus these (KiCad's sense); EasyEDA's are the other way.
    let rotation = m.rotation.map(|a| if a == 0.0 { 0.0 } else { (-a).rem_euclid(360.0) });
    Model3d { offset, rotation, body, ..Model3d::file(source) }
}

/// A name without its bracketed parts in other scripts ("Ai-Thinker(安信可)" → "Ai-Thinker"):
/// cadrs' font has Latin letters only.
pub fn latin(s: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(open) = rest.find(['(', '（']) {
        let close = rest[open..].find([')', '）']).map(|c| open + c + rest[open + c..].chars().next().map_or(1, char::len_utf8));
        let Some(close) = close else { break };
        out += &rest[..open];
        if rest[open..close].is_ascii() {
            out += &rest[open..close];
        }
        rest = &rest[close..];
    }
    out += rest;
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn latin_names() {
        assert_eq!(super::latin("Ai-Thinker(安信可)"), "Ai-Thinker");
        assert_eq!(super::latin("UNI-ROYAL(Uniroyal Elec)"), "UNI-ROYAL(Uniroyal Elec)");
        assert_eq!(super::latin("TI（德州仪器） x"), "TI x");
    }
}

//! What the canvases draw (GS2–GS26 views): a schematic sheet or a board as coloured lines and
//! filled triangles in design coordinates (nm, Y up). The app uploads these as gizmo lines and
//! meshes; tests and exports can read them too. Colours come from a theme.

use crate::board::Board;
use crate::connectivity::netlist;
use crate::font;
use crate::footprint::{PadKind, PadShape};
use crate::graphics::{Fill, Geom, HAlign, Shape, Text, TextStyle, VAlign};
use crate::layer::{Layer, Side};
use crate::poly::{self, Region};
use crate::schematic::{LabelKind, Schematic};
use crate::symbol::{PinShape, SymbolItem};
use crate::units::{Bounds, Nm, Pt, Size, mm};

pub type Rgba = [u8; 4];

/// A polyline of a given width (0: the thinnest the canvas draws).
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub color: Rgba,
    pub width: Nm,
    pub pts: Vec<Pt>,
}

/// Filled triangles (corners in nm), drawn in `z` order.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    pub color: Rgba,
    pub z: i32,
    pub tris: Vec<[f64; 2]>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrawList {
    pub background: Rgba,
    pub lines: Vec<Line>,
    pub areas: Vec<Area>,
    /// What "fit" shows.
    pub bounds: Option<Bounds>,
}

impl DrawList {
    fn line(&mut self, color: Rgba, width: Nm, pts: Vec<Pt>) {
        if pts.len() >= 2 {
            self.lines.push(Line { color, width, pts });
        }
    }

    fn region(&mut self, color: Rgba, z: i32, r: &Region) {
        let tris = poly::triangulate(r);
        if !tris.is_empty() {
            self.areas.push(Area { color, z, tris });
        }
    }

    fn text(&mut self, t: &Text, color: Rgba) {
        if !t.visible || t.text.is_empty() {
            return;
        }
        let w = font::stroke_width(&t.style);
        for s in font::strokes(t) {
            if s.len() == 1 {
                self.line(color, w, vec![s[0], s[0] + Pt::new(1, 0)]);
            } else {
                self.line(color, w, s);
            }
        }
    }

    fn shape(&mut self, s: &Shape, stroke: Rgba, fill: Option<Rgba>, default_width: Nm, z: i32) {
        let (mut pts, closed) = poly::geom_points(&s.geom);
        if closed
            && let Some(f) = fill
        {
            let mut ring = pts.clone();
            if poly::ring_area(&ring) < 0.0 {
                ring.reverse();
            }
            self.region(f, z, &vec![ring]);
        }
        if closed && !pts.is_empty() {
            pts.push(pts[0]);
        }
        let w = if s.stroke.width > 0 { s.stroke.width } else { default_width };
        self.line(stroke, w, pts);
    }
}

// ---------------------------------------------------------------------------------------------
// Schematic

#[derive(Clone, Debug, PartialEq)]
pub struct SchematicTheme {
    pub background: Rgba,
    pub paper: Rgba,
    pub frame: Rgba,
    pub body: Rgba,
    pub body_fill: Rgba,
    pub pin: Rgba,
    pub pin_number: Rgba,
    pub pin_name: Rgba,
    pub field: Rgba,
    pub wire: Rgba,
    pub bus: Rgba,
    pub junction: Rgba,
    pub label: Rgba,
    pub no_connect: Rgba,
    pub note: Rgba,
    pub highlight: Rgba,
}

impl Default for SchematicTheme {
    fn default() -> Self {
        SchematicTheme {
            background: [0xe4, 0xe4, 0xe0, 0xff],
            paper: [0xf6, 0xf5, 0xf0, 0xff],
            frame: [0x84, 0x1a, 0x1a, 0xff],
            body: [0x84, 0x00, 0x00, 0xff],
            body_fill: [0xff, 0xfb, 0xd2, 0xff],
            pin: [0x84, 0x00, 0x00, 0xff],
            pin_number: [0xa9, 0x00, 0x00, 0xff],
            pin_name: [0x00, 0x64, 0x64, 0xff],
            field: [0x00, 0x64, 0x64, 0xff],
            wire: [0x00, 0x96, 0x00, 0xff],
            bus: [0x00, 0x00, 0x84, 0xff],
            junction: [0x00, 0x96, 0x00, 0xff],
            label: [0x15, 0x15, 0x15, 0xff],
            no_connect: [0x00, 0x00, 0xc8, 0xff],
            note: [0x00, 0x00, 0xc8, 0xff],
            highlight: [0xf9, 0x7a, 0x16, 0xff],
        }
    }
}

const WIRE_W: Nm = 152_400;
const BODY_W: Nm = 254_000;

fn text_at(s: &str, at: Pt, angle: f64, size: Nm, h: HAlign, v: VAlign) -> Text {
    Text { text: s.into(), at, angle, style: TextStyle { size: Size::new(size, size), h_align: h, v_align: v, ..Default::default() }, visible: true }
}

/// What is highlighted (selected, hovered) on a sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlight {
    pub items: Vec<crate::sch_edit::SchItem>,
}

/// A schematic sheet: paper, frame and title block, symbols with pins and fields, wires,
/// buses, junctions, no-connect flags, labels and notes; unconnected pin ends marked.
pub fn schematic(sch: &Schematic, sheet: usize, th: &SchematicTheme, hl: &Highlight) -> DrawList {
    use crate::sch_edit::SchItem;
    let mut d = DrawList { background: th.background, ..Default::default() };
    let Some(sh) = sch.sheets.get(sheet) else { return d };
    let (w, h) = (sh.paper.size.w, sh.paper.size.h);
    d.bounds = Some(Bounds { min: Pt::ZERO, max: Pt::new(w, h) });
    d.region(th.paper, -10, &vec![vec![Pt::ZERO, Pt::new(w, 0), Pt::new(w, h), Pt::new(0, h)]]);
    // Frame 10 mm in, title block bottom right.
    let m = mm(10.0);
    d.line(th.frame, WIRE_W, vec![Pt::new(m, m), Pt::new(w - m, m), Pt::new(w - m, h - m), Pt::new(m, h - m), Pt::new(m, m)]);
    let (tw, tbh) = (mm(110.0), mm(32.0));
    let (x0, y0) = (w - m - tw, m);
    d.line(th.frame, WIRE_W, vec![Pt::new(x0, y0), Pt::new(x0, y0 + tbh), Pt::new(w - m, y0 + tbh)]);
    for k in 1..4 {
        let y = y0 + mm(8.0) * k;
        d.line(th.frame, WIRE_W, vec![Pt::new(x0, y), Pt::new(w - m, y)]);
    }
    let tb = &sh.title_block;
    let small = mm(1.5);
    let rows = [
        format!("Title: {}", tb.title),
        format!("Size: {}   Date: {}   Rev: {}", sh.paper.name, tb.date, tb.revision),
        tb.company.clone(),
        format!("Sheet: /{}", if sh.name == "Root" { "" } else { &sh.name }),
    ];
    for (k, r) in rows.iter().enumerate() {
        let y = y0 + tbh - mm(8.0) * (k as i64 + 1) + mm(3.0);
        let size = if k == 0 { mm(2.5) } else { small };
        d.text(&text_at(r, Pt::new(x0 + mm(2.0), y), 0.0, size, HAlign::Left, VAlign::Bottom), th.frame);
    }

    let nl = netlist(sch);
    let lone: Vec<Pt> = nl.nets.iter().filter(|n| n.pins.len() == 1 && n.labels.is_empty() && !n.no_connect).map(|n| n.pins[0].at).collect();
    let lit = |it: &SchItem| hl.items.contains(it);

    for s in &sh.symbols {
        let Some(def) = sch.symbol(&s.symbol) else { continue };
        let sel = lit(&SchItem::Symbol(s.id));
        let (body, pin, fill_bg) = if sel { (th.highlight, th.highlight, th.body_fill) } else { (th.body, th.pin, th.body_fill) };
        for g in def.unit_graphics(s.unit, s.style) {
            match &g.item {
                SymbolItem::Shape(shape) => {
                    let geom = shape.geom.map(|p| s.placement.apply(p));
                    let fill = match shape.fill {
                        Fill::None => None,
                        Fill::Outline => Some(body),
                        Fill::Background => Some(fill_bg),
                        Fill::Color(c) => Some([c.r, c.g, c.b, c.a]),
                    };
                    d.shape(&Shape { geom, ..shape.clone() }, body, fill, BODY_W, 1);
                }
                SymbolItem::Text(t) => d.text(&s.placement.text(t), body),
            }
        }
        for p in def.unit_pins(s.unit, s.style) {
            if !p.visible && !def.power {
                continue;
            }
            let (a, b) = (s.placement.apply(p.at), s.placement.apply(p.inner_end()));
            if p.length > 0 {
                d.line(pin, WIRE_W, vec![a, b]);
            }
            let dir = s.placement.apply_angle(p.angle);
            if matches!(p.shape, PinShape::Inverted | PinShape::InvertedClock) {
                let r = mm(0.4);
                let c = b - Pt::new(r, 0).rotated(dir);
                d.shape(&Shape { geom: Geom::Circle { center: c, radius: r }, stroke: Default::default(), fill: Fill::None }, pin, None, WIRE_W, 1);
            }
            if lone.contains(&a) {
                d.shape(&Shape { geom: Geom::Circle { center: a, radius: mm(0.25) }, stroke: Default::default(), fill: Fill::None }, pin, None, 0, 1);
            }
            if p.length == 0 || !p.visible {
                continue;
            }
            // Number above the pin's middle; name inside the body past its inner end.
            if def.show_pin_numbers {
                let mid = Pt::new((a.x + b.x) / 2, (a.y + b.y) / 2);
                let up = Pt::new(0, mm(0.3)).rotated(dir);
                d.text(&text_at(&p.number, mid + up, dir, p.number_size, HAlign::Center, VAlign::Bottom), th.pin_number);
            }
            if def.show_pin_names && !p.name.is_empty() && p.name != "~" {
                let at = b + Pt::new(def.pin_name_offset.max(mm(0.5)), 0).rotated(dir);
                d.text(&text_at(&p.name, at, dir, p.name_size, HAlign::Left, VAlign::Center), th.pin_name);
            }
        }
        for f in &s.fields {
            let c = if sel || lit(&SchItem::Field(s.id, f.name.clone())) { th.highlight } else { th.field };
            let mut t = f.text.clone();
            if f.show_name {
                t.text = format!("{}: {}", f.name, t.text);
            }
            d.text(&t, c);
        }
    }
    for wire in &sh.wires {
        let c = if lit(&SchItem::Wire(wire.id)) { th.highlight } else { th.wire };
        d.line(c, if wire.stroke.width > 0 { wire.stroke.width } else { WIRE_W }, vec![wire.a, wire.b]);
    }
    for b in &sh.buses {
        d.line(th.bus, mm(0.3), vec![b.a, b.b]);
    }
    for j in &sh.junctions {
        let r = if j.diameter > 0 { j.diameter / 2 } else { mm(0.457) };
        let c = if lit(&SchItem::Junction(j.id)) { th.highlight } else { th.junction };
        d.region(c, 2, &poly::circle(j.at, r));
    }
    for n in &sh.no_connects {
        let k = mm(0.635);
        d.line(th.no_connect, WIRE_W, vec![n.at - Pt::new(k, k), n.at + Pt::new(k, k)]);
        d.line(th.no_connect, WIRE_W, vec![n.at - Pt::new(k, -k), n.at + Pt::new(k, -k)]);
    }
    for l in &sh.labels {
        let c = if lit(&SchItem::Label(l.id)) { th.highlight } else { th.label };
        let mut t = l.text.clone();
        if !matches!(l.kind, LabelKind::Local) {
            // A flag around the text: pointed at the connection end.
            let size = t.style.size.h;
            let len = font::line_width(&t.text, &t.style).round() as Nm + size;
            let (hh, tip) = (size * 3 / 4, size / 2);
            let dir = t.angle;
            let pts: Vec<Pt> = [Pt::new(0, 0), Pt::new(tip, hh), Pt::new(tip + len, hh), Pt::new(tip + len, -hh), Pt::new(tip, -hh), Pt::new(0, 0)].iter().map(|p| t.at + p.rotated(dir)).collect();
            d.line(c, WIRE_W, pts);
            t.at = t.at + Pt::new(tip + size / 2, 0).rotated(dir);
            t.style.v_align = VAlign::Center;
            t.style.h_align = HAlign::Left;
        }
        d.text(&t, c);
    }
    for n in &sh.notes {
        d.text(&n.text, th.note);
    }
    for g in &sh.drawings {
        d.shape(&g.shape, th.note, None, WIRE_W, 0);
    }
    d
}

// ---------------------------------------------------------------------------------------------
// Board

#[derive(Clone, Debug, PartialEq)]
pub struct BoardTheme {
    pub background: Rgba,
    pub board: Rgba,
    pub top_copper: Rgba,
    pub bottom_copper: Rgba,
    pub inner_copper: Rgba,
    pub top_silk: Rgba,
    pub bottom_silk: Rgba,
    pub top_courtyard: Rgba,
    pub bottom_courtyard: Rgba,
    pub top_fab: Rgba,
    pub bottom_fab: Rgba,
    pub outline: Rgba,
    pub via: Rgba,
    pub hole: Rgba,
    pub ratsnest: Rgba,
    pub pad_text: Rgba,
    pub highlight: Rgba,
}

impl Default for BoardTheme {
    fn default() -> Self {
        BoardTheme {
            background: [0x00, 0x10, 0x23, 0xff],
            board: [0x1c, 0x2a, 0x3b, 0xff],
            top_copper: [0xc8, 0x34, 0x34, 0xe0],
            bottom_copper: [0x4d, 0x7f, 0xc4, 0xe0],
            inner_copper: [0x7f, 0xc8, 0x7f, 0xe0],
            top_silk: [0xf2, 0xed, 0xa1, 0xff],
            bottom_silk: [0xe8, 0xb2, 0xa7, 0xff],
            top_courtyard: [0xff, 0x26, 0xe2, 0xff],
            bottom_courtyard: [0x26, 0xe9, 0xff, 0xff],
            top_fab: [0xaf, 0xaf, 0xaf, 0xff],
            bottom_fab: [0x58, 0x5d, 0x84, 0xff],
            outline: [0xd0, 0xd2, 0xcd, 0xff],
            via: [0xb5, 0xb5, 0xb5, 0xff],
            hole: [0x10, 0x16, 0x20, 0xff],
            ratsnest: [0x00, 0xf8, 0xff, 0xff],
            pad_text: [0xff, 0xff, 0xff, 0xd0],
            highlight: [0xf9, 0x7a, 0x16, 0xff],
        }
    }
}

impl BoardTheme {
    pub fn layer(&self, l: Layer) -> Rgba {
        match l {
            Layer::TopCopper => self.top_copper,
            Layer::BottomCopper => self.bottom_copper,
            Layer::Inner(_) => self.inner_copper,
            Layer::TopSilk => self.top_silk,
            Layer::BottomSilk => self.bottom_silk,
            Layer::TopCourtyard => self.top_courtyard,
            Layer::BottomCourtyard => self.bottom_courtyard,
            Layer::TopFab => self.top_fab,
            Layer::BottomFab => self.bottom_fab,
            Layer::Outline => self.outline,
            _ => [0x80, 0x80, 0x80, 0xff],
        }
    }
}

/// What the layout view shows and highlights.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardView {
    /// Layers drawn (others hidden).
    pub visible: Vec<Layer>,
    /// Drawn above the other copper.
    pub active: Layer,
    /// Footprints, tracks and vias shown selected.
    pub selected: Vec<uuid::Uuid>,
    pub ratsnest: bool,
}

impl BoardView {
    pub fn all(board: &Board) -> BoardView {
        let mut visible: Vec<Layer> = board.copper().collect();
        visible.extend([Layer::TopSilk, Layer::BottomSilk, Layer::TopCourtyard, Layer::BottomCourtyard, Layer::TopFab, Layer::BottomFab, Layer::Outline, Layer::Drawings, Layer::Comments]);
        BoardView { visible, active: Layer::TopCopper, selected: vec![], ratsnest: true }
    }
}

fn draw_order(board: &Board, active: Layer) -> Vec<Layer> {
    let mut v = vec![Layer::BottomFab, Layer::BottomCourtyard, Layer::BottomSilk];
    let mut copper: Vec<Layer> = board.copper().collect();
    copper.reverse();
    copper.retain(|l| *l != active);
    v.extend(copper);
    if active.is_copper() {
        v.push(active);
    }
    v.extend([Layer::TopSilk, Layer::TopCourtyard, Layer::TopFab, Layer::Outline, Layer::Drawings, Layer::Comments]);
    v
}

/// A board seen from the top: the board's area, each layer's copper (pads, tracks, vias,
/// zone fills), holes, silkscreen, courtyards, fabrication drawing and outline, pad numbers,
/// and the ratsnest.
pub fn board(b: &Board, th: &BoardTheme, view: &BoardView) -> DrawList {
    let mut d = DrawList { background: th.background, ..Default::default() };
    let region = crate::outline::board_region(b);
    d.region(th.board, -10, &region);
    let mut bounds: Option<Bounds> = region.iter().flatten().fold(None, |acc, p| Some(Bounds::union(acc, Bounds::of(*p))));
    let items = crate::copper::items(b);
    let shown = |l: Layer| view.visible.contains(&l);
    for (z, layer) in draw_order(b, view.active).into_iter().enumerate() {
        let z = z as i32;
        if !shown(layer) {
            continue;
        }
        let color = th.layer(layer);
        if layer.is_copper() {
            let parts: Vec<Region> = items.iter().filter_map(|c| c.on(layer).cloned()).collect();
            let all = poly::union_all(&parts);
            d.region(color, z, &all);
            // Selected copper on top, in the highlight colour.
            let sel: Vec<Region> = items
                .iter()
                .filter(|c| match &c.item {
                    crate::copper::Item::Pad(f, _) => view.selected.contains(f),
                    crate::copper::Item::Track(t) | crate::copper::Item::Via(t) => view.selected.contains(t),
                    _ => false,
                })
                .filter_map(|c| c.on(layer).cloned())
                .collect();
            if !sel.is_empty() {
                d.region(th.highlight, z, &poly::union_all(&sel));
            }
        }
        for f in &b.footprints {
            let lit = view.selected.contains(&f.id);
            for s in f.footprint.shapes.iter().filter(|s| f.placement.layer(s.layer) == layer) {
                let geom = if f.placement.angle % 90.0 == 0.0 { s.shape.geom.clone() } else { s.shape.geom.rect_as_polyline() };
                let shape = Shape { geom: geom.map(|p| f.placement.apply(p)), ..s.shape.clone() };
                d.shape(&shape, if lit { th.highlight } else { color }, None, mm(0.1), z);
            }
            let texts = f.footprint.fields.iter().map(|x| &x.text).chain(f.footprint.texts.iter());
            for t in texts.filter(|t| f.placement.layer(t.layer) == layer) {
                let mut tt = t.text.clone();
                tt.text = tt.text.replace("${REFERENCE}", f.reference());
                tt.at = f.placement.apply(t.text.at);
                tt.angle = if t.keep_upright { crate::units::normalize_deg(t.text.angle + f.placement.angle) } else { f.placement.apply_angle(t.text.angle) };
                if f.placement.side == Side::Bottom {
                    tt.style.mirrored = !tt.style.mirrored;
                }
                d.text(&tt, if lit { th.highlight } else { color });
            }
        }
        for s in b.shapes.iter().filter(|s| s.layer == layer) {
            d.shape(&s.shape, color, None, mm(0.1), z);
            for p in poly::geom_points(&s.shape.geom).0 {
                bounds = Some(Bounds::union(bounds, Bounds::of(p)));
            }
        }
        for t in b.texts.iter().filter(|t| t.layer == layer) {
            d.text(&t.text, color);
        }
    }
    // Vias and holes over the copper.
    let top = 100;
    for v in &b.vias {
        let c = if view.selected.contains(&v.id) { th.highlight } else { th.via };
        d.region(c, top, &poly::circle(v.at, v.diameter / 2));
        d.region(th.hole, top + 1, &poly::circle(v.at, v.drill / 2));
    }
    for f in &b.footprints {
        for p in &f.footprint.pads {
            if let Some(dr) = p.drill {
                let c = f.placement.apply(p.at + dr.offset.rotated(p.angle));
                d.region(th.hole, top + 1, &poly::hole(c, dr.size, f.placement.apply_angle(p.angle)));
            }
            // Pad numbers, small, in the pad.
            let on_shown = f.placement.layers(p.layers).iter().any(|l| l.is_copper() && shown(l));
            if on_shown && !p.number.is_empty() && !matches!(p.shape, PadShape::Custom { .. }) && p.kind != PadKind::NonPlated {
                let s = (p.size.w.min(p.size.h) * 2 / 5).min(mm(1.0));
                let mut t = text_at(&p.number, f.placement.apply(p.at), 0.0, s, HAlign::Center, VAlign::Center);
                t.style.thickness = Some(s / 8);
                d.text(&t, th.pad_text);
            }
            bounds = Some(Bounds::union(bounds, Bounds::of(f.placement.apply(p.at))));
        }
    }
    if view.ratsnest {
        for a in crate::copper::ratsnest(b) {
            d.line(th.ratsnest, 0, vec![a.a, a.b]);
        }
    }
    d.bounds = bounds.map(|b| b.grow(mm(2.0)));
    d
}

/// The draw list as an SVG document (Y up becomes SVG's Y down), millimetre units.
pub fn to_svg(d: &DrawList) -> String {
    use std::fmt::Write;
    let b = d.bounds.unwrap_or(Bounds { min: Pt::ZERO, max: Pt::mm(100.0, 100.0) });
    let f = |v: Nm| v as f64 / 1e6;
    let (x0, y1, w, h) = (f(b.min.x), f(b.max.y), f(b.size().w), f(b.size().h));
    let col = |c: Rgba| format!("rgba({},{},{},{:.3})", c[0], c[1], c[2], c[3] as f64 / 255.0);
    let mut s = String::new();
    let _ = writeln!(s, r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="{x0} {} {w} {h}">"#, -y1);
    let _ = writeln!(s, r#"<rect x="{x0}" y="{}" width="{w}" height="{h}" fill="{}"/>"#, -y1, col(d.background));
    let mut areas: Vec<&Area> = d.areas.iter().collect();
    areas.sort_by_key(|a| a.z);
    for a in areas {
        let _ = write!(s, r#"<path fill="{}" d=""#, col(a.color));
        for t in a.tris.chunks(3) {
            let _ = write!(s, "M{} {}L{} {}L{} {}Z", t[0][0] / 1e6, -t[0][1] / 1e6, t[1][0] / 1e6, -t[1][1] / 1e6, t[2][0] / 1e6, -t[2][1] / 1e6);
        }
        let _ = writeln!(s, r#""/>"#);
    }
    for l in &d.lines {
        let pts: Vec<String> = l.pts.iter().map(|p| format!("{},{}", f(p.x), -f(p.y))).collect();
        let _ = writeln!(s, r#"<polyline fill="none" stroke="{}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round" points="{}"/>"#, col(l.color), f(l.width.max(50_000)), pts.join(" "));
    }
    s.push_str("</svg>\n");
    s
}

/// Faint cross-hairs through the origin, for the part editors.
/// An editor view ([`symbol_view`], [`footprint_view`]) as a thumbnail: without the origin
/// axes, its box just around what is drawn (`margin` out).
pub fn tight(mut d: DrawList, margin: Nm) -> DrawList {
    let axis = |l: &Line| l.pts.len() == 2 && l.width == 0 && (l.pts[0].x == -l.pts[1].x && l.pts[0].y == 0 && l.pts[1].y == 0 || l.pts[0].y == -l.pts[1].y && l.pts[0].x == 0 && l.pts[1].x == 0) && l.pts[0] != l.pts[1];
    d.lines.retain(|l| !axis(l));
    let mut b: Option<Bounds> = None;
    for l in &d.lines {
        for p in &l.pts {
            b = Some(Bounds::union(b, Bounds::of(*p).grow(l.width / 2)));
        }
    }
    for a in &d.areas {
        for t in &a.tris {
            b = Some(Bounds::union(b, Bounds::of(Pt::new(t[0] as Nm, t[1] as Nm))));
        }
    }
    d.bounds = b.map(|b| b.grow(margin));
    d
}

fn origin_axes(d: &mut DrawList, color: Rgba, reach: Nm) {
    d.line(color, 0, vec![Pt::new(-reach, 0), Pt::new(reach, 0)]);
    d.line(color, 0, vec![Pt::new(0, -reach), Pt::new(0, reach)]);
}

/// The symbol editor's view: the symbol at the origin with its pins (open circles at their
/// ends), names, numbers and fields; the selected parts highlighted.
pub fn symbol_view(sym: Option<&crate::symbol::Symbol>, th: &SchematicTheme, selected: &[crate::lib_edit::SymbolPart]) -> DrawList {
    use crate::lib_edit::SymbolPart;
    use crate::schematic::{Paper, Placement, PlacedSymbol, Sheet};
    let mut d = DrawList { background: th.paper, ..Default::default() };
    origin_axes(&mut d, [0x60, 0x60, 0xd0, 0x90], mm(50.0));
    let Some(sym) = sym else {
        d.bounds = Some(Bounds { min: Pt::mm(-20.0, -15.0), max: Pt::mm(20.0, 15.0) });
        return d;
    };
    // The symbol placed at the origin on a sheet with no frame.
    let mut s = sym.clone();
    let hl = |p: SymbolPart| selected.contains(&p);
    let placed = PlacedSymbol {
        id: uuid::Uuid::nil(),
        symbol: s.id.clone(),
        placement: Placement::default(),
        unit: 1,
        style: 1,
        fields: s.fields.clone(),
        in_bom: true,
        on_board: true,
        dnp: false,
        exclude_from_sim: false,
        pin_ids: vec![],
    };
    // Selected pins and graphics are drawn again, highlighted, on top.
    let mut sel = s.clone();
    sel.pins = s.pins.iter().enumerate().filter(|(i, _)| hl(SymbolPart::Pin(*i))).map(|(_, p)| p.clone()).collect();
    sel.graphics = s.graphics.iter().enumerate().filter(|(i, _)| hl(SymbolPart::Graphic(*i))).map(|(_, g)| g.clone()).collect();
    s.power = false;
    let sheet = Sheet { paper: Paper { name: "User".into(), size: Size::mm(1.0, 1.0) }, symbols: vec![placed], ..Default::default() };
    let sch = Schematic { symbols: vec![s.clone()], sheets: vec![sheet] };
    let mut body = schematic(&sch, 0, th, &Highlight::default());
    // Drop the sheet's paper and frame (everything drawn before the symbol).
    body.areas.retain(|a| a.z != -10);
    body.lines.retain(|l| l.color != th.frame);
    d.lines.extend(body.lines);
    d.areas.extend(body.areas);
    if !sel.pins.is_empty() || !sel.graphics.is_empty() {
        sel.fields.clear();
        let mut lit = th.clone();
        (lit.body, lit.pin, lit.pin_number, lit.pin_name) = (th.highlight, th.highlight, th.highlight, th.highlight);
        let sheet = Sheet { paper: Paper { name: "User".into(), size: Size::mm(1.0, 1.0) }, symbols: vec![PlacedSymbol { fields: vec![], ..sch.sheets[0].symbols[0].clone() }], ..Default::default() };
        let mut over = schematic(&Schematic { symbols: vec![sel], sheets: vec![sheet] }, 0, &lit, &Highlight::default());
        over.areas.retain(|a| a.z != -10);
        over.lines.retain(|l| l.color != lit.frame);
        d.lines.extend(over.lines);
        d.areas.extend(over.areas.into_iter().map(|mut a| {
            a.z += 1;
            a
        }));
    }
    for (i, f) in sym.fields.iter().enumerate() {
        if hl(SymbolPart::Field(i))
            && let Some(b) = crate::font::bounds(&f.text)
        {
            d.line(th.highlight, 0, vec![b.min, Pt::new(b.max.x, b.min.y), b.max, Pt::new(b.min.x, b.max.y), b.min]);
        }
    }
    let mut b: Option<Bounds> = Some(Bounds { min: Pt::mm(-10.0, -10.0), max: Pt::mm(10.0, 10.0) });
    for l in &d.lines {
        if l.pts.iter().any(|p| p.x.abs() >= mm(50.0) || p.y.abs() >= mm(50.0)) {
            continue;
        }
        for p in &l.pts {
            b = Some(Bounds::union(b, Bounds::of(*p)));
        }
    }
    d.bounds = b.map(|b| b.grow(mm(5.0)));
    d
}

/// The footprint editor's view: the footprint at the origin, top side up; the selected pads
/// and shapes highlighted.
pub fn footprint_view(fp: Option<&crate::footprint::Footprint>, th: &BoardTheme, selected: &[uuid::Uuid]) -> DrawList {
    use crate::board::PlacedFootprint;
    let mut board = Board::default();
    let mut d = DrawList { background: th.background, ..Default::default() };
    if let Some(fp) = fp {
        board.footprints.push(PlacedFootprint { id: uuid::Uuid::nil(), footprint: fp.clone(), placement: Default::default(), locked: false, symbol: None });
        let mut view = BoardView::all(&board);
        view.ratsnest = false;
        d = self::board(&board, th, &view);
        // Selected pads and shapes, outlined.
        for p in fp.pads.iter().filter(|p| selected.contains(&p.id)) {
            let r = poly::pad_local(p, mm(0.05));
            for ring in r {
                let mut pts = ring.clone();
                pts.push(ring[0]);
                d.line(th.highlight, mm(0.05), pts);
            }
        }
        for s in fp.shapes.iter().filter(|s| selected.contains(&s.id)) {
            d.shape(&s.shape, th.highlight, None, mm(0.1), 50);
        }
    }
    origin_axes(&mut d, [0x80, 0x80, 0x90, 0xa0], mm(50.0));
    let mut b: Option<Bounds> = Some(Bounds { min: Pt::mm(-5.0, -5.0), max: Pt::mm(5.0, 5.0) });
    for a in &d.areas {
        for t in &a.tris {
            b = Some(Bounds::union(b, Bounds::of(Pt::new(t[0] as Nm, t[1] as Nm))));
        }
    }
    for l in &d.lines {
        if l.pts.iter().any(|p| p.x.abs() >= mm(50.0) || p.y.abs() >= mm(50.0)) {
            continue;
        }
        for p in &l.pts {
            b = Some(Bounds::union(b, Bounds::of(*p)));
        }
    }
    d.bounds = b.map(|b| b.grow(mm(2.0)));
    d
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::LibraryTable;

    #[test]
    fn course_views_draw() {
        let lib = LibraryTable::builtin();
        let d = crate::getting_started::gs18(&lib);
        let s = schematic(&d.schematic, 0, &SchematicTheme::default(), &Highlight::default());
        assert!(s.lines.len() > 50);
        // Wires in the wire colour, junction dots filled.
        assert!(s.lines.iter().filter(|l| l.color == SchematicTheme::default().wire).count() >= 8);
        assert!(s.areas.iter().any(|a| a.color == SchematicTheme::default().junction));
        let b = board(&d.board, &BoardTheme::default(), &BoardView::all(&d.board));
        let th = BoardTheme::default();
        assert!(b.areas.iter().any(|a| a.color == th.top_copper) && b.areas.iter().any(|a| a.color == th.bottom_copper));
        assert!(b.lines.iter().any(|l| l.color == th.top_silk));
        let bb = b.bounds.unwrap();
        assert!(bb.contains(Pt::mm(45.0, 50.0)) && bb.contains(Pt::ZERO));
        // Nothing left to route: no ratsnest lines.
        assert!(b.lines.iter().all(|l| l.color != th.ratsnest));
    }
}

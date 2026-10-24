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
    /// The box around every line and area.
    pub fn drawn_bounds(&self) -> Option<Bounds> {
        let pts = self.lines.iter().flat_map(|l| l.pts.iter().copied());
        let tris = self
            .areas
            .iter()
            .flat_map(|a| a.tris.iter().map(|t| Pt::new(t[0] as Nm, t[1] as Nm)));
        pts.chain(tris)
            .fold(None, |acc, p| Some(Bounds::union(acc, Bounds::of(p))))
    }

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
        if closed && let Some(f) = fill {
            let mut ring = pts.clone();
            if poly::ring_area(&ring) < 0.0 {
                ring.reverse();
            }
            self.region(f, z, &vec![ring]);
        }
        if closed && !pts.is_empty() {
            pts.push(pts[0]);
        }
        let w = if s.stroke.width > 0 {
            s.stroke.width
        } else {
            default_width
        };
        self.line(stroke, w, pts);
    }
}

/// The colours of a made board, for [`board_surface`].
pub mod made {
    use super::Rgba;
    /// Green solder mask over bare laminate.
    pub const MASK: Rgba = [0x12, 0x2e, 0x19, 0xff];
    /// The mask over copper: a little lighter.
    pub const MASK_OVER_COPPER: Rgba = [0x21, 0x48, 0x28, 0xff];
    /// Bare copper finished in gold (ENIG) where the mask is open.
    pub const GOLD: Rgba = [0xb8, 0x9c, 0x14, 0xff];
    pub const SILK: Rgba = [0xf4, 0xf4, 0xf0, 0xff];
    /// Soldered pads (where paste goes).
    pub const SOLDER: Rgba = [0xb8, 0xb8, 0xb4, 0xff];
    pub const HOLE: Rgba = [0x14, 0x14, 0x14, 0xff];
}

/// One face of the board as it looks made (the 3D view lays it over the board): solder mask,
/// lighter where copper runs under it; pads and other openings in the mask bare gold; the
/// silkscreen on top; the holes. In board coordinates, whichever side.
pub fn board_surface(b: &Board, side: Side) -> DrawList {
    let (cu, mask, paste, silk) = match side {
        Side::Top => (
            Layer::TopCopper,
            Layer::TopMask,
            Layer::TopPaste,
            Layer::TopSilk,
        ),
        Side::Bottom => (
            Layer::BottomCopper,
            Layer::BottomMask,
            Layer::BottomPaste,
            Layer::BottomSilk,
        ),
    };
    // The silkscreen and the holes, as the layout draws them, in the made colours.
    let th = BoardTheme {
        background: made::MASK,
        board: made::MASK,
        top_silk: made::SILK,
        bottom_silk: made::SILK,
        via: made::MASK_OVER_COPPER,
        hole: made::HOLE,
        highlight: made::SILK,
        ..BoardTheme::default()
    };
    let view = BoardView {
        visible: vec![silk],
        active: cu,
        selected: vec![],
        hovered: None,
        hot_corner: None,
        handle: mm(0.25),
        ratsnest: false,
        dim_inactive: false,
        highlight_net: None,
    };
    let mut d = board(b, &th, &view);
    let items = crate::copper::items(b);
    let pad_of = |c: &crate::copper::Copper| match c.item {
        crate::copper::Item::Pad(fid, i) => {
            b.footprints.iter().find(|f| f.id == fid).and_then(|f| {
                f.footprint
                    .pads
                    .get(i)
                    .map(|p| f.placement.layers(p.layers))
            })
        }
        _ => None,
    };
    let pads_open = |c: &crate::copper::Copper| pad_of(c).is_some_and(|l| l.contains(mask));
    // Pads that get solder paste (SMD) look silver, as soldered; the rest bare gold.
    let pasted = |c: &crate::copper::Copper| pad_of(c).is_some_and(|l| l.contains(paste));
    let under: Vec<Region> = items
        .iter()
        .filter(|c| !pads_open(c))
        .filter_map(|c| c.on(cu).cloned())
        .collect();
    d.region(made::MASK_OVER_COPPER, -9, &poly::union_all(&under));
    let open: Vec<Region> = items
        .iter()
        .filter(|c| pads_open(c) && !pasted(c))
        .filter_map(|c| c.on(cu).cloned())
        .collect();
    d.region(made::GOLD, -8, &poly::union_all(&open));
    let soldered: Vec<Region> = items
        .iter()
        .filter(|c| pads_open(c) && pasted(c))
        .filter_map(|c| c.on(cu).cloned())
        .collect();
    d.region(made::SOLDER, -8, &poly::union_all(&soldered));
    // Copper drawings on the mask layer itself (openings drawn by hand) are bare too.
    for s in b.shapes.iter().filter(|s| s.layer == mask) {
        let (pts, closed) = poly::geom_points(&s.shape.geom);
        if closed && pts.len() >= 3 {
            d.region(made::GOLD, -8, &vec![pts]);
        }
    }
    d
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
    /// The marks on pin ends where wires join ([`PinEnds`]).
    pub pin_end: Rgba,
    /// The grid's dots, or (Horizon) its crosses on every fifth line.
    pub grid: Rgba,
    /// The grid's faint lines (Horizon; none when transparent).
    pub grid_line: Rgba,
    /// The placing crosshair.
    pub cursor: Rgba,
    /// Drawn as Horizon EDA draws schematics: every line but a bus's a hairline (the canvas's
    /// thinnest, whatever the zoom), small pin numbers by the body, every label in a flag
    /// pointing at its anchor. Off, as KiCad: lines as wide as the design says.
    pub horizon: bool,
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
            pin_end: [0x1e, 0x6f, 0xd9, 0xff],
            grid: [0x73, 0x73, 0x73, 0xb3],
            grid_line: [0, 0, 0, 0],
            cursor: [0x1a, 0x1a, 0x1a, 0xff],
            horizon: false,
        }
    }
}

impl SchematicTheme {
    /// The dark scheme, after Horizon EDA's "Nostalgia" colours: a warm near-black ground with
    /// no paper, gold symbols with salmon pin names and numbers, green nets (wires, junctions,
    /// labels), a faint grid, and Horizon's drawing ([`Self::horizon`]).
    pub fn dark() -> Self {
        let ground = [0x21, 0x20, 0x1d, 0xff];
        let symbol = [0xc9, 0x9c, 0x49, 0xff];
        let pin_text = [0xe6, 0x7b, 0x54, 0xff];
        let net = [0x73, 0xbc, 0x74, 0xff];
        SchematicTheme {
            background: ground,
            paper: ground,
            frame: [0x2e, 0x6a, 0x50, 0xff],
            body: symbol,
            // Bodies aren't filled: the ground shows through.
            body_fill: ground,
            pin: symbol,
            pin_number: pin_text,
            pin_name: pin_text,
            field: symbol,
            wire: net,
            bus: [0xff, 0x66, 0x00, 0xff],
            junction: net,
            label: net,
            no_connect: [0xe6, 0x7b, 0x54, 0xff],
            note: [0xa8, 0xa4, 0x98, 0xff],
            highlight: [0xd1, 0x3e, 0x47, 0xff],
            pin_end: [0x50, 0xbc, 0x8c, 0xff],
            grid: [0x8a, 0x80, 0x66, 0x48],
            grid_line: [0x5a, 0x52, 0x3e, 0x18],
            cursor: [0xdd, 0x41, 0x1a, 0xff],
            horizon: true,
        }
    }
}

/// The way a label's flag points from its anchor: the way its text runs (along its angle, or
/// half a turn around when it lies the other side: a KiCad label justified right), unless a
/// wire or pin in `taken` leaves the anchor that way and the other way is free; then the other
/// way, so the flag doesn't lie over what the label joins.
fn label_direction(t: &Text, taken: &[(Pt, Pt)]) -> f64 {
    let a = t.angle;
    let runs = font::bounds(t).map_or(a, |b| {
        let c = Pt::new((b.min.x + b.max.x) / 2, (b.min.y + b.max.y) / 2) - t.at;
        let (s, co) = a.to_radians().sin_cos();
        if c.x as f64 * co + c.y as f64 * s < 0.0 {
            a + 180.0
        } else {
            a
        }
    });
    // Whether a segment leaves the anchor at `deg` (one through it leaves both ways).
    let used = |deg: f64| {
        let (s, c) = deg.to_radians().sin_cos();
        taken.iter().any(|(p, q)| {
            [(*p, *q), (*q, *p)].iter().any(|(from, to)| {
                if !crate::connectivity::on_segment(t.at, *from, *to) || t.at == *to {
                    return false;
                }
                let (dx, dy) = ((to.x - t.at.x) as f64, (to.y - t.at.y) as f64);
                let len = dx.hypot(dy);
                len > 0.0 && (dx * c + dy * s) / len > 0.999
            })
        })
    };
    if used(runs) && !used(runs + 180.0) {
        runs + 180.0
    } else {
        runs
    }
}

/// A Horizon flag's box in the label's frame (`t.at` the origin, `dir` along +x): from `x0` to
/// `x1` along, `hh` either side.
fn flag_box(t: &Text) -> (Nm, Nm, Nm) {
    let size = t.style.size.h;
    let len = font::line_width(&t.text, &t.style).round() as Nm;
    let pad = size / 4;
    (size - pad, size + len + pad, size / 2 + pad)
}

/// The pieces of the segment `a`–`b` outside the flags' boxes (a wire under a label's flag
/// gives way to its text).
fn outside_flags(a: Pt, b: Pt, flags: &[(Pt, f64, (Nm, Nm, Nm))]) -> Vec<(Pt, Pt)> {
    // The parts of the segment (as 0..1) each box covers.
    let mut cut: Vec<(f64, f64)> = Vec::new();
    for (at, dir, (x0, x1, hh)) in flags {
        let (s, c) = dir.to_radians().sin_cos();
        let local = |p: Pt| {
            let (x, y) = ((p.x - at.x) as f64, (p.y - at.y) as f64);
            (x * c + y * s, -x * s + y * c)
        };
        let (p, q) = (local(a), local(b));
        // Liang–Barsky against the box.
        let (dx, dy) = (q.0 - p.0, q.1 - p.1);
        let (mut t0, mut t1) = (0.0f64, 1.0f64);
        let mut inside = true;
        for (pp, qq) in [
            (-dx, p.0 - *x0 as f64),
            (dx, *x1 as f64 - p.0),
            (-dy, p.1 + *hh as f64),
            (dy, *hh as f64 - p.1),
        ] {
            if pp == 0.0 {
                if qq < 0.0 {
                    inside = false;
                }
            } else {
                let r = qq / pp;
                if pp < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
            }
        }
        if inside && t0 < t1 {
            cut.push((t0, t1));
        }
    }
    if cut.is_empty() {
        return vec![(a, b)];
    }
    cut.sort_by(|x, y| x.0.total_cmp(&y.0));
    let at = |t: f64| {
        Pt::new(
            a.x + ((b.x - a.x) as f64 * t).round() as Nm,
            a.y + ((b.y - a.y) as f64 * t).round() as Nm,
        )
    };
    let mut out = Vec::new();
    let mut from = 0.0;
    for (t0, t1) in cut {
        if t0 > from {
            out.push((at(from), at(t0)));
        }
        from = from.max(t1);
    }
    if from < 1.0 {
        out.push((at(from), b));
    }
    out
}

/// Horizon EDA's net label flag: the text one text height out from `t.at` along `dir`, in a box
/// a quarter of its height bigger all round, whose near corners run to `t.at` in a point.
fn horizon_flag(d: &mut DrawList, t: &Text, dir: f64, c: Rgba) {
    let size = t.style.size.h;
    let (x0, x1, hh) = flag_box(t);
    let pts: Vec<Pt> = [
        Pt::new(0, 0),
        Pt::new(x0, hh),
        Pt::new(x1, hh),
        Pt::new(x1, -hh),
        Pt::new(x0, -hh),
        Pt::new(0, 0),
    ]
    .iter()
    .map(|p| t.at + p.rotated(dir))
    .collect();
    d.line(c, 0, pts);
    let mut text = t.clone();
    text.at = t.at + Pt::new(size, 0).rotated(dir);
    text.angle = dir;
    text.style.h_align = HAlign::Left;
    text.style.v_align = VAlign::Center;
    d.text(&text, c);
}

/// How far a wire's own colour turns the net colour's hue toward its own (Horizon theme): most
/// of the way, so nets read apart (purple, pink, blue, yellow) at the net colour's lightness.
const TINT: f64 = 0.9;

/// `c` as hue (degrees), saturation and lightness (0–1).
fn to_hsl(c: Rgba) -> (f64, f64, f64) {
    let [r, g, b] = [c[0], c[1], c[2]].map(|v| v as f64 / 255.0);
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.0;
    if max == min {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 {
        d / (2.0 - max - min)
    } else {
        d / (max + min)
    };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn from_hsl(h: f64, s: f64, l: f64, a: u8) -> Rgba {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let v = |t: f64| ((t + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    [v(r), v(g), v(b), a]
}

/// The net colour `base` with its hue turned [`TINT`] of the way toward `own`'s (the shorter
/// way round) and a little more saturated, at `base`'s lightness; black, white and greys leave
/// it as it is, so no wire is dark on the ground.
fn tint(base: Rgba, own: Rgba) -> Rgba {
    let (h0, s0, l0) = to_hsl(base);
    let (h1, s1, _) = to_hsl(own);
    if s1 < 0.15 {
        return base;
    }
    let turn = ((h1 - h0 + 540.0).rem_euclid(360.0)) - 180.0;
    from_hsl(h0 + turn * TINT, (s0 + 0.2).min(0.75), l0, base[3])
}

/// A lit colour (the hovered net): fully saturated and bright, standing well clear of the rest.
fn glow(c: Rgba) -> Rgba {
    let (h, _, l) = to_hsl(c);
    from_hsl(h, 1.0, (l + 0.25).clamp(0.72, 0.8), c[3])
}

/// A net faded while another is hovered: most of the way to the ground.
fn fade(c: Rgba, ground: Rgba) -> Rgba {
    let mix = |a: u8, b: u8| ((a as u32 * 2 + b as u32 * 3) / 5) as u8;
    [
        mix(c[0], ground[0]),
        mix(c[1], ground[1]),
        mix(c[2], ground[2]),
        c[3],
    ]
}

/// With [`SchematicTheme::horizon`], the lines from `from` on become hairlines (width 0).
fn hairlines(d: &mut DrawList, th: &SchematicTheme, from: usize) {
    if th.horizon {
        for l in &mut d.lines[from..] {
            l.width = 0;
        }
    }
}

const WIRE_W: Nm = 250_000;

/// The mark where a wire joins (a pin's end, a label's anchor): a small square, filled where a
/// wire would join now.
fn join_mark(d: &mut DrawList, th: &SchematicTheme, at: Pt, filled: bool) {
    let k = mm(0.5);
    let sq = vec![
        at + Pt::new(-k, -k),
        at + Pt::new(k, -k),
        at + Pt::new(k, k),
        at + Pt::new(-k, k),
    ];
    if filled {
        d.region(th.pin_end, 3, &vec![sq]);
    } else {
        let mut ring = sq.clone();
        ring.push(sq[0]);
        d.line(th.pin_end, mm(0.15), ring);
    }
}

const BODY_W: Nm = 254_000;
/// Horizon EDA's pin number size.
const HORIZON_PIN_NUMBER: Nm = 750_000;

fn text_at(s: &str, at: Pt, angle: f64, size: Nm, h: HAlign, v: VAlign) -> Text {
    Text {
        text: s.into(),
        at,
        angle,
        style: TextStyle {
            size: Size::new(size, size),
            h_align: h,
            v_align: v,
            ..Default::default()
        },
        visible: true,
    }
}

/// What is highlighted (selected, hovered) on a sheet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Highlight {
    pub items: Vec<crate::sch_edit::SchItem>,
    /// Which pins show where wires join them (a square on each pin's end).
    pub pin_ends: PinEnds,
    /// The pin end a wire would join now (under the pointer): its square filled.
    pub hot_pin: Option<Pt>,
    /// The net under the pointer (its wires, labels and junctions): drawn lit, more saturated
    /// and lighter ([`glow`]).
    pub hover: Vec<crate::sch_edit::SchItem>,
}

/// Which pins' ends are marked.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PinEnds {
    #[default]
    None,
    /// Every pin on the sheet (wiring, or moving something).
    All,
    /// One symbol's pins (the one under the pointer).
    Symbol(uuid::Uuid),
}

/// A schematic sheet, unbounded (no paper, frame or title block): symbols with pins and
/// fields, wires, buses, junctions, no-connect flags, labels and notes; where wires join pins
/// and labels marked ([`join_mark`]) where nothing joins them yet, and as [`Highlight`] asks.
pub fn schematic(sch: &Schematic, sheet: usize, th: &SchematicTheme, hl: &Highlight) -> DrawList {
    use crate::sch_edit::SchItem;
    // No paper, frame or title block: the sheet is unbounded, the paper's colour all over.
    let mut d = DrawList {
        background: th.paper,
        ..Default::default()
    };
    let Some(sh) = sch.sheets.get(sheet) else {
        return d;
    };

    let nl = netlist(sch);
    let lone: Vec<Pt> = nl
        .nets
        .iter()
        .filter(|n| n.pins.len() == 1 && n.labels.is_empty() && !n.no_connect)
        .map(|n| n.pins[0].at)
        .collect();
    let lit = |it: &SchItem| hl.items.contains(it);
    // The pins drawn (end to body), which a label's flag keeps clear of.
    let mut pin_lines: Vec<(Pt, Pt)> = Vec::new();

    for s in &sh.symbols {
        let Some(def) = sch.symbol(&s.symbol) else {
            continue;
        };
        let sel = lit(&SchItem::Symbol(s.id));
        let (body, pin, fill_bg) = if sel {
            (th.highlight, th.highlight, th.body_fill)
        } else {
            (th.body, th.pin, th.body_fill)
        };
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
                    d.shape(
                        &Shape {
                            geom,
                            ..shape.clone()
                        },
                        body,
                        fill,
                        BODY_W,
                        1,
                    );
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
                pin_lines.push((a, b));
            }
            let dir = s.placement.apply_angle(p.angle);
            if matches!(p.shape, PinShape::Inverted | PinShape::InvertedClock) {
                let r = mm(0.4);
                let c = b - Pt::new(r, 0).rotated(dir);
                d.shape(
                    &Shape {
                        geom: Geom::Circle {
                            center: c,
                            radius: r,
                        },
                        stroke: Default::default(),
                        fill: Fill::None,
                    },
                    pin,
                    None,
                    WIRE_W,
                    1,
                );
            }
            if lone.contains(&a) && p.kind == crate::symbol::PinType::NoConnect {
                // A pin that connects to nothing by design: KiCad's small X at its end, in the
                // pin's colour.
                let k = mm(0.4);
                d.line(pin, WIRE_W, vec![a - Pt::new(k, k), a + Pt::new(k, k)]);
                d.line(pin, WIRE_W, vec![a - Pt::new(k, -k), a + Pt::new(k, -k)]);
            } else {
                // Where a wire joins the pin: on its end when nothing joins it yet, while wiring
                // (or on the hovered symbol), filled where a wire would join now.
                let marked = lone.contains(&a)
                    || match hl.pin_ends {
                        PinEnds::None => false,
                        PinEnds::All => true,
                        PinEnds::Symbol(id) => id == s.id,
                    };
                if marked || hl.hot_pin == Some(a) {
                    join_mark(&mut d, th, a, hl.hot_pin == Some(a));
                }
            }
            if p.length == 0 || !p.visible {
                continue;
            }
            // Number above a level pin's middle (left of an upright one), reading as the
            // sheet does whichever way the pin points; name inside the body past its inner end.
            if def.show_pin_numbers && th.horizon {
                // Horizon's: small, ending half a millimetre short of the body and as far above
                // (left of an upright pin), running toward the pin's end.
                let level = (dir.rem_euclid(180.0) - 90.0).abs() > 45.0;
                let toward_end = Pt::new(-mm(0.5), 0).rotated(dir);
                let (angle, up, h) = if level {
                    // The end is left of the body when the pin points right (dir 0).
                    let end_left = (a.x - b.x) < 0;
                    (
                        0.0,
                        Pt::new(0, mm(0.5)),
                        if end_left {
                            HAlign::Right
                        } else {
                            HAlign::Left
                        },
                    )
                } else {
                    let end_below = (a.y - b.y) < 0;
                    (
                        90.0,
                        Pt::new(-mm(0.5), 0),
                        if end_below {
                            HAlign::Right
                        } else {
                            HAlign::Left
                        },
                    )
                };
                d.text(
                    &text_at(
                        &p.number,
                        b + toward_end + up,
                        angle,
                        HORIZON_PIN_NUMBER,
                        h,
                        VAlign::Bottom,
                    ),
                    th.pin_number,
                );
            } else if def.show_pin_numbers {
                let mid = Pt::new((a.x + b.x) / 2, (a.y + b.y) / 2);
                let level = (dir.rem_euclid(180.0) - 90.0).abs() > 45.0;
                let (angle, up) = if level {
                    (0.0, Pt::new(0, mm(0.3)))
                } else {
                    (90.0, Pt::new(-mm(0.3), 0))
                };
                d.text(
                    &text_at(
                        &p.number,
                        mid + up,
                        angle,
                        p.number_size,
                        HAlign::Center,
                        VAlign::Bottom,
                    ),
                    th.pin_number,
                );
            }
            if def.show_pin_names && !p.name.is_empty() && p.name != "~" {
                let at = b + Pt::new(def.pin_name_offset.max(mm(0.5)), 0).rotated(dir);
                d.text(
                    &text_at(&p.name, at, dir, p.name_size, HAlign::Left, VAlign::Center),
                    th.pin_name,
                );
            }
        }
        for f in &s.fields {
            let c = if sel || lit(&SchItem::Field(s.id, f.name.clone())) {
                th.highlight
            } else {
                th.field
            };
            let mut t = f.text.clone();
            if f.show_name {
                t.text = format!("{}: {}", f.name, t.text);
            }
            d.text(&t, c);
        }
    }
    // A wire's and a junction's own colour when they have one, else the theme's.
    let rgba =
        |c: Option<crate::graphics::Color>| c.filter(|c| c.a > 0).map(|c| [c.r, c.g, c.b, 255]);
    // Horizon's label flags: wires give way under their boxes.
    let taken: Vec<(Pt, Pt)> = sh
        .wires
        .iter()
        .map(|w| (w.a, w.b))
        .chain(pin_lines.iter().copied())
        .collect();
    let flags: Vec<(uuid::Uuid, Pt, f64, (Nm, Nm, Nm))> = if th.horizon {
        sh.labels
            .iter()
            .filter(|l| l.text.visible && !l.text.text.is_empty())
            .map(|l| {
                let dir = label_direction(&l.text, &taken);
                (l.id, l.text.at, dir, flag_box(&l.text))
            })
            .collect()
    } else {
        Vec::new()
    };
    let boxes: Vec<(Pt, f64, (Nm, Nm, Nm))> = flags
        .iter()
        .map(|(_, at, dir, b)| (*at, *dir, *b))
        .collect();
    // A wire's or junction's own colour: as is (KiCad), or (Horizon) only tinting the net
    // colour so every wire stays a clear green on the dark ground ([`tint`]).
    let own = |c: Option<crate::graphics::Color>, base: Rgba| match rgba(c) {
        Some(c) if th.horizon => tint(base, c),
        Some(c) => c,
        None => base,
    };
    // While a net is hovered it glows and the other nets fade toward the ground.
    let hovered = |it: &SchItem| hl.hover.contains(it);
    let net_color = |it: &SchItem, c: Rgba| {
        if hl.hover.is_empty() {
            c
        } else if hovered(it) {
            glow(c)
        } else {
            fade(c, th.background)
        }
    };
    for wire in &sh.wires {
        let c = if lit(&SchItem::Wire(wire.id)) {
            th.highlight
        } else {
            net_color(&SchItem::Wire(wire.id), own(wire.stroke.color, th.wire))
        };
        let w = if wire.stroke.width > 0 {
            wire.stroke.width
        } else {
            WIRE_W
        };
        for (a, b) in outside_flags(wire.a, wire.b, &boxes) {
            d.line(c, w, vec![a, b]);
        }
    }
    for j in &sh.junctions {
        // Horizon's 0.75 mm dot with hairlines, KiCad's 0.914 mm otherwise.
        let r = if j.diameter > 0 {
            j.diameter / 2
        } else if th.horizon {
            mm(0.375)
        } else {
            mm(0.457)
        };
        // A dot with no colour of its own takes its wires' colour.
        let wire_color = sh
            .wires
            .iter()
            .filter(|w| crate::connectivity::on_segment(j.at, w.a, w.b))
            .find_map(|w| rgba(w.stroke.color));
        let c = if lit(&SchItem::Junction(j.id)) {
            th.highlight
        } else {
            let c = match rgba(j.color).or(wire_color) {
                Some(c) if th.horizon => tint(th.junction, c),
                Some(c) => c,
                None => th.junction,
            };
            net_color(&SchItem::Junction(j.id), c)
        };
        d.region(c, 2, &poly::circle(j.at, r));
    }
    for n in &sh.no_connects {
        let k = mm(0.635);
        d.line(
            th.no_connect,
            WIRE_W,
            vec![n.at - Pt::new(k, k), n.at + Pt::new(k, k)],
        );
        d.line(
            th.no_connect,
            WIRE_W,
            vec![n.at - Pt::new(k, -k), n.at + Pt::new(k, -k)],
        );
    }
    for l in &sh.labels {
        // Where the label joins a wire (its anchor): marked while it joins nothing, while
        // wiring, and filled where it would join now.
        let at = l.text.at;
        let joined = sh
            .wires
            .iter()
            .any(|w| crate::connectivity::on_segment(at, w.a, w.b))
            || sh
                .symbols
                .iter()
                .any(|s| sch.placed_pins(s).any(|(_, p)| p == at));
        if !joined || hl.pin_ends == PinEnds::All || hl.hot_pin == Some(at) {
            join_mark(&mut d, th, at, hl.hot_pin == Some(at));
        }
        let c = if lit(&SchItem::Label(l.id)) {
            th.highlight
        } else {
            net_color(&SchItem::Label(l.id), th.label)
        };
        let mut t = l.text.clone();
        if th.horizon {
            // Horizon's flag around every label, its point on the anchor (which stays where it
            // is, so what the label joins doesn't change; the flag may lie over its wire). It
            // points the way the text runs from the anchor.
            let dir = flags.iter().find(|f| f.0 == l.id).map_or(t.angle, |f| f.2);
            horizon_flag(&mut d, &t, dir, c);
            continue;
        }
        if matches!(l.kind, LabelKind::Local) {
            // A local label's text stands just above its wire.
            t.at = t.at + Pt::new(0, mm(0.3)).rotated(t.angle);
            t.style.v_align = VAlign::Bottom;
        } else {
            // A flag around the text: pointed at the connection end.
            let size = t.style.size.h;
            let len = font::line_width(&t.text, &t.style).round() as Nm + size;
            let (hh, tip) = (size * 3 / 4, size / 2);
            let dir = t.angle;
            let pts: Vec<Pt> = [
                Pt::new(0, 0),
                Pt::new(tip, hh),
                Pt::new(tip + len, hh),
                Pt::new(tip + len, -hh),
                Pt::new(tip, -hh),
                Pt::new(0, 0),
            ]
            .iter()
            .map(|p| t.at + p.rotated(dir))
            .collect();
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
    hairlines(&mut d, th, 0);
    // Buses stay wide.
    for b in &sh.buses {
        d.line(th.bus, mm(0.3), vec![b.a, b.b]);
    }
    // Fit shows what is drawn (an empty sheet: an A4's worth from the origin).
    d.bounds = Some(d.drawn_bounds().map_or(
        Bounds {
            min: Pt::ZERO,
            max: Pt::mm(297.0, 210.0),
        },
        |b| b.grow(mm(10.0)),
    ));
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
    /// A zone's or polygon's corner handles: fill, edge, and the one under the pointer.
    pub handle: Rgba,
    pub handle_edge: Rgba,
    pub handle_hot: Rgba,
}

impl Default for BoardTheme {
    fn default() -> Self {
        BoardTheme {
            background: [0x00, 0x10, 0x23, 0xff],
            // The board inside its outline as the canvas (KiCad leaves it unfilled).
            board: [0x00, 0x10, 0x23, 0xff],
            top_copper: [0xc8, 0x34, 0x34, 0xff],
            bottom_copper: [0x4d, 0x7f, 0xc4, 0xff],
            inner_copper: [0x7f, 0xc8, 0x7f, 0xff],
            top_silk: [0xf2, 0xed, 0xa1, 0xff],
            bottom_silk: [0xe8, 0xb2, 0xa7, 0xff],
            top_courtyard: [0xff, 0x26, 0xe2, 0xff],
            bottom_courtyard: [0x26, 0xe9, 0xff, 0xff],
            top_fab: [0xaf, 0xaf, 0xaf, 0xff],
            bottom_fab: [0x58, 0x5d, 0x84, 0xff],
            outline: [0xd0, 0xd2, 0xcd, 0xff],
            via: [0xe3, 0xb7, 0x2e, 0xff],
            hole: [0x10, 0x16, 0x20, 0xff],
            ratsnest: [0x00, 0xf8, 0xff, 0xff],
            pad_text: [0xd8, 0xd8, 0xd8, 0xe0],
            highlight: [0xf9, 0x7a, 0x16, 0xff],
            handle: [0xff, 0xff, 0xff, 0xff],
            handle_edge: [0x20, 0x20, 0x20, 0xff],
            handle_hot: [0x1e, 0x6f, 0xd9, 0xff],
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
    /// Items shown selected (footprints, tracks, vias, zones, drawn shapes, text), in the
    /// highlight colour; a selected zone or polygon shows its corners.
    pub selected: Vec<uuid::Uuid>,
    /// The item under the pointer, drawn lighter.
    pub hovered: Option<uuid::Uuid>,
    /// The corner of a zone or polygon under the pointer (its handle filled), and the handles'
    /// half size (a few pixels at the view's zoom).
    pub hot_corner: Option<Pt>,
    pub handle: Nm,
    pub ratsnest: bool,
    /// Other layers than the active one drawn faint (KiCad's high-contrast mode).
    pub dim_inactive: bool,
    /// A net drawn in the highlight colour (` in the Layout).
    pub highlight_net: Option<String>,
}

impl BoardView {
    pub fn all(board: &Board) -> BoardView {
        let mut visible: Vec<Layer> = board.copper().collect();
        visible.extend([
            Layer::TopSilk,
            Layer::BottomSilk,
            Layer::TopCourtyard,
            Layer::BottomCourtyard,
            Layer::TopFab,
            Layer::BottomFab,
            Layer::Outline,
            Layer::Drawings,
            Layer::Comments,
        ]);
        BoardView {
            visible,
            active: Layer::TopCopper,
            selected: vec![],
            hovered: None,
            hot_corner: None,
            handle: mm(0.25),
            ratsnest: true,
            dim_inactive: false,
            highlight_net: None,
        }
    }
}

/// A colour a little lighter (the item under the pointer).
fn lighter(c: Rgba) -> Rgba {
    let up = |v: u8| (v as u32 + (255 - v as u32) * 45 / 100) as u8;
    [up(c[0]), up(c[1]), up(c[2]), c[3].max(0xc0)]
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
    v.extend([
        Layer::TopSilk,
        Layer::TopCourtyard,
        Layer::TopFab,
        Layer::Outline,
        Layer::Drawings,
        Layer::Comments,
    ]);
    v
}

/// A board seen from the top: the board's area, each layer's copper (pads, tracks, vias,
/// zone fills), holes, silkscreen, courtyards, fabrication drawing and outline, pad numbers,
/// and the ratsnest.
pub fn board(b: &Board, th: &BoardTheme, view: &BoardView) -> DrawList {
    let mut d = DrawList {
        background: th.background,
        ..Default::default()
    };
    let region = crate::outline::board_region(b);
    d.region(th.board, -10, &region);
    let mut bounds: Option<Bounds> = region
        .iter()
        .flatten()
        .fold(None, |acc, p| Some(Bounds::union(acc, Bounds::of(*p))));
    let items = crate::copper::items(b);
    let shown = |l: Layer| view.visible.contains(&l);
    for (z, layer) in draw_order(b, view.active).into_iter().enumerate() {
        // Three depths a layer: its zone fills, the rest of it, and what is selected or under the
        // pointer over both (at one depth with the copper they would be drawn under it as
        // often as over it).
        let z = z as i32 * 3;
        if !shown(layer) {
            continue;
        }
        let mut color = th.layer(layer);
        if view.dim_inactive && layer != view.active && layer != Layer::Outline {
            color[3] = (color[3] as u32 * 30 / 100) as u8;
        }
        if layer.is_copper() {
            // Zone fills a little see-through, so copper of the other side shows under them;
            // pads, tracks and vias on top.
            let zone = |c: &&crate::copper::Copper| matches!(c.item, crate::copper::Item::Zone(..));
            let fills: Vec<Region> = items
                .iter()
                .filter(zone)
                .filter_map(|c| c.on(layer).cloned())
                .collect();
            if !fills.is_empty() {
                let mut faint = color;
                faint[3] = (faint[3] as u32 * 55 / 100) as u8;
                d.region(faint, z, &poly::union_all(&fills));
            }
            let parts: Vec<Region> = items
                .iter()
                .filter(|c| !zone(c))
                .filter_map(|c| c.on(layer).cloned())
                .collect();
            let all = poly::union_all(&parts);
            d.region(color, z + 1, &all);
            // Each pad outlined at its clearance, a little way out (KiCad's clearance outline).
            for c in items
                .iter()
                .filter(|c| matches!(c.item, crate::copper::Item::Pad(..)))
            {
                if let Some(r) = c.on(layer) {
                    for ring in poly::inflate(r, mm(0.2)) {
                        let mut pts = ring.clone();
                        pts.push(ring[0]);
                        d.line(color, 0, pts);
                    }
                }
            }
            // Selected copper on top, in the highlight colour; the copper under the pointer
            // lighter.
            let owner = |c: &crate::copper::Copper| match &c.item {
                crate::copper::Item::Pad(f, _) | crate::copper::Item::FpShape(f, _) => *f,
                crate::copper::Item::Track(t)
                | crate::copper::Item::Via(t)
                | crate::copper::Item::Shape(t)
                | crate::copper::Item::Zone(t, ..) => *t,
            };
            let of = |want: &dyn Fn(uuid::Uuid) -> bool| -> Vec<Region> {
                items
                    .iter()
                    .filter(|c| want(owner(c)))
                    .filter_map(|c| c.on(layer).cloned())
                    .collect()
            };
            let sel = of(&|id| view.selected.contains(&id));
            if !sel.is_empty() {
                d.region(th.highlight, z + 2, &poly::union_all(&sel));
            }
            let hov = of(&|id| view.hovered == Some(id) && !view.selected.contains(&id));
            if !hov.is_empty() {
                d.region(lighter(color), z + 2, &poly::union_all(&hov));
            }
        }
        // The colour of an item on this layer: selected, under the pointer, or as the layer.
        let tint = |id: uuid::Uuid| {
            if view.selected.contains(&id) {
                th.highlight
            } else if view.hovered == Some(id) {
                lighter(color)
            } else {
                color
            }
        };
        for f in &b.footprints {
            let fc = tint(f.id);
            // Copper drawings are drawn as copper (above, from the copper items), not again as lines.
            for s in f
                .footprint
                .shapes
                .iter()
                .filter(|s| f.placement.layer(s.layer) == layer && !layer.is_copper())
            {
                let geom = if f.placement.angle % 90.0 == 0.0 {
                    s.shape.geom.clone()
                } else {
                    s.shape.geom.rect_as_polyline()
                };
                let shape = Shape {
                    geom: geom.map(|p| f.placement.apply(p)),
                    ..s.shape.clone()
                };
                let fill = (s.shape.fill != crate::graphics::Fill::None).then_some(fc);
                d.shape(&shape, fc, fill, mm(0.1), z);
            }
            let texts = f
                .footprint
                .fields
                .iter()
                .map(|x| &x.text)
                .chain(f.footprint.texts.iter());
            for t in texts.filter(|t| f.placement.layer(t.layer) == layer) {
                let mut tt = t.text.clone();
                tt.text = tt.text.replace("${REFERENCE}", f.reference());
                tt.at = f.placement.apply(t.text.at);
                tt.angle = if t.keep_upright {
                    crate::units::normalize_deg(t.text.angle + f.placement.angle)
                } else {
                    f.placement.apply_angle(t.text.angle)
                };
                if f.placement.side == Side::Bottom {
                    tt.style.mirrored = !tt.style.mirrored;
                }
                d.text(&tt, fc);
            }
        }
        for s in b
            .shapes
            .iter()
            .filter(|s| s.layer == layer && !layer.is_copper())
        {
            // A filled shape (a logo, a copper area) is filled, the rest outlined.
            let c = tint(s.id);
            let fill = (s.shape.fill != crate::graphics::Fill::None).then_some(c);
            d.shape(&s.shape, c, fill, mm(0.1), z);
            for p in poly::geom_points(&s.shape.geom).0 {
                bounds = Some(Bounds::union(bounds, Bounds::of(p)));
            }
        }
        for t in b.texts.iter().filter(|t| t.layer == layer) {
            d.text(&t.text, tint(t.id));
        }
    }
    // The highlighted net's copper, on every shown layer, over the rest.
    if let Some(net) = view.highlight_net.as_ref().filter(|n| !n.is_empty()) {
        let mine: Vec<Region> = items
            .iter()
            .filter(|c| &c.net == net)
            .flat_map(|c| {
                c.layers
                    .iter()
                    .filter(|(l, _)| shown(*l))
                    .map(|(_, r)| r.clone())
            })
            .collect();
        let mut lit = th.highlight;
        lit[3] = 0xd0;
        d.region(lit, 80, &poly::union_all(&mine));
    }
    // Zone outlines, and keep-outs hatched (the board's and footprints' ones).
    let mut zones: Vec<(Vec<Vec<Pt>>, crate::layer::LayerSet, bool)> = b
        .zones
        .iter()
        .map(|z| (z.outline.clone(), z.layers, z.keepout.is_some()))
        .collect();
    for f in &b.footprints {
        for z in &f.footprint.zones {
            let rings = z
                .outline
                .iter()
                .map(|r| r.iter().map(|p| f.placement.apply(*p)).collect())
                .collect();
            zones.push((rings, f.placement.layers(z.layers), z.keepout.is_some()));
        }
    }
    for (rings, layers, keepout) in zones {
        let Some(layer) = layers.iter().find(|l| shown(*l)) else {
            continue;
        };
        let mut color = th.layer(layer);
        if view.dim_inactive && layer != view.active {
            color[3] = (color[3] as u32 * 30 / 100) as u8;
        }
        for r in &rings {
            let mut pts = r.clone();
            if let Some(p) = pts.first().copied() {
                pts.push(p);
            }
            d.line(color, mm(0.1), pts);
        }
        if keepout {
            let region: Region = rings
                .iter()
                .map(|r| {
                    let mut r = r.clone();
                    if poly::ring_area(&r) < 0.0 {
                        r.reverse();
                    }
                    r
                })
                .collect();
            let bb = region
                .iter()
                .flatten()
                .fold(None, |acc: Option<Bounds>, p| {
                    Some(Bounds::union(acc, Bounds::of(*p)))
                });
            if let Some(bb) = bb {
                // 45° hatching, 1 mm apart.
                let h = bb.max.y - bb.min.y;
                let step = mm(1.0);
                let mut strokes = vec![];
                let mut x = bb.min.x - h;
                while x < bb.max.x {
                    strokes.push(poly::stroke(
                        &[Pt::new(x, bb.min.y), Pt::new(x + h, bb.max.y)],
                        mm(0.05),
                    ));
                    x += step;
                }
                let hatch = poly::intersection(&poly::union_all(&strokes), &region);
                let mut faint = color;
                faint[3] = (faint[3] as u32 * 40 / 100) as u8;
                d.region(faint, 90, &hatch);
            }
        }
    }
    // The corners of a selected or hovered zone or drawn polygon, to drag (Select: press on
    // one): white squares edged dark, the one under the pointer filled blue.
    let mut handled: Vec<uuid::Uuid> = view.selected.clone();
    handled.extend(view.hovered);
    handled.dedup();
    for id in handled {
        let item = if b.zones.iter().any(|z| z.id == id) {
            crate::board_edit::BoardItem::Zone(id)
        } else if b.shapes.iter().any(|s| s.id == id) {
            crate::board_edit::BoardItem::Shape(id)
        } else {
            continue;
        };
        for (_, _, p) in crate::board_edit::corners(b, item) {
            let k = view.handle.max(mm(0.1));
            let sq = vec![
                p + Pt::new(-k, -k),
                p + Pt::new(k, -k),
                p + Pt::new(k, k),
                p + Pt::new(-k, k),
            ];
            d.region(
                if view.hot_corner == Some(p) {
                    th.handle_hot
                } else {
                    th.handle
                },
                110,
                &vec![sq.clone()],
            );
            let mut ring = sq.clone();
            ring.push(sq[0]);
            d.line(th.handle_edge, k / 4, ring);
        }
    }
    // Vias and holes over the copper.
    let top = 100;
    for v in &b.vias {
        let c = if view.selected.contains(&v.id) {
            th.highlight
        } else if view.hovered == Some(v.id) {
            lighter(th.via)
        } else {
            th.via
        };
        d.region(c, top, &poly::circle(v.at, v.diameter / 2));
        d.region(th.hole, top + 1, &poly::circle(v.at, v.drill / 2));
    }
    for f in &b.footprints {
        for p in &f.footprint.pads {
            if let Some(dr) = p.drill {
                let c = f.placement.apply(p.at + dr.offset.rotated(p.angle));
                let hole = poly::hole(c, dr.size, f.placement.apply_angle(p.angle));
                d.region(th.hole, top + 1, &hole);
                // A plated hole's wall, outlined as KiCad does.
                if p.kind == PadKind::ThroughHole {
                    for ring in &hole {
                        let mut pts = ring.clone();
                        pts.push(ring[0]);
                        d.line(th.via, 0, pts);
                    }
                }
            }
            // Pad numbers, small, in the pad.
            let on_shown = f
                .placement
                .layers(p.layers)
                .iter()
                .any(|l| l.is_copper() && shown(l));
            if on_shown
                && !p.number.is_empty()
                && !matches!(p.shape, PadShape::Custom { .. })
                && p.kind != PadKind::NonPlated
            {
                let at = f.placement.apply(p.at);
                // The pad's size as it lies on the board (a quarter turn swaps its sides); the
                // text runs along its long side, as KiCad's (a tall pad's reads upwards).
                let turned =
                    (f.placement.apply_angle(p.angle).rem_euclid(180.0) - 90.0).abs() < 1.0;
                let (pw, ph) = if turned {
                    (p.size.h, p.size.w)
                } else {
                    (p.size.w, p.size.h)
                };
                let tall = ph > pw;
                let (long, short, angle) = if tall { (ph, pw, 90.0) } else { (pw, ph, 0.0) };
                // "Up" in the text's frame.
                let up = |k: Nm| if tall { Pt::new(-k, 0) } else { Pt::new(0, k) };
                let s = (short * 9 / 20).min(mm(1.0));
                // A no-connect pin's pad reads "x", as KiCad's.
                // A pad on no net is "unconnected-(REF-pin-PadN)", KiCad's name for it.
                let lone = format!(
                    "unconnected-({}-{}-Pad{})",
                    f.reference(),
                    if p.pin_function.is_empty() {
                        "~"
                    } else {
                        p.pin_function.as_str()
                    },
                    p.number
                );
                let net = if p.pin_type.starts_with("no_connect") {
                    Some("x")
                } else {
                    p.net
                        .as_deref()
                        .filter(|n| !n.is_empty())
                        .or(Some(lone.as_str()))
                };
                // With a net: the number above, the net's name under it, shrunk to fit.
                let (num_at, net_size) = match net {
                    Some(n) if short >= mm(0.5) => {
                        let probe =
                            text_at(n, Pt::ZERO, 0.0, mm(1.0), HAlign::Center, VAlign::Center);
                        let w = crate::font::bounds(&probe)
                            .map_or(mm(1.0), |b| b.max.x - b.min.x)
                            .max(1);
                        let size = ((long as f64 * 0.85 / w as f64) * mm(1.0) as f64) as Nm;
                        // Smaller than the number, as KiCad's.
                        let cap = if long >= short * 9 / 5 {
                            short * 2 / 7
                        } else {
                            short / 4
                        };
                        (at + up(short / 5), Some(size.min(cap)))
                    }
                    _ => (at, None),
                };
                let s = if net_size.is_some() {
                    s.min(short * 7 / 20)
                } else {
                    s
                };
                let mut t = text_at(&p.number, num_at, angle, s, HAlign::Center, VAlign::Center);
                // Thin strokes, in proportion to the size (KiCad's pad text).
                t.style.thickness = Some(s / 9);
                d.text(&t, th.pad_text);
                if let (Some(n), Some(ns)) = (net, net_size)
                    && ns >= mm(0.05)
                {
                    let mut t = text_at(
                        n,
                        at - up(short / 5),
                        angle,
                        ns,
                        HAlign::Center,
                        VAlign::Center,
                    );
                    t.style.thickness = Some(ns / 9);
                    d.text(&t, th.pad_text);
                }
            }
            bounds = Some(Bounds::union(bounds, Bounds::of(f.placement.apply(p.at))));
        }
    }
    // Net names along the tracks long enough to carry them, as KiCad writes them.
    for t in b
        .tracks
        .iter()
        .filter(|t| !t.net.is_empty() && shown(t.layer))
    {
        let len = t.a.dist(t.b);
        let size = (t.width as f64 * 0.6).min(mm(0.8) as f64) as Nm;
        let probe = text_at(&t.net, Pt::ZERO, 0.0, size, HAlign::Center, VAlign::Center);
        let need = crate::font::bounds(&probe).map_or(0, |b| b.max.x - b.min.x) as f64;
        // Only tracks wide enough to read (KiCad's show on the wide ones at board scale).
        // (With room to spare, so the name stays clear of the pads at the ends.)
        if size < mm(0.25) || len < need * 1.3 + mm(1.0) as f64 {
            continue;
        }
        let mut angle = ((t.b.y - t.a.y) as f64)
            .atan2((t.b.x - t.a.x) as f64)
            .to_degrees();
        // Reading left to right, or upwards.
        if !(-90.0..90.0).contains(&angle) || (angle - 90.0).abs() < 1e-6 {
            angle = crate::units::normalize_deg(angle + 180.0);
        }
        let mid = Pt::new((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2);
        let mut tt = text_at(&t.net, mid, angle, size, HAlign::Center, VAlign::Center);
        tt.style.thickness = Some(size / 8);
        d.text(&tt, th.pad_text);
    }
    // Each footprint's anchor, a small + in its courtyard's colour (as KiCad marks them).
    for f in &b.footprints {
        let court = f.placement.layer(Layer::TopCourtyard);
        if !shown(court) {
            continue;
        }
        let (c, k) = (f.placement.at, mm(0.4));
        let col = th.layer(court);
        d.line(col, 0, vec![c - Pt::new(k, 0), c + Pt::new(k, 0)]);
        d.line(col, 0, vec![c - Pt::new(0, k), c + Pt::new(0, k)]);
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
    let b = d.bounds.unwrap_or(Bounds {
        min: Pt::ZERO,
        max: Pt::mm(100.0, 100.0),
    });
    let f = |v: Nm| v as f64 / 1e6;
    let (x0, y1, w, h) = (f(b.min.x), f(b.max.y), f(b.size().w), f(b.size().h));
    let col = |c: Rgba| {
        format!(
            "rgba({},{},{},{:.3})",
            c[0],
            c[1],
            c[2],
            c[3] as f64 / 255.0
        )
    };
    let mut s = String::new();
    let _ = writeln!(
        s,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="{x0} {} {w} {h}">"#,
        -y1
    );
    let _ = writeln!(
        s,
        r#"<rect x="{x0}" y="{}" width="{w}" height="{h}" fill="{}"/>"#,
        -y1,
        col(d.background)
    );
    let mut areas: Vec<&Area> = d.areas.iter().collect();
    areas.sort_by_key(|a| a.z);
    for a in areas {
        let _ = write!(s, r#"<path fill="{}" d=""#, col(a.color));
        for t in a.tris.chunks(3) {
            let _ = write!(
                s,
                "M{} {}L{} {}L{} {}Z",
                t[0][0] / 1e6,
                -t[0][1] / 1e6,
                t[1][0] / 1e6,
                -t[1][1] / 1e6,
                t[2][0] / 1e6,
                -t[2][1] / 1e6
            );
        }
        let _ = writeln!(s, r#""/>"#);
    }
    for l in &d.lines {
        let pts: Vec<String> = l
            .pts
            .iter()
            .map(|p| format!("{},{}", f(p.x), -f(p.y)))
            .collect();
        let _ = writeln!(
            s,
            r#"<polyline fill="none" stroke="{}" stroke-width="{}" stroke-linecap="round" stroke-linejoin="round" points="{}"/>"#,
            col(l.color),
            f(l.width.max(50_000)),
            pts.join(" ")
        );
    }
    s.push_str("</svg>\n");
    s
}

/// Faint cross-hairs through the origin, for the part editors.
/// An editor view ([`symbol_view`], [`footprint_view`]) as a thumbnail: without the origin
/// axes, its box just around what is drawn (`margin` out).
pub fn tight(mut d: DrawList, margin: Nm) -> DrawList {
    let axis = |l: &Line| {
        l.pts.len() == 2
            && l.width == 0
            && (l.pts[0].x == -l.pts[1].x && l.pts[0].y == 0 && l.pts[1].y == 0
                || l.pts[0].y == -l.pts[1].y && l.pts[0].x == 0 && l.pts[1].x == 0)
            && l.pts[0] != l.pts[1]
    };
    d.lines.retain(|l| !axis(l));
    let mut b: Option<Bounds> = None;
    for l in &d.lines {
        for p in &l.pts {
            b = Some(Bounds::union(b, Bounds::of(*p).grow(l.width / 2)));
        }
    }
    for a in &d.areas {
        for t in &a.tris {
            b = Some(Bounds::union(
                b,
                Bounds::of(Pt::new(t[0] as Nm, t[1] as Nm)),
            ));
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
pub fn symbol_view(
    sym: Option<&crate::symbol::Symbol>,
    th: &SchematicTheme,
    selected: &[crate::lib_edit::SymbolPart],
) -> DrawList {
    use crate::lib_edit::SymbolPart;
    use crate::schematic::{Paper, PlacedSymbol, Placement, Sheet};
    let mut d = DrawList {
        background: th.paper,
        ..Default::default()
    };
    origin_axes(&mut d, [0x60, 0x60, 0xd0, 0x90], mm(50.0));
    let Some(sym) = sym else {
        d.bounds = Some(Bounds {
            min: Pt::mm(-20.0, -15.0),
            max: Pt::mm(20.0, 15.0),
        });
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
    sel.pins = s
        .pins
        .iter()
        .enumerate()
        .filter(|(i, _)| hl(SymbolPart::Pin(*i)))
        .map(|(_, p)| p.clone())
        .collect();
    sel.graphics = s
        .graphics
        .iter()
        .enumerate()
        .filter(|(i, _)| hl(SymbolPart::Graphic(*i)))
        .map(|(_, g)| g.clone())
        .collect();
    s.power = false;
    let sheet = Sheet {
        paper: Paper {
            name: "User".into(),
            size: Size::mm(1.0, 1.0),
        },
        symbols: vec![placed],
        ..Default::default()
    };
    let sch = Schematic {
        symbols: vec![s.clone()],
        sheets: vec![sheet],
    };
    let mut body = schematic(&sch, 0, th, &Highlight::default());
    // Drop the sheet's paper and frame (everything drawn before the symbol).
    body.areas.retain(|a| a.z != -10);
    body.lines.retain(|l| l.color != th.frame);
    d.lines.extend(body.lines);
    d.areas.extend(body.areas);
    if !sel.pins.is_empty() || !sel.graphics.is_empty() {
        sel.fields.clear();
        let mut lit = th.clone();
        (lit.body, lit.pin, lit.pin_number, lit.pin_name) =
            (th.highlight, th.highlight, th.highlight, th.highlight);
        let sheet = Sheet {
            paper: Paper {
                name: "User".into(),
                size: Size::mm(1.0, 1.0),
            },
            symbols: vec![PlacedSymbol {
                fields: vec![],
                ..sch.sheets[0].symbols[0].clone()
            }],
            ..Default::default()
        };
        let mut over = schematic(
            &Schematic {
                symbols: vec![sel],
                sheets: vec![sheet],
            },
            0,
            &lit,
            &Highlight::default(),
        );
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
            d.line(
                th.highlight,
                0,
                vec![
                    b.min,
                    Pt::new(b.max.x, b.min.y),
                    b.max,
                    Pt::new(b.min.x, b.max.y),
                    b.min,
                ],
            );
        }
    }
    let mut b: Option<Bounds> = Some(Bounds {
        min: Pt::mm(-10.0, -10.0),
        max: Pt::mm(10.0, 10.0),
    });
    for l in &d.lines {
        if l.pts
            .iter()
            .any(|p| p.x.abs() >= mm(50.0) || p.y.abs() >= mm(50.0))
        {
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
pub fn footprint_view(
    fp: Option<&crate::footprint::Footprint>,
    th: &BoardTheme,
    selected: &[uuid::Uuid],
) -> DrawList {
    use crate::board::PlacedFootprint;
    let mut board = Board::default();
    let mut d = DrawList {
        background: th.background,
        ..Default::default()
    };
    if let Some(fp) = fp {
        board.footprints.push(PlacedFootprint {
            id: uuid::Uuid::nil(),
            footprint: fp.clone(),
            placement: Default::default(),
            locked: false,
            symbol: None,
        });
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
    let mut b: Option<Bounds> = Some(Bounds {
        min: Pt::mm(-5.0, -5.0),
        max: Pt::mm(5.0, 5.0),
    });
    for a in &d.areas {
        for t in &a.tris {
            b = Some(Bounds::union(
                b,
                Bounds::of(Pt::new(t[0] as Nm, t[1] as Nm)),
            ));
        }
    }
    for l in &d.lines {
        if l.pts
            .iter()
            .any(|p| p.x.abs() >= mm(50.0) || p.y.abs() >= mm(50.0))
        {
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
        let s = schematic(
            &d.schematic,
            0,
            &SchematicTheme::default(),
            &Highlight::default(),
        );
        assert!(s.lines.len() > 50);
        // Wires in the wire colour, junction dots filled.
        assert!(
            s.lines
                .iter()
                .filter(|l| l.color == SchematicTheme::default().wire)
                .count()
                >= 8
        );
        assert!(
            s.areas
                .iter()
                .any(|a| a.color == SchematicTheme::default().junction)
        );
        let b = board(&d.board, &BoardTheme::default(), &BoardView::all(&d.board));
        let th = BoardTheme::default();
        assert!(
            b.areas.iter().any(|a| a.color == th.top_copper)
                && b.areas.iter().any(|a| a.color == th.bottom_copper)
        );
        assert!(b.lines.iter().any(|l| l.color == th.top_silk));
        let bb = b.bounds.unwrap();
        assert!(bb.contains(Pt::mm(45.0, 50.0)) && bb.contains(Pt::ZERO));
        // Nothing left to route: no ratsnest lines.
        assert!(b.lines.iter().all(|l| l.color != th.ratsnest));
    }
}

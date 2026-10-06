//! Fabrication outputs (GS21): Gerber X2 for copper, solder mask, paste, silkscreen and the
//! board outline, a Gerber job file listing them, and Excellon drill files (plated and
//! non-plated holes).
//!
//! Coordinates are millimetres with six decimals (`%FSLAX46Y46*%`), which are our nanometres
//! as they are. Round pads and vias are flashed with circle apertures, tracks and lines are
//! drawn with round apertures, everything else (pad shapes, zone fills, filled shapes) is a
//! region; holes in zone fills are cleared with clear polarity before the other copper is
//! drawn on top.

use crate::board::Board;
use crate::footprint::{PadKind, PadShape};
use crate::graphics::{Fill, Geom};
use crate::layer::{Layer, Side};
use crate::poly::{self, Region};
use crate::units::{Nm, Pt, to_mm};
use std::collections::BTreeMap;
use std::fmt::Write;

/// The layers a fabricator needs, with their file suffix and X2 file function.
pub fn standard_layers(board: &Board) -> Vec<(Layer, String, String)> {
    let mut v = vec![];
    for (i, l) in board.copper().enumerate() {
        let (suffix, side) = match l {
            Layer::TopCopper => ("F_Cu".to_string(), "Top"),
            Layer::BottomCopper => ("B_Cu".to_string(), "Bot"),
            Layer::Inner(k) => (format!("In{k}_Cu"), "Inr"),
            _ => unreachable!(),
        };
        v.push((l, suffix, format!("Copper,L{},{side}", i + 1)));
    }
    for (l, s, f) in [
        (Layer::TopMask, "F_Mask", "Soldermask,Top"),
        (Layer::BottomMask, "B_Mask", "Soldermask,Bot"),
        (Layer::TopPaste, "F_Paste", "Paste,Top"),
        (Layer::BottomPaste, "B_Paste", "Paste,Bot"),
        (Layer::TopSilk, "F_Silkscreen", "Legend,Top"),
        (Layer::BottomSilk, "B_Silkscreen", "Legend,Bot"),
        (Layer::Outline, "Edge_Cuts", "Profile,NP"),
    ] {
        v.push((l, s.into(), f.into()));
    }
    v
}

/// Gerber's six-decimal millimetres: our nanometres.
fn c(v: Nm) -> String {
    v.to_string()
}

struct Writer {
    out: String,
    apertures: BTreeMap<(char, Nm), u32>,
    current: Option<u32>,
    body: String,
}

impl Writer {
    fn new() -> Writer {
        Writer { out: String::new(), apertures: BTreeMap::new(), current: None, body: String::new() }
    }

    fn aperture(&mut self, kind: char, d: Nm) -> u32 {
        let n = self.apertures.len() as u32 + 10;
        *self.apertures.entry((kind, d.max(1))).or_insert(n)
    }

    fn select(&mut self, d: u32) {
        if self.current != Some(d) {
            let _ = writeln!(self.body, "D{d}*");
            self.current = Some(d);
        }
    }

    fn polarity(&mut self, dark: bool) {
        let _ = writeln!(self.body, "%LP{}*%", if dark { "D" } else { "C" });
    }

    fn flash_circle(&mut self, at: Pt, d: Nm) {
        let a = self.aperture('C', d);
        self.select(a);
        let _ = writeln!(self.body, "X{}Y{}D03*", c(at.x), c(at.y));
    }

    fn stroke(&mut self, pts: &[Pt], width: Nm) {
        if pts.len() < 2 {
            if let Some(p) = pts.first() {
                self.flash_circle(*p, width);
            }
            return;
        }
        let a = self.aperture('C', width);
        self.select(a);
        let _ = writeln!(self.body, "X{}Y{}D02*", c(pts[0].x), c(pts[0].y));
        for p in &pts[1..] {
            let _ = writeln!(self.body, "X{}Y{}D01*", c(p.x), c(p.y));
        }
    }

    fn region(&mut self, r: &Region) {
        for ring in r.iter().filter(|x| x.len() >= 3) {
            let _ = writeln!(self.body, "G36*");
            let _ = writeln!(self.body, "X{}Y{}D02*", c(ring[0].x), c(ring[0].y));
            for p in &ring[1..] {
                let _ = writeln!(self.body, "X{}Y{}D01*", c(p.x), c(p.y));
            }
            let _ = writeln!(self.body, "X{}Y{}D01*", c(ring[0].x), c(ring[0].y));
            let _ = writeln!(self.body, "G37*");
        }
    }

    fn finish(mut self, function: &str, polarity_negative: bool) -> String {
        let _ = writeln!(self.out, "%TF.GenerationSoftware,cadrs,cadrs_eda,{}*%", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(self.out, "%TF.SameCoordinates,Original*%");
        let _ = writeln!(self.out, "%TF.FileFunction,{function}*%");
        let _ = writeln!(self.out, "%TF.FilePolarity,{}*%", if polarity_negative { "Negative" } else { "Positive" });
        let _ = writeln!(self.out, "%FSLAX46Y46*%");
        let _ = writeln!(self.out, "%MOMM*%");
        let _ = writeln!(self.out, "%LPD*%");
        let _ = writeln!(self.out, "G01*");
        for ((kind, d), n) in &self.apertures {
            let _ = writeln!(self.out, "%ADD{n}{kind},{:.6}*%", to_mm(*d));
        }
        self.out.push_str(&self.body);
        self.out.push_str("M02*\n");
        self.out
    }
}

fn geom_stroke(w: &mut Writer, g: &Geom, width: Nm, fill: Fill, map: &dyn Fn(Pt) -> Pt) {
    let (pts, closed) = poly::geom_points(g);
    let mut pts: Vec<Pt> = pts.into_iter().map(map).collect();
    if closed && fill != Fill::None {
        let mut ring = pts.clone();
        if poly::ring_area(&ring) < 0.0 {
            ring.reverse();
        }
        w.region(&vec![ring]);
    }
    if closed && !pts.is_empty() {
        pts.push(pts[0]);
    }
    if width > 0 {
        w.stroke(&pts, width);
    }
}

fn text_strokes(w: &mut Writer, t: &crate::graphics::Text) {
    if !t.visible || t.text.is_empty() {
        return;
    }
    let width = crate::font::stroke_width(&t.style);
    for s in crate::font::strokes(t) {
        w.stroke(&s, width);
    }
}

/// One layer as a Gerber file.
pub fn gerber(board: &Board, layer: Layer, function: &str) -> String {
    let mut w = Writer::new();
    if layer.is_copper() {
        // Zone fills first, their holes cleared, then everything else on top.
        for z in board.zones.iter().filter(|z| z.keepout.is_none()) {
            for (_, polys) in z.filled.iter().filter(|(l, _)| *l == layer) {
                for pg in polys {
                    w.polarity(true);
                    w.region(&vec![pg.outer.clone()]);
                    if !pg.holes.is_empty() {
                        w.polarity(false);
                        let holes: Region = pg.holes.iter().map(|h| h.iter().rev().copied().collect()).collect();
                        w.region(&holes);
                    }
                }
            }
        }
        w.polarity(true);
        for t in board.tracks.iter().filter(|t| t.layer == layer) {
            let pts: Vec<Pt> = match t.mid {
                Some(m) => poly::geom_points(&Geom::Arc { start: t.a, mid: m, end: t.b }).0,
                None => vec![t.a, t.b],
            };
            w.stroke(&pts, t.width);
        }
        for v in &board.vias {
            if crate::copper::via_layers(board, v.from, v.to, v.kind).contains(&layer) {
                w.flash_circle(v.at, v.diameter);
            }
        }
    }
    for (fi, f) in board.footprints.iter().enumerate() {
        for (pi, pad) in f.footprint.pads.iter().enumerate() {
            let layers = f.placement.layers(pad.layers);
            let on = if layer.is_copper() { layers.contains(layer) && pad.kind != PadKind::NonPlated } else { layers.contains(layer) };
            if !on {
                continue;
            }
            let margin = match layer {
                Layer::TopMask | Layer::BottomMask => pad.rules.mask_margin.unwrap_or(board.rules.mask_margin),
                Layer::TopPaste | Layer::BottomPaste => pad.rules.paste_margin.unwrap_or(0) + pad.rules.paste_ratio.map_or(0, |r| (pad.size.w.min(pad.size.h) as f64 * r) as Nm),
                _ => 0,
            };
            if matches!(pad.shape, PadShape::Circle) {
                w.flash_circle(f.placement.apply(pad.at), pad.size.w + 2 * margin);
            } else {
                w.region(&crate::copper::pad_region(board, fi, pi, margin));
            }
        }
        for s in f.footprint.shapes.iter().filter(|s| f.placement.layer(s.layer) == layer) {
            geom_stroke(&mut w, &s.shape.geom, s.shape.stroke.width, s.shape.fill, &|p| f.placement.apply(p));
        }
        let texts = f.footprint.texts.iter().chain(f.footprint.fields.iter().map(|x| &x.text));
        for t in texts.filter(|t| f.placement.layer(t.layer) == layer) {
            let mut tt = t.text.clone();
            tt.text = tt.text.replace("${REFERENCE}", f.reference());
            tt.at = f.placement.apply(t.text.at);
            tt.angle = if t.keep_upright { crate::units::normalize_deg(t.text.angle + f.placement.angle) } else { f.placement.apply_angle(t.text.angle) };
            if f.placement.side == Side::Bottom {
                tt.style.mirrored = !tt.style.mirrored;
            }
            text_strokes(&mut w, &tt);
        }
    }
    for s in board.shapes.iter().filter(|s| s.layer == layer) {
        let width = if layer == Layer::Outline && s.shape.stroke.width == 0 { crate::units::mm(0.1) } else { s.shape.stroke.width };
        geom_stroke(&mut w, &s.shape.geom, width, if layer == Layer::Outline { Fill::None } else { s.shape.fill }, &|p| p);
    }
    for t in board.texts.iter().filter(|t| t.layer == layer) {
        text_strokes(&mut w, &t.text);
    }
    w.finish(function, false)
}

/// Every standard layer: (file name, contents). Zones should be filled first.
pub fn gerbers(board: &Board, name: &str) -> Vec<(String, String)> {
    standard_layers(board).into_iter().map(|(l, suffix, function)| (format!("{name}-{suffix}.gbr"), gerber(board, l, &function))).collect()
}

/// The Gerber job file (JSON) describing the set.
pub fn job_file(board: &Board, name: &str) -> String {
    let files: Vec<String> = standard_layers(board)
        .into_iter()
        .map(|(_, suffix, function)| format!("    {{ \"Path\": \"{name}-{suffix}.gbr\", \"FileFunction\": \"{function}\", \"FilePolarity\": \"Positive\" }}"))
        .collect();
    let b = crate::outline::board_region(board);
    let size = b.iter().flatten().fold(None, |acc: Option<crate::units::Bounds>, p| Some(crate::units::Bounds::union(acc, crate::units::Bounds::of(*p)))).map(|b| b.size()).unwrap_or_default();
    format!(
        "{{\n  \"Header\": {{ \"GenerationSoftware\": {{ \"Vendor\": \"cadrs\", \"Application\": \"cadrs_eda\", \"Version\": \"{}\" }} }},\n  \"GeneralSpecs\": {{ \"ProjectId\": {{ \"Name\": \"{name}\" }}, \"Size\": {{ \"X\": {:.4}, \"Y\": {:.4} }}, \"LayerNumber\": {}, \"BoardThickness\": {:.4} }},\n  \"FilesAttributes\": [\n{}\n  ]\n}}\n",
        env!("CARGO_PKG_VERSION"),
        to_mm(size.w),
        to_mm(size.h),
        board.copper_layers,
        to_mm(board.thickness),
        files.join(",\n")
    )
}

/// A drilled hole: where, its size (oval when width ≠ height), its angle, plated or not.
#[derive(Clone, Debug, PartialEq)]
pub struct Hole {
    pub at: Pt,
    pub size: crate::units::Size,
    pub angle: f64,
    pub plated: bool,
}

/// Every hole of the board.
pub fn holes(board: &Board) -> Vec<Hole> {
    let mut v = vec![];
    for f in &board.footprints {
        for p in &f.footprint.pads {
            if let Some(d) = p.drill {
                v.push(Hole { at: f.placement.apply(p.at + d.offset.rotated(p.angle)), size: d.size, angle: f.placement.apply_angle(p.angle), plated: p.kind != PadKind::NonPlated });
            }
        }
    }
    for via in &board.vias {
        v.push(Hole { at: via.at, size: crate::units::Size::new(via.drill, via.drill), angle: 0.0, plated: true });
    }
    v
}

/// An Excellon drill file (metric, decimal coordinates) for the plated or non-plated holes.
/// Oval holes are routed slots (G85).
pub fn excellon(board: &Board, plated: bool) -> String {
    let hs: Vec<Hole> = holes(board).into_iter().filter(|h| h.plated == plated).collect();
    let mut tools: Vec<Nm> = hs.iter().map(|h| h.size.w.min(h.size.h)).collect();
    tools.sort();
    tools.dedup();
    let mut s = String::new();
    let _ = writeln!(s, "M48");
    let _ = writeln!(s, "; DRILL file {{cadrs {}}}", env!("CARGO_PKG_VERSION"));
    let _ = writeln!(s, "; FORMAT={{-:-/ absolute / metric / decimal}}");
    let _ = writeln!(s, "; #@! TF.FileFunction,{},1,{},PTH", if plated { "Plated" } else { "NonPlated" }, board.copper_layers);
    let _ = writeln!(s, "FMAT,2");
    let _ = writeln!(s, "METRIC");
    for (i, t) in tools.iter().enumerate() {
        let _ = writeln!(s, "T{}C{:.3}", i + 1, to_mm(*t));
    }
    let _ = writeln!(s, "%");
    let _ = writeln!(s, "G90");
    let _ = writeln!(s, "G05");
    let f = |v: Nm| format!("{}", (to_mm(v) * 1e4).round() / 1e4);
    for (i, t) in tools.iter().enumerate() {
        let _ = writeln!(s, "T{}", i + 1);
        for h in hs.iter().filter(|h| h.size.w.min(h.size.h) == *t) {
            if h.size.w == h.size.h {
                let _ = writeln!(s, "X{}Y{}", f(h.at.x), f(h.at.y));
            } else {
                let half = (h.size.w.max(h.size.h) - t) / 2;
                let along = if h.size.w > h.size.h { h.angle } else { h.angle + 90.0 };
                let d = Pt::new(half, 0).rotated(along);
                let (a, b) = (h.at - d, h.at + d);
                let _ = writeln!(s, "X{}Y{}G85X{}Y{}", f(a.x), f(a.y), f(b.x), f(b.y));
            }
        }
    }
    let _ = writeln!(s, "M30");
    s
}

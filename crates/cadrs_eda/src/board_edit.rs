//! Board editing (GS16–GS17, GS25): moving, rotating and flipping footprints, dragging them
//! with their tracks attached, routing tracks with 45° bends, vias, selecting a connection's
//! tracks (U) and deleting.

use crate::board::{Board, Track, Via, ViaKind};
use crate::copper;
use crate::footprint::PadKind;
use crate::layer::{Layer, Side};
use crate::poly;
use crate::units::{Nm, Pt, normalize_deg};
use uuid::Uuid;

pub fn footprint_index(board: &Board, reference: &str) -> Option<usize> {
    board.footprints.iter().position(|f| f.reference() == reference)
}

/// Moves a footprint to `at` (M).
pub fn move_footprint(board: &mut Board, fp: usize, at: Pt) {
    board.footprints[fp].placement.at = at;
}

/// Turns a footprint by `deg` counter-clockwise about its anchor (R: 90).
pub fn rotate_footprint(board: &mut Board, fp: usize, deg: f64) {
    let p = &mut board.footprints[fp].placement;
    p.angle = normalize_deg(p.angle + deg);
}

/// Flips a footprint to the other side, mirrored left to right about its anchor (F).
pub fn flip_footprint(board: &mut Board, fp: usize) {
    let p = &mut board.footprints[fp].placement;
    p.side = p.side.flipped();
    p.angle = normalize_deg(180.0 - p.angle);
}

/// Track ends lying on footprint `fp`'s pads: (track index, which end: 0 = a, 1 = b).
fn attached_ends(board: &Board, fp: usize) -> Vec<(usize, u8)> {
    let mut out = vec![];
    let f = &board.footprints[fp];
    for (pi, pad) in f.footprint.pads.iter().enumerate() {
        let region = copper::pad_region(board, fp, pi, 0);
        for (ti, t) in board.tracks.iter().enumerate() {
            if !f.placement.layers(pad.layers).contains(t.layer) {
                continue;
            }
            for (e, p) in [(0u8, t.a), (1u8, t.b)] {
                if poly::contains(&region, p) {
                    out.push((ti, e));
                }
            }
        }
    }
    out
}

/// Drags a footprint by `d` (D): the track ends on its pads follow, so the tracks stay
/// connected (they stretch).
pub fn drag_footprint(board: &mut Board, fp: usize, d: Pt) {
    let ends = attached_ends(board, fp);
    board.footprints[fp].placement.at = board.footprints[fp].placement.at + d;
    for (ti, e) in ends {
        let t = &mut board.tracks[ti];
        if e == 0 { t.a = t.a + d } else { t.b = t.b + d }
    }
}

/// The pad under `p` on a copper layer: (footprint, pad).
pub fn pad_at(board: &Board, p: Pt) -> Option<(usize, usize)> {
    for (fi, f) in board.footprints.iter().enumerate() {
        for (pi, _) in f.footprint.pads.iter().enumerate() {
            if poly::contains(&copper::pad_region(board, fi, pi, 0), p) {
                return Some((fi, pi));
            }
        }
    }
    None
}

/// The pad's centre on the board.
pub fn pad_center(board: &Board, fp: usize, pad: usize) -> Pt {
    let f = &board.footprints[fp];
    f.placement.apply(f.footprint.pads[pad].at)
}

/// The layer a route starting at `p` goes on: an SMD pad's own layer (a bottom pad switches
/// to the bottom), else `active`.
pub fn start_layer(board: &Board, p: Pt, active: Layer) -> Layer {
    if let Some((fi, pi)) = pad_at(board, p) {
        let f = &board.footprints[fi];
        let pad = &f.footprint.pads[pi];
        if pad.kind == PadKind::Smd || pad.kind == PadKind::Connector {
            return if f.placement.side == Side::Bottom { Layer::BottomCopper } else { Layer::TopCopper };
        }
    }
    active
}

/// The net of the copper under `p` on `layer` (pads, tracks, vias), "" when none.
pub fn net_at(board: &Board, p: Pt, layer: Layer) -> String {
    if let Some((fi, pi)) = pad_at(board, p) {
        return board.footprints[fi].footprint.pads[pi].net.clone().unwrap_or_default();
    }
    if let Some(v) = board.vias.iter().find(|v| v.at.dist(p) <= v.diameter as f64 / 2.0) {
        return v.net.clone();
    }
    // A track ending here that has a net (another one there may be new, without one yet).
    board.tracks.iter().filter(|t| t.layer == layer && (t.a == p || t.b == p)).map(|t| t.net.clone()).find(|n| !n.is_empty()).unwrap_or_default()
}

/// The 45° path from `a` to `b`: a straight run then a diagonal (or the reverse with
/// `diagonal_first`), as the router lays a track between two clicks.
pub fn posture(a: Pt, b: Pt, diagonal_first: bool) -> Vec<Pt> {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    if dx == 0 || dy == 0 || dx.abs() == dy.abs() {
        return vec![a, b];
    }
    let diag = dx.abs().min(dy.abs());
    let d = Pt::new(diag * dx.signum(), diag * dy.signum());
    let mid = if diagonal_first { a + d } else { b - d };
    vec![a, mid, b]
}

/// Lays tracks along `pts` on `layer` with the net's track width (from its class unless
/// `width` is given); the net is taken from the copper at the start. Returns the new ids.
pub fn route(board: &mut Board, pts: &[Pt], layer: Layer, width: Option<Nm>) -> Vec<Uuid> {
    let net = pts.first().map(|p| net_at(board, *p, layer)).unwrap_or_default();
    let w = width.unwrap_or_else(|| board.rules.class_of(&net).track_width);
    let mut ids = vec![];
    for s in pts.windows(2) {
        if s[0] == s[1] {
            continue;
        }
        let id = Uuid::new_v4();
        board.tracks.push(Track { id, a: s[0], mid: None, b: s[1], width: w, layer, net: net.clone(), locked: false });
        ids.push(id);
    }
    ids
}

/// Adds a through via at `at` on `net` with the net class's size (V while routing).
pub fn add_via(board: &mut Board, at: Pt, net: &str) -> Uuid {
    let c = board.rules.class_of(net);
    let id = Uuid::new_v4();
    let (diameter, drill) = (c.via_diameter, c.via_drill);
    board.vias.push(Via { id, at, diameter, drill, kind: ViaKind::Through, from: Layer::TopCopper, to: Layer::BottomCopper, net: net.into(), locked: false, tented: None });
    id
}

/// The tracks of the connection a track belongs to (U): following shared ends both ways, up
/// to pads, and up to vias — or, pressing U again (`through_vias`), on through them, the vias
/// included.
pub fn select_connected(board: &Board, track: Uuid, through_vias: bool) -> Vec<Uuid> {
    let Some(start) = board.tracks.iter().position(|t| t.id == track) else { return vec![] };
    let on_pad = |p: Pt, layer: Layer| -> bool {
        pad_at(board, p).is_some_and(|(fi, pi)| board.footprints[fi].placement.layers(board.footprints[fi].footprint.pads[pi].layers).contains(layer))
    };
    let mut seen = vec![start];
    let mut vias: Vec<Uuid> = vec![];
    let mut todo = vec![start];
    while let Some(i) = todo.pop() {
        let t = &board.tracks[i];
        for end in [t.a, t.b] {
            if on_pad(end, t.layer) {
                continue;
            }
            let via = board.vias.iter().find(|v| v.at == end);
            if let Some(v) = via {
                if !through_vias {
                    continue;
                }
                if !vias.contains(&v.id) {
                    vias.push(v.id);
                }
            }
            for (j, o) in board.tracks.iter().enumerate() {
                let same_layer = o.layer == t.layer || via.is_some();
                if !seen.contains(&j) && same_layer && (o.a == end || o.b == end) {
                    seen.push(j);
                    todo.push(j);
                }
            }
        }
    }
    seen.into_iter().map(|i| board.tracks[i].id).chain(vias).collect()
}

pub fn delete_tracks(board: &mut Board, ids: &[Uuid]) {
    board.tracks.retain(|t| !ids.contains(&t.id));
    board.vias.retain(|v| !ids.contains(&v.id));
}


/// The line width drawings take on `layer` unless told: KiCad's defaults (silkscreen 0.12 mm,
/// fabrication 0.1, courtyard and outline 0.05, copper 0.2, the rest 0.15).
pub fn default_width(layer: Layer) -> Nm {
    let mm = crate::units::mm;
    match layer {
        Layer::TopSilk | Layer::BottomSilk => mm(0.12),
        Layer::TopFab | Layer::BottomFab => mm(0.1),
        Layer::TopCourtyard | Layer::BottomCourtyard | Layer::Outline => mm(0.05),
        l if l.is_copper() => mm(0.2),
        _ => mm(0.15),
    }
}

/// Draws a line, rectangle or circle on `layer` (the Draw tools); returns its id.
pub fn add_shape(board: &mut Board, geom: crate::graphics::Geom, layer: Layer) -> Uuid {
    let id = Uuid::new_v4();
    let shape = crate::graphics::Shape { geom, stroke: crate::graphics::Stroke::width(default_width(layer)), fill: crate::graphics::Fill::None };
    board.shapes.push(crate::board::BoardShape { id, shape, layer, locked: false, net: String::new() });
    id
}

/// Writes `text` on `layer` at `at` (1 mm high, 0.15 mm strokes; mirrored on the bottom so it
/// reads from below); returns its id.
pub fn add_text(board: &mut Board, text: &str, at: Pt, layer: Layer) -> Uuid {
    let id = Uuid::new_v4();
    let style = crate::graphics::TextStyle {
        size: crate::units::Size::mm(1.0, 1.0),
        thickness: Some(crate::units::mm(0.15)),
        mirrored: layer.side() == Some(Side::Bottom),
        ..Default::default()
    };
    let t = crate::graphics::Text { text: text.into(), at, angle: 0.0, style, visible: true };
    board.texts.push(crate::board::BoardText { id, text: t, layer, locked: false, knockout: false });
    id
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn postures() {
        let p = Pt::mm;
        assert_eq!(posture(p(0.0, 0.0), p(10.0, 0.0), false).len(), 2);
        let s = posture(p(0.0, 0.0), p(10.0, 3.0), false);
        assert_eq!(s, vec![p(0.0, 0.0), p(7.0, 0.0), p(10.0, 3.0)]);
        let d = posture(p(0.0, 0.0), p(10.0, 3.0), true);
        assert_eq!(d[1], p(3.0, 3.0));
    }
}

/// Something on a board that can be selected.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoardItem {
    Footprint(Uuid),
    Track(Uuid),
    Via(Uuid),
    Zone(Uuid),
    /// A drawn shape (the board outline, silkscreen art).
    Shape(Uuid),
    /// Text drawn on a layer.
    Text(Uuid),
}

impl BoardItem {
    pub fn id(self) -> Uuid {
        match self {
            BoardItem::Footprint(i) | BoardItem::Track(i) | BoardItem::Via(i) | BoardItem::Zone(i) | BoardItem::Shape(i) | BoardItem::Text(i) => i,
        }
    }
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let l2 = dx * dx + dy * dy;
    let t = if l2 == 0.0 { 0.0 } else { (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / l2).clamp(0.0, 1.0) };
    (p.x as f64 - (a.x as f64 + t * dx)).hypot(p.y as f64 - (a.y as f64 + t * dy))
}

/// The item under `p` (within `tol`): vias, then pads (their footprint), tracks, footprints
/// by their courtyard or drawing, drawn shapes (the outline), then zones.
pub fn hit(board: &Board, p: Pt, tol: Nm) -> Option<BoardItem> {
    if let Some(v) = board.vias.iter().find(|v| v.at.dist(p) <= (v.diameter / 2 + tol) as f64) {
        return Some(BoardItem::Via(v.id));
    }
    // A pad wins over the track ending on it.
    if let Some((fi, _)) = pad_at(board, p) {
        return Some(BoardItem::Footprint(board.footprints[fi].id));
    }
    if let Some(t) = board.tracks.iter().find(|t| seg_dist(p, t.a, t.b) <= (t.width / 2 + tol) as f64) {
        return Some(BoardItem::Track(t.id));
    }
    for f in board.footprints.iter().rev() {
        let mut b: Option<crate::units::Bounds> = None;
        for s in f.footprint.shapes.iter().filter(|s| matches!(s.layer, Layer::TopCourtyard | Layer::TopFab | Layer::TopSilk)) {
            for q in s.shape.geom.extent() {
                b = Some(crate::units::Bounds::union(b, crate::units::Bounds::of(f.placement.apply(q))));
            }
        }
        if b.is_some_and(|b| b.grow(tol).contains(p)) {
            return Some(BoardItem::Footprint(f.id));
        }
    }
    // Text by its box, drawn shapes by their line (the outline's edge, silkscreen art).
    if let Some(t) = board.texts.iter().rev().find(|t| crate::font::bounds(&t.text).is_some_and(|b| b.grow(tol).contains(p))) {
        return Some(BoardItem::Text(t.id));
    }
    for s in board.shapes.iter().rev() {
        let (pts, closed) = poly::geom_points(&s.shape.geom);
        let w = s.shape.stroke.width.max(1) + 2 * tol;
        let r = if closed { poly::stroke_closed(&pts, w) } else { poly::stroke(&pts, w) };
        if poly::contains(&r, p) {
            return Some(BoardItem::Shape(s.id));
        }
    }
    board.zones.iter().find(|z| z.outline.iter().any(|r| poly::contains(&vec![r.clone()], p))).map(|z| BoardItem::Zone(z.id))
}

/// Deletes items (Del).
pub fn delete_items(board: &mut Board, items: &[BoardItem]) {
    let ids: Vec<Uuid> = items.iter().map(|i| i.id()).collect();
    board.tracks.retain(|t| !ids.contains(&t.id));
    board.vias.retain(|v| !ids.contains(&v.id));
    board.footprints.retain(|f| !ids.contains(&f.id));
    board.zones.retain(|z| !ids.contains(&z.id));
    board.shapes.retain(|s| !ids.contains(&s.id));
    board.texts.retain(|t| !ids.contains(&t.id));
}

/// The footprint with this id.
pub fn footprint_by_id(board: &Board, id: Uuid) -> Option<usize> {
    board.footprints.iter().position(|f| f.id == id)
}

#[cfg(test)]
mod hit_tests {
    use super::*;
    use crate::library::LibraryTable;

    #[test]
    fn hits_on_the_course_board() {
        let lib = LibraryTable::builtin();
        let d = crate::getting_started::gs18(&lib);
        let b = &d.board;
        let r1 = footprint_index(b, "R1").unwrap();
        let pad2 = pad_center(b, r1, 1);
        assert_eq!(hit(b, pad2, 1000), Some(BoardItem::Footprint(b.footprints[r1].id)));
        let v = &b.vias[0];
        assert_eq!(hit(b, v.at, 1000), Some(BoardItem::Via(v.id)));
        // The middle of the led track, between D1 and R1.
        let t = b.tracks.iter().find(|t| t.net == "led").unwrap();
        let mid = Pt::new((t.a.x + t.b.x) / 2, (t.a.y + t.b.y) / 2);
        assert_eq!(hit(b, mid, 1000), Some(BoardItem::Track(t.id)));
        // Empty board area: the zone.
        assert!(matches!(hit(b, Pt::mm(2.0, 2.0), 1000), Some(BoardItem::Zone(_))));
        let mut b2 = b.clone();
        delete_items(&mut b2, &[BoardItem::Track(t.id)]);
        assert_eq!(b2.tracks.len(), b.tracks.len() - 1);
    }
}

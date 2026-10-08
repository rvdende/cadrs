//! Design rules check (GS19): copper too close, holes too close, solder mask openings
//! exposing two nets, copper near the board edge, undersized tracks, drills and rings,
//! overlapping courtyards, a broken outline; plus the unrouted connections (the ratsnest).

use crate::board::Board;
use crate::copper::{self, Copper, Item};
use crate::layer::{Layer, Side};
use crate::poly::{self, Region};
use crate::units::{Nm, Pt, to_mm};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rule {
    Clearance,
    HoleClearance,
    MaskBridge,
    EdgeClearance,
    TrackWidth,
    DrillTooSmall,
    AnnularRing,
    HoleToHole,
    CourtyardOverlap,
    OutlineNotClosed,
    DanglingTrack,
    /// A track, via, pad or footprint inside a keep-out that forbids it.
    KeepoutViolation,
    /// A footprint without a courtyard (unless it allows that).
    MissingCourtyard,
    /// Silkscreen over a pad's solder mask opening (the fab clips it).
    SilkOverMask,
}

impl Rule {
    pub fn is_warning(self) -> bool {
        matches!(self, Rule::DanglingTrack | Rule::MissingCourtyard | Rule::SilkOverMask)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Violation {
    pub rule: Rule,
    /// "Clearance violation (zone clearance 0.5000 mm; actual 0.0000 mm)".
    pub message: String,
    pub items: Vec<String>,
    pub at: Pt,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub violations: Vec<Violation>,
    pub unconnected: Vec<copper::Airwire>,
}

impl Report {
    pub fn errors(&self) -> impl Iterator<Item = &Violation> {
        self.violations.iter().filter(|v| !v.rule.is_warning())
    }
}

fn mm4(v: f64) -> String {
    format!("{:.4} mm", v / 1e6)
}

fn layer_short(l: Layer) -> String {
    l.name()
}

/// How an item reads in a report: "PTH pad 1 [VCC] of R1", "Zone [GND] on Bottom copper".
pub fn describe(board: &Board, c: &Copper) -> String {
    let net = |n: &str| if n.is_empty() { "<no net>".to_string() } else { n.to_string() };
    match &c.item {
        Item::Pad(fid, pi) => {
            let f = board.footprints.iter().find(|f| f.id == *fid).unwrap();
            let p = &f.footprint.pads[*pi];
            let kind = if p.drill.is_some() { "PTH pad" } else { "Pad" };
            format!("{kind} {} [{}] of {}", p.number, net(&c.net), f.reference())
        }
        Item::Track(id) => {
            let t = board.tracks.iter().find(|t| t.id == *id).unwrap();
            format!("Track [{}] on {}, length {}", net(&c.net), layer_short(t.layer), mm4(t.a.dist(t.b)))
        }
        Item::Via(_) => format!("Via [{}]", net(&c.net)),
        Item::FpShape(fid, _) => {
            let f = board.footprints.iter().find(|f| f.id == *fid).unwrap();
            format!("Copper drawing [{}] of {}", net(&c.net), f.reference())
        }
        Item::Shape(_) => format!("Copper drawing [{}]", net(&c.net)),
        Item::Zone(zid, l, _) => {
            let z = board.zones.iter().find(|z| z.id == *zid).unwrap();
            format!("Zone [{}] on {}, priority {}", net(&c.net), layer_short(*l), z.priority)
        }
    }
}

fn zone_clearance(board: &Board, c: &Copper) -> Option<Nm> {
    if let Item::Zone(zid, ..) = &c.item {
        board.zones.iter().find(|z| z.id == *zid).map(|z| z.fill.clearance)
    } else {
        None
    }
}

fn same_net(a: &Copper, b: &Copper) -> bool {
    !a.net.is_empty() && a.net == b.net
}

fn bounds_near(a: &Copper, b: &Copper, gap: Nm) -> bool {
    let (x, y) = (a.bounds.grow(gap), &b.bounds);
    x.min.x <= y.max.x && x.max.x >= y.min.x && x.min.y <= y.max.y && x.max.y >= y.min.y
}

/// One of the same zone's polygons, or parts of one footprint where one is a copper drawing or
/// an unnumbered pad (a PCB antenna's strips and plated holes are one conductor): never checked
/// against each other here.
fn related(board: &Board, a: &Copper, b: &Copper) -> bool {
    let owner = |c: &Copper| match c.item {
        Item::Pad(f, _) | Item::FpShape(f, _) => Some(f),
        _ => None,
    };
    let loose = |c: &Copper| match c.item {
        Item::FpShape(..) => true,
        Item::Pad(f, i) => board.footprints.iter().find(|x| x.id == f).is_some_and(|x| x.footprint.pads[i].number.is_empty()),
        _ => false,
    };
    match (&a.item, &b.item) {
        (Item::Zone(x, ..), Item::Zone(y, ..)) => x == y,
        _ => owner(a).is_some() && owner(a) == owner(b) && (loose(a) || loose(b)),
    }
}

/// Runs every rule on the board as it is (fill zones first for an up-to-date check).
pub fn check(board: &Board) -> Report {
    let items = copper::items(board);
    let mut v: Vec<Violation> = vec![];
    let rules = &board.rules;

    // Copper to copper.
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            let (a, b) = (&items[i], &items[j]);
            if same_net(a, b) || related(board, a, b) {
                continue;
            }
            let netclass = rules.clearance(&a.net, &b.net);
            let (need, why) = match zone_clearance(board, a).or_else(|| zone_clearance(board, b)) {
                Some(z) => (z.max(netclass), "zone clearance"),
                None => (netclass, "netclass 'Default' clearance"),
            };
            if !bounds_near(a, b, need) {
                continue;
            }
            let mut worst: Option<(f64, Pt)> = None;
            for (l, ra) in &a.layers {
                if let Some(rb) = b.on(*l) {
                    let d = poly::distance(ra, rb);
                    if d < need as f64 - 1.0 && worst.is_none_or(|w| d < w.0) {
                        worst = Some((d, a.anchors.first().copied().unwrap_or_default()));
                    }
                }
            }
            if let Some((d, at)) = worst {
                v.push(Violation {
                    rule: Rule::Clearance,
                    message: format!("Clearance violation ({why} {}; actual {})", mm4(need as f64), mm4(d)),
                    items: vec![describe(board, a), describe(board, b)],
                    at,
                });
            }
        }
    }
    // Holes to copper of other nets.
    for a in items.iter().filter(|c| c.hole.is_some()) {
        let h = a.hole.as_ref().unwrap();
        for b in &items {
            if std::ptr::eq(a, b) || same_net(a, b) || !bounds_near(a, b, rules.hole_clearance) {
                continue;
            }
            let d = b.layers.iter().map(|(_, rb)| poly::distance(h, rb)).fold(f64::MAX, f64::min);
            if d < rules.hole_clearance as f64 - 1.0 {
                v.push(Violation {
                    rule: Rule::HoleClearance,
                    message: format!("Hole clearance violation (board setup constraints hole clearance {}; actual {})", mm4(rules.hole_clearance as f64), mm4(d)),
                    items: vec![describe(board, a), describe(board, b)],
                    at: a.anchors[0],
                });
            }
        }
    }
    // Solder mask openings exposing copper of another net.
    for side in [Side::Top, Side::Bottom] {
        let (mask, cu) = if side == Side::Top { (Layer::TopMask, Layer::TopCopper) } else { (Layer::BottomMask, Layer::BottomCopper) };
        for (fi, f) in board.footprints.iter().enumerate() {
            for (pi, pad) in f.footprint.pads.iter().enumerate() {
                if !f.placement.layers(pad.layers).contains(mask) {
                    continue;
                }
                let margin = pad.rules.mask_margin.unwrap_or(rules.mask_margin);
                let opening = copper::pad_region(board, fi, pi, margin);
                // The pad as copper: its net is the one connectivity gives it (an unnumbered
                // pad's is its footprint's).
                let me = items.iter().find(|x| matches!(x.item, Item::Pad(id, k) if id == f.id && k == pi));
                let net = me.map_or(String::new(), |m| m.net.clone());
                let exposed: Vec<&Copper> = items
                    .iter()
                    .filter(|c| !(c.net == net && !net.is_empty()) && !matches!(c.item, Item::Pad(id, k) if id == f.id && k == pi))
                    .filter(|c| me.is_none_or(|m| !related(board, m, c)))
                    .filter(|c| c.on(cu).is_some_and(|r| poly::overlaps(r, &opening)))
                    .collect();
                if let Some(c) = exposed.first() {
                    let name = if side == Side::Top { "Front" } else { "Back" };
                    v.push(Violation {
                        rule: Rule::MaskBridge,
                        message: format!("{name} solder mask aperture bridges items with different nets"),
                        items: me.map(|m| describe(board, m)).into_iter().chain([describe(board, c)]).collect(),
                        at: f.placement.apply(pad.at),
                    });
                }
            }
        }
    }
    // The board edge.
    let region = crate::outline::board_region(board);
    if region.is_empty() || !crate::outline::open_ends(board).is_empty() {
        v.push(Violation { rule: Rule::OutlineNotClosed, message: "Board has malformed outline (not closed)".into(), items: vec![], at: Pt::ZERO });
    } else {
        let inside = poly::inflate(&region, -rules.copper_edge_clearance);
        for c in items.iter().filter(|c| !matches!(c.item, Item::Zone(..))) {
            let outside: Region = c.layers.iter().flat_map(|(_, r)| poly::difference(r, &inside)).collect();
            if poly::area(&outside) > 1e6 {
                v.push(Violation {
                    rule: Rule::EdgeClearance,
                    message: format!("Board edge clearance violation (board setup constraints edge clearance {})", mm4(rules.copper_edge_clearance as f64)),
                    items: vec![describe(board, c)],
                    at: c.anchors[0],
                });
            }
        }
    }
    // Sizes.
    for t in &board.tracks {
        if t.width < rules.min_track_width {
            v.push(Violation { rule: Rule::TrackWidth, message: format!("Track width {} is below {}", mm4(t.width as f64), mm4(rules.min_track_width as f64)), items: vec![format!("Track [{}]", t.net)], at: t.a });
        }
    }
    let mut holes: Vec<(Pt, Nm, String)> = vec![];
    for f in &board.footprints {
        for p in &f.footprint.pads {
            if let Some(d) = p.drill {
                let c = f.placement.apply(p.at + d.offset);
                holes.push((c, d.size.w.min(d.size.h), format!("Pad {} of {}", p.number, f.reference())));
                if d.size.w.min(d.size.h) < rules.min_drill {
                    v.push(Violation { rule: Rule::DrillTooSmall, message: format!("Drill too small ({})", mm4(d.size.w as f64)), items: vec![format!("Pad {} of {}", p.number, f.reference())], at: c });
                }
                let ring = (p.size.w.min(p.size.h) - d.size.w.max(d.size.h)) / 2;
                if p.kind == crate::footprint::PadKind::ThroughHole && ring < rules.min_annular_ring {
                    v.push(Violation { rule: Rule::AnnularRing, message: format!("Annular width {} is below {}", mm4(ring as f64), mm4(rules.min_annular_ring as f64)), items: vec![format!("PTH pad {} of {}", p.number, f.reference())], at: c });
                }
            }
        }
    }
    for via in &board.vias {
        holes.push((via.at, via.drill, format!("Via [{}]", via.net)));
        if via.drill < rules.min_drill {
            v.push(Violation { rule: Rule::DrillTooSmall, message: format!("Drill too small ({})", mm4(via.drill as f64)), items: vec![format!("Via [{}]", via.net)], at: via.at });
        }
        if (via.diameter - via.drill) / 2 < rules.min_annular_ring {
            v.push(Violation { rule: Rule::AnnularRing, message: "Annular width too small".into(), items: vec![format!("Via [{}]", via.net)], at: via.at });
        }
    }
    for i in 0..holes.len() {
        for j in i + 1..holes.len() {
            let gap = holes[i].0.dist(holes[j].0) - (holes[i].1 + holes[j].1) as f64 / 2.0;
            if gap < rules.hole_to_hole as f64 {
                v.push(Violation { rule: Rule::HoleToHole, message: format!("Drilled holes too close together ({})", mm4(gap)), items: vec![holes[i].2.clone(), holes[j].2.clone()], at: holes[i].0 });
            }
        }
    }
    // Courtyards on the same side.
    let courtyards: Vec<(usize, Side, Region)> = board
        .footprints
        .iter()
        .enumerate()
        .map(|(i, f)| {
            // Rectangles, or lines chained into loops (KiCad's courtyards are four lines).
            let geoms = f.footprint.shapes.iter().filter(|s| s.layer == Layer::TopCourtyard).map(|s| &s.shape.geom);
            let r: Vec<Region> = crate::outline::loops_of(geoms)
                .iter()
                .map(|l| {
                    let mut ring: Vec<Pt> = crate::outline::ring(l).into_iter().map(|p| f.placement.apply(p)).collect();
                    if poly::ring_area(&ring) < 0.0 {
                        ring.reverse();
                    }
                    vec![ring]
                })
                .collect();
            (i, f.placement.side, poly::union_all(&r))
        })
        .collect();
    for a in 0..courtyards.len() {
        for b in a + 1..courtyards.len() {
            let (ia, sa, ra) = &courtyards[a];
            let (ib, sb, rb) = &courtyards[b];
            if sa == sb && poly::overlaps(ra, rb) {
                v.push(Violation {
                    rule: Rule::CourtyardOverlap,
                    message: "Courtyards overlap".into(),
                    items: vec![board.footprints[*ia].reference().into(), board.footprints[*ib].reference().into()],
                    at: board.footprints[*ia].placement.at,
                });
            }
        }
    }
    // Footprints without a courtyard.
    for (i, _, r) in &courtyards {
        let f = &board.footprints[*i];
        if r.is_empty() && !f.footprint.attrs.allow_missing_courtyard && !f.footprint.pads.is_empty() {
            v.push(Violation { rule: Rule::MissingCourtyard, message: "Footprint has no courtyard defined".into(), items: vec![f.reference().into()], at: f.placement.at });
        }
    }
    // Silkscreen over solder mask openings (pads), per side.
    for side in [Side::Top, Side::Bottom] {
        let (silk, mask) = if side == Side::Top { (Layer::TopSilk, Layer::TopMask) } else { (Layer::BottomSilk, Layer::BottomMask) };
        let mut openings: Vec<(Region, String)> = vec![];
        for (fi, f) in board.footprints.iter().enumerate() {
            for (pi, pad) in f.footprint.pads.iter().enumerate() {
                if f.placement.layers(pad.layers).contains(mask) {
                    openings.push((copper::pad_region(board, fi, pi, pad.rules.mask_margin.unwrap_or(rules.mask_margin)), format!("Pad {} of {}", pad.number, f.reference())));
                }
            }
        }
        let mut silk_items: Vec<(Region, String)> = vec![];
        for f in &board.footprints {
            for s in f.footprint.shapes.iter().filter(|s| f.placement.layer(s.layer) == silk) {
                silk_items.push((poly::map(&poly::shape_region(&s.shape), |p| f.placement.apply(p)), format!("Silkscreen of {}", f.reference())));
            }
        }
        for s in board.shapes.iter().filter(|s| s.layer == silk) {
            silk_items.push((poly::shape_region(&s.shape), "Silkscreen drawing".into()));
        }
        for (sr, sname) in &silk_items {
            if let Some((_, pname)) = openings.iter().find(|(o, _)| poly::overlaps(o, sr)) {
                let at = sr.first().and_then(|r| r.first()).copied().unwrap_or_default();
                v.push(Violation { rule: Rule::SilkOverMask, message: "Silkscreen clipped by solder mask".into(), items: vec![sname.clone(), pname.clone()], at });
            }
        }
    }
    // Keep-outs: what they forbid, inside them on their layers (a footprint's own keep-out
    // doesn't forbid its own parts).
    for (owner, layers, region, rules) in keepouts(board) {
        for c in &items {
            let forbidden = match c.item {
                Item::Track(_) => rules.tracks,
                Item::Via(_) => rules.vias,
                Item::Pad(f, _) => rules.pads && Some(f) != owner,
                _ => false,
            };
            if forbidden && c.layers.iter().any(|(l, r)| layers.contains(*l) && poly::overlaps(r, &region)) {
                v.push(Violation { rule: Rule::KeepoutViolation, message: "Items not allowed (keep-out)".into(), items: vec![describe(board, c)], at: c.anchors.first().copied().unwrap_or_default() });
            }
        }
        if rules.footprints {
            for (i, side, r) in &courtyards {
                let f = &board.footprints[*i];
                let on = if *side == Side::Top { Layer::TopCopper } else { Layer::BottomCopper };
                if Some(f.id) != owner && layers.contains(on) && poly::overlaps(r, &region) {
                    v.push(Violation { rule: Rule::KeepoutViolation, message: "Footprint not allowed (keep-out)".into(), items: vec![f.reference().into()], at: f.placement.at });
                }
            }
        }
    }
    // Track ends on nothing.
    for (ti, c) in items.iter().enumerate().filter(|(_, c)| matches!(c.item, Item::Track(_))) {
        let Item::Track(id) = &c.item else { continue };
        let t = board.tracks.iter().find(|t| t.id == *id).unwrap();
        for end in [t.a, t.b] {
            let dot = poly::circle(end, t.width / 4);
            let on = items.iter().enumerate().any(|(j, o)| j != ti && o.on(t.layer).is_some_and(|r| poly::overlaps(r, &dot)));
            if !on {
                v.push(Violation { rule: Rule::DanglingTrack, message: "Track has unconnected end".into(), items: vec![describe(board, c)], at: end });
            }
        }
    }
    Report { violations: v, unconnected: copper::ratsnest(board) }
}

/// For messages: millimetres with four decimals.
pub fn fmt_mm(v: Nm) -> String {
    format!("{:.4}", to_mm(v))
}

/// Every keep-out: (the footprint it belongs to, its layers, its area, what it forbids).
fn keepouts(board: &Board) -> Vec<(Option<uuid::Uuid>, crate::layer::LayerSet, Region, crate::board::Keepout)> {
    let ring = |pts: Vec<Pt>| {
        let mut r = pts;
        if poly::ring_area(&r) < 0.0 {
            r.reverse();
        }
        r
    };
    let mut out = vec![];
    for z in &board.zones {
        if let Some(k) = z.keepout {
            out.push((None, z.layers, z.outline.iter().map(|r| ring(r.clone())).collect(), k));
        }
    }
    for f in &board.footprints {
        for z in &f.footprint.zones {
            if let Some(k) = z.keepout {
                let region: Region = z.outline.iter().map(|r| ring(r.iter().map(|p| f.placement.apply(*p)).collect())).collect();
                out.push((Some(f.id), f.placement.layers(z.layers), region, k));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::PlacedFootprint;

    #[test]
    fn silkscreen_over_pads_and_missing_courtyards() {
        let lib = crate::library::LibraryTable::builtin();
        let mut b = Board::with_rect_outline(crate::units::mm(20.0), crate::units::mm(20.0));
        let mut f = lib.footprint("Resistor_SMD:R_0805_2012Metric").unwrap().clone();
        let place = |f: crate::footprint::Footprint, x: f64| PlacedFootprint { id: uuid::Uuid::new_v4(), footprint: f, placement: crate::footprint::FootprintPlacement { at: Pt::mm(x, 10.0), ..Default::default() }, locked: false, symbol: None };
        b.footprints.push(place(f.clone(), 5.0));
        assert!(check(&b).violations.iter().all(|v| !matches!(v.rule, Rule::SilkOverMask | Rule::MissingCourtyard)));
        // Silkscreen across pad 1, and a copy without its courtyard.
        crate::lib_edit::add_fp_shape(&mut f, crate::graphics::Geom::Line { a: Pt::mm(-1.5, 0.0), b: Pt::mm(1.5, 0.0) }, Layer::TopSilk, crate::units::mm(0.12));
        f.shapes.retain(|s| s.layer != Layer::TopCourtyard);
        b.footprints.push(place(f, 15.0));
        let rules: Vec<Rule> = check(&b).violations.iter().map(|v| v.rule).collect();
        assert!(rules.contains(&Rule::SilkOverMask), "{rules:?}");
        assert!(rules.contains(&Rule::MissingCourtyard), "{rules:?}");
    }
}

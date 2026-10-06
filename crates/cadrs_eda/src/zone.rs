//! Copper zones (GS18): drawing one, and filling (B).
//!
//! A fill is the zone's outline, inside the board less its edge clearance, with copper of
//! other nets (pads, tracks, vias, other zones' fills, every hole) cut away with clearance.
//! Pads of the zone's own net join through thermal reliefs (a gap with spokes) or solidly;
//! tracks and vias of its net join solidly. Necks thinner than the minimum width are removed,
//! and islands touching nothing of the net are dropped.

use crate::board::{Board, Polygon, Zone, ZoneFill};
use crate::copper::{self, Item};
use crate::footprint::ZoneConnection;
use crate::layer::{Layer, LayerSet};
use crate::poly::{self, Region};
use crate::units::{Nm, Pt};
use uuid::Uuid;

/// Draws a zone on `layer` for `net` with the board's usual fill settings; it starts unfilled.
pub fn add_zone(board: &mut Board, net: &str, layer: Layer, outline: Vec<Pt>) -> Uuid {
    let id = Uuid::new_v4();
    board.zones.push(Zone {
        id,
        name: String::new(),
        net: net.into(),
        layers: LayerSet::of(&[layer]),
        priority: 0,
        outline: vec![outline],
        fill: ZoneFill::default(),
        keepout: None,
        locked: false,
        filled: vec![],
    });
    id
}

/// A region's rings as polygons with holes (holes go to the outer ring holding them).
pub fn to_polygons(r: &Region) -> Vec<Polygon> {
    let mut outers: Vec<Polygon> = r.iter().filter(|x| poly::ring_area(x) > 0.0).map(|x| Polygon { outer: x.clone(), holes: vec![] }).collect();
    for h in r.iter().filter(|x| poly::ring_area(x) < 0.0) {
        let inside = |p: &Polygon| poly::contains(&vec![p.outer.clone()], h[0]);
        // The smallest outer ring around the hole.
        if let Some(o) = outers.iter_mut().filter(|o| inside(o)).min_by(|a, b| poly::ring_area(&a.outer).total_cmp(&poly::ring_area(&b.outer))) {
            o.holes.push(h.clone());
        }
    }
    outers
}

fn thermal_spokes(center: Pt, angle: f64, reach: Nm, width: Nm) -> Region {
    let mut r = vec![];
    for k in 0..4 {
        let d = Pt::new(reach, 0).rotated(angle + 90.0 * k as f64);
        r.extend(poly::stroke(&[center, center + d], width));
    }
    r
}

/// Fills one zone on each of its layers.
pub fn fill_zone(board: &Board, zone: &Zone) -> Vec<(Layer, Vec<Polygon>)> {
    if zone.keepout.is_some() {
        return vec![];
    }
    let board_area = poly::inflate(&crate::outline::board_region(board), -board.rules.copper_edge_clearance);
    let outline: Region = zone
        .outline
        .iter()
        .map(|r| {
            let mut r = r.clone();
            if poly::ring_area(&r) < 0.0 {
                r.reverse();
            }
            r
        })
        .collect();
    let items = copper::items(board);
    let mut out = vec![];
    for layer in zone.layers.copper().iter() {
        let mut area = poly::intersection(&outline, &board_area);
        let mut cut: Vec<Region> = vec![];
        let mut spokes: Vec<Region> = vec![];
        for c in &items {
            if let Item::Zone(zid, ..) = &c.item
                && *zid == zone.id
            {
                continue;
            }
            // Holes clear everything on every layer.
            if let Some(h) = &c.hole {
                cut.push(poly::inflate(h, board.rules.hole_clearance.max(zone.fill.clearance) + poly::MAX_ERROR));
            }
            let Some(shape) = c.on(layer) else { continue };
            let same = !zone.net.is_empty() && c.net == zone.net;
            // Offsets are drawn with chords inside the true curve: clear by one chord error more.
            let clearance = zone.fill.clearance.max(board.rules.clearance(&zone.net, &c.net)) + poly::MAX_ERROR;
            match (&c.item, same) {
                (Item::Pad(fid, pi), true) => {
                    let f = board.footprints.iter().find(|f| f.id == *fid).unwrap();
                    let pad = &f.footprint.pads[*pi];
                    let mode = match pad.rules.zone_connection {
                        ZoneConnection::Inherit => zone.fill.pad_connection,
                        m => m,
                    };
                    match mode {
                        ZoneConnection::Solid => {}
                        ZoneConnection::None | ZoneConnection::Inherit => cut.push(poly::inflate(shape, clearance)),
                        ZoneConnection::Thermal => {
                            let gap = pad.rules.thermal_gap.unwrap_or(zone.fill.thermal_gap);
                            let spoke = pad.rules.thermal_spoke.unwrap_or(zone.fill.thermal_spoke).max(zone.fill.min_width);
                            cut.push(poly::inflate(shape, gap));
                            let center = f.placement.apply(pad.at);
                            let reach = pad.size.w.max(pad.size.h) / 2 + gap + zone.fill.min_width;
                            let mut s = thermal_spokes(center, f.placement.apply_angle(pad.angle), reach, spoke);
                            // Spokes stop short of the hole.
                            if let Some(h) = &c.hole {
                                s = poly::difference(&s, &poly::inflate(h, board.rules.hole_clearance.min(gap)));
                            }
                            spokes.push(s);
                        }
                    }
                }
                (_, true) => {}
                (Item::Zone(..), false) => cut.push(poly::inflate(shape, clearance)),
                (_, false) => cut.push(poly::inflate(shape, clearance)),
            }
        }
        area = poly::difference(&area, &poly::union_all(&cut));
        if !spokes.is_empty() {
            // Spokes reach back into the fill but stay inside the zone and clear of others.
            let mut s = poly::intersection(&poly::union_all(&spokes), &poly::intersection(&outline, &board_area));
            let others: Vec<Region> = items
                .iter()
                .filter(|c| c.net != zone.net)
                .filter_map(|c| c.on(layer).map(|r| poly::inflate(r, zone.fill.clearance.max(board.rules.clearance(&zone.net, &c.net)) + poly::MAX_ERROR)))
                .collect();
            s = poly::difference(&s, &poly::union_all(&others));
            area = poly::union_all(&[area, s]);
        }
        // Remove necks thinner than the minimum width.
        let half = zone.fill.min_width / 2;
        area = poly::inflate(&poly::inflate(&area, -half), half);
        let mut polys = to_polygons(&area);
        if zone.fill.remove_islands {
            let mine: Vec<&Region> = items.iter().filter(|c| c.net == zone.net && !matches!(c.item, Item::Zone(..))).filter_map(|c| c.on(layer)).collect();
            polys.retain(|pg| {
                let mut r = vec![pg.outer.clone()];
                r.extend(pg.holes.iter().cloned());
                mine.iter().any(|m| poly::overlaps(&r, m))
            });
        }
        out.push((layer, polys));
    }
    out
}

/// Fills every zone (B), higher priority first so lower ones keep clear of them.
pub fn fill_all(board: &mut Board) {
    let mut order: Vec<usize> = (0..board.zones.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(board.zones[i].priority));
    for z in &mut board.zones {
        z.filled.clear();
    }
    for i in order {
        let filled = fill_zone(board, &board.zones[i]);
        board.zones[i].filled = filled;
    }
}

/// Unfills every zone.
pub fn unfill_all(board: &mut Board) {
    board.zones.iter_mut().for_each(|z| z.filled.clear());
}

/// The filled area of a zone on a layer (mm² for messages and tests).
pub fn filled_area(z: &Zone, layer: Layer) -> f64 {
    z.filled
        .iter()
        .filter(|(l, _)| *l == layer)
        .flat_map(|(_, p)| p)
        .map(|pg| poly::ring_area(&pg.outer) + pg.holes.iter().map(|h| poly::ring_area(h)).sum::<f64>())
        .sum::<f64>()
        / 1e12
}

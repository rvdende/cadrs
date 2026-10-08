//! Copper on a board: every pad, track, via and zone fill as polygons per layer, what touches
//! what, and the ratsnest (GS16–GS17): for each net, the connections not yet made in copper.

use crate::board::{Board, ViaKind};
use crate::footprint::PadKind;
use crate::layer::{Layer, LayerSet};
use crate::poly::{self, Region};
use crate::units::{Bounds, Nm, Pt};
use uuid::Uuid;

/// What a copper item is.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Item {
    /// Footprint id, pad index.
    Pad(Uuid, usize),
    Track(Uuid),
    Via(Uuid),
    /// Zone id, layer, filled polygon index.
    Zone(Uuid, Layer, usize),
    /// A footprint's drawing on a copper layer (footprint id, shape index): a PCB antenna's
    /// strips, a net tie. It carries the footprint's net when its pads have only one.
    FpShape(Uuid, usize),
    /// A board drawing on a copper layer (shape id), on its own net.
    Shape(Uuid),
}

/// A copper item: its net, its shape on each copper layer it is on, its hole (pads, vias), and
/// the points ratsnest lines start from.
#[derive(Clone, Debug)]
pub struct Copper {
    pub item: Item,
    pub net: String,
    pub layers: Vec<(Layer, Region)>,
    pub hole: Option<Region>,
    pub anchors: Vec<Pt>,
    pub bounds: Bounds,
}

impl Copper {
    pub fn on(&self, l: Layer) -> Option<&Region> {
        self.layers.iter().find(|(x, _)| *x == l).map(|(_, r)| r)
    }

    pub fn layer_set(&self) -> LayerSet {
        LayerSet::of(&self.layers.iter().map(|(l, _)| *l).collect::<Vec<_>>())
    }
}

fn bounds_of(regions: &[&Region]) -> Bounds {
    let mut b: Option<Bounds> = None;
    for r in regions {
        for ring in r.iter() {
            for p in ring {
                b = Some(Bounds::union(b, Bounds::of(*p)));
            }
        }
    }
    b.unwrap_or(Bounds::of(Pt::ZERO))
}

/// The copper layers of a via: from its top layer down to its bottom one.
pub fn via_layers(board: &Board, from: Layer, to: Layer, kind: ViaKind) -> Vec<Layer> {
    let all: Vec<Layer> = board.copper().collect();
    if kind == ViaKind::Through {
        return all;
    }
    let (i, j) = (all.iter().position(|l| *l == from).unwrap_or(0), all.iter().position(|l| *l == to).unwrap_or(all.len() - 1));
    all[i.min(j)..=i.max(j)].to_vec()
}

/// A placed pad's copper on the board, grown by `margin`.
pub fn pad_region(board: &Board, fp: usize, pad: usize, margin: Nm) -> Region {
    let f = &board.footprints[fp];
    let local = poly::pad_local(&f.footprint.pads[pad], margin);
    poly::map(&local, |p| f.placement.apply(p))
}

/// A placed pad's hole on the board.
pub fn pad_hole(board: &Board, fp: usize, pad: usize) -> Option<Region> {
    let f = &board.footprints[fp];
    let p = &f.footprint.pads[pad];
    let d = p.drill?;
    let center = f.placement.apply(p.at + d.offset.rotated(p.angle));
    Some(poly::hole(center, d.size, f.placement.apply_angle(p.angle)))
}

/// Every copper item of the board.
pub fn items(board: &Board) -> Vec<Copper> {
    let copper: Vec<Layer> = board.copper().collect();
    let mut out = vec![];
    for (fi, f) in board.footprints.iter().enumerate() {
        let loose_net = footprint_net(&f.footprint);
        for (pi, pad) in f.footprint.pads.iter().enumerate() {
            let layers: Vec<Layer> = copper.iter().copied().filter(|l| f.placement.layers(pad.layers).contains(*l)).collect();
            if layers.is_empty() && pad.kind != PadKind::NonPlated {
                continue;
            }
            let r = pad_region(board, fi, pi, 0);
            let hole = pad_hole(board, fi, pi);
            let plated = pad.kind != PadKind::NonPlated;
            out.push(Copper {
                item: Item::Pad(f.id, pi),
                // An unnumbered pad (a PCB antenna's plated hole) carries its footprint's net.
                net: match (&pad.net, pad.number.is_empty()) {
                    (Some(n), _) if !n.is_empty() => n.clone(),
                    (_, true) => loose_net.clone(),
                    _ => String::new(),
                },
                bounds: bounds_of(&[&r]),
                layers: if plated { layers.iter().map(|l| (*l, r.clone())).collect() } else { vec![] },
                hole,
                anchors: vec![f.placement.apply(pad.at)],
            });
        }
    }
    for t in &board.tracks {
        let pts: Vec<Pt> = match t.mid {
            Some(m) => crate::geom::arc_points(t.a, m, t.b, poly::MAX_ERROR).into_iter().map(|q| Pt::new(q[0].round() as Nm, q[1].round() as Nm)).collect(),
            None => vec![t.a, t.b],
        };
        let r = poly::stroke(&pts, t.width);
        out.push(Copper { item: Item::Track(t.id), net: t.net.clone(), bounds: bounds_of(&[&r]), layers: vec![(t.layer, r)], hole: None, anchors: vec![t.a, t.b] });
    }
    for v in &board.vias {
        let r = poly::circle(v.at, v.diameter / 2);
        let layers = via_layers(board, v.from, v.to, v.kind);
        out.push(Copper {
            item: Item::Via(v.id),
            net: v.net.clone(),
            bounds: bounds_of(&[&r]),
            layers: layers.into_iter().map(|l| (l, r.clone())).collect(),
            hole: Some(poly::circle(v.at, v.drill / 2)),
            anchors: vec![v.at],
        });
    }
    for f in &board.footprints {
        let net = footprint_net(&f.footprint);
        for (si, s) in f.footprint.shapes.iter().enumerate() {
            let layer = f.placement.layer(s.layer);
            if !copper.contains(&layer) {
                continue;
            }
            let r = poly::map(&poly::shape_region(&s.shape), |p| f.placement.apply(p));
            let anchors = poly::geom_points(&s.shape.geom).0.into_iter().take(1).map(|p| f.placement.apply(p)).collect();
            out.push(Copper { item: Item::FpShape(f.id, si), net: net.clone(), bounds: bounds_of(&[&r]), layers: vec![(layer, r)], hole: None, anchors });
        }
    }
    for s in board.shapes.iter().filter(|s| copper.contains(&s.layer)) {
        let r = poly::shape_region(&s.shape);
        let anchors = poly::geom_points(&s.shape.geom).0.into_iter().take(1).collect();
        out.push(Copper { item: Item::Shape(s.id), net: s.net.clone(), bounds: bounds_of(&[&r]), layers: vec![(s.layer, r)], hole: None, anchors });
    }
    for z in board.zones.iter().filter(|z| z.keepout.is_none()) {
        for (l, polys) in &z.filled {
            for (i, pg) in polys.iter().enumerate() {
                let mut r: Region = vec![pg.outer.clone()];
                r.extend(pg.holes.iter().cloned());
                let anchors = pg.outer.first().copied().into_iter().collect();
                out.push(Copper { item: Item::Zone(z.id, *l, i), net: z.net.clone(), bounds: bounds_of(&[&r]), layers: vec![(*l, r)], hole: None, anchors });
            }
        }
    }
    out
}

fn bounds_touch(a: &Bounds, b: &Bounds) -> bool {
    a.min.x <= b.max.x && a.max.x >= b.min.x && a.min.y <= b.max.y && a.max.y >= b.min.y
}

/// Whether two items' copper touches on a shared layer.
pub fn touching(a: &Copper, b: &Copper) -> bool {
    if !bounds_touch(&a.bounds, &b.bounds) {
        return false;
    }
    a.layers.iter().any(|(l, ra)| b.on(*l).is_some_and(|rb| poly::overlaps(ra, rb) || poly::distance(ra, rb) < 1.0))
}

/// Groups of touching items (indices into `items`), each group one piece of copper.
pub fn islands(items: &[Copper]) -> Vec<Vec<usize>> {
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        p[i] = r;
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            if touching(&items[i], &items[j]) {
                let (a, b) = (find(&mut parent, i), find(&mut parent, j));
                parent[a] = b;
            }
        }
    }
    let mut groups: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = find(&mut parent, i);
        groups.entry(r).or_default().push(i);
    }
    groups.into_values().collect()
}

/// One ratsnest line: a connection of `net` still to be made, between two pieces of copper.
#[derive(Clone, Debug, PartialEq)]
pub struct Airwire {
    pub net: String,
    pub a: Pt,
    pub b: Pt,
}

/// The ratsnest: per net, the shortest lines joining its separate pieces of copper (a minimum
/// spanning tree). Its length is the number of unrouted connections.
pub fn ratsnest(board: &Board) -> Vec<Airwire> {
    let all = items(board);
    let mut out = vec![];
    let mut nets: Vec<&str> = all.iter().map(|c| c.net.as_str()).filter(|n| !n.is_empty()).collect();
    nets.sort();
    nets.dedup();
    for net in nets {
        let mine: Vec<Copper> = all.iter().filter(|c| c.net == net).cloned().collect();
        // Islands of loose copper alone need no connection.
        let groups: Vec<Vec<usize>> = islands(&mine).into_iter().filter(|g| g.iter().any(|&i| needs_connection(board, &mine[i]))).collect();
        if groups.len() < 2 {
            continue;
        }
        let anchors: Vec<Vec<Pt>> = groups.iter().map(|g| g.iter().flat_map(|&i| mine[i].anchors.clone()).collect()).collect();
        // Prim's algorithm over the groups.
        let mut joined = vec![false; groups.len()];
        joined[0] = true;
        for _ in 1..groups.len() {
            let mut best: Option<(f64, usize, Pt, Pt)> = None;
            for (gi, ga) in anchors.iter().enumerate().filter(|(i, _)| joined[*i]) {
                let _ = gi;
                for (gj, gb) in anchors.iter().enumerate().filter(|(j, _)| !joined[*j]) {
                    for &a in ga {
                        for &b in gb {
                            let d = a.dist(b);
                            if best.is_none_or(|x| d < x.0) {
                                best = Some((d, gj, a, b));
                            }
                        }
                    }
                }
            }
            let Some((_, j, a, b)) = best else { break };
            joined[j] = true;
            out.push(Airwire { net: net.to_string(), a, b });
        }
    }
    out
}

/// The net a footprint's loose copper (drawings on copper, unnumbered pads) belongs to: its
/// numbered pads' net when they all have the same one, else none.
pub fn footprint_net(f: &crate::footprint::Footprint) -> String {
    let mut nets = f.pads.iter().filter_map(|p| p.net.as_deref()).filter(|n| !n.is_empty());
    let first = nets.next();
    first.filter(|n| nets.all(|m| m == *n)).unwrap_or("").to_string()
}

/// Whether an item has to be connected to the rest of its net: not a footprint's loose copper
/// (drawings, unnumbered pads) or a board drawing, which only join what touches them.
pub fn needs_connection(board: &Board, c: &Copper) -> bool {
    match c.item {
        Item::FpShape(..) | Item::Shape(..) => false,
        Item::Pad(f, i) => board.footprints.iter().find(|x| x.id == f).is_some_and(|x| !x.footprint.pads[i].number.is_empty()),
        _ => true,
    }
}

//! The board outline (GS15): the shapes on [`Layer::Outline`] chained into closed loops —
//! lines and arcs end to end in any order and direction, rectangles, circles and polygons as
//! they are. The largest loop is the board's edge; loops inside it are cut-outs.

use crate::board::Board;
use crate::graphics::Geom;
use crate::layer::Layer;
use crate::poly::{self, Region, Ring};
use crate::units::{Nm, Pt, mm};

/// Ends closer than this join. Drawn outlines often miss by a few micrometres.
pub const JOIN_TOL: Nm = 20_000;

/// One edge of a loop: a line, or an arc through `mid`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Edge {
    pub a: Pt,
    pub b: Pt,
    pub mid: Option<Pt>,
}

impl Edge {
    fn reversed(self) -> Edge {
        Edge { a: self.b, b: self.a, mid: self.mid }
    }

    /// Points from `a` to `b` (arcs flattened), `b` included.
    fn points(&self) -> Vec<Pt> {
        match self.mid {
            None => vec![self.b],
            Some(m) => {
                let pts = crate::geom::arc_points(self.a, m, self.b, poly::MAX_ERROR);
                pts.into_iter().skip(1).map(|q| Pt::new(q[0].round() as Nm, q[1].round() as Nm)).collect()
            }
        }
    }
}

fn near(a: Pt, b: Pt) -> bool {
    a.dist(b) <= JOIN_TOL as f64
}

/// A closed loop of edges, each starting where the previous one ends.
pub type Loop = Vec<Edge>;

/// The outline's closed loops, largest first. Open chains are left out (see
/// [`open_ends`]).
pub fn loops(board: &Board) -> Vec<Loop> {
    let mut out: Vec<Loop> = vec![];
    let mut edges: Vec<Edge> = vec![];
    for s in board.outline_shapes() {
        match s.geom {
            Geom::Line { a, b } => edges.push(Edge { a, b, mid: None }),
            Geom::Arc { start, mid, end } => edges.push(Edge { a: start, b: end, mid: Some(mid) }),
            Geom::Rect { a, b } => {
                let c = [a, Pt::new(b.x, a.y), b, Pt::new(a.x, b.y)];
                out.push((0..4).map(|i| Edge { a: c[i], b: c[(i + 1) % 4], mid: None }).collect());
            }
            Geom::Circle { center, radius } => {
                let (e, w) = (center + Pt::new(radius, 0), center - Pt::new(radius, 0));
                out.push(vec![
                    Edge { a: e, b: w, mid: Some(center + Pt::new(0, radius)) },
                    Edge { a: w, b: e, mid: Some(center - Pt::new(0, radius)) },
                ]);
            }
            Geom::Polyline { pts, closed } => {
                let n = pts.len();
                let segs = if closed { n } else { n.saturating_sub(1) };
                let list: Vec<Edge> = (0..segs).map(|i| Edge { a: pts[i], b: pts[(i + 1) % n], mid: None }).collect();
                if closed { out.push(list) } else { edges.extend(list) }
            }
            Geom::Bezier { pts } => {
                let p: Vec<Pt> = crate::geom::bezier_points(pts, poly::MAX_ERROR).into_iter().map(|q| Pt::new(q[0].round() as Nm, q[1].round() as Nm)).collect();
                edges.extend(p.windows(2).map(|w| Edge { a: w[0], b: w[1], mid: None }));
            }
        }
    }
    let mut used = vec![false; edges.len()];
    for i in 0..edges.len() {
        if used[i] {
            continue;
        }
        used[i] = true;
        let mut chain = vec![edges[i]];
        while !near(chain.last().unwrap().b, chain[0].a) || chain.len() < 2 {
            let end = chain.last().unwrap().b;
            let next = (0..edges.len()).find_map(|j| {
                if used[j] {
                    None
                } else if near(edges[j].a, end) {
                    Some((j, edges[j]))
                } else if near(edges[j].b, end) {
                    Some((j, edges[j].reversed()))
                } else {
                    None
                }
            });
            let Some((j, e)) = next else { break };
            used[j] = true;
            chain.push(e);
        }
        if chain.len() >= 2 && near(chain.last().unwrap().b, chain[0].a) {
            out.push(chain);
        }
    }
    out.sort_by(|a, b| ring_area(&ring(b)).abs().total_cmp(&ring_area(&ring(a)).abs()));
    out
}

/// Free ends of outline chains that didn't close (for DRC's "board outline not closed").
pub fn open_ends(board: &Board) -> Vec<Pt> {
    let closed: Vec<Pt> = loops(board).iter().flatten().flat_map(|e| [e.a, e.b]).collect();
    let mut points = vec![];
    for s in board.outline_shapes() {
        let (a, b) = match s.geom {
            Geom::Line { a, b } => (a, b),
            Geom::Arc { start, end, .. } => (start, end),
            _ => continue,
        };
        points.extend([a, b].into_iter().filter(|p| !closed.iter().any(|q| near(*q, *p))));
    }
    // An end met by only one edge is free.
    points.iter().copied().filter(|p| points.iter().filter(|q| near(**q, *p)).count() == 1).collect()
}

/// A loop's points (arcs flattened), starting at its first edge's start.
pub fn ring(l: &Loop) -> Ring {
    let mut pts = vec![l[0].a];
    for e in l {
        pts.extend(e.points());
    }
    pts.pop();
    pts
}

fn ring_area(r: &Ring) -> f64 {
    poly::ring_area(r)
}

/// The board's area: the outline minus its cut-outs. Empty without a closed outline.
pub fn board_region(board: &Board) -> Region {
    let ls = loops(board);
    let Some((outer, cuts)) = ls.split_first() else { return vec![] };
    let mut o = ring(outer);
    if poly::ring_area(&o) < 0.0 {
        o.reverse();
    }
    let holes: Region = cuts
        .iter()
        .map(|c| {
            let mut r = ring(c);
            if poly::ring_area(&r) < 0.0 {
                r.reverse();
            }
            r
        })
        .collect();
    poly::difference(&vec![o], &holes)
}

/// Adds a rectangular outline from `a` to `b` (the rectangle tool on the outline layer).
pub fn add_rect(board: &mut Board, a: Pt, b: Pt) -> uuid::Uuid {
    let id = uuid::Uuid::new_v4();
    board.shapes.push(crate::board::BoardShape {
        id,
        shape: crate::graphics::Shape { geom: Geom::Rect { a, b }, stroke: crate::graphics::Stroke::width(mm(0.05)), fill: crate::graphics::Fill::None },
        layer: Layer::Outline,
        locked: false,
        net: String::new(),
    });
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chains_and_regions() {
        let mut b = Board::default();
        let p = Pt::mm;
        let line = |a, c| crate::board::BoardShape {
            id: uuid::Uuid::new_v4(),
            shape: crate::graphics::Shape { geom: Geom::Line { a, b: c }, stroke: Default::default(), fill: Default::default() },
            layer: Layer::Outline,
            locked: false,
            net: String::new(),
        };
        // A 30 × 20 rectangle drawn as four lines, one reversed and one 8 µm short.
        b.shapes = vec![line(p(0.0, 0.0), p(30.0, 0.0)), line(p(30.0, 20.0), p(30.0, 0.0)), line(p(30.0, 20.0), p(0.0, 20.0)), line(p(0.0, 20.0), p(0.0, 0.008))];
        let r = board_region(&b);
        assert!((poly::area(&r) / 1e12 - 600.0).abs() < 0.01);
        assert!(open_ends(&b).is_empty());
        // A round cut-out.
        b.shapes.push(crate::board::BoardShape { shape: crate::graphics::Shape { geom: Geom::Circle { center: p(15.0, 10.0), radius: mm(2.0) }, stroke: Default::default(), fill: Default::default() }, ..b.shapes[0].clone() });
        let r = board_region(&b);
        assert!((poly::area(&r) / 1e12 - (600.0 - std::f64::consts::PI * 4.0)).abs() < 0.05);
        assert!(!poly::contains(&r, p(15.0, 10.0)));
        b.shapes.remove(1);
        assert_eq!(open_ends(&b).len(), 2);
        assert!(board_region(&b).is_empty() || poly::area(&board_region(&b)) < 13e12);
    }
}

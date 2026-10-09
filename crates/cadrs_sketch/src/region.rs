//! Closed-region detection: the areas a sketch fills grey, what Extrude picks, and what a
//! region selection measures (its **Area** readout).
//!
//! Regular (non-construction) lines, arcs and circles are first split into pieces wherever they
//! cross or touch each other (an end lying on another curve, a tangent point), as Onshape does:
//! overlapping geometry makes regions without trimming. The pieces form a planar graph on the
//! points where they meet. After pruning dangling pieces (an open chain encloses nothing), each
//! face of the graph is traced by always turning to the next piece clockwise at each vertex;
//! faces traced counter-clockwise (positive area) are the bounded regions. A circle nothing
//! touches is a region on its own. Regions of one connected piece of geometry that lie inside a
//! region of another become its holes.
//!
//! Each region keeps its boundary both as polygons (arcs tessellated, for drawing, filling and
//! hit-testing) and exactly, as line and arc [`Piece`]s, so [`Region::area`] is exact (Green's
//! theorem over the true curves, not the tessellation).

use std::collections::HashMap;
use std::f64::consts::TAU;

use serde::{Deserialize, Serialize};

use crate::geom::{ArcGeom, BezierGeom, EllipseArcGeom, EllipseGeom, norm_angle, point_in_polygon, polygon_area};
use crate::{CurveId, CurveKind, ImprintShape, Sketch, Vec2};

/// One piece of a region's boundary, in the direction the boundary runs: a straight segment,
/// or a circular arc (its `sweep` is signed: positive runs counter-clockwise).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Piece {
    Line(Vec2, Vec2),
    Arc(ArcGeom),
    /// A piece of an ellipse, from parameter `t0` through `sweep` (signed).
    Ellipse { g: EllipseGeom, t0: f64, sweep: f64 },
    /// A cubic Bézier curve (or a piece of one, itself a cubic), from its `p[0]` to `p[3]`.
    Bezier(BezierGeom),
}

impl Piece {
    /// `½∮(x dy − y dx)` along the piece: summed over a closed boundary, its signed area.
    pub fn area_term(&self) -> f64 {
        match *self {
            Piece::Line(a, b) => a.cross(b) / 2.0,
            Piece::Arc(g) => {
                let (c, r) = (g.center, g.radius);
                let (t0, t1) = (g.start_angle, g.start_angle + g.sweep);
                (r * c.x * (t1.sin() - t0.sin()) - r * c.y * (t1.cos() - t0.cos())
                    + r * r * g.sweep)
                    / 2.0
            }
            Piece::Ellipse { g, t0, sweep } if g.offset != 0.0 => {
                // An offset ellipse: ½∫ p × p' dt by Simpson's rule (1024 intervals).
                let n = 1024;
                let h = sweep / n as f64;
                let f = |t: f64| g.point_at(t).cross(g.tangent_at(t));
                (0..=n)
                    .map(|i| {
                        let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
                        w * f(t0 + i as f64 * h)
                    })
                    .sum::<f64>()
                    * h
                    / 6.0
            }
            Piece::Bezier(g) => g.area_term(),
            Piece::Ellipse { g, t0, sweep } => {
                // p(t) = c + A cos t + B sin t: ½[(c×A)(cos t1 − cos t0) + (c×B)(sin t1 − sin
                // t0) + (A×B)(t1 − t0)].
                let (c, a, b) = (g.center, g.a, g.b());
                let t1 = t0 + sweep;
                (c.cross(a) * (t1.cos() - t0.cos())
                    + c.cross(b) * (t1.sin() - t0.sin())
                    + a.cross(b) * sweep)
                    / 2.0
            }
        }
    }

    pub fn start(&self) -> Vec2 {
        match *self {
            Piece::Line(a, _) => a,
            Piece::Arc(g) => g.start(),
            Piece::Ellipse { g, t0, .. } => g.point_at(t0),
            Piece::Bezier(g) => g.p[0],
        }
    }

    pub fn end(&self) -> Vec2 {
        match *self {
            Piece::Line(_, b) => b,
            Piece::Arc(g) => g.end(),
            Piece::Ellipse { g, t0, sweep } => g.point_at(t0 + sweep),
            Piece::Bezier(g) => g.p[3],
        }
    }

    /// The same piece, run the other way.
    pub fn reversed(&self) -> Piece {
        match *self {
            Piece::Line(a, b) => Piece::Line(b, a),
            Piece::Arc(g) => Piece::Arc(ArcGeom {
                start_angle: g.start_angle + g.sweep,
                sweep: -g.sweep,
                ..g
            }),
            Piece::Ellipse { g, t0, sweep } => Piece::Ellipse {
                g,
                t0: t0 + sweep,
                sweep: -sweep,
            },
            Piece::Bezier(g) => Piece::Bezier(g.reversed()),
        }
    }

    fn length(&self) -> f64 {
        match *self {
            Piece::Line(a, b) => a.distance(b),
            Piece::Arc(g) => g.length(),
            Piece::Ellipse { .. } => self.path().windows(2).map(|w| w[0].distance(w[1])).sum(),
            Piece::Bezier(g) => g.length(),
        }
    }

    /// Points along the piece, both ends included.
    fn path(&self) -> Vec<Vec2> {
        match *self {
            Piece::Line(a, b) => vec![a, b],
            Piece::Arc(g) => g.tessellate(STEP, 4),
            Piece::Ellipse { g, t0, sweep } => g.tessellate_range(t0, t0 + sweep, STEP, 4),
            Piece::Bezier(g) => g.tessellate(STEP, 8),
        }
    }

    /// The direction the piece leaves its start, as an angle (0..2π), nudged by its curvature
    /// so pieces leaving along the same tangent sort by how they turn (one turning left comes
    /// counter-clockwise after one going straight).
    fn leave_key(&self) -> f64 {
        let (t, k) = match *self {
            Piece::Line(a, b) => ((b - a).normalize(), 0.0),
            Piece::Arc(g) => {
                let k = if g.sweep >= 0.0 { 1.0 } else { -1.0 } / g.radius.max(1e-300);
                (g.start_tangent(), k)
            }
            Piece::Ellipse { g, t0, sweep } => {
                let d = g.tangent_at(t0);
                let t = if sweep >= 0.0 { d } else { -d };
                // Curvature sign: an ellipse run counter-clockwise turns left.
                let k = if sweep >= 0.0 { 1.0 } else { -1.0 } / g.major().max(1e-300);
                (t.normalize(), k)
            }
            Piece::Bezier(g) => {
                // A handle on the end leaves along the next control point.
                let d = g.tangent_at(0.0);
                let d = if d.length() > 1e-12 { d } else { g.p[2] - g.p[0] };
                (d.normalize(), g.curvature_at(0.0))
            }
        };
        norm_angle(t.angle()) + 1e-9 * (k * 100.0).atan() / std::f64::consts::FRAC_PI_2
    }
}

/// The signed area enclosed by a closed boundary of pieces (positive when counter-clockwise).
pub fn pieces_area(pieces: &[Piece]) -> f64 {
    pieces.iter().map(Piece::area_term).sum()
}

/// A closed region: an outer boundary (counter-clockwise) and holes (clockwise), as polygons in
/// sketch coordinates (arcs tessellated), and exactly as pieces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Region {
    pub outer: Vec<Vec2>,
    pub holes: Vec<Vec<Vec2>>,
    /// The curves on the outer boundary, one per piece of `outer_pieces`.
    pub curves: Vec<CurveId>,
    /// For each segment of `outer` (from point `i` to point `i + 1`, wrapping), the curve it
    /// lies on.
    pub outer_curves: Vec<CurveId>,
    /// The same for each hole.
    pub hole_curves: Vec<Vec<CurveId>>,
    /// The outer boundary exactly (counter-clockwise).
    pub outer_pieces: Vec<Piece>,
    /// Each hole exactly (clockwise).
    pub hole_pieces: Vec<Vec<Piece>>,
    /// The curve of each piece of each hole (parallel to `hole_pieces`).
    pub hole_piece_curves: Vec<Vec<CurveId>>,
}

impl Region {
    /// The enclosed area, holes excluded: exact, from the boundary pieces.
    pub fn area(&self) -> f64 {
        if self.outer_pieces.is_empty() {
            return polygon_area(&self.outer)
                - self.holes.iter().map(|h| polygon_area(h).abs()).sum::<f64>();
        }
        pieces_area(&self.outer_pieces).abs()
            - self
                .hole_pieces
                .iter()
                .map(|h| pieces_area(h).abs())
                .sum::<f64>()
    }

    /// Triangles covering the region: vertices and indices (three per triangle).
    pub fn triangulate(&self) -> (Vec<Vec2>, Vec<u32>) {
        let mut flat = Vec::new();
        let mut hole_starts = Vec::new();
        let mut verts = Vec::new();
        for (i, ring) in std::iter::once(&self.outer).chain(&self.holes).enumerate() {
            if i > 0 {
                hole_starts.push(verts.len());
            }
            for p in ring {
                flat.push(p.x);
                flat.push(p.y);
                verts.push(*p);
            }
        }
        let indices = earcutr::earcut(&flat, &hole_starts, 2)
            .unwrap_or_default()
            .into_iter()
            .map(|i| i as u32)
            .collect();
        (verts, indices)
    }

    /// True if `p` lies in the region (inside the outer boundary and outside every hole).
    pub fn contains(&self, p: Vec2) -> bool {
        point_in_polygon(p, &self.outer) && !self.holes.iter().any(|h| point_in_polygon(p, h))
    }
}

/// The region under a sketch point, as an index into `regions`: the smallest one containing it
/// (a region inside another's hole is not part of it, but pick the innermost to be safe).
pub fn region_at(regions: &[Region], p: Vec2) -> Option<usize> {
    regions
        .iter()
        .enumerate()
        .filter(|(_, r)| r.contains(p))
        .min_by(|a, b| a.1.area().total_cmp(&b.1.area()))
        .map(|(i, _)| i)
}

/// Angle step for tessellating arcs and circles in region outlines.
const STEP: f64 = std::f64::consts::PI / 72.0;

/// A regular curve as the arrangement sees it.
#[derive(Debug, Clone, Copy)]
enum Carrier {
    Seg(Vec2, Vec2),
    /// Counter-clockwise.
    Arc(ArcGeom),
    Circle(Vec2, f64),
    Ellipse(EllipseGeom),
    /// Part of an ellipse, parametrized by the ellipse's parameter swept from its start,
    /// 0..sweep.
    EllipseArc(EllipseArcGeom),
    /// Parametrized 0..1.
    Bezier(BezierGeom),
}

impl Carrier {
    /// The parameter of the point of the carrier nearest `p` (segment: 0..1; arc: the angle
    /// swept from its start, 0..sweep; circle: its angle, 0..2π), and how far `p` is from it.
    /// Segments and arcs clamp to their ends.
    fn project(&self, p: Vec2) -> (f64, f64) {
        match *self {
            Carrier::Seg(a, b) => {
                let d = b - a;
                let l2 = d.dot(d).max(1e-300);
                let t = ((p - a).dot(d) / l2).clamp(0.0, 1.0);
                (t, p.distance(a + d * t))
            }
            Carrier::Arc(g) => {
                let v = p - g.center;
                let off = norm_angle(v.angle() - g.start_angle);
                let off = if off <= g.sweep {
                    off
                } else if off - g.sweep < TAU - off {
                    g.sweep
                } else {
                    0.0
                };
                (off, p.distance(g.point_at(g.start_angle + off)))
            }
            Carrier::Circle(c, r) => {
                let v = p - c;
                (norm_angle(v.angle()), (v.length() - r).abs())
            }
            Carrier::Ellipse(g) => {
                let t = g.nearest_t(p);
                (t, g.point_at(t).distance(p))
            }
            Carrier::EllipseArc(g) => {
                let t = g.nearest_t(p);
                (t - g.t0, g.point_at(t).distance(p))
            }
            Carrier::Bezier(g) => {
                let t = g.nearest_t(p);
                (t, g.point_at(t).distance(p))
            }
        }
    }

    fn at(&self, t: f64) -> Vec2 {
        match *self {
            Carrier::Seg(a, b) => a.lerp(b, t),
            Carrier::Arc(g) => g.point_at(g.start_angle + t),
            Carrier::Circle(c, r) => c + Vec2::from_angle(t) * r,
            Carrier::Ellipse(g) => g.point_at(t),
            Carrier::EllipseArc(g) => g.point_at(g.t0 + t),
            Carrier::Bezier(g) => g.point_at(t),
        }
    }

    /// True for a closed carrier (a circle or an ellipse), parametrized 0..2π.
    fn closed(&self) -> bool {
        matches!(self, Carrier::Circle(..) | Carrier::Ellipse(_))
    }

    /// A signed distance-like value of `p` from the whole carrier (zero on it; its sign tells
    /// the two sides apart): for crossings with an ellipse, found numerically.
    fn side(&self, p: Vec2) -> f64 {
        match *self {
            Carrier::Seg(a, b) => (b - a).normalize().cross(p - a),
            Carrier::Arc(g) => p.distance(g.center) - g.radius,
            Carrier::Circle(c, r) => p.distance(c) - r,
            Carrier::Ellipse(g) => g.implicit(p) * g.major().min(g.minor.abs()) / 2.0,
            // Its whole ellipse (crossings off the arc are dropped by `project`).
            Carrier::EllipseArc(g) => g.e.implicit(p) * g.e.major().min(g.e.minor.abs()) / 2.0,
            // Which side of the nearest point's tangent (not used for crossings, see
            // `bezier_crossings`).
            Carrier::Bezier(g) => {
                let t = g.nearest_t(p);
                g.tangent_at(t).normalize().cross(p - g.point_at(t))
            }
        }
    }

    /// The piece from parameter `t0` to `t1` (increasing; a circle may wrap past 2π).
    fn piece(&self, t0: f64, t1: f64) -> Piece {
        match *self {
            Carrier::Seg(a, b) => Piece::Line(a.lerp(b, t0), a.lerp(b, t1)),
            Carrier::Arc(g) => Piece::Arc(ArcGeom {
                start_angle: g.start_angle + t0,
                sweep: t1 - t0,
                ..g
            }),
            Carrier::Circle(c, r) => Piece::Arc(ArcGeom {
                center: c,
                radius: r,
                start_angle: t0,
                sweep: t1 - t0,
            }),
            Carrier::Ellipse(g) => Piece::Ellipse {
                g,
                t0,
                sweep: t1 - t0,
            },
            Carrier::EllipseArc(g) => Piece::Ellipse {
                g: g.e,
                t0: g.t0 + t0,
                sweep: t1 - t0,
            },
            Carrier::Bezier(g) => Piece::Bezier(g.sub(t0, t1)),
        }
    }

    fn bounds(&self) -> (Vec2, Vec2) {
        match *self {
            Carrier::Seg(a, b) => (a.min(b), a.max(b)),
            Carrier::Arc(g) => g.bounds(),
            Carrier::Circle(c, r) => (c - Vec2::new(r, r), c + Vec2::new(r, r)),
            Carrier::Ellipse(g) => g.bounds(),
            Carrier::EllipseArc(g) => g.bounds(),
            Carrier::Bezier(g) => g.bounds(),
        }
    }

    /// Where a Bézier curve crosses another carrier. Against a line, circle or ellipse: sign
    /// changes of the other's [`Carrier::side`] along the Bézier, refined by bisection. Against
    /// another Bézier: crossings of their polylines refined by Newton's method.
    fn bezier_crossings(g: &BezierGeom, o: &Carrier) -> Vec<Vec2> {
        const N: usize = 512;
        let mut out = Vec::new();
        if let Carrier::Bezier(h) = o {
            let pa: Vec<Vec2> = (0..=N).map(|i| g.point_at(i as f64 / N as f64)).collect();
            let pb: Vec<Vec2> = (0..=N).map(|i| h.point_at(i as f64 / N as f64)).collect();
            for i in 0..N {
                let (a0, a1) = (pa[i], pa[i + 1]);
                let (lo, hi) = (a0.min(a1), a0.max(a1));
                for j in 0..N {
                    let (b0, b1) = (pb[j], pb[j + 1]);
                    if b0.max(b1).x < lo.x || b0.min(b1).x > hi.x || b0.max(b1).y < lo.y || b0.min(b1).y > hi.y {
                        continue;
                    }
                    let (r, q) = (a1 - a0, b1 - b0);
                    let den = r.cross(q);
                    if den.abs() < 1e-300 {
                        continue;
                    }
                    let u = (b0 - a0).cross(q) / den;
                    let v = (b0 - a0).cross(r) / den;
                    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                        continue;
                    }
                    // Newton on g(s) − h(t) = 0.
                    let (mut s, mut t) = ((i as f64 + u) / N as f64, (j as f64 + v) / N as f64);
                    for _ in 0..20 {
                        let f = g.point_at(s) - h.point_at(t);
                        let (ds, dt) = (g.tangent_at(s), -h.tangent_at(t));
                        let det = ds.cross(dt);
                        if det.abs() < 1e-300 {
                            break;
                        }
                        let (es, et) = (f.cross(dt) / det, ds.cross(f) / det);
                        s = (s - es).clamp(0.0, 1.0);
                        t = (t - et).clamp(0.0, 1.0);
                        if es.abs() + et.abs() < 1e-15 {
                            break;
                        }
                    }
                    out.push(g.point_at(s));
                }
            }
            return out;
        }
        let f = |t: f64| o.side(g.point_at(t));
        let mut prev = f(0.0);
        if prev == 0.0 {
            out.push(g.p[0]);
        }
        for i in 1..=N {
            let (ta, tb) = ((i - 1) as f64 / N as f64, i as f64 / N as f64);
            let cur = f(tb);
            if cur == 0.0 {
                out.push(g.point_at(tb));
            } else if prev != 0.0 && prev.signum() != cur.signum() {
                let (mut lo, mut hi, mut flo) = (ta, tb, prev);
                for _ in 0..60 {
                    let mid = (lo + hi) / 2.0;
                    let fm = f(mid);
                    if fm.signum() == flo.signum() {
                        lo = mid;
                        flo = fm;
                    } else {
                        hi = mid;
                    }
                }
                out.push(g.point_at((lo + hi) / 2.0));
            }
            prev = cur;
        }
        out
    }

    /// Where an ellipse crosses another carrier (its whole curve): sign changes of the other's
    /// [`Carrier::side`] around the ellipse, refined by bisection.
    fn ellipse_crossings(g: &EllipseGeom, o: &Carrier) -> Vec<Vec2> {
        const N: usize = 720;
        let f = |t: f64| o.side(g.point_at(t));
        let mut out = Vec::new();
        let mut prev = f(0.0);
        for i in 1..=N {
            let (ta, tb) = ((i - 1) as f64 * TAU / N as f64, i as f64 * TAU / N as f64);
            let cur = f(tb);
            if prev == 0.0 {
                out.push(g.point_at(ta));
            } else if prev.signum() != cur.signum() && cur != 0.0 {
                let (mut lo, mut hi, mut flo) = (ta, tb, prev);
                for _ in 0..60 {
                    let mid = (lo + hi) / 2.0;
                    let fm = f(mid);
                    if fm.signum() == flo.signum() {
                        lo = mid;
                        flo = fm;
                    } else {
                        hi = mid;
                    }
                }
                out.push(g.point_at((lo + hi) / 2.0));
            }
            prev = cur;
        }
        out
    }

    /// Where the full carriers (infinite line or whole circle) cross: 0, 1 (tangent) or 2
    /// points. Pairs that miss or cross by less than `tol` are tangent: one point.
    fn crossings(&self, o: &Carrier, tol: f64) -> Vec<Vec2> {
        if let Carrier::Bezier(g) = self {
            return Self::bezier_crossings(g, o);
        }
        if let Carrier::Bezier(g) = o {
            return Self::bezier_crossings(g, self);
        }
        if let Carrier::Ellipse(g) | Carrier::EllipseArc(EllipseArcGeom { e: g, .. }) = self {
            return Self::ellipse_crossings(g, o);
        }
        if let Carrier::Ellipse(g) | Carrier::EllipseArc(EllipseArcGeom { e: g, .. }) = o {
            return Self::ellipse_crossings(g, self);
        }
        enum C {
            L(Vec2, Vec2),
            O(Vec2, f64),
        }
        let full = |c: &Carrier| match *c {
            Carrier::Seg(a, b) => C::L(a, (b - a).normalize()),
            Carrier::Arc(g) => C::O(g.center, g.radius),
            Carrier::Circle(c, r) => C::O(c, r),
            Carrier::Ellipse(_) | Carrier::EllipseArc(_) | Carrier::Bezier(_) => unreachable!("handled above"),
        };
        match (full(self), full(o)) {
            (C::L(p, d), C::L(q, e)) => {
                let den = d.cross(e);
                if den.abs() < 1e-12 {
                    return vec![];
                }
                vec![p + d * ((q - p).cross(e) / den)]
            }
            (C::L(p, d), C::O(c, r)) | (C::O(c, r), C::L(p, d)) => {
                let foot = p + d * (c - p).dot(d);
                let off = foot - c;
                let h2 = r * r - off.dot(off);
                if h2 < 0.0 {
                    return if off.length() - r < tol {
                        vec![c + off.normalize() * r]
                    } else {
                        vec![]
                    };
                }
                let h = h2.sqrt();
                if h < tol {
                    vec![foot]
                } else {
                    vec![foot - d * h, foot + d * h]
                }
            }
            (C::O(c0, r0), C::O(c1, r1)) => {
                let d = c0.distance(c1);
                if d < tol {
                    return vec![];
                }
                let u = (c1 - c0) / d;
                if (d - (r0 + r1)).abs() < tol {
                    return vec![c0 + u * r0];
                }
                if (d - (r0 - r1).abs()).abs() < tol {
                    let sign = if r0 >= r1 { 1.0 } else { -1.0 };
                    return vec![c0 + u * (r0 * sign)];
                }
                if d > r0 + r1 || d < (r0 - r1).abs() {
                    return vec![];
                }
                let a = (r0 * r0 - r1 * r1 + d * d) / (2.0 * d);
                let h = (r0 * r0 - a * a).max(0.0).sqrt();
                let m = c0 + u * a;
                vec![m + u.perp() * h, m - u.perp() * h]
            }
        }
    }
}

/// One piece of the planar graph between two vertices.
struct Edge {
    curve: CurveId,
    from: usize,
    to: usize,
    piece: Piece,
    /// From `from` to `to`, both included.
    path: Vec<Vec2>,
}

/// A face cycle: its polygon, pieces and the curves along it.
struct Cycle {
    poly: Vec<Vec2>,
    pieces: Vec<Piece>,
    curves: Vec<CurveId>,
    /// The curve of each segment of `poly`.
    seg: Vec<CurveId>,
    component: usize,
}

/// The regular curves of a sketch as carriers.
fn carriers(s: &Sketch) -> Vec<(CurveId, Carrier)> {
    let mut srcs: Vec<(CurveId, Carrier)> = Vec::new();
    for (id, c) in &s.curves {
        if c.construction {
            continue;
        }
        match c.kind {
            CurveKind::Line { a, b } => {
                let (pa, pb) = (s.pos(a), s.pos(b));
                if a == b || pa.distance(pb) < 1e-12 {
                    continue;
                }
                // The same segment twice (two lines between the same points) is one edge.
                let dup = srcs.iter().any(|(_, k)| {
                    matches!(*k, Carrier::Seg(x, y)
                        if (x.distance(pa) < 1e-9 && y.distance(pb) < 1e-9)
                            || (x.distance(pb) < 1e-9 && y.distance(pa) < 1e-9))
                });
                if !dup {
                    srcs.push((id, Carrier::Seg(pa, pb)));
                }
            }
            CurveKind::Arc { start, end, .. } => {
                if start == end {
                    continue;
                }
                if let Some(g) = s.arc_geom(id)
                    && g.radius > 1e-12
                {
                    srcs.push((id, Carrier::Arc(g)));
                }
            }
            CurveKind::Circle { center, radius } => {
                if radius > 1e-12 {
                    srcs.push((id, Carrier::Circle(s.pos(center), radius)));
                }
            }
            CurveKind::Spline { .. } => {
                for b in s.spline_spans(id).unwrap_or_default() {
                    let g = BezierGeom::new(b);
                    if b[0].distance(b[3]) > 1e-12 || g.length() > 1e-12 {
                        srcs.push((id, Carrier::Bezier(g)));
                    }
                }
            }
            CurveKind::Ellipse { .. } | CurveKind::EllipseOffset { .. } => {
                if let Some(g) = s.ellipse_geom(id)
                    && g.major() > 1e-12
                    && g.minor.abs() > 1e-12
                    && g.offset_is_smooth()
                {
                    srcs.push((id, Carrier::Ellipse(g)));
                }
            }
            CurveKind::Bezier { a, b, .. } => {
                if let Some(g) = s.bezier_geom(id)
                    && (a != b || g.length() > 1e-9)
                {
                    srcs.push((id, Carrier::Bezier(g)));
                }
            }
            CurveKind::EllipseArc { .. } => {
                if let Some(g) = s.ellipse_arc_geom(id)
                    && g.e.major() > 1e-12
                    && g.e.minor.abs() > 1e-12
                {
                    srcs.push((id, Carrier::EllipseArc(g)));
                }
            }
        }
    }
    // Text outlines (S16): each contour's segments, one id per run between sharp corners.
    for (tid, _) in &s.texts {
        for (ci, contour) in crate::text::outlines(s, tid).iter().enumerate() {
            let runs = crate::text::contour_runs(contour);
            let n = contour.len();
            for i in 0..n {
                let (a, b) = (contour[i], contour[(i + 1) % n]);
                if a.distance(b) > 1e-9 {
                    srcs.push((crate::text::piece_id(tid, ci, runs[i]), Carrier::Seg(a, b)));
                }
            }
        }
    }
    // Imprinted face edges (S21), unless a curve is there already.
    let sketch_count = srcs.len();
    for im in &s.imprint {
        let c = match im.shape {
            ImprintShape::Line(a, b) => Carrier::Seg(a, b),
            ImprintShape::Circle(c, r) => Carrier::Circle(c, r),
            ImprintShape::Arc { center, radius, start_angle, sweep } => Carrier::Arc(ArcGeom {
                center,
                radius,
                start_angle,
                sweep,
            }),
        };
        let same = |k: &Carrier| match (*k, c) {
            (Carrier::Seg(x, y), Carrier::Seg(p, q)) => {
                (x.distance(p) < 1e-6 && y.distance(q) < 1e-6)
                    || (x.distance(q) < 1e-6 && y.distance(p) < 1e-6)
            }
            (Carrier::Circle(x, r), Carrier::Circle(y, q)) => x.distance(y) < 1e-6 && (r - q).abs() < 1e-6,
            (Carrier::Arc(g), Carrier::Arc(h)) => {
                g.center.distance(h.center) < 1e-6
                    && (g.radius - h.radius).abs() < 1e-6
                    && ((g.start().distance(h.start()) < 1e-6 && g.end().distance(h.end()) < 1e-6)
                        || (g.start().distance(h.end()) < 1e-6 && g.end().distance(h.start()) < 1e-6))
            }
            _ => false,
        };
        if !srcs[..sketch_count].iter().any(|(_, k)| same(k)) {
            srcs.push((im.id, c));
        }
    }
    srcs
}

/// Every closed region of the sketch.
/// [`regions`], remembered for the last few sketches: the view, the Part Studio's region list
/// and the dimension knockouts all ask for the same sketch's regions in a frame, and finding
/// them in a 500-entity sketch takes a few milliseconds.
pub fn regions_shared(s: &Sketch) -> std::sync::Arc<Vec<Region>> {
    use std::sync::{Arc, Mutex};
    type Memo = Vec<(Sketch, Arc<Vec<Region>>)>;
    static MEMO: Mutex<Memo> = Mutex::new(Vec::new());
    const KEEP: usize = 6;
    if let Ok(mut m) = MEMO.lock()
        && let Some(i) = m.iter().position(|(k, _)| k == s)
    {
        let hit = m.remove(i);
        let r = hit.1.clone();
        m.insert(0, hit);
        return r;
    }
    let r = Arc::new(regions(s));
    if let Ok(mut m) = MEMO.lock() {
        m.insert(0, (s.clone(), r.clone()));
        m.truncate(KEEP);
    }
    r
}

pub fn regions(s: &Sketch) -> Vec<Region> {
    // The imprinted curves, looked up for every edge and region below (a face of a perfboard
    // imprints a thousand edges: searching the list each time took 36 s).
    let imprint: std::collections::HashSet<CurveId> = s.imprint.iter().map(|i| i.id).collect();
    let imprinted = |c: CurveId| imprint.contains(&c);
    let srcs = carriers(s);
    if srcs.iter().all(|(c, _)| imprinted(*c)) {
        return Vec::new();
    }
    // Points closer than this are one vertex (relative to the sketch's size).
    let (lo, hi) = srcs.iter().fold(
        (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN)),
        |(lo, hi), (_, c)| {
            let (a, b) = c.bounds();
            (lo.min(a), hi.max(b))
        },
    );
    let tol = 1e-5 * (hi - lo).length().max(1.0);

    // 1. Split parameters: each curve's own ends, the ends of other curves lying on it, and
    //    where it crosses other curves.
    let mut params: Vec<Vec<f64>> = srcs
        .iter()
        .map(|(_, c)| match *c {
            Carrier::Seg(..) => vec![0.0, 1.0],
            Carrier::Arc(g) => vec![0.0, g.sweep],
            Carrier::Bezier(_) => vec![0.0, 1.0],
            Carrier::EllipseArc(g) => vec![0.0, g.sweep],
            Carrier::Circle(..) | Carrier::Ellipse(_) => vec![],
        })
        .collect();
    let ends: Vec<Vec2> = srcs
        .iter()
        .flat_map(|(_, c)| match *c {
            Carrier::Seg(a, b) => vec![a, b],
            Carrier::Arc(g) => vec![g.start(), g.end()],
            Carrier::Bezier(g) => vec![g.p[0], g.p[3]],
            Carrier::EllipseArc(g) => vec![g.start(), g.end()],
            Carrier::Circle(..) | Carrier::Ellipse(_) => vec![],
        })
        .collect();
    let boxes: Vec<(Vec2, Vec2)> = srcs.iter().map(|(_, c)| c.bounds()).collect();
    let near_box = |i: usize, p: Vec2| {
        let (a, b) = boxes[i];
        p.x >= a.x - tol && p.x <= b.x + tol && p.y >= a.y - tol && p.y <= b.y + tol
    };
    for (i, (_, c)) in srcs.iter().enumerate() {
        for p in &ends {
            if !near_box(i, *p) {
                continue;
            }
            let (t, d) = c.project(*p);
            if d < tol {
                params[i].push(t);
            }
        }
    }
    for i in 0..srcs.len() {
        for j in i + 1..srcs.len() {
            let (a0, a1) = boxes[i];
            let (b0, b1) = boxes[j];
            if a1.x < b0.x - tol || b1.x < a0.x - tol || a1.y < b0.y - tol || b1.y < a0.y - tol {
                continue;
            }
            for p in srcs[i].1.crossings(&srcs[j].1, tol) {
                let (ti, di) = srcs[i].1.project(p);
                let (tj, dj) = srcs[j].1.project(p);
                if di < tol && dj < tol {
                    params[i].push(ti);
                    params[j].push(tj);
                }
            }
        }
    }

    // 2. Vertices (every split point, merged within `tol`) and the pieces between them.
    // (A grid of `2 tol` cells finds a vertex within `tol` among the 3×3 cells around.)
    let mut verts: Vec<Vec2> = Vec::new();
    let mut grid: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    let cell = |p: Vec2| ((p.x / (2.0 * tol)).floor() as i64, (p.y / (2.0 * tol)).floor() as i64);
    let mut vertex = |p: Vec2| -> usize {
        let (cx, cy) = cell(p);
        let mut best: Option<usize> = None;
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = grid.get(&(cx + dx, cy + dy)) {
                    for &k in list {
                        if verts[k].distance(p) < tol && best.is_none_or(|b| k < b) {
                            best = Some(k);
                        }
                    }
                }
            }
        }
        if let Some(k) = best {
            return k;
        }
        verts.push(p);
        grid.entry((cx, cy)).or_default().push(verts.len() - 1);
        verts.len() - 1
    };
    let mut edges: Vec<Edge> = Vec::new();
    let mut circles: Vec<(CurveId, Carrier)> = Vec::new();
    for (i, (curve, c)) in srcs.iter().enumerate() {
        let mut ts = std::mem::take(&mut params[i]);
        ts.sort_by(f64::total_cmp);
        let circle = c.closed();
        if circle && ts.is_empty() {
            circles.push((*curve, *c));
            continue;
        }
        // Consecutive splits at the same vertex are one.
        let mut stops: Vec<(f64, usize)> = Vec::new();
        for t in ts {
            let v = vertex(c.at(t));
            match stops.last() {
                Some(&(_, last)) if last == v => {}
                _ => stops.push((t, v)),
            }
        }
        if circle {
            // Around the circle, back to the first stop.
            if stops.len() > 1 && stops[0].1 == stops[stops.len() - 1].1 {
                stops.pop();
            }
            let (t0, v0) = stops[0];
            stops.push((t0 + TAU, v0));
        }
        for w in stops.windows(2) {
            let ((t0, v0), (t1, v1)) = (w[0], w[1]);
            let piece = c.piece(t0, t1);
            if piece.length() < tol {
                continue;
            }
            edges.push(Edge {
                curve: *curve,
                from: v0,
                to: v1,
                path: piece.path(),
                piece,
            });
        }
    }

    // Overlapping curves (a line drawn along part of another, collinear) give the same piece
    // twice: keep one, a drawn curve's over an imprinted edge's, so the graph stays planar.
    {
        let mid = |e: &Edge| e.path.get(e.path.len() / 2).copied().unwrap_or_else(|| e.piece.start());
        let mut keep: Vec<Edge> = Vec::with_capacity(edges.len());
        for e in edges {
            let same = keep.iter().position(|k| {
                ((k.from == e.from && k.to == e.to) || (k.from == e.to && k.to == e.from))
                    && (k.piece.length() - e.piece.length()).abs() < tol
                    && k.path.iter().map(|p| p.distance(mid(&e))).fold(f64::MAX, f64::min) < tol * 10.0
            });
            match same {
                Some(i) if imprinted(keep[i].curve) && !imprinted(e.curve) => keep[i] = e,
                Some(_) => {}
                None => keep.push(e),
            }
        }
        edges = keep;
    }

    // 3. Prune dangling edges until every vertex has degree ≥ 2.
    loop {
        let mut degree: HashMap<usize, usize> = HashMap::new();
        for e in &edges {
            *degree.entry(e.from).or_default() += 1;
            *degree.entry(e.to).or_default() += 1;
        }
        let before = edges.len();
        edges.retain(|e| degree[&e.from] >= 2 && degree[&e.to] >= 2);
        if edges.len() == before {
            break;
        }
    }

    // 4. Connected components (union-find over vertices).
    let mut parent: Vec<usize> = (0..verts.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for e in &edges {
        let (a, b) = (find(&mut parent, e.from), find(&mut parent, e.to));
        if a != b {
            parent[a] = b;
        }
    }
    let mut component_ids: HashMap<usize, usize> = HashMap::new();
    let mut component_of = |v: usize, parent: &mut [usize]| {
        let root = find(parent, v);
        let n = component_ids.len();
        *component_ids.entry(root).or_insert(n)
    };

    // 5. Half-edges, sorted by the direction they leave their vertex.
    // Half-edge 2i runs along edge i; 2i+1 runs back.
    let half_from = |h: usize| {
        let e = &edges[h / 2];
        if h.is_multiple_of(2) { e.from } else { e.to }
    };
    let half_piece = |h: usize| {
        let e = &edges[h / 2];
        if h.is_multiple_of(2) {
            e.piece
        } else {
            e.piece.reversed()
        }
    };
    let half_path = |h: usize| -> Vec<Vec2> {
        let e = &edges[h / 2];
        if h.is_multiple_of(2) {
            e.path.clone()
        } else {
            e.path.iter().rev().copied().collect()
        }
    };
    let mut outgoing: HashMap<usize, Vec<(f64, usize)>> = HashMap::new();
    for h in 0..edges.len() * 2 {
        outgoing
            .entry(half_from(h))
            .or_default()
            .push((half_piece(h).leave_key(), h));
    }
    for list in outgoing.values_mut() {
        list.sort_by(|a, b| a.0.total_cmp(&b.0));
    }

    // 6. Trace faces: after arriving at v along h, leave along the half-edge just clockwise of
    //    h's twin.
    let mut used = vec![false; edges.len() * 2];
    let mut bounded: Vec<Cycle> = Vec::new();
    let mut outers: Vec<Cycle> = Vec::new();
    for start in 0..edges.len() * 2 {
        if used[start] {
            continue;
        }
        let mut poly = Vec::new();
        let mut pieces = Vec::new();
        let mut curves = Vec::new();
        let mut seg = Vec::new();
        let mut h = start;
        let mut ok = true;
        for _ in 0..=edges.len() * 2 {
            used[h] = true;
            let path = half_path(h);
            poly.extend_from_slice(&path[..path.len() - 1]);
            seg.extend(std::iter::repeat_n(edges[h / 2].curve, path.len() - 1));
            pieces.push(half_piece(h));
            curves.push(edges[h / 2].curve);
            let twin = h ^ 1;
            let v = half_from(twin);
            let list = &outgoing[&v];
            let i = list.iter().position(|(_, x)| *x == twin).unwrap_or(0);
            h = list[(i + list.len() - 1) % list.len()].1;
            if h == start {
                break;
            }
            if used[h] {
                ok = false;
                break;
            }
        }
        if !ok || poly.len() < 3 {
            continue;
        }
        let component = component_of(half_from(start), &mut parent);
        let area = pieces_area(&pieces);
        let cycle = Cycle {
            poly,
            pieces,
            curves,
            seg,
            component,
        };
        if area > 1e-12 {
            bounded.push(cycle);
        } else if area < -1e-12 {
            outers.push(cycle);
        }
    }

    // Circles nothing touches: a bounded face and an outer boundary of their own component.
    let first_circle_component = component_ids.len();
    for (next_component, (id, carrier)) in (first_circle_component..).zip(circles) {
        let piece = carrier.piece(0.0, TAU);
        let mut poly = match piece {
            Piece::Arc(g) => g.tessellate(STEP, 16),
            _ => piece.path(),
        };
        poly.pop();
        let reversed: Vec<Vec2> = poly.iter().rev().copied().collect();
        let seg = vec![id; poly.len()];
        bounded.push(Cycle {
            poly,
            pieces: vec![piece],
            curves: vec![id],
            seg: seg.clone(),
            component: next_component,
        });
        outers.push(Cycle {
            poly: reversed,
            pieces: vec![piece.reversed()],
            curves: vec![id],
            seg,
            component: next_component,
        });
    }

    // 7. Holes: each component's outer boundary is a hole in the smallest region of another
    //    component that contains it.
    let mut out: Vec<Region> = bounded
        .iter()
        .map(|c| Region {
            outer: c.poly.clone(),
            holes: Vec::new(),
            curves: c.curves.clone(),
            outer_curves: c.seg.clone(),
            hole_curves: Vec::new(),
            outer_pieces: c.pieces.clone(),
            hole_pieces: Vec::new(),
            hole_piece_curves: Vec::new(),
        })
        .collect();
    let bounded_boxes: Vec<(Vec2, Vec2, f64)> = bounded
        .iter()
        .map(|b| {
            let (lo, hi) = b.poly.iter().fold(
                (Vec2::new(f64::MAX, f64::MAX), Vec2::new(f64::MIN, f64::MIN)),
                |(lo, hi), p| (lo.min(*p), hi.max(*p)),
            );
            (lo, hi, pieces_area(&b.pieces))
        })
        .collect();
    for o in &outers {
        let probe = o.poly[0];
        let container = bounded
            .iter()
            .enumerate()
            .filter(|(i, b)| {
                let (lo, hi, _) = bounded_boxes[*i];
                b.component != o.component
                    && probe.x >= lo.x
                    && probe.x <= hi.x
                    && probe.y >= lo.y
                    && probe.y <= hi.y
                    && point_in_polygon(probe, &b.poly)
            })
            .min_by(|a, b| bounded_boxes[a.0].2.total_cmp(&bounded_boxes[b.0].2))
            .map(|(i, _)| i);
        if let Some(i) = container {
            out[i].holes.push(o.poly.clone());
            out[i].hole_curves.push(o.seg.clone());
            out[i].hole_pieces.push(o.pieces.clone());
            out[i].hole_piece_curves.push(o.curves.clone());
        }
    }
    // A face's own outline is a region of the sketch only when sketch geometry bounds it (the
    // overlaps of S21.1), or when the sketch's curves bound nothing but touch its edge (Onshape
    // offers the face then: a revolve profile drawn from a hole's rim).
    if !s.imprint.is_empty() {
        let drawn_points: Vec<Vec2> = s
            .curves
            .values()
            .filter(|c| !c.construction)
            .flat_map(|c| match c.kind {
                crate::CurveKind::Line { a, b } => vec![a, b],
                crate::CurveKind::Arc { start, end, .. } | crate::CurveKind::Spline { start, end } => vec![start, end],
                _ => Vec::new(),
            })
            .filter_map(|p| s.points.get(p).map(|p| p.pos))
            .collect();
        let bound: Vec<bool> = out.iter().map(|r| r.outer_curves.iter().chain(r.hole_curves.iter().flatten()).any(|c| !imprinted(*c))).collect();
        // Only when the sketch's own curves bound no region (a lone line from a rim): where they
        // do, the face's outline stays out, as before.
        let lone = !bound.contains(&true);
        // A face outline inside a loop of sketch geometry is a region of it in Onshape too. The
        // loop is of drawn curves alone: a hole through the face that drawn lines merely cross
        // near is no region (its grey fill hid the hole while sketching on the face).
        let drawn_loops = std::sync::OnceLock::new();
        let drawn_loops = || {
            drawn_loops.get_or_init(|| {
                let mut d = s.clone();
                d.imprint.clear();
                regions(&d)
            })
        };
        // A face piece across an imprinted edge from a region the sketch bounds: that region is
        // closed by the face's edge, and in Onshape the face on the other side of it is a region
        // too (an extrude can take both: the profile and the face it rests on).
        let edges_of = |r: &Region| -> std::collections::HashSet<CurveId> { r.outer_curves.iter().chain(r.hole_curves.iter().flatten()).copied().filter(|c| imprinted(*c)).collect() };
        // (Its outer edge: a hole the face has through the region is no region.)
        let bound_edges: std::collections::HashSet<CurveId> = out.iter().zip(&bound).filter(|(_, b)| **b).flat_map(|(r, _)| r.outer_curves.iter().copied().filter(|c| imprinted(*c))).collect();
        let keep: Vec<bool> = out
            .iter()
            .enumerate()
            .map(|(i, r)| {
                bound[i]
                    || edges_of(r).iter().any(|c| bound_edges.contains(c))
                    || lone && std::iter::once(&r.outer).chain(r.holes.iter()).any(|poly| drawn_points.iter().any(|p| polyline_distance(poly, *p) < tol))
                    || inner_point(r).is_some_and(|p| {
                        out.iter().enumerate().any(|(j, q)| j != i && bound[j] && point_in_polygon(p, &q.outer))
                            && drawn_loops().iter().any(|q| point_in_polygon(p, &q.outer))
                    })
            })
            .collect();
        let mut k = keep.into_iter();
        out.retain(|_| k.next().unwrap_or(true));
    }
    out
}

/// How far `p` is from the closed polygon `poly`'s boundary.
fn polyline_distance(poly: &[Vec2], p: Vec2) -> f64 {
    let n = poly.len();
    (0..n)
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % n]);
            let d = b - a;
            let t = ((p - a).dot(d) / d.dot(d).max(1e-18)).clamp(0.0, 1.0);
            p.distance(a + d * t)
        })
        .fold(f64::MAX, f64::min)
}

/// A point inside a region: on a horizontal line across it, the middle of the widest stretch
/// inside the outer polygon and outside the holes (even-odd over all its polygons). Linear in
/// its edges: triangulating it instead took seconds for a board with 825 holes (ear clipping
/// bridges each hole into the outline).
fn inner_point(r: &Region) -> Option<Vec2> {
    let (lo, hi) = r.outer.iter().fold((f64::MAX, f64::MIN), |(l, h), p| (l.min(p.y), h.max(p.y)));
    if hi.partial_cmp(&lo) != Some(std::cmp::Ordering::Greater) {
        return None;
    }
    // Lines at other heights when one only grazes the region (through vertices, along an edge).
    for k in [0.5, 0.37, 0.63, 0.21, 0.79, 0.11, 0.89] {
        let y = lo + (hi - lo) * k;
        let mut xs: Vec<f64> = Vec::new();
        for poly in std::iter::once(&r.outer).chain(r.holes.iter()) {
            let n = poly.len();
            for i in 0..n {
                let (a, b) = (poly[i], poly[(i + 1) % n]);
                if (a.y > y) != (b.y > y) {
                    xs.push(a.x + (y - a.y) * (b.x - a.x) / (b.y - a.y));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let widest = xs.as_chunks::<2>().0.iter().map(|p| (p[0], p[1])).max_by(|a, b| (a.1 - a.0).total_cmp(&(b.1 - b.0)));
        if let Some((x0, x1)) = widest
            && x1 - x0 > 1e-9
        {
            return Some(Vec2::new(0.5 * (x0 + x1), y));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SketchOp;
    use std::f64::consts::PI;

    fn poly(s: &mut Sketch, pts: &[(f64, f64)], closed: bool) {
        SketchOp::AddPolyline {
            points: pts.iter().map(|&(x, y)| Vec2::new(x, y)).collect(),
            closed,
            construction: false,
            label: "Add line",
        }
        .apply(s)
        .unwrap();
    }

    fn rect(s: &mut Sketch, x: f64, y: f64, w: f64, h: f64) {
        poly(s, &[(x, y), (x + w, y), (x + w, y + h), (x, y + h)], true);
    }

    fn circle(s: &mut Sketch, x: f64, y: f64, r: f64) {
        SketchOp::AddCircle {
            center: Vec2::new(x, y),
            radius: r,
            construction: false,
        }
        .apply(s)
        .unwrap();
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    /// A 40 × 40 face outline imprinted on the sketch (S21.1).
    fn imprint_square(s: &mut Sketch) {
        let c = [
            Vec2::new(0.0, 0.0),
            Vec2::new(40.0, 0.0),
            Vec2::new(40.0, 40.0),
            Vec2::new(0.0, 40.0),
        ];
        s.imprint = (0..4)
            .map(|i| crate::Imprint {
                id: crate::synthetic_curve(1, i as u32),
                shape: crate::ImprintShape::Line(c[i], c[(i + 1) % 4]),
                link: None,
            })
            .collect();
    }

    #[test]
    fn imprinted_face_edges_split_regions() {
        let mut s = Sketch::new();
        imprint_square(&mut s);
        // The face alone is no region of the sketch.
        assert!(regions(&s).is_empty());
        // A circle over the face's right edge: inside and outside the face, and the face
        // around it.
        circle(&mut s, 40.0, 20.0, 10.0);
        let r = regions(&s);
        assert_eq!(r.len(), 3, "{r:?}");
        let mut areas: Vec<f64> = r.iter().map(|r| r.area()).collect();
        areas.sort_by(f64::total_cmp);
        let half = PI * 100.0 / 2.0;
        assert!(close(areas[0], half) && close(areas[1], half), "{areas:?}");
        assert!(close(areas[2], 1600.0 - half), "{areas:?}");
        // The ids of imprinted edges are not sketch curves.
        assert!(r.iter().any(|r| r.curves.iter().any(|c| crate::is_synthetic(*c))));
        // Without imprinting (the flag clears the imprint): only the circle.
        s.imprint.clear();
        assert_eq!(regions(&s).len(), 1);
    }

    #[test]
    fn a_profile_along_two_face_edges_is_a_region() {
        // P3H.6 (PCB10 step 6): a corner profile drawn on a face along two of its edges (the
        // board's bottom-left corner): its sides lie on part of the imprinted edges. Both the
        // profile and the rest of the face are regions, the profile bounded by its drawn sides.
        let mut s = Sketch::new();
        imprint_square(&mut s);
        rect(&mut s, 0.0, 0.0, 12.0, 9.0);
        let r = regions(&s);
        assert_eq!(r.len(), 2, "{r:?}");
        let mut areas: Vec<f64> = r.iter().map(|r| r.area()).collect();
        areas.sort_by(f64::total_cmp);
        assert!(close(areas[0], 108.0) && close(areas[1], 1600.0 - 108.0), "{areas:?}");
        let small = r.iter().find(|r| close(r.area(), 108.0)).unwrap();
        assert!(small.curves.iter().all(|c| !crate::is_synthetic(*c)), "the profile's own sides");
        // Two drawn rectangles sharing a corner likewise.
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        rect(&mut s, 0.0, 0.0, 3.0, 2.0);
        let mut areas: Vec<f64> = regions(&s).iter().map(|r| r.area()).collect();
        areas.sort_by(f64::total_cmp);
        assert!(areas.len() == 2 && close(areas[0], 6.0) && close(areas[1], 94.0), "{areas:?}");
    }

    #[test]
    fn a_circle_inside_a_face_leaves_a_ring() {
        let mut s = Sketch::new();
        imprint_square(&mut s);
        circle(&mut s, 20.0, 20.0, 5.0);
        let r = regions(&s);
        assert_eq!(r.len(), 2);
        assert!(r.iter().any(|r| r.holes.len() == 1 && close(r.area(), 1600.0 - PI * 25.0)));
        // An edge drawn on top of an imprinted one is used instead.
        poly(&mut s, &[(0.0, 0.0), (40.0, 0.0)], false);
        assert_eq!(regions(&s).len(), 2);
    }

    #[test]
    fn face_region_touched_by_a_curve() {
        // A line drawn outward from a point on an imprinted circle (a revolve profile from a
        // hole's rim): the circle's face is a region; without the line it isn't.
        let mut s = Sketch::new();
        s.imprint = vec![crate::Imprint { id: crate::synthetic_curve(1, 0), shape: crate::ImprintShape::Circle(Vec2::new(7.0, -9.5), 0.3), link: None }];
        assert_eq!(regions(&s).len(), 0);
        poly(&mut s, &[(7.0, -9.8), (6.62, -9.8)], false);
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        assert!(close(r[0].area(), PI * 0.09));
    }

    #[test]
    fn a_face_across_the_edge_closing_a_profile_is_a_region() {
        // A U drawn down onto the face's top edge, its opening closed by that edge (a sketch
        // on a small cap, its profile reaching out past the cap): the profile, and the face
        // below the edge it rests on (Onshape's extrude took both).
        let mut s = Sketch::new();
        imprint_square(&mut s);
        poly(&mut s, &[(10.0, 40.0), (10.0, 60.0), (30.0, 60.0), (30.0, 40.0)], false);
        let r = regions(&s);
        let mut areas: Vec<f64> = r.iter().map(Region::area).collect();
        areas.sort_by(f64::total_cmp);
        assert!(areas.len() == 2 && close(areas[0], 400.0) && close(areas[1], 1600.0), "{areas:?}");
    }

    #[test]
    fn a_face_outline_inside_sketch_geometry_is_a_region() {
        // A sketch loop drawn around the face the sketch is on: the ring between them and the
        // face inside it are both regions (the face alone, with nothing drawn round it, isn't).
        let mut s = Sketch::new();
        imprint_square(&mut s);
        assert!(regions(&s).is_empty());
        rect(&mut s, -10.0, -10.0, 60.0, 60.0);
        let r = regions(&s);
        assert_eq!(r.len(), 2, "{r:?}");
        let mut areas: Vec<f64> = r.iter().map(Region::area).collect();
        areas.sort_by(f64::total_cmp);
        assert!(close(areas[0], 1600.0) && close(areas[1], 3600.0 - 1600.0), "{areas:?}");
    }

    #[test]
    fn overlapping_collinear_lines() {
        // A line drawn along part of a longer one (an Onshape sketch had this): the shared
        // stretch is one boundary, so the L-shaped loop is still a region.
        let mut s = Sketch::new();
        for (a, b) in [
            ((100.0, -15.0), (0.0, -15.0)),
            ((30.0, -115.0), (30.0, -15.0)),
            ((30.0, -15.0), (30.0, -45.0)),
            ((30.0, -45.0), (34.0, -45.0)),
            ((34.0, -45.0), (34.0, -19.0)),
            ((34.0, -19.0), (60.0, -19.0)),
            ((60.0, -19.0), (60.0, -15.0)),
        ] {
            poly(&mut s, &[a, b], false);
        }
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        assert!(close(r[0].area(), 30.0 * 4.0 + 26.0 * 4.0));
    }

    #[test]
    fn rectangle_is_one_region() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 50.0, 30.0);
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        assert!(close(r[0].area(), 1500.0));
        assert_eq!(r[0].curves.len(), 4);
        assert_eq!(r[0].outer_curves.len(), r[0].outer.len());
        assert!(r[0].contains(Vec2::new(25.0, 15.0)));
        let (v, i) = r[0].triangulate();
        assert_eq!((v.len(), i.len()), (4, 6));
    }

    #[test]
    fn circle_is_a_region_with_its_exact_area() {
        let mut s = Sketch::new();
        circle(&mut s, 5.0, 5.0, 10.0);
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        // Exact, not the tessellated polygon's area.
        assert!((r[0].area() - PI * 100.0).abs() < 1e-9);
    }

    #[test]
    fn open_chain_has_no_region() {
        let mut s = Sketch::new();
        poly(&mut s, &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)], false);
        assert!(regions(&s).is_empty());
    }

    #[test]
    fn construction_geometry_encloses_nothing() {
        let mut s = Sketch::new();
        SketchOp::AddCircle {
            center: Vec2::ZERO,
            radius: 3.0,
            construction: true,
        }
        .apply(&mut s)
        .unwrap();
        assert!(regions(&s).is_empty());
    }

    #[test]
    fn two_touching_rectangles() {
        // Sharing a whole side (its two corners, with the side drawn twice).
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        rect(&mut s, 10.0, 0.0, 20.0, 10.0);
        let mut areas: Vec<f64> = regions(&s).iter().map(|r| r.area()).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2);
        assert!(close(areas[0], 100.0) && close(areas[1], 200.0));
        // Touching at a single corner.
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        rect(&mut s, 10.0, 10.0, 10.0, 10.0);
        let r = regions(&s);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|r| close(r.area(), 100.0)));
    }

    #[test]
    fn tail_on_a_rectangle_is_ignored() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 10.0, 10.0);
        poly(&mut s, &[(10.0, 10.0), (20.0, 20.0)], false);
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        assert!(close(r[0].area(), 100.0));
    }

    #[test]
    fn circle_inside_a_rectangle_is_a_hole() {
        let mut s = Sketch::new();
        rect(&mut s, 0.0, 0.0, 40.0, 40.0);
        circle(&mut s, 20.0, 20.0, 5.0);
        let r = regions(&s);
        assert_eq!(r.len(), 2);
        let square = r.iter().find(|r| r.curves.len() == 4).unwrap();
        assert_eq!(square.holes.len(), 1);
        assert_eq!(square.hole_curves[0].len(), square.holes[0].len());
        assert!(!square.contains(Vec2::new(20.0, 20.0)));
        assert!(square.contains(Vec2::new(2.0, 2.0)));
        // The hole is subtracted exactly.
        assert!((square.area() - (1600.0 - PI * 25.0)).abs() < 1e-9);
        assert_eq!(region_at(&r, Vec2::new(20.0, 20.0)), r.iter().position(|x| x.curves.len() == 1));
    }

    #[test]
    fn line_and_arc_region() {
        // A D shape: a line from (0,-5) to (0,5) and a half circle back around the right.
        let mut s = Sketch::new();
        poly(&mut s, &[(0.0, -5.0), (0.0, 5.0)], false);
        SketchOp::AddArc {
            center: Vec2::ZERO,
            start: Vec2::new(0.0, -5.0),
            end: Vec2::new(0.0, 5.0),
            construction: false,
        }
        .apply(&mut s)
        .unwrap();
        let r = regions(&s);
        assert_eq!(r.len(), 1);
        assert!((r[0].area() - PI * 25.0 / 2.0).abs() < 1e-9);
    }

    #[test]
    fn crossing_lines_make_regions_without_shared_points() {
        // Two rectangles overlapping like a plus sign: 5 regions (the middle and 4 arms).
        let mut s = Sketch::new();
        rect(&mut s, -30.0, -10.0, 60.0, 20.0);
        rect(&mut s, -10.0, -30.0, 20.0, 60.0);
        let r = regions(&s);
        assert_eq!(r.len(), 5);
        let total: f64 = r.iter().map(|r| r.area()).sum();
        assert!(close(total, 1200.0 + 1200.0 - 400.0));
        assert!(r.iter().any(|r| close(r.area(), 400.0)));
    }

    #[test]
    fn lines_ending_on_a_circle_split_it() {
        // A slot-like shape: a circle of radius 10 at the origin, and a rectangle's three
        // sides from its top (0,10) to its bottom (0,-10) around to x = 30. The regions are the
        // disc and the rectangle minus the right half disc (like exercise 1's plate).
        let mut s = Sketch::new();
        circle(&mut s, 0.0, 0.0, 10.0);
        poly(&mut s, &[(0.0, 10.0), (30.0, 10.0), (30.0, -10.0), (0.0, -10.0)], false);
        let r = regions(&s);
        let mut areas: Vec<f64> = r.iter().map(|r| r.area()).collect();
        areas.sort_by(f64::total_cmp);
        assert_eq!(areas.len(), 2, "{areas:?}");
        assert!((areas[0] - PI * 100.0).abs() < 1e-6, "{areas:?}");
        assert!((areas[1] - (600.0 - PI * 50.0)).abs() < 1e-6, "{areas:?}");
    }

    #[test]
    fn a_circle_crossed_by_a_line_is_two_regions() {
        let mut s = Sketch::new();
        circle(&mut s, 0.0, 0.0, 10.0);
        poly(&mut s, &[(-20.0, 0.0), (20.0, 0.0)], false);
        let r = regions(&s);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|r| (r.area() - PI * 50.0).abs() < 1e-6));
    }
}

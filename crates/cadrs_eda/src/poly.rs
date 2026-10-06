//! Copper and outline geometry as polygons (Clipper2, exact `i64` nanometres): strokes,
//! circles and pad shapes as regions, and the booleans and offsets zone fill and DRC use.
//!
//! A [`Region`] is a list of rings filled by the non-zero rule: outer rings counter-clockwise,
//! holes clockwise.

use crate::footprint::{Pad, PadShape};
use crate::geom;
use crate::graphics::{Fill, Geom, Shape};
use crate::units::{Nm, Pt};
use clipper2_rust::{
    EndType, FillRule, JoinType, Path64, Paths64, Point64, PointInPolygonResult, difference_64, inflate_paths_64, intersect_64,
    point_in_polygon, union_subjects_64,
};

pub type Ring = Vec<Pt>;
pub type Region = Vec<Ring>;

/// How far a curve's chords may stray (KiCad's default "maximum allowed deviation").
pub const MAX_ERROR: Nm = 5_000;

fn p64(p: Pt) -> Point64 {
    Point64 { x: p.x, y: p.y }
}

pub fn to_paths(r: &Region) -> Paths64 {
    r.iter().map(|ring| ring.iter().map(|p| p64(*p)).collect()).collect()
}

pub fn from_paths(p: &Paths64) -> Region {
    p.iter().map(|ring| ring.iter().map(|q| Pt::new(q.x, q.y)).collect()).collect()
}

fn arc_tol() -> f64 {
    MAX_ERROR as f64
}

/// An open polyline swept by a round pen of `width` (a track, a drawn line).
pub fn stroke(pts: &[Pt], width: Nm) -> Region {
    if pts.is_empty() || width <= 0 {
        return vec![];
    }
    let path: Path64 = if pts.len() == 1 { vec![p64(pts[0]), p64(pts[0])] } else { pts.iter().map(|p| p64(*p)).collect() };
    from_paths(&inflate_paths_64(&vec![path], width as f64 / 2.0, JoinType::Round, EndType::Round, 2.0, arc_tol()))
}

/// A closed loop's outline swept by a round pen.
pub fn stroke_closed(ring: &[Pt], width: Nm) -> Region {
    if ring.len() < 2 || width <= 0 {
        return vec![];
    }
    let path: Path64 = ring.iter().map(|p| p64(*p)).collect();
    from_paths(&inflate_paths_64(&vec![path], width as f64 / 2.0, JoinType::Round, EndType::Joined, 2.0, arc_tol()))
}

/// A filled circle.
pub fn circle(center: Pt, radius: Nm) -> Region {
    vec![geom::circle_points(center, radius, MAX_ERROR).into_iter().map(|q| Pt::new(q[0].round() as Nm, q[1].round() as Nm)).collect()]
}

fn round_pts(v: Vec<[f64; 2]>) -> Vec<Pt> {
    v.into_iter().map(|q| Pt::new(q[0].round() as Nm, q[1].round() as Nm)).collect()
}

/// A shape's points along its path (arcs and curves flattened).
pub fn geom_points(g: &Geom) -> (Vec<Pt>, bool) {
    match g {
        Geom::Line { a, b } => (vec![*a, *b], false),
        Geom::Polyline { pts, closed } => (pts.clone(), *closed),
        Geom::Rect { a, b } => (vec![*a, Pt::new(b.x, a.y), *b, Pt::new(a.x, b.y)], true),
        Geom::Circle { center, radius } => (round_pts(geom::circle_points(*center, *radius, MAX_ERROR)), true),
        Geom::Arc { start, mid, end } => (round_pts(geom::arc_points(*start, *mid, *end, MAX_ERROR)), false),
        Geom::Bezier { pts } => (round_pts(geom::bezier_points(*pts, MAX_ERROR)), false),
    }
}

/// What a drawn shape covers: its outline stroke, plus its inside when filled.
pub fn shape_region(s: &Shape) -> Region {
    let (pts, closed) = geom_points(&s.geom);
    let mut r = if closed { stroke_closed(&pts, s.stroke.width) } else { stroke(&pts, s.stroke.width) };
    if closed && s.fill != Fill::None {
        let mut inside = pts.clone();
        if ring_area(&inside) < 0.0 {
            inside.reverse();
        }
        r.push(inside);
        r = union(&r);
    }
    r
}

/// Signed area (positive counter-clockwise), in nm².
pub fn ring_area(r: &[Pt]) -> f64 {
    let n = r.len();
    (0..n).map(|i| {
        let (a, b) = (r[i], r[(i + 1) % n]);
        a.x as f64 * b.y as f64 - b.x as f64 * a.y as f64
    }).sum::<f64>() / 2.0
}

pub fn area(r: &Region) -> f64 {
    r.iter().map(|ring| ring_area(ring)).sum()
}

pub fn union(r: &Region) -> Region {
    from_paths(&union_subjects_64(&to_paths(r), FillRule::NonZero))
}

pub fn union_all(parts: &[Region]) -> Region {
    let all: Region = parts.iter().flatten().cloned().collect();
    union(&all)
}

pub fn difference(a: &Region, b: &Region) -> Region {
    from_paths(&difference_64(&to_paths(a), &to_paths(b), FillRule::NonZero))
}

pub fn intersection(a: &Region, b: &Region) -> Region {
    from_paths(&intersect_64(&to_paths(a), &to_paths(b), FillRule::NonZero))
}

/// Grown (positive) or shrunk (negative) by `by`, round corners.
pub fn inflate(r: &Region, by: Nm) -> Region {
    if by == 0 {
        return r.clone();
    }
    from_paths(&inflate_paths_64(&to_paths(r), by as f64, JoinType::Round, EndType::Polygon, 2.0, arc_tol()))
}

/// Whether two regions overlap (share area, not just touch).
pub fn overlaps(a: &Region, b: &Region) -> bool {
    area(&intersection(a, b)) > 1.0
}

/// Whether `p` is inside (or on the edge of) the region (non-zero rule).
pub fn contains(r: &Region, p: Pt) -> bool {
    let mut winding = 0;
    for ring in r {
        let path: Path64 = ring.iter().map(|q| p64(*q)).collect();
        match point_in_polygon(p64(p), &path) {
            PointInPolygonResult::IsOn => return true,
            PointInPolygonResult::IsInside => winding += if ring_area(ring) >= 0.0 { 1 } else { -1 },
            PointInPolygonResult::IsOutside => {}
        }
    }
    winding != 0
}

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = ((b.x - a.x) as f64, (b.y - a.y) as f64);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) as f64 * dx + (p.y - a.y) as f64 * dy) / len2).clamp(0.0, 1.0) };
    let (cx, cy) = (a.x as f64 + t * dx, a.y as f64 + t * dy);
    (p.x as f64 - cx).hypot(p.y as f64 - cy)
}

/// The smallest gap between two regions: 0 when they touch or overlap.
pub fn distance(a: &Region, b: &Region) -> f64 {
    if overlaps(a, b) {
        return 0.0;
    }
    let mut best = f64::MAX;
    for (x, y) in [(a, b), (b, a)] {
        for ring in x {
            for &p in ring {
                for other in y {
                    let n = other.len();
                    for i in 0..n {
                        best = best.min(seg_dist(p, other[i], other[(i + 1) % n]));
                    }
                }
            }
        }
    }
    best
}

/// A rounded rectangle centred on the origin, `w` × `h`, corner radius `r`.
fn round_rect(w: Nm, h: Nm, r: Nm) -> Ring {
    let r = r.clamp(0, w.min(h) / 2);
    if r == 0 {
        return vec![Pt::new(-w / 2, -h / 2), Pt::new(w / 2, -h / 2), Pt::new(w / 2, h / 2), Pt::new(-w / 2, h / 2)];
    }
    let inner = vec![
        Pt::new(-w / 2 + r, -h / 2 + r),
        Pt::new(w / 2 - r, -h / 2 + r),
        Pt::new(w / 2 - r, h / 2 - r),
        Pt::new(-w / 2 + r, h / 2 - r),
    ];
    let mut out = union(&inflate(&vec![inner.clone()], r));
    if out.is_empty() {
        out = vec![inner];
    }
    out.swap_remove(0)
}

/// A pad's copper in footprint coordinates (before the footprint's placement), grown by
/// `margin` (clearance, mask expansion; negative shrinks).
pub fn pad_local(pad: &Pad, margin: Nm) -> Region {
    let (w, h) = (pad.size.w, pad.size.h);
    let base: Region = match &pad.shape {
        PadShape::Circle => return transform(&circle(Pt::ZERO, w / 2 + margin), pad.at, pad.angle),
        PadShape::Rect => vec![round_rect(w, h, 0)],
        PadShape::Oval => vec![round_rect(w, h, w.min(h) / 2)],
        PadShape::RoundRect { ratio } => vec![round_rect(w, h, (w.min(h) as f64 * ratio).round() as Nm)],
        PadShape::Trapezoid { delta } => {
            let (dx, dy) = (delta.w / 2, delta.h / 2);
            vec![vec![
                Pt::new(-w / 2 - dy, -h / 2 - dx),
                Pt::new(w / 2 + dy, -h / 2 + dx),
                Pt::new(w / 2 - dy, h / 2 - dx),
                Pt::new(-w / 2 + dy, h / 2 + dx),
            ]]
        }
        PadShape::Chamfered { ratio, corners, round_ratio } => {
            let c = (w.min(h) as f64 * ratio).round() as Nm;
            let (x, y) = (w / 2, h / 2);
            let mut ring = vec![];
            // Corners as seen with Y up: top = +y.
            let mut corner = |p: Pt, cut: bool, a: Pt, b: Pt| {
                if cut {
                    ring.push(a);
                    ring.push(b);
                } else {
                    ring.push(p);
                }
            };
            corner(Pt::new(-x, -y), corners.bottom_left, Pt::new(-x, -y + c), Pt::new(-x + c, -y));
            corner(Pt::new(x, -y), corners.bottom_right, Pt::new(x - c, -y), Pt::new(x, -y + c));
            corner(Pt::new(x, y), corners.top_right, Pt::new(x, y - c), Pt::new(x - c, y));
            corner(Pt::new(-x, y), corners.top_left, Pt::new(-x + c, y), Pt::new(-x, y - c));
            let r = (w.min(h) as f64 * round_ratio).round() as Nm;
            if r > 0 { union(&inflate(&inflate(&vec![ring], -r), r)) } else { vec![ring] }
        }
        PadShape::Custom { anchor_rect, shapes } => {
            let mut parts: Vec<Region> = shapes.iter().map(shape_region).collect();
            let a = w.min(h);
            parts.push(if *anchor_rect { vec![round_rect(a, a, 0)] } else { circle(Pt::ZERO, a / 2) });
            union_all(&parts)
        }
    };
    let grown = if margin != 0 { inflate(&base, margin) } else { base };
    transform(&grown, pad.at, pad.angle)
}

/// Rotated by `angle` degrees about the origin, then moved by `at`.
pub fn transform(r: &Region, at: Pt, angle: f64) -> Region {
    r.iter().map(|ring| ring.iter().map(|p| at + p.rotated(angle)).collect()).collect()
}

/// Mapped point by point (a placement; a mirror reverses ring direction, so rings are turned
/// back to keep outer rings counter-clockwise).
pub fn map(r: &Region, f: impl Fn(Pt) -> Pt) -> Region {
    let area_before: Vec<f64> = r.iter().map(|ring| ring_area(ring)).collect();
    r.iter()
        .zip(area_before)
        .map(|(ring, a0)| {
            let mut out: Ring = ring.iter().map(|p| f(*p)).collect();
            if (ring_area(&out) >= 0.0) != (a0 >= 0.0) {
                out.reverse();
            }
            out
        })
        .collect()
}

/// A board-level hole's drilled area (a circle, or a stadium for oval holes).
pub fn hole(center: Pt, size: crate::units::Size, angle: f64) -> Region {
    if size.w == size.h {
        return circle(center, size.w / 2);
    }
    let r = size.w.min(size.h) / 2;
    let half = (size.w.max(size.h) / 2 - r).max(0);
    let dir = if size.w > size.h { Pt::new(half, 0) } else { Pt::new(0, half) };
    let d = dir.rotated(angle);
    stroke(&[center - d, center + d], r * 2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::footprint::{PadKind, PadRules};
    use crate::layer::LayerSet;
    use crate::units::{Size, mm};

    fn pad(shape: PadShape, w: f64, h: f64) -> Pad {
        Pad {
            id: uuid::Uuid::nil(),
            number: "1".into(),
            kind: PadKind::Smd,
            shape,
            at: Pt::ZERO,
            angle: 0.0,
            size: Size::mm(w, h),
            drill: None,
            layers: LayerSet::EMPTY,
            net: None,
            pin_function: String::new(),
            pin_type: String::new(),
            rules: PadRules::default(),
            die_length: 0,
        }
    }

    fn mm2(a: f64) -> f64 {
        a / 1e12
    }

    #[test]
    fn pad_areas() {
        assert!((mm2(area(&pad_local(&pad(PadShape::Rect, 2.0, 1.0), 0))) - 2.0).abs() < 1e-9);
        let c = mm2(area(&pad_local(&pad(PadShape::Circle, 2.0, 2.0), 0)));
        // Chords 5 µm inside the true circle: about 0.02 mm² less.
        assert!((c - std::f64::consts::PI).abs() < 0.03, "{c}");
        let o = mm2(area(&pad_local(&pad(PadShape::Oval, 2.0, 1.0), 0)));
        assert!((o - (1.0 + std::f64::consts::PI / 4.0)).abs() < 0.03, "{o}");
        let rr = mm2(area(&pad_local(&pad(PadShape::RoundRect { ratio: 0.25 }, 1.0, 1.45), 0)));
        assert!(rr < 1.45 && rr > 1.35, "{rr}");
        // Grown by 0.5 mm: a 2 × 1 rect becomes a 3 × 2 rounded rect.
        let g = mm2(area(&pad_local(&pad(PadShape::Rect, 2.0, 1.0), mm(0.5))));
        assert!((g - (6.0 - (1.0 - std::f64::consts::PI / 4.0))).abs() < 0.01, "{g}");
    }

    #[test]
    fn strokes_booleans_distance() {
        let t = stroke(&[Pt::mm(0.0, 0.0), Pt::mm(10.0, 0.0)], mm(0.4));
        let a = mm2(area(&t));
        assert!((a - (4.0 + std::f64::consts::PI * 0.04)).abs() < 0.01, "{a}");
        let c = circle(Pt::mm(5.0, 2.0), mm(1.0));
        let gap = distance(&t, &c) / 1e6;
        assert!((gap - 0.8).abs() < 0.01, "{gap}");
        assert!(!overlaps(&t, &c));
        assert!(overlaps(&inflate(&t, mm(1.0)), &c));
        assert!(contains(&c, Pt::mm(5.0, 2.5)));
        assert!(!contains(&c, Pt::mm(5.0, 3.5)));
        // A hole punched out keeps working with containment.
        let ring = difference(&circle(Pt::ZERO, mm(3.0)), &circle(Pt::ZERO, mm(1.0)));
        assert!(contains(&ring, Pt::mm(2.0, 0.0)));
        assert!(!contains(&ring, Pt::ZERO));
    }
}

/// Triangles covering a region (holes respected), as a flat list of corners in nm.
pub fn triangulate(r: &Region) -> Vec<[f64; 2]> {
    let mut out = vec![];
    for pg in crate::zone::to_polygons(r) {
        let mut flat: Vec<f64> = pg.outer.iter().flat_map(|p| [p.x as f64, p.y as f64]).collect();
        let mut holes = vec![];
        for h in &pg.holes {
            holes.push(flat.len() / 2);
            flat.extend(h.iter().flat_map(|p| [p.x as f64, p.y as f64]));
        }
        if let Ok(idx) = earcutr::earcut(&flat, &holes, 2) {
            out.extend(idx.into_iter().map(|i| [flat[2 * i], flat[2 * i + 1]]));
        }
    }
    out
}

#[cfg(test)]
mod triangulate_tests {
    use super::*;
    use crate::units::mm;

    #[test]
    fn triangles_cover_the_area() {
        let ring = difference(&circle(Pt::ZERO, mm(3.0)), &circle(Pt::ZERO, mm(1.0)));
        let t = triangulate(&ring);
        assert_eq!(t.len() % 3, 0);
        let sum: f64 = t.chunks(3).map(|c| ((c[1][0] - c[0][0]) * (c[2][1] - c[0][1]) - (c[2][0] - c[0][0]) * (c[1][1] - c[0][1])).abs() / 2.0).sum();
        assert!((sum - area(&ring)).abs() / area(&ring) < 1e-6);
    }
}

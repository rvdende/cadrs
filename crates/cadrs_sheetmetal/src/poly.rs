//! 2D polygons for walls and flat patterns, and the few operations the flat solver needs:
//! point-in-polygon, clipping by a half-plane (trimming a wall back to a bend's tangent line),
//! affine maps, and booleans (through `geo`) for the outline, the reliefs and the collision check.

use geo::{Area, BooleanOps};
use nalgebra::{Matrix2, Point2, Vector2};
use serde::{Deserialize, Serialize};

pub type P2 = Point2<f64>;
pub type V2 = Vector2<f64>;

/// A polygon with holes. The outer loop is counter-clockwise, holes clockwise (normalised by
/// [`Polygon::normalized`]); loops are not closed (the last point isn't repeated).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Polygon {
    pub outer: Vec<P2>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub holes: Vec<Vec<P2>>,
}

impl Polygon {
    pub fn new(outer: Vec<P2>) -> Polygon {
        Polygon { outer, holes: Vec::new() }.normalized()
    }

    pub fn with_holes(outer: Vec<P2>, holes: Vec<Vec<P2>>) -> Polygon {
        Polygon { outer, holes }.normalized()
    }

    /// An axis-aligned rectangle.
    pub fn rect(min: P2, max: P2) -> Polygon {
        Polygon::new(vec![min, P2::new(max.x, min.y), max, P2::new(min.x, max.y)])
    }

    /// The outer loop counter-clockwise and the holes clockwise.
    pub fn normalized(mut self) -> Polygon {
        if signed_area(&self.outer) < 0.0 {
            self.outer.reverse();
        }
        for h in &mut self.holes {
            if signed_area(h) > 0.0 {
                h.reverse();
            }
        }
        self
    }

    /// Area of the outer loop minus the holes.
    pub fn area(&self) -> f64 {
        signed_area(&self.outer).abs() - self.holes.iter().map(|h| signed_area(h).abs()).sum::<f64>()
    }

    /// Strictly inside the material (inside the outer loop and outside every hole). Points on the
    /// boundary count as inside or outside arbitrarily.
    pub fn contains(&self, p: P2) -> bool {
        inside_loop(&self.outer, p) && !self.holes.iter().any(|h| inside_loop(h, p))
    }

    pub fn map(&self, f: impl Fn(P2) -> P2) -> Polygon {
        Polygon {
            outer: self.outer.iter().map(|p| f(*p)).collect(),
            holes: self.holes.iter().map(|h| h.iter().map(|p| f(*p)).collect()).collect(),
        }
        .normalized()
    }

    /// The part on the side of the line through `p` with normal `n` where `(x − p)·n ≥ 0`
    /// (Sutherland–Hodgman on each loop; a loop that vanishes is dropped).
    pub fn clip_half_plane(&self, p: P2, n: V2) -> Polygon {
        let outer = clip_loop(&self.outer, p, n);
        if outer.len() < 3 {
            return Polygon::default();
        }
        Polygon {
            outer,
            holes: self.holes.iter().map(|h| clip_loop(h, p, n)).filter(|h| h.len() >= 3).collect(),
        }
        .normalized()
    }

    pub fn bounds(&self) -> Option<(P2, P2)> {
        let mut it = self.outer.iter();
        let first = *it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| {
            (P2::new(lo.x.min(p.x), lo.y.min(p.y)), P2::new(hi.x.max(p.x), hi.y.max(p.y)))
        }))
    }

    pub fn is_empty(&self) -> bool {
        self.outer.len() < 3
    }

    /// For the booleans: coordinates snapped to [`GRID`], so edges that should coincide but
    /// came out of different computations a hair apart do coincide.
    pub(crate) fn to_geo(&self) -> geo::Polygon<f64> {
        let ring = |l: &[P2]| geo::LineString::from(l.iter().map(|p| (snap(p.x), snap(p.y))).collect::<Vec<_>>());
        geo::Polygon::new(ring(&self.outer), self.holes.iter().map(|h| ring(h)).collect())
    }

    pub(crate) fn from_geo(g: &geo::Polygon<f64>) -> Polygon {
        let ring = |l: &geo::LineString<f64>| {
            let mut pts: Vec<P2> = l.coords().map(|c| P2::new(c.x, c.y)).collect();
            if pts.len() > 1 && pts.first() == pts.last() {
                pts.pop();
            }
            pts
        };
        Polygon {
            outer: ring(g.exterior()),
            holes: g.interiors().iter().map(ring).collect(),
        }
        .normalized()
    }
}

/// The grid (mm) polygon booleans work on: 1 nm, far below any modelling tolerance.
pub const GRID: f64 = 1e-6;

fn snap(v: f64) -> f64 {
    (v / GRID).round() * GRID
}

/// Twice-signed shoelace area / 2 (counter-clockwise positive).
pub fn signed_area(l: &[P2]) -> f64 {
    let n = l.len();
    (0..n).map(|i| {
        let (a, b) = (l[i], l[(i + 1) % n]);
        a.x * b.y - b.x * a.y
    })
    .sum::<f64>()
        / 2.0
}

fn inside_loop(l: &[P2], p: P2) -> bool {
    let n = l.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (l[i], l[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

fn clip_loop(l: &[P2], p: P2, n: V2) -> Vec<P2> {
    let d = |q: P2| (q - p).dot(&n);
    let mut out = Vec::with_capacity(l.len() + 2);
    for i in 0..l.len() {
        let (a, b) = (l[i], l[(i + 1) % l.len()]);
        let (da, db) = (d(a), d(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da >= 0.0) != (db >= 0.0) {
            let s = da / (da - db);
            out.push(a + (b - a) * s);
        }
    }
    // Drop repeated points the clip can leave on the line.
    out.dedup_by(|a, b| (*a - *b).norm() < 1e-12);
    if out.len() > 1 && (out[0] - out[out.len() - 1]).norm() < 1e-12 {
        out.pop();
    }
    out
}

/// A rigid map of the plane (rotation or reflection, then translation): `x ↦ m·x + t`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Affine2 {
    pub m: Matrix2<f64>,
    pub t: V2,
}

impl Affine2 {
    /// The map undoing this one (`None` if it is degenerate).
    pub fn inverse(&self) -> Option<Affine2> {
        let m = self.m.try_inverse()?;
        Some(Affine2 { m, t: -(m * self.t) })
    }

    pub fn identity() -> Affine2 {
        Affine2 {
            m: Matrix2::identity(),
            t: V2::zeros(),
        }
    }

    pub fn apply(&self, p: P2) -> P2 {
        P2::from(self.m * p.coords + self.t)
    }

    pub fn apply_vec(&self, v: V2) -> V2 {
        self.m * v
    }

    /// Whether this map mirrors (a wall laid flat with its other side up).
    pub fn is_reflection(&self) -> bool {
        self.m.determinant() < 0.0
    }
}

/// A line segment.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Seg2 {
    pub a: P2,
    pub b: P2,
}

impl Seg2 {
    pub fn new(a: P2, b: P2) -> Seg2 {
        Seg2 { a, b }
    }

    pub fn len(&self) -> f64 {
        (self.b - self.a).norm()
    }

    pub fn dir(&self) -> V2 {
        (self.b - self.a).normalize()
    }

    pub fn offset(&self, v: V2) -> Seg2 {
        Seg2::new(self.a + v, self.b + v)
    }

    pub fn map(&self, f: &Affine2) -> Seg2 {
        Seg2::new(f.apply(self.a), f.apply(self.b))
    }
}

/// The unit normal of `s` pointing into `poly`: the side where a point just off the segment's
/// middle is inside (`None` if neither or both sides are).
pub fn inward_normal(poly: &Polygon, s: Seg2) -> Option<V2> {
    if s.len() < 1e-12 {
        return None;
    }
    let n = perp(s.dir());
    let mid = P2::from((s.a.coords + s.b.coords) / 2.0);
    let size = poly.bounds().map(|(lo, hi)| (hi - lo).norm()).unwrap_or(1.0);
    for step in [1e-5, 1e-4, 1e-3] {
        let d = step * size.max(s.len());
        match (poly.contains(mid + n * d), poly.contains(mid - n * d)) {
            (true, false) => return Some(n),
            (false, true) => return Some(-n),
            _ => {}
        }
    }
    None
}

/// Moves every vertex of `poly` that lies within `tol` of one of `exact` onto it: polygon booleans
/// round their output to [`GRID`], and the inputs' exact coordinates are better.
pub fn snap_to(poly: &Polygon, exact: &[P2], tol: f64) -> Polygon {
    let fix = |p: P2| exact.iter().copied().find(|q| (q - p).norm() <= tol).unwrap_or(p);
    Polygon {
        outer: poly.outer.iter().map(|p| fix(*p)).collect(),
        holes: poly.holes.iter().map(|h| h.iter().map(|p| fix(*p)).collect()).collect(),
    }
}

/// `a` less `cuts`, with every vertex put back on the inputs' exact coordinates: their vertices
/// and the points where a cut's edges cross `a`'s (the booleans round to [`GRID`], which can
/// leave an oblique edge a micrometre off the face it should meet).
pub fn difference_exact(a: &Polygon, cuts: &[Polygon]) -> Vec<Polygon> {
    let loops = |p: &Polygon| -> Vec<Vec<P2>> { std::iter::once(p.outer.clone()).chain(p.holes.iter().cloned()).collect() };
    let mut exact: Vec<P2> = loops(a).into_iter().flatten().collect();
    exact.extend(cuts.iter().flat_map(|c| loops(c).into_iter().flatten()));
    for la in loops(a) {
        for i in 0..la.len() {
            let (p, q) = (la[i], la[(i + 1) % la.len()]);
            for c in cuts {
                for lc in loops(c) {
                    for k in 0..lc.len() {
                        let (r, s2) = (lc[k], lc[(k + 1) % lc.len()]);
                        let (d, e) = (q - p, s2 - r);
                        let den = d.perp(&e);
                        if den.abs() < 1e-15 {
                            continue;
                        }
                        let t = (r - p).perp(&e) / den;
                        let u = (r - p).perp(&d) / den;
                        if (-1e-9..=1.0 + 1e-9).contains(&t) && (-1e-9..=1.0 + 1e-9).contains(&u) {
                            exact.push(p + d * t);
                        }
                    }
                }
            }
        }
    }
    difference(std::slice::from_ref(a), cuts).iter().map(|p| snap_to(p, &exact, 10.0 * GRID)).collect()
}

/// The parts of segment `s` inside `polys` (material), as sub-segments in order along `s`.
pub fn clip_segment(s: Seg2, polys: &[Polygon]) -> Vec<Seg2> {
    let d = s.b - s.a;
    let len = d.norm();
    if len < 1e-12 {
        return Vec::new();
    }
    // Every crossing with any loop's edges, as a parameter along s.
    let mut ts = vec![0.0, 1.0];
    for p in polys {
        for l in std::iter::once(&p.outer).chain(p.holes.iter()) {
            for i in 0..l.len() {
                let (a, b) = (l[i], l[(i + 1) % l.len()]);
                let e = b - a;
                let den = d.perp(&e);
                if den.abs() < 1e-15 {
                    continue;
                }
                let w = a - s.a;
                let t = w.perp(&e) / den;
                let u = w.perp(&d) / den;
                if (0.0..=1.0).contains(&t) && (-1e-12..=1.0 + 1e-12).contains(&u) {
                    ts.push(t);
                }
            }
        }
    }
    ts.sort_by(f64::total_cmp);
    ts.dedup_by(|a, b| (*a - *b).abs() * len < 1e-9);
    let mut out: Vec<Seg2> = Vec::new();
    for w in ts.windows(2) {
        if (w[1] - w[0]) * len < 1e-9 {
            continue;
        }
        let mid = s.a + d * ((w[0] + w[1]) / 2.0);
        if polys.iter().any(|p| p.contains(mid)) {
            let (a, b) = (s.a + d * w[0], s.a + d * w[1]);
            match out.last_mut() {
                Some(last) if (last.b - a).norm() < 1e-9 => last.b = b,
                _ => out.push(Seg2::new(a, b)),
            }
        }
    }
    out
}

/// The left-hand normal of a direction.
pub fn perp(v: V2) -> V2 {
    V2::new(-v.y, v.x)
}

/// A circle as a polygon of `n` points.
pub fn circle(center: P2, radius: f64, n: usize) -> Polygon {
    Polygon::new(
        (0..n)
            .map(|i| {
                let a = i as f64 / n as f64 * std::f64::consts::TAU;
                center + V2::new(a.cos(), a.sin()) * radius
            })
            .collect(),
    )
}

/// The union of polygons, as separate polygons with holes.
pub fn union(polys: &[Polygon]) -> Vec<Polygon> {
    let once = |polys: &[geo::Polygon<f64>]| {
        let mut acc = geo::MultiPolygon::<f64>(Vec::new());
        for p in polys {
            acc = acc.union(&geo::MultiPolygon(vec![p.clone()]));
            // Back onto the grid: the boolean's output drifts off it by an ulp or so, and a
            // piece added later along that edge would then only nearly touch it.
            acc = geo::MultiPolygon(acc.0.iter().map(|g| Polygon::from_geo(g).to_geo()).collect());
        }
        acc.0
    };
    let mut out = once(&polys.iter().filter(|p| !p.is_empty()).map(Polygon::to_geo).collect::<Vec<_>>());
    // P3I.6: adding a piece that joins two others only along their edges can leave them apart
    // (the boolean's result depends on the order): union again until nothing more joins.
    while out.len() > 1 {
        // Back on the grid first: the boolean's output can be a hair off it.
        let snapped: Vec<geo::Polygon<f64>> = out.iter().map(|g| Polygon::from_geo(g).to_geo()).collect();
        let again = once(&snapped);
        if again.len() >= out.len() {
            break;
        }
        out = again;
    }
    out.iter().map(Polygon::from_geo).collect()
}

/// `a` minus every polygon of `cuts`.
pub fn difference(a: &[Polygon], cuts: &[Polygon]) -> Vec<Polygon> {
    let mut acc = geo::MultiPolygon(a.iter().filter(|p| !p.is_empty()).map(Polygon::to_geo).collect());
    for c in cuts.iter().filter(|p| !p.is_empty()) {
        acc = acc.difference(&geo::MultiPolygon(vec![c.to_geo()]));
    }
    acc.0.iter().map(Polygon::from_geo).collect()
}

/// The region two polygons share.
pub fn intersection(a: &Polygon, b: &Polygon) -> Vec<Polygon> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    a.to_geo().intersection(&b.to_geo()).0.iter().map(Polygon::from_geo).filter(|p| p.area() > 0.0).collect()
}

/// Total length of the polygon's loops.
pub fn perimeter(p: &Polygon) -> f64 {
    std::iter::once(&p.outer)
        .chain(p.holes.iter())
        .map(|l| (0..l.len()).map(|i| (l[(i + 1) % l.len()] - l[i]).norm()).sum::<f64>())
        .sum()
}

/// The area two polygons share.
pub fn overlap_area(a: &Polygon, b: &Polygon) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    // Cheap reject on the bounds first.
    if let (Some((alo, ahi)), Some((blo, bhi))) = (a.bounds(), b.bounds())
        && (ahi.x < blo.x || bhi.x < alo.x || ahi.y < blo.y || bhi.y < alo.y)
    {
        return 0.0;
    }
    a.to_geo().intersection(&b.to_geo()).unsigned_area()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_area_and_contains() {
        let r = Polygon::rect(P2::new(0.0, 0.0), P2::new(4.0, 2.0));
        assert!((r.area() - 8.0).abs() < 1e-12);
        assert!(r.contains(P2::new(1.0, 1.0)));
        assert!(!r.contains(P2::new(5.0, 1.0)));
    }

    #[test]
    fn clipping_keeps_the_positive_side() {
        let r = Polygon::rect(P2::new(0.0, 0.0), P2::new(4.0, 2.0));
        let c = r.clip_half_plane(P2::new(1.0, 0.0), V2::new(1.0, 0.0));
        assert!((c.area() - 6.0).abs() < 1e-12);
        let gone = r.clip_half_plane(P2::new(9.0, 0.0), V2::new(1.0, 0.0));
        assert!(gone.is_empty());
    }

    #[test]
    fn union_of_abutting_rects_is_one_polygon() {
        let a = Polygon::rect(P2::new(0.0, 0.0), P2::new(2.0, 2.0));
        let b = Polygon::rect(P2::new(2.0, 0.0), P2::new(5.0, 2.0));
        let u = union(&[a.clone(), b.clone()]);
        assert_eq!(u.len(), 1);
        assert!((u[0].area() - 10.0).abs() < 1e-9);
        assert!(overlap_area(&a, &b) < 1e-9);
        let c = Polygon::rect(P2::new(1.0, 1.0), P2::new(3.0, 3.0));
        assert!((overlap_area(&a, &c) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn union_joins_pieces_bridged_along_their_edges() {
        // Two walls apart, then the strip between them touching both along their edges (the order
        // the flat solver adds a U's pieces in).
        let w0 = Polygon::rect(P2::new(0.0, 2.0), P2::new(200.0, 118.0));
        let w1 = Polygon::rect(P2::new(0.0, -48.2776546738526), P2::new(200.0, -0.27765467385260023));
        let w2 = Polygon::rect(P2::new(0.0, 120.2776546738526), P2::new(200.0, 168.2776546738526));
        let b0 = Polygon::rect(P2::new(0.0, -0.27765467385260023), P2::new(200.0, 2.0));
        let b1 = Polygon::rect(P2::new(0.0, 118.0), P2::new(200.0, 120.2776546738526));
        let u = union(&[w0, w1, w2, b0, b1]);
        assert_eq!(u.len(), 1, "{:?}", u.iter().map(|p| p.outer.iter().map(|q| (q.x, q.y)).collect::<Vec<_>>()).collect::<Vec<_>>());
    }

    #[test]
    fn difference_cuts_a_hole() {
        let a = Polygon::rect(P2::new(0.0, 0.0), P2::new(10.0, 10.0));
        let d = difference(&[a], &[circle(P2::new(5.0, 5.0), 1.0, 64)]);
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].holes.len(), 1);
        assert!((d[0].area() - (100.0 - std::f64::consts::PI)).abs() < 0.01);
    }
}

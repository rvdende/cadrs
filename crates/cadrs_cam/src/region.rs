//! 2D areas as rings (Clipper2 on integers of 0.1 µm): offsets, booleans and the split into
//! faces with their holes.
//!
//! A [`Region`] is a list of closed rings filled by the non-zero rule. The functions here
//! return normalised regions: outer rings counter-clockwise, holes clockwise, no overlaps.

use clipper2_rust::{
    ClipType, EndType, FillRule, JoinType, Path64, Paths64, Point64, PolyTree64,
    boolean_op_tree_64, difference_64, inflate_paths_64, intersect_64, simplify_paths,
    union_subjects_64,
};

use crate::geom::{P2, ring_area2};

pub type Ring = Vec<P2>;
pub type Region = Vec<Ring>;

/// Integer units per mm.
const SCALE: f64 = 10_000.0;
/// How far an offset's round corners may stray from the true arc (mm).
pub const ARC_TOLERANCE: f64 = 0.002;

fn p64(p: P2) -> Point64 {
    Point64 {
        x: (p[0] * SCALE).round() as i64,
        y: (p[1] * SCALE).round() as i64,
    }
}

fn to_paths(r: &[Ring]) -> Paths64 {
    r.iter()
        .filter(|ring| ring.len() >= 3)
        .map(|ring| ring.iter().map(|p| p64(*p)).collect::<Path64>())
        .collect()
}

fn from_path(p: &Path64) -> Ring {
    p.iter()
        .map(|q| [q.x as f64 / SCALE, q.y as f64 / SCALE])
        .collect()
}

fn from_paths(p: &Paths64) -> Region {
    p.iter().map(from_path).filter(|r| r.len() >= 3).collect()
}

/// The rings merged into a clean region (non-zero fill).
pub fn normalize(r: &[Ring]) -> Region {
    from_paths(&union_subjects_64(&to_paths(r), FillRule::NonZero))
}

/// The union of two regions.
pub fn union(a: &[Ring], b: &[Ring]) -> Region {
    let mut all = to_paths(a);
    all.extend(to_paths(b));
    from_paths(&union_subjects_64(&all, FillRule::NonZero))
}

/// The union of many rings, each a filled area (whatever its orientation): projected triangles,
/// faces' outlines.
pub fn union_all(rings: impl IntoIterator<Item = Ring>) -> Region {
    let paths: Paths64 = rings
        .into_iter()
        .filter(|r| r.len() >= 3)
        .map(|mut r| {
            if ring_area2(&r) < 0.0 {
                r.reverse();
            }
            r.iter().map(|p| p64(*p)).collect::<Path64>()
        })
        .collect();
    from_paths(&union_subjects_64(&paths, FillRule::NonZero))
}

/// The union of regions, each already normalised (outer rings counter-clockwise, holes
/// clockwise): unlike [`union_all`], holes stay holes.
pub fn union_regions(regions: impl IntoIterator<Item = Region>) -> Region {
    let paths: Paths64 = regions.into_iter().flat_map(|r| to_paths(&r)).collect();
    from_paths(&union_subjects_64(&paths, FillRule::NonZero))
}

/// `a` without `b`.
pub fn difference(a: &[Ring], b: &[Ring]) -> Region {
    from_paths(&difference_64(
        &to_paths(a),
        &to_paths(b),
        FillRule::NonZero,
    ))
}

/// Where `a` and `b` overlap.
pub fn intersection(a: &[Ring], b: &[Ring]) -> Region {
    from_paths(&intersect_64(&to_paths(a), &to_paths(b), FillRule::NonZero))
}

/// The region grown by `delta` (shrunk when negative), with round corners.
/// The holes of a region (its clockwise rings), each as the region inside it.
pub fn holes(r: &[Ring]) -> Region {
    r.iter()
        .filter(|ring| crate::geom::ring_area2(ring) < 0.0)
        .map(|ring| ring.iter().rev().copied().collect())
        .collect()
}

pub fn offset(r: &[Ring], delta: f64) -> Region {
    if delta == 0.0 {
        return normalize(r);
    }
    from_paths(&inflate_paths_64(
        &to_paths(r),
        delta * SCALE,
        JoinType::Round,
        EndType::Polygon,
        2.0,
        ARC_TOLERANCE * SCALE,
    ))
}

/// The region grown by `delta` with sharp (mitred) corners, for a stock outline.
pub fn offset_miter(r: &[Ring], delta: f64) -> Region {
    from_paths(&inflate_paths_64(
        &to_paths(r),
        delta * SCALE,
        JoinType::Miter,
        EndType::Polygon,
        4.0,
        ARC_TOLERANCE * SCALE,
    ))
}

/// The area swept by a round pen of `width` along an open polyline.
pub fn stroke(line: &[P2], width: f64) -> Region {
    if line.is_empty() || width <= 0.0 {
        return Vec::new();
    }
    let mut path: Path64 = line.iter().map(|p| p64(*p)).collect();
    if path.len() == 1 {
        path.push(path[0]);
    }
    from_paths(&inflate_paths_64(
        &vec![path],
        width * SCALE / 2.0,
        JoinType::Round,
        EndType::Round,
        2.0,
        ARC_TOLERANCE * SCALE,
    ))
}

/// Points closer than `epsilon` to the line through their neighbours dropped.
pub fn simplify(r: &[Ring], epsilon: f64) -> Region {
    from_paths(&simplify_paths(&to_paths(r), epsilon * SCALE, true))
}

/// The pieces of open polylines inside a region, each running the way its line does, in the
/// order they come along the lines.
pub fn clip_open(lines: &[Vec<P2>], r: &[Ring]) -> Vec<Vec<P2>> {
    if lines.is_empty() || r.is_empty() {
        return Vec::new();
    }
    let mut c = clipper2_rust::Clipper64::new();
    c.add_open_subject(
        &lines
            .iter()
            .map(|l| l.iter().map(|p| p64(*p)).collect())
            .collect(),
    );
    c.add_clip(&to_paths(r));
    let mut closed = Paths64::new();
    let mut open = Paths64::new();
    c.execute(
        ClipType::Intersection,
        FillRule::NonZero,
        &mut closed,
        Some(&mut open),
    );
    // Where along the lines a point is: (line, segment, fraction), and the segment's way.
    let place = |q: P2| -> ((usize, usize, f64), P2) {
        let mut best = (f64::INFINITY, (0, 0, 0.0), [1.0, 0.0]);
        for (li, l) in lines.iter().enumerate() {
            for (si, w) in l.windows(2).enumerate() {
                let d = crate::geom::dist_to_segment2(q, w[0], w[1]);
                if d < best.0 {
                    let v = [w[1][0] - w[0][0], w[1][1] - w[0][1]];
                    let l2 = (v[0] * v[0] + v[1] * v[1]).max(1e-18);
                    let t = ((q[0] - w[0][0]) * v[0] + (q[1] - w[0][1]) * v[1]) / l2;
                    best = (d, (li, si, t.clamp(0.0, 1.0)), v);
                }
            }
        }
        (best.1, best.2)
    };
    let mut pieces: Vec<((usize, usize, f64), Vec<P2>)> = open
        .iter()
        .map(from_path)
        .filter(|piece| piece.len() >= 2)
        .map(|mut piece| {
            // The way the line runs where the piece starts.
            let (_, v) = place(piece[0]);
            let d = [piece[1][0] - piece[0][0], piece[1][1] - piece[0][1]];
            if d[0] * v[0] + d[1] * v[1] < 0.0 {
                piece.reverse();
            }
            (place(piece[0]).0, piece)
        })
        .collect();
    pieces.sort_by(|a, b| {
        (a.0.0, a.0.1)
            .cmp(&(b.0.0, b.0.1))
            .then(a.0.2.total_cmp(&b.0.2))
    });
    pieces.into_iter().map(|(_, p)| p).collect()
}

/// Triangles covering a region (holes kept out), as corner triples.
pub fn triangulate(r: &[Ring]) -> Vec<[P2; 3]> {
    let mut out = Vec::new();
    for face in faces(r) {
        let mut flat: Vec<f64> = face.outer.iter().flat_map(|p| [p[0], p[1]]).collect();
        let mut holes = Vec::new();
        for h in &face.holes {
            holes.push(flat.len() / 2);
            flat.extend(h.iter().flat_map(|p| [p[0], p[1]]));
        }
        if let Ok(idx) = earcutr::earcut(&flat, &holes, 2) {
            let pt = |i: usize| [flat[2 * i], flat[2 * i + 1]];
            out.extend(
                idx.as_chunks::<3>()
                    .0
                    .iter()
                    .map(|t| [pt(t[0]), pt(t[1]), pt(t[2])]),
            );
        }
    }
    out
}

/// Whether `p` is inside the region (even-odd over its rings: holes are inside their outer
/// ring).
pub fn contains(r: &[Ring], p: P2) -> bool {
    r.iter()
        .filter(|ring| crate::geom::ring_contains(ring, p))
        .count()
        % 2
        == 1
}

/// Whether the straight segment from `a` to `b` stays inside the region: it crosses no ring
/// (touching one is allowed, so ends on the boundary count as inside) and points along it are
/// inside.
pub fn segment_inside(r: &[Ring], a: P2, b: P2) -> bool {
    use crate::geom::{cross2, sub2};
    let ab = sub2(b, a);
    let l = ab[0].hypot(ab[1]);
    if l < 1e-9 {
        return contains(r, a);
    }
    let eps = 1e-4;
    for ring in r {
        let n = ring.len();
        for i in 0..n {
            let (c, d) = (ring[i], ring[(i + 1) % n]);
            let cd = sub2(d, c);
            let lcd = cd[0].hypot(cd[1]);
            if lcd < 1e-12 {
                continue;
            }
            // Signed distances of c, d from the line ab, and of a, b from the line cd.
            let sc = cross2(ab, sub2(c, a)) / l;
            let sd = cross2(ab, sub2(d, a)) / l;
            let sa = cross2(cd, sub2(a, c)) / lcd;
            let sb = cross2(cd, sub2(b, c)) / lcd;
            if ((sc > eps && sd < -eps) || (sc < -eps && sd > eps))
                && ((sa > eps && sb < -eps) || (sa < -eps && sb > eps))
            {
                return false;
            }
        }
    }
    [0.25, 0.5, 0.75]
        .iter()
        .all(|t| contains(r, [a[0] + ab[0] * t, a[1] + ab[1] * t]))
}

/// The region's area (mm²).
pub fn area(r: &[Ring]) -> f64 {
    r.iter().map(|ring| ring_area2(ring) / 2.0).sum()
}

/// A connected piece of a region: its outer ring (counter-clockwise) and its holes
/// (clockwise). Islands inside the holes are faces of their own.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub outer: Ring,
    pub holes: Vec<Ring>,
}

impl Face {
    /// Its rings, outer first.
    pub fn rings(&self) -> Region {
        let mut v = vec![self.outer.clone()];
        v.extend(self.holes.iter().cloned());
        v
    }

    pub fn area(&self) -> f64 {
        area(&self.rings())
    }
}

/// The region split into faces.
pub fn faces(r: &[Ring]) -> Vec<Face> {
    let mut tree = PolyTree64::new();
    boolean_op_tree_64(
        ClipType::Union,
        FillRule::NonZero,
        &to_paths(r),
        &Paths64::new(),
        &mut tree,
    );
    let mut out = Vec::new();
    collect_faces(&tree, 0, &mut out);
    out
}

fn collect_faces(tree: &PolyTree64, node: usize, out: &mut Vec<Face>) {
    for &child in tree.nodes[node].children() {
        if tree.is_hole(child) {
            collect_faces(tree, child, out);
            continue;
        }
        let outer = from_path(tree.nodes[child].polygon());
        let holes = tree.nodes[child]
            .children()
            .iter()
            .filter(|&&h| tree.is_hole(h))
            .map(|&h| from_path(tree.nodes[h].polygon()))
            .filter(|h| h.len() >= 3)
            .collect();
        if outer.len() >= 3 {
            out.push(Face { outer, holes });
        }
        for &h in tree.nodes[child].children() {
            collect_faces(tree, h, out);
        }
    }
}

/// A rectangle as a counter-clockwise ring.
pub fn rect(min: P2, max: P2) -> Ring {
    vec![
        [min[0], min[1]],
        [max[0], min[1]],
        [max[0], max[1]],
        [min[0], max[1]],
    ]
}

/// A circle as a counter-clockwise ring within [`ARC_TOLERANCE`].
pub fn circle(center: P2, radius: f64) -> Ring {
    let n = segments_for(radius, std::f64::consts::TAU);
    (0..n)
        .map(|i| {
            let a = std::f64::consts::TAU * i as f64 / n as f64;
            [center[0] + radius * a.cos(), center[1] + radius * a.sin()]
        })
        .collect()
}

/// How many chords keep an arc of `radius` and `sweep` (radians) within [`ARC_TOLERANCE`].
pub fn segments_for(radius: f64, sweep: f64) -> usize {
    if radius <= ARC_TOLERANCE {
        return 4;
    }
    let step = 2.0 * (1.0 - ARC_TOLERANCE / radius).clamp(-1.0, 1.0).acos();
    ((sweep.abs() / step.max(1e-3)).ceil() as usize).clamp(4, 720)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_lines_clipped_keep_their_way() {
        let sq = vec![rect([0.0, 0.0], [10.0, 10.0])];
        let lines = vec![
            vec![[-5.0, 5.0], [15.0, 5.0]],
            vec![[5.0, 15.0], [5.0, -5.0]],
        ];
        let mut out = clip_open(&lines, &sq);
        out.sort_by(|a, b| a[0][0].total_cmp(&b[0][0]));
        assert_eq!(out.len(), 2);
        let h = out
            .iter()
            .find(|l| (l[0][1] - 5.0).abs() < 1e-9 && (l[1][1] - 5.0).abs() < 1e-9)
            .unwrap();
        assert!(h[0][0] < h[h.len() - 1][0] && (h[0][0]).abs() < 1e-6);
        let v = out
            .iter()
            .find(|l| (l[0][0] - 5.0).abs() < 1e-9 && (l[1][0] - 5.0).abs() < 1e-9)
            .unwrap();
        assert!(v[0][1] > v[v.len() - 1][1]);
    }

    #[test]
    fn triangles_cover_a_holed_square() {
        let holed = difference(
            &[rect([0.0, 0.0], [10.0, 10.0])],
            &[rect([3.0, 3.0], [7.0, 7.0])],
        );
        let t = triangulate(&holed);
        let a: f64 = t
            .iter()
            .map(|t| {
                ((t[1][0] - t[0][0]) * (t[2][1] - t[0][1])
                    - (t[2][0] - t[0][0]) * (t[1][1] - t[0][1]))
                    .abs()
                    / 2.0
            })
            .sum();
        assert!((a - 84.0).abs() < 1e-6, "{a}");
    }

    #[test]
    fn segments_inside_a_u() {
        // A U: 0..30 × 0..20 with the notch 10..20 × 5..20 cut out.
        let u = difference(
            &[rect([0.0, 0.0], [30.0, 20.0])],
            &[rect([10.0, 5.0], [20.0, 25.0])],
        );
        assert!(contains(&u, [5.0, 10.0]));
        assert!(!contains(&u, [15.0, 10.0]));
        // Across the notch: out.
        assert!(!segment_inside(&u, [5.0, 10.0], [25.0, 10.0]));
        // Along the bottom bar: in.
        assert!(segment_inside(&u, [5.0, 2.0], [25.0, 2.0]));
        // From a boundary point into the inside: in.
        assert!(segment_inside(&u, [0.0, 10.0], [5.0, 2.0]));
        // A hole in the middle of a square blocks the way through it.
        let holed = difference(
            &[rect([0.0, 0.0], [30.0, 30.0])],
            &[circle([15.0, 15.0], 3.0)],
        );
        assert!(!segment_inside(&holed, [2.0, 15.0], [28.0, 15.0]));
        assert!(segment_inside(&holed, [2.0, 2.0], [28.0, 2.0]));
    }

    #[test]
    fn offset_square_out_and_in() {
        let sq = vec![rect([0.0, 0.0], [10.0, 10.0])];
        let grown = offset(&sq, 1.0);
        // 12 × 12 minus the four corners' squares plus quarter circles.
        let expect = 144.0 - 4.0 + std::f64::consts::PI;
        assert!((area(&grown) - expect).abs() < 0.05, "{}", area(&grown));
        let shrunk = offset(&sq, -2.0);
        assert!((area(&shrunk) - 36.0).abs() < 1e-6);
        assert!(offset(&sq, -6.0).is_empty());
    }

    #[test]
    fn faces_split_holes_and_islands() {
        let outer = rect([0.0, 0.0], [30.0, 30.0]);
        let mut hole = rect([5.0, 5.0], [25.0, 25.0]);
        hole.reverse();
        let island = rect([10.0, 10.0], [20.0, 20.0]);
        let f = faces(&[outer, hole, island]);
        assert_eq!(f.len(), 2);
        let big = f.iter().find(|f| f.holes.len() == 1).unwrap();
        assert!((big.area() - 500.0).abs() < 1e-6);
        assert!(ring_area2(&big.holes[0]) < 0.0);
    }

    #[test]
    fn union_all_ignores_orientation() {
        let a = rect([0.0, 0.0], [10.0, 10.0]);
        let mut b = rect([5.0, 0.0], [15.0, 10.0]);
        b.reverse();
        let u = union_all([a, b]);
        assert!((area(&u) - 150.0).abs() < 1e-6);
    }

    #[test]
    fn stroke_area() {
        let s = stroke(&[[0.0, 0.0], [10.0, 0.0]], 2.0);
        let expect = 20.0 + std::f64::consts::PI;
        assert!((area(&s) - expect).abs() < 0.02);
    }

    #[test]
    fn circle_area() {
        let c = circle([1.0, 2.0], 5.0);
        let a = ring_area2(&c) / 2.0;
        assert!((a - std::f64::consts::PI * 25.0).abs() < 0.1);
    }
}

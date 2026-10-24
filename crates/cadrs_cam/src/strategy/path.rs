//! Polyline helpers the strategies share: chaining edges into loops, hatching an area, picking
//! where a loop starts, ramping down along a path and lifting over tabs.

use crate::geom::{P2, P3, add2, cross2, dist2, dot2, len2, lerp2, ring_area2, scale2, sub2};
use crate::region::{self, Region};

/// A path to cut: points in order, closed (the last point joins the first) or open.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub pts: Vec<P2>,
    pub closed: bool,
}

impl Contour {
    pub fn closed(pts: Vec<P2>) -> Contour {
        Contour { pts, closed: true }
    }

    pub fn open(pts: Vec<P2>) -> Contour {
        Contour { pts, closed: false }
    }

    pub fn len(&self) -> f64 {
        let mut l: f64 = self.pts.windows(2).map(|w| dist2(w[0], w[1])).sum();
        if self.closed && self.pts.len() > 1 {
            l += dist2(self.pts[self.pts.len() - 1], self.pts[0]);
        }
        l
    }

    /// Counter-clockwise (only meaningful when closed).
    pub fn ccw(&self) -> bool {
        ring_area2(&self.pts) > 0.0
    }

    pub fn reversed(&self) -> Contour {
        let mut pts = self.pts.clone();
        pts.reverse();
        Contour {
            pts,
            closed: self.closed,
        }
    }

    /// A closed contour started at its point nearest to `p` (a new vertex there if the nearest
    /// place is along a segment).
    pub fn starting_near(&self, p: P2) -> Contour {
        if !self.closed || self.pts.len() < 2 {
            return self.clone();
        }
        let n = self.pts.len();
        let mut best = (f64::INFINITY, 0usize, self.pts[0]);
        for i in 0..n {
            let (a, b) = (self.pts[i], self.pts[(i + 1) % n]);
            let d = sub2(b, a);
            let l2 = dot2(d, d);
            let t = if l2 > 0.0 {
                (dot2(sub2(p, a), d) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let q = add2(a, scale2(d, t));
            let dd = dist2(p, q);
            if dd < best.0 - 1e-12 {
                best = (dd, i, q);
            }
        }
        let (_, i, q) = best;
        let mut pts = Vec::with_capacity(n + 1);
        pts.push(q);
        for k in 1..=n {
            let v = self.pts[(i + k) % n];
            if dist2(v, *pts.last().unwrap()) > 1e-9 && (k < n || dist2(v, q) > 1e-9) {
                pts.push(v);
            }
        }
        if dist2(*pts.last().unwrap(), q) < 1e-9 {
            pts.pop();
        }
        Contour::closed(pts)
    }

    /// The points walked in order, the closing point repeated for a closed contour.
    pub fn walk(&self) -> Vec<P2> {
        let mut v = self.pts.clone();
        if self.closed && !v.is_empty() {
            v.push(v[0]);
        }
        v
    }

    /// The point and unit tangent at the start.
    pub fn start_tangent(&self) -> Option<(P2, P2)> {
        let w = self.walk();
        let p = *w.first()?;
        let q = w.iter().skip(1).find(|q| dist2(**q, p) > 1e-9)?;
        let d = sub2(*q, p);
        Some((p, scale2(d, 1.0 / len2(d))))
    }

    /// The point and unit tangent at the end (arriving).
    pub fn end_tangent(&self) -> Option<(P2, P2)> {
        let w = self.walk();
        let p = *w.last()?;
        let q = w.iter().rev().skip(1).find(|q| dist2(**q, p) > 1e-9)?;
        let d = sub2(p, *q);
        Some((p, scale2(d, 1.0 / len2(d))))
    }
}

/// The rings of a region as closed contours.
pub fn region_contours(r: &Region) -> Vec<Contour> {
    r.iter()
        .filter(|ring| ring.len() >= 3)
        .map(|ring| Contour::closed(ring.clone()))
        .collect()
}

/// Chains 3D polylines that meet end to end (within `tol`) into contours (projected on XY).
/// A chain that comes back to where it started is closed.
pub fn chain(polylines: &[Vec<P3>], tol: f64) -> Vec<Contour> {
    let mut left: Vec<Vec<P2>> = polylines
        .iter()
        .filter(|p| p.len() >= 2)
        .map(|p| p.iter().map(|q| [q[0], q[1]]).collect())
        .collect();
    let mut out = Vec::new();
    while let Some(mut cur) = left.pop() {
        loop {
            let head = cur[0];
            let tail = *cur.last().unwrap();
            if cur.len() > 2 && dist2(head, tail) <= tol {
                cur.pop();
                out.push(Contour::closed(cur));
                break;
            }
            let found = left.iter().position(|p| {
                dist2(p[0], tail) <= tol
                    || dist2(*p.last().unwrap(), tail) <= tol
                    || dist2(p[0], head) <= tol
                    || dist2(*p.last().unwrap(), head) <= tol
            });
            let Some(i) = found else {
                out.push(Contour::open(cur));
                break;
            };
            let mut p = left.swap_remove(i);
            if dist2(p[0], tail) <= tol {
                cur.extend(p.into_iter().skip(1));
            } else if dist2(*p.last().unwrap(), tail) <= tol {
                p.reverse();
                cur.extend(p.into_iter().skip(1));
            } else if dist2(*p.last().unwrap(), head) <= tol {
                p.pop();
                p.extend(cur);
                cur = p;
            } else {
                p.reverse();
                p.pop();
                p.extend(cur);
                cur = p;
            }
        }
    }
    out
}

/// An open polyline offset sideways by `d` (positive: to the left of its direction), with
/// mitred joins (limited to 4×`d`).
pub fn offset_open(pts: &[P2], d: f64) -> Vec<P2> {
    let n = pts.len();
    if n < 2 || d == 0.0 {
        return pts.to_vec();
    }
    let normal = |a: P2, b: P2| {
        let t = sub2(b, a);
        let l = len2(t).max(1e-12);
        [-t[1] / l, t[0] / l]
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let n0 = (i > 0).then(|| normal(pts[i - 1], pts[i]));
        let n1 = (i + 1 < n).then(|| normal(pts[i], pts[i + 1]));
        let m = match (n0, n1) {
            (Some(a), Some(b)) => {
                let s = add2(a, b);
                let l = len2(s);
                if l < 1e-9 {
                    a
                } else {
                    let m = scale2(s, 1.0 / l);
                    let c = dot2(m, a).max(0.25);
                    scale2(m, 1.0 / c)
                }
            }
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => [0.0, 0.0],
        };
        out.push(add2(pts[i], scale2(m, d)));
    }
    out
}

/// Parallel lines across a region at `angle` (degrees from +X), `spacing` apart: the pieces
/// inside the region, ordered in rows, alternately reversed when `zigzag`.
pub fn hatch(r: &Region, angle: f64, spacing: f64, zigzag: bool) -> Vec<Vec<P2>> {
    if r.is_empty() || spacing <= 0.0 {
        return Vec::new();
    }
    let (s, c) = angle.to_radians().sin_cos();
    // Rotate so the lines run along X.
    let rot = |p: P2| [p[0] * c + p[1] * s, -p[0] * s + p[1] * c];
    let unrot = |p: P2| [p[0] * c - p[1] * s, p[0] * s + p[1] * c];
    let rr: Vec<Vec<P2>> = r
        .iter()
        .map(|ring| ring.iter().map(|p| rot(*p)).collect())
        .collect();
    let (ymin, ymax) = rr
        .iter()
        .flatten()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p[1]), b.max(p[1]))
        });
    let h = ymax - ymin;
    let rows = ((h / spacing).ceil() as usize).max(1);
    // Spread the rows evenly so the first and last sit half a spacing in.
    let step = h / rows as f64;
    let mut out = Vec::new();
    for k in 0..rows {
        let y = ymin + step * (k as f64 + 0.5);
        let mut xs = Vec::new();
        for ring in &rr {
            let n = ring.len();
            for i in 0..n {
                let (a, b) = (ring[i], ring[(i + 1) % n]);
                if (a[1] > y) != (b[1] > y) {
                    xs.push(a[0] + (y - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let mut segs: Vec<Vec<P2>> = xs
            .chunks(2)
            .filter(|p| p.len() == 2 && p[1] - p[0] > 1e-6)
            .map(|p| vec![unrot([p[0], y]), unrot([p[1], y])])
            .collect();
        if zigzag && k % 2 == 1 {
            segs.reverse();
            for s in &mut segs {
                s.reverse();
            }
        }
        out.extend(segs);
    }
    out
}

/// Points along a polyline every `step` at most (the corners kept).
pub fn densify(pts: &[P2], step: f64) -> Vec<P2> {
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let n = ((dist2(w[0], w[1]) / step).ceil() as usize).max(1);
        for i in 0..n {
            out.push(lerp2(w[0], w[1], i as f64 / n as f64));
        }
    }
    if let Some(l) = pts.last() {
        out.push(*l);
    }
    out
}

/// A closed contour walked from its start, descending from `z0` to `z1` at `angle` (degrees
/// from horizontal) along it, lap after lap as needed; then once round at `z1` from where the
/// ramp ended back to it. An open contour ramps back and forth along its first stretch.
pub fn ramp_along(c: &Contour, z0: f64, z1: f64, angle: f64) -> Vec<P3> {
    let walk = c.walk();
    if walk.len() < 2 || z0 <= z1 + 1e-9 {
        return walk.iter().map(|p| [p[0], p[1], z1]).collect();
    }
    let slope = angle.clamp(0.5, 89.0).to_radians().tan();
    let need = (z0 - z1) / slope;
    let total = c.len();
    if total < 1e-6 {
        return vec![[walk[0][0], walk[0][1], z1]];
    }
    let mut out = vec![[walk[0][0], walk[0][1], z0]];
    if c.closed {
        let mut travelled = 0.0;
        let mut lap_pts = walk.clone();
        lap_pts.remove(0);
        // Descend.
        let mut i = 0usize;
        let mut prev = walk[0];
        let n = lap_pts.len();
        loop {
            let p = lap_pts[i % n];
            let l = dist2(prev, p);
            if travelled + l >= need {
                let t = if l > 0.0 { (need - travelled) / l } else { 1.0 };
                let q = lerp2(prev, p, t);
                out.push([q[0], q[1], z1]);
                // Once round at depth from q: the rest of this segment, the loop, back to q.
                for k in 0..n {
                    let r = lap_pts[(i + k) % n];
                    out.push([r[0], r[1], z1]);
                }
                out.push([q[0], q[1], z1]);
                break;
            }
            travelled += l;
            out.push([p[0], p[1], z0 - travelled * slope]);
            prev = p;
            i += 1;
            if i > n * 1000 {
                break;
            }
        }
        dedup3(&mut out);
        out
    } else {
        // Back and forth along the start until at depth, then the path.
        let first = c.pts[0];
        let second = c.pts[1];
        let seg = dist2(first, second).max(1e-6);
        let mut z = z0;
        let mut at_first = true;
        while z > z1 + 1e-9 {
            let next = (z - seg * slope).max(z1);
            let p = if at_first { second } else { first };
            // A partial stroke when it reaches depth before the end of the segment.
            let frac = ((z - next) / slope / seg).min(1.0);
            let from = if at_first { first } else { second };
            let q = lerp2(from, p, frac);
            out.push([q[0], q[1], next]);
            z = next;
            if frac < 1.0 - 1e-9 {
                // Back to the start of the stroke at depth.
                out.push([from[0], from[1], z1]);
                at_first = true;
                break;
            }
            at_first = !at_first;
        }
        if !at_first {
            out.push([first[0], first[1], z1]);
        }
        for p in &c.pts {
            out.push([p[0], p[1], z1]);
        }
        dedup3(&mut out);
        out
    }
}

pub fn dedup3(v: &mut Vec<P3>) {
    v.dedup_by(|a, b| {
        (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9 && (a[2] - b[2]).abs() < 1e-9
    });
}

/// A closed contour's points at `z`, lifted to `tab_top` over `count` tabs (`width` long,
/// evenly spaced, measured along the path), with the tab's ends as their own points.
pub fn with_tabs(walk: &[P2], z: f64, tab_top: f64, count: usize, width: f64) -> Vec<P3> {
    if count == 0 || tab_top <= z || walk.len() < 2 {
        return walk.iter().map(|p| [p[0], p[1], z]).collect();
    }
    let total: f64 = walk.windows(2).map(|w| dist2(w[0], w[1])).sum();
    if width * count as f64 >= total * 0.8 {
        return walk.iter().map(|p| [p[0], p[1], z]).collect();
    }
    let spacing = total / count as f64;
    // Tab spans, centred along the path.
    let spans: Vec<(f64, f64)> = (0..count)
        .map(|k| {
            let c = spacing * (k as f64 + 0.5);
            (c - width / 2.0, c + width / 2.0)
        })
        .collect();
    let in_tab = |s: f64| spans.iter().any(|(a, b)| s > *a + 1e-9 && s < *b - 1e-9);
    let mut cuts: Vec<f64> = spans.iter().flat_map(|(a, b)| [*a, *b]).collect();
    cuts.sort_by(f64::total_cmp);
    let mut out: Vec<P3> = Vec::new();
    let mut s = 0.0;
    let mut ci = 0;
    let height = |s: f64| if in_tab(s) { tab_top } else { z };
    out.push([walk[0][0], walk[0][1], height(0.0)]);
    for w in walk.windows(2) {
        let l = dist2(w[0], w[1]);
        while ci < cuts.len() && cuts[ci] <= s + l {
            let t = if l > 0.0 { (cuts[ci] - s) / l } else { 0.0 };
            let q = lerp2(w[0], w[1], t.clamp(0.0, 1.0));
            // Before the cut point at the old height, then up or down.
            let before = height(cuts[ci] - 1e-6);
            let after = height(cuts[ci] + 1e-6);
            out.push([q[0], q[1], before]);
            out.push([q[0], q[1], after]);
            ci += 1;
        }
        s += l;
        out.push([w[1][0], w[1][1], height(s - 1e-6)]);
    }
    dedup3(&mut out);
    out
}

/// The part of a walk between arc lengths `s0` and `s1`.
pub fn sub_walk(walk: &[P2], s0: f64, s1: f64) -> Vec<P2> {
    let mut out = Vec::new();
    let mut s = 0.0;
    for w in walk.windows(2) {
        let l = dist2(w[0], w[1]);
        let (a, b) = (s, s + l);
        s = b;
        if l <= 0.0 || b < s0 || a > s1 {
            continue;
        }
        if out.is_empty() {
            out.push(lerp2(w[0], w[1], ((s0 - a) / l).clamp(0.0, 1.0)));
        }
        if b <= s1 {
            out.push(w[1]);
        } else {
            out.push(lerp2(w[0], w[1], ((s1 - a) / l).clamp(0.0, 1.0)));
            break;
        }
    }
    out
}

/// A tangent arc lead of `radius` sweeping `sweep_deg` onto (or off) the point `p` with unit
/// tangent `t`, on the side `side` (+1 left of the tangent, −1 right). Returns the far end and
/// the arc's centre and direction (clockwise), for a lead-in arc from the far end to `p`.
pub fn lead_arc(p: P2, t: P2, side: f64, radius: f64, sweep_deg: f64) -> (P2, P2, bool) {
    let n = [-t[1] * side, t[0] * side];
    let c = add2(p, scale2(n, radius));
    // Angle of p seen from c, then back by the sweep (against the motion direction).
    let a = (p[1] - c[1]).atan2(p[0] - c[0]);
    // Moving along t at p round c: counter-clockwise when the centre is on the left.
    let ccw = side > 0.0;
    let sweep = sweep_deg.to_radians();
    let a0 = if ccw { a - sweep } else { a + sweep };
    let start = [c[0] + radius * a0.cos(), c[1] + radius * a0.sin()];
    (start, c, !ccw)
}

/// The same, leaving `p` (a lead-out): the far end the arc goes to.
pub fn lead_out_arc(p: P2, t: P2, side: f64, radius: f64, sweep_deg: f64) -> (P2, P2, bool) {
    let n = [-t[1] * side, t[0] * side];
    let c = add2(p, scale2(n, radius));
    let a = (p[1] - c[1]).atan2(p[0] - c[0]);
    let ccw = side > 0.0;
    let sweep = sweep_deg.to_radians();
    let a1 = if ccw { a + sweep } else { a - sweep };
    (
        [c[0] + radius * a1.cos(), c[1] + radius * a1.sin()],
        c,
        !ccw,
    )
}

/// Which side of a closed contour's start tangent is outside it: +1 left, −1 right.
pub fn outside_side(c: &Contour) -> f64 {
    if c.ccw() { -1.0 } else { 1.0 }
}

/// Orders contours greedily, nearest start first from `from` (closed ones may start
/// anywhere, so they're restarted at their point nearest the tool).
pub fn nearest_order(cs: Vec<Contour>, from: P2) -> Vec<Contour> {
    nearest_order_indexed(cs, from)
        .into_iter()
        .map(|(_, c)| c)
        .collect()
}

/// [`nearest_order`], each contour with its index in `cs`.
pub fn nearest_order_indexed(cs: Vec<Contour>, mut from: P2) -> Vec<(usize, Contour)> {
    let mut left: Vec<(usize, Contour)> = cs.into_iter().enumerate().collect();
    let mut out = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let (i, c) = left
            .iter()
            .enumerate()
            .map(|(i, (_, c))| {
                let c2 = if c.closed {
                    c.starting_near(from)
                } else {
                    c.clone()
                };
                let d = dist2(from, c2.pts[0]);
                // An open contour may run either way.
                let r = (!c.closed).then(|| c.reversed());
                match r {
                    Some(r) if dist2(from, r.pts[0]) < d => {
                        let dr = dist2(from, r.pts[0]);
                        (i, r, dr)
                    }
                    _ => (i, c2, d),
                }
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, c, _)| (i, c))
            .unwrap();
        let (k, _) = left.swap_remove(i);
        from = *c.walk().last().unwrap();
        out.push((k, c));
    }
    out
}

/// How many rings of a region contain the point (its nesting depth).
pub fn depth_in(rings: &[Vec<P2>], p: P2) -> usize {
    rings
        .iter()
        .filter(|r| crate::geom::ring_contains(r, p))
        .count()
}

/// A point well inside a closed ring (for a pierce or a probe): the middle of what is left
/// after shrinking the ring as far as it goes.
pub fn inner_point(ring: &[P2]) -> P2 {
    let mut cur = vec![ring.to_vec()];
    let step = (region::area(&cur).abs().sqrt() / 40.0).max(0.01);
    for _ in 0..60 {
        let next = region::offset(&cur, -step);
        if next.is_empty() {
            break;
        }
        cur = next;
    }
    let pts = cur.first().map_or(ring, |r| r.as_slice());
    let n = pts.len().max(1) as f64;
    [
        pts.iter().map(|p| p[0]).sum::<f64>() / n,
        pts.iter().map(|p| p[1]).sum::<f64>() / n,
    ]
}

/// The unit vector of `d`.
pub fn unit(d: P2) -> P2 {
    let l = len2(d);
    if l > 1e-12 {
        scale2(d, 1.0 / l)
    } else {
        [1.0, 0.0]
    }
}

/// The turn at `b` from `a`→`b` to `b`→`c` (positive: left).
pub fn turn(a: P2, b: P2, c: P2) -> f64 {
    cross2(sub2(b, a), sub2(c, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square() -> Contour {
        Contour::closed(vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]])
    }

    #[test]
    fn chain_joins_reversed_pieces() {
        let pieces = vec![
            vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]],
            vec![[10.0, 10.0, 0.0], [10.0, 0.0, 0.0]],
            vec![[10.0, 10.0, 0.0], [0.0, 10.0, 0.0]],
            vec![[0.0, 10.0, 0.0], [0.0, 0.0, 0.0]],
            vec![[50.0, 0.0, 0.0], [60.0, 0.0, 0.0]],
        ];
        let c = chain(&pieces, 1e-6);
        assert_eq!(c.iter().filter(|c| c.closed).count(), 1);
        assert_eq!(c.iter().filter(|c| !c.closed).count(), 1);
        let sq = c.iter().find(|c| c.closed).unwrap();
        assert_eq!(sq.pts.len(), 4);
        assert!((sq.len() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn start_near_splits_segment() {
        let s = square().starting_near([5.0, -3.0]);
        assert_eq!(s.pts[0], [5.0, 0.0]);
        assert_eq!(s.pts.len(), 5);
        assert!((s.len() - 40.0).abs() < 1e-9);
    }

    #[test]
    fn hatch_covers_square() {
        let lines = hatch(&vec![square().pts], 0.0, 2.0, true);
        assert_eq!(lines.len(), 5);
        assert!((lines[0][0][1] - 1.0).abs() < 1e-9);
        // Zigzag: the second row runs back.
        assert!(lines[1][0][0] > lines[1][1][0]);
    }

    #[test]
    fn ramp_reaches_depth_and_laps() {
        let pts = ramp_along(&square(), 0.0, -2.0, 5.0);
        // Ramp length 2/tan5° ≈ 22.9 mm, then a full lap at depth.
        assert!((pts[0][2]).abs() < 1e-12);
        let first_deep = pts.iter().position(|p| (p[2] + 2.0).abs() < 1e-9).unwrap();
        let ramp_len: f64 = pts[..=first_deep]
            .windows(2)
            .map(|w| dist2([w[0][0], w[0][1]], [w[1][0], w[1][1]]))
            .sum();
        assert!((ramp_len - 2.0 / 5f64.to_radians().tan()).abs() < 1e-6);
        let lap: f64 = pts[first_deep..]
            .windows(2)
            .map(|w| dist2([w[0][0], w[0][1]], [w[1][0], w[1][1]]))
            .sum();
        assert!((lap - 40.0).abs() < 1e-6);
    }

    #[test]
    fn tabs_lift_the_path() {
        let w = square().walk();
        let pts = with_tabs(&w, -5.0, -3.0, 4, 2.0);
        let up = pts.iter().filter(|p| (p[2] + 3.0).abs() < 1e-9).count();
        assert!(up >= 8);
        assert!(pts.iter().all(|p| p[2] == -5.0 || p[2] == -3.0));
    }

    #[test]
    fn lead_arc_is_tangent() {
        let (s, c, cw) = lead_arc([0.0, 0.0], [1.0, 0.0], -1.0, 2.0, 90.0);
        assert!((c[1] + 2.0).abs() < 1e-12);
        // Right of the tangent, coming in: from (−2, −2), clockwise.
        assert!(
            (s[0] + 2.0).abs() < 1e-9 && (s[1] + 2.0).abs() < 1e-9,
            "{s:?}"
        );
        assert!(cw);
    }

    #[test]
    fn open_offset_left() {
        let o = offset_open(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]], 1.0);
        assert_eq!(o[0], [0.0, 1.0]);
        assert!((o[1][0] - 9.0).abs() < 1e-9 && (o[1][1] - 1.0).abs() < 1e-9);
    }
}

//! The union of regions of one plane bounded by line segments and circular arcs, exact: every
//! boundary piece cut where another region's boundary meets it (lines and circles meet in
//! closed form), the pieces within another region left out, the rest chained into the union's
//! outlines and holes, and pieces of one line or circle that follow one another made one again.
//! What an end mill fills at one depth of a pocket (every run's slot and every plunge's disc)
//! is one such figure, cut once, where its solids united in space would be a boolean each.

use crate::types::{Curve2, Loop, Region};
use crate::{KernelError, Result};
use std::f64::consts::TAU;

type P2 = nalgebra::Point2<f64>;
type V2 = nalgebra::Vector2<f64>;

/// A boundary piece: a segment, or an arc from the angle `a0` by the signed sweep `sw` (a whole
/// circle's is ±τ).
#[derive(Clone, Copy, Debug)]
enum G {
    Line { a: P2, b: P2 },
    Arc { c: P2, r: f64, a0: f64, sw: f64 },
}

impl G {
    fn at(&self, s: f64) -> P2 {
        match *self {
            G::Line { a, b } => a + (b - a) * s,
            G::Arc { c, r, a0, sw } => {
                let t = a0 + sw * s;
                c + V2::new(t.cos(), t.sin()) * r
            }
        }
    }
    /// The way it runs at `s` (a unit vector).
    fn dir(&self, s: f64) -> V2 {
        match *self {
            G::Line { a, b } => (b - a).normalize(),
            G::Arc { a0, sw, .. } => {
                let t = a0 + sw * s;
                V2::new(-t.sin(), t.cos()) * sw.signum()
            }
        }
    }
    fn length(&self) -> f64 {
        match *self {
            G::Line { a, b } => (b - a).norm(),
            G::Arc { r, sw, .. } => r * sw.abs(),
        }
    }
    fn whole(&self) -> bool {
        matches!(*self, G::Arc { sw, .. } if (sw.abs() - TAU).abs() < 1e-12)
    }
    /// Where `p` lies along it (0 to 1), when it lies on it within `tol`.
    fn param(&self, p: &P2, tol: f64) -> Option<f64> {
        let len = self.length();
        let s = match *self {
            G::Line { a, b } => {
                let d = b - a;
                let s = (p - a).dot(&d) / d.norm_squared();
                if (a + d * s - p).norm() > tol {
                    return None;
                }
                s
            }
            G::Arc { c, r, a0, sw } => {
                let v = p - c;
                if (v.norm() - r).abs() > tol {
                    return None;
                }
                // The angle from a0 the way the arc runs, in [0, τ), then nearest to its span.
                let mut t = ((v.y.atan2(v.x) - a0) * sw.signum()).rem_euclid(TAU);
                if t > sw.abs() && TAU - t < t - sw.abs() {
                    t -= TAU;
                }
                t / sw.abs()
            }
        };
        let e = tol / len;
        (s > -e && s < 1.0 + e).then(|| s.clamp(0.0, 1.0))
    }
    fn part(&self, s0: f64, s1: f64) -> G {
        match *self {
            G::Line { .. } => G::Line {
                a: self.at(s0),
                b: self.at(s1),
            },
            G::Arc { c, r, a0, sw } => G::Arc {
                c,
                r,
                a0: a0 + sw * s0,
                sw: sw * (s1 - s0),
            },
        }
    }
    fn reversed(&self) -> G {
        match *self {
            G::Line { a, b } => G::Line { a: b, b: a },
            G::Arc { c, r, a0, sw } => G::Arc {
                c,
                r,
                a0: a0 + sw,
                sw: -sw,
            },
        }
    }
    /// Twice the area between it and the origin, signed (a loop's pieces sum to twice its
    /// area, counter-clockwise positive).
    fn area2(&self) -> f64 {
        let (p, q) = (self.at(0.0), self.at(1.0));
        let chord = p.x * q.y - q.x * p.y;
        match *self {
            G::Line { .. } => chord,
            G::Arc { r, sw, .. } => {
                // (A whole circle's chord is nothing and its segment the disc.)
                chord + r * r * (sw - sw.sin())
            }
        }
    }
    fn bounds(&self) -> [f64; 4] {
        match *self {
            G::Line { a, b } => [a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y)],
            // (The circle's: generous for a short arc, and only a filter.)
            G::Arc { c, r, .. } => [c.x - r, c.y - r, c.x + r, c.y + r],
        }
    }
    /// How often the ray from `q` along `d` (a unit vector) crosses it, its end not counted.
    fn crossings(&self, q: &P2, d: &V2) -> usize {
        let n = V2::new(-d.y, d.x);
        match *self {
            G::Line { a, b } => {
                let (ha, hb) = ((a - q).dot(&n), (b - q).dot(&n));
                if (ha > 0.0) == (hb > 0.0) {
                    return 0;
                }
                let s = ha / (ha - hb);
                usize::from((a + (b - a) * s - q).dot(d) > 0.0)
            }
            G::Arc { c, r, a0, sw } => {
                let v = c - q;
                let (along, off) = (v.dot(d), v.dot(&n));
                let h2 = r * r - off * off;
                if h2 <= 0.0 {
                    return 0;
                }
                let h = h2.sqrt();
                [along - h, along + h]
                    .into_iter()
                    .filter(|&t| t > 0.0)
                    .filter(|&t| {
                        let p = q + d * t - c;
                        let s = ((p.y.atan2(p.x) - a0) * sw.signum()).rem_euclid(TAU) / sw.abs();
                        s < 1.0
                    })
                    .count()
            }
        }
    }
}

fn boxes_meet(a: &[f64; 4], b: &[f64; 4], tol: f64) -> bool {
    a[0] <= b[2] + tol && b[0] <= a[2] + tol && a[1] <= b[3] + tol && b[1] <= a[3] + tol
}

/// Where the carriers of `p` and `q` meet (their whole line or circle), in closed form; `same`
/// when they are one line or one circle.
fn carriers_meet(p: &G, q: &G, tol: f64) -> (Vec<P2>, bool) {
    match (*p, *q) {
        (G::Line { a, b }, G::Line { a: c, b: d }) => {
            let (u, v) = ((b - a).normalize(), (d - c).normalize());
            let cross = u.x * v.y - u.y * v.x;
            let w = c - a;
            let off = (u.x * w.y - u.y * w.x).abs();
            if cross.abs() < 1e-12 {
                return (vec![], off <= tol);
            }
            let t = (w.x * v.y - w.y * v.x) / cross;
            (vec![a + u * t], false)
        }
        (G::Line { a, b }, G::Arc { c, r, .. }) | (G::Arc { c, r, .. }, G::Line { a, b }) => {
            let u = (b - a).normalize();
            let foot = a + u * (c - a).dot(&u);
            let d = (foot - c).norm();
            if d > r + tol {
                (vec![], false)
            } else if d > r - tol {
                // Touching: the one point.
                (vec![foot], false)
            } else {
                let h = (r * r - d * d).sqrt();
                (vec![foot - u * h, foot + u * h], false)
            }
        }
        (G::Arc { c, r, .. }, G::Arc { c: c2, r: r2, .. }) => {
            let v = c2 - c;
            let d = v.norm();
            if d <= tol && (r - r2).abs() <= tol {
                return (vec![], true);
            }
            if d > r + r2 + tol || d < (r - r2).abs() - tol || d < 1e-300 {
                return (vec![], false);
            }
            let u = v / d;
            if d > r + r2 - tol {
                return (vec![c + u * (0.5 * (d + r - r2))], false);
            }
            if d < (r - r2).abs() + tol {
                let out = if r > r2 { u } else { -u };
                return (vec![c + out * r], false);
            }
            let x = (d * d + r * r - r2 * r2) / (2.0 * d);
            let h = (r * r - x * x).max(0.0).sqrt();
            let n = V2::new(-u.y, u.x);
            (vec![c + u * x + n * h, c + u * x - n * h], false)
        }
    }
}

/// A region's pieces, its ground to the left of each (outline counter-clockwise, holes
/// clockwise), and its box.
struct Flat {
    pieces: Vec<G>,
    bounds: [f64; 4],
}

impl Flat {
    /// Where `q` stands to the region: inside it (1), outside (0), or on its boundary, the way
    /// the boundary runs there given.
    fn locate(&self, q: &P2, tol: f64) -> Stand {
        if !boxes_meet(&self.bounds, &[q.x, q.y, q.x, q.y], tol) {
            return Stand::Out;
        }
        for g in &self.pieces {
            if let Some(s) = g.param(q, tol) {
                return Stand::On(g.dir(s));
            }
        }
        // (A ray no line of an upright program runs along.)
        let d = V2::new(0.7368_f64.cos(), 0.7368_f64.sin());
        let n: usize = self.pieces.iter().map(|g| g.crossings(q, &d)).sum();
        if n % 2 == 1 { Stand::In } else { Stand::Out }
    }
}

enum Stand {
    In,
    Out,
    On(V2),
}

fn pieces_of(l: &Loop) -> Result<Vec<G>> {
    l.curves
        .iter()
        .map(|c| {
            Ok(match *c {
                Curve2::Line { a, b, .. } => G::Line { a, b },
                Curve2::Arc {
                    center,
                    radius,
                    start_angle,
                    sweep,
                    ..
                } => G::Arc {
                    c: center,
                    r: radius,
                    a0: start_angle,
                    sw: sweep,
                },
                Curve2::Circle { center, radius, .. } => G::Arc {
                    c: center,
                    r: radius.abs(),
                    a0: 0.0,
                    sw: TAU.copysign(radius),
                },
                _ => {
                    return Err(KernelError::Unsupported(
                        "a union of regions not of lines and arcs",
                    ));
                }
            })
        })
        .collect()
}

fn turned(mut l: Vec<G>, ccw: bool) -> Vec<G> {
    let area: f64 = l.iter().map(G::area2).sum();
    if (area > 0.0) != ccw {
        l.reverse();
        for g in &mut l {
            *g = g.reversed();
        }
    }
    l
}

/// The union of `regions` (each an outline and holes of line segments, arcs and circles, in one
/// plane): its pieces of ground, each an outline with its holes. With `from`, also whether any
/// of the union's boundary is of a region from that index on: false when those add nothing to
/// the regions before them.
pub fn union(regions: &[Region], from: Option<usize>) -> Result<(Vec<Region>, bool)> {
    let mut flats = Vec::with_capacity(regions.len());
    let mut all = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for r in regions {
        let mut pieces = turned(pieces_of(&r.outer)?, true);
        for h in &r.holes {
            pieces.extend(turned(pieces_of(h)?, false));
        }
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for g in &pieces {
            let b = g.bounds();
            bounds = [
                bounds[0].min(b[0]),
                bounds[1].min(b[1]),
                bounds[2].max(b[2]),
                bounds[3].max(b[3]),
            ];
        }
        all = [
            all[0].min(bounds[0]),
            all[1].min(bounds[1]),
            all[2].max(bounds[2]),
            all[3].max(bounds[3]),
        ];
        flats.push(Flat { pieces, bounds });
    }
    let size = (all[2] - all[0]).hypot(all[3] - all[1]);
    if !size.is_finite() || size <= 0.0 {
        return Err(KernelError::Unsupported("a union of no regions"));
    }
    let tol = 1e-9 * size;
    // Every piece, with its region, and where the other regions' pieces meet it.
    let index: Vec<(usize, usize)> = flats
        .iter()
        .enumerate()
        .flat_map(|(i, f)| (0..f.pieces.len()).map(move |j| (i, j)))
        .collect();
    let boxes: Vec<[f64; 4]> = index
        .iter()
        .map(|&(i, j)| flats[i].pieces[j].bounds())
        .collect();
    let mut cuts: Vec<Vec<f64>> = vec![Vec::new(); index.len()];
    for a in 0..index.len() {
        let (ia, ja) = index[a];
        for b in a + 1..index.len() {
            let (ib, jb) = index[b];
            if ia == ib || !boxes_meet(&boxes[a], &boxes[b], tol) {
                continue;
            }
            let (p, q) = (flats[ia].pieces[ja], flats[ib].pieces[jb]);
            let (mut pts, same) = carriers_meet(&p, &q, tol);
            if same {
                // One line or circle: each is cut at the other's ends.
                pts = vec![p.at(0.0), p.at(1.0), q.at(0.0), q.at(1.0)];
            }
            for pt in pts {
                if let (Some(s), Some(t)) = (p.param(&pt, tol), q.param(&pt, tol)) {
                    cuts[a].push(s);
                    cuts[b].push(t);
                }
            }
        }
    }
    // The parts between the cuts, kept where no other region covers them.
    let mut kept: Vec<(G, usize)> = Vec::new();
    for (a, &(i, j)) in index.iter().enumerate() {
        let g = flats[i].pieces[j];
        let e = tol / g.length();
        let mut ss = std::mem::take(&mut cuts[a]);
        ss.retain(|s| *s > e && *s < 1.0 - e);
        ss.sort_by(f64::total_cmp);
        ss.dedup_by(|x, y| (*x - *y).abs() <= e);
        let mut ends = vec![0.0];
        ends.extend(ss);
        ends.push(1.0);
        'parts: for w in ends.windows(2) {
            let part = g.part(w[0], w[1]);
            let (m, way) = (part.at(0.5), part.dir(0.5));
            for (o, f) in flats.iter().enumerate() {
                if o == i {
                    continue;
                }
                match f.locate(&m, tol) {
                    Stand::In => continue 'parts,
                    // Along another region's boundary: the same way, one of the two is the
                    // union's (the earlier region's); against it, ground on both sides.
                    Stand::On(d) if d.dot(&way) < 0.0 || o < i => continue 'parts,
                    _ => {}
                }
            }
            kept.push((part, i));
        }
    }
    let adds = from.is_some_and(|n| kept.iter().any(|k| k.1 >= n));
    // Chained end to start into loops; where several start at one point (outlines touching
    // there), the one turning furthest to the right, which keeps each outline its own loop.
    let near = 1e3 * tol;
    let mut used = vec![false; kept.len()];
    let mut loops: Vec<Vec<G>> = Vec::new();
    for first in 0..kept.len() {
        if used[first] {
            continue;
        }
        used[first] = true;
        let start = kept[first].0.at(0.0);
        let mut lp = vec![kept[first].0];
        loop {
            let last = lp[lp.len() - 1];
            let (end, way) = (last.at(1.0), last.dir(1.0));
            if (end - start).norm() <= near
                && !(lp.len() == 1 && !last.whole() && last.length() <= near)
            {
                break;
            }
            let next = (0..kept.len())
                .filter(|&k| !used[k] && (kept[k].0.at(0.0) - end).norm() <= near)
                .min_by(|&x, &y| {
                    let turn = |k: usize| {
                        let d = kept[k].0.dir(0.0);
                        (way.x * d.y - way.y * d.x).atan2(way.dot(&d))
                    };
                    turn(x).total_cmp(&turn(y))
                });
            let Some(next) = next else {
                return Err(KernelError::Unsupported(
                    "a union whose boundary doesn't close",
                ));
            };
            used[next] = true;
            lp.push(kept[next].0);
        }
        loops.push(fused(lp, tol));
    }
    // Counter-clockwise loops are outlines; each clockwise one a hole of the smallest outline
    // round it.
    let area = |l: &[G]| 0.5 * l.iter().map(G::area2).sum::<f64>();
    let mut outs: Vec<(Vec<G>, f64, Vec<Vec<G>>)> = Vec::new();
    let mut holes = Vec::new();
    for l in loops {
        let a = area(&l);
        if a > 0.0 {
            outs.push((l, a, Vec::new()));
        } else {
            holes.push(l);
        }
    }
    let d = V2::new(0.7368_f64.cos(), 0.7368_f64.sin());
    for h in holes {
        let q = h[0].at(0.5);
        let round = outs
            .iter_mut()
            .filter(|o| o.0.iter().map(|g| g.crossings(&q, &d)).sum::<usize>() % 2 == 1)
            .min_by(|x, y| x.1.total_cmp(&y.1));
        match round {
            Some(o) => o.2.push(h),
            None => return Err(KernelError::Unsupported("a union's hole in no outline")),
        }
    }
    let as_loop = |l: &[G]| Loop {
        curves: l
            .iter()
            .map(|g| match *g {
                G::Line { a, b } => Curve2::Line { a, b, source: None },
                G::Arc { c, r, sw, .. } if g.whole() => Curve2::Circle {
                    center: c,
                    radius: r.copysign(sw),
                    source: None,
                },
                G::Arc { c, r, a0, sw } => Curve2::Arc {
                    center: c,
                    radius: r,
                    start_angle: a0,
                    sweep: sw,
                    source: None,
                },
            })
            .collect(),
    };
    Ok((
        outs.iter()
            .map(|(l, _, hs)| Region {
                outer: as_loop(l),
                holes: hs.iter().map(|h| as_loop(h)).collect(),
                source: None,
            })
            .collect(),
        adds,
    ))
}

/// A loop with the pieces that follow one another along one line or one circle made one.
fn fused(l: Vec<G>, tol: f64) -> Vec<G> {
    let joins = |p: &G, q: &G| -> Option<G> {
        match (*p, *q) {
            (G::Line { a, b }, G::Line { b: d, .. }) => {
                let u = (b - a).normalize();
                let v = d - a;
                ((u.x * v.y - u.y * v.x).abs() <= tol && v.dot(&u) > (b - a).norm())
                    .then_some(G::Line { a, b: d })
            }
            (
                G::Arc { c, r, a0, sw },
                G::Arc {
                    c: c2,
                    r: r2,
                    sw: sw2,
                    ..
                },
            ) => ((c - c2).norm() <= tol
                && (r - r2).abs() <= tol
                && (sw > 0.0) == (sw2 > 0.0)
                && (sw + sw2).abs() <= TAU + 1e-9)
                .then(|| G::Arc {
                    c,
                    r,
                    a0,
                    sw: if ((sw + sw2).abs() - TAU).abs() <= 1e-9 {
                        TAU.copysign(sw)
                    } else {
                        sw + sw2
                    },
                }),
            _ => None,
        }
    };
    // Begun at a piece that doesn't go on from the one before it (any, of a lone circle).
    let n = l.len();
    let begin = (0..n)
        .find(|&i| joins(&l[(i + n - 1) % n], &l[i]).is_none())
        .unwrap_or(0);
    let mut out: Vec<G> = Vec::with_capacity(n);
    for i in 0..n {
        let g = l[(begin + i) % n];
        match out.last().and_then(|p| joins(p, &g)) {
            Some(j) => *out.last_mut().expect("a piece") = j,
            None => out.push(g),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn disc(x: f64, y: f64, r: f64) -> Region {
        Region {
            outer: Loop {
                curves: vec![Curve2::circle(P2::new(x, y), r)],
            },
            holes: vec![],
            source: None,
        }
    }

    fn area(r: &Region) -> f64 {
        let a = |l: &Loop| {
            0.5 * pieces_of(l)
                .unwrap()
                .iter()
                .map(G::area2)
                .sum::<f64>()
                .abs()
        };
        a(&r.outer) - r.holes.iter().map(a).sum::<f64>()
    }

    #[test]
    fn two_discs_crossing_are_a_lens_less() {
        let (u, _) = union(&[disc(0.0, 0.0, 1.0), disc(1.0, 0.0, 1.0)], None).unwrap();
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].outer.curves.len(), 2);
        // Two discs less their lens: 2π − (2π/3 − √3/2).
        let want = TAU - (TAU / 3.0 - 3f64.sqrt() / 2.0);
        assert!((area(&u[0]) - want).abs() < 1e-12, "{}", area(&u[0]));
    }

    #[test]
    fn a_disc_twice_or_within_another_adds_nothing() {
        let (u, adds) = union(
            &[
                disc(0.0, 0.0, 2.0),
                disc(0.0, 0.0, 2.0),
                disc(0.5, 0.0, 1.0),
            ],
            Some(1),
        )
        .unwrap();
        assert_eq!(u.len(), 1);
        assert!(!adds);
        assert!(matches!(u[0].outer.curves[..], [Curve2::Circle { .. }]));
        // (Touching the bigger from within, too.)
        let (u, adds) = union(&[disc(0.0, 0.0, 2.0), disc(1.0, 0.0, 1.0)], Some(1)).unwrap();
        assert!(!adds && u.len() == 1);
        assert!((area(&u[0]) - 2.0 * TAU).abs() < 1e-12);
    }

    #[test]
    fn a_ring_and_a_disc_over_its_hole_leave_no_hole() {
        let mut ring = disc(0.0, 0.0, 3.0);
        ring.holes.push(Loop {
            curves: vec![Curve2::circle(P2::new(0.0, 0.0), 1.0)],
        });
        let (u, _) = union(&[ring.clone(), disc(0.0, 0.0, 1.0)], None).unwrap();
        assert!(u.len() == 1 && u[0].holes.is_empty());
        // A smaller disc leaves a ring between them, and stands apart within it.
        let (u, adds) = union(&[ring, disc(0.0, 0.0, 0.5)], Some(1)).unwrap();
        assert!(adds && u.len() == 2);
    }
}

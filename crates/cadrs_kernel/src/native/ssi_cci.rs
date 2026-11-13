//! Curve–curve intersection by subdivision, for the pairs [`super::ssi::curve_curve`] has no
//! closed form for (and the near misses and overlaps the closed forms leave out): pieces of the
//! two curves are halved while their boxes come within the tolerance of each other; small pairs
//! are solved for their nearest points (within the curves' ranges, so an end that stops just
//! short counts), and pairs where each piece lies on the other curve are overlaps, reported by
//! where the overlap begins and ends.

use std::collections::HashMap;

use super::curve::Curve;
use super::math::*;
use super::tol;

/// A piece of a curve: its range, a box sure to hold it, its size, and a capsule sure to hold
/// it (the chord between its ends, `a` to `b`, grown by `r`).
#[derive(Clone, Copy)]
struct Piece {
    t0: f64,
    t1: f64,
    bb: Bbox,
    size: f64,
    a: P3,
    b: P3,
    r: f64,
}

impl Piece {
    /// Whether the two pieces may come within `tol` of each other: their boxes do, and their
    /// capsules. (Boxes alone keep two pieces running side by side aslant together, however
    /// far apart they are for their size: a curve that touches another and leaves it slowly
    /// was halved down to its smallest pieces all along where it left.)
    fn near(&self, o: &Piece, tol: f64) -> bool {
        self.bb.grow(tol).overlaps(&o.bb)
            && segment_distance(&self.a, &self.b, &o.a, &o.b) <= self.r + o.r + tol
    }
}

/// The piece of `c` over [t0, t1]: five samples' box grown by twice the chord's error bound
/// (h²/8 · max |C''| over the samples), and their chord grown by as much again past the sample
/// farthest from it.
fn piece(c: &Curve, t0: f64, t1: f64) -> Piece {
    let mut bb = Bbox::empty();
    let mut m2 = 0.0f64;
    const N: usize = 4;
    let mut ps = [P3::origin(); N + 1];
    for (i, q) in ps.iter_mut().enumerate() {
        let (p, _, d2) = c.d2(t0 + (t1 - t0) * i as f64 / N as f64);
        bb.add(&p);
        m2 = m2.max(d2.norm());
        *q = p;
    }
    let h = (t1 - t0) / N as f64;
    let grow = 2.0 * h * h * m2 / 8.0 + 1e-15 * (1.0 + bb.max.coords.abs().max());
    let bb = bb.grow(grow);
    let (a, b) = (ps[0], ps[N]);
    let off = ps[1..N]
        .iter()
        .map(|p| point_segment_distance(p, &a, &b))
        .fold(0.0, f64::max);
    Piece {
        t0,
        t1,
        bb,
        size: bb.diagonal(),
        a,
        b,
        // (And a little more, for the distances' rounding.)
        r: off + grow + 1e-12 * (off + grow),
    }
}

/// The distance from `p` to the segment from `a` to `b`.
fn point_segment_distance(p: &P3, a: &P3, b: &P3) -> f64 {
    let d = b - a;
    let l2 = d.norm_squared();
    let t = if l2 > 0.0 {
        ((p - a).dot(&d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + d * t)).norm()
}

/// The distance between the segments from `p0` to `p1` and from `q0` to `q1` (Ericson, Real-Time
/// Collision Detection, 5.1.9).
fn segment_distance(p0: &P3, p1: &P3, q0: &P3, q1: &P3) -> f64 {
    let (d1, d2, r) = (p1 - p0, q1 - q0, p0 - q0);
    let (a, e, f) = (d1.norm_squared(), d2.norm_squared(), d2.dot(&r));
    let (s, t) = if a <= f64::MIN_POSITIVE && e <= f64::MIN_POSITIVE {
        (0.0, 0.0)
    } else if a <= f64::MIN_POSITIVE {
        (0.0, (f / e).clamp(0.0, 1.0))
    } else {
        let c = d1.dot(&r);
        if e <= f64::MIN_POSITIVE {
            ((-c / a).clamp(0.0, 1.0), 0.0)
        } else {
            let b = d1.dot(&d2);
            let den = a * e - b * b;
            let mut s = if den > 0.0 {
                ((b * f - c * e) / den).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let mut t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
            (s, t)
        }
    };
    ((p0 + d1 * s) - (q0 + d2 * t)).norm()
}

fn halves(c: &Curve, p: &Piece) -> [Piece; 2] {
    let m = 0.5 * (p.t0 + p.t1);
    [piece(c, p.t0, m), piece(c, m, p.t1)]
}

/// The nearest points of `a` on [a0, a1] and `b` on [b0, b1] from (s, t): Newton on the squared
/// distance, clamped to the ranges (an end of one curve, then the nearest point of the other).
/// (`sa`, `sb`: the curves' [`Curve::closest_samples`] over those ranges, taken once.)
#[allow(clippy::too_many_arguments)]
fn nearest(
    a: &Curve,
    (a0, a1): (f64, f64),
    b: &Curve,
    (b0, b1): (f64, f64),
    mut s: f64,
    mut t: f64,
    sa: &[P3],
    sb: &[P3],
) -> (f64, f64) {
    for _ in 0..60 {
        let (p, pa, paa) = a.d2(s);
        let (q, qb, qbb) = b.d2(t);
        let r = p - q;
        let (fs, ft) = (r.dot(&pa), -r.dot(&qb));
        let (j11, j12, j22) = (
            pa.dot(&pa) + r.dot(&paa),
            -pa.dot(&qb),
            qb.dot(&qb) - r.dot(&qbb),
        );
        let Some((ds, dt)) = solve2(j11, j12, j12, j22, -fs, -ft) else {
            break;
        };
        let (ns, nt) = ((s + ds).clamp(a0, a1), (t + dt).clamp(b0, b1));
        // Clamped on one curve's end: the other's nearest point to it.
        let (ns, nt) = if ns != s + ds && nt == t + dt {
            (ns, b.closest_from(&a.point(ns), b0, b1, sb))
        } else if nt != t + dt && ns == s + ds {
            (a.closest_from(&b.point(nt), a0, a1, sa), nt)
        } else {
            (ns, nt)
        };
        let step = (ns - s).abs() + (nt - t).abs();
        s = ns;
        t = nt;
        if step < 1e-15 * (1.0 + s.abs() + t.abs()) {
            break;
        }
    }
    (s, t)
}

/// Whether every sample of the piece of `c` lies within `tol` of `o` over its range. `memo`
/// keeps each sample's answer by its parameter: a piece is tested against every piece of the
/// other curve its box meets, and halving a piece reuses three of its five samples, so most
/// samples recur (and each costs a closest-point search over the whole of `o`).
/// (`samples`: `o`'s [`Curve::closest_samples`] over its range, taken once.)
#[allow(clippy::too_many_arguments)]
fn lies_on(
    c: &Curve,
    p: &Piece,
    o: &Curve,
    (o0, o1): (f64, f64),
    tol: f64,
    memo: &mut HashMap<u64, bool>,
    samples: &[P3],
) -> bool {
    (0..=4).all(|i| {
        let t = p.t0 + (p.t1 - p.t0) * i as f64 / 4.0;
        *memo.entry(t.to_bits()).or_insert_with(|| {
            let q = c.point(t);
            (o.point(o.closest_from(&q, o0, o1, samples)) - q).norm() <= tol
        })
    })
}

/// Where `a` on [a0, a1] and `b` on [b0, b1] come within `tol`: (t on a, t on b) for each
/// crossing or near miss, and the two ends of each overlap.
pub fn curve_curve(
    a: &Curve,
    a0: f64,
    a1: f64,
    b: &Curve,
    b0: f64,
    b1: f64,
    tol: f64,
) -> Vec<(f64, f64)> {
    let (ra, rb) = ((a0.min(a1), a0.max(a1)), (b0.min(b1), b0.max(b1)));
    let pa: Vec<Piece> = a
        .pieces(ra.0, ra.1)
        .windows(2)
        .map(|w| piece(a, w[0], w[1]))
        .collect();
    let pb: Vec<Piece> = b
        .pieces(rb.0, rb.1)
        .windows(2)
        .map(|w| piece(b, w[0], w[1]))
        .collect();
    let mut size = 0.0f64;
    for p in pa.iter().chain(&pb) {
        size = size.max(p.size);
    }
    let small = (tol::REL_POINT * size).max(tol * 0.1);
    // (Only the pairs whose boxes come within the tolerance, in the order they were all pushed
    // before: two fitted curves of 450 pieces each made 200 000 pairs, nearly all apart.)
    let mut stack: Vec<(Piece, Piece)> = Vec::new();
    for x in &pa {
        for y in &pb {
            if x.near(y, tol) {
                stack.push((*x, *y));
            }
        }
    }
    let mut out: Vec<(f64, f64)> = Vec::new();
    let mut overlap = false;
    let mut budget = 200_000usize;
    let (mut memo_a, mut memo_b) = (HashMap::new(), HashMap::new());
    let (samples_a, samples_b) = (a.closest_samples(ra.0, ra.1), b.closest_samples(rb.0, rb.1));
    while let Some((x, y)) = stack.pop() {
        if !x.near(&y, tol) {
            continue;
        }
        budget = budget.saturating_sub(1);
        // An overlap: each piece on the other curve.
        if lies_on(a, &x, b, rb, tol, &mut memo_a, &samples_b)
            && lies_on(b, &y, a, ra, tol, &mut memo_b, &samples_a)
        {
            overlap = true;
            continue;
        }
        if (x.size <= small && y.size <= small) || budget == 0 {
            let (s, t) = nearest(
                a,
                ra,
                b,
                rb,
                0.5 * (x.t0 + x.t1),
                0.5 * (y.t0 + y.t1),
                &samples_a,
                &samples_b,
            );
            // (A gap of exactly the tolerance counts: the rounding of its computation aside.)
            if (a.point(s) - b.point(t)).norm() <= tol * (1.0 + tol::REL) {
                out.push((s, t));
            }
            continue;
        }
        if x.size >= y.size {
            let [x1, x2] = halves(a, &x);
            stack.push((x2, y));
            stack.push((x1, y));
        } else {
            let [y1, y2] = halves(b, &y);
            stack.push((x, y2));
            stack.push((x, y1));
        }
    }
    if overlap {
        // The overlaps' ends: each curve's ends lying on the other.
        for ta in [ra.0, ra.1] {
            let p = a.point(ta);
            let tb = b.closest_from(&p, rb.0, rb.1, &samples_b);
            if (b.point(tb) - p).norm() <= tol {
                out.push((ta, tb));
            }
        }
        for tb in [rb.0, rb.1] {
            let q = b.point(tb);
            let ta = a.closest_from(&q, ra.0, ra.1, &samples_a);
            if (a.point(ta) - q).norm() <= tol {
                out.push((ta, tb));
            }
        }
    }
    // One point per crossing (many small pairs solve to it).
    // (Each kept point's position with it: a near-overlap leaves tens of thousands to sift.)
    let mut uniq: Vec<(f64, f64, P3)> = Vec::new();
    for (s, t) in out {
        let p = a.point(s);
        if !uniq.iter().any(|(_, _, q)| (q - p).norm() <= tol) {
            uniq.push((s, t, p));
        }
    }
    uniq.sort_by(|x, y| x.0.total_cmp(&y.0));
    uniq.into_iter().map(|(s, t, _)| (s, t)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The segments' distance is the least of their points' distances, found by sampling both
    /// densely: crossing, skew, parallel, end to end, and one a point.
    #[test]
    fn segment_distance_matches_sampling() {
        let cases = [
            (
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 0.0, 0.0),
                P3::new(0.5, -1.0, 0.3),
                P3::new(0.5, 1.0, 0.3),
            ),
            (
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 1.0, 0.0),
                P3::new(2.0, 0.0, 1.0),
                P3::new(3.0, 2.0, -1.0),
            ),
            (
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 0.0, 0.0),
                P3::new(0.2, 0.1, 0.0),
                P3::new(1.7, 0.1, 0.0),
            ),
            (
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 0.0, 0.0),
                P3::new(1.5, 0.5, 0.0),
                P3::new(3.0, 0.5, 0.0),
            ),
            (
                P3::new(0.0, 0.0, 0.0),
                P3::new(1.0, 2.0, 3.0),
                P3::new(0.4, 0.4, 0.4),
                P3::new(0.4, 0.4, 0.4),
            ),
        ];
        for (p0, p1, q0, q1) in cases {
            let n = 400;
            let mut best = f64::INFINITY;
            for i in 0..=n {
                let p = p0 + (p1 - p0) * (i as f64 / n as f64);
                for j in 0..=n {
                    let q = q0 + (q1 - q0) * (j as f64 / n as f64);
                    best = best.min((p - q).norm());
                }
            }
            let d = segment_distance(&p0, &p1, &q0, &q1);
            assert!(d <= best + 1e-12 && d >= best - 1e-2, "{d} {best}");
            assert!((segment_distance(&q0, &q1, &p0, &p1) - d).abs() < 1e-12);
        }
    }

    /// A circle touching a line it is tangent to meets it once, at the touch.
    #[test]
    fn tangent_touch_found_once() {
        let c = Curve::Circle {
            c: P3::new(0.0, 1.0, 0.0),
            x: V3::new(0.6, -0.8, 0.0),
            y: V3::new(0.8, 0.6, 0.0),
            r: 1.0,
        };
        let l = Curve::Line {
            o: P3::new(-3.0, 0.0, 0.0),
            d: V3::new(1.0, 0.0, 0.0),
        };
        let hits = curve_curve(&c, 0.0, 6.0, &l, 0.0, 6.0, 5e-7);
        assert_eq!(hits.len(), 1, "{hits:?}");
        let (s, t) = hits[0];
        assert!((c.point(s) - P3::new(0.0, 0.0, 0.0)).norm() < 1e-3, "{s}");
        assert!((t - 3.0).abs() < 1e-3, "{t}");
    }
}

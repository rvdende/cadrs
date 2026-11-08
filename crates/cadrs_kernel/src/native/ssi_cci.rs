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

/// A piece of a curve: its range, a box sure to hold it, its size.
#[derive(Clone, Copy)]
struct Piece {
    t0: f64,
    t1: f64,
    bb: Bbox,
    size: f64,
}

/// The piece of `c` over [t0, t1]: five samples' box grown by twice the chord's error bound
/// (h²/8 · max |C''| over the samples).
fn piece(c: &Curve, t0: f64, t1: f64) -> Piece {
    let mut bb = Bbox::empty();
    let mut m2 = 0.0f64;
    const N: usize = 4;
    for i in 0..=N {
        let (p, _, d2) = c.d2(t0 + (t1 - t0) * i as f64 / N as f64);
        bb.add(&p);
        m2 = m2.max(d2.norm());
    }
    let h = (t1 - t0) / N as f64;
    let bb = bb.grow(2.0 * h * h * m2 / 8.0 + 1e-15 * (1.0 + bb.max.coords.abs().max()));
    Piece {
        t0,
        t1,
        bb,
        size: bb.diagonal(),
    }
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
        let xb = x.bb.grow(tol);
        for y in &pb {
            if xb.overlaps(&y.bb) {
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
        if !x.bb.grow(tol).overlaps(&y.bb) {
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

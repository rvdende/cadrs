//! Plane regions bounded by sketch curves ([`Curve2`]): a curve's piece and its reverse, and a
//! disc less a closed loop of curves (a rod's section less a groove's, both screwed up the rod).

use crate::Curve2;
use nalgebra::Point2;
use std::f64::consts::TAU;

type P2 = Point2<f64>;

/// The kernel's B-spline of a sketch spline, and back.
fn spline_of(c: &Curve2) -> Option<super::nurbs::NurbsCurve> {
    let Curve2::Spline {
        degree,
        knots,
        poles,
        weights,
        ..
    } = c
    else {
        return None;
    };
    crate::spline_nurbs(*degree, knots, poles, weights)
}

fn sketch_spline(n: &super::nurbs::NurbsCurve, source: Option<u64>) -> Curve2 {
    let rational = n.cps.iter().any(|h| h.w != 1.0);
    Curve2::Spline {
        degree: n.degree,
        knots: n.knots.clone(),
        poles: n
            .cps
            .iter()
            .map(|h| P2::new(h.x / h.w, h.y / h.w))
            .collect(),
        weights: if rational {
            n.cps.iter().map(|h| h.w).collect()
        } else {
            Vec::new()
        },
        source,
    }
}

fn source_of(c: &Curve2) -> Option<u64> {
    c.source()
}

/// The piece of `c` from `s0` to `s1` (its 0..1 places, `s0 < s1`), the same kind of curve.
pub(crate) fn part(c: &Curve2, s0: f64, s1: f64) -> Option<Curve2> {
    if let Curve2::Spline { .. } = c {
        let n = spline_of(c)?;
        let (a, b) = n.range();
        return Some(sketch_spline(
            &n.restricted(a + (b - a) * s0, a + (b - a) * s1),
            source_of(c),
        ));
    }
    super::loft_build::curve_part(c, s0, s1)
}

/// `c` run the other way.
pub(crate) fn reversed(c: &Curve2) -> Option<Curve2> {
    if let Curve2::Spline { .. } = c {
        return Some(sketch_spline(&spline_of(c)?.reversed(), source_of(c)));
    }
    super::loft_build::reversed2(c)
}

/// A closed loop's polygon, `per` points a curve.
fn polygon(lp: &[Curve2], per: usize) -> Vec<P2> {
    lp.iter()
        .flat_map(|c| (0..per).map(move |i| c.point_at(i as f64 / per as f64)))
        .collect()
}

fn signed_area(poly: &[P2]) -> f64 {
    (0..poly.len())
        .map(|i| {
            let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
            a.x * b.y - b.x * a.y
        })
        .sum::<f64>()
        / 2.0
}

fn inside(poly: &[P2], q: &P2) -> bool {
    super::math::point_in_polygon(q, poly)
}

/// The disc about `c` of radius `r` less the region `lp` bounds (a closed loop of curves, end to
/// start, either way round): its boundary's loops, each counter-clockwise for what is kept round
/// it and clockwise round a hole, the outer ones first. None where a crossing can't be settled
/// (a curve along the circle, a kind of curve that can't be cut) or nothing is left.
pub(crate) fn disc_less(c: P2, r: f64, lp: &[Curve2]) -> Option<Vec<Vec<Curve2>>> {
    const PER: usize = 64;
    let poly = polygon(lp, PER);
    if poly.len() < 3 {
        return None;
    }
    // The loop counter-clockwise.
    let lp: Vec<Curve2> = if signed_area(&poly) < 0.0 {
        lp.iter().rev().map(reversed).collect::<Option<_>>()?
    } else {
        lp.to_vec()
    };
    let poly = polygon(&lp, PER);
    let f = |q: P2| (q - c).norm() - r;
    let eps = 1e-12 * r;
    // Each curve's crossings of the circle (its places, in order).
    let mut cuts: Vec<Vec<f64>> = Vec::with_capacity(lp.len());
    for cv in &lp {
        let mut at: Vec<f64> = Vec::new();
        let mut prev = (0.0, f(cv.point_at(0.0)));
        for i in 1..=PER {
            let s = i as f64 / PER as f64;
            let cur = (s, f(cv.point_at(s)));
            if (prev.1 < 0.0) != (cur.1 < 0.0) && prev.1.abs() > eps && cur.1.abs() > eps {
                let (mut a, mut b, fa) = (prev.0, cur.0, prev.1);
                for _ in 0..80 {
                    let m = 0.5 * (a + b);
                    if (f(cv.point_at(m)) < 0.0) == (fa < 0.0) {
                        a = m;
                    } else {
                        b = m;
                    }
                }
                at.push(0.5 * (a + b));
            } else if cur.1.abs() <= eps && i < PER {
                // A sample on the circle: no crossing to place; refuse rather than guess.
                return None;
            }
            prev = cur;
        }
        cuts.push(at);
    }
    let crossings: usize = cuts.iter().map(Vec::len).sum();
    let full_circle = || Curve2::Circle {
        center: c,
        radius: r,
        source: None,
    };
    if crossings == 0 {
        let loop_in_disc = poly.iter().all(|q| f(*q) < 0.0);
        let disc_in_loop = inside(&poly, &c) && poly.iter().all(|q| f(*q) > 0.0);
        return if disc_in_loop {
            None
        } else if loop_in_disc {
            let hole: Vec<Curve2> = lp.iter().rev().map(reversed).collect::<Option<_>>()?;
            Some(vec![vec![full_circle()], hole])
        } else {
            Some(vec![vec![full_circle()]])
        };
    }
    // The loop's pieces inside the disc, each run backward (the kept region lies to its left
    // then), and the circle's arcs outside the loop, counter-clockwise.
    let mut pieces: Vec<Curve2> = Vec::new();
    let mut angles: Vec<f64> = Vec::new();
    for (cv, at) in lp.iter().zip(&cuts) {
        let mut ss = vec![0.0];
        ss.extend(at.iter().copied());
        ss.push(1.0);
        for w in ss.windows(2) {
            if w[1] - w[0] <= 1e-12 {
                continue;
            }
            if f(cv.point_at(0.5 * (w[0] + w[1]))) < 0.0 {
                pieces.push(reversed(&part(cv, w[0], w[1])?)?);
            }
        }
        for &s in at {
            let q = cv.point_at(s) - c;
            angles.push(q.y.atan2(q.x).rem_euclid(TAU));
        }
    }
    angles.sort_by(f64::total_cmp);
    for i in 0..angles.len() {
        let a0 = angles[i];
        let a1 = if i + 1 < angles.len() {
            angles[i + 1]
        } else {
            angles[0] + TAU
        };
        if a1 - a0 <= 1e-12 {
            continue;
        }
        let m = 0.5 * (a0 + a1);
        if !inside(
            &poly,
            &(c + nalgebra::Vector2::new(r * m.cos(), r * m.sin())),
        ) {
            pieces.push(Curve2::Arc {
                center: c,
                radius: r,
                start_angle: a0,
                sweep: a1 - a0,
                source: None,
            });
        }
    }
    // Chained end to start into loops.
    let near = 1e-7 * r;
    let mut used = vec![false; pieces.len()];
    let mut loops: Vec<Vec<Curve2>> = Vec::new();
    for start in 0..pieces.len() {
        if used[start] {
            continue;
        }
        used[start] = true;
        let mut lp2 = vec![pieces[start].clone()];
        loop {
            let end = lp2.last()?.end();
            if (end - lp2[0].start()).norm() <= near && lp2.len() > 1 {
                break;
            }
            let next = (0..pieces.len()).filter(|&j| !used[j]).min_by(|&a, &b| {
                (pieces[a].start() - end)
                    .norm()
                    .total_cmp(&(pieces[b].start() - end).norm())
            })?;
            if (pieces[next].start() - end).norm() > near {
                return None;
            }
            used[next] = true;
            lp2.push(pieces[next].clone());
        }
        loops.push(lp2);
    }
    // Outer loops (counter-clockwise) first.
    loops.sort_by(|a, b| {
        let (sa, sb) = (signed_area(&polygon(a, 16)), signed_area(&polygon(b, 16)));
        sb.total_cmp(&sa)
    });
    if loops.is_empty() || signed_area(&polygon(&loops[0], 16)) <= 0.0 {
        return None;
    }
    Some(loops)
}

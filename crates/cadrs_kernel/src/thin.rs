//! Thin walls (PS4.11) as a profile of exact 2D bands.
//!
//! A thin extrude gives each curve of the profile a wall `left` mm to its left and `right` mm to
//! its right (seen from the plane normal). The loops of a region run with the material on their
//! left (outer loops counter-clockwise, holes clockwise), so `left` is the inside. The wall is
//! built in the plane:
//!
//! - every curve gets a **band** between its offsets (a line gives a rectangle, an arc an
//!   annular sector, a circle an annulus), closed across its ends;
//! - where two curves meet at an angle, the outer side of the corner gets a **fill** up to where
//!   the offset lines meet (a sharp, mitred corner); on the inner side the bands overlap.
//!
//! The bands and fills are ordinary regions, so the thin wall is extruded like any profile (every
//! end type works) and the kernel fuses them. The sides of a band are named after the curve they
//! offset: its sketch curve id, with [`THIN_LEFT`] or [`THIN_RIGHT`] set for an offset side.
//! Ellipses are not offset (their offsets are not ellipses).

use nalgebra::{Point2, Vector2};

use crate::{Chain, Curve2, KernelError, Loop, Profile, Region, Result};

/// Set in the curve id of a thin wall's left-hand offset side.
pub const THIN_LEFT: u64 = 1 << 61;
/// Set in the curve id of a thin wall's right-hand offset side.
pub const THIN_RIGHT: u64 = 1 << 62;

const EPS: f64 = 1e-9;

/// The regions of a thin wall along every curve of `profile` (its regions' loops and its open
/// chains). Each band and fill keeps the source of the region or chain it came from.
pub fn thin_profile(profile: &Profile, left: f64, right: f64) -> Result<Profile> {
    if !(left >= 0.0 && right >= 0.0 && left + right > 1e-6) {
        return Err(KernelError::InvalidParameter(
            "the wall thickness must be greater than zero".into(),
        ));
    }
    let mut regions = Vec::new();
    for (i, r) in profile.regions.iter().enumerate() {
        let source = r.source.unwrap_or(i as u64);
        wall(&oriented(&r.outer.curves, true), true, left, right, source, &mut regions)?;
        for h in &r.holes {
            wall(&oriented(&h.curves, false), true, left, right, source, &mut regions)?;
        }
    }
    for (i, c) in profile.chains.iter().enumerate() {
        let source = c.source.unwrap_or((profile.regions.len() + i) as u64);
        wall(&c.curves, false, left, right, source, &mut regions)?;
    }
    if regions.is_empty() {
        return Err(KernelError::InvalidProfile("nothing to extrude".into()));
    }
    Ok(Profile {
        plane: profile.plane,
        regions,
        chains: Vec::<Chain>::new(),
    })
}

/// The bands and corner fills of one loop (`closed`) or chain.
fn wall(curves: &[Curve2], closed: bool, left: f64, right: f64, source: u64, out: &mut Vec<Region>) -> Result<()> {
    for c in curves {
        out.push(band(c, curves.len() == 1 && closed, left, right, source)?);
    }
    let n = curves.len();
    // A whole circle has no corner.
    let joints = match (closed, n) {
        (true, 1) => 0,
        (true, _) => n,
        (false, _) => n.saturating_sub(1),
    };
    for i in 0..joints {
        let (a, b) = (&curves[i], &curves[(i + 1) % n]);
        if let Some(f) = fill(b.start(), (a, end_tangent(a)), (b, start_tangent(b)), left, right, source) {
            out.push(f);
        }
    }
    Ok(())
}

fn perp(v: Vector2<f64>) -> Vector2<f64> {
    Vector2::new(-v.y, v.x)
}

fn id(c: &Curve2) -> u64 {
    c.source().unwrap_or(0)
}

fn line(a: Point2<f64>, b: Point2<f64>, source: Option<u64>) -> Curve2 {
    Curve2::Line { a, b, source }
}

/// The side of a band: the curve itself (no offset) or its offset, named with `flag`.
fn side_source(c: &Curve2, offset: f64, flag: u64) -> Option<u64> {
    Some(if offset > 0.0 { id(c) ^ flag } else { id(c) })
}

fn band(c: &Curve2, whole: bool, left: f64, right: f64, source: u64) -> Result<Region> {
    let region = |curves: Vec<Curve2>, holes: Vec<Loop>| Region {
        outer: Loop { curves },
        holes,
        source: Some(source),
    };
    match *c {
        Curve2::Line { a, b, .. } => {
            let d = b - a;
            if d.norm() < EPS {
                return Err(KernelError::InvalidProfile("zero-length line".into()));
            }
            let nl = perp(d.normalize());
            let (la, lb) = (a + nl * left, b + nl * left);
            let (ra, rb) = (a - nl * right, b - nl * right);
            Ok(region(
                vec![
                    line(ra, rb, side_source(c, right, THIN_RIGHT)),
                    line(rb, lb, None),
                    line(lb, la, side_source(c, left, THIN_LEFT)),
                    line(la, ra, None),
                ],
                vec![],
            ))
        }
        Curve2::Arc { center, radius, start_angle, sweep, .. } if !whole && sweep.abs() < std::f64::consts::TAU - 1e-9 => {
            // Left of a counter-clockwise arc is toward its centre.
            let ccw = sweep > 0.0;
            let rl = if ccw { radius - left } else { radius + left };
            let rr = if ccw { radius + right } else { radius - right };
            if rl < EPS || rr < EPS {
                return Err(KernelError::InvalidParameter(
                    "the wall is thicker than an arc's radius".into(),
                ));
            }
            let arc = |r: f64, src: Option<u64>| Curve2::Arc {
                center,
                radius: r,
                start_angle,
                sweep,
                source: src,
            };
            let right_arc = arc(rr, side_source(c, right, THIN_RIGHT));
            let left_arc = arc(rl, side_source(c, left, THIN_LEFT));
            Ok(region(
                vec![
                    right_arc.clone(),
                    line(right_arc.end(), left_arc.end(), None),
                    reverse(&left_arc),
                    line(left_arc.start(), right_arc.start(), None),
                ],
                vec![],
            ))
        }
        Curve2::Arc { center, radius, .. } | Curve2::Circle { center, radius, .. } => {
            // A whole circle, counter-clockwise (an outer loop) unless a hole's loop reversed it.
            let ccw = match *c {
                Curve2::Arc { sweep, .. } => sweep > 0.0,
                _ => true,
            };
            let r = radius.abs();
            let (inner, outer) = if ccw { (r - left, r + right) } else { (r - right, r + left) };
            if inner < EPS {
                return Err(KernelError::InvalidParameter(
                    "the wall is thicker than a circle's radius".into(),
                ));
            }
            let circle = |r: f64, src: Option<u64>| Curve2::Circle { center, radius: r, source: src };
            let (inner_src, outer_src) = if ccw {
                (side_source(c, left, THIN_LEFT), side_source(c, right, THIN_RIGHT))
            } else {
                (side_source(c, right, THIN_RIGHT), side_source(c, left, THIN_LEFT))
            };
            Ok(region(
                vec![circle(outer, outer_src)],
                vec![Loop { curves: vec![circle(inner, inner_src)] }],
            ))
        }
        Curve2::Ellipse { .. } | Curve2::EllipseArc { .. } | Curve2::OffsetEllipseArc { .. } => {
            Err(KernelError::Unsupported("thin walls along ellipses"))
        }
        Curve2::Bezier { ref poles, .. } => {
            // A spline span's offsets aren't Béziers: each side is four cubic pieces fitted to
            // the offset curve at their ends (points and derivatives, scaled by 1 − d·κ).
            let right_side = offset_bezier(poles, -right, side_source(c, right, THIN_RIGHT))?;
            let left_side = offset_bezier(poles, left, side_source(c, left, THIN_LEFT))?;
            let (rs, re) = (right_side[0].start(), right_side[right_side.len() - 1].end());
            let (ls, le) = (left_side[0].start(), left_side[left_side.len() - 1].end());
            let mut curves = right_side;
            curves.push(line(re, le, None));
            curves.extend(left_side.iter().rev().map(reverse));
            curves.push(line(ls, rs, None));
            Ok(region(curves, vec![]))
        }
    }
}

/// The curve `d` mm to the left of the cubic Bézier `poles` (right when negative), as cubic
/// pieces.
fn offset_bezier(poles: &[Point2<f64>], d: f64, source: Option<u64>) -> Result<Vec<Curve2>> {
    const PIECES: usize = 4;
    let b = [poles[0], poles[1], poles[2], poles[3]];
    let at = |t: f64| crate::types::bezier_point(&b, t);
    let deriv = |t: f64| {
        let u = 1.0 - t;
        (b[1] - b[0]) * (3.0 * u * u) + (b[2] - b[1]) * (6.0 * u * t) + (b[3] - b[2]) * (3.0 * t * t)
    };
    let second = |t: f64| {
        let u = 1.0 - t;
        (b[2] - b[1] * 2.0 + b[0].coords) * (6.0 * u) + (b[3] - b[2] * 2.0 + b[1].coords) * (6.0 * t)
    };
    // The offset point and its derivative (in the piece's own parameter) at `t`.
    let off = |t: f64, scale: f64| -> Result<(Point2<f64>, Vector2<f64>)> {
        let d1 = deriv(t);
        let l = d1.norm();
        if l < EPS {
            return Err(KernelError::InvalidProfile("a spline with a cusp can't have a thin wall".into()));
        }
        let k = (d1.x * second(t).y - d1.y * second(t).x) / (l * l * l);
        let f = 1.0 - k * d;
        if f <= EPS {
            return Err(KernelError::InvalidParameter("the wall is thicker than a spline's radius of curvature".into()));
        }
        Ok((at(t) + perp(d1 / l) * d, d1 * (f * scale)))
    };
    let h = 1.0 / PIECES as f64;
    (0..PIECES)
        .map(|i| {
            let (t0, t1) = (i as f64 * h, (i + 1) as f64 * h);
            let (p0, d0) = off(t0, h)?;
            let (p3, d3) = off(t1, h)?;
            Ok(Curve2::Bezier { poles: [p0, p0 + d0 / 3.0, p3 - d3 / 3.0, p3], source })
        })
        .collect()
}

/// The corner fill where a curve `ca` ending with tangent `ta` meets one `cb` starting with `tb`
/// at `p`: on the outer side of the turn, up to where the offset lines meet. Its outer sides
/// continue the two curves' offset sides (and are named after them, so no seam shows).
fn fill(
    p: Point2<f64>,
    (ca, ta): (&Curve2, Vector2<f64>),
    (cb, tb): (&Curve2, Vector2<f64>),
    left: f64,
    right: f64,
    source: u64,
) -> Option<Region> {
    let cross = ta.x * tb.y - ta.y * tb.x;
    if cross.abs() < 1e-9 {
        return None;
    }
    // Turning left, the outside of the corner is on the right.
    let (t, na, nb, flag) = if cross > 0.0 {
        (right, -perp(ta), -perp(tb), THIN_RIGHT)
    } else {
        (left, perp(ta), perp(tb), THIN_LEFT)
    };
    if t <= 0.0 {
        return None;
    }
    let (a, b) = (p + na * t, p + nb * t);
    // a + ta·s = b + tb·u.
    let s = ((b - a).x * tb.y - (b - a).y * tb.x) / cross;
    let m = a + ta * s;
    Some(Region {
        outer: Loop {
            curves: vec![
                line(p, a, None),
                line(a, m, side_source(ca, t, flag)),
                line(m, b, side_source(cb, t, flag)),
                line(b, p, None),
            ],
        },
        holes: vec![],
        source: Some(source),
    })
}

fn start_tangent(c: &Curve2) -> Vector2<f64> {
    tangent(c, 0.0)
}

fn end_tangent(c: &Curve2) -> Vector2<f64> {
    tangent(c, 1.0)
}

fn tangent(c: &Curve2, s: f64) -> Vector2<f64> {
    match *c {
        Curve2::Line { a, b, .. } => (b - a).normalize(),
        Curve2::Arc { start_angle, sweep, .. } => {
            let t = start_angle + sweep * s;
            Vector2::new(-t.sin(), t.cos()) * sweep.signum()
        }
        Curve2::Bezier { ref poles, .. } => {
            // The exact end tangents (the joints of a spline's spans line up to rounding, so
            // they get no corner fill).
            let u = 1.0 - s;
            let v = (poles[1] - poles[0]) * (u * u) + (poles[2] - poles[1]) * (2.0 * u * s) + (poles[3] - poles[2]) * (s * s);
            if v.norm() > 1e-12 { v.normalize() } else { (poles[3] - poles[0]).normalize() }
        }
        _ => {
            let (a, b) = (c.point_at((s - 1e-6).max(0.0)), c.point_at((s + 1e-6).min(1.0)));
            (b - a).normalize()
        }
    }
}

/// The curve run backwards.
fn reverse(c: &Curve2) -> Curve2 {
    match c.clone() {
        Curve2::Line { a, b, source } => Curve2::Line { a: b, b: a, source },
        Curve2::Arc { center, radius, start_angle, sweep, source } => Curve2::Arc {
            center,
            radius,
            start_angle: start_angle + sweep,
            sweep: -sweep,
            source,
        },
        Curve2::EllipseArc { center, major_radius, minor_radius, rotation, start, sweep, source } => {
            Curve2::EllipseArc {
                center,
                major_radius,
                minor_radius,
                rotation,
                start: start + sweep,
                sweep: -sweep,
                source,
            }
        }
        // Whole circles and ellipses: marked clockwise by a negative sweep, as an arc.
        Curve2::Circle { center, radius, source } => Curve2::Arc {
            center,
            radius,
            start_angle: 0.0,
            sweep: -std::f64::consts::TAU,
            source,
        },
        Curve2::Bezier { poles: [a, b, c, d], source } => Curve2::Bezier { poles: [d, c, b, a], source },
        other => other,
    }
}

/// The loop's curves running counter-clockwise (`ccw`) or clockwise (by the sign of its
/// sampled area).
fn oriented(curves: &[Curve2], ccw: bool) -> Vec<Curve2> {
    let pts: Vec<Point2<f64>> = curves
        .iter()
        .flat_map(|c| (0..16).map(move |k| c.point_at(k as f64 / 16.0)))
        .collect();
    let n = pts.len();
    let twice: f64 = (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a.x * b.y - b.x * a.y
        })
        .sum();
    if (twice > 0.0) == ccw {
        curves.to_vec()
    } else {
        curves.iter().rev().map(reverse).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Plane;

    fn area(r: &Region) -> f64 {
        let ring = |l: &Loop| -> f64 {
            let pts: Vec<Point2<f64>> = l
                .curves
                .iter()
                .flat_map(|c| (0..256).map(move |k| c.point_at(k as f64 / 256.0)))
                .collect();
            let n = pts.len();
            ((0..n)
                .map(|i| pts[i].x * pts[(i + 1) % n].y - pts[(i + 1) % n].x * pts[i].y)
                .sum::<f64>()
                / 2.0)
                .abs()
        };
        ring(&r.outer) - r.holes.iter().map(ring).sum::<f64>()
    }

    #[test]
    fn a_square_wall_is_bands_and_fills() {
        let p = Point2::new;
        let sq = [p(0.0, 0.0), p(20.0, 0.0), p(20.0, 20.0), p(0.0, 20.0)];
        let curves = (0..4).map(|i| line(sq[i], sq[(i + 1) % 4], Some(i as u64 + 1))).collect();
        let profile = Profile::new(
            Plane::top(),
            vec![Region { outer: Loop { curves }, holes: vec![], source: Some(9) }],
        );
        // 1 mm outside: four 20 × 1 bands and four 1 × 1 corner fills (the outside of every
        // left turn): 84 = 22² − 20².
        let t = thin_profile(&profile, 0.0, 1.0).unwrap();
        assert_eq!(t.regions.len(), 8);
        let total: f64 = t.regions.iter().map(area).sum();
        assert!((total - 84.0).abs() < 1e-9, "{total}");
        assert!(t.regions.iter().all(|r| r.source == Some(9)));
        // Inside, the bands overlap at the corners and nothing is filled.
        let t = thin_profile(&profile, 2.0, 0.0).unwrap();
        assert_eq!(t.regions.len(), 4);
    }

    #[test]
    fn a_circle_wall_is_an_annulus() {
        let profile = Profile::new(
            Plane::top(),
            vec![Region {
                outer: Loop {
                    curves: vec![Curve2::Circle { center: Point2::origin(), radius: 10.0, source: Some(3) }],
                },
                holes: vec![],
                source: None,
            }],
        );
        let t = thin_profile(&profile, 2.0, 1.0).unwrap();
        assert_eq!(t.regions.len(), 1);
        let a = area(&t.regions[0]);
        let expected = std::f64::consts::PI * (11.0f64.powi(2) - 8.0f64.powi(2));
        assert!((a - expected).abs() < 0.05, "{a} vs {expected}");
        assert_eq!(t.regions[0].outer.curves[0].source(), Some(3 ^ THIN_RIGHT));
        assert!(thin_profile(&profile, 10.0, 0.0).is_err());
    }
}

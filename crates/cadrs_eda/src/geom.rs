//! Curve maths: arcs through three points, and curves as polylines. Results are `f64`
//! nanometres.

use crate::units::{Nm, Pt};
use std::f64::consts::TAU;

/// The centre and radius of the circle through `a`, `b` and `c`; `None` when they are in line.
pub fn circle_through(a: Pt, b: Pt, c: Pt) -> Option<([f64; 2], f64)> {
    let (ax, ay, bx, by, cx, cy) = (a.x as f64, a.y as f64, b.x as f64, b.y as f64, c.x as f64, c.y as f64);
    let d = 2.0 * (ax * (by - cy) + bx * (cy - ay) + cx * (ay - by));
    if d.abs() < 1e-6 {
        return None;
    }
    let (a2, b2, c2) = (ax * ax + ay * ay, bx * bx + by * by, cx * cx + cy * cy);
    let ux = (a2 * (by - cy) + b2 * (cy - ay) + c2 * (ay - by)) / d;
    let uy = (a2 * (cx - bx) + b2 * (ax - cx) + c2 * (bx - ax)) / d;
    Some(([ux, uy], (ax - ux).hypot(ay - uy)))
}

/// An arc through `start`, `mid` and `end` as its centre, radius, start angle and signed sweep
/// (radians, positive counter-clockwise).
pub fn arc_params(start: Pt, mid: Pt, end: Pt) -> Option<([f64; 2], f64, f64, f64)> {
    let (c, r) = circle_through(start, mid, end)?;
    let ang = |p: Pt| (p.y as f64 - c[1]).atan2(p.x as f64 - c[0]);
    let (a0, am, a1) = (ang(start), ang(mid), ang(end));
    let ccw = |from: f64, to: f64| (to - from).rem_euclid(TAU);
    let sweep = if ccw(a0, am) <= ccw(a0, a1) { ccw(a0, a1) } else { ccw(a0, a1) - TAU };
    Some((c, r, a0, sweep))
}

/// Segments for a curve of radius `r` turning `sweep` radians, so no chord strays more than
/// `max_err` from the curve.
pub fn segments_for(r: f64, sweep: f64, max_err: f64) -> usize {
    if r <= max_err {
        return 4;
    }
    let step = 2.0 * (1.0 - max_err / r).acos();
    ((sweep.abs() / step).ceil() as usize).clamp(2, 720)
}

/// The arc through `start`, `mid`, `end` as points from start to end. A straight line when the
/// three are in line.
pub fn arc_points(start: Pt, mid: Pt, end: Pt, max_err: Nm) -> Vec<[f64; 2]> {
    let f = |p: Pt| [p.x as f64, p.y as f64];
    let Some((c, r, a0, sweep)) = arc_params(start, mid, end) else {
        return vec![f(start), f(end)];
    };
    let n = segments_for(r, sweep, max_err as f64);
    let mut pts: Vec<[f64; 2]> = (0..=n)
        .map(|i| {
            let a = a0 + sweep * i as f64 / n as f64;
            [c[0] + r * a.cos(), c[1] + r * a.sin()]
        })
        .collect();
    // Exact ends.
    pts[0] = f(start);
    pts[n] = f(end);
    pts
}

/// A full circle as a closed loop (the first point is not repeated).
pub fn circle_points(center: Pt, radius: Nm, max_err: Nm) -> Vec<[f64; 2]> {
    let r = radius as f64;
    let n = segments_for(r, TAU, max_err as f64).max(8);
    (0..n)
        .map(|i| {
            let a = TAU * i as f64 / n as f64;
            [center.x as f64 + r * a.cos(), center.y as f64 + r * a.sin()]
        })
        .collect()
}

/// A cubic Bézier as points.
pub fn bezier_points(p: [Pt; 4], max_err: Nm) -> Vec<[f64; 2]> {
    let q = p.map(|p| [p.x as f64, p.y as f64]);
    let hull = (0..3).map(|i| (q[i + 1][0] - q[i][0]).hypot(q[i + 1][1] - q[i][1])).sum::<f64>();
    let n = ((hull / (max_err.max(1) as f64 * 8.0)).sqrt().ceil() as usize).clamp(2, 256);
    (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            let u = 1.0 - t;
            let (b0, b1, b2, b3) = (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
            [
                b0 * q[0][0] + b1 * q[1][0] + b2 * q[2][0] + b3 * q[3][0],
                b0 * q[0][1] + b1 * q[1][1] + b2 * q[2][1] + b3 * q[3][1],
            ]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::mm;

    #[test]
    fn arc_direction_follows_the_mid_point() {
        let (s, e) = (Pt::mm(1.0, 0.0), Pt::mm(-1.0, 0.0));
        let (_, r, _, up) = arc_params(s, Pt::mm(0.0, 1.0), e).unwrap();
        assert!((r - 1e6).abs() < 1e-3);
        assert!((up - std::f64::consts::PI).abs() < 1e-9);
        let (_, _, _, down) = arc_params(s, Pt::mm(0.0, -1.0), e).unwrap();
        assert!((down + std::f64::consts::PI).abs() < 1e-9);
        let pts = arc_points(s, Pt::mm(0.0, 1.0), e, mm(0.001));
        assert!(pts.iter().all(|p| p[1] >= -1e-6));
    }

    #[test]
    fn line_when_in_line() {
        assert_eq!(arc_points(Pt::mm(0.0, 0.0), Pt::mm(1.0, 0.0), Pt::mm(2.0, 0.0), 1000).len(), 2);
    }
}

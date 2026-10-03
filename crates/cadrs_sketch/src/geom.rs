//! 2D geometry helpers: vector math, distances, and arc construction (3-point and tangent arcs).

use std::f64::consts::{PI, TAU};
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

use serde::{Deserialize, Serialize};

use crate::Vec2;

impl Add for Vec2 {
    type Output = Vec2;
    fn add(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x + o.x, self.y + o.y)
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, o: Vec2) {
        *self = *self + o;
    }
}

impl Sub for Vec2 {
    type Output = Vec2;
    fn sub(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x - o.x, self.y - o.y)
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, o: Vec2) {
        *self = *self - o;
    }
}

impl Mul<f64> for Vec2 {
    type Output = Vec2;
    fn mul(self, k: f64) -> Vec2 {
        Vec2::new(self.x * k, self.y * k)
    }
}

impl Div<f64> for Vec2 {
    type Output = Vec2;
    fn div(self, k: f64) -> Vec2 {
        Vec2::new(self.x / k, self.y / k)
    }
}

impl Neg for Vec2 {
    type Output = Vec2;
    fn neg(self) -> Vec2 {
        Vec2::new(-self.x, -self.y)
    }
}

impl Vec2 {
    pub fn dot(self, o: Vec2) -> f64 {
        self.x * o.x + self.y * o.y
    }

    /// The z component of the 3D cross product (positive when `o` is counter-clockwise from
    /// `self`).
    pub fn cross(self, o: Vec2) -> f64 {
        self.x * o.y - self.y * o.x
    }

    pub fn length(self) -> f64 {
        self.dot(self).sqrt()
    }

    /// The unit vector in this direction, or zero for a zero vector.
    pub fn normalize(self) -> Vec2 {
        let l = self.length();
        if l < 1e-300 { Vec2::ZERO } else { self / l }
    }

    /// Rotated 90° counter-clockwise.
    pub fn perp(self) -> Vec2 {
        Vec2::new(-self.y, self.x)
    }

    /// The angle from +X, in radians (-π..=π).
    pub fn angle(self) -> f64 {
        self.y.atan2(self.x)
    }

    pub fn from_angle(a: f64) -> Vec2 {
        Vec2::new(a.cos(), a.sin())
    }

    pub fn lerp(self, o: Vec2, t: f64) -> Vec2 {
        self + (o - self) * t
    }

    pub fn midpoint(self, o: Vec2) -> Vec2 {
        self.lerp(o, 0.5)
    }

    pub fn min(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.min(o.x), self.y.min(o.y))
    }

    pub fn max(self, o: Vec2) -> Vec2 {
        Vec2::new(self.x.max(o.x), self.y.max(o.y))
    }
}

/// The mirror image of `p` in the infinite line through `a` and `b` (`p` itself if the line is
/// degenerate).
pub fn mirror_point(p: Vec2, a: Vec2, b: Vec2) -> Vec2 {
    let d = b - a;
    let l2 = d.dot(d);
    if l2 < 1e-300 {
        return p;
    }
    let foot = a + d * ((p - a).dot(d) / l2);
    foot * 2.0 - p
}

/// Distance from `p` to the segment `a`–`b`.
pub fn dist_point_segment(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let ab = b - a;
    let len2 = ab.dot(ab);
    if len2 < 1e-300 {
        return p.distance(a);
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Normalizes an angle to `0..2π`.
pub fn norm_angle(a: f64) -> f64 {
    let r = a.rem_euclid(TAU);
    if r >= TAU { 0.0 } else { r }
}

/// A circular arc: `sweep` radians counter-clockwise from `start_angle` (a negative sweep runs
/// clockwise).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ArcGeom {
    pub center: Vec2,
    pub radius: f64,
    pub start_angle: f64,
    pub sweep: f64,
}

impl ArcGeom {
    /// The arc of a circle from `start` counter-clockwise to `end` (how sketch arcs are stored).
    pub fn ccw(center: Vec2, start: Vec2, end: Vec2) -> Self {
        let a0 = (start - center).angle();
        let a1 = (end - center).angle();
        let mut sweep = norm_angle(a1 - a0);
        if sweep < 1e-12 {
            sweep = TAU;
        }
        Self {
            center,
            radius: center.distance(start),
            start_angle: a0,
            sweep,
        }
    }

    pub fn point_at(&self, angle: f64) -> Vec2 {
        self.center + Vec2::from_angle(angle) * self.radius
    }

    pub fn start(&self) -> Vec2 {
        self.point_at(self.start_angle)
    }

    pub fn end(&self) -> Vec2 {
        self.point_at(self.start_angle + self.sweep)
    }

    pub fn mid(&self) -> Vec2 {
        self.point_at(self.start_angle + self.sweep / 2.0)
    }

    /// The same arc, running counter-clockwise (start and end swap if it ran clockwise).
    pub fn to_ccw(self) -> Self {
        if self.sweep >= 0.0 {
            self
        } else {
            Self {
                start_angle: self.start_angle + self.sweep,
                sweep: -self.sweep,
                ..self
            }
        }
    }

    /// True if the direction at `angle` from the center lies within the swept range.
    pub fn contains_angle(&self, angle: f64) -> bool {
        let a = self.to_ccw();
        norm_angle(angle - a.start_angle) <= a.sweep + 1e-12
    }

    /// Distance from `p` to the arc.
    pub fn distance(&self, p: Vec2) -> f64 {
        let d = p - self.center;
        if d.length() > 1e-300 && self.contains_angle(d.angle()) {
            (d.length() - self.radius).abs()
        } else {
            p.distance(self.start()).min(p.distance(self.end()))
        }
    }

    /// The unit tangent in the direction of travel at the start.
    pub fn start_tangent(&self) -> Vec2 {
        let t = Vec2::from_angle(self.start_angle).perp();
        if self.sweep >= 0.0 { t } else { -t }
    }

    /// The unit tangent in the direction of travel at the end.
    pub fn end_tangent(&self) -> Vec2 {
        let t = Vec2::from_angle(self.start_angle + self.sweep).perp();
        if self.sweep >= 0.0 { t } else { -t }
    }

    pub fn length(&self) -> f64 {
        self.radius * self.sweep.abs()
    }

    /// Points along the arc, `start` and `end` included, with about one segment per
    /// `max_angle` radians (at least `min_segments`).
    pub fn tessellate(&self, max_angle: f64, min_segments: usize) -> Vec<Vec2> {
        let n = ((self.sweep.abs() / max_angle).ceil() as usize).max(min_segments).max(1);
        (0..=n)
            .map(|i| self.point_at(self.start_angle + self.sweep * i as f64 / n as f64))
            .collect()
    }

    /// Bounding box `(min, max)`.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let mut lo = self.start().min(self.end());
        let mut hi = self.start().max(self.end());
        for k in 0..4 {
            let a = k as f64 * PI / 2.0;
            if self.contains_angle(a) {
                let p = self.point_at(a);
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
        (lo, hi)
    }
}

/// An ellipse: `center + a·cos t + b·sin t`, where `a` is the major semi-axis vector (to the
/// stored major point) and `b` the minor semi-axis vector, square to it (to its left). With a
/// non-zero `offset` (P3.7, X13), the curve that far from the ellipse along its outward normal
/// (an offset ellipse: not an ellipse, but it shares the ellipse's parameter and normals).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EllipseGeom {
    pub center: Vec2,
    /// The major semi-axis: from the center to the major point.
    pub a: Vec2,
    /// The minor radius.
    pub minor: f64,
    /// How far the curve lies outside the ellipse (negative: inside); 0 for the ellipse itself.
    pub offset: f64,
}

impl EllipseGeom {
    pub fn new(center: Vec2, major: Vec2, minor: f64) -> Self {
        Self {
            center,
            a: major - center,
            minor,
            offset: 0.0,
        }
    }

    /// The same ellipse's curve `offset` outside it.
    pub fn with_offset(self, offset: f64) -> Self {
        Self { offset, ..self }
    }

    /// The ellipse itself (no offset).
    pub fn base(&self) -> Self {
        Self { offset: 0.0, ..*self }
    }

    /// The gradient direction `(cos t / |a|, sin t / minor)` in the ellipse's axes (outward
    /// whatever the minor radius's sign) and its derivative.
    fn grad(&self, t: f64) -> (Vec2, Vec2) {
        let ra = self.major().max(1e-300);
        let rb = if self.minor.abs() < 1e-300 { 1e-300 } else { self.minor };
        let (s, c) = t.sin_cos();
        (Vec2::new(c / ra, s / rb), Vec2::new(-s / ra, c / rb))
    }

    /// The unit outward normal at parameter `t`.
    pub fn normal_at(&self, t: f64) -> Vec2 {
        let (g, _) = self.grad(t);
        let n = g.normalize();
        let u = self.u();
        u * n.x + u.perp() * n.y
    }

    /// The ellipse's perimeter (Simpson's rule on its speed).
    pub fn perimeter(&self) -> f64 {
        let n = 2000;
        let h = TAU / n as f64;
        let base = self.base();
        (0..=n)
            .map(|i| {
                let w = if i == 0 || i == n { 1.0 } else if i % 2 == 1 { 4.0 } else { 2.0 };
                w * base.tangent_at(i as f64 * h).length()
            })
            .sum::<f64>()
            * h
            / 3.0
    }

    /// True if an inward offset stays smooth: it is less than the ellipse's smallest radius of
    /// curvature (`b²/a`).
    pub fn offset_is_smooth(&self) -> bool {
        let (a, b) = (self.major(), self.minor.abs());
        let r = if a >= b { b * b / a.max(1e-300) } else { a * a / b.max(1e-300) };
        self.offset > -r + 1e-9
    }

    /// The major radius.
    pub fn major(&self) -> f64 {
        self.a.length()
    }

    /// The unit direction of the major axis.
    pub fn u(&self) -> Vec2 {
        let u = self.a.normalize();
        if u == Vec2::ZERO { Vec2::new(1.0, 0.0) } else { u }
    }

    /// The minor semi-axis vector.
    pub fn b(&self) -> Vec2 {
        self.u().perp() * self.minor
    }

    /// The point at parameter `t` (radians; 0 is the major point).
    pub fn point_at(&self, t: f64) -> Vec2 {
        let p = self.center + self.a * t.cos() + self.b() * t.sin();
        if self.offset == 0.0 { p } else { p + self.normal_at(t) * self.offset }
    }

    /// The derivative of [`EllipseGeom::point_at`] (the direction of travel, counter-clockwise).
    pub fn tangent_at(&self, t: f64) -> Vec2 {
        let d = self.a * -t.sin() + self.b() * t.cos();
        if self.offset == 0.0 {
            return d;
        }
        // n = g/|g|, so n' = (g' − n (n·g'))/|g|.
        let (g, dg) = self.grad(t);
        let l = g.length().max(1e-300);
        let n = g / l;
        let dn = (dg - n * n.dot(dg)) / l;
        let u = self.u();
        d + (u * dn.x + u.perp() * dn.y) * self.offset
    }

    /// `p` in the ellipse's own axes (major along x).
    pub fn local(&self, p: Vec2) -> Vec2 {
        let u = self.u();
        let d = p - self.center;
        Vec2::new(d.dot(u), u.cross(d))
    }

    /// The parameter of the point of the ellipse nearest `p` (Newton on the foot-point
    /// condition, from the best of a coarse sampling). An offset curve shares the ellipse's
    /// normals, so its nearest point has the same parameter.
    pub fn nearest_t(&self, p: Vec2) -> f64 {
        if self.offset != 0.0 {
            return self.base().nearest_t(p);
        }
        let mut best = (0.0, f64::INFINITY);
        for i in 0..64 {
            let t = i as f64 * TAU / 64.0;
            let d = self.point_at(t).distance(p);
            if d < best.1 {
                best = (t, d);
            }
        }
        let mut t = best.0;
        for _ in 0..20 {
            // f(t) = (P(t) − p)·P'(t); f'(t) = |P'|² + (P − p)·P''.
            let q = self.point_at(t) - p;
            let d1 = self.tangent_at(t);
            let d2 = self.point_at(t) - self.center;
            let f = q.dot(d1);
            let df = d1.dot(d1) - q.dot(d2);
            if df.abs() < 1e-300 {
                break;
            }
            let step = f / df;
            t -= step.clamp(-0.5, 0.5);
            if step.abs() < 1e-14 {
                break;
            }
        }
        norm_angle(t)
    }

    /// The parameter of a point on the ellipse (its eccentric angle; a point off the ellipse
    /// gives the parameter of the point on it along the same "ray" in the ellipse's axes).
    pub fn param_of(&self, p: Vec2) -> f64 {
        let l = self.local(p);
        norm_angle((l.y / self.minor).atan2(l.x / self.major().max(1e-300)))
    }

    /// The nearest point of the ellipse to `p`.
    pub fn closest(&self, p: Vec2) -> Vec2 {
        self.point_at(self.nearest_t(p))
    }

    /// Distance from `p` to the ellipse.
    pub fn distance(&self, p: Vec2) -> f64 {
        self.closest(p).distance(p)
    }

    /// The implicit value `(x/a)² + (y/b)² − 1` of `p` (negative inside). For an offset curve,
    /// its signed distance (negative inside), scaled to about the same size near the curve.
    pub fn implicit(&self, p: Vec2) -> f64 {
        if self.offset != 0.0 {
            let base = self.base();
            let t = base.nearest_t(p);
            let d = (p - base.point_at(t)).dot(base.normal_at(t)) - self.offset;
            return d * 2.0 / self.major().min(self.minor.abs()).max(1e-300);
        }
        let l = self.local(p);
        let (ra, rb) = (self.major().max(1e-300), self.minor.abs().max(1e-300));
        (l.x / ra).powi(2) + (l.y / rb).powi(2) - 1.0
    }

    /// The enclosed area, π·a·b; for an offset curve Steiner's `π·a·b + L·d + π·d²` (L the
    /// ellipse's perimeter; exact while the curve stays smooth).
    pub fn area(&self) -> f64 {
        let e = PI * self.major() * self.minor.abs();
        if self.offset == 0.0 {
            e
        } else {
            e + self.perimeter() * self.offset + PI * self.offset * self.offset
        }
    }

    /// Points around the whole ellipse (the first repeated at the end), about one segment
    /// per `max_angle` of parameter (at least `min_segments`).
    pub fn tessellate(&self, max_angle: f64, min_segments: usize) -> Vec<Vec2> {
        self.tessellate_range(0.0, TAU, max_angle, min_segments)
    }

    /// Points from parameter `t0` to `t1`, both included.
    pub fn tessellate_range(&self, t0: f64, t1: f64, max_angle: f64, min_segments: usize) -> Vec<Vec2> {
        let n = (((t1 - t0).abs() / max_angle).ceil() as usize)
            .max(min_segments)
            .max(1);
        (0..=n)
            .map(|i| self.point_at(t0 + (t1 - t0) * i as f64 / n as f64))
            .collect()
    }

    /// Bounding box `(min, max)`.
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let (a, b) = (self.a, self.b());
        let hx = (a.x * a.x + b.x * b.x).sqrt() + self.offset.max(0.0);
        let hy = (a.y * a.y + b.y * b.y).sqrt() + self.offset.max(0.0);
        (
            self.center - Vec2::new(hx, hy),
            self.center + Vec2::new(hx, hy),
        )
    }
}

/// Part of an ellipse: from parameter `t0` counter-clockwise through `sweep` (0..2π) (an arc
/// edge of a part seen at an angle, projected by Use). The ellipse's minor radius is positive,
/// so its parameter runs counter-clockwise.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EllipseArcGeom {
    pub e: EllipseGeom,
    pub t0: f64,
    pub sweep: f64,
}

impl EllipseArcGeom {
    /// The arc of the ellipse (center, major point, minor radius) from `start` counter-clockwise
    /// to `end` (both on the ellipse, or put onto it along their parameter).
    pub fn ccw(center: Vec2, major: Vec2, minor: f64, start: Vec2, end: Vec2) -> Self {
        let e = EllipseGeom::new(center, major, minor.abs());
        let t0 = e.param_of(start);
        let mut sweep = norm_angle(e.param_of(end) - t0);
        if sweep < 1e-12 {
            sweep = TAU;
        }
        Self { e, t0, sweep }
    }

    pub fn point_at(&self, t: f64) -> Vec2 {
        self.e.point_at(t)
    }

    pub fn start(&self) -> Vec2 {
        self.e.point_at(self.t0)
    }

    pub fn end(&self) -> Vec2 {
        self.e.point_at(self.t0 + self.sweep)
    }

    pub fn mid(&self) -> Vec2 {
        self.e.point_at(self.t0 + self.sweep / 2.0)
    }

    /// True if the parameter `t` is on the arc.
    pub fn contains_t(&self, t: f64) -> bool {
        norm_angle(t - self.t0) <= self.sweep + 1e-12
    }

    /// The parameter of the arc's point nearest `p` (an end, off the arc's span).
    pub fn nearest_t(&self, p: Vec2) -> f64 {
        let t = self.e.nearest_t(p);
        if self.contains_t(t) {
            return self.t0 + norm_angle(t - self.t0);
        }
        let (a, b) = (self.t0, self.t0 + self.sweep);
        if self.e.point_at(a).distance(p) <= self.e.point_at(b).distance(p) { a } else { b }
    }

    pub fn closest(&self, p: Vec2) -> Vec2 {
        self.e.point_at(self.nearest_t(p))
    }

    pub fn distance(&self, p: Vec2) -> f64 {
        self.closest(p).distance(p)
    }

    /// The unit direction of travel at the start (into the arc) and at the end (out of it).
    pub fn start_tangent(&self) -> Vec2 {
        self.e.tangent_at(self.t0).normalize()
    }

    pub fn end_tangent(&self) -> Vec2 {
        self.e.tangent_at(self.t0 + self.sweep).normalize()
    }

    /// Points from start to end, both included.
    pub fn tessellate(&self, max_angle: f64, min_segments: usize) -> Vec<Vec2> {
        self.e.tessellate_range(self.t0, self.t0 + self.sweep, max_angle, min_segments)
    }

    /// Bounding box `(min, max)` (of its points, sampled finely).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let pts = self.tessellate(PI / 180.0, 8);
        pts.iter().fold((pts[0], pts[0]), |(lo, hi), p| (lo.min(*p), hi.max(*p)))
    }
}

/// A cubic Bézier curve (Final, S12.14: the sketch's spline, which the Curvature constraint
/// joins with G2 continuity): `p[0]` and `p[3]` are its ends, `p[1]` and `p[2]` its control
/// points. The parameter runs 0..1.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BezierGeom {
    pub p: [Vec2; 4],
}

impl BezierGeom {
    pub fn new(p: [Vec2; 4]) -> Self {
        Self { p }
    }

    /// The point at `t`.
    pub fn point_at(&self, t: f64) -> Vec2 {
        let u = 1.0 - t;
        let [a, b, c, d] = self.p;
        a * (u * u * u) + b * (3.0 * u * u * t) + c * (3.0 * u * t * t) + d * (t * t * t)
    }

    /// The first derivative at `t`.
    pub fn tangent_at(&self, t: f64) -> Vec2 {
        let u = 1.0 - t;
        let [a, b, c, d] = self.p;
        (b - a) * (3.0 * u * u) + (c - b) * (6.0 * u * t) + (d - c) * (3.0 * t * t)
    }

    /// The second derivative at `t`.
    pub fn second_at(&self, t: f64) -> Vec2 {
        let [a, b, c, d] = self.p;
        (c - b * 2.0 + a) * (6.0 * (1.0 - t)) + (d - c * 2.0 + b) * (6.0 * t)
    }

    /// The signed curvature at `t` (positive turning left as `t` grows).
    pub fn curvature_at(&self, t: f64) -> f64 {
        let (d1, d2) = (self.tangent_at(t), self.second_at(t));
        let l = d1.length();
        if l < 1e-12 { 0.0 } else { d1.cross(d2) / (l * l * l) }
    }

    /// The same curve, run the other way.
    pub fn reversed(&self) -> Self {
        let [a, b, c, d] = self.p;
        Self { p: [d, c, b, a] }
    }

    /// The part from `t0` to `t1` (either order), exactly (a cubic again).
    pub fn sub(&self, t0: f64, t1: f64) -> Self {
        // Blossom: the control points of the piece are f(t0,t0,t0), f(t0,t0,t1), f(t0,t1,t1),
        // f(t1,t1,t1).
        let blossom = |x: f64, y: f64, z: f64| {
            let lerp = |p: Vec2, q: Vec2, t: f64| p + (q - p) * t;
            let [a, b, c, d] = self.p;
            let (ab, bc, cd) = (lerp(a, b, x), lerp(b, c, x), lerp(c, d, x));
            let (abc, bcd) = (lerp(ab, bc, y), lerp(bc, cd, y));
            lerp(abc, bcd, z)
        };
        Self {
            p: [
                blossom(t0, t0, t0),
                blossom(t0, t0, t1),
                blossom(t0, t1, t1),
                blossom(t1, t1, t1),
            ],
        }
    }

    /// `½∫ p × p' dt` over the curve: exact (the integrand is a quintic, and three-point
    /// Gauss–Legendre integrates quintics exactly).
    pub fn area_term(&self) -> f64 {
        let x = (0.6_f64).sqrt() / 2.0;
        [(0.5 - x, 5.0 / 18.0), (0.5, 8.0 / 18.0), (0.5 + x, 5.0 / 18.0)]
            .iter()
            .map(|(t, w)| w * self.point_at(*t).cross(self.tangent_at(*t)))
            .sum::<f64>()
            / 2.0
    }

    /// Points along the curve, both ends included, at least `min_segments` and about one per
    /// `max_angle` of turning.
    pub fn tessellate(&self, max_angle: f64, min_segments: usize) -> Vec<Vec2> {
        // The control polygon's total turn bounds the curve's.
        let [a, b, c, d] = self.p;
        let turn = |u: Vec2, v: Vec2| {
            if u.length() < 1e-12 || v.length() < 1e-12 { 0.0 } else { u.cross(v).atan2(u.dot(v)).abs() }
        };
        let total = turn(b - a, c - b) + turn(c - b, d - c);
        let n = ((total / max_angle).ceil() as usize).max(min_segments).max(8);
        (0..=n).map(|i| self.point_at(i as f64 / n as f64)).collect()
    }

    /// The parameter of the point nearest `p` (0..1): the best of a sampling, refined by Newton.
    pub fn nearest_t(&self, p: Vec2) -> f64 {
        let n = 64;
        let mut best = (0.0, f64::INFINITY);
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let d = self.point_at(t).distance(p);
            if d < best.1 {
                best = (t, d);
            }
        }
        let mut t: f64 = best.0;
        for _ in 0..20 {
            let q = self.point_at(t) - p;
            let (d1, d2) = (self.tangent_at(t), self.second_at(t));
            let f = q.dot(d1);
            let df = d1.dot(d1) + q.dot(d2);
            if df.abs() < 1e-300 {
                break;
            }
            let step = f / df;
            t = (t - step.clamp(-0.1, 0.1)).clamp(0.0, 1.0);
            if step.abs() < 1e-14 {
                break;
            }
        }
        t
    }

    /// Distance from `p` to the curve.
    pub fn distance(&self, p: Vec2) -> f64 {
        self.point_at(self.nearest_t(p)).distance(p)
    }

    /// The curve's length (Gauss–Legendre on 16 pieces).
    pub fn length(&self) -> f64 {
        let x = (0.6_f64).sqrt() / 2.0;
        let n = 16;
        (0..n)
            .map(|k| {
                let (t0, h) = (k as f64 / n as f64, 1.0 / n as f64);
                [(0.5 - x, 5.0 / 18.0), (0.5, 8.0 / 18.0), (0.5 + x, 5.0 / 18.0)]
                    .iter()
                    .map(|(s, w)| w * self.tangent_at(t0 + s * h).length())
                    .sum::<f64>()
                    * h
            })
            .sum()
    }

    /// Bounding box `(min, max)` (of the curve, not its control points).
    pub fn bounds(&self) -> (Vec2, Vec2) {
        let pts = self.tessellate(std::f64::consts::PI / 90.0, 32);
        let mut lo = pts[0];
        let mut hi = pts[0];
        for q in &pts {
            lo = lo.min(*q);
            hi = hi.max(*q);
        }
        (lo, hi)
    }
}

/// The circle through three points: `(center, radius)`, or `None` if they are collinear.
pub fn circle_through(a: Vec2, b: Vec2, c: Vec2) -> Option<(Vec2, f64)> {
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    let scale = (b - a).length().max((c - a).length()).max(1e-300);
    if d.abs() < 1e-9 * scale * scale {
        return None;
    }
    let (a2, b2, c2) = (a.dot(a), b.dot(b), c.dot(c));
    let center = Vec2::new(
        (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d,
        (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d,
    );
    Some((center, center.distance(a)))
}

/// The arc that starts at `start`, ends at `end` and passes through `through` (the 3-point
/// arc tool: two endpoints, then a point on the arc). `None` if the points are collinear.
pub fn arc_through(start: Vec2, end: Vec2, through: Vec2) -> Option<ArcGeom> {
    let (center, radius) = circle_through(start, end, through)?;
    let a0 = (start - center).angle();
    let ccw = norm_angle(end.angle_from(center) - a0);
    let mid = norm_angle(through.angle_from(center) - a0);
    // Counter-clockwise from start, `through` comes before `end` iff the arc runs ccw.
    let sweep = if mid < ccw { ccw } else { ccw - std::f64::consts::TAU };
    Some(ArcGeom {
        center,
        radius,
        start_angle: a0,
        sweep,
    })
}

impl Vec2 {
    fn angle_from(self, center: Vec2) -> f64 {
        (self - center).angle()
    }
}

/// The arc that leaves `start` along `tangent` and ends at `end` (the tangent arc tool).
/// `None` if `end` lies on the tangent line (the "arc" would be straight) or on `start`.
pub fn tangent_arc(start: Vec2, tangent: Vec2, end: Vec2) -> Option<ArcGeom> {
    let t = tangent.normalize();
    let chord = end - start;
    let n = t.perp();
    let h = chord.dot(n);
    if chord.length() < 1e-9 || h.abs() < 1e-9 * chord.length() {
        return None;
    }
    // The center lies on the normal through `start`, equidistant from `start` and `end`:
    // |start + n·r - end|² = r²  =>  r = |chord|² / (2 chord·n) (signed).
    let r = chord.dot(chord) / (2.0 * h);
    let center = start + n * r;
    let a0 = (start - center).angle();
    let a1 = (end - center).angle();
    // r > 0: the center is to the left of the tangent, so the arc turns left (ccw).
    let sweep = if r > 0.0 {
        norm_angle(a1 - a0)
    } else {
        -norm_angle(a0 - a1)
    };
    Some(ArcGeom {
        center,
        radius: r.abs(),
        start_angle: a0,
        sweep,
    })
}

/// True if segment `a`–`b` intersects the axis-aligned box `lo`–`hi` (Liang–Barsky).
pub fn segment_hits_box(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> bool {
    let d = b - a;
    let mut t0 = 0.0f64;
    let mut t1 = 1.0f64;
    for (p, q) in [
        (-d.x, a.x - lo.x),
        (d.x, hi.x - a.x),
        (-d.y, a.y - lo.y),
        (d.y, hi.y - a.y),
    ] {
        if p.abs() < 1e-300 {
            if q < 0.0 {
                return false;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
            if t0 > t1 {
                return false;
            }
        }
    }
    true
}

/// Signed area of a closed polygon (positive when counter-clockwise).
pub fn polygon_area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i].cross(pts[(i + 1) % n])).sum::<f64>() / 2.0
}

/// True if `p` is inside the closed polygon (even-odd rule).
pub fn point_in_polygon(p: Vec2, pts: &[Vec2]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn bezier_basics() {
        // A quarter circle's usual cubic: k = 4(√2 − 1)/3.
        let k = 4.0 * (2f64.sqrt() - 1.0) / 3.0;
        let b = BezierGeom::new([Vec2::new(1.0, 0.0), Vec2::new(1.0, k), Vec2::new(k, 1.0), Vec2::new(0.0, 1.0)]);
        // Area of the sector from the origin: ½∮ over the arc plus two radii (which add 0).
        assert!((b.area_term() - std::f64::consts::FRAC_PI_4).abs() < 3e-4, "{}", b.area_term());
        assert!((b.curvature_at(0.0) - 1.0).abs() < 0.2);
        assert!((b.length() - std::f64::consts::FRAC_PI_2).abs() < 1e-3);
        let (l, r) = (b.sub(0.0, 0.4), b.sub(0.4, 1.0));
        assert!(l.p[3].distance(b.point_at(0.4)) < 1e-12);
        assert!(r.point_at(0.5).distance(b.point_at(0.7)) < 1e-12);
        assert!((l.area_term() + r.area_term() - b.area_term()).abs() < 1e-12);
        assert!((b.nearest_t(b.point_at(0.3) * 1.1) - 0.3).abs() < 1e-3);
        assert!(b.reversed().point_at(0.25).distance(b.point_at(0.75)) < 1e-12);
    }

    #[test]
    fn segment_distance() {
        let (a, b) = (Vec2::ZERO, Vec2::new(10.0, 0.0));
        assert!(close(dist_point_segment(Vec2::new(5.0, 3.0), a, b), 3.0));
        assert!(close(dist_point_segment(Vec2::new(-4.0, 3.0), a, b), 5.0));
        assert!(close(dist_point_segment(Vec2::new(13.0, 4.0), a, b), 5.0));
    }

    #[test]
    fn three_point_arc() {
        // Upper half circle from (1,0) to (-1,0) through (0,1): counter-clockwise.
        let arc = arc_through(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, 1.0))
            .unwrap();
        assert!(arc.center.distance(Vec2::ZERO) < 1e-9);
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.sweep, PI));
        assert!(arc.mid().distance(Vec2::new(0.0, 1.0)) < 1e-9);
        // Through (0,-1) instead: clockwise, the lower half.
        let arc = arc_through(Vec2::new(1.0, 0.0), Vec2::new(-1.0, 0.0), Vec2::new(0.0, -1.0))
            .unwrap();
        assert!(close(arc.sweep, -PI));
        assert!(arc.mid().distance(Vec2::new(0.0, -1.0)) < 1e-9);
        let ccw = arc.to_ccw();
        assert!(ccw.start().distance(Vec2::new(-1.0, 0.0)) < 1e-9);
        assert!(ccw.end().distance(Vec2::new(1.0, 0.0)) < 1e-9);
        // A short arc: most of the circle is not swept.
        let arc = arc_through(
            Vec2::new(0.0, 0.0),
            Vec2::new(10.0, 0.0),
            Vec2::new(5.0, 1.0),
        )
        .unwrap();
        assert!(arc.sweep < 0.0 && arc.sweep.abs() < PI);
        assert!(arc.end().distance(Vec2::new(10.0, 0.0)) < 1e-9);
        assert!(arc_through(Vec2::ZERO, Vec2::new(2.0, 0.0), Vec2::new(1.0, 0.0)).is_none());
    }

    #[test]
    fn tangent_arc_leaves_along_the_tangent() {
        // Leaving (0,0) heading +X, ending at (0,2): a left-turning half circle, center (0,1).
        let arc = tangent_arc(Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 2.0)).unwrap();
        assert!(arc.center.distance(Vec2::new(0.0, 1.0)) < 1e-9);
        assert!(close(arc.radius, 1.0));
        assert!(close(arc.sweep, PI));
        assert!(arc.start_tangent().distance(Vec2::new(1.0, 0.0)) < 1e-9);
        assert!(arc.end().distance(Vec2::new(0.0, 2.0)) < 1e-9);
        // Ending below: turns right (clockwise).
        let arc = tangent_arc(Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(1.0, -1.0)).unwrap();
        assert!(arc.sweep < 0.0);
        assert!(arc.start_tangent().distance(Vec2::new(1.0, 0.0)) < 1e-9);
        assert!(arc.end().distance(Vec2::new(1.0, -1.0)) < 1e-9);
        assert!(close(arc.sweep, -PI / 2.0));
        // Straight ahead: no arc.
        assert!(tangent_arc(Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(5.0, 0.0)).is_none());
    }

    #[test]
    fn arc_distance_and_bounds() {
        let arc = ArcGeom::ccw(Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0));
        assert!(close(arc.sweep, PI / 2.0));
        assert!(close(arc.distance(Vec2::new(2.0, 2.0) * (0.5f64).sqrt()), 1.0));
        // Opposite side: nearest is an endpoint.
        assert!(close(arc.distance(Vec2::new(-1.0, 0.0)), 2f64.sqrt()));
        let (lo, hi) = arc.bounds();
        assert!(lo.distance(Vec2::ZERO) < 1e-9 && hi.distance(Vec2::new(1.0, 1.0)) < 1e-9);
    }

    #[test]
    fn box_intersection() {
        let (lo, hi) = (Vec2::ZERO, Vec2::new(10.0, 10.0));
        assert!(segment_hits_box(Vec2::new(-5.0, 5.0), Vec2::new(15.0, 5.0), lo, hi));
        assert!(!segment_hits_box(Vec2::new(-5.0, 15.0), Vec2::new(15.0, 12.0), lo, hi));
        assert!(segment_hits_box(Vec2::new(2.0, 2.0), Vec2::new(3.0, 3.0), lo, hi));
    }
}

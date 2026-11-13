//! Small numeric helpers for the native kernel: vector aliases, frames, tolerances, quadrature.

use super::tol;
use nalgebra::{Matrix3, Point2, Point3, Vector2, Vector3};

/// A point in model space (mm).
pub type P3 = Point3<f64>;
/// A vector in model space.
pub type V3 = Vector3<f64>;
pub type P2 = Point2<f64>;
pub type V2 = Vector2<f64>;

/// The linear modelling tolerance (mm): points closer than this are the same point
/// (OCCT's `Precision::Confusion`).
pub const TOL: f64 = 1e-7;
/// Angular tolerance (radians) for parallel and coincident directions.
pub const ANG_TOL: f64 = 1e-10;

pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;

/// An orthonormal frame: origin and three axes. The axes may be left-handed after a reflection
/// (surfaces keep evaluating through them; faces flip their sense instead).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// The origin.
    pub o: P3,
    /// The x axis (a unit vector).
    pub x: V3,
    /// The y axis.
    pub y: V3,
    /// The z axis.
    pub z: V3,
}

impl Frame {
    /// The world frame: the origin and the coordinate axes.
    pub fn world() -> Self {
        Self {
            o: P3::origin(),
            x: V3::x(),
            y: V3::y(),
            z: V3::z(),
        }
    }

    /// A right-handed frame with `z` along `dir` and `x` along `x_hint` made square to it.
    pub fn from_xz(o: P3, x_hint: V3, z: V3) -> Self {
        let z = z.normalize();
        let mut x = x_hint - z * x_hint.dot(&z);
        if x.norm() < tol::DEGENERATE {
            x = any_perpendicular(&z);
        }
        let x = x.normalize();
        let y = z.cross(&x);
        Self { o, x, y, z }
    }

    /// A right-handed frame with `z` along `z` and any x square to it.
    pub fn from_z(o: P3, z: V3) -> Self {
        Self::from_xz(o, any_perpendicular(&z), z)
    }

    /// Model point → local coordinates.
    pub fn local(&self, p: &P3) -> V3 {
        let d = p - self.o;
        V3::new(d.dot(&self.x), d.dot(&self.y), d.dot(&self.z))
    }

    /// Local coordinates → model point.
    pub fn world_point(&self, l: &V3) -> P3 {
        self.o + self.x * l.x + self.y * l.y + self.z * l.z
    }

    /// A local vector in model space.
    pub fn world_vec(&self, l: &V3) -> V3 {
        self.x * l.x + self.y * l.y + self.z * l.z
    }

    /// The frame moved by `m` (its axes scaled with it if `m` scales).
    pub fn transformed(&self, m: &Affine) -> Self {
        Self {
            o: m.point(&self.o),
            x: m.vector(&self.x),
            y: m.vector(&self.y),
            z: m.vector(&self.z),
        }
    }

    /// True when x × y = z (not mirrored).
    pub fn right_handed(&self) -> bool {
        self.x.cross(&self.y).dot(&self.z) > 0.0
    }
}

/// A unit vector square to `v`.
pub fn any_perpendicular(v: &V3) -> V3 {
    let a = if v.x.abs() < 0.6 {
        V3::x()
    } else if v.y.abs() < 0.6 {
        V3::y()
    } else {
        V3::z()
    };
    v.cross(&a).normalize()
}

/// An orthonormal (rotation or reflection) map plus a translation, optionally scaled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    /// The linear part: a scaled rotation or reflection.
    pub m: Matrix3<f64>,
    /// The translation.
    pub t: V3,
}

impl Affine {
    /// The map that moves nothing.
    pub fn identity() -> Self {
        Self {
            m: Matrix3::identity(),
            t: V3::zeros(),
        }
    }
    /// A translation by `t`.
    pub fn translation(t: V3) -> Self {
        Self {
            m: Matrix3::identity(),
            t,
        }
    }
    /// The image of a point.
    pub fn point(&self, p: &P3) -> P3 {
        P3::from(self.m * p.coords + self.t)
    }
    /// The image of a vector (the linear part only).
    pub fn vector(&self, v: &V3) -> V3 {
        self.m * v
    }
    /// The determinant of the linear part: negative for a reflection.
    pub fn det(&self) -> f64 {
        self.m.determinant()
    }
    /// The uniform scale factor (1 for rigid motions).
    pub fn scale(&self) -> f64 {
        self.det().abs().cbrt()
    }
}

/// Gauss–Legendre nodes and weights on [-1, 1].
pub struct Gauss {
    pub x: &'static [f64],
    pub w: &'static [f64],
}

pub const GAUSS8: Gauss = Gauss {
    x: &[
        -0.960_289_856_497_536_3,
        -0.796_666_477_413_626_7,
        -0.525_532_409_916_329,
        -0.183_434_642_495_649_8,
        0.183_434_642_495_649_8,
        0.525_532_409_916_329,
        0.796_666_477_413_626_7,
        0.960_289_856_497_536_3,
    ],
    w: &[
        0.101_228_536_290_376_26,
        0.222_381_034_453_374_47,
        0.313_706_645_877_887_3,
        0.362_683_783_378_362,
        0.362_683_783_378_362,
        0.313_706_645_877_887_3,
        0.222_381_034_453_374_47,
        0.101_228_536_290_376_26,
    ],
};

pub const GAUSS16: Gauss = Gauss {
    x: &[
        -0.989_400_934_991_649_9,
        -0.944_575_023_073_232_6,
        -0.865_631_202_387_831_8,
        -0.755_404_408_355_003,
        -0.617_876_244_402_643_8,
        -0.458_016_777_657_227_4,
        -0.281_603_550_779_258_9,
        -0.095_012_509_837_637_44,
        0.095_012_509_837_637_44,
        0.281_603_550_779_258_9,
        0.458_016_777_657_227_4,
        0.617_876_244_402_643_8,
        0.755_404_408_355_003,
        0.865_631_202_387_831_8,
        0.944_575_023_073_232_6,
        0.989_400_934_991_649_9,
    ],
    w: &[
        0.027_152_459_411_754_095,
        0.062_253_523_938_647_89,
        0.095_158_511_682_492_78,
        0.124_628_971_255_533_87,
        0.149_595_988_816_576_73,
        0.169_156_519_395_002_54,
        0.182_603_415_044_923_6,
        0.189_450_610_455_068_5,
        0.189_450_610_455_068_5,
        0.182_603_415_044_923_6,
        0.169_156_519_395_002_54,
        0.149_595_988_816_576_73,
        0.124_628_971_255_533_87,
        0.095_158_511_682_492_78,
        0.062_253_523_938_647_89,
        0.027_152_459_411_754_095,
    ],
};

/// Composite Gauss–Legendre of `f` over [a, b] in `n` equal pieces.
pub fn integrate<F: FnMut(f64) -> f64>(a: f64, b: f64, n: usize, g: &Gauss, mut f: F) -> f64 {
    let h = (b - a) / n as f64;
    let mut sum = 0.0;
    for k in 0..n {
        let (lo, hi) = (a + h * k as f64, a + h * (k + 1) as f64);
        let (m, r) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
        for (x, w) in g.x.iter().zip(g.w) {
            sum += w * f(m + r * x) * r;
        }
    }
    sum
}

/// `a` wrapped into [lo, lo + period).
pub fn wrap(a: f64, lo: f64, period: f64) -> f64 {
    let mut x = (a - lo) % period;
    if x < 0.0 {
        x += period;
    }
    lo + x
}

/// The value of `a + k·period` nearest `near`.
pub fn unwrap_near(a: f64, near: f64, period: f64) -> f64 {
    a + ((near - a) / period).round() * period
}

/// 2D cross product.
pub fn cross2(a: &V2, b: &V2) -> f64 {
    a.x * b.y - a.y * b.x
}

/// Signed area of a closed polygon (CCW positive).
pub fn polygon_area(pts: &[P2]) -> f64 {
    let n = pts.len();
    let mut s = 0.0;
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        s += a.x * b.y - b.x * a.y;
    }
    s * 0.5
}

/// Even–odd point in polygon.
pub fn point_in_polygon(p: &P2, pts: &[P2]) -> bool {
    let n = pts.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (a, b) = (pts[i], pts[j]);
        if (a.y > p.y) != (b.y > p.y) {
            let x = a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x);
            if p.x < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// Distance from `p` to the segment a–b in 2D.
pub fn seg_dist2(p: &P2, a: &P2, b: &P2) -> f64 {
    let ab = b - a;
    let l = ab.norm_squared();
    let t = if l < tol::TINY {
        0.0
    } else {
        ((p - a).dot(&ab) / l).clamp(0.0, 1.0)
    };
    (a + ab * t - p).norm()
}

/// Solves a 2×2 system; None when singular.
pub fn solve2(a: f64, b: f64, c: f64, d: f64, r0: f64, r1: f64) -> Option<(f64, f64)> {
    let det = a * d - b * c;
    if det.abs() < tol::TINY {
        return None;
    }
    Some(((r0 * d - b * r1) / det, (a * r1 - c * r0) / det))
}

/// Real roots of a·x² + b·x + c = 0.
pub fn quadratic(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a.abs() < tol::TINY {
        if b.abs() < tol::TINY {
            return vec![];
        }
        return vec![-c / b];
    }
    let disc = b * b - 4.0 * a * c;
    if disc < 0.0 {
        if disc > -1e-14 * (b * b).max(1.0) {
            return vec![-b / (2.0 * a)];
        }
        return vec![];
    }
    let s = disc.sqrt();
    let q = -0.5 * (b + b.signum() * s);
    let q = if q == 0.0 { -0.5 * b } else { q };
    let mut r = vec![q / a];
    if q.abs() > tol::TINY {
        r.push(c / q);
    } else {
        r.push(-b / a - q / a);
    }
    r.sort_by(f64::total_cmp);
    r
}

/// The real roots in [lo, hi] of the polynomial c[0] + c[1]·x + … + c[n]·xⁿ (c[n] ≠ 0), in
/// order: each found by bisection between neighbouring roots of its derivative (where it is
/// monotone), so every root where it changes sign is found, however close to another. A root
/// where it touches zero without changing sign is kept only where it is zero exactly.
pub fn poly_roots(c: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let eval = |x: f64| c.iter().rev().fold(0.0, |acc, &k| acc * x + k);
    let n = c.len().saturating_sub(1);
    if n == 0 || !(lo <= hi) {
        return Vec::new();
    }
    if n == 1 {
        let x = -c[0] / c[1];
        return if x >= lo && x <= hi {
            vec![x]
        } else {
            Vec::new()
        };
    }
    let dc: Vec<f64> = (1..=n).map(|k| c[k] * k as f64).collect();
    let mut stops = vec![lo];
    stops.extend(poly_roots(&dc, lo, hi));
    stops.push(hi);
    let mut out: Vec<f64> = Vec::new();
    for w in stops.windows(2) {
        let (mut a, mut b) = (w[0], w[1]);
        let (fa, fb) = (eval(a), eval(b));
        if fa == 0.0 {
            if out.last() != Some(&a) {
                out.push(a);
            }
            continue;
        }
        if fb == 0.0 || fa.signum() == fb.signum() {
            continue;
        }
        // Bisection to the last bit (monotone between the stops: one root).
        let rising = fb > 0.0;
        loop {
            let m = 0.5 * (a + b);
            if m <= a || m >= b {
                break;
            }
            let fm = eval(m);
            if fm == 0.0 {
                (a, b) = (m, m);
                break;
            }
            if (fm > 0.0) == rising {
                b = m;
            } else {
                a = m;
            }
        }
        out.push(0.5 * (a + b));
    }
    if eval(hi) == 0.0 && out.last() != Some(&hi) {
        out.push(hi);
    }
    out
}

/// Bounding box helper.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bbox {
    pub min: P3,
    pub max: P3,
}

impl Bbox {
    pub fn empty() -> Self {
        Self {
            min: P3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
            max: P3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
        }
    }
    pub fn add(&mut self, p: &P3) {
        self.min = self.min.inf(p);
        self.max = self.max.sup(p);
    }
    pub fn union(&mut self, o: &Bbox) {
        self.min = self.min.inf(&o.min);
        self.max = self.max.sup(&o.max);
    }
    pub fn grow(&self, d: f64) -> Bbox {
        Bbox {
            min: self.min - V3::repeat(d),
            max: self.max + V3::repeat(d),
        }
    }
    pub fn overlaps(&self, o: &Bbox) -> bool {
        self.min.x <= o.max.x
            && o.min.x <= self.max.x
            && self.min.y <= o.max.y
            && o.min.y <= self.max.y
            && self.min.z <= o.max.z
            && o.min.z <= self.max.z
    }
    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }
    pub fn center(&self) -> P3 {
        nalgebra::center(&self.min, &self.max)
    }
    pub fn diagonal(&self) -> f64 {
        if self.is_empty() {
            0.0
        } else {
            (self.max - self.min).norm()
        }
    }
    pub fn contains(&self, p: &P3) -> bool {
        p.x >= self.min.x
            && p.x <= self.max.x
            && p.y >= self.min.y
            && p.y <= self.max.y
            && p.z >= self.min.z
            && p.z <= self.max.z
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn gauss_integrates_polynomials_and_trig() {
        let v = integrate(0.0, 2.0, 1, &GAUSS8, |x| x.powi(7));
        assert!((v - 2f64.powi(8) / 8.0).abs() < 1e-11);
        let v = integrate(0.0, TAU, 8, &GAUSS16, |x| x.cos().powi(4));
        assert!((v - 3.0 * PI / 4.0).abs() < 1e-14);
    }
    /// The roots of products of known factors, close, far apart and a double one touching
    /// zero exactly; none outside the range.
    #[test]
    fn polynomial_roots() {
        let mul = |a: &[f64], b: &[f64]| {
            let mut r = vec![0.0; a.len() + b.len() - 1];
            for (i, x) in a.iter().enumerate() {
                for (j, y) in b.iter().enumerate() {
                    r[i + j] += x * y;
                }
            }
            r
        };
        let from = |rs: &[f64]| rs.iter().fold(vec![1.0], |p, r| mul(&p, &[-r, 1.0]));
        for rs in [
            vec![-3.0, -1.0, 2.0, 5.0],
            vec![1.0, 1.0 + 1e-6, 4.0, 4.5],
            vec![-100.0, 0.25, 0.5, 300.0],
            vec![0.7, 2.0],
        ] {
            let got = poly_roots(&from(&rs), -1e3, 1e3);
            assert_eq!(got.len(), rs.len(), "{rs:?}: {got:?}");
            for (g, r) in got.iter().zip(&rs) {
                assert!((g - r).abs() < 1e-9 * (1.0 + r.abs()), "{rs:?}: {got:?}");
            }
        }
        // x² (x − 2)²: two double roots, zero there exactly.
        assert_eq!(
            poly_roots(&from(&[0.0, 0.0, 2.0, 2.0]), -5.0, 5.0),
            vec![0.0, 2.0]
        );
        // (x² + 1)(x − 3): one root; none in a range short of it.
        assert_eq!(poly_roots(&[-3.0, 1.0, -3.0, 1.0], -10.0, 10.0).len(), 1);
        assert!(poly_roots(&[-3.0, 1.0, -3.0, 1.0], -10.0, 2.0).is_empty());
    }

    #[test]
    fn quadratic_roots() {
        let r = quadratic(1.0, -3.0, 2.0);
        assert!((r[0] - 1.0).abs() < 1e-15 && (r[1] - 2.0).abs() < 1e-15);
    }
}

//! Small vector helpers over `[f64; 3]` and the exact predicates.

pub type V3 = [f64; 3];

#[inline]
pub fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
#[inline]
pub fn add(a: V3, b: V3) -> V3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
#[inline]
pub fn scale(a: V3, s: f64) -> V3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
#[inline]
pub fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
#[inline]
pub fn cross(a: V3, b: V3) -> V3 {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
#[inline]
pub fn len(a: V3) -> f64 {
    dot(a, a).sqrt()
}
#[inline]
pub fn mid(a: V3, b: V3) -> V3 {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5, (a[2] + b[2]) * 0.5]
}
#[inline]
pub fn normalize(a: V3) -> V3 {
    let l = len(a);
    if l > 0.0 { scale(a, 1.0 / l) } else { a }
}

/// A triangle's area vector (half the cross product: its normal times its area).
#[inline]
pub fn area_vector(a: V3, b: V3, c: V3) -> V3 {
    scale(cross(sub(b, a), sub(c, a)), 0.5)
}

/// Six times the signed volume of the tetrahedron (a, b, c, d): positive when d is on the side
/// of (a, b, c) its counter-clockwise normal points away from (the right-handed order).
#[inline]
pub fn vol6(a: V3, b: V3, c: V3, d: V3) -> f64 {
    dot(cross(sub(b, a), sub(c, a)), sub(d, a))
}

#[inline]
fn c3(p: V3) -> robust::Coord3D<f64> {
    robust::Coord3D { x: p[0], y: p[1], z: p[2] }
}

/// The exact orientation of (a, b, c, d), with the sign of [`vol6`] (robust's `orient3d` is
/// positive for the opposite order, so the arguments are swapped).
#[inline]
pub fn orient(a: V3, b: V3, c: V3, d: V3) -> f64 {
    robust::orient3d(c3(a), c3(c), c3(b), c3(d))
}

/// Exact: positive when `e` is strictly inside the sphere through a positively oriented
/// ([`orient`] > 0) tetrahedron (a, b, c, d), zero on it.
#[inline]
pub fn insphere(a: V3, b: V3, c: V3, d: V3, e: V3) -> f64 {
    // robust's insphere wants its own positive orientation, which is ours with b and c swapped.
    robust::insphere(c3(a), c3(c), c3(b), c3(d), c3(e))
}

/// Exact 2D orientation: positive when (a, b, c) turn counter-clockwise.
#[inline]
pub fn orient2(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    robust::orient2d(robust::Coord { x: a[0], y: a[1] }, robust::Coord { x: b[0], y: b[1] }, robust::Coord { x: c[0], y: c[1] })
}

/// The point of triangle (a, b, c) closest to `p` (Ericson, "Real-Time Collision Detection"
/// §5.1.5), with its barycentric weights.
pub fn closest_on_triangle(p: V3, a: V3, b: V3, c: V3) -> (V3, [f64; 3]) {
    let ab = sub(b, a);
    let ac = sub(c, a);
    let ap = sub(p, a);
    let d1 = dot(ab, ap);
    let d2 = dot(ac, ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, [1.0, 0.0, 0.0]);
    }
    let bp = sub(p, b);
    let d3 = dot(ab, bp);
    let d4 = dot(ac, bp);
    if d3 >= 0.0 && d4 <= d3 {
        return (b, [0.0, 1.0, 0.0]);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (add(a, scale(ab, v)), [1.0 - v, v, 0.0]);
    }
    let cp = sub(p, c);
    let d5 = dot(ab, cp);
    let d6 = dot(ac, cp);
    if d6 >= 0.0 && d5 <= d6 {
        return (c, [0.0, 0.0, 1.0]);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (add(a, scale(ac, w)), [1.0 - w, 0.0, w]);
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (add(b, scale(sub(c, b), w)), [0.0, 1.0 - w, w]);
    }
    let denom = 1.0 / (va + vb + vc);
    let v = vb * denom;
    let w = vc * denom;
    (add(a, add(scale(ab, v), scale(ac, w))), [1.0 - v - w, v, w])
}

/// A small deterministic random number generator (xorshift64*), so meshes are repeatable.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    /// Uniform in [0, 1).
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n.max(1) as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_signs_agree() {
        let (a, b, c, d) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        assert!(vol6(a, b, c, d) > 0.0);
        assert!(orient(a, b, c, d) > 0.0);
        assert!(orient(a, c, b, d) < 0.0);
        // The unit tetrahedron's circumsphere is centred at (½, ½, ½): the centroid is inside,
        // (1, 1, 1) is on it, (2, 2, 2) outside.
        assert!(insphere(a, b, c, d, [0.25, 0.25, 0.25]) > 0.0);
        assert_eq!(insphere(a, b, c, d, [1.0, 1.0, 1.0]), 0.0);
        assert!(insphere(a, b, c, d, [2.0, 2.0, 2.0]) < 0.0);
    }
}

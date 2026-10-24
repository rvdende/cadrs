//! Points, vectors, boxes and coordinate frames (mm, `f64`).
//!
//! A [`Frame`] is a right-handed orthonormal frame: a setup's work coordinate system or a work
//! plane. Toolpaths are computed and posted in a frame's local coordinates, where the tool
//! points down its −Z.

use serde::{Deserialize, Serialize};

pub type P2 = [f64; 2];
pub type P3 = [f64; 3];

pub fn add(a: P3, b: P3) -> P3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: P3, b: P3) -> P3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn scale(a: P3, s: f64) -> P3 {
    [a[0] * s, a[1] * s, a[2] * s]
}

pub fn dot(a: P3, b: P3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: P3, b: P3) -> P3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

pub fn len(a: P3) -> f64 {
    dot(a, a).sqrt()
}

pub fn dist(a: P3, b: P3) -> f64 {
    len(sub(a, b))
}

/// `a` scaled to unit length (`a` itself when it is zero).
pub fn normalize(a: P3) -> P3 {
    let l = len(a);
    if l > 1e-300 { scale(a, 1.0 / l) } else { a }
}

pub fn lerp(a: P3, b: P3, t: f64) -> P3 {
    add(a, scale(sub(b, a), t))
}

pub fn sub2(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

pub fn add2(a: P2, b: P2) -> P2 {
    [a[0] + b[0], a[1] + b[1]]
}

pub fn scale2(a: P2, s: f64) -> P2 {
    [a[0] * s, a[1] * s]
}

pub fn dot2(a: P2, b: P2) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}

/// The z of `a × b`.
pub fn cross2(a: P2, b: P2) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

pub fn len2(a: P2) -> f64 {
    dot2(a, a).sqrt()
}

pub fn dist2(a: P2, b: P2) -> f64 {
    len2(sub2(a, b))
}

pub fn lerp2(a: P2, b: P2, t: f64) -> P2 {
    add2(a, scale2(sub2(b, a), t))
}

pub fn xy(p: P3) -> P2 {
    [p[0], p[1]]
}

pub fn at_z(p: P2, z: f64) -> P3 {
    [p[0], p[1], z]
}

/// The distance from `p` to the segment `a`–`b`.
pub fn dist_to_segment2(p: P2, a: P2, b: P2) -> f64 {
    let d = sub2(b, a);
    let l2 = dot2(d, d);
    let t = if l2 > 0.0 {
        (dot2(sub2(p, a), d) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist2(p, add2(a, scale2(d, t)))
}

/// The length of an open polyline.
pub fn polyline_len(pts: &[P3]) -> f64 {
    pts.windows(2).map(|w| dist(w[0], w[1])).sum()
}

/// The length of a closed 2D ring (its closing segment included).
pub fn ring_len(ring: &[P2]) -> f64 {
    if ring.len() < 2 {
        return 0.0;
    }
    let open: f64 = ring.windows(2).map(|w| dist2(w[0], w[1])).sum();
    open + dist2(ring[ring.len() - 1], ring[0])
}

/// Twice the signed area of a ring: positive when it runs counter-clockwise.
pub fn ring_area2(ring: &[P2]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| cross2(ring[i], ring[(i + 1) % n]))
        .sum::<f64>()
}

/// Whether `p` is inside the ring (even-odd).
pub fn ring_contains(ring: &[P2], p: P2) -> bool {
    let n = ring.len();
    let mut inside = false;
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (ring[i], ring[j]);
        if (a[1] > p[1]) != (b[1] > p[1]) {
            let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
            if p[0] < x {
                inside = !inside;
            }
        }
        j = i;
    }
    inside
}

/// An axis-aligned box. [`Aabb::EMPTY`] contains nothing.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: P3,
    pub max: P3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };

    pub fn new(min: P3, max: P3) -> Self {
        Aabb { min, max }
    }

    pub fn of(points: impl IntoIterator<Item = P3>) -> Self {
        let mut b = Aabb::EMPTY;
        for p in points {
            b.include(p);
        }
        b
    }

    pub fn is_empty(&self) -> bool {
        (0..3).any(|i| self.min[i] > self.max[i])
    }

    pub fn include(&mut self, p: P3) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    pub fn union(&self, o: &Aabb) -> Aabb {
        let mut b = *self;
        if !o.is_empty() {
            b.include(o.min);
            b.include(o.max);
        }
        b
    }

    pub fn size(&self) -> P3 {
        if self.is_empty() {
            [0.0; 3]
        } else {
            sub(self.max, self.min)
        }
    }

    pub fn center(&self) -> P3 {
        scale(add(self.min, self.max), 0.5)
    }

    /// Grown by `d` on every side.
    pub fn grown(&self, d: f64) -> Aabb {
        Aabb {
            min: [self.min[0] - d, self.min[1] - d, self.min[2] - d],
            max: [self.max[0] + d, self.max[1] + d, self.max[2] + d],
        }
    }

    pub fn contains(&self, p: P3) -> bool {
        (0..3).all(|i| p[i] >= self.min[i] && p[i] <= self.max[i])
    }

    /// Its eight corners.
    pub fn corners(&self) -> [P3; 8] {
        let (a, b) = (self.min, self.max);
        [
            [a[0], a[1], a[2]],
            [b[0], a[1], a[2]],
            [b[0], b[1], a[2]],
            [a[0], b[1], a[2]],
            [a[0], a[1], b[2]],
            [b[0], a[1], b[2]],
            [b[0], b[1], b[2]],
            [a[0], b[1], b[2]],
        ]
    }
}

/// A right-handed orthonormal frame: an origin and its X, Y and Z axes in world coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub origin: P3,
    pub x: P3,
    pub y: P3,
    pub z: P3,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::WORLD
    }
}

impl Frame {
    pub const WORLD: Frame = Frame {
        origin: [0.0; 3],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        z: [0.0, 0.0, 1.0],
    };

    /// The frame at `origin` whose Z is `z` and whose X is `x` made perpendicular to it (any
    /// perpendicular when `x` is parallel to `z`).
    pub fn from_zx(origin: P3, z: P3, x: P3) -> Frame {
        let z = normalize(z);
        let mut x = sub(x, scale(z, dot(x, z)));
        if len(x) < 1e-9 {
            let helper = if z[0].abs() < 0.9 {
                [1.0, 0.0, 0.0]
            } else {
                [0.0, 1.0, 0.0]
            };
            x = sub(helper, scale(z, dot(helper, z)));
        }
        let x = normalize(x);
        Frame {
            origin,
            x,
            y: cross(z, x),
            z,
        }
    }

    /// World point → local coordinates.
    pub fn to_local(&self, p: P3) -> P3 {
        let d = sub(p, self.origin);
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }

    /// Local coordinates → world point.
    pub fn to_world(&self, p: P3) -> P3 {
        add(
            self.origin,
            add(
                add(scale(self.x, p[0]), scale(self.y, p[1])),
                scale(self.z, p[2]),
            ),
        )
    }

    /// World direction → local.
    pub fn dir_to_local(&self, d: P3) -> P3 {
        [dot(d, self.x), dot(d, self.y), dot(d, self.z)]
    }

    pub fn dir_to_world(&self, d: P3) -> P3 {
        add(
            add(scale(self.x, d[0]), scale(self.y, d[1])),
            scale(self.z, d[2]),
        )
    }

    /// `other` expressed in this frame's coordinates.
    pub fn relative(&self, other: &Frame) -> Frame {
        Frame {
            origin: self.to_local(other.origin),
            x: self.dir_to_local(other.x),
            y: self.dir_to_local(other.y),
            z: self.dir_to_local(other.z),
        }
    }

    /// True when the two frames' Z axes point the same way (a 3-axis machine can cut in both
    /// without re-fixturing).
    pub fn same_z(&self, other: &Frame) -> bool {
        dot(self.z, other.z) > 1.0 - 1e-9
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_round_trip() {
        let f = Frame::from_zx([10.0, 5.0, -2.0], [0.0, 1.0, 1.0], [1.0, 0.0, 0.0]);
        let p = [3.0, -4.0, 7.5];
        let q = f.to_world(f.to_local(p));
        assert!(dist(p, q) < 1e-12);
        assert!((len(f.y) - 1.0).abs() < 1e-12);
        assert!(dot(cross(f.x, f.y), f.z) > 0.999);
    }

    #[test]
    fn frame_from_parallel_x_still_orthonormal() {
        let f = Frame::from_zx([0.0; 3], [1.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
        assert!(dot(f.x, f.z).abs() < 1e-12);
        assert!((len(f.x) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn ring_area_and_contains() {
        let sq = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert_eq!(ring_area2(&sq), 200.0);
        assert!(ring_contains(&sq, [5.0, 5.0]));
        assert!(!ring_contains(&sq, [15.0, 5.0]));
        assert_eq!(ring_len(&sq), 40.0);
    }

    #[test]
    fn aabb_basics() {
        let b = Aabb::of([[0.0, 0.0, 0.0], [2.0, 4.0, 6.0]]);
        assert_eq!(b.center(), [1.0, 2.0, 3.0]);
        assert!(Aabb::EMPTY.is_empty());
        assert_eq!(Aabb::EMPTY.union(&b), b);
    }
}

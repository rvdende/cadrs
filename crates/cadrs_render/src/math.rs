//! A small 3-vector and the random numbers of the path tracer.

use std::ops::{Add, AddAssign, Div, Mul, MulAssign, Neg, Sub};

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

pub const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    pub const ZERO: V3 = v3(0.0, 0.0, 0.0);
    pub const ONE: V3 = v3(1.0, 1.0, 1.0);
    pub const Z: V3 = v3(0.0, 0.0, 1.0);

    pub fn splat(v: f32) -> V3 {
        v3(v, v, v)
    }
    pub fn from_array(a: [f32; 3]) -> V3 {
        v3(a[0], a[1], a[2])
    }
    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }
    pub fn dot(self, o: V3) -> f32 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn length(self) -> f32 {
        self.dot(self).sqrt()
    }
    pub fn normalize(self) -> V3 {
        let l = self.length();
        if l > 0.0 { self / l } else { V3::Z }
    }
    pub fn min(self, o: V3) -> V3 {
        v3(self.x.min(o.x), self.y.min(o.y), self.z.min(o.z))
    }
    pub fn max(self, o: V3) -> V3 {
        v3(self.x.max(o.x), self.y.max(o.y), self.z.max(o.z))
    }
    pub fn mul_v(self, o: V3) -> V3 {
        v3(self.x * o.x, self.y * o.y, self.z * o.z)
    }
    pub fn axis(self, i: usize) -> f32 {
        match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
    pub fn max_elem(self) -> f32 {
        self.x.max(self.y).max(self.z)
    }
    /// Rec. 709 luminance.
    pub fn luminance(self) -> f32 {
        0.2126 * self.x + 0.7152 * self.y + 0.0722 * self.z
    }
    pub fn lerp(self, o: V3, t: f32) -> V3 {
        self + (o - self) * t
    }
    /// Two unit vectors completing an orthonormal frame with this unit vector (Duff et al.).
    pub fn basis(self) -> (V3, V3) {
        let s = if self.z >= 0.0 { 1.0 } else { -1.0 };
        let a = -1.0 / (s + self.z);
        let b = self.x * self.y * a;
        (v3(1.0 + s * self.x * self.x * a, s * b, -s * self.x), v3(b, s + self.y * self.y * a, -self.y))
    }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Mul<f32> for V3 {
    type Output = V3;
    fn mul(self, s: f32) -> V3 {
        v3(self.x * s, self.y * s, self.z * s)
    }
}
impl MulAssign<f32> for V3 {
    fn mul_assign(&mut self, s: f32) {
        *self = *self * s;
    }
}
impl Div<f32> for V3 {
    type Output = V3;
    fn div(self, s: f32) -> V3 {
        v3(self.x / s, self.y / s, self.z / s)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}

/// The random numbers of one path: PCG32 seeded from the render's seed, the pixel and the
/// sample, so every pixel's samples are the same whatever thread renders them.
pub struct Rng {
    state: u64,
    inc: u64,
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl Rng {
    pub fn new(seed: u64, pixel: u64, sample: u64) -> Self {
        let a = mix(seed ^ mix(pixel.wrapping_mul(0x2545_F491_4F6C_DD1D) ^ mix(sample)));
        let mut r = Rng { state: 0, inc: (mix(a ^ 0xDA3E_39CB_94B9_5BDB) << 1) | 1 };
        r.next_u32();
        r.state = r.state.wrapping_add(a);
        r.next_u32();
        r
    }

    pub fn next_u32(&mut self) -> u32 {
        let old = self.state;
        self.state = old.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(self.inc);
        let xorshifted = (((old >> 18) ^ old) >> 27) as u32;
        let rot = (old >> 59) as u32;
        xorshifted.rotate_right(rot)
    }

    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }
}

/// sRGB (0–1) to linear.
pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

/// Linear to sRGB (0–1).
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

/// An sRGB byte colour as linear.
pub fn rgb8(r: u8, g: u8, b: u8) -> V3 {
    v3(srgb_to_linear(r as f32 / 255.0), srgb_to_linear(g as f32 / 255.0), srgb_to_linear(b as f32 / 255.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basis_is_orthonormal() {
        for n in [v3(0.0, 0.0, 1.0), v3(0.0, 0.0, -1.0), v3(1.0, 2.0, -3.0).normalize()] {
            let (a, b) = n.basis();
            assert!((a.length() - 1.0).abs() < 1e-5 && (b.length() - 1.0).abs() < 1e-5);
            assert!(a.dot(b).abs() < 1e-5 && a.dot(n).abs() < 1e-5 && b.dot(n).abs() < 1e-5);
        }
    }

    #[test]
    fn rng_repeats_and_spreads() {
        let mut a = Rng::new(7, 100, 3);
        let mut b = Rng::new(7, 100, 3);
        let xs: Vec<f32> = (0..1000).map(|_| a.f()).collect();
        let ys: Vec<f32> = (0..1000).map(|_| b.f()).collect();
        assert_eq!(xs, ys);
        let mean = xs.iter().sum::<f32>() / 1000.0;
        assert!((mean - 0.5).abs() < 0.05, "{mean}");
        let mut c = Rng::new(7, 101, 3);
        assert_ne!(c.f(), xs[0]);
    }
}

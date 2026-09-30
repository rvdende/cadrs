//! The surface model: a Lambert base under a GGX (Trowbridge–Reitz) specular lobe with Smith
//! masking and Schlick's Fresnel, the metallic/roughness model glTF and Bevy use. A metal has
//! no diffuse part and a coloured specular one; a dielectric reflects 4 % at normal incidence.

use std::f32::consts::PI;

use crate::math::{Rng, V3};

/// A part's surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Material {
    /// Linear RGB (not sRGB).
    pub base_color: [f32; 3],
    /// 0 (dielectric) to 1 (metal).
    pub metallic: f32,
    /// Perceptual roughness, 0 (mirror) to 1.
    pub roughness: f32,
    /// 1 opaque; less lets light pass straight through that often (a see-through appearance).
    pub opacity: f32,
}

impl Default for Material {
    fn default() -> Self {
        Material { base_color: [0.6, 0.6, 0.6], metallic: 0.0, roughness: 0.5, opacity: 1.0 }
    }
}

impl Material {
    /// From an sRGB colour.
    pub fn from_srgb(rgb: [u8; 3], metallic: f32, roughness: f32, opacity: f32) -> Self {
        let c = crate::math::rgb8(rgb[0], rgb[1], rgb[2]);
        Material { base_color: c.to_array(), metallic, roughness, opacity }
    }
}

/// A material prepared for shading.
pub struct Bsdf {
    diffuse: V3,
    f0: V3,
    alpha: f32,
    /// The chance of sampling the specular lobe.
    p_spec: f32,
}

fn schlick(f0: V3, cos: f32) -> V3 {
    let m = (1.0 - cos).clamp(0.0, 1.0);
    let m5 = m * m * m * m * m;
    f0 + (V3::ONE - f0) * m5
}

impl Bsdf {
    pub fn new(m: &Material, n_dot_o: f32) -> Self {
        let base = V3::from_array(m.base_color);
        let metallic = m.metallic.clamp(0.0, 1.0);
        let f0 = V3::splat(0.04).lerp(base, metallic);
        let diffuse = base * (1.0 - metallic);
        let r = m.roughness.clamp(0.045, 1.0);
        let alpha = r * r;
        let ws = schlick(f0, n_dot_o).luminance();
        let wd = diffuse.luminance();
        let p_spec = if wd <= 1e-6 { 1.0 } else { (ws / (ws + wd)).clamp(0.15, 0.9) };
        Bsdf { diffuse, f0, alpha, p_spec }
    }

    fn d(&self, n_dot_h: f32) -> f32 {
        let a2 = self.alpha * self.alpha;
        let t = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
        a2 / (PI * t * t)
    }

    fn g1(&self, n_dot_v: f32) -> f32 {
        let a2 = self.alpha * self.alpha;
        2.0 * n_dot_v / (n_dot_v + (a2 + (1.0 - a2) * n_dot_v * n_dot_v).sqrt())
    }

    /// f(wo, wi) for unit vectors about the unit normal `n` (0 below the surface).
    pub fn eval(&self, n: V3, wo: V3, wi: V3) -> V3 {
        let (no, ni) = (n.dot(wo), n.dot(wi));
        if no <= 0.0 || ni <= 0.0 {
            return V3::ZERO;
        }
        let h = (wo + wi).normalize();
        let nh = n.dot(h).max(0.0);
        let f = schlick(self.f0, wo.dot(h).max(0.0));
        let spec = f * (self.d(nh) * self.g1(no) * self.g1(ni) / (4.0 * no * ni));
        let diff = (V3::ONE - f).mul_v(self.diffuse) * (1.0 / PI);
        diff + spec
    }

    /// The density `sample` picks `wi` with (solid angle).
    pub fn pdf(&self, n: V3, wo: V3, wi: V3) -> f32 {
        let ni = n.dot(wi);
        if ni <= 0.0 || n.dot(wo) <= 0.0 {
            return 0.0;
        }
        let h = (wo + wi).normalize();
        let spec = self.d(n.dot(h).max(0.0)) * n.dot(h).max(0.0) / (4.0 * wo.dot(h).abs().max(1e-6));
        let diff = ni / PI;
        self.p_spec * spec + (1.0 - self.p_spec) * diff
    }

    /// A direction `wi` drawn from the lobes, with f(wo, wi) and its density.
    pub fn sample(&self, n: V3, wo: V3, rng: &mut Rng) -> Option<(V3, V3, f32)> {
        let (t, b) = n.basis();
        let (u1, u2) = (rng.f(), rng.f());
        let wi = if rng.f() < self.p_spec {
            // A half vector from D(h)·cos θh.
            let a2 = self.alpha * self.alpha;
            let cos_t = ((1.0 - u1) / (1.0 + (a2 - 1.0) * u1)).max(0.0).sqrt();
            let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
            let phi = 2.0 * PI * u2;
            let h = t * (sin_t * phi.cos()) + b * (sin_t * phi.sin()) + n * cos_t;
            let wi = h * (2.0 * wo.dot(h)) - wo;
            wi.normalize()
        } else {
            // Cosine-weighted.
            let r = u1.sqrt();
            let phi = 2.0 * PI * u2;
            (t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt()).normalize()
        };
        let pdf = self.pdf(n, wo, wi);
        if pdf <= 1e-8 {
            return None;
        }
        let f = self.eval(n, wo, wi);
        Some((wi, f, pdf))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v3;

    /// ∫ f cos dω ≤ 1 (no energy made) and the sampler's estimate agrees with a uniform one.
    #[test]
    fn conserves_energy_and_samples_consistently() {
        let n = V3::Z;
        for m in [
            Material { base_color: [0.8, 0.8, 0.8], metallic: 0.0, roughness: 0.5, opacity: 1.0 },
            Material { base_color: [0.9, 0.9, 0.9], metallic: 1.0, roughness: 0.3, opacity: 1.0 },
        ] {
            let wo = v3(0.3, 0.0, 1.0).normalize();
            let bsdf = Bsdf::new(&m, wo.z);
            let mut rng = Rng::new(3, 4, 5);
            let n_samples = 200_000;
            let mut importance = 0.0;
            for _ in 0..n_samples {
                if let Some((wi, f, pdf)) = bsdf.sample(n, wo, &mut rng) {
                    importance += f.luminance() * wi.z.max(0.0) / pdf;
                }
            }
            importance /= n_samples as f32;
            let mut uniform = 0.0;
            for _ in 0..n_samples {
                let z = rng.f();
                let r = (1.0 - z * z).max(0.0).sqrt();
                let phi = 2.0 * PI * rng.f();
                let wi = v3(r * phi.cos(), r * phi.sin(), z);
                uniform += bsdf.eval(n, wo, wi).luminance() * z * 2.0 * PI;
            }
            uniform /= n_samples as f32;
            assert!(importance <= 1.02, "{importance}");
            assert!((importance - uniform).abs() < 0.03, "{importance} vs {uniform}");
        }
    }
}

//! The lighting environments: procedural, so nothing has to be bundled or licensed. Each is a
//! smooth sky (reached by the paths' bounces) plus a few round area lights at infinity (a
//! studio's soft boxes, the sun), which are also sampled directly. Z is up.
//!
//! - **Studio**: a neutral grey dome, a large key soft box front left and above, a fill low on
//!   the right, a rim light behind and a soft box overhead; the backdrop is a light grey sweep.
//! - **Soft light**: an overcast dome, brighter overhead, no hard lights (soft contact
//!   shadows); a near-white backdrop.
//! - **Outdoor**: a blue sky, darker at the zenith, with the sun high on the left; a warm grey
//!   ground.
//! - **Sunset**: an orange sky with a low sun.

use crate::math::{V3, rgb8, v3};

/// The named environments, as the Render tab offers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum EnvironmentPreset {
    #[default]
    Studio,
    SoftLight,
    Outdoor,
    Sunset,
}

impl EnvironmentPreset {
    pub const ALL: [EnvironmentPreset; 4] =
        [EnvironmentPreset::Studio, EnvironmentPreset::SoftLight, EnvironmentPreset::Outdoor, EnvironmentPreset::Sunset];

    pub fn label(self) -> &'static str {
        match self {
            EnvironmentPreset::Studio => "Studio",
            EnvironmentPreset::SoftLight => "Soft light",
            EnvironmentPreset::Outdoor => "Outdoor",
            EnvironmentPreset::Sunset => "Sunset",
        }
    }
}

/// A round light at infinity: a disc of directions around `dir` of angular radius `radius`
/// (radians) with constant radiance.
#[derive(Debug, Clone, Copy)]
pub struct ConeLight {
    pub dir: V3,
    pub cos_max: f32,
    pub radiance: V3,
    /// The solid angle it covers.
    pub solid_angle: f32,
}

impl ConeLight {
    fn new(dir: V3, radius_deg: f32, radiance: V3) -> Self {
        let cos_max = radius_deg.to_radians().cos();
        ConeLight { dir: dir.normalize(), cos_max, radiance, solid_angle: 2.0 * std::f32::consts::PI * (1.0 - cos_max) }
    }

    pub fn contains(&self, d: V3) -> bool {
        d.dot(self.dir) >= self.cos_max
    }

    /// A direction uniformly inside the cone.
    pub fn sample(&self, u1: f32, u2: f32) -> V3 {
        let cos_t = 1.0 - u1 * (1.0 - self.cos_max);
        let sin_t = (1.0 - cos_t * cos_t).max(0.0).sqrt();
        let phi = 2.0 * std::f32::consts::PI * u2;
        let (a, b) = self.dir.basis();
        (a * (sin_t * phi.cos()) + b * (sin_t * phi.sin()) + self.dir * cos_t).normalize()
    }
}

/// Where a light is, by azimuth (degrees from −Y, the Front view's line of sight, toward +X)
/// and elevation above the horizon: the same angles as the 3D view's camera.
fn toward(azimuth: f32, elevation: f32) -> V3 {
    let (az, el) = (azimuth.to_radians(), elevation.to_radians());
    v3(az.sin() * el.cos(), -az.cos() * el.cos(), el.sin())
}

pub struct Environment {
    pub preset: EnvironmentPreset,
    pub lights: Vec<ConeLight>,
    /// The dome: zenith, horizon and ground radiance.
    zenith: V3,
    horizon: V3,
    ground: V3,
    /// The backdrop the camera sees (display colours, sRGB 0–1): centre and edge of a
    /// vignette for the studio looks, or the sky itself.
    backdrop: Backdrop,
}

enum Backdrop {
    /// A radial sweep from the image centre to its corners (sRGB).
    Sweep { center: V3, edge: V3 },
    /// The dome, tone mapped like the model.
    Sky,
}

impl Environment {
    /// The environment turned `rotation` degrees about Z (moves the lights round the model).
    pub fn new(preset: EnvironmentPreset, rotation: f32) -> Self {
        let l = |az: f32, el: f32, r: f32, rad: V3| ConeLight::new(toward(az + rotation, el), r, rad);
        match preset {
            EnvironmentPreset::Studio => Environment {
                preset,
                lights: vec![
                    l(-35.0, 50.0, 16.0, V3::splat(4.2)),
                    l(70.0, 18.0, 22.0, V3::splat(1.2)),
                    l(170.0, 40.0, 10.0, V3::splat(4.5)),
                    // An overhead soft box: metal tops catch it.
                    l(0.0, 84.0, 26.0, V3::splat(1.8)),
                ],
                zenith: V3::splat(0.45),
                horizon: V3::splat(0.40),
                ground: V3::splat(0.22),
                backdrop: Backdrop::Sweep { center: v3(0.965, 0.968, 0.975), edge: v3(0.80, 0.82, 0.85) },
            },
            EnvironmentPreset::SoftLight => Environment {
                preset,
                lights: Vec::new(),
                zenith: V3::splat(1.35),
                horizon: V3::splat(0.75),
                ground: V3::splat(0.28),
                backdrop: Backdrop::Sweep { center: v3(0.985, 0.985, 0.985), edge: v3(0.90, 0.90, 0.91) },
            },
            EnvironmentPreset::Outdoor => Environment {
                preset,
                lights: vec![l(-55.0, 55.0, 1.6, rgb8(255, 246, 228) * 1100.0)],
                zenith: rgb8(92, 140, 214) * 0.9,
                horizon: rgb8(200, 220, 240) * 1.1,
                ground: rgb8(176, 170, 160) * 0.9,
                backdrop: Backdrop::Sky,
            },
            EnvironmentPreset::Sunset => Environment {
                preset,
                lights: vec![l(-60.0, 12.0, 1.8, rgb8(255, 176, 100) * 900.0)],
                zenith: rgb8(90, 110, 170) * 1.1,
                horizon: rgb8(250, 170, 110) * 1.3,
                ground: rgb8(160, 130, 110) * 0.8,
                backdrop: Backdrop::Sky,
            },
        }
    }

    /// The dome's radiance in direction `d` (without the lights).
    pub fn dome(&self, d: V3) -> V3 {
        if d.z >= 0.0 {
            let t = d.z.sqrt();
            self.horizon.lerp(self.zenith, t)
        } else {
            // A short blend below the horizon, then the ground.
            let t = (-d.z * 8.0).min(1.0);
            self.horizon.lerp(self.ground, t)
        }
    }

    /// The radiance seen along `d`: the dome and any light containing it.
    pub fn radiance(&self, d: V3) -> V3 {
        let mut c = self.dome(d);
        for l in &self.lights {
            if l.contains(d) {
                c += l.radiance;
            }
        }
        c
    }

    /// The backdrop behind the model at image position `(sx, sy)` (0–1, top left) along the
    /// camera ray `d`, as a display colour (sRGB 0–1) if it is a flat backdrop, else `None`
    /// (the sky is seen and tone mapped like the model).
    pub fn backdrop(&self, sx: f32, sy: f32) -> Option<V3> {
        match self.backdrop {
            Backdrop::Sweep { center, edge } => {
                let dx = (sx - 0.5) * 1.3;
                let dy = sy - 0.42;
                let r = (dx * dx + dy * dy).sqrt().min(1.0);
                let t = r * r * (3.0 - 2.0 * r);
                Some(center.lerp(edge, t))
            }
            Backdrop::Sky => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cone_samples_stay_inside() {
        let l = ConeLight::new(v3(1.0, 2.0, 3.0), 10.0, V3::ONE);
        let mut rng = crate::math::Rng::new(0, 0, 0);
        for _ in 0..1000 {
            let d = l.sample(rng.f(), rng.f());
            assert!(l.contains(d) || d.dot(l.dir) > l.cos_max - 1e-4);
            assert!((d.length() - 1.0).abs() < 1e-4);
        }
    }

    #[test]
    fn light_directions_follow_the_view_angles() {
        // Azimuth 0, elevation 0 is the Front view's camera direction: toward −Y.
        let d = toward(0.0, 0.0);
        assert!((d.y + 1.0).abs() < 1e-6);
        let d = toward(90.0, 0.0);
        assert!((d.x - 1.0).abs() < 1e-6);
    }
}

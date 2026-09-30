//! Photorealistic rendering for cadrs (P3F.6, `intro-to-parametric-cad.md` P3.6): a small CPU
//! path tracer over the parts' tessellation. No Bevy, so it renders the same in tests, in
//! headless scenarios and on every machine.
//!
//! - **Scene**: triangles with smooth normals and a [`Material`] each (base colour, metallic,
//!   roughness, opacity), in a [`bvh::Bvh`].
//! - **Light**: a procedural [`env::Environment`] (a dome plus round area lights at infinity,
//!   sampled directly with multiple importance sampling against the surface's lobes).
//! - **Ground**: an infinite plane under the model that only catches shadows: its pixels show
//!   the backdrop darkened by how much of the light the model hides from them (the ratio of
//!   occluded to unoccluded light, accumulated separately so it converges without noise
//!   bias), or, on a transparent background, a black shadow of that opacity.
//! - **Accumulation**: [`Renderer::pass`] adds one sample to every pixel (on a thread pool);
//!   the image so far is [`Renderer::image`]. Every sample's random numbers come from (seed,
//!   pixel, sample index), so the result is the same whatever the thread count or scheduling:
//!   **a fixed seed gives identical bytes**.
//! - **Output**: the model's radiance is denoised (an edge-avoiding à-trous filter guided by
//!   the first hit's normal, albedo and position), exposed and tone mapped (Khronos PBR
//!   Neutral, which keeps appearance colours), then composited over the backdrop in sRGB.

pub mod bsdf;
pub mod bvh;
pub mod env;
pub mod math;

use std::sync::atomic::{AtomicBool, Ordering};

use image::{Rgba, RgbaImage};
use rayon::prelude::*;

pub use bsdf::Material;
pub use env::EnvironmentPreset;
use math::{Rng, V3, v3};

// ---------------------------------------------------------------------------------------------
// Scene

/// Triangles to render, gathered by [`SceneBuilder`].
pub struct Scene {
    normals: Vec<V3>,
    tris: Vec<[u32; 3]>,
    tri_material: Vec<u32>,
    face_normals: Vec<V3>,
    materials: Vec<Material>,
    bvh: bvh::Bvh,
}

#[derive(Default)]
pub struct SceneBuilder {
    positions: Vec<V3>,
    normals: Vec<V3>,
    tris: Vec<[u32; 3]>,
    tri_material: Vec<u32>,
    materials: Vec<Material>,
}

impl SceneBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a material; triangles refer to it by the returned index.
    pub fn material(&mut self, m: Material) -> u32 {
        if let Some(i) = self.materials.iter().position(|x| *x == m) {
            return i as u32;
        }
        self.materials.push(m);
        (self.materials.len() - 1) as u32
    }

    /// Adds a mesh: `normals` per position (or empty for flat shading), `indices` in threes,
    /// `material_of(k)` the material of triangle `k`.
    pub fn add_mesh(&mut self, positions: &[[f32; 3]], normals: &[[f32; 3]], indices: &[u32], material_of: impl Fn(usize) -> u32) {
        let base = self.positions.len() as u32;
        self.positions.extend(positions.iter().map(|p| V3::from_array(*p)));
        if normals.len() == positions.len() {
            self.normals.extend(normals.iter().map(|n| V3::from_array(*n).normalize()));
        } else {
            // No normals: marked zero, so the face normal is used.
            self.normals.extend(std::iter::repeat_n(V3::ZERO, positions.len()));
        }
        for (k, t) in indices.chunks_exact(3).enumerate() {
            if t.iter().any(|&i| i as usize >= positions.len()) {
                continue;
            }
            self.tris.push([base + t[0], base + t[1], base + t[2]]);
            self.tri_material.push(material_of(k));
        }
    }

    pub fn build(self) -> Scene {
        let corners: Vec<[V3; 3]> =
            self.tris.iter().map(|t| [self.positions[t[0] as usize], self.positions[t[1] as usize], self.positions[t[2] as usize]]).collect();
        let face_normals = corners.iter().map(|[a, b, c]| (*b - *a).cross(*c - *a).normalize()).collect();
        let bvh = bvh::Bvh::build(&corners);
        let mut materials = self.materials;
        if materials.is_empty() {
            materials.push(Material::default());
        }
        Scene { normals: self.normals, tris: self.tris, tri_material: self.tri_material, face_normals, materials, bvh }
    }
}

impl Scene {
    pub fn triangle_count(&self) -> usize {
        self.tris.len()
    }

    /// The bounds of the triangles (min, max); `None` when empty.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let b = self.bvh.bounds();
        (!b.is_empty()).then(|| (b.min.to_array(), b.max.to_array()))
    }
}

// ---------------------------------------------------------------------------------------------
// Camera

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Projection {
    /// A pinhole camera with this vertical field of view (degrees).
    Perspective { fov_y: f32 },
    /// Parallel rays; `height` is the view's height in model units.
    Orthographic { height: f32 },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub eye: [f32; 3],
    /// Unit vectors: toward the scene, screen right, screen up.
    pub forward: [f32; 3],
    pub right: [f32; 3],
    pub up: [f32; 3],
    pub projection: Projection,
}

impl Camera {
    /// A camera looking back along `back` (the unit vector from the model toward the camera)
    /// with `up` the screen's up, framing `bounds` with `margin` (a fraction of the image) on
    /// each side at `aspect` (width / height).
    pub fn fit(bounds: ([f32; 3], [f32; 3]), back: [f32; 3], up: [f32; 3], projection: Projection, aspect: f32, margin: f32) -> Camera {
        let back = V3::from_array(back).normalize();
        let right = V3::from_array(up).cross(back).normalize();
        let up = back.cross(right).normalize();
        let (lo, hi) = (V3::from_array(bounds.0), V3::from_array(bounds.1));
        let center = (lo + hi) * 0.5;
        let corners: Vec<V3> = (0..8)
            .map(|i| v3(if i & 1 == 0 { lo.x } else { hi.x }, if i & 2 == 0 { lo.y } else { hi.y }, if i & 4 == 0 { lo.z } else { hi.z }) - center)
            .collect();
        let radius = corners.iter().map(|c| c.length()).fold(0.0f32, f32::max).max(1e-3);
        let fill = (1.0 - 2.0 * margin).clamp(0.1, 1.0);
        match projection {
            Projection::Orthographic { .. } => {
                let (mut x, mut y) = (0.0f32, 0.0f32);
                for c in &corners {
                    x = x.max(c.dot(right).abs());
                    y = y.max(c.dot(up).abs());
                }
                let height = (2.0 * y).max(2.0 * x / aspect) / fill;
                let eye = center + back * (radius * 4.0);
                Camera { eye: eye.to_array(), forward: (-back).to_array(), right: right.to_array(), up: up.to_array(), projection: Projection::Orthographic { height } }
            }
            Projection::Perspective { fov_y } => {
                let ty = (fov_y.to_radians() / 2.0).tan() * fill;
                let tx = ty * aspect;
                // The nearest distance at which every corner is inside the frame.
                let mut d = radius * 1.05;
                for c in &corners {
                    let z = c.dot(back);
                    d = d.max(z + c.dot(right).abs() / tx).max(z + c.dot(up).abs() / ty);
                }
                let eye = center + back * d;
                Camera { eye: eye.to_array(), forward: (-back).to_array(), right: right.to_array(), up: up.to_array(), projection }
            }
        }
    }

    /// The ray through image point (`sx`, `sy`) in 0–1 from the top left.
    fn ray(&self, sx: f32, sy: f32, aspect: f32) -> (V3, V3) {
        let (eye, f, r, u) = (V3::from_array(self.eye), V3::from_array(self.forward), V3::from_array(self.right), V3::from_array(self.up));
        let (px, py) = (2.0 * sx - 1.0, 1.0 - 2.0 * sy);
        match self.projection {
            Projection::Perspective { fov_y } => {
                let t = (fov_y.to_radians() / 2.0).tan();
                (eye, (f + r * (px * t * aspect) + u * (py * t)).normalize())
            }
            Projection::Orthographic { height } => (eye + r * (px * height * aspect / 2.0) + u * (py * height / 2.0), f),
        }
    }

    /// The model-space size of one pixel at distance `t` along a ray.
    fn footprint(&self, t: f32, height_px: u32) -> f32 {
        match self.projection {
            Projection::Perspective { fov_y } => t * 2.0 * (fov_y.to_radians() / 2.0).tan() / height_px as f32,
            Projection::Orthographic { height } => height / height_px as f32,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Settings

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// The environment's backdrop (a studio sweep, or the sky).
    Environment,
    /// Plain white.
    White,
    /// Transparent (PNG alpha); the ground shadow stays as a translucent black.
    Transparent,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Settings {
    pub width: u32,
    pub height: u32,
    /// Samples per pixel.
    pub samples: u32,
    pub seed: u64,
    pub environment: EnvironmentPreset,
    /// Turns the environment's lights about Z (degrees).
    pub environment_rotation: f32,
    pub background: Background,
    /// The ground plane under the model catches shadows.
    pub ground_shadow: bool,
    /// Exposure in stops.
    pub exposure: f32,
    pub denoise: bool,
    /// Surface interactions per path.
    pub max_bounces: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            width: 1920,
            height: 1080,
            samples: 64,
            seed: 1,
            environment: EnvironmentPreset::Studio,
            environment_rotation: 0.0,
            background: Background::Environment,
            ground_shadow: true,
            exposure: 0.0,
            denoise: true,
            max_bounces: 5,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Rendering

/// One pixel's sums over its samples.
#[derive(Debug, Clone, Copy, Default)]
struct Accum {
    /// The model's radiance and how many samples hit it.
    model: [f32; 3],
    n_model: f32,
    /// A sky backdrop's radiance (tone mapped like the model).
    sky: [f32; 3],
    /// Samples on the ground plane and their light, occluded and not.
    n_ground: f32,
    occluded: f32,
    open: f32,
}

/// The first hit through the pixel's centre, which guides the denoiser.
#[derive(Debug, Clone, Copy, Default)]
struct Guide {
    normal: V3,
    albedo: V3,
    position: V3,
    footprint: f32,
    hit: bool,
}

enum SampleResult {
    Model { radiance: V3 },
    Ground { occluded: f32, open: f32, sky: V3 },
    Miss { sky: V3 },
}

/// A render in progress: its accumulated samples.
pub struct Renderer {
    scene: std::sync::Arc<Scene>,
    camera: Camera,
    settings: Settings,
    env: env::Environment,
    accum: Vec<Accum>,
    guide: Vec<Guide>,
    samples_done: u32,
    ground_z: f32,
    eps: f32,
    pool: rayon::ThreadPool,
}

/// The threads a render uses: half the machine's, at most eight (the app stays responsive).
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get() / 2).unwrap_or(2).clamp(1, 8)
}

impl Renderer {
    pub fn new(scene: std::sync::Arc<Scene>, camera: Camera, settings: Settings, threads: usize) -> Self {
        let n = (settings.width as usize) * (settings.height as usize);
        let (ground_z, eps) = match scene.bounds() {
            Some((lo, hi)) => {
                let size = (V3::from_array(hi) - V3::from_array(lo)).length().max(1e-3);
                (lo[2] - size * 1e-4, size * 2e-5)
            }
            None => (0.0, 1e-4),
        };
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads.max(1)).build().expect("render threads");
        Renderer {
            scene,
            camera,
            settings,
            env: env::Environment::new(settings.environment, settings.environment_rotation),
            accum: vec![Accum::default(); n],
            guide: vec![Guide::default(); n],
            samples_done: 0,
            ground_z,
            eps,
            pool,
        }
    }

    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    pub fn samples_done(&self) -> u32 {
        self.samples_done
    }

    pub fn is_done(&self) -> bool {
        self.samples_done >= self.settings.samples
    }

    /// Adds one sample to every pixel.
    pub fn pass(&mut self) {
        let s = self.samples_done;
        let w = self.settings.width as usize;
        let mut accum = std::mem::take(&mut self.accum);
        let mut guide = std::mem::take(&mut self.guide);
        {
            let this = &*self;
            this.pool.install(|| {
                accum.par_chunks_mut(w).zip(guide.par_chunks_mut(w)).enumerate().for_each(|(y, (acc, gd))| {
                    for x in 0..w {
                        let r = this.sample(x as u32, y as u32, s, (s == 0).then_some(&mut gd[x]));
                        let a = &mut acc[x];
                        match r {
                            SampleResult::Model { radiance } => {
                                a.model[0] += radiance.x;
                                a.model[1] += radiance.y;
                                a.model[2] += radiance.z;
                                a.n_model += 1.0;
                            }
                            SampleResult::Ground { occluded, open, sky } => {
                                a.n_ground += 1.0;
                                a.occluded += occluded;
                                a.open += open;
                                a.sky[0] += sky.x;
                                a.sky[1] += sky.y;
                                a.sky[2] += sky.z;
                            }
                            SampleResult::Miss { sky } => {
                                a.sky[0] += sky.x;
                                a.sky[1] += sky.y;
                                a.sky[2] += sky.z;
                            }
                        }
                    }
                });
            });
        }
        self.accum = accum;
        self.guide = guide;
        self.samples_done += 1;
    }

    fn sample(&self, x: u32, y: u32, s: u32, guide: Option<&mut Guide>) -> SampleResult {
        let (w, h) = (self.settings.width, self.settings.height);
        let mut rng = Rng::new(self.settings.seed, (y as u64) * (w as u64) + x as u64, s as u64);
        // The first sample goes through the pixel's centre (the denoiser's guides); the others
        // are jittered over it.
        let (jx, jy) = if s == 0 { (0.5, 0.5) } else { (rng.f(), rng.f()) };
        let aspect = w as f32 / h as f32;
        let (o, d) = self.camera.ray((x as f32 + jx) / w as f32, (y as f32 + jy) / h as f32, aspect);
        let hit = self.scene.bvh.intersect(o, d, f32::MAX);
        let ground_t = if self.settings.ground_shadow && d.z < -1e-6 {
            let t = (self.ground_z - o.z) / d.z;
            (t > 0.0).then_some(t)
        } else {
            None
        };
        let sky = || if self.env.backdrop(0.5, 0.5).is_none() { self.env.dome(d) } else { V3::ZERO };
        match (hit, ground_t) {
            (Some(hit), g) if g.is_none_or(|g| hit.t < g) => {
                let radiance = self.trace(o, d, hit, &mut rng, guide);
                SampleResult::Model { radiance }
            }
            (_, Some(t)) => {
                let p = o + d * t;
                let (occluded, open) = self.ground_light(p, &mut rng);
                SampleResult::Ground { occluded, open, sky: sky() }
            }
            _ => SampleResult::Miss { sky: sky() },
        }
    }

    /// The light reaching the ground at `p` with and without the model in the way (one dome
    /// sample and one light sample, as luminance).
    fn ground_light(&self, p: V3, rng: &mut Rng) -> (f32, f32) {
        let n = V3::Z;
        let o = p + n * self.eps;
        let (mut occ, mut open) = (0.0, 0.0);
        let (t, b) = n.basis();
        // A few dome samples (only visibility rays, so cheap): the dome's shadow is broad.
        const DOME: usize = 4;
        for _ in 0..DOME {
            let (u1, u2) = (rng.f(), rng.f());
            let r = u1.sqrt();
            let phi = 2.0 * std::f32::consts::PI * u2;
            let d = t * (r * phi.cos()) + b * (r * phi.sin()) + n * (1.0 - u1).max(0.0).sqrt();
            // cos / pdf = π for cosine sampling.
            let l = self.env.dome(d).luminance() * std::f32::consts::PI / DOME as f32;
            open += l;
            if !self.scene.bvh.occluded(o, d, f32::MAX) {
                occ += l;
            }
        }
        let lights = &self.env.lights;
        if !lights.is_empty() {
            let k = ((rng.f() * lights.len() as f32) as usize).min(lights.len() - 1);
            let light = &lights[k];
            let wi = light.sample(rng.f(), rng.f());
            if wi.z > 0.0 {
                let c = light.radiance.luminance() * wi.z * light.solid_angle * lights.len() as f32;
                open += c;
                if !self.scene.bvh.occluded(o, wi, f32::MAX) {
                    occ += c;
                }
            }
        }
        (occ, open)
    }

    /// A path from the first hit: next-event estimation of the lights with multiple importance
    /// sampling, the dome reached by the surface lobes, Russian roulette after three bounces.
    fn trace(&self, mut o: V3, mut d: V3, first: bvh::Hit, rng: &mut Rng, mut guide: Option<&mut Guide>) -> V3 {
        let scene = &*self.scene;
        let lights = &self.env.lights;
        let n_lights = lights.len() as f32;
        let mut radiance = V3::ZERO;
        let mut throughput = V3::ONE;
        let mut hit = Some(first);
        let mut last_pdf = 0.0f32;
        let mut specular_bounce = true;
        let mut bounce = 0;
        let mut passes = 0;
        while bounce < self.settings.max_bounces {
            let Some(h) = hit else {
                // Out to the environment.
                let mut c = self.env.dome(d);
                for l in lights {
                    if l.contains(d) {
                        let w = if specular_bounce {
                            1.0
                        } else {
                            let pl = 1.0 / (n_lights * l.solid_angle);
                            last_pdf * last_pdf / (last_pdf * last_pdf + pl * pl)
                        };
                        c += l.radiance * w;
                    }
                }
                radiance += throughput.mul_v(c);
                break;
            };
            let tri = h.tri as usize;
            let [ia, ib, ic] = scene.tris[tri];
            let p = o + d * h.t;
            let mut ng = scene.face_normals[tri];
            let (na, nb, nc) = (scene.normals[ia as usize], scene.normals[ib as usize], scene.normals[ic as usize]);
            let mut ns = if na == V3::ZERO || nb == V3::ZERO || nc == V3::ZERO {
                ng
            } else {
                (na * (1.0 - h.u - h.v) + nb * h.u + nc * h.v).normalize()
            };
            // Both sides shade alike (open surfaces, and inward faces seen through a gap).
            if ng.dot(d) > 0.0 {
                ng = -ng;
            }
            if ns.dot(ng) < 0.0 {
                ns = -ns;
            }
            let wo = -d;
            // Keep the shading normal on the viewer's side (flat-shaded rims seen edge-on).
            if ns.dot(wo) <= 0.0 {
                ns = (ns + ng * (0.01 - ns.dot(wo)) * 2.0).normalize();
                if ns.dot(wo) <= 0.0 {
                    ns = ng;
                }
            }
            let m = &scene.materials[scene.tri_material[tri] as usize];
            if m.opacity < 1.0 && rng.f() >= m.opacity && passes < 8 {
                // Straight through a see-through surface.
                passes += 1;
                o = p + d * self.eps;
                hit = scene.bvh.intersect(o, d, f32::MAX);
                continue;
            }
            if let Some(g) = guide.take() {
                *g = Guide { normal: ns, albedo: V3::from_array(m.base_color), position: p, footprint: self.camera.footprint(h.t, self.settings.height), hit: true };
            }
            let bsdf = bsdf::Bsdf::new(m, ns.dot(wo));
            let origin = p + ng * self.eps;
            // Next-event estimation: one light, chosen uniformly.
            if !lights.is_empty() {
                let k = ((rng.f() * n_lights) as usize).min(lights.len() - 1);
                let l = &lights[k];
                let wi = l.sample(rng.f(), rng.f());
                if wi.dot(ng) > 0.0 && wi.dot(ns) > 0.0 {
                    let f = bsdf.eval(ns, wo, wi);
                    if f.max_elem() > 0.0 && !scene.bvh.occluded(origin, wi, f32::MAX) {
                        let pl = 1.0 / (n_lights * l.solid_angle);
                        let pb = bsdf.pdf(ns, wo, wi);
                        let w = pl * pl / (pl * pl + pb * pb);
                        radiance += throughput.mul_v(f.mul_v(l.radiance)) * (wi.dot(ns) * w / pl);
                    }
                }
            }
            let Some((wi, f, pdf)) = bsdf.sample(ns, wo, rng) else {
                break;
            };
            if wi.dot(ng) <= 0.0 {
                break;
            }
            throughput = throughput.mul_v(f) * (wi.dot(ns) / pdf);
            last_pdf = pdf;
            specular_bounce = false;
            bounce += 1;
            if bounce >= 3 {
                let q = throughput.max_elem().clamp(0.05, 0.95);
                if rng.f() >= q {
                    break;
                }
                throughput *= 1.0 / q;
            }
            o = origin;
            d = wi;
            hit = scene.bvh.intersect(o, d, f32::MAX);
        }
        // Fireflies (a rare path onto a small bright light) are clamped.
        let m = radiance.max_elem();
        if m > 24.0 { radiance * (24.0 / m) } else { radiance }
    }

    /// The image so far, denoised if asked, as straight-alpha sRGB.
    pub fn image(&self) -> RgbaImage {
        self.compose(self.settings.denoise)
    }

    /// The image so far without denoising (cheap: for showing progress).
    pub fn image_noisy(&self) -> RgbaImage {
        self.compose(false)
    }

    fn compose(&self, denoise: bool) -> RgbaImage {
        let (w, h) = (self.settings.width as usize, self.settings.height as usize);
        let n = self.samples_done.max(1) as f32;
        let exposure = 2f32.powf(self.settings.exposure);
        // The model's mean radiance where it was hit.
        let mut model: Vec<V3> = self
            .accum
            .iter()
            .map(|a| if a.n_model > 0.0 { v3(a.model[0], a.model[1], a.model[2]) / a.n_model } else { V3::ZERO })
            .collect();
        if denoise && self.samples_done > 1 {
            model = self.denoise(&model);
        }
        let shadows = self.shadows();
        let mut out = RgbaImage::new(w as u32, h as u32);
        let background = self.settings.background;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let a = &self.accum[i];
                let coverage = a.n_model / n;
                let m = tonemap(model[i] * exposure);
                let rest = 1.0 - coverage;
                // The shadow: how much of the ground's light the model takes, over the pixel's
                // background samples.
                let bg_samples = (n - a.n_model).max(0.0);
                let shadow = if a.n_ground > 0.0 && bg_samples > 0.0 { shadows[i] * 0.85 * (a.n_ground / bg_samples) } else { 0.0 };
                let (sx, sy) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let (rgb, alpha) = match background {
                    Background::Transparent => {
                        // Premultiplied: the model over a black shadow of opacity `shadow`.
                        let alpha = coverage + rest * shadow;
                        let c = if alpha > 0.0 { m * (coverage / alpha) } else { V3::ZERO };
                        (c, alpha)
                    }
                    _ => {
                        let back = match (background, self.env.backdrop(sx, sy)) {
                            (Background::White, _) => V3::ONE,
                            (_, Some(c)) => c,
                            (_, None) => {
                                let s = if bg_samples > 0.0 { v3(a.sky[0], a.sky[1], a.sky[2]) / bg_samples } else { V3::ZERO };
                                tonemap(s * exposure)
                            }
                        };
                        (m * coverage + back * (rest * (1.0 - shadow)), 1.0)
                    }
                };
                let px = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u8;
                out.put_pixel(x as u32, y as u32, Rgba([px(rgb.x), px(rgb.y), px(rgb.z), px(alpha)]));
            }
        }
        out
    }

    /// How much light the model takes from each ground pixel (0–1), smoothed over the ground
    /// (three à-trous steps over the ground's pixels only): the shadow is smooth, its samples
    /// are not.
    fn shadows(&self) -> Vec<f32> {
        let (w, h) = (self.settings.width as usize, self.settings.height as usize);
        let ground: Vec<bool> = self.accum.iter().map(|a| a.n_ground > 0.0 && a.open > 0.0).collect();
        let mut cur: Vec<f32> = self.accum.iter().map(|a| if a.open > 0.0 { (1.0 - a.occluded / a.open).clamp(0.0, 1.0) } else { 0.0 }).collect();
        if self.samples_done <= 1 {
            return cur;
        }
        let kernel = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];
        let steps: &[usize] = if self.samples_done >= 256 { &[1] } else { &[1, 2, 4] };
        for &step in steps {
            let src = &cur;
            let ground = &ground;
            cur = self.pool.install(|| {
                (0..h)
                    .into_par_iter()
                    .flat_map_iter(|y| {
                        (0..w).map(move |x| {
                            let i = y * w + x;
                            if !ground[i] {
                                return src[i];
                            }
                            let (mut sum, mut wsum) = (0.0, 0.0);
                            for (ky, kyw) in kernel.iter().enumerate() {
                                let yy = y as isize + (ky as isize - 2) * step as isize;
                                if yy < 0 || yy >= h as isize {
                                    continue;
                                }
                                for (kx, kxw) in kernel.iter().enumerate() {
                                    let xx = x as isize + (kx as isize - 2) * step as isize;
                                    if xx < 0 || xx >= w as isize {
                                        continue;
                                    }
                                    let j = yy as usize * w + xx as usize;
                                    if ground[j] {
                                        sum += src[j] * kxw * kyw;
                                        wsum += kxw * kyw;
                                    }
                                }
                            }
                            if wsum > 0.0 { sum / wsum } else { src[i] }
                        })
                    })
                    .collect()
            });
        }
        cur
    }

    /// An edge-avoiding à-trous filter (Dammertz et al. 2010) over the model's radiance divided
    /// by its albedo, guided by the first hit's normal and position and the luminance.
    fn denoise(&self, input: &[V3]) -> Vec<V3> {
        let (w, h) = (self.settings.width as usize, self.settings.height as usize);
        let guide = &self.guide;
        let albedo = |i: usize| guide[i].albedo.max(V3::splat(0.02));
        let mut cur: Vec<V3> = input.iter().enumerate().map(|(i, c)| if guide[i].hit { v3(c.x / albedo(i).x, c.y / albedo(i).y, c.z / albedo(i).z) } else { *c }).collect();
        // Stronger with fewer samples.
        let noise = (1.0 / (self.samples_done as f32).sqrt()).clamp(0.02, 0.7);
        let kernel = [1.0 / 16.0, 1.0 / 4.0, 3.0 / 8.0, 1.0 / 4.0, 1.0 / 16.0];
        for (iteration, step) in [1usize, 2, 4, 8].into_iter().enumerate() {
            let sigma_l = noise * 4.0 / (1u32 << iteration) as f32;
            let src = &cur;
            let next: Vec<V3> = self.pool.install(|| {
                (0..h)
                    .into_par_iter()
                    .flat_map_iter(|y| {
                        (0..w).map(move |x| {
                            let i = y * w + x;
                            let g = &guide[i];
                            if !g.hit {
                                return src[i];
                            }
                            let lp = src[i].luminance();
                            let mut sum = V3::ZERO;
                            let mut wsum = 0.0;
                            for (ky, kyw) in kernel.iter().enumerate() {
                                let yy = y as isize + (ky as isize - 2) * step as isize;
                                if yy < 0 || yy >= h as isize {
                                    continue;
                                }
                                for (kx, kxw) in kernel.iter().enumerate() {
                                    let xx = x as isize + (kx as isize - 2) * step as isize;
                                    if xx < 0 || xx >= w as isize {
                                        continue;
                                    }
                                    let j = yy as usize * w + xx as usize;
                                    let q = &guide[j];
                                    if !q.hit {
                                        continue;
                                    }
                                    let wn = g.normal.dot(q.normal).max(0.0).powi(64);
                                    let plane = g.normal.dot(q.position - g.position).abs() / (g.footprint * step as f32 * 1.5 + 1e-6);
                                    let wp = (-plane).exp();
                                    let da = g.albedo - q.albedo;
                                    let wa = (-da.dot(da) * 200.0).exp();
                                    let dl = (src[j].luminance() - lp).abs() / (sigma_l * (lp + 0.05));
                                    let wl = (-dl).exp();
                                    let wgt = kxw * kyw * wn * wp * wa * wl;
                                    sum += src[j] * wgt;
                                    wsum += wgt;
                                }
                            }
                            if wsum > 0.0 { sum / wsum } else { src[i] }
                        })
                    })
                    .collect()
            });
            cur = next;
        }
        cur.iter().enumerate().map(|(i, c)| if guide[i].hit { c.mul_v(albedo(i)) } else { input[i] }).collect()
    }
}

/// Khronos PBR Neutral tone mapping (linear in, sRGB display out): keeps base colours up to
/// about 0.8 and compresses highlights toward white.
pub fn tonemap(c: V3) -> V3 {
    let start = 0.8 - 0.04;
    let desaturation = 0.15;
    let x = c.x.min(c.y).min(c.z);
    let offset = if x < 0.08 { x - 6.25 * x * x } else { 0.04 };
    let mut c = c - V3::splat(offset);
    let peak = c.max_elem();
    if peak >= start {
        let d = 1.0 - start;
        let new_peak = 1.0 - d * d / (peak + d - start);
        c *= new_peak / peak;
        let g = 1.0 - 1.0 / (desaturation * (peak - new_peak) + 1.0);
        c = c.lerp(V3::splat(new_peak), g);
    }
    v3(math::linear_to_srgb(c.x), math::linear_to_srgb(c.y), math::linear_to_srgb(c.z))
}

/// Renders every sample: `progress(done, total)` after each pass; `None` if `cancel` was set.
pub fn render(scene: std::sync::Arc<Scene>, camera: Camera, settings: Settings, threads: usize, cancel: &AtomicBool, mut progress: impl FnMut(u32, u32)) -> Option<RgbaImage> {
    let mut r = Renderer::new(scene, camera, settings, threads);
    while !r.is_done() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        r.pass();
        progress(r.samples_done(), settings.samples);
    }
    Some(r.image())
}

/// Mean and variance of an image's luminance (0–1), over its opaque pixels weighted by alpha:
/// a blank image has none.
pub fn luminance_stats(img: &RgbaImage) -> (f64, f64) {
    let (mut sum, mut sum2, mut wsum) = (0.0f64, 0.0f64, 0.0f64);
    for p in img.pixels() {
        let a = p[3] as f64 / 255.0;
        let l = (0.2126 * p[0] as f64 + 0.7152 * p[1] as f64 + 0.0722 * p[2] as f64) / 255.0;
        sum += l * a;
        sum2 += l * l * a;
        wsum += a;
    }
    if wsum == 0.0 {
        return (0.0, 0.0);
    }
    let mean = sum / wsum;
    (mean, (sum2 / wsum - mean * mean).max(0.0))
}

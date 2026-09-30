//! The path tracer on small scenes: deterministic for a fixed seed, never blank, framed, with
//! a ground shadow and a transparent background.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use cadrs_render::*;

/// A box `lo`–`hi` as 12 triangles with flat normals.
fn cuboid(b: &mut SceneBuilder, lo: [f32; 3], hi: [f32; 3], material: u32) {
    let c = |i: usize| [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
    let faces = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    for f in faces {
        let pos: Vec<[f32; 3]> = f.iter().map(|&i| c(i)).collect();
        b.add_mesh(&pos, &[], &[0, 1, 2, 0, 2, 3], |_| material);
    }
}

/// A UV sphere with smooth normals.
fn sphere(b: &mut SceneBuilder, center: [f32; 3], r: f32, material: u32) {
    let (nu, nv) = (48, 24);
    let mut pos = Vec::new();
    let mut nrm = Vec::new();
    for j in 0..=nv {
        let t = std::f32::consts::PI * j as f32 / nv as f32;
        for i in 0..=nu {
            let p = 2.0 * std::f32::consts::PI * i as f32 / nu as f32;
            let n = [t.sin() * p.cos(), t.sin() * p.sin(), t.cos()];
            nrm.push(n);
            pos.push([center[0] + r * n[0], center[1] + r * n[1], center[2] + r * n[2]]);
        }
    }
    let mut idx = Vec::new();
    for j in 0..nv {
        for i in 0..nu {
            let a = (j * (nu + 1) + i) as u32;
            let b2 = a + nu as u32 + 1;
            idx.extend_from_slice(&[a, b2, a + 1, a + 1, b2, b2 + 1]);
        }
    }
    b.add_mesh(&pos, &nrm, &idx, |_| material);
}

fn scene() -> Arc<Scene> {
    let mut b = SceneBuilder::new();
    let blue = b.material(Material::from_srgb([0x9b, 0xc1, 0xd8], 0.0, 0.45, 1.0));
    let steel = b.material(Material::from_srgb([0xc8, 0xc8, 0xcc], 1.0, 0.3, 1.0));
    cuboid(&mut b, [0.0, 0.0, 0.0], [60.0, 40.0, 20.0], blue);
    sphere(&mut b, [30.0, 20.0, 35.0], 15.0, steel);
    Arc::new(b.build())
}

fn camera(scene: &Scene, s: &Settings, projection: Projection) -> Camera {
    // The isometric view (azimuth 45°, elevation 35.26°).
    let (az, el) = (45f32.to_radians(), 35.264f32.to_radians());
    let back = [az.sin() * el.cos(), -az.cos() * el.cos(), el.sin()];
    Camera::fit(scene.bounds().unwrap(), back, [0.0, 0.0, 1.0], projection, s.width as f32 / s.height as f32, 0.08)
}

fn out_dir() -> std::path::PathBuf {
    let d = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("render");
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_fixed_seed_renders_identical_bytes() {
    let sc = scene();
    let s = Settings { width: 160, height: 90, samples: 6, seed: 42, ..Settings::default() };
    let cam = camera(&sc, &s, Projection::Perspective { fov_y: 30.0 });
    let cancel = AtomicBool::new(false);
    // Different thread counts: the samples don't depend on scheduling.
    let a = render(sc.clone(), cam, s, 1, &cancel, |_, _| {}).unwrap();
    let b = render(sc.clone(), cam, s, 4, &cancel, |_, _| {}).unwrap();
    assert_eq!(a.as_raw(), b.as_raw());
    // Another seed differs.
    let c = render(sc, cam, Settings { seed: 43, ..s }, 4, &cancel, |_, _| {}).unwrap();
    assert_ne!(a.as_raw(), c.as_raw());
}

#[test]
fn renders_are_not_blank_and_shadowed() {
    let sc = scene();
    let cancel = AtomicBool::new(false);
    for (name, env, projection) in [
        ("studio", EnvironmentPreset::Studio, Projection::Perspective { fov_y: 30.0 }),
        ("soft", EnvironmentPreset::SoftLight, Projection::Orthographic { height: 1.0 }),
        ("outdoor", EnvironmentPreset::Outdoor, Projection::Perspective { fov_y: 30.0 }),
        ("sunset", EnvironmentPreset::Sunset, Projection::Perspective { fov_y: 30.0 }),
    ] {
        let s = Settings { width: 480, height: 270, samples: 32, environment: env, ..Settings::default() };
        let cam = camera(&sc, &s, projection);
        let mut calls = 0;
        let img = render(sc.clone(), cam, s, 4, &cancel, |d, t| {
            calls += 1;
            assert!(d <= t);
        })
        .unwrap();
        assert_eq!(calls, 32);
        img.save(out_dir().join(format!("{name}.png"))).unwrap();
        assert_eq!(img.dimensions(), (480, 270));
        let (mean, var) = luminance_stats(&img);
        assert!(var > 0.002, "{name}: variance {var}");
        assert!(mean > 0.15 && mean < 0.95, "{name}: mean {mean}");
    }
}

#[test]
fn transparent_background_keeps_the_shadow() {
    let sc = scene();
    let s = Settings { width: 240, height: 135, samples: 16, background: Background::Transparent, ..Settings::default() };
    let cam = camera(&sc, &s, Projection::Perspective { fov_y: 30.0 });
    let img = render(sc, cam, s, 4, &AtomicBool::new(false), |_, _| {}).unwrap();
    img.save(out_dir().join("transparent.png")).unwrap();
    // The corners are clear, the centre (the model) opaque, and some pixels half-shadowed.
    assert_eq!(img.get_pixel(0, 0)[3], 0);
    assert_eq!(img.get_pixel(120, 60)[3], 255);
    assert!(img.pixels().any(|p| p[3] > 20 && p[3] < 230));
}

#[test]
fn cancelling_stops() {
    let sc = scene();
    let s = Settings { width: 64, height: 36, samples: 100, ..Settings::default() };
    let cam = camera(&sc, &s, Projection::Perspective { fov_y: 30.0 });
    let cancel = AtomicBool::new(false);
    let mut n = 0;
    let r = render(sc, cam, s, 2, &cancel, |d, _| {
        n = d;
        if d == 3 {
            cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    assert!(r.is_none());
    assert_eq!(n, 3);
}

#[test]
fn fitting_frames_the_model() {
    let sc = scene();
    let s = Settings::default();
    for projection in [Projection::Perspective { fov_y: 30.0 }, Projection::Orthographic { height: 1.0 }] {
        let cam = camera(&sc, &s, projection);
        // A cheap render's coverage: the model reaches near the frame but not past it.
        let small = Settings { width: 192, height: 108, samples: 1, ground_shadow: false, background: Background::Transparent, denoise: false, ..s };
        let img = render(sc.clone(), cam, small, 2, &AtomicBool::new(false), |_, _| {}).unwrap();
        let (mut x0, mut x1, mut y0, mut y1) = (u32::MAX, 0, u32::MAX, 0);
        for (x, y, p) in img.enumerate_pixels() {
            if p[3] > 0 {
                x0 = x0.min(x);
                x1 = x1.max(x);
                y0 = y0.min(y);
                y1 = y1.max(y);
            }
        }
        assert!(x0 > 0 && y0 > 0 && x1 < 191 && y1 < 107, "{projection:?}: {x0} {x1} {y0} {y1}");
        // It fills one direction to within the margin (8 % a side, perspective a little less).
        assert!((y1 - y0) as f32 > 108.0 * 0.6 || (x1 - x0) as f32 > 192.0 * 0.6, "{projection:?}: {x0} {x1} {y0} {y1}");
    }
}

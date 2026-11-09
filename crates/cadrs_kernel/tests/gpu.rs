//! The GPU path of the native kernel (`--features gpu`): mass properties of trimmed B-spline
//! faces computed on the GPU equal the CPU's (the same quadrature, the same Rust compiled by
//! rust-gpu). Without a GPU that runs f64 shaders the test only checks the CPU path.
#![cfg(feature = "gpu")]

use cadrs_kernel::native::*;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

fn p(x: f64, y: f64) -> Point2<f64> {
    Point2::new(x, y)
}

/// A thread profile swept along two turns of a helix (cubic Bézier pieces), its inner side on
/// the cylinder it winds round, joined to it: many trimmed B-spline faces.
fn threaded_cylinder(k: &mut NativeKernel) -> BodyId {
    use std::f64::consts::TAU;
    let (r, pitch, turns, per) = (18.375, 4.125, 2.0, 16usize);
    let n = (turns * per as f64) as usize;
    let th = TAU / per as f64;
    let kk = 4.0 / 3.0 * (th / 4.0).tan();
    let pt = |a: f64| Point3::new(r * a.cos(), r * a.sin(), pitch * a / TAU);
    let tan = |a: f64| Vector3::new(-r * a.sin(), r * a.cos(), pitch / TAU);
    let path: Vec<PathCurve> = (0..n)
        .map(|i| {
            let (a0, a1) = (th * i as f64, th * (i + 1) as f64);
            PathCurve::Bezier3 {
                poles: vec![pt(a0), pt(a0) + tan(a0) * kk, pt(a1) - tan(a1) * kk, pt(a1)],
                weights: vec![],
            }
        })
        .collect();
    let front = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: -Vector3::y_axis(),
    };
    let pts = [(18.375, -1.2), (19.8, -0.7), (19.8, 0.7), (18.375, 1.2)];
    let lp = Loop {
        curves: (0..4)
            .map(|i| Curve2::Line {
                a: p(pts[i].0, pts[i].1),
                b: p(pts[(i + 1) % 4].0, pts[(i + 1) % 4].1),
                source: None,
            })
            .collect(),
    };
    let prof = Profile::new(
        front,
        vec![Region {
            outer: lp,
            holes: vec![],
            source: None,
        }],
    );
    let thread = k
        .sweep_with(
            &prof,
            &SweepSpec {
                body: BodyKind::Solid,
                path,
                control: SweepControl::None,
                corners: Default::default(),
                faces: vec![],
            },
        )
        .unwrap()
        .bodies[0];
    let disc = Loop {
        curves: vec![Curve2::Circle {
            center: p(0.0, 0.0),
            radius: r,
            source: None,
        }],
    };
    let cyl = k
        .extrude(
            &Profile::new(
                Plane::top(),
                vec![Region {
                    outer: disc,
                    holes: vec![],
                    source: None,
                }],
            ),
            Extent::Blind(10.0),
        )
        .unwrap()
        .bodies[0];
    k.boolean(BoolOp::Union, cyl, &[thread]).unwrap().bodies[0]
}

#[test]
fn gpu_mass_equals_cpu() {
    let mut k = NativeKernel::new();
    let b = threaded_cylinder(&mut k);
    let gpu = native::mass::gpu_active();
    eprintln!(
        "GPU: {}",
        cadrs_gpu::get().map_or("none".to_string(), |g| g.adapter_name.clone())
    );
    native::mass::set_gpu(false);
    let t = std::time::Instant::now();
    let cpu = k.mass_properties(b).unwrap();
    let t_cpu = t.elapsed();
    native::mass::set_gpu(true);
    // Once to open the device and build the pipeline, then timed.
    let _ = k.mass_properties(b).unwrap();
    let t = std::time::Instant::now();
    let g = k.mass_properties(b).unwrap();
    let t_gpu = t.elapsed();
    eprintln!(
        "volume cpu {} gpu {} ({t_cpu:?} vs {t_gpu:?}, gpu active {gpu})",
        cpu.volume, g.volume
    );
    if std::env::var("FACE_TIMES").is_ok() {
        let body = k.get(b).unwrap().clone();
        native::mass::set_gpu(false);
        for (fi, f) in body.faces.iter().enumerate() {
            let t = std::time::Instant::now();
            native::mass::face_integrals(&body, f, &Vector3::zeros());
            let e = t.elapsed();
            if e.as_micros() > 2000 {
                let pcs: Vec<usize> = f
                    .loops
                    .iter()
                    .flatten()
                    .map(|c| match &c.pc {
                        native::curve::Curve::Nurbs(n) => n.cps.len(),
                        _ => 2,
                    })
                    .collect();
                eprintln!(
                    "face {fi} {:?}: {e:?}, p-curve poles {pcs:?}",
                    f.surf.kind()
                );
            }
        }
        native::mass::set_gpu(true);
    }
    let rel = |a: f64, b: f64| (a - b).abs() / b.abs().max(1e-300);
    assert!(
        rel(g.volume, cpu.volume) < 1e-11,
        "{} vs {}",
        g.volume,
        cpu.volume
    );
    assert!(rel(g.surface_area, cpu.surface_area) < 1e-11);
    assert!((g.center_of_mass - cpu.center_of_mass).norm() < 1e-9);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (g.inertia[(i, j)] - cpu.inertia[(i, j)]).abs() < 1e-8 * cpu.inertia[(i, i)].abs()
            );
        }
    }
}

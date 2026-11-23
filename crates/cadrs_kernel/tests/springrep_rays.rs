//! Ported from springrep's tests/rays.rs (MIT, https://github.com/springcraft-ai/springrep): rays
//! against faces, against closed forms: where they cross, how often, and the normal there.
//! springrep's `Rays::cast(o, d, range)` is cadrs's `Kernel::ray_hits` (every crossing of the
//! line, along the unit direction) kept to the range; the normal at a hit is
//! `Kernel::face_normals_at` there.

mod springrep_port;

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

struct Hit {
    t: f64,
    point: P3,
    normal: V3,
}

fn cast(k: &NativeKernel, b: BodyId, o: P3, d: V3, range: (f64, f64)) -> Vec<Hit> {
    let mut hits: Vec<Hit> = k
        .ray_hits(b, o, d)
        .unwrap()
        .into_iter()
        .filter(|h| h.t >= range.0 && h.t <= range.1)
        .map(|h| Hit {
            t: h.t,
            point: h.point,
            normal: k.face_normals_at(b, h.face, &[h.point]).unwrap()[0],
        })
        .collect();
    hits.sort_by(|a, b| a.t.total_cmp(&b.t));
    hits
}

fn assert_ts(hits: &[Hit], want: &[f64]) {
    let got: Vec<f64> = hits.iter().map(|h| h.t).collect();
    assert_eq!(got.len(), want.len(), "{got:?} vs {want:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g - w).abs() <= 1e-9 * (1.0 + w.abs()),
            "{got:?} vs {want:?}"
        );
    }
}

const FAR: (f64, f64) = (0.0, 1e3);

fn xz() -> Plane {
    frame(P3::origin(), -V3::y(), V3::x())
}

#[test]
fn a_ray_beside_a_balls_seam_crosses_it_on_either_side() {
    let mut k = NativeKernel::new();
    let a = ball(&mut k, [0.0; 3], 3.0);
    let c = ball(&mut k, [-0.5, -1.0, 1.5], 2.0);
    let b = union(&mut k, a, c);
    let x: f64 = 1.0;
    for y in [-0.05, -0.02, 0.02, 0.05] {
        let z = (9.0 - x * x - y * y).sqrt();
        let hits = cast(&k, b, P3::new(x, y, -10.0), V3::z(), FAR);
        assert_ts(&hits, &[10.0 - z, 10.0 + z]);
    }
}

#[test]
fn a_ray_through_a_ball_crosses_twice_touching_once_and_missing_never() {
    let mut k = NativeKernel::new();
    let (c, r) = (P3::new(1.0, 2.0, 3.0), 3.0);
    let b = ball(&mut k, [c.x, c.y, c.z], r);
    let o = c + V3::new(-10.0, 1.0, 0.0);
    let hits = cast(&k, b, o, V3::x(), FAR);
    let half = (r * r - 1.0f64).sqrt();
    assert_ts(&hits, &[10.0 - half, 10.0 + half]);
    // The normal points out of the ball: from the centre to the point.
    for h in &hits {
        assert!(
            ((h.point - c) * (1.0 / r) - h.normal).norm() < 1e-9,
            "{:?} {:?}",
            h.point,
            h.normal
        );
    }
    // Grazing the ball at h = r touches it once; past it, nothing.
    assert_ts(
        &cast(&k, b, c + V3::new(-10.0, r, 0.0), V3::x(), FAR),
        &[10.0],
    );
    assert_ts(
        &cast(&k, b, c + V3::new(-10.0, 3.5, 0.0), V3::x(), FAR),
        &[],
    );
    // A range that stops inside keeps the entry only.
    assert_ts(&cast(&k, b, o, V3::x(), (0.0, 10.0)), &[10.0 - half]);
}

#[test]
fn rays_from_inside_a_cylinder_kilometres_across_leave_through_its_side() {
    let mut k = NativeKernel::new();
    let (r, h) = (5e4, 4e4);
    let b = extrude(&mut k, world(), circle(0.0, 0.0, r), h);
    let d = V3::new(0.6213, -0.2841, 0.07302).normalize();
    for p in [
        P3::new(-24471.3, -24.0, 9520.7),
        P3::new(21381.1, 5364.3, 3236.0),
        P3::new(-40844.9, 3285.6, 18088.7),
    ] {
        let (a, bq, c) = (
            d.x * d.x + d.y * d.y,
            2.0 * (p.x * d.x + p.y * d.y),
            p.x * p.x + p.y * p.y - r * r,
        );
        let t = (-bq + (bq * bq - 4.0 * a * c).sqrt()) / (2.0 * a);
        assert!((p + d * t).z < h, "the ray leaves through the side");
        let hits = cast(&k, b, p, d, (0.0, 1e6));
        assert_ts(&hits, &[t]);
    }
}

#[test]
fn a_ray_clipping_a_cylinder_shallower_than_the_mesh_crosses_it_twice() {
    let mut k = NativeKernel::new();
    let (r, h) = (2.0, 10.0);
    let b = extrude(&mut k, world(), circle(0.0, 0.0, r), h);
    let d = V3::new(0.3713, 0.5347, 0.07591).normalize();
    let across = V3::new(d.y, -d.x, 0.0).normalize();
    let steep = V3::new(0.3713, 0.5347, 0.7591).normalize();
    let found = (
        P3::new(0.6263603775040814, -2.600153099460448, -1.2639721248298266),
        steep,
    );
    let shallow = [3e-3, 3e-4, 3e-5, 3e-6].map(|depth| {
        (
            P3::origin() + across * (r - depth) - d * 4.0 + V3::new(0.0, 0.0, 5.0),
            d,
        )
    });
    for (p, d) in shallow.into_iter().chain([found]) {
        let (a, bq, c) = (
            d.x * d.x + d.y * d.y,
            p.x * d.x + p.y * d.y,
            p.x * p.x + p.y * p.y - r * r,
        );
        let root = (bq * bq - a * c).sqrt();
        let hits = cast(&k, b, p, d, (0.0, 100.0));
        assert_ts(&hits, &[(-bq - root) / a, (-bq + root) / a]);
        assert!(hits[0].normal.dot(&d) < 0.0 && hits[1].normal.dot(&d) > 0.0);
    }
}

#[test]
fn rays_from_inside_a_lens_micrometres_across_leave_where_its_spheres_say() {
    let mut k = NativeKernel::new();
    let (ca, ra) = (P3::origin(), 0.004054250852155359);
    let (cb, rb) = (
        P3::new(0.00338806114604008, 0.0, 0.0),
        0.0026689525243786373,
    );
    let a = ball(&mut k, [ca.x, ca.y, ca.z], ra);
    let bb = ball(&mut k, [cb.x, cb.y, cb.z], rb);
    let lens = intersection(&mut k, a, bb);
    let p = P3::new(0.001017, -0.000028, -0.000248);
    for d in [
        V3::new(0.3713, 0.5347, 0.7591),
        V3::new(-0.6213, 0.2841, 0.7302),
        V3::new(0.4472, -0.8018, 0.3961),
    ] {
        let d = d.normalize();
        let exit = |c: P3, r: f64| {
            let (b, cc) = ((p - c).dot(&d), (p - c).dot(&(p - c)) - r * r);
            -b + (b * b - cc).sqrt()
        };
        assert_ts(
            &cast(&k, lens, p, d, FAR),
            &[exit(ca, ra).min(exit(cb, rb))],
        );
    }
}

#[test]
fn a_ray_through_a_box_enters_and_leaves_where_the_slabs_say() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, [0.0; 3], [10.0, 6.0, 3.0]);
    let o = P3::new(-5.0, 2.0, 1.0);
    let d = V3::new(1.0, 0.1, 0.05).normalize();
    let slab = |o: f64, d: f64, lo: f64, hi: f64| {
        let (a, z) = ((lo - o) / d, (hi - o) / d);
        (a.min(z), a.max(z))
    };
    let s = [
        slab(o.x, d.x, 0.0, 10.0),
        slab(o.y, d.y, 0.0, 6.0),
        slab(o.z, d.z, 0.0, 3.0),
    ];
    let enter = s.iter().map(|x| x.0).fold(f64::MIN, f64::max);
    let leave = s.iter().map(|x| x.1).fold(f64::MAX, f64::min);
    assert_ts(&cast(&k, b, o, d, FAR), &[enter, leave]);
}

#[test]
fn a_ray_through_an_edge_crosses_there_once() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, [0.0; 3], [10.0, 6.0, 3.0]);
    let d = V3::new(1.0, 1.0, 0.0);
    let hits = cast(&k, b, P3::new(-1.0, -1.0, 1.5), d, FAR);
    assert_ts(&hits, &[2.0f64.sqrt(), 7.0 * 2.0f64.sqrt()]);
}

#[test]
fn a_ray_along_a_torus_diameter_crosses_four_times_and_down_its_axis_none() {
    let mut k = NativeKernel::new();
    let t = revolve(&mut k, xz(), circle(10.0, 0.0, 2.0), P3::origin(), V3::z());
    let o = P3::new(-20.0, 0.0, 0.0);
    assert_ts(&cast(&k, t, o, V3::x(), FAR), &[8.0, 12.0, 28.0, 32.0]);
    assert_ts(&cast(&k, t, P3::new(0.0, 0.0, -20.0), V3::z(), FAR), &[]);
}

/// The 24 points of springrep's spline disc outline.
fn outline_points() -> Vec<[f64; 2]> {
    (0..24)
        .map(|i| {
            let a = TAU * i as f64 / 24.0;
            [30.0 * a.cos(), 20.0 * a.sin() + 3.0 * (3.0 * a).sin()]
        })
        .collect()
}

#[test]
fn a_ray_across_a_spline_extrusion_crosses_where_the_outline_does() {
    // The crossings are where the outline passes y = 9, found by sampling its curve (the profile's
    // own, which the disc's bottom edge is) and bisecting each change of side.
    let mut k = NativeKernel::new();
    let pts = outline_points();
    let disc = extrude(&mut k, world(), periodic_spline(&pts), 10.0);
    let c = periodic_curve(&pts);
    let (t0, t1) = c.range();
    let side = |t: f64| c.point(t).y - 9.0;
    let n = 20_000;
    let at = |i: usize| t0 + (t1 - t0) * i as f64 / n as f64;
    let o = P3::new(-40.0, 9.0, 5.0);
    let mut want = Vec::new();
    for i in 0..n {
        let (mut lo, mut hi) = (at(i), at(i + 1));
        if side(lo).signum() == side(hi).signum() {
            continue;
        }
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if side(mid).signum() == side(lo).signum() {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        want.push(c.point(0.5 * (lo + hi)).x - o.x);
    }
    want.sort_by(f64::total_cmp);
    assert!(want.len() >= 2, "{want:?}");
    assert_ts(&cast(&k, disc, o, V3::x(), FAR), &want);
}

#[test]
fn a_ray_just_inside_a_curved_rim_crosses_both_ends() {
    let mut k = NativeKernel::new();
    let disc = extrude(&mut k, world(), circle(0.0, 0.0, 3.0), 4.0);
    let a = PI / 128.0;
    let r = 3.0 - 2e-4;
    let o = P3::new(r * a.cos(), r * a.sin(), -5.0);
    let hits = cast(&k, disc, o, V3::z(), FAR);
    assert_ts(&hits, &[5.0, 9.0]);
}

//! Ported from springrep's tests/classify.rs (MIT, https://github.com/springcraft-ai/springrep):
//! points against solids with closed forms: which the solid holds, wherever the classifier's
//! rays meet its edges, corners, seams and poles. springrep's `Rays::holds` / `Rays::side` are
//! cadrs's `Kernel::classify` at springrep's tolerance of 1e-7 mm (a point classified on the
//! boundary is springrep's undecided `None` where it asks `holds`).

mod springrep_port;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::Matrix3;
use springrep_port::*;

/// springrep's linear tolerance.
const TOL: f64 = 1e-7;

/// A micrometre: a point nearer the boundary than this may be placed either way, or not at all.
const NEAR: f64 = 1e-3;

fn holds(k: &NativeKernel, b: BodyId, p: P3) -> Option<bool> {
    match k.classify(b, p, TOL).unwrap() {
        PointClass::Inside => Some(true),
        PointClass::Outside => Some(false),
        PointClass::OnBoundary => None,
    }
}

fn side(k: &NativeKernel, b: BodyId, p: P3) -> PointClass {
    k.classify(b, p, TOL).unwrap()
}

#[test]
fn points_about_a_cylinder_are_held_where_the_closed_form_says() {
    let mut k = NativeKernel::new();
    let (r, h) = (2.0, 10.0);
    let b = cylinder(&mut k, &world(), r, h);
    let mut seq = Seq(11);
    for _ in 0..20_000 {
        let p = seq.within([-3.0, -3.0, -2.0], [3.0, 3.0, 12.0]);
        let rho = p.x.hypot(p.y);
        if (rho - r).abs() < NEAR || p.z.abs() < NEAR || (p.z - h).abs() < NEAR {
            continue;
        }
        let inside = rho < r && p.z > 0.0 && p.z < h;
        assert_eq!(holds(&k, b, p), Some(inside), "{p:?}");
    }
}

#[test]
fn points_about_a_block_are_held_where_its_slabs_say() {
    let mut k = NativeKernel::new();
    let b = pblock(&mut k, &world(), [4.0, 3.0, 2.0]);
    let size = [4.0, 3.0, 2.0];
    let mut seq = Seq(5);
    for _ in 0..20_000 {
        let p = seq.within([-1.0; 3], [5.0, 4.0, 3.0]);
        let q = [p.x, p.y, p.z];
        if (0..3).any(|i| q[i].abs() < NEAR || (q[i] - size[i]).abs() < NEAR) {
            continue;
        }
        let inside = (0..3).all(|i| q[i] > 0.0 && q[i] < size[i]);
        assert_eq!(holds(&k, b, p), Some(inside), "{p:?}");
    }
}

#[test]
fn a_torus_holds_its_tube_and_neither_its_hole_nor_a_void_cut_in_a_block() {
    let mut k = NativeKernel::new();
    let (major, minor) = (3.0, 1.0);
    let ring = torus(&mut k, &world(), major, minor);
    let mut seq = Seq(3);
    for _ in 0..5_000 {
        let p = seq.within([-4.5, -4.5, -1.5], [4.5, 4.5, 1.5]);
        let from_tube = (p.x.hypot(p.y) - major).hypot(p.z);
        if (from_tube - minor).abs() < NEAR {
            continue;
        }
        assert_eq!(holds(&k, ring, p), Some(from_tube < minor), "{p:?}");
    }

    let outer = pblock(&mut k, &world(), [6.0, 6.0, 6.0]);
    let c = P3::new(3.0, 3.0, 3.0);
    let centre = frame(c, V3::z(), V3::x());
    let s = sphere(&mut k, &centre, 1.5);
    let hollow = difference(&mut k, outer, s);
    for _ in 0..5_000 {
        let p = seq.within([-1.0; 3], [7.0; 3]);
        let q = [p.x, p.y, p.z];
        let from_void = (p - c).norm();
        let on_wall = (0..3).any(|i| q[i].abs() < NEAR || (q[i] - 6.0).abs() < NEAR);
        if (from_void - 1.5).abs() < NEAR || on_wall {
            continue;
        }
        let in_block = (0..3).all(|i| q[i] > 0.0 && q[i] < 6.0);
        assert_eq!(
            holds(&k, hollow, p),
            Some(in_block && from_void > 1.5),
            "{p:?}"
        );
    }
}

#[test]
fn an_arena_with_no_solid_holds_no_point() {
    // springrep classifies against an empty B-rep (`Brep::default()`); cadrs has no empty body to
    // ask, so there is no counterpart.
}

/// The map springrep's conformance case stretched and sheared a ball by.
fn stretch() -> (Matrix3<f64>, V3) {
    (
        Matrix3::new(
            0.1028, -0.9280, 0.5227, -1.0971, 0.5789, 0.4802, 1.2920, 0.5654, 0.3662,
        ),
        V3::new(20.147, -35.624, 7.546),
    )
}

/// A ball stretched and sheared: its skin a rational spline surface with a pole at each end.
fn stretched_ball(k: &mut NativeKernel) -> (BodyId, Matrix3<f64>, V3, P3, f64) {
    let c = P3::new(-2.919, -1.302, -1.162);
    let r = 1.815;
    let (m, t) = stretch();
    let f = frame(c, V3::z(), V3::x());
    let s = sphere(k, &f, r);
    let b = k.transform_affine(s, m, t).unwrap().first().unwrap();
    (b, m, t, c, r)
}

#[test]
fn a_point_near_a_stretched_balls_pole_projects_onto_its_skin_there() {
    let mut k = NativeKernel::new();
    let (b, m, t, c, r) = stretched_ball(&mut k);
    let body = k.get(b).unwrap();
    let s = &body.faces[0].surf;
    let map = |p: P3| m * p + t;
    for lat in [80.0f64, 85.0, 88.0, 89.5, 89.99, -85.0, -89.5] {
        for lon in [10.0f64, 100.0, 200.0, 300.0] {
            let (la, lo) = (lat.to_radians(), lon.to_radians());
            let local = V3::new(la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin());
            for scale in [1.0, 0.97, 1.03] {
                let p = map(c + local * (r * scale));
                let (u, v) = s.project(&p, None);
                let foot = s.eval(u, v);
                if scale == 1.0 {
                    let off = (foot - p).norm();
                    assert!(off < 1e-9, "lat {lat} lon {lon}: {off} off");
                }
                // The foot is no farther than the point of the skin on the same radius.
                let radial = map(c + local * r);
                assert!(
                    (foot - p).norm() <= (radial - p).norm() + 1e-9,
                    "lat {lat} lon {lon} at {scale}: {} against {}",
                    (foot - p).norm(),
                    (radial - p).norm()
                );
            }
        }
    }
}

#[test]
fn a_stretched_ball_holds_the_points_its_ball_did() {
    let mut k = NativeKernel::new();
    let (b, m, t, c, r) = stretched_ball(&mut k);
    let mut seq = Seq(7);
    for _ in 0..20_000 {
        let l = seq.within([-1.2 * r; 3], [1.2 * r; 3]).coords;
        if (l.norm() - r).abs() < NEAR {
            continue;
        }
        let p = m * (c + l) + t;
        assert_eq!(holds(&k, b, p), Some(l.norm() < r), "{p:?}");
    }
}

#[test]
fn a_point_of_the_boundary_is_on_it_and_a_step_off_is_in_or_out() {
    let mut k = NativeKernel::new();
    let (r, h) = (2.0, 10.0);
    let b = cylinder(&mut k, &world(), r, h);
    // On the side, on each end, on a rim, and where the side's seam meets a rim.
    for p in [
        P3::new(r * 0.6, r * 0.8, 4.0),
        P3::new(0.5, -0.3, 0.0),
        P3::new(-1.1, 0.2, h),
        P3::new(0.0, r, h),
        P3::new(r, 0.0, 0.0),
        P3::new(r, 0.0, 3.0),
    ] {
        assert_eq!(side(&k, b, p), PointClass::OnBoundary, "{p:?}");
    }
    let s = |k: &NativeKernel, p| side(k, b, p);
    assert_eq!(
        s(&k, P3::new(r * 0.6 - 1e-3, r * 0.8, 4.0)),
        PointClass::Inside
    );
    assert_eq!(
        s(&k, P3::new(r * 0.6 + 1e-3, r * 0.8, 4.0)),
        PointClass::Outside
    );
    assert_eq!(s(&k, P3::new(0.5, -0.3, 1e-3)), PointClass::Inside);
    assert_eq!(s(&k, P3::new(0.5, -0.3, -1e-3)), PointClass::Outside);

    let b = pblock(&mut k, &world(), [4.0, 3.0, 2.0]);
    // On a face, along an edge and at a corner.
    for p in [
        P3::new(1.0, 1.0, 2.0),
        P3::new(4.0, 1.0, 2.0),
        P3::new(4.0, 3.0, 2.0),
        P3::new(0.0, 0.0, 0.0),
    ] {
        assert_eq!(side(&k, b, p), PointClass::OnBoundary, "{p:?}");
    }
    assert_eq!(
        side(&k, b, P3::new(3.999, 2.999, 1.999)),
        PointClass::Inside
    );
    assert_eq!(
        side(&k, b, P3::new(4.001, 3.001, 2.001)),
        PointClass::Outside
    );
}

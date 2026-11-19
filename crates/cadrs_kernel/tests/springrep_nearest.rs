//! Ported from springrep's tests/nearest.rs (MIT, https://github.com/springcraft-ai/springrep):
//! the point of a solid's boundary nearest a point, against closed forms: on a face, on an edge,
//! at a corner, and from inside the solid as from outside. springrep's `Nearest::boundary(b).to(p)`
//! is cadrs's `min_distance(Point(p), Body(b))`, and `Nearest::of(b, face)` its `Face` target;
//! springrep's `Nearest::solid` (nil inside the solid) has no counterpart, cadrs's body target
//! being its boundary, so it is that here too.

mod springrep_port;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::Matrix3;
use springrep_port::*;

fn to(k: &NativeKernel, b: BodyId, p: P3) -> DistanceResult {
    k.min_distance(&DistanceTarget::Point(p), &DistanceTarget::Body(b))
        .unwrap()
}

fn assert_near(got: f64, want: f64, what: &str) {
    assert!(
        (got - want).abs() <= 1e-9 * (1.0 + want),
        "{what}: {got} against {want}"
    );
}

#[test]
fn the_nearest_point_of_a_ball_lies_along_the_radius() {
    let mut k = NativeKernel::new();
    let c = P3::new(1.0, -2.0, 0.5);
    let b = sphere(&mut k, &frame(c, V3::z(), V3::x()), 2.0);
    let mut seq = Seq(1);
    for _ in 0..300 {
        let p = c + seq.within([-4.0; 3], [4.0; 3]).coords;
        let from_centre = (p - c).norm();
        if from_centre < 1e-3 {
            continue;
        }
        let got = to(&k, b, p);
        assert_near(got.distance, (from_centre - 2.0).abs(), "distance");
        let foot = c + (p - c) * (2.0 / from_centre);
        assert!((got.b - foot).norm() < 1e-7, "{p:?}: {:?}", got.b);
    }
    for z in [3.0, 2.5, 1.0, -1.0, -3.5] {
        let got = to(&k, b, c + V3::z() * z);
        assert_near(got.distance, (z.abs() - 2.0).abs(), "on the axis");
    }
}

#[test]
fn the_nearest_point_of_a_block_is_on_a_face_an_edge_or_a_corner() {
    let mut k = NativeKernel::new();
    let size = [4.0, 3.0, 2.0];
    let b = pblock(&mut k, &world(), size);
    let mut seq = Seq(2);
    for _ in 0..300 {
        let p = seq.within([-2.0; 3], [6.0, 5.0, 4.0]);
        let q = [p.x, p.y, p.z];
        let inside = (0..3).all(|i| q[i] > 0.0 && q[i] < size[i]);
        let want = if inside {
            (0..3)
                .map(|i| q[i].min(size[i] - q[i]))
                .fold(f64::INFINITY, f64::min)
        } else {
            let clamped = [0, 1, 2].map(|i| q[i].clamp(0.0, size[i]));
            (p - p3(clamped)).norm()
        };
        let got = to(&k, b, p);
        assert_near(got.distance, want, "distance");
        assert_near((got.b - p).norm(), want, "the point given");
    }
}

#[test]
fn the_nearest_point_of_a_cylinder_is_on_its_side_an_end_or_a_rim() {
    let mut k = NativeKernel::new();
    let (r, h) = (1.5, 4.0);
    let b = cylinder(&mut k, &world(), r, h);
    let mut seq = Seq(3);
    for _ in 0..300 {
        let p = seq.within([-3.0, -3.0, -2.0], [3.0, 3.0, 6.0]);
        let rho = p.x.hypot(p.y);
        let (dr, dz) = (rho - r, (-p.z).max(p.z - h));
        let want = if dr <= 0.0 && dz <= 0.0 {
            (-dr).min(-dz)
        } else {
            dr.max(0.0).hypot(dz.max(0.0))
        };
        assert_near(to(&k, b, p).distance, want, "distance");
    }
}

#[test]
fn the_nearest_point_of_a_torus_is_on_its_tube() {
    let mut k = NativeKernel::new();
    let (major, minor) = (3.0, 1.0);
    let b = torus(&mut k, &world(), major, minor);
    let mut seq = Seq(4);
    for _ in 0..300 {
        let p = seq.within([-5.0, -5.0, -2.0], [5.0, 5.0, 2.0]);
        let from_tube = (p.x.hypot(p.y) - major).hypot(p.z);
        assert_near(to(&k, b, p).distance, (from_tube - minor).abs(), "distance");
    }
}

#[test]
fn the_nearest_point_of_a_cone_is_on_its_side_its_base_or_its_rim() {
    let mut k = NativeKernel::new();
    let (r, h) = (2.0, 3.0);
    let b = cone(&mut k, &world(), r, 0.0, h);
    let to_segment = |x: f64, y: f64, a: (f64, f64), b: (f64, f64)| {
        let (ux, uy) = (b.0 - a.0, b.1 - a.1);
        let t = (((x - a.0) * ux + (y - a.1) * uy) / (ux * ux + uy * uy)).clamp(0.0, 1.0);
        (x - a.0 - t * ux).hypot(y - a.1 - t * uy)
    };
    let mut seq = Seq(5);
    for _ in 0..300 {
        let p = seq.within([-3.0, -3.0, -1.0], [3.0, 3.0, 4.0]);
        let rho = p.x.hypot(p.y);
        let want = to_segment(rho, p.z, (0.0, 0.0), (r, 0.0)).min(to_segment(
            rho,
            p.z,
            (r, 0.0),
            (0.0, h),
        ));
        assert_near(to(&k, b, p).distance, want, "distance");
    }
}

#[test]
fn the_nearest_point_of_a_stretched_ball_is_found_at_its_poles_as_elsewhere() {
    let mut k = NativeKernel::new();
    let c = P3::new(-2.919, -1.302, -1.162);
    let r = 1.815;
    let m = Matrix3::new(
        0.1028, -0.9280, 0.5227, -1.0971, 0.5789, 0.4802, 1.2920, 0.5654, 0.3662,
    );
    let t = V3::new(20.147, -35.624, 7.546);
    let s = sphere(&mut k, &frame(c, V3::z(), V3::x()), r);
    let b = k.transform_affine(s, m, t).unwrap().first().unwrap();
    for lat in [0.0f64, 60.0, 85.0, 89.0, 90.0, -89.0, -90.0] {
        for lon in [10.0f64, 130.0, 250.0] {
            let (la, lo) = (lat.to_radians(), lon.to_radians());
            let local = V3::new(la.cos() * lo.cos(), la.cos() * lo.sin(), la.sin());
            let on = m * (c + local * r) + t;
            let got = to(&k, b, on);
            assert!(got.distance < 1e-9, "lat {lat} lon {lon}: {}", got.distance);
        }
    }
}

#[test]
#[ignore = "cadrs measures distance between boundaries, and has no distance between solids"]
fn the_nearest_point_of_a_solid_to_a_point_it_holds_is_the_point() {
    let mut k = NativeKernel::new();
    let b = pblock(&mut k, &world(), [4.0, 3.0, 2.0]);
    let inside = P3::new(1.0, 2.0, 0.5);
    // springrep's `Nearest::solid`: nil for a point the solid holds (cadrs: the boundary's).
    let got = to(&k, b, inside);
    assert!(
        got.distance == 0.0 && (got.b - inside).norm() < 1e-12,
        "{got:?}"
    );
    let out = P3::new(6.0, 1.0, 1.0);
    let got = to(&k, b, out);
    assert_near(got.distance, 2.0, "outside");
    assert!((got.b - P3::new(4.0, 1.0, 1.0)).norm() < 1e-9);
    // The boundary alone is half a millimetre from the point inside.
    let got = to(&k, b, inside);
    assert_near(got.distance, 0.5, "to the boundary");
}

#[test]
fn the_nearest_point_of_a_rim_is_found_either_side_of_where_the_rim_closes() {
    let mut k = NativeKernel::new();
    let (r, h) = (2.0, 5.0);
    let b = cylinder(&mut k, &world(), r, h);
    // springrep asks its boundary and its solid; the points are outside, so both are the
    // boundary's here.
    for _ in 0..2 {
        for y in [0.021, -0.021, 0.3, -0.3, 1e-4, -1e-4, 0.0] {
            let q = P3::new(3.2, y, 7.2);
            let rho = q.x.hypot(q.y);
            let got = to(&k, b, q);
            assert_near(got.distance, (rho - r).hypot(q.z - h), "distance");
            let foot = P3::new(r * q.x / rho, r * q.y / rho, h);
            assert!((got.b - foot).norm() < 1e-7, "y {y}: {:?}", got.b);
        }
    }
}

#[test]
fn the_nearest_point_of_one_face_ignores_the_rest_of_the_solid() {
    let mut k = NativeKernel::new();
    let b = pblock(&mut k, &world(), [20.0, 20.0, 10.0]);
    let face = |k: &NativeKernel, z: f64| {
        k.faces(b)
            .unwrap()
            .into_iter()
            .find(|f| (f.center.z - z).abs() < 1e-9)
            .unwrap()
            .id
    };
    let p = P3::new(25.0, 10.0, 12.0);
    for (z, want) in [(10.0, 29f64.sqrt()), (0.0, 169f64.sqrt())] {
        let f = face(&k, z);
        let found = k
            .min_distance(&DistanceTarget::Point(p), &DistanceTarget::Face(b, f))
            .unwrap();
        assert_near(found.distance, want, "one face");
        assert_near(found.b.z, z, "on that face");
    }
}

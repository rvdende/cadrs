//! The shape queries against closed forms: principal curvatures of the boundary, plane sections
//! and their areas, Betti numbers and the interference of two solids.

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point3, Unit, Vector3};

fn p(x: f64, y: f64, z: f64) -> Point3<f64> {
    Point3::new(x, y, z)
}

fn one(r: Result<OpResult>) -> BodyId {
    r.unwrap().bodies[0]
}

fn block(k: &mut NativeKernel, min: Point3<f64>, max: Point3<f64>) -> BodyId {
    one(k.primitive(&Primitive::Box { min, max }))
}

fn ball(k: &mut NativeKernel, c: Point3<f64>, r: f64) -> BodyId {
    one(k.primitive(&Primitive::Sphere {
        center: c,
        radius: r,
        axis: Vector3::z_axis(),
        seam: Vector3::x_axis(),
    }))
}

fn cylinder(k: &mut NativeKernel, base: Point3<f64>, r: f64, h: f64) -> BodyId {
    one(k.primitive(&Primitive::Cylinder {
        base,
        axis: Vector3::z_axis(),
        radius: r,
        height: h,
    }))
}

fn ring(k: &mut NativeKernel, major: f64, minor: f64) -> BodyId {
    one(k.primitive(&Primitive::Torus {
        center: Point3::origin(),
        axis: Vector3::z_axis(),
        major,
        minor,
        spindle: Default::default(),
    }))
}

fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-9 * (1.0 + want.abs()),
        "{got} against {want}"
    );
}

fn bends(got: Option<[f64; 2]>, want: [f64; 2]) {
    let g = got.unwrap_or_else(|| panic!("a crease, against {want:?}"));
    close(g[0], want[0]);
    close(g[1], want[1]);
}

#[test]
fn curvatures_of_the_primitives() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(0.0, 0.0, 0.0), p(10.0, 20.0, 30.0));
    let c = cylinder(&mut k, p(0.0, 0.0, 0.0), 5.0, 20.0);
    let s = ball(&mut k, p(1.0, 2.0, 3.0), 4.0);
    let t = ring(&mut k, 10.0, 3.0);
    let cone = one(k.primitive(&Primitive::Cone {
        base: Point3::origin(),
        axis: Vector3::z_axis(),
        base_radius: 6.0,
        top_radius: 2.0,
        height: 8.0,
    }));
    let got = k
        .curvatures_at(b, &[p(5.0, 10.0, 31.0), p(10.0, 20.0, 15.0)])
        .unwrap();
    bends(got[0], [0.0, 0.0]);
    assert_eq!(got[1], None, "an edge of the block is a crease");
    // On the side, off it, and on the seam.
    let got = k
        .curvatures_at(c, &[p(5.0, 0.0, 10.0), p(0.0, 7.0, 3.0), p(5.0, 0.0, 0.0)])
        .unwrap();
    bends(got[0], [0.2, 0.0]);
    bends(got[1], [0.2, 0.0]);
    assert_eq!(got[2], None, "the rim is a crease");
    // Anywhere, the poles too.
    let got = k
        .curvatures_at(s, &[p(9.0, 2.0, 3.0), p(1.0, 2.0, 7.0), p(1.0, 2.0, -5.0)])
        .unwrap();
    for g in got {
        bends(g, [0.25, 0.25]);
    }
    // Outside, top and inside of the tube: 1/r, and cos φ/(R + r cos φ).
    let got = k
        .curvatures_at(
            t,
            &[p(14.0, 0.0, 0.0), p(0.0, 10.0, 4.0), p(-7.0, 0.0, 0.0)],
        )
        .unwrap();
    bends(got[0], [1.0 / 3.0, 1.0 / 13.0]);
    bends(got[1], [1.0 / 3.0, 0.0]);
    bends(got[2], [1.0 / 3.0, -1.0 / 7.0]);
    // A cone leaning α from its axis: cos α/ρ at radius ρ.
    let alpha = (4.0f64 / 8.0).atan();
    let got = k.curvatures_at(cone, &[p(4.0, 0.0, 4.0)]).unwrap();
    bends(got[0], [alpha.cos() / 4.0, 0.0]);
}

#[test]
fn curvature_in_a_hole() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(-10.0, -10.0, 0.0), p(10.0, 10.0, 5.0));
    let c = cylinder(&mut k, p(0.0, 0.0, -1.0), 3.0, 7.0);
    let holed = one(k.boolean(BoolOp::Subtract, b, &[c]));
    let got = k
        .curvatures_at(holed, &[p(0.0, 2.0, 2.5), p(3.0, 0.0, 1.0)])
        .unwrap();
    bends(got[0], [0.0, -1.0 / 3.0]);
    bends(got[1], [0.0, -1.0 / 3.0]);
}

/// Each piece's points on the plane, and the pieces' total length.
fn section_length(pieces: &[Vec<Point3<f64>>], o: Point3<f64>, n: Vector3<f64>) -> f64 {
    let mut len = 0.0;
    for piece in pieces {
        assert!(piece.len() >= 2);
        for w in piece.windows(2) {
            assert!((w[0] - o).dot(&n).abs() < 1e-9, "{:?} off the plane", w[0]);
            len += (w[1] - w[0]).norm();
        }
    }
    len
}

#[test]
fn plane_sections() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(0.0, 0.0, 0.0), p(10.0, 20.0, 30.0));
    let z = Vector3::z();
    let pieces = k.plane_section(b, p(0.0, 0.0, 15.0), z, 0.01).unwrap();
    close(section_length(&pieces, p(0.0, 0.0, 15.0), z), 60.0);
    assert!(
        k.plane_section(b, p(0.0, 0.0, 40.0), z, 0.01)
            .unwrap()
            .is_empty()
    );
    // A ball cut 3 from its centre: a circle of radius 4, its chords within the tolerance.
    let s = ball(&mut k, p(0.0, 0.0, 0.0), 5.0);
    let pieces = k.plane_section(s, p(0.0, 0.0, 3.0), z, 1e-3).unwrap();
    let len = section_length(&pieces, p(0.0, 0.0, 3.0), z);
    let short = TAU * 4.0 - len;
    assert!((0.0..TAU * 4.0 * 1e-3 / 4.0).contains(&short), "{len}");
    for q in pieces.iter().flatten() {
        assert!((q.coords.norm() - 5.0).abs() < 1e-9);
    }
    // A ring through its axis: two circles of its tube.
    let t = ring(&mut k, 10.0, 3.0);
    let y = Vector3::y();
    let pieces = k.plane_section(t, Point3::origin(), y, 1e-3).unwrap();
    let len = section_length(&pieces, Point3::origin(), y);
    assert!(
        (2.0 * TAU * 3.0 - len).abs() < 2.0 * TAU * 3.0 * 1e-3,
        "{len}"
    );
}

#[test]
fn section_areas() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(0.0, 0.0, 0.0), p(10.0, 20.0, 30.0));
    let z = Vector3::z();
    close(k.section_area(b, p(0.0, 0.0, 15.0), z).unwrap(), 200.0);
    close(k.section_area(b, p(0.0, 0.0, 15.0), -z).unwrap(), 200.0);
    close(k.section_area(b, p(0.0, 0.0, 31.0), z).unwrap(), 0.0);
    let s = ball(&mut k, p(0.0, 0.0, 0.0), 5.0);
    let tilted = Vector3::new(1.0, 2.0, 2.0) / 3.0;
    close(
        k.section_area(s, Point3::from(tilted * 3.0), tilted)
            .unwrap(),
        PI * 16.0,
    );
    // A ring through its axis: two disks of its tube.
    let t = ring(&mut k, 10.0, 3.0);
    close(
        k.section_area(t, Point3::origin(), Vector3::x()).unwrap(),
        2.0 * PI * 9.0,
    );
}

#[test]
fn betti_numbers() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(0.0, 0.0, 0.0), p(10.0, 10.0, 10.0));
    assert_eq!(k.betti_numbers(b).unwrap(), [1, 0, 0]);
    let s = ball(&mut k, p(5.0, 5.0, 5.0), 2.0);
    assert_eq!(k.betti_numbers(s).unwrap(), [1, 0, 0]);
    let c = cylinder(&mut k, p(5.0, 5.0, -1.0), 2.0, 12.0);
    assert_eq!(k.betti_numbers(c).unwrap(), [1, 0, 0]);
    let t = ring(&mut k, 10.0, 3.0);
    assert_eq!(k.betti_numbers(t).unwrap(), [1, 1, 0]);
    let hollow = one(k.boolean(BoolOp::Subtract, b, &[s]));
    assert_eq!(k.betti_numbers(hollow).unwrap(), [1, 0, 1]);
    let holed = one(k.boolean(BoolOp::Subtract, b, &[c]));
    assert_eq!(k.betti_numbers(holed).unwrap(), [1, 1, 0]);
    // Two blocks apart, then two touching along a face.
    let far = block(&mut k, p(20.0, 0.0, 0.0), p(30.0, 10.0, 10.0));
    let two = one(k.compound(&[b, far]));
    assert_eq!(k.betti_numbers(two).unwrap(), [2, 0, 0]);
    let near = block(&mut k, p(10.0, 0.0, 0.0), p(20.0, 10.0, 10.0));
    let touching = one(k.compound(&[b, near]));
    assert_eq!(k.betti_numbers(touching).unwrap(), [1, 0, 0]);
}

#[test]
fn interference() {
    let mut k = NativeKernel::new();
    let b = block(&mut k, p(0.0, 0.0, 0.0), p(10.0, 20.0, 30.0));
    let c = cylinder(&mut k, p(0.0, 0.0, 0.0), 5.0, 20.0);
    let m = k.interference(b, c).unwrap();
    close(m.volume, PI * 25.0 * 20.0 / 4.0);
    close(m.gap, 0.0);
    // Apart by 3.
    let s = ball(&mut k, p(15.0, 10.0, 10.0), 2.0);
    let m = k.interference(b, s).unwrap();
    close(m.volume, 0.0);
    close(m.gap, 3.0);
    // Touching along a face.
    let next = block(&mut k, p(10.0, 0.0, 0.0), p(20.0, 20.0, 30.0));
    let m = k.interference(b, next).unwrap();
    close(m.volume, 0.0);
    close(m.gap, 0.0);
    // Held inside: its whole volume.
    let inner = ball(&mut k, p(5.0, 10.0, 15.0), 2.0);
    let m = k.interference(b, inner).unwrap();
    close(m.volume, 4.0 / 3.0 * PI * 8.0);
    close(m.gap, 0.0);
}

#[test]
fn a_ring_is_bent_one_way_round_its_hole() {
    // On the inside of the tube, nearest the axis: saddle-shaped.
    let mut k = NativeKernel::new();
    let t = ring(&mut k, 10.0, 3.0);
    let q = Unit::new_normalize(Vector3::new(1.0, 1.0, 0.0)).into_inner() * 7.0;
    let got = k.curvatures_at(t, &[Point3::from(q)]).unwrap();
    bends(got[0], [1.0 / 3.0, -1.0 / 7.0]);
}

#[test]
fn curvature_inside_a_ball_near_its_axis() {
    // A ball keeps its axis as a free edge: a point inside near it is nearest the sphere.
    let mut k = NativeKernel::new();
    let s = ball(&mut k, p(0.0, 0.0, 0.0), 3.0);
    let got = k
        .curvatures_at(s, &[p(0.01, 0.0, 2.5), p(0.0, 0.0, 0.5)])
        .unwrap();
    for g in got {
        bends(g, [1.0 / 3.0, 1.0 / 3.0]);
    }
}

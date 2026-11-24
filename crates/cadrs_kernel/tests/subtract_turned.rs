//! `Kernel::subtract_turned`: a target less a tool and its copies turned about an axis, made as
//! one sector patterned round (`native::pattern`), against the copies cut one after another.

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point3, Unit, Vector3};

fn one(r: Result<OpResult>) -> BodyId {
    r.unwrap().bodies[0]
}

fn cylinder(k: &mut NativeKernel, base: Point3<f64>, r: f64, h: f64) -> BodyId {
    one(k.primitive(&Primitive::Cylinder {
        base,
        axis: Vector3::z_axis(),
        radius: r,
        height: h,
    }))
}

fn z_axis() -> Axis {
    Axis {
        origin: Point3::origin(),
        dir: Vector3::z_axis(),
    }
}

/// The tool's copies cut one after another, each a turn of TAU/count on.
fn one_by_one(k: &mut NativeKernel, target: BodyId, tool: BodyId, count: usize) -> BodyId {
    let mut at = target;
    for i in 0..count {
        let a = TAU * i as f64 / count as f64;
        let m = *nalgebra::Rotation3::from_axis_angle(&Vector3::z_axis(), a).matrix();
        let t = one(k.transform_affine(tool, m, Vector3::zeros()));
        at = one(k.boolean(BoolOp::Subtract, at, &[t]));
    }
    at
}

/// geomsolver's indexed pattern (brep-bench `boolean/indexed-pattern`): a disk of radius 20 and
/// thickness 5 with six holes of radius 2 on the circle of radius 12: 1880π, its area 1072π, its
/// faces the disk's three and the holes' six.
#[test]
fn six_holes_turned_round_a_disk() {
    let mut k = NativeKernel::new();
    let disk = cylinder(&mut k, Point3::new(0.0, 0.0, -5.0), 20.0, 5.0);
    let hole = cylinder(&mut k, Point3::new(12.0, 0.0, -6.0), 2.0, 7.0);
    let r = one(k.subtract_turned(disk, hole, z_axis(), 6));
    let b = k.get(r).unwrap();
    assert!(b.check().is_empty(), "{:?}", b.check());
    let m = k.mass_properties(r).unwrap();
    assert!(
        (m.volume - 1880.0 * PI).abs() < 1e-9 * m.volume,
        "{}",
        m.volume
    );
    assert!(
        (m.surface_area - 1072.0 * PI).abs() < 1e-9 * m.surface_area,
        "{}",
        m.surface_area
    );
    assert_eq!(b.faces().len(), 9);
}

/// Balls overlapping their neighbours and reaching past a pitch's wedge (24 of radius 3, their
/// centres 4.7 apart on the circle of radius 18, half sunk in the disk's rim): each sector is cut
/// by its own ball and its neighbours', and the patterned body is the one the balls cut one
/// after another make.
#[test]
fn overlapping_balls_turned_round_a_rim() {
    let mut k = NativeKernel::new();
    let disk = cylinder(&mut k, Point3::new(0.0, 0.0, -5.0), 20.0, 5.0);
    let ball = one(k.primitive(&Primitive::Sphere {
        center: Point3::new(18.0, 0.0, 0.0),
        radius: 3.0,
        axis: Unit::new_normalize(Vector3::new(0.3, 0.2, 1.0)),
        seam: Vector3::x_axis(),
    }));
    let r = one(k.subtract_turned(disk, ball, z_axis(), 24));
    let b = k.get(r).unwrap();
    assert!(b.check().is_empty(), "{:?}", b.check());
    let v = k.mass_properties(r).unwrap();
    let w = one_by_one(&mut k, disk, ball, 24);
    let u = k.mass_properties(w).unwrap();
    assert!(
        (v.volume - u.volume).abs() < 1e-8 * u.volume,
        "{} {}",
        v.volume,
        u.volume
    );
    assert!(
        (v.surface_area - u.surface_area).abs() < 1e-8 * u.surface_area,
        "{} {}",
        v.surface_area,
        u.surface_area
    );
    assert!((v.center_of_mass - u.center_of_mass).norm() < 1e-8);
}

//! Ported from springrep's tests/primitives.rs (MIT, https://github.com/springcraft-ai/springrep):
//! the five primitive solids against closed-form volume and area, each checked valid, on a frame
//! away from the origin and tilted off every axis. Here each is cadrs's `Primitive` made about the
//! world's axes (the box from the origin along them) and moved onto springrep's frame.

mod springrep_port;

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Matrix3, Rotation3, Translation3, UnitQuaternion};
use springrep_port::*;

fn tilted() -> Plane {
    frame(
        P3::new(3.0, -2.0, 1.5),
        V3::new(0.3, -0.5, 0.8),
        V3::new(1.0, 0.2, 0.0),
    )
}

/// The rigid motion taking the world's axes onto the frame's.
fn onto(f: &Plane) -> Transform {
    let m = Matrix3::from_columns(&[f.x_dir.into_inner(), y_of(f), f.normal.into_inner()]);
    let r = UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(m));
    Transform::from_parts(Translation3::from(f.origin.coords), r)
}

fn make(k: &mut NativeKernel, f: &Plane, p: Primitive) -> Result<BodyId> {
    let b = k.primitive(&p)?.first()?;
    k.transform(b, &onto(f))?.first()
}

fn pblock(k: &mut NativeKernel, f: &Plane, s: [f64; 3]) -> Result<BodyId> {
    let p = Primitive::Box {
        min: P3::origin(),
        max: p3(s),
    };
    make(k, f, p)
}

fn pcylinder(k: &mut NativeKernel, f: &Plane, r: f64, h: f64) -> Result<BodyId> {
    let p = Primitive::Cylinder {
        base: P3::origin(),
        axis: V3::z_axis(),
        radius: r,
        height: h,
    };
    make(k, f, p)
}

fn psphere(k: &mut NativeKernel, f: &Plane, r: f64) -> Result<BodyId> {
    let p = Primitive::Sphere {
        center: P3::origin(),
        radius: r,
        axis: V3::z_axis(),
        seam: V3::x_axis(),
    };
    make(k, f, p)
}

fn ptorus(k: &mut NativeKernel, f: &Plane, major: f64, minor: f64) -> Result<BodyId> {
    let p = Primitive::Torus {
        center: P3::origin(),
        axis: V3::z_axis(),
        major,
        minor,
    };
    make(k, f, p)
}

fn pcone(k: &mut NativeKernel, f: &Plane, r0: f64, r1: f64, h: f64) -> Result<BodyId> {
    let p = Primitive::Cone {
        base: P3::origin(),
        axis: V3::z_axis(),
        base_radius: r0,
        top_radius: r1,
        height: h,
    };
    make(k, f, p)
}

fn assert_prim(k: &NativeKernel, b: BodyId, vol: f64, ar: f64) {
    assert_solid_rel(k, b, vol, ar, 1e-10);
}

/// The centroid in the frame's own coordinates.
fn local_centroid(k: &NativeKernel, b: BodyId, f: &Plane) -> V3 {
    to_local(f, k.mass_properties(b).unwrap().center_of_mass)
}

fn assert_at(got: V3, want: [f64; 3]) {
    assert!((got - v3(want)).amax() < 1e-9, "{got:?}");
}

#[test]
fn a_block_runs_from_its_frames_origin_along_each_axis() {
    let mut k = NativeKernel::new();
    let f = tilted();
    let b = pblock(&mut k, &f, [3.0, 2.0, 4.0]).unwrap();
    assert_prim(&k, b, 24.0, 2.0 * (6.0 + 12.0 + 8.0));
    assert_at(local_centroid(&k, b, &f), [1.5, 1.0, 2.0]);
}

#[test]
fn a_cylinder_stands_on_its_frames_origin_about_z() {
    let mut k = NativeKernel::new();
    let f = tilted();
    let b = pcylinder(&mut k, &f, 1.5, 4.0).unwrap();
    assert_prim(
        &k,
        b,
        PI * 2.25 * 4.0,
        2.0 * PI * 2.25 + 2.0 * PI * 1.5 * 4.0,
    );
    assert_at(local_centroid(&k, b, &f), [0.0, 0.0, 2.0]);
}

#[test]
fn a_sphere_is_centred_on_its_frames_origin() {
    let mut k = NativeKernel::new();
    let f = tilted();
    let b = psphere(&mut k, &f, 2.0).unwrap();
    assert_prim(&k, b, 4.0 / 3.0 * PI * 8.0, 4.0 * PI * 4.0);
    assert_at(local_centroid(&k, b, &f), [0.0; 3]);
}

#[test]
fn a_torus_rings_its_frames_z_axis() {
    let mut k = NativeKernel::new();
    let f = tilted();
    let b = ptorus(&mut k, &f, 3.0, 0.75).unwrap();
    let tube = PI * 0.75 * 0.75;
    assert_prim(
        &k,
        b,
        tube * 2.0 * PI * 3.0,
        2.0 * PI * 0.75 * 2.0 * PI * 3.0,
    );
    assert_at(local_centroid(&k, b, &f), [0.0; 3]);
}

#[test]
fn a_cone_is_a_frustum_or_comes_to_an_apex_at_either_end() {
    let mut k = NativeKernel::new();
    let f = tilted();
    let frustum = |r0: f64, r1: f64, h: f64| {
        let slant = ((r0 - r1).powi(2) + h * h).sqrt();
        (
            PI * h / 3.0 * (r0 * r0 + r0 * r1 + r1 * r1),
            PI * (r0 * r0 + r1 * r1 + (r0 + r1) * slant),
        )
    };
    for (r0, r1) in [(2.0, 0.5), (2.0, 0.0), (0.0, 1.5)] {
        let b = pcone(&mut k, &f, r0, r1, 3.0).unwrap();
        let (v, a) = frustum(r0, r1, 3.0);
        assert_prim(&k, b, v, a);
    }
    // Equal radii are the frustum that is a cylinder.
    let b = pcone(&mut k, &f, 1.5, 1.5, 4.0).unwrap();
    assert_prim(
        &k,
        b,
        PI * 2.25 * 4.0,
        2.0 * PI * 2.25 + 2.0 * PI * 1.5 * 4.0,
    );
}

#[test]
fn a_size_at_or_below_zero_is_invalid_input() {
    // springrep refuses with `InvalidInput`; any refusal counts here.
    let mut k = NativeKernel::new();
    let f = tilted();
    let refused = |r: Result<BodyId>| r.is_err();
    assert!(refused(pblock(&mut k, &f, [3.0, 0.0, 4.0])), "block");
    assert!(refused(pcylinder(&mut k, &f, -1.0, 4.0)), "cylinder r -1");
    assert!(refused(pcylinder(&mut k, &f, 1.0, 0.0)), "cylinder h 0");
    assert!(refused(psphere(&mut k, &f, 0.0)), "sphere");
    assert!(refused(pcone(&mut k, &f, 0.0, 0.0, 3.0)), "cone 0 0");
    assert!(refused(pcone(&mut k, &f, 1.0, -0.5, 3.0)), "cone 1 -0.5");
    // A tube wider than its ring crosses the axis. (springrep refuses a horn torus too, whose
    // tube just reaches the axis; cadrs makes it: `Primitive::Torus`'s doc.)
    assert!(refused(ptorus(&mut k, &f, 1.0, 1.5)), "torus 1 1.5");
}

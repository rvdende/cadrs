//! Solids swept by a rigid motion, built exactly (no meshed stand-in).

use cadrs_kernel::native::*;
use cadrs_kernel::*;
use nalgebra::{Point3, Vector3};

/// brep-bench's bull-nose carried along a spline: a mill leaning 30 degrees toward its way, its
/// end's corner rounded, riding a planar Bézier. Its envelope is four fitted sheets (its
/// cylinder's two sides, its top rim's trailing half, its corner's leading half) closed by its
/// own faces at the two ends.
#[test]
fn a_leaning_bull_nose_carried_along_a_spline_is_exact() {
    let mut k = NativeKernel::new();
    let lean = 30f64.to_radians();
    let axis = Axis::new(
        Point3::new(0.0, 0.0, -1.0),
        Vector3::new(lean.sin(), 0.0, lean.cos()),
    );
    let section = Loop::from_point([0.0, 0.0])
        .line_to([1.5, 0.0])
        .conic_to([2.0, 0.0], 2f64.sqrt() / 2.0, [2.0, 0.5])
        .line_to([2.0, 3.0])
        .line_to([0.0, 3.0])
        .close();
    let sketch = Profile::of(Plane::through_axis(&axis), vec![section]);
    let mill = k
        .revolve_with(&sketch, &RevolveSpec::solid(axis, None))
        .unwrap()
        .first()
        .unwrap();
    let path = PathCurve::bezier(&[
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(20.0, 20.0, 0.0),
        Point3::new(30.0, 20.0, 0.0),
    ]);
    let motion = SolidMotion::along(vec![path], Vector3::z());
    let swept = k.sweep_solid(mill, &motion).unwrap().bodies[0];
    let faces = k.faces(swept).unwrap();
    assert_eq!(
        faces.len(),
        10,
        "four sheets, and the mill's faces at each end"
    );
    let faults = k.get(swept).unwrap().check();
    assert!(faults.is_empty(), "{faults:?}");
    // (Measured against the mill's own distance over the motion: every point of its mesh lies
    // within a ten-millionth of what the mill touches.)
    let volume = k.mass_properties(swept).unwrap().volume;
    assert!((volume - 619.91255).abs() < 1e-3, "volume {volume}");
}

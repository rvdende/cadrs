//! Solids swept by a rigid motion, built exactly (no meshed stand-in).

use cadrs_kernel::native::*;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

/// brep-bench's bull-nose carried along a spline: a mill leaning 30 degrees toward its way, its
/// end's corner rounded, riding a planar Bézier. Its envelope is four fitted sheets (its
/// cylinder's two sides, its top rim's trailing half, its corner's leading half) closed by its
/// own faces at the two ends.
#[test]
fn a_leaning_bull_nose_carried_along_a_spline_is_exact() {
    let mut k = NativeKernel::new();
    let lean = 30f64.to_radians();
    let a = Vector3::new(lean.sin(), 0.0, lean.cos());
    let c = Point3::new(0.0, 0.0, -1.0);
    let radial = (Vector3::x() - a * a.x).normalize();
    let on = Plane {
        origin: c,
        x_dir: nalgebra::Unit::new_normalize(radial),
        normal: nalgebra::Unit::new_normalize(radial.cross(&a)),
    };
    let l = |a: (f64, f64), b: (f64, f64)| Curve2::Line {
        a: Point2::new(a.0, a.1),
        b: Point2::new(b.0, b.1),
        source: None,
    };
    let section = Loop {
        curves: vec![
            l((0.0, 0.0), (1.5, 0.0)),
            Curve2::Spline {
                degree: 2,
                knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                poles: vec![
                    Point2::new(1.5, 0.0),
                    Point2::new(2.0, 0.0),
                    Point2::new(2.0, 0.5),
                ],
                weights: vec![1.0, 2f64.sqrt() / 2.0, 1.0],
                source: None,
            },
            l((2.0, 0.5), (2.0, 3.0)),
            l((2.0, 3.0), (0.0, 3.0)),
            l((0.0, 3.0), (0.0, 0.0)),
        ],
    };
    let axis = Axis {
        origin: c,
        dir: nalgebra::Unit::new_normalize(a),
    };
    let region = Region {
        outer: section,
        holes: vec![],
        source: None,
    };
    let mill = k
        .revolve_with(
            &Profile::new(on, vec![region]),
            &RevolveSpec::solid(axis, None),
        )
        .unwrap()
        .bodies[0];
    let motion = SolidMotion::Path {
        path: vec![PathCurve::Bezier3 {
            poles: [(0.0, 0.0), (10.0, 0.0), (20.0, 20.0), (30.0, 20.0)]
                .iter()
                .map(|&(x, y)| Point3::new(x, y, 0.0))
                .collect(),
            weights: vec![],
        }],
        up: Vector3::z_axis(),
    };
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

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

/// brep-bench's ring pushed in past its fold: a torus leaning 65 degrees from upright, slid
/// straight down more than its own height. Seen along the slide its outline folds, so the
/// sweeps of its leading faces cross one another; sewn together they made a solid 0.68 too
/// large (282.67). Its envelope, trimmed by what the torus covers of itself, is the solid.
#[test]
fn a_leaning_ring_slid_past_its_fold_is_not_overfilled() {
    let mut k = NativeKernel::new();
    let lean = 65f64.to_radians();
    let ring = k
        .primitive(&Primitive::Torus {
            center: Point3::new(0.0, 0.0, 4.0),
            axis: nalgebra::Unit::new_normalize(Vector3::new(lean.sin(), 0.0, lean.cos())),
            major: 3.0,
            minor: 1.0,
        })
        .unwrap()
        .bodies[0];
    let motion = SolidMotion::Rigid {
        steps: vec![RigidStep::Slide {
            from: Vector3::zeros(),
            to: Vector3::new(0.0, 0.0, -8.0),
        }],
    };
    let swept = k.sweep_solid(ring, &motion).unwrap().bodies[0];
    let faults = k.get(swept).unwrap().check();
    assert!(faults.is_empty(), "{faults:?}");
    // (Integrated numerically, column by column along the slide: 282.00 to the hundredth.)
    let volume = k.mass_properties(swept).unwrap().volume;
    assert!((volume - 282.0).abs() < 0.03, "volume {volume}");
}

/// A thread cut a lead at a time: a rod (a prism along the screw's axis) less a bar screwed
/// from short of it to past it. One lead's slab is cut and its copies glued, the last cut
/// short where the rod is no whole number of leads long; the solid is the one every half turn
/// cut in turn makes (464.45743, by that way).
#[test]
fn a_thread_cut_a_lead_at_a_time_is_the_thread() {
    let mut k = NativeKernel::new();
    let line = |a: (f64, f64), b: (f64, f64)| Curve2::Line {
        a: nalgebra::Point2::new(a.0, a.1),
        b: nalgebra::Point2::new(b.0, b.1),
        source: None,
    };
    let on = |z: f64| Plane {
        origin: Point3::new(0.0, 0.0, z),
        ..Plane::top()
    };
    let region = |outer: Loop| Region {
        outer,
        holes: vec![],
        source: None,
    };
    let round = |radius: f64| Loop {
        curves: vec![Curve2::Circle {
            center: nalgebra::Point2::new(0.0, 0.0),
            radius,
            source: None,
        }],
    };
    let bar = Loop {
        curves: vec![
            line((4.5, -8.0), (12.0, -8.0)),
            line((12.0, -8.0), (12.0, 8.0)),
            line((12.0, 8.0), (4.5, 8.0)),
            line((4.5, 8.0), (4.5, -8.0)),
        ],
    };
    let bar = k
        .extrude(
            &Profile::new(on(-0.45), vec![region(bar)]),
            Extent::Blind(0.9),
        )
        .unwrap()
        .bodies[0];
    let within = k
        .extrude(
            &Profile::new(on(-1.45), vec![region(round(5.5))]),
            Extent::Blind(2.9),
        )
        .unwrap()
        .bodies[0];
    let bar = k.boolean(BoolOp::Intersect, bar, &[within]).unwrap().bodies[0];
    let rod = k
        .extrude(
            &Profile::new(on(0.0), vec![region(round(5.0))]),
            Extent::Blind(7.3),
        )
        .unwrap()
        .bodies[0];
    let (lead, from, to) = (0.93, -std::f64::consts::TAU, 10.0 * std::f64::consts::TAU);
    let rise = |turn: f64| Vector3::z() * (lead * turn / std::f64::consts::TAU);
    let screw = SolidMotion::Rigid {
        steps: vec![
            RigidStep::Turn {
                axis: Axis {
                    origin: Point3::origin(),
                    dir: Vector3::z_axis(),
                },
                from,
                to,
            },
            RigidStep::Slide {
                from: rise(from),
                to: rise(to),
            },
        ],
    };
    let threaded = k.cut_swept(rod, bar, &screw).unwrap().bodies[0];
    let faults = k.get(threaded).unwrap().check();
    assert!(faults.is_empty(), "{faults:?}");
    let volume = k.mass_properties(threaded).unwrap().volume;
    assert!((volume - 464.457_43).abs() < 1e-5, "volume {volume}");
}

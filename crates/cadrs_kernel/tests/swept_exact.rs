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
            spindle: Default::default(),
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

/// brep-bench's tumbling torus crossed the other way: a torus turned about two axes at once,
/// whose core circle's sweep all but pinches once, so that one of its sheets has a hole cut out
/// (a band's own patch fills it). Its area is its faces' own: the sheet's fit inside that hole
/// follows nothing, and an integral carried across it came to 66.7837; the solid's triangles
/// come to 66.7483 as they are refined.
#[test]
fn a_sheet_with_a_hole_is_measured_inside_its_face() {
    let mut k = NativeKernel::new();
    let torus = k
        .primitive(&Primitive::torus(
            Point3::new(3.0, 0.0, 2.0),
            Vector3::z(),
            1.0,
            0.22,
        ))
        .unwrap()
        .first()
        .unwrap();
    let (lean, tilt) = (45f64.to_radians(), 20f64.to_radians());
    let (from, to) = ((-100f64).to_radians(), 100f64.to_radians());
    // (The case's own figure, four places of a root of a half.)
    let off = 7071.0 / 1e4;
    let motion = SolidMotion::rigid(vec![
        RigidStep::turn(
            Axis::new(Point3::new(2.0, 0.0, 0.0), Vector3::z()),
            0.7 * from,
            0.7 * to,
        ),
        RigidStep::turn(
            Axis::new(
                Point3::new(0.0, off * lean.cos(), off * lean.sin()),
                Vector3::new(tilt.cos(), tilt.sin() * lean.cos(), tilt.sin() * lean.sin()),
            ),
            from,
            to,
        ),
    ]);
    let swept = k.sweep_solid(torus, &motion).unwrap().first().unwrap();
    let m = k.mass_properties(swept).unwrap();
    assert!((m.volume - 13.789847).abs() < 1e-5, "volume {}", m.volume);
    assert!(
        (m.surface_area - 66.74839).abs() < 1e-4,
        "area {}",
        m.surface_area
    );
}

/// brep-bench's gear ladder: a blank's faces counted by kind, with its volume and what its
/// check finds.
fn kinds(
    k: &NativeKernel,
    id: BodyId,
) -> (
    std::collections::BTreeMap<&'static str, usize>,
    f64,
    Vec<String>,
) {
    let b = k.get(id).unwrap();
    let mut kinds = std::collections::BTreeMap::new();
    for f in b.faces() {
        let kind = match &f.surf {
            Surface::Plane { .. } => "plane",
            Surface::Cylinder { .. } => "cylinder",
            Surface::Cone { .. } => "cone",
            Surface::Nurbs(_) => "bspline",
            _ => "other",
        };
        *kinds.entry(kind).or_default() += 1;
    }
    (
        kinds,
        k.mass_properties(id).unwrap().volume,
        b.check_with(CheckLevel::Full),
    )
}

/// The gear ladder's single-start worm thread about the line through `at` along x, from
/// x = `from` to `to` (lead 2π, pitch radius 10, its axial section a rack's trapezoid of
/// module 2): swept along its helix about the z axis, cut to its ends, turned and placed.
fn worm_thread(k: &mut NativeKernel, at: Point3<f64>, from: f64, to: f64) -> BodyId {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let tan20 = 20f64.to_radians().tan();
    let w = |r: f64| FRAC_PI_2 - (r - 10.0) * tan20;
    let start = -TAU * ((-from / TAU).ceil() + 1.0);
    let mid = start - FRAC_PI_2;
    let section = [
        [7.5, mid - w(7.5)],
        [12.5, mid - w(12.5)],
        [12.5, mid + w(12.5)],
        [7.5, mid + w(7.5)],
    ];
    let profile = Profile::of(Plane::xz(0.0), vec![Loop::polygon(&section)]);
    let turns = ((to - start) / TAU).ceil() + 1.0;
    let path = Helix::new(
        Axis::new(Point3::new(0.0, 0.0, start), Vector3::z()),
        10.0,
        TAU,
        0.0,
        turns,
    )
    .path();
    let thread = k
        .sweep_with(&profile, &SweepSpec::solid(path))
        .unwrap()
        .first()
        .unwrap();
    let ends = Profile::of(
        Plane::square_to(Point3::new(0.0, 0.0, from), Vector3::z()),
        vec![Loop::circle([0.0, 0.0], 13.0)],
    );
    let ends = k
        .extrude_with(&ends, &ExtrudeSpec::normal_to(&ends, to - from))
        .unwrap()
        .first()
        .unwrap();
    let thread = k
        .boolean(BoolOp::Intersect, thread, &[ends])
        .unwrap()
        .first()
        .unwrap();
    let thread = k
        .transform_motion(
            thread,
            &Motion::rotation(&Axis::new(Point3::origin(), Vector3::y()), FRAC_PI_2),
        )
        .unwrap()
        .first()
        .unwrap();
    k.transform(thread, &Transform::translation(at.x, at.y, at.z))
        .unwrap()
        .first()
        .unwrap()
}

/// The worm turning thirty times about its axis while the gear turns once about the z axis.
fn hobbing(at: Point3<f64>) -> SolidMotion {
    use std::f64::consts::{PI, TAU};
    SolidMotion::rigid(vec![
        RigidStep::turn(Axis::new(at, Vector3::x()), 0.0, 60.0 * PI),
        RigidStep::turn(Axis::new(Point3::origin(), Vector3::z()), 0.0, -TAU),
    ])
}

/// brep-bench's straight bevel gear cut by its pinion: a 30-tooth gear's blank with one tooth
/// space cut by a straight-sided tooth of its 15-tooth pinion, the shafts meeting at the
/// origin. The space is five cones from the origin: the root the tooth's tip envelops, the
/// paths of the tip's two edges beside it, and beyond where each path runs into the flank's
/// envelope (the pinion's tip undercuts the flank it generates) that envelope. Its volume
/// agrees with the tooth's places sampled one by one (11804.1 ± 0.4); taking the tip's paths
/// alone for the space's sides left 27 too much.
/// brep-bench's bevel gear's blank and its pinion's tooth: the blank a quadrilateral turned
/// about the z axis; the tooth the pyramid with its apex at the origin over a rack's trapezoid,
/// between two planes square to its length.
fn bevel_blank_and_tooth(k: &mut NativeKernel) -> (BodyId, BodyId) {
    use std::f64::consts::FRAC_PI_2;
    let s5 = 5f64.sqrt();
    let (g, d) = (
        Vector3::new(2.0 / s5, 0.0, 1.0 / s5),
        Vector3::new(-1.0 / s5, 0.0, 2.0 / s5),
    );
    let z_axis = Axis::new(Point3::origin(), Vector3::z());
    let (ae, ai) = (15.0 * s5, 15.0 * s5 - 10.0);
    let at = |s: f64, u: f64| {
        let p = g * s + d * u;
        [p.x, p.z]
    };
    let quad = [
        at(ae, -2.0),
        at(ae, 6.0),
        at(ai, 6.0),
        at(ai, -2.0 * ai / ae),
    ];
    let blank = k
        .revolve_with(
            &Profile::of(Plane::xz(0.0), vec![Loop::polygon(&quad)]),
            &RevolveSpec::solid(z_axis, None),
        )
        .unwrap()
        .first()
        .unwrap();
    // The tooth: the pyramid with its apex at the origin over a rack's trapezoid, between two
    // planes square to its length.
    let tan20 = 20f64.to_radians().tan();
    let (inner, outer) = (ae - 12.0, ae + 2.0);
    let w = |u: f64| FRAC_PI_2 - u * tan20;
    let trapezoid = [
        (2.5, w(2.5)),
        (-2.5, w(-2.5)),
        (-2.5, -w(-2.5)),
        (2.5, -w(2.5)),
    ]
    .map(|(u, w)| [u * outer / ae, w * outer / ae]);
    let pyramid = k
        .loft_with(&LoftSpec {
            body: BodyKind::Solid,
            sections: vec![
                LoftSection::Profile(Profile::of(
                    Plane::new(Point3::origin() + g * outer, d, g),
                    vec![Loop::polygon(&trapezoid)],
                )),
                LoftSection::Point(Point3::origin()),
            ],
            start: LoftEnd::default(),
            end: LoftEnd::default(),
            source: 1,
        })
        .unwrap()
        .first()
        .unwrap();
    let slab = Profile::of(
        Plane::new(Point3::origin() + g * inner, d, g),
        vec![Loop::rectangle(-10.0, -10.0, 10.0, 10.0)],
    );
    let slab = k
        .extrude_with(&slab, &ExtrudeSpec::normal_to(&slab, 15.0))
        .unwrap()
        .first()
        .unwrap();
    let tooth = k
        .boolean(BoolOp::Intersect, pyramid, &[slab])
        .unwrap()
        .first()
        .unwrap();
    (blank, tooth)
}

#[test]
fn a_bevel_gear_cut_by_its_pinion_has_the_flanks_its_tooth_generates() {
    let mut k = NativeKernel::new();
    let (blank, tooth) = bevel_blank_and_tooth(&mut k);
    let z_axis = Axis::new(Point3::origin(), Vector3::z());
    let roll = 60f64.to_radians();
    let motion = SolidMotion::rigid(vec![
        RigidStep::turn(Axis::new(Point3::origin(), Vector3::x()), -roll, roll),
        RigidStep::turn(z_axis, -0.5 * roll, 0.5 * roll),
    ]);
    let cut = k.cut_swept(blank, tooth, &motion).unwrap().first().unwrap();
    let (kinds, volume, faults) = kinds(&k, cut);
    assert_eq!(kinds.get("bspline"), Some(&5), "{kinds:?}");
    assert_eq!(kinds.get("cone"), Some(&4), "{kinds:?}");
    assert_eq!(kinds.get("plane"), None, "{kinds:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert!((volume - 11803.8045).abs() < 1e-3, "volume {volume}");
}

/// brep-bench's hypoid gear cut by its pinion: the bevel gear's blank, and its tooth turned
/// about the gear's axis to lie along the pitch cone where a pinion on a shaft 12 from the
/// gear's can carry it. The tooth's two end faces and its corners come into the cut at both
/// ends of the tooth space, so the space's outline changes along it within the blank, a dozen
/// times: it is made in as many stretches under a roof, their links that come and go each a
/// three-sided face. Its volume agrees with the tooth's places sampled one by one
/// (11791.7 ± 0.7); the tooth swept as a mesh of flat facets gave 11717.3.
#[test]
#[ignore = "slow: about 3 s"]
fn a_hypoid_gear_cut_by_its_pinion_has_a_space_whose_outline_changes_along_it() {
    let mut k = NativeKernel::new();
    let (blank, tooth) = bevel_blank_and_tooth(&mut k);
    let z_axis = Axis::new(Point3::origin(), Vector3::z());
    let phi = (24.0 / (75.0 - 5.0 * 5f64.sqrt())).asin();
    let tooth = k
        .transform_motion(tooth, &Motion::rotation(&z_axis, phi))
        .unwrap()
        .first()
        .unwrap();
    let roll = 70f64.to_radians();
    let motion = SolidMotion::rigid(vec![
        RigidStep::turn(
            Axis::new(Point3::new(0.0, 12.0, 0.0), Vector3::x()),
            -roll,
            roll,
        ),
        RigidStep::turn(z_axis, -7.0 / 15.0 * roll, 7.0 / 15.0 * roll),
    ]);
    let cut = k.cut_swept(blank, tooth, &motion).unwrap().first().unwrap();
    let (kinds, volume, faults) = kinds(&k, cut);
    assert_eq!(kinds.get("cone"), Some(&4), "{kinds:?}");
    assert_eq!(kinds.get("plane"), None, "{kinds:?}");
    assert!(kinds.get("bspline").is_some_and(|n| *n >= 60), "{kinds:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert!((volume - 11792.2465).abs() < 1e-2, "volume {volume}");
}

/// brep-bench's worm wheel hobbed by its worm: thirty tooth spaces, each two flanks the
/// thread's flanks envelop, the root its crest envelops and the two fillets its crest's edges
/// trace: five faces a space, one plug made and its thirty copies cut at once. Its volume
/// agrees with the thread's places sampled one by one (34452 ± 14).
#[test]
fn a_worm_wheel_hobbed_by_its_worm_has_thirty_spaces_of_five_faces() {
    let mut k = NativeKernel::new();
    let wheel = Profile::of(
        Plane::square_to(Point3::new(0.0, 0.0, -6.0), Vector3::z()),
        vec![Loop::circle([0.0, 0.0], 32.0)],
    );
    let wheel = k
        .extrude_with(&wheel, &ExtrudeSpec::normal_to(&wheel, 12.0))
        .unwrap()
        .first()
        .unwrap();
    let at = Point3::new(0.0, 40.0, 0.0);
    let thread = worm_thread(&mut k, at, -20.0, 20.0);
    let cut = k
        .cut_swept(wheel, thread, &hobbing(at))
        .unwrap()
        .first()
        .unwrap();
    let (kinds, volume, faults) = kinds(&k, cut);
    assert_eq!(kinds.get("bspline"), Some(&150), "{kinds:?}");
    assert_eq!(kinds.get("cylinder"), Some(&30), "{kinds:?}");
    assert_eq!(kinds.get("plane"), Some(&2), "{kinds:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert!((volume - 34429.3498).abs() < 1e-2, "volume {volume}");
}

/// brep-bench's helicon face gear hobbed by its worm: the worm over a ring's face. Each tooth
/// space has one side the thread's flank envelops and the other the path of its crest's edge
/// alone (that flank's envelope lies under it): four faces a space.
#[test]
#[ignore = "slow: about 3 s"]
fn a_face_gear_hobbed_by_its_worm_has_thirty_spaces_of_four_faces() {
    let mut k = NativeKernel::new();
    let ring = Profile::with_holes(
        Plane::xy(-6.0),
        Loop::circle([0.0, 0.0], 52.0),
        vec![Loop::circle([0.0, 0.0], 40.0)],
    );
    let ring = k
        .extrude_with(&ring, &ExtrudeSpec::normal_to(&ring, 8.0))
        .unwrap()
        .first()
        .unwrap();
    let at = Point3::new(0.0, 30.0, 10.0);
    let thread = worm_thread(&mut k, at, 0.0, 52.0);
    let cut = k
        .cut_swept(ring, thread, &hobbing(at))
        .unwrap()
        .first()
        .unwrap();
    let (kinds, volume, faults) = kinds(&k, cut);
    assert_eq!(kinds.get("bspline"), Some(&120), "{kinds:?}");
    assert_eq!(kinds.get("cylinder"), Some(&2), "{kinds:?}");
    assert_eq!(kinds.get("plane"), Some(&31), "{kinds:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert!((volume - 18610.424).abs() < 2e-2, "volume {volume}");
}

/// The spiroid's tapered worm thread about the line through `at` along x, from x = `from` to
/// `to`: lead 2π, its pitch radius 6 + x tan 5° at x, its section in the half plane through
/// the axis a trapezoid between the cones 2.5 inside and outside the pitch cone, carried along
/// the spiral on the pitch cone by its screw ([`SweepControl::Helix`]).
fn tapered_thread(k: &mut NativeKernel, at: Point3<f64>, from: f64, to: f64) -> BodyId {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let (tan5, tan20) = (5f64.to_radians().tan(), 20f64.to_radians().tan());
    let pitch = |z: f64| 6.0 + z * tan5;
    let w = |u: f64| FRAC_PI_2 - u * tan20;
    let start = -TAU * ((-from / TAU).ceil() + 1.0);
    let mid = start - FRAC_PI_2;
    let corner = |u: f64, side: f64| {
        let z = mid + side * w(u);
        [pitch(z) + u, z]
    };
    let section = [
        corner(-2.5, -1.0),
        corner(2.5, -1.0),
        corner(2.5, 1.0),
        corner(-2.5, 1.0),
    ];
    let profile = Profile::of(Plane::xz(0.0), vec![Loop::polygon(&section)]);
    let turns = ((to - start) / TAU).ceil() + 1.0;
    let spiral = Helix::conical(
        Axis::new(Point3::new(0.0, 0.0, mid), Vector3::z()),
        pitch(mid),
        pitch(mid + TAU * turns),
        TAU * turns,
        0.0,
        turns,
    );
    let mut spec = SweepSpec::solid(spiral.path());
    spec.control = SweepControl::Helix(spiral);
    let thread = k.sweep_with(&profile, &spec).unwrap().first().unwrap();
    let ends = Profile::of(
        Plane::square_to(Point3::new(0.0, 0.0, from), Vector3::z()),
        vec![Loop::circle([0.0, 0.0], 20.0)],
    );
    let ends = k
        .extrude_with(&ends, &ExtrudeSpec::normal_to(&ends, to - from))
        .unwrap()
        .first()
        .unwrap();
    let thread = k
        .boolean(BoolOp::Intersect, thread, &[ends])
        .unwrap()
        .first()
        .unwrap();
    let thread = k
        .transform_motion(
            thread,
            &Motion::rotation(&Axis::new(Point3::origin(), Vector3::y()), FRAC_PI_2),
        )
        .unwrap()
        .first()
        .unwrap();
    k.transform(thread, &Transform::translation(at.x, at.y, at.z))
        .unwrap()
        .first()
        .unwrap()
}

/// brep-bench's spiroid face gear hobbed by its worm: a tapered worm over a ring whose face is
/// a cone. Toward the ring's inside a tooth space is as the face gear's (a flank's envelope,
/// the root and the path of the crest's other edge: four faces); toward its outside the other
/// flank's envelope comes out from under that edge's path, a fifth face. The chain of the
/// space's outline has that link only where it is within the ring, so the space is made in two
/// stretches: nine faces a space.
#[test]
#[ignore = "slow: about 3 s"]
fn a_spiroid_face_gear_hobbed_by_its_worm_has_thirty_spaces_of_nine_faces() {
    let mut k = NativeKernel::new();
    let quad = [[40.0, -9.0], [52.0, -9.0], [52.0, -1.7], [40.0, -0.3]];
    let quad = Profile::of(Plane::xz(0.0), vec![Loop::polygon(&quad)]);
    let ring = k
        .revolve_with(
            &quad,
            &RevolveSpec::solid(Axis::new(Point3::origin(), Vector3::z()), None),
        )
        .unwrap()
        .first()
        .unwrap();
    let at = Point3::new(0.0, 30.0, 6.0);
    let thread = tapered_thread(&mut k, at, 0.0, 52.0);
    let cut = k
        .cut_swept(ring, thread, &hobbing(at))
        .unwrap()
        .first()
        .unwrap();
    let (kinds, volume, faults) = kinds(&k, cut);
    assert_eq!(kinds.get("bspline"), Some(&270), "{kinds:?}");
    assert_eq!(kinds.get("cone"), Some(&30), "{kinds:?}");
    assert_eq!(kinds.get("cylinder"), Some(&2), "{kinds:?}");
    assert_eq!(kinds.get("plane"), Some(&1), "{kinds:?}");
    assert!(faults.is_empty(), "{faults:?}");
    assert!((volume - 19123.8203).abs() < 2e-2, "volume {volume}");
}

/// A thread swept along its helix is made in pieces, each a quintic through sections given
/// with their exact first and second derivatives: the pieces lie on the thread's own helicoids
/// to a hundred-millionth, and where two meet their faces' normals agree (the cubic skins they
/// were before left the flank's normal a hundred-thousandth of a radian apart there, which a
/// contact followed over the flank felt as a step).
#[test]
fn a_thread_swept_along_its_helix_is_smooth_where_its_pieces_meet() {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let mut k = NativeKernel::new();
    let tan20 = 20f64.to_radians().tan();
    let w = |r: f64| FRAC_PI_2 - (r - 10.0) * tan20;
    let section = [
        [7.5, -w(7.5)],
        [12.5, -w(12.5)],
        [12.5, w(12.5)],
        [7.5, w(7.5)],
    ];
    let profile = Profile::of(Plane::xz(0.0), vec![Loop::polygon(&section)]);
    let path = Helix::new(
        Axis::new(Point3::origin(), Vector3::z()),
        10.0,
        TAU,
        0.0,
        2.0,
    )
    .path();
    let thread = k
        .sweep_with(&profile, &SweepSpec::solid(path))
        .unwrap()
        .first()
        .unwrap();
    let b = k.get(thread).unwrap();
    // Every curved face's points on the thread: at angle θ and radius r from the axis, its
    // height θ plus or minus the flank's half width there, or its radius the crest's or the
    // root's.
    let mut worst: f64 = 0.0;
    for f in b.faces() {
        let Surface::Nurbs(n) = &f.surf else {
            continue;
        };
        let ((u0, u1), (v0, v1)) = n.range();
        for i in 0..=8 {
            for j in 0..=8 {
                let p = n.point(
                    u0 + (u1 - u0) * i as f64 / 8.0,
                    v0 + (v1 - v0) * j as f64 / 8.0,
                );
                let (r, th) = (p.x.hypot(p.y), p.y.atan2(p.x));
                let turn = ((p.z - th) / TAU).round();
                let xi = p.z - th - TAU * turn;
                let off = (xi.abs() - w(r))
                    .abs()
                    .min((r - 7.5).abs())
                    .min((r - 12.5).abs());
                worst = worst.max(off);
            }
        }
    }
    assert!(worst < 1e-8, "a face's point is {worst:e} off the thread");
    // Where two faces share an edge and neither is flat, their normals along it: the same, or
    // a sharp corner's apart.
    let mut smoothest: f64 = 1.0;
    let mut roughest: f64 = 0.0;
    for (e, faces) in b.edge_faces().iter().enumerate() {
        let [fa, fb] = faces else {
            continue;
        };
        let (fa, fb) = (&b.faces()[*fa], &b.faces()[*fb]);
        let edge = &b.edges()[e];
        for i in 1..8 {
            let p = edge.point(edge.t[0] + (edge.t[1] - edge.t[0]) * i as f64 / 8.0);
            let normal = |f: &Face| {
                let (u, v) = f.surf.project(&p, None);
                f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 }
            };
            let bend = normal(fa).cross(&normal(fb)).norm();
            if bend < 1e-2 {
                roughest = roughest.max(bend);
            } else {
                smoothest = smoothest.min(bend);
            }
        }
    }
    assert!(
        roughest < 1e-9,
        "faces meant to join smoothly bend by {roughest:e}"
    );
    assert!(smoothest > 0.3, "a corner of only {smoothest}");
}

/// A tapered thread ([`SweepControl::Helix`] along a spiral on a cone): its section, drawn in a
/// plane through the axis, stays in one as it is carried round, moving out as the cone widens.
/// Every face's points lie on the thread as brep-bench's spiroid worm describes it: between
/// the cones 2.5 inside and outside the pitch cone, its sides 20° from square to the axis.
#[test]
fn a_tapered_thread_keeps_its_section_in_the_plane_through_its_axis() {
    use std::f64::consts::{FRAC_PI_2, TAU};
    let mut k = NativeKernel::new();
    let (tan5, tan20) = (5f64.to_radians().tan(), 20f64.to_radians().tan());
    let pitch = |z: f64| 6.0 + z * tan5;
    let w = |u: f64| FRAC_PI_2 - u * tan20;
    let corner = |u: f64, side: f64| {
        let z = side * w(u);
        [pitch(z) + u, z]
    };
    let section = [
        corner(-2.5, -1.0),
        corner(2.5, -1.0),
        corner(2.5, 1.0),
        corner(-2.5, 1.0),
    ];
    let profile = Profile::of(Plane::xz(0.0), vec![Loop::polygon(&section)]);
    let turns = 3.0;
    let spiral = Helix::conical(
        Axis::new(Point3::origin(), Vector3::z()),
        pitch(0.0),
        pitch(TAU * turns),
        TAU * turns,
        0.0,
        turns,
    );
    let mut spec = SweepSpec::solid(spiral.path());
    spec.control = SweepControl::Helix(spiral);
    let thread = k.sweep_with(&profile, &spec).unwrap().first().unwrap();
    let b = k.get(thread).unwrap();
    assert!(b.check().is_empty(), "{:?}", b.check());
    let mut worst: f64 = 0.0;
    for f in b.faces() {
        let Surface::Nurbs(n) = &f.surf else {
            continue;
        };
        let ((u0, u1), (v0, v1)) = n.range();
        for i in 0..=8 {
            for j in 0..=8 {
                let p = n.point(
                    u0 + (u1 - u0) * i as f64 / 8.0,
                    v0 + (v1 - v0) * j as f64 / 8.0,
                );
                let (r, th) = (p.x.hypot(p.y), p.y.atan2(p.x));
                let u = r - pitch(p.z);
                let turn = ((p.z - th) / TAU).round();
                let xi = p.z - th - TAU * turn;
                worst = worst.max((xi.abs() - w(u)).abs().min((u.abs() - 2.5).abs()));
            }
        }
    }
    assert!(worst < 1e-8, "a face's point is {worst:e} off the thread");
}

/// brep-bench's ball screwed round a tight helix: a ball of radius 2 whose centre runs two
/// turns of a helix of radius 1.5 rising 3 a turn. The helix curves tighter than the ball
/// (its radius of curvature is 1.65), so the tube about it folds, and its turns overlap. The
/// solid is everything within 2 of the helix's arc: its faces lie on the tube and on the ball
/// at each end, and every point of them is 2 from the arc.
#[test]
fn a_ball_screwed_round_a_tight_helix_is_exact() {
    use std::f64::consts::TAU;
    let mut k = NativeKernel::new();
    let ball = k
        .primitive(&Primitive::ball(Point3::new(1.5, 0.0, 0.0), 2.0))
        .unwrap()
        .first()
        .unwrap();
    let axis = Axis::new(Point3::origin(), Vector3::z());
    let motion = SolidMotion::helical(axis, 3.0, 0.0, 2.0 * TAU);
    let swept = k.sweep_solid(ball, &motion).unwrap().bodies[0];
    let body = k.get(swept).unwrap();
    assert!(
        body.faces
            .iter()
            .all(|f| !matches!(f.surf, Surface::Plane { .. })),
        "the solid is not a faceted stand-in"
    );
    assert!(body.faces.len() < 100, "{} faces", body.faces.len());
    let faults = body.check();
    assert!(faults.is_empty(), "{faults:?}");
    let volume = k.mass_properties(swept).unwrap().volume;
    assert!((volume - 253.4494).abs() < 1e-3, "volume {volume}");
    // Every point of its mesh is the ball's radius from the helix's arc (the mesh's corners
    // are on the faces).
    let mesh = k
        .tessellate(
            swept,
            Tessellation {
                deflection: 0.01,
                angle: 15f64.to_radians(),
            },
        )
        .unwrap();
    let helix = |w: f64| Point3::new(1.5 * w.cos(), 1.5 * w.sin(), 3.0 * w / TAU);
    let mut off: f64 = 0.0;
    for p in &mesh.positions {
        let p = Point3::new(p[0], p[1], p[2]);
        let from_arc = (0..=20_000)
            .map(|i| (helix(2.0 * TAU * i as f64 / 20_000.0) - p).norm())
            .fold(f64::INFINITY, f64::min);
        off = off.max((from_arc - 2.0).abs());
    }
    assert!(off < 1e-5, "a point of the surface is {off} off");
}

/// A ring (radii 6 and 2) moved in its own plane, in closed form: everything within 2 of the
/// flat region its core circle sweeps. Past its hole (a slide of 15) that region is the stadium of the circle's
/// disc, and the solid's volume Steiner's 2rA + πr²P/2 + 4πr³/3 for a flat convex region of
/// area A and perimeter P; short of it (a slide of 5) the region has the lens the two ends'
/// discs share for a hole, and every point of the solid's faces is 2 from it. Analytic faces
/// only, and few.
#[test]
fn a_ring_moved_in_its_plane_is_exact() {
    use std::f64::consts::PI;
    let (big, small) = (6.0f64, 2.0f64);
    let slid = |len: f64| {
        let mut k = NativeKernel::new();
        let ring = k
            .primitive(&Primitive::torus(
                Point3::origin(),
                Vector3::z(),
                big,
                small,
            ))
            .unwrap()
            .first()
            .unwrap();
        let swept = k
            .sweep_solid(ring, &SolidMotion::slide(Vector3::new(len, 0.0, 0.0)))
            .unwrap()
            .bodies[0];
        let body = k.get(swept).unwrap();
        let faults = body.check();
        assert!(faults.is_empty(), "{faults:?}");
        assert!(
            body.faces.len() <= 16
                && body
                    .faces
                    .iter()
                    .all(|f| !matches!(f.surf, Surface::Nurbs(_))),
            "{} faces",
            body.faces.len()
        );
        (k, swept)
    };
    let (k, past) = slid(15.0);
    let (area, perimeter) = (PI * big * big + 2.0 * big * 15.0, 2.0 * PI * big + 30.0);
    let want =
        2.0 * small * area + PI * small * small * perimeter / 2.0 + 4.0 * PI * small.powi(3) / 3.0;
    let volume = k.mass_properties(past).unwrap().volume;
    assert!((volume - want).abs() < 1e-8 * want, "{volume} {want}");
    // Turned about an axis parallel to its own (a ring of radii 1 and 0.4, ten from the z
    // axis, from −140° to 30°): the band is bent round the axis, and the same formula holds
    // (its inner edge curves the other way, no tighter than the tube is thick).
    {
        let (big, small, reach) = (1.0f64, 0.4f64, 10.0f64);
        let turn = 170f64.to_radians();
        let mut k = NativeKernel::new();
        let ring = k
            .primitive(&Primitive::torus(
                Point3::new(reach, 0.0, 0.0),
                Vector3::z(),
                big,
                small,
            ))
            .unwrap()
            .first()
            .unwrap();
        let axis = Axis::new(Point3::origin(), Vector3::z());
        let swept = k
            .sweep_solid(
                ring,
                &SolidMotion::swing(axis, -140f64.to_radians(), 30f64.to_radians()),
            )
            .unwrap()
            .bodies[0];
        let body = k.get(swept).unwrap();
        let faults = body.check();
        assert!(faults.is_empty(), "{faults:?}");
        assert!(
            body.faces
                .iter()
                .all(|f| !matches!(f.surf, Surface::Nurbs(_))),
            "an analytic solid"
        );
        let (area, perimeter) = (
            2.0 * big * reach * turn + PI * big * big,
            2.0 * reach * turn + 2.0 * PI * big,
        );
        let want = 2.0 * small * area
            + PI * small * small * perimeter / 2.0
            + 4.0 * PI * small.powi(3) / 3.0;
        let volume = k.mass_properties(swept).unwrap().volume;
        assert!((volume - want).abs() < 1e-8 * want, "{volume} {want}");
    }
    // Short of its hole: each mesh corner's distance from the region.
    let len = 5.0;
    let (k, short) = slid(len);
    let mesh = k
        .tessellate(
            short,
            Tessellation {
                deflection: 0.01,
                angle: 15f64.to_radians(),
            },
        )
        .unwrap();
    let ends = [(0.0f64, 0.0f64), (len, 0.0)];
    let corner = (len / 2.0, (big * big - len * len / 4.0).sqrt());
    let from_region = |x: f64, y: f64| -> f64 {
        let to = |c: (f64, f64)| (x - c.0).hypot(y - c.1);
        let from_segment = (x - x.clamp(0.0, len)).hypot(y);
        if from_segment > big {
            return from_segment - big;
        }
        if to(ends[0]) >= big || to(ends[1]) >= big {
            return 0.0;
        }
        // In the lens: its nearest arc, where the foot is on the lens's side of the other
        // circle, else a corner.
        let mut least = (x - corner.0).hypot(y.abs() - corner.1);
        for (c, other) in [(ends[0], ends[1]), (ends[1], ends[0])] {
            let d = to(c);
            if d > 1e-12 {
                let foot = (c.0 + (x - c.0) * big / d, c.1 + (y - c.1) * big / d);
                if (foot.0 - other.0).hypot(foot.1 - other.1) <= big {
                    least = least.min(big - d);
                }
            }
        }
        least
    };
    let mut off: f64 = 0.0;
    for p in &mesh.positions {
        let flat = from_region(p[0], p[1]);
        off = off.max((flat.hypot(p[2]) - small).abs());
    }
    assert!(off < 1e-5, "a point of the surface is {off} off");
}

/// A block turned whole turns about an axis clear of it, in closed form: the solid of revolution
/// of its shadow in (distance, height). Standing along the axis (4 ≤ x ≤ 7, |y| ≤ 2, 0 ≤ z ≤ 5
/// about z) the shadow is a rectangle, from the nearest face (4) out to the farthest edges
/// (√53): π (53 − 16) 5, and its faces two cylinders and two flat rings. Leaning (turned 20°
/// about y first, then 5° about x) its outer side is hyperboloids, its edges' sweeps, and its
/// inner side cones and hyperboloids: its volume the shadow's, the heights sliced and at each
/// the box's nearest and farthest points from the axis found by a search of its own.
#[test]
fn a_block_turned_whole_turns_is_exact() {
    use std::f64::consts::PI;
    let z = Axis::new(Point3::origin(), Vector3::z());
    let turned = |k: &mut NativeKernel, body: BodyId| {
        let swept = k
            .sweep_solid(body, &SolidMotion::Turn { axis: z })
            .unwrap()
            .bodies[0];
        let faults = k.get(swept).unwrap().check();
        assert!(faults.is_empty(), "{faults:?}");
        swept
    };
    let block = |k: &mut NativeKernel| {
        k.primitive(&Primitive::block(
            Point3::new(4.0, -2.0, 0.0),
            Point3::new(7.0, 2.0, 5.0),
        ))
        .unwrap()
        .first()
        .unwrap()
    };
    let mut k = NativeKernel::new();
    let upright = block(&mut k);
    let swept = turned(&mut k, upright);
    assert_eq!(k.get(swept).unwrap().faces.len(), 4);
    let want = PI * (53.0 - 16.0) * 5.0;
    let volume = k.mass_properties(swept).unwrap().volume;
    assert!((volume - want).abs() < 1e-9 * want, "{volume} {want}");
    // Leaning.
    let about = |dir: Vector3<f64>, angle: f64| {
        Transform::rotation_wrt_point(
            nalgebra::UnitQuaternion::from_axis_angle(&nalgebra::Unit::new_normalize(dir), angle),
            Point3::new(5.5, 0.0, 2.5),
        )
    };
    let lean = [
        about(Vector3::y(), 20f64.to_radians()),
        about(Vector3::x(), 5f64.to_radians()),
    ];
    let mut body = block(&mut k);
    for t in &lean {
        body = k.transform(body, t).unwrap().bodies[0];
    }
    let corners: Vec<Point3<f64>> = k.get(body).unwrap().verts.iter().map(|v| v.p).collect();
    assert_eq!(corners.len(), 8);
    let swept = turned(&mut k, body);
    assert!(
        k.get(swept)
            .unwrap()
            .faces
            .iter()
            .all(|f| !matches!(f.surf, Surface::Nurbs(_))),
        "its faces are the lines' own sweeps"
    );
    // The shadow by slices: at each height the box's section is the hull of where its edges
    // cross it; the farthest of those points from the axis, and the nearest point of the hull.
    let edges: Vec<(Point3<f64>, Point3<f64>)> = {
        let b = k.get(body).unwrap();
        b.edges
            .iter()
            .map(|e| (b.verts[e.v[0]].p, b.verts[e.v[1]].p))
            .collect()
    };
    assert_eq!(edges.len(), 12);
    let (lo, hi) = corners
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
            (a.min(p.z), b.max(p.z))
        });
    let n = 4000;
    let mut want = 0.0;
    for s in 0..n {
        let h = lo + (hi - lo) * (s as f64 + 0.5) / n as f64;
        let pts: Vec<(f64, f64)> = edges
            .iter()
            .filter(|(a, b)| (a.z - h) * (b.z - h) < 0.0)
            .map(|(a, b)| {
                let t = (h - a.z) / (b.z - a.z);
                (a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t)
            })
            .collect();
        let far = pts.iter().map(|p| p.0.hypot(p.1)).fold(0.0, f64::max);
        // The nearest point of the hull: of every segment between two of the points (the
        // hull's sides are among them, and the axis is outside it).
        let mut near = f64::INFINITY;
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                let (dx, dy) = (b.0 - a.0, b.1 - a.1);
                let t = (-(a.0 * dx + a.1 * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                near = near.min((a.0 + dx * t).hypot(a.1 + dy * t));
            }
        }
        want += PI * (far * far - near * near) * (hi - lo) / n as f64;
    }
    let volume = k.mass_properties(swept).unwrap().volume;
    assert!((volume - want).abs() < 1e-5 * want, "{volume} {want}");
}

/// A flat end mill (Ø1, pointing at the bar's axis, its tip 1 into a bar of radius 10)
/// screwed half a turn round the bar, rising 6 a turn along it: a rotary roughing pass, as
/// CAM's exact simulation cuts it. Its swept pieces are made exactly; cut from the bar they
/// once left an open shell, and the cut fell back to a mesh (tens of thousands of facets,
/// 18 s). The cut is exact, sound, and removes the volume the screw's places sweep in the bar
/// (integrated here cell by cell), the same after a plunge to the depth first (the tool
/// starting in its own hole).
#[test]
fn a_flat_end_mill_screwed_half_round_a_bar_is_cut_exactly() {
    use std::f64::consts::{PI, TAU};
    // The volume of the bar the screw's places cover: cylinder cells (ρ, φ, x) over where the
    // groove can be, each in if some place of the mill holds its centre.
    let lead = 6.0;
    let inside = |x: f64, y: f64, z: f64| {
        // The places with the point within the mill's x span (20 ± 0.5 once slid back).
        let (t0, t1) = (
            ((x - 20.5) * TAU / lead).max(0.0),
            ((x - 19.5) * TAU / lead).min(PI),
        );
        (t1 >= t0)
            && (0..=96).any(|i| {
                let t = t0 + (t1 - t0) * i as f64 / 96.0;
                let qx = x - lead * t / TAU;
                let (qy, qz) = (y * t.cos() + z * t.sin(), -y * t.sin() + z * t.cos());
                (qx - 20.0).powi(2) + qy * qy <= 0.25 && qz >= 9.0
            })
    };
    let (nr, nphi, nx) = (24, 900, 120);
    let (phi0, phi1, x0, x1) = (-0.1, PI + 0.1, 19.4, 23.6);
    let (dr, dphi, dx) = (
        1.0 / nr as f64,
        (phi1 - phi0) / nphi as f64,
        (x1 - x0) / nx as f64,
    );
    let mut expected = 0.0;
    for i in 0..nr {
        let r = 9.0 + (i as f64 + 0.5) * dr;
        for j in 0..nphi {
            let phi = phi0 + (j as f64 + 0.5) * dphi;
            let (y, z) = (-r * phi.sin(), r * phi.cos());
            for l in 0..nx {
                if inside(x0 + (l as f64 + 0.5) * dx, y, z) {
                    expected += r * dr * dphi * dx;
                }
            }
        }
    }
    for plunge in [false, true] {
        let mut k = NativeKernel::new();
        let bar = k
            .primitive(&Primitive::cylinder(
                Point3::origin(),
                Vector3::x(),
                10.0,
                60.0,
            ))
            .unwrap()
            .first()
            .unwrap();
        let mill = k
            .primitive(&Primitive::cylinder(
                Point3::origin(),
                Vector3::z(),
                0.5,
                6.0,
            ))
            .unwrap()
            .first()
            .unwrap();
        let at = |k: &mut NativeKernel, z: f64| {
            k.transform(mill, &Transform::translation(20.0, 0.0, z))
                .unwrap()
                .first()
                .unwrap()
        };
        let mut body = bar;
        if plunge {
            let above = at(&mut k, 12.0);
            body = k
                .cut_swept_exact(
                    body,
                    above,
                    &SolidMotion::slide(Vector3::new(0.0, 0.0, -3.0)),
                )
                .unwrap()
                .first()
                .unwrap();
        }
        let before = k.mass_properties(body).unwrap().volume;
        let start = at(&mut k, 9.0);
        let axis = Axis::new(Point3::origin(), Vector3::x());
        let cut = k
            .cut_swept_exact(body, start, &SolidMotion::helical(axis, lead, 0.0, PI))
            .unwrap_or_else(|e| panic!("plunge {plunge}: {e}"))
            .first()
            .unwrap();
        let b = k.get(cut).unwrap();
        assert!(b.check().is_empty(), "plunge {plunge}: {:?}", b.check());
        let faces = k.faces(cut).unwrap().len();
        assert!(faces < 60, "plunge {plunge}: {faces} faces");
        let removed = 10.0 * 10.0 * PI * 60.0 - k.mass_properties(cut).unwrap().volume;
        // (The plunge's hole is within the groove's start.)
        let plunged = 10.0 * 10.0 * PI * 60.0 - before;
        assert!(
            (removed - expected).abs() < 0.015 * expected,
            "plunge {plunge}: removed {removed} (the plunge {plunged}), expected {expected}"
        );
    }
}

/// Rotary roughing as CAM's exact simulation cuts it: a flat end mill (Ø1, pointing at the
/// bar's axis) screwed a whole turn round a bar of radius 13, then again a level deeper along
/// the same helix. The second pass's walls lie on the first's (the mill's side moved along its
/// own axis sweeps the same surface), and its start on the first's start. Swept a face at a
/// time, the pieces of neighbouring leading faces ran into one another, and the cut, the union
/// of the pieces and the sweep itself all left an open shell; the leading faces are now swept
/// as one sheet. Both cuts exact and sound, the second taking more away.
#[test]
fn a_second_roughing_level_on_the_first_ones_walls_is_cut_exactly() {
    use std::f64::consts::TAU;
    let mut k = NativeKernel::new();
    let mut body = k
        .primitive(&Primitive::cylinder(
            Point3::new(-62.5, 0.0, -13.0),
            Vector3::x(),
            13.0,
            125.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    let half = Profile::of(
        Plane::xz(0.0),
        vec![Loop::new(vec![
            Curve2::line([0.0, 0.0], [0.5, 0.0]),
            Curve2::line([0.5, 0.0], [0.5, 6.0]),
            Curve2::line([0.5, 6.0], [0.0, 6.0]),
            Curve2::line([0.0, 6.0], [0.0, 0.0]),
        ])],
    );
    let mill = k
        .revolve_with(
            &half,
            &RevolveSpec::solid(Axis::new(Point3::origin(), Vector3::z()), None),
        )
        .unwrap()
        .first()
        .unwrap();
    let axis = Axis::new(Point3::new(0.0, 0.0, -13.0), Vector3::x());
    let mut removed = Vec::new();
    for depth in [0.9, 1.9] {
        let start = k
            .transform(mill, &Transform::translation(16.5, 0.0, -depth))
            .unwrap()
            .first()
            .unwrap();
        let motion = SolidMotion::rigid(vec![
            RigidStep::turn(axis, 0.0, TAU),
            RigidStep::slide(Vector3::zeros(), Vector3::new(TAU, 0.0, 0.0)),
        ]);
        let before = k.mass_properties(body).unwrap().volume;
        body = k
            .cut_swept_exact(body, start, &motion)
            .unwrap_or_else(|e| panic!("depth {depth}: {e}"))
            .first()
            .unwrap();
        let b = k.get(body).unwrap();
        assert!(b.check().is_empty(), "depth {depth}: {:?}", b.check());
        removed.push(before - k.mass_properties(body).unwrap().volume);
    }
    // The first a groove 0.9 deep along a turn of the helix (about 1 × 0.9 × 2π·12.55 less its
    // ends' rounding); the second as much again, a level below it.
    assert!(removed[0] > 50.0 && removed[0] < 80.0, "{removed:?}");
    assert!(removed[1] > 0.9 * removed[0], "{removed:?}");
}

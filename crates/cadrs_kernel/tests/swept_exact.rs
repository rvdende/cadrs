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
#[test]
fn a_bevel_gear_cut_by_its_pinion_has_the_flanks_its_tooth_generates() {
    use std::f64::consts::FRAC_PI_2;
    let mut k = NativeKernel::new();
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
        let [fa, fb] = faces.as_slice() else {
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

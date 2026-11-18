//! Thicken and Fill ([`Kernel::thicken_surfaces`], [`Kernel::fill`]).

use cadrs_kernel::DefaultKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Unit, Vector3};

fn close(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-9), "{a} vs {b}");
}

/// A straight boundary curve from `a` to `b` (a line on a plane through them).
fn segment(a: Point3<f64>, b: Point3<f64>) -> FillCurve {
    let d = b - a;
    let x = Unit::new_normalize(d);
    let helper = if x.z.abs() < 0.9 {
        Vector3::z()
    } else {
        Vector3::x()
    };
    let normal = Unit::new_normalize(x.cross(&helper));
    FillCurve::Sketch {
        plane: Plane {
            origin: a,
            x_dir: x,
            normal,
        },
        curve: Curve2::Line {
            a: Point2::new(0.0, 0.0),
            b: Point2::new(d.norm(), 0.0),
            source: None,
        },
    }
}

#[test]
fn fill_a_skew_quadrilateral_with_its_bilinear_patch() {
    let mut k = DefaultKernel::new();
    let (a, b, c, d) = (
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(10.0, 10.0, 5.0),
        Point3::new(0.0, 10.0, 0.0),
    );
    let spec = FillSpec {
        curves: vec![segment(a, b), segment(b, c), segment(c, d), segment(d, a)],
        source: 1,
    };
    let r = k.fill(&spec).unwrap();
    let faces = k.faces(r.bodies[0]).unwrap();
    let area: f64 = faces.iter().map(|f| f.area).sum();
    // The bilinear patch through straight sides is the Coons patch: its area by quadrature.
    let s = |u: f64, v: f64| {
        a.coords * ((1.0 - u) * (1.0 - v))
            + b.coords * (u * (1.0 - v))
            + c.coords * (u * v)
            + d.coords * ((1.0 - u) * v)
    };
    let n = 400;
    let h = 1e-6;
    let mut want = 0.0;
    for i in 0..n {
        for j in 0..n {
            let (u, v) = ((i as f64 + 0.5) / n as f64, (j as f64 + 0.5) / n as f64);
            let su = (s(u + h, v) - s(u - h, v)) / (2.0 * h);
            let sv = (s(u, v + h) - s(u, v - h)) / (2.0 * h);
            want += su.cross(&sv).norm() / (n * n) as f64;
        }
    }
    close(area, want, 1e-3);
    // A flat boundary is filled with its plane.
    let e = Point3::new(0.0, 10.0, 0.0);
    let flat = FillSpec {
        curves: vec![
            segment(a, b),
            segment(b, Point3::new(10.0, 10.0, 0.0)),
            segment(Point3::new(10.0, 10.0, 0.0), e),
            segment(e, a),
        ],
        source: 2,
    };
    let r = k.fill(&flat).unwrap();
    let faces = k.faces(r.bodies[0]).unwrap();
    assert_eq!(faces.len(), 1);
    assert!(faces[0].plane.is_some());
    close(faces[0].area, 100.0, 1e-9);
    // An open chain is refused.
    let open = FillSpec {
        curves: vec![segment(a, b), segment(b, c)],
        source: 3,
    };
    assert!(k.fill(&open).is_err());
}

#[test]
fn thicken_a_face_both_ways() {
    let mut k = DefaultKernel::new();
    let e = |a: Point3<f64>, b: Point3<f64>| segment(a, b);
    let (a, b, c, d) = (
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(10.0, 5.0, 0.0),
        Point3::new(0.0, 5.0, 0.0),
    );
    let sheet = k
        .fill(&FillSpec {
            curves: vec![e(a, b), e(b, c), e(c, d), e(d, a)],
            source: 1,
        })
        .unwrap()
        .bodies[0];
    let r = k
        .thicken_surfaces(&ThickenSpec {
            bodies: vec![sheet],
            faces: vec![],
            profiles: vec![],
            along: 1.0,
            against: 0.5,
            sharp: false,
            source: 7,
        })
        .unwrap();
    close(k.mass_properties(r.bodies[0]).unwrap().volume, 75.0, 1e-9);
    let bb = k.bounding_box(r.bodies[0]).unwrap();
    close(bb.max.z - bb.min.z, 1.5, 1e-9);
    assert!(
        k.thicken_surfaces(&ThickenSpec {
            bodies: vec![sheet],
            faces: vec![],
            profiles: vec![],
            along: 0.0,
            against: 0.0,
            sharp: false,
            source: 7
        })
        .is_err()
    );
}

/// A quarter of a cup thickened outwards: its floor (a quarter disc) and its wall (a quarter
/// of a cylinder) meet along a quarter circle, where the sheet bends away from the side
/// thickened, so the floor's slab and the wall's leave a gap all along it. Rounded, the gap is
/// a quarter disc of the thickness turned about the cup's axis; mitred, the square ring
/// between the slabs. (A crease along a curve had no fill: the slabs alone were an error.)
#[test]
fn thicken_fills_a_crease_round_a_circle() {
    use std::f64::consts::PI;
    let mut k = DefaultKernel::new();
    let floor = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let quarter = Loop::from_point([0.0, 0.0])
        .line_to([5.0, 0.0])
        .conic_to([5.0, 5.0], 2f64.sqrt() / 2.0, [0.0, 5.0])
        .close();
    let solid = k
        .extrude_with(
            &Profile::of(floor, vec![quarter]),
            &ExtrudeSpec::blind(Vector3::z_axis(), 5.0),
        )
        .unwrap()
        .first()
        .unwrap();
    // Its floor (the flat face at z = 0) and its round wall.
    let faces = k.faces(solid).unwrap();
    let pick = |kind: SurfaceKind, z: f64| {
        faces
            .iter()
            .find(|f| f.kind == kind && (f.center.z - z).abs() < 1e-6)
            .map(|f| FaceInput {
                body: solid,
                face: f.id,
                source: 1,
            })
            .expect("the face")
    };
    let sheet = vec![
        pick(SurfaceKind::Plane, 0.0),
        pick(SurfaceKind::Cylinder, 2.5),
    ];
    let t = 1.0;
    let thickened = |k: &mut DefaultKernel, sharp: bool| {
        let r = k
            .thicken_surfaces(&ThickenSpec {
                bodies: vec![],
                faces: sheet.clone(),
                profiles: vec![],
                along: t,
                against: 0.0,
                sharp,
                source: 7,
            })
            .unwrap();
        k.mass_properties(r.bodies[0]).unwrap().volume
    };
    let slabs = PI * 25.0 / 4.0 * t + PI * ((5.0 + t).powi(2) - 25.0) / 4.0 * 5.0;
    let round = slabs + (PI * t * t / 4.0) * (PI / 2.0) * (5.0 + 4.0 * t / (3.0 * PI));
    close(thickened(&mut k, false), round, 1e-7);
    let mitred = (PI * (5.0 + t).powi(3) - PI * 125.0) / 4.0;
    close(thickened(&mut k, true), mitred, 1e-7);
}

/// A cylinder cut on a slant: its slanted top and its wall, thickened outwards by a half,
/// rounded. They meet along an ellipse, at an angle that changes round it, and the sheet bends
/// away from the side thickened: the gap between the top's slab and the wall's is the tube
/// round the ellipse, less what of it lies behind the sheet; mitred, what lies within the
/// thickness of both surfaces run on past the crease. The same as the whole solid grown by a
/// half the same way, above its foot, less the solid.
#[test]
fn thicken_fills_a_crease_round_an_ellipse() {
    let mut k = DefaultKernel::new();
    let rod = k
        .primitive(&Primitive::cylinder(
            Point3::origin(),
            Vector3::z(),
            5.0,
            20.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    let side = Plane {
        origin: Point3::new(0.0, -10.0, 0.0),
        x_dir: Vector3::x_axis(),
        normal: Unit::new_normalize(-Vector3::y()),
    };
    let outline = Loop::from_point([-10.0, 12.0])
        .line_to([10.0, 16.0])
        .line_to([10.0, 30.0])
        .line_to([-10.0, 30.0])
        .close();
    let wedge = k
        .extrude_with(
            &Profile::of(side, vec![outline]),
            &ExtrudeSpec::blind(Vector3::y_axis(), 20.0),
        )
        .unwrap()
        .first()
        .unwrap();
    let cut = k
        .boolean(BoolOp::Subtract, rod, &[wedge])
        .unwrap()
        .first()
        .unwrap();
    let faces = k.faces(cut).unwrap();
    let sheet: Vec<FaceInput> = faces
        .iter()
        .filter(|f| f.kind == SurfaceKind::Cylinder || f.center.z > 5.0)
        .map(|f| FaceInput {
            body: cut,
            face: f.id,
            source: 1,
        })
        .collect();
    assert_eq!(sheet.len(), 2);
    let above = k
        .primitive(&Primitive::block(
            Point3::new(-20.0, -20.0, 0.0),
            Point3::new(20.0, 20.0, 40.0),
        ))
        .unwrap()
        .first()
        .unwrap();
    let solid = k.mass_properties(cut).unwrap().volume;
    // Rounded, then mitred (the corner between the slabs out to where their far sides meet).
    for sharp in [false, true] {
        let thick = k
            .thicken_surfaces(&ThickenSpec {
                bodies: vec![],
                faces: sheet.clone(),
                profiles: vec![],
                along: 0.5,
                against: 0.0,
                sharp,
                source: 7,
            })
            .unwrap()
            .bodies[0];
        let got = k.mass_properties(thick).unwrap().volume;
        // The solid grown the same way, cut off at its foot's plane, less the solid.
        let grown = k
            .offset(
                cut,
                &OffsetSpec {
                    distance: 0.5,
                    faces: vec![],
                    sharp,
                },
            )
            .unwrap()
            .bodies[0];
        let grown = k
            .boolean(BoolOp::Intersect, grown, &[above])
            .unwrap()
            .first()
            .unwrap();
        let want = k.mass_properties(grown).unwrap().volume - solid;
        close(got, want, 1e-6);
    }
}

/// Two walls of a prism thickened outwards with a mitred join: a flat one and one over a
/// Bézier curve, meeting along an upright edge where the curve ends. The curved wall's surface
/// stops at that edge (its curve does), so it can't be run on past it: the mitre is cut from
/// the strip that leaves it there along its own tangent. In plan the two bands are the walls'
/// lengths by the thickness (the curved one's with half its turning by the thickness squared),
/// and the corner between them the kite d² tan(θ/2), θ the turn from one wall to the other.
#[test]
fn thicken_mitres_a_wall_that_ends_at_the_crease() {
    use std::f64::consts::PI;
    let mut k = DefaultKernel::new();
    let floor = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let poles = [[10.0, 5.0], [7.0, 9.0], [3.0, 9.0], [0.0, 5.0]];
    let outline = Loop::from_point([0.0, 0.0])
        .line_to([10.0, 0.0])
        .line_to(poles[0])
        .bezier_to(&poles[1..], &[1.0; 4])
        .close();
    let prism = k
        .extrude_with(
            &Profile::of(floor, vec![outline]),
            &ExtrudeSpec::blind(Vector3::z_axis(), 6.0),
        )
        .unwrap()
        .first()
        .unwrap();
    let faces = k.faces(prism).unwrap();
    // The flat wall at x = 10 and the wall over the curve (the one that is no plane).
    let sheet: Vec<FaceInput> = faces
        .iter()
        .filter(|f| {
            f.kind != SurfaceKind::Plane
                || ((f.center.x - 10.0).abs() < 1e-6 && (f.center.z - 3.0).abs() < 1e-6)
        })
        .map(|f| FaceInput {
            body: prism,
            face: f.id,
            source: 1,
        })
        .collect();
    assert_eq!(sheet.len(), 2);
    let d = 0.5;
    let thickened = |k: &mut DefaultKernel, sharp: bool| {
        let r = k
            .thicken_surfaces(&ThickenSpec {
                bodies: vec![],
                faces: sheet.clone(),
                profiles: vec![],
                along: d,
                against: 0.0,
                sharp,
                source: 7,
            })
            .unwrap();
        k.mass_properties(r.bodies[0]).unwrap().volume
    };
    let (round, sharp) = (thickened(&mut k, false), thickened(&mut k, true));
    // The curve's length and its turning, by its own formula.
    let at = |t: f64| {
        let s = 1.0 - t;
        let w = [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t];
        (0..4).fold([0.0, 0.0], |p, i| {
            [p[0] + w[i] * poles[i][0], p[1] + w[i] * poles[i][1]]
        })
    };
    let n = 200_000;
    let length: f64 = (0..n)
        .map(|i| {
            let (a, b) = (at(i as f64 / n as f64), at((i + 1) as f64 / n as f64));
            (b[0] - a[0]).hypot(b[1] - a[1])
        })
        .sum();
    let heading = |a: [f64; 2], b: [f64; 2]| (b[1] - a[1]).atan2(b[0] - a[0]);
    let turning = (heading(poles[2], poles[3]) - heading(poles[0], poles[1])).rem_euclid(2.0 * PI);
    let corner = (heading(poles[0], poles[1]) - PI / 2.0).abs();
    let bands = 5.0 * d + length * d + turning * d * d / 2.0;
    close(round, 6.0 * (bands + corner * d * d / 2.0), 1e-5);
    close(sharp, 6.0 * (bands + d * d * (corner / 2.0).tan()), 1e-5);
}

/// The same two walls, the curved one over a B-spline of two spans: its surface can't be run
/// on past the edge where the curve ends, so the mitre is cut from the strip that leaves the
/// wall there along its own tangent. Mitred, the join is the kite d² tan(θ/2) in plan where
/// rounded it is the sector θ d²/2, θ the angle between the walls' normals at the edge.
#[test]
fn thicken_mitres_a_spline_wall_by_its_tangent_strip() {
    use cadrs_kernel::native::NativeKernel;
    let mut k = NativeKernel::new();
    let floor = Plane {
        origin: Point3::origin(),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let line = |a: [f64; 2], b: [f64; 2]| Curve2::Line {
        a: Point2::new(a[0], a[1]),
        b: Point2::new(b[0], b[1]),
        source: None,
    };
    let outline = Loop {
        curves: vec![
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 5.0]),
            Curve2::spline(
                3,
                &[0.0, 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0, 1.0],
                &[[10.0, 5.0], [8.5, 7.5], [5.0, 9.0], [1.5, 7.5], [0.0, 5.0]],
                &[],
            ),
            line([0.0, 5.0], [0.0, 0.0]),
        ],
    };
    let prism = k
        .extrude_with(
            &Profile::of(floor, vec![outline]),
            &ExtrudeSpec::blind(Vector3::z_axis(), 6.0),
        )
        .unwrap()
        .first()
        .unwrap();
    let faces = k.faces(prism).unwrap();
    let sheet: Vec<FaceInput> = faces
        .iter()
        .filter(|f| {
            f.kind != SurfaceKind::Plane
                || ((f.center.x - 10.0).abs() < 1e-6 && (f.center.z - 3.0).abs() < 1e-6)
        })
        .map(|f| FaceInput {
            body: prism,
            face: f.id,
            source: 1,
        })
        .collect();
    assert_eq!(sheet.len(), 2);
    // The angle between the two walls' normals at the edge they share.
    let turn = {
        let b = k.get(prism).unwrap();
        let at = Point3::new(10.0, 5.0, 3.0);
        let normal = |f: &FaceInput| {
            let face = &b.faces[f.face.0 as usize];
            let (u, v) = face.surf.project(&at, None);
            face.surf.normal(u, v).normalize() * if face.sense { 1.0 } else { -1.0 }
        };
        normal(&sheet[0])
            .dot(&normal(&sheet[1]))
            .clamp(-1.0, 1.0)
            .acos()
    };
    assert!(turn > 0.1 && turn < 1.5, "{turn}");
    let d = 0.5;
    let mut thickened = |sharp: bool| {
        let r = k
            .thicken_surfaces(&ThickenSpec {
                bodies: vec![],
                faces: sheet.clone(),
                profiles: vec![],
                along: d,
                against: 0.0,
                sharp,
                source: 7,
            })
            .unwrap();
        k.mass_properties(r.bodies[0]).unwrap().volume
    };
    let (round, sharp) = (thickened(false), thickened(true));
    let more = 6.0 * d * d * ((turn / 2.0).tan() - turn / 2.0);
    assert!(
        (sharp - round - more).abs() < 1e-5,
        "{sharp} - {round} against {more}"
    );
}

/// A whole band of a surface of revolution thickened: its slab is its meridian's flat figure
/// turned once (`offset::slab::band_slab`), so a drum's wall is a tube and a frustum's a
/// rectangle turned about the axis, whose volume is its area round its centre (Pappus); both
/// ways at once is one slab, not two united.
#[test]
fn thicken_a_band_turns_its_meridians_figure() {
    use std::f64::consts::PI;
    let side = |k: &DefaultKernel, b: BodyId, kind: SurfaceKind| {
        let f = k
            .faces(b)
            .unwrap()
            .into_iter()
            .find(|f| f.kind == kind)
            .unwrap();
        FaceInput {
            body: b,
            face: f.id,
            source: 0,
        }
    };
    let thick = |k: &mut DefaultKernel, face: FaceInput, along: f64, against: f64| {
        let r = k
            .thicken_surfaces(&ThickenSpec {
                bodies: vec![],
                faces: vec![face],
                profiles: vec![],
                along,
                against,
                sharp: false,
                source: 7,
            })
            .unwrap();
        assert_eq!(r.bodies.len(), 1);
        r.bodies[0]
    };
    let mut k = DefaultKernel::new();
    let (r, h) = (5.0, 10.0);
    let drum = k
        .primitive(&Primitive::cylinder(Point3::origin(), Vector3::z(), r, h))
        .unwrap()
        .first()
        .unwrap();
    let wall = side(&k, drum, SurfaceKind::Cylinder);
    let out = thick(&mut k, wall, 1.0, 0.0);
    close(
        k.mass_properties(out).unwrap().volume,
        PI * ((r + 1.0).powi(2) - r * r) * h,
        1e-9,
    );
    assert_eq!(
        k.faces(out).unwrap().len(),
        4,
        "two walls and two flat rings"
    );
    let out = thick(&mut k, wall, 1.0, 0.5);
    close(
        k.mass_properties(out).unwrap().volume,
        PI * ((r + 1.0).powi(2) - (r - 0.5).powi(2)) * h,
        1e-9,
    );
    assert_eq!(k.faces(out).unwrap().len(), 4, "one slab, not two united");

    // A frustum from radius 6 at the bottom to 3 at height 4: its wall 5 long, leaning out
    // of upright by atan(3/4); thickened 1 outward, a 5 × 1 rectangle whose centre is half
    // the thickness out along the wall's normal from the wall's middle.
    let frustum = k
        .primitive(&Primitive::cone(
            Point3::origin(),
            Vector3::z(),
            6.0,
            3.0,
            4.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    let wall = side(&k, frustum, SurfaceKind::Cone);
    let out = thick(&mut k, wall, 1.0, 0.0);
    let centre = 4.5 + 0.5 * (4.0 / 5.0);
    close(
        k.mass_properties(out).unwrap().volume,
        2.0 * PI * centre * 5.0,
        1e-9,
    );
    assert_eq!(
        k.faces(out)
            .unwrap()
            .iter()
            .filter(|f| f.kind == SurfaceKind::Cone)
            .count(),
        4,
        "its two walls and its two ends, each a cone, none a fitted surface"
    );
}

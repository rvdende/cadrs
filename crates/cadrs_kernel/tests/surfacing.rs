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

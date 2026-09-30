//! Thicken and Fill of the OCCT backend ([`Kernel::thicken_surfaces`], [`Kernel::fill`]).
#![cfg(feature = "occt")]

use cadrs_kernel::backend::occt::OcctKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Unit, Vector3};

fn close(a: f64, b: f64, rel: f64) {
    assert!((a - b).abs() <= rel * b.abs().max(1e-9), "{a} vs {b}");
}

/// A straight boundary curve from `a` to `b` (a line on a plane through them).
fn segment(a: Point3<f64>, b: Point3<f64>) -> FillCurve {
    let d = b - a;
    let x = Unit::new_normalize(d);
    let helper = if x.z.abs() < 0.9 { Vector3::z() } else { Vector3::x() };
    let normal = Unit::new_normalize(x.cross(&helper));
    FillCurve::Sketch {
        plane: Plane { origin: a, x_dir: x, normal },
        curve: Curve2::Line { a: Point2::new(0.0, 0.0), b: Point2::new(d.norm(), 0.0), source: None },
    }
}

#[test]
fn fill_a_skew_quadrilateral_with_its_bilinear_patch() {
    let mut k = OcctKernel::new();
    let (a, b, c, d) = (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 10.0, 5.0), Point3::new(0.0, 10.0, 0.0));
    let spec = FillSpec { curves: vec![segment(a, b), segment(b, c), segment(c, d), segment(d, a)], source: 1 };
    let r = k.fill(&spec).unwrap();
    let faces = k.faces(r.bodies[0]).unwrap();
    let area: f64 = faces.iter().map(|f| f.area).sum();
    // The bilinear patch through straight sides is the Coons patch: its area by quadrature.
    let s = |u: f64, v: f64| a.coords * ((1.0 - u) * (1.0 - v)) + b.coords * (u * (1.0 - v)) + c.coords * (u * v) + d.coords * ((1.0 - u) * v);
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
    let flat = FillSpec { curves: vec![segment(a, b), segment(b, Point3::new(10.0, 10.0, 0.0)), segment(Point3::new(10.0, 10.0, 0.0), e), segment(e, a)], source: 2 };
    let r = k.fill(&flat).unwrap();
    let faces = k.faces(r.bodies[0]).unwrap();
    assert_eq!(faces.len(), 1);
    assert!(faces[0].plane.is_some());
    close(faces[0].area, 100.0, 1e-9);
    // An open chain is refused.
    let open = FillSpec { curves: vec![segment(a, b), segment(b, c)], source: 3 };
    assert!(k.fill(&open).is_err());
}

#[test]
fn thicken_a_face_both_ways() {
    let mut k = OcctKernel::new();
    let e = |a: Point3<f64>, b: Point3<f64>| segment(a, b);
    let (a, b, c, d) = (Point3::new(0.0, 0.0, 0.0), Point3::new(10.0, 0.0, 0.0), Point3::new(10.0, 5.0, 0.0), Point3::new(0.0, 5.0, 0.0));
    let sheet = k.fill(&FillSpec { curves: vec![e(a, b), e(b, c), e(c, d), e(d, a)], source: 1 }).unwrap().bodies[0];
    let r = k.thicken_surfaces(&ThickenSpec { bodies: vec![sheet], faces: vec![], profiles: vec![], along: 1.0, against: 0.5, source: 7 }).unwrap();
    close(k.mass_properties(r.bodies[0]).unwrap().volume, 75.0, 1e-9);
    let bb = k.bounding_box(r.bodies[0]).unwrap();
    close(bb.max.z - bb.min.z, 1.5, 1e-9);
    assert!(k.thicken_surfaces(&ThickenSpec { bodies: vec![sheet], faces: vec![], profiles: vec![], along: 0.0, against: 0.0, source: 7 }).is_err());
}

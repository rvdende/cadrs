//! `Helix::through` / `Helix::through_points`: a helix or conical spiral given as sampled points
//! (a fitted spline, a path another system gives as points) found again exactly, and points on
//! none refused.

use cadrs_kernel::{Axis, Helix};
use nalgebra::{Point3, Unit, Vector3};

type P3 = Point3<f64>;
type V3 = Vector3<f64>;

fn samples(h: &Helix, n: usize) -> (Vec<P3>, Vec<V3>) {
    (0..=n)
        .map(|i| {
            let (p, d) = h.at(i as f64 / n as f64);
            (p, d.normalize())
        })
        .unzip()
}

/// The fitted helix runs through the same points as `want`, end to end.
fn same_path(got: &Helix, want: &Helix, tol: f64) {
    for i in 0..=50 {
        let s = i as f64 / 50.0;
        let (a, b) = (got.at(s).0, want.at(s).0);
        assert!((a - b).norm() < tol, "at {s}: {a} vs {b}");
    }
    assert!(
        (got.height - want.height).abs() < tol,
        "{} vs {}",
        got.height,
        want.height
    );
    assert!(
        (got.turns - want.turns).abs() < tol,
        "{} vs {}",
        got.turns,
        want.turns
    );
    assert!((got.r0 - want.r0).abs() < tol && (got.r1 - want.r1).abs() < tol);
}

fn axis(o: [f64; 3], d: [f64; 3]) -> Axis {
    Axis {
        origin: P3::new(o[0], o[1], o[2]),
        dir: Unit::new_normalize(V3::new(d[0], d[1], d[2])),
    }
}

#[test]
fn helix_found_again_from_its_points() {
    // Off the origin, its axis tilted, two and a half turns.
    let want = Helix::new(axis([3.0, -1.0, 2.0], [0.2, 0.3, 1.0]), 4.0, 1.5, 0.7, 2.5);
    let (pts, tans) = samples(&want, 400);
    let got = Helix::through(&pts, &tans, 1e-6).expect("a helix");
    same_path(&got, &want, 1e-7);
}

#[test]
fn left_handed_helix_found_again() {
    let want = Helix::new(axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), 2.0, 0.8, 0.0, -3.0);
    let (pts, tans) = samples(&want, 400);
    let got = Helix::through(&pts, &tans, 1e-6).expect("a helix");
    assert!(got.turns < 0.0, "{}", got.turns);
    same_path(&got, &want, 1e-7);
}

#[test]
fn conical_spiral_found_again() {
    let want = Helix::conical(
        axis([1.0, 2.0, 0.0], [0.0, 1.0, 0.0]),
        5.0,
        2.0,
        6.0,
        0.3,
        1.75,
    );
    let (pts, tans) = samples(&want, 400);
    let got = Helix::through(&pts, &tans, 1e-6).expect("a conical spiral");
    same_path(&got, &want, 1e-7);
}

#[test]
fn helix_found_from_points_alone() {
    let want = Helix::new(axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), 10.0, 3.0, 0.0, 4.0);
    let (pts, _) = samples(&want, 600);
    let got = Helix::through_points(&pts, 1e-6).expect("a helix");
    // (Fitted to the tolerance asked: the tangents are the polyline's.)
    same_path(&got, &want, 1e-6);
}

#[test]
fn points_on_no_helix_are_refused() {
    // A flat circle: it doesn't rise.
    let circle: Vec<P3> = (0..=100)
        .map(|i| {
            let t = std::f64::consts::TAU * i as f64 / 100.0 * 0.9;
            P3::new(3.0 * t.cos(), 3.0 * t.sin(), 0.0)
        })
        .collect();
    assert!(Helix::through_points(&circle, 1e-6).is_none());
    // A straight line.
    let line: Vec<P3> = (0..=50)
        .map(|i| P3::new(i as f64, 2.0 * i as f64, 0.5 * i as f64))
        .collect();
    assert!(Helix::through_points(&line, 1e-6).is_none());
    // A helix's points pushed off it by far more than the tolerance.
    let h = Helix::new(axis([0.0, 0.0, 0.0], [0.0, 0.0, 1.0]), 4.0, 2.0, 0.0, 2.0);
    let (mut pts, tans) = samples(&h, 200);
    for (i, p) in pts.iter_mut().enumerate() {
        p.x += 1e-3 * ((i * 7919) % 13) as f64 / 13.0;
    }
    assert!(Helix::through(&pts, &tans, 1e-6).is_none());
    // Too few points, or a tangent for each missing.
    assert!(Helix::through(&pts[..3], &tans[..3], 1e-6).is_none());
    assert!(Helix::through(&pts, &tans[..10], 1e-6).is_none());
}

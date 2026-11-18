//! A ball-end mill's raster pass cut from a block whole (`native::sweep_ball_pass`), against
//! the same pass cut a move at a time.

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point3, Unit, Vector3};

/// A ball-end mill of radius 3, 30 long, upright with its tip at the origin: its half section
/// turned about z.
fn mill(k: &mut NativeKernel) -> BodyId {
    let w = 2f64.sqrt() / 2.0;
    let half = Profile::of(
        Plane::xz(0.0),
        vec![Loop::new(vec![
            Curve2::rational_bezier(&[[0.0, 0.0], [3.0, 0.0], [3.0, 3.0]], &[1.0, w, 1.0]),
            Curve2::line([3.0, 3.0], [3.0, 30.0]),
            Curve2::line([3.0, 30.0], [0.0, 30.0]),
            Curve2::line([0.0, 30.0], [0.0, 0.0]),
        ])],
    );
    let axis = Axis::new(Point3::origin(), Vector3::z());
    k.revolve_with(&half, &RevolveSpec::solid(axis, None))
        .unwrap()
        .first()
        .unwrap()
}

fn plate(k: &mut NativeKernel) -> BodyId {
    k.primitive(&Primitive::block(
        Point3::new(0.0, 0.0, -20.0),
        Point3::new(40.0, 30.0, 0.0),
    ))
    .unwrap()
    .first()
    .unwrap()
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

/// Five moves over a plate, down into a valley, over a hill and out the far side, askew in
/// plan: the pass cut whole leaves what cutting each move's own sweep leaves, in a few faces
/// (a half cylinder a move, the ball at the valley, two walls), none of them a mesh's.
#[test]
fn a_pass_cut_whole_is_the_moves_cut_one_by_one() {
    let tips: Vec<Point3<f64>> = [
        (-5.0, -7.0),
        (5.0, -4.0),
        (14.0, -8.0),
        (22.0, -3.0),
        (31.0, -5.0),
        (46.0, -6.0),
    ]
    .iter()
    .map(|&(s, z)| Point3::new(s, 6.0 + 0.4 * s, z))
    .collect();

    let mut k = NativeKernel::new();
    let (stock, tool) = (plate(&mut k), mill(&mut k));
    let at = |k: &mut NativeKernel, p: &Point3<f64>| {
        k.transform(tool, &Transform::translation(p.x, p.y, p.z))
            .unwrap()
            .first()
            .unwrap()
    };
    let placed = at(&mut k, &tips[0]);
    let path: Vec<PathCurve> = tips
        .windows(2)
        .map(|m| PathCurve::Bezier3 {
            poles: vec![m[0], m[1]],
            weights: Vec::new(),
        })
        .collect();
    let whole = k
        .cut_swept(
            stock,
            placed,
            &SolidMotion::Path {
                path,
                up: Unit::new_normalize(Vector3::z()),
            },
        )
        .unwrap()
        .first()
        .unwrap();
    let b = k.get(whole).unwrap();
    assert!(b.check().is_empty(), "{:?}", b.check());
    assert!(b.faces.len() < 30, "{} faces", b.faces.len());

    let mut one_by_one = stock;
    for m in tips.windows(2) {
        let placed = at(&mut k, &m[0]);
        one_by_one = k
            .cut_swept(one_by_one, placed, &SolidMotion::slide(m[1] - m[0]))
            .unwrap()
            .first()
            .unwrap();
    }
    let (v, want) = (volume(&k, whole), volume(&k, one_by_one));
    assert!(want < 40.0 * 30.0 * 20.0 - 100.0, "nothing cut: {want}");
    assert!((v - want).abs() < 1e-9 * want, "whole {v}, by moves {want}");
}

/// What is no pass is cut as before: a move straight down among the moves (the mill plunging)
/// still cuts, by the roads there were.
#[test]
fn a_plunge_is_no_pass_and_still_cuts() {
    let mut k = NativeKernel::new();
    let (stock, tool) = (plate(&mut k), mill(&mut k));
    let placed = k
        .transform(tool, &Transform::translation(10.0, 10.0, 2.0))
        .unwrap()
        .first()
        .unwrap();
    let cut = k
        .cut_swept(
            stock,
            placed,
            &SolidMotion::slide(Vector3::new(0.0, 0.0, -6.0)),
        )
        .unwrap()
        .first()
        .unwrap();
    // A hole 4 deep with the ball's half at its bottom: the cylinder less the ball's
    // complement in its own bounding cylinder.
    let want = 40.0 * 30.0 * 20.0
        - (std::f64::consts::PI * 9.0 * 4.0
            - (std::f64::consts::PI * 27.0 - 2.0 / 3.0 * std::f64::consts::PI * 27.0));
    let v = volume(&k, cut);
    assert!((v - want).abs() < 1e-9 * want, "{v} for {want}");
}

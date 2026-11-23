//! A flat end mill's whole program (level runs, plunges, rises, moves clear above the work) cut
//! from a plate as one path (`native::sweep_solid`'s program road: each depth's figure united
//! in the plane, extruded, cut once), against the same program cut a move at a time; and a
//! drill's pecks, one slide a hole.

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Unit, Vector3};
use std::f64::consts::TAU;

fn plate(k: &mut NativeKernel) -> BodyId {
    k.primitive(&Primitive::block(
        Point3::new(0.0, 0.0, -10.0),
        Point3::new(60.0, 40.0, 0.0),
    ))
    .unwrap()
    .first()
    .unwrap()
}

fn volume(k: &NativeKernel, b: BodyId) -> f64 {
    k.mass_properties(b).unwrap().volume
}

/// A move of the tip: to a point, or a whole turn about the upright line through a point.
#[derive(Clone, Copy)]
enum Move {
    To(f64, f64, f64),
    Round(f64, f64),
}

/// The program cut whole, and a move at a time; their volumes.
fn both_ways(
    k: &mut NativeKernel,
    tool: BodyId,
    start: Point3<f64>,
    moves: &[Move],
) -> (f64, f64, usize) {
    let stock = plate(k);
    let at = |k: &mut NativeKernel, p: &Point3<f64>| {
        k.transform(tool, &Transform::translation(p.x, p.y, p.z))
            .unwrap()
            .first()
            .unwrap()
    };
    let mut path = Vec::new();
    let mut tip = start;
    for m in moves {
        match *m {
            Move::To(x, y, z) => {
                let to = Point3::new(x, y, z);
                path.push(PathCurve::Bezier3 {
                    poles: vec![tip, to],
                    weights: Vec::new(),
                });
                tip = to;
            }
            Move::Round(x, y) => {
                let (dx, dy) = (tip.x - x, tip.y - y);
                path.push(PathCurve::Sketch {
                    plane: Plane::xy(tip.z),
                    curve: Curve2::Circle {
                        center: Point2::new(x, y),
                        radius: dx.hypot(dy),
                        source: None,
                    },
                });
            }
        }
    }
    let placed = at(k, &start);
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
    let faces = b.faces.len();

    let mut one_by_one = stock;
    let mut tip = start;
    for m in moves {
        let placed = at(k, &tip);
        let (motion, to) = match *m {
            Move::To(x, y, z) => {
                let to = Point3::new(x, y, z);
                (SolidMotion::slide(to - tip), to)
            }
            Move::Round(x, y) => (
                SolidMotion::swing(Axis::new(Point3::new(x, y, 0.0), Vector3::z()), 0.0, TAU),
                tip,
            ),
        };
        // (A move clear above the plate cuts nothing.)
        if tip.z.min(to.z) < 0.0 {
            one_by_one = k
                .cut_swept(one_by_one, placed, &motion)
                .unwrap()
                .first()
                .unwrap();
        }
        tip = to;
    }
    (volume(k, whole), volume(k, one_by_one), faces)
}

#[test]
fn a_pocket_program_cut_whole_is_its_moves_cut_one_by_one() {
    let mut k = NativeKernel::new();
    let mill = k
        .primitive(&Primitive::cylinder(
            Point3::origin(),
            Vector3::z(),
            3.0,
            30.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    use Move::*;
    let moves = [
        // Two depths of a run along x (the shallower within the deeper), joined by a step over.
        To(10.0, 10.0, -3.0),
        To(50.0, 10.0, -3.0),
        To(50.0, 10.0, 5.0),
        To(10.0, 10.0, 5.0),
        To(10.0, 10.0, -6.0),
        To(50.0, 10.0, -6.0),
        To(50.0, 14.0, -6.0),
        To(10.0, 14.0, -6.0),
        To(10.0, 14.0, 5.0),
        // A ring round a boss, wider than the mill: its middle left standing.
        To(35.0, 27.0, 5.0),
        To(35.0, 27.0, -4.0),
        Round(28.0, 27.0),
        To(35.0, 27.0, 5.0),
        // A plunge alone.
        To(52.0, 33.0, 5.0),
        To(52.0, 33.0, -8.0),
        To(52.0, 33.0, 5.0),
    ];
    let (whole, by_moves, faces) = both_ways(&mut k, mill, Point3::new(10.0, 10.0, 5.0), &moves);
    assert!(
        by_moves < 60.0 * 40.0 * 10.0 - 1000.0,
        "nothing cut: {by_moves}"
    );
    assert!(
        (whole - by_moves).abs() < 1e-9 * by_moves,
        "whole {whole}, by moves {by_moves}"
    );
    assert!(faces < 40, "{faces} faces");
}

#[test]
fn a_drills_pecks_cut_whole_are_its_moves_cut_one_by_one() {
    let mut k = NativeKernel::new();
    // A drill of radius 2 with a 90° point.
    let half = Profile::of(
        Plane::xz(0.0),
        vec![Loop::new(vec![
            Curve2::line([0.0, 0.0], [2.0, 2.0]),
            Curve2::line([2.0, 2.0], [2.0, 30.0]),
            Curve2::line([2.0, 30.0], [0.0, 30.0]),
            Curve2::line([0.0, 30.0], [0.0, 0.0]),
        ])],
    );
    let drill = k
        .revolve_with(
            &half,
            &RevolveSpec::solid(Axis::new(Point3::origin(), Vector3::z()), None),
        )
        .unwrap()
        .first()
        .unwrap();
    use Move::*;
    let mut moves = Vec::new();
    for (x, y) in [(8.0, 8.0), (50.0, 30.0)] {
        moves.push(To(x, y, 5.0));
        for (down, back) in [(-3.0, 1.0), (-6.0, 1.0), (-8.0, 3.0)] {
            moves.push(To(x, y, down));
            moves.push(To(x, y, back));
        }
    }
    let (whole, by_moves, _) = both_ways(&mut k, drill, Point3::new(8.0, 8.0, 5.0), &moves);
    assert!(
        by_moves < 60.0 * 40.0 * 10.0 - 100.0,
        "nothing cut: {by_moves}"
    );
    assert!(
        (whole - by_moves).abs() < 1e-9 * by_moves,
        "whole {whole}, by moves {by_moves}"
    );
}

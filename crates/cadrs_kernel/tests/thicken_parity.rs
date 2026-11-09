//! Surfaces and thicken as OCCT made them (pibox/piboxv7: a surface box's walls thickened
//! "against" with its lid; the documents were built with OCCT's results, and later features
//! pick the thickened faces by where they are).

use cadrs_kernel::native::kernel::NativeKernel;
use cadrs_kernel::native::tol;
use cadrs_kernel::*;
use nalgebra::{Point2, Point3, Vector3};

const SIDE: f64 = 10.0;
const HEIGHT: f64 = 10.0;
const WALL: f64 = 1.0;

fn square(plane: Plane) -> Profile {
    let c = [(0.0, 0.0), (SIDE, 0.0), (SIDE, SIDE), (0.0, SIDE)].map(|(x, y)| Point2::new(x, y));
    let curves = (0..4)
        .map(|i| Curve2::Line {
            a: c[i],
            b: c[(i + 1) % 4],
            source: None,
        })
        .collect();
    Profile::new(
        plane,
        vec![Region {
            outer: Loop { curves },
            holes: vec![],
            source: None,
        }],
    )
}

/// A surface extrude of a counter-clockwise square: four walls.
fn walls(k: &mut NativeKernel) -> BodyId {
    let spec = ExtrudeSpec {
        body: BodyKind::Surface,
        ..ExtrudeSpec::blind(Vector3::z_axis(), HEIGHT)
    };
    k.extrude_with(&square(Plane::top()), &spec).unwrap().bodies[0]
}

fn bounds(k: &NativeKernel, id: BodyId) -> (Point3<f64>, Point3<f64>) {
    let f = k.faces(id).unwrap();
    let mut lo = Point3::new(f64::MAX, f64::MAX, f64::MAX);
    let mut hi = Point3::new(f64::MIN, f64::MIN, f64::MIN);
    for e in k.edges(id).unwrap() {
        for p in [e.start, e.end, e.mid] {
            lo = lo.inf(&p);
            hi = hi.sup(&p);
        }
    }
    assert!(!f.is_empty());
    (lo, hi)
}

/// A surface's faces face the sweep crossed with its curves, as OCCT's extruded wires did: a
/// counter-clockwise loop's walls face in.
#[test]
fn a_surface_extrudes_walls_facing_in() {
    let mut k = NativeKernel::new();
    let id = walls(&mut k);
    let centre = Point3::new(SIDE / 2.0, SIDE / 2.0, HEIGHT / 2.0);
    for f in k.faces(id).unwrap() {
        let n = f.plane.expect("planar walls").normal.into_inner();
        assert!(
            (centre - f.center).dot(&n) > 0.0,
            "{:?} faces out",
            f.center
        );
    }
}

/// pibox: the walls thickened against their normals grow outwards, and a lid facing up,
/// thickened with them, grows down: each face along its own normal, as OCCT's slabs did.
#[test]
fn faces_facing_different_ways_thicken_each_along_its_own_normal() {
    let mut k = NativeKernel::new();
    let w = walls(&mut k);
    let lid_plane = Plane {
        origin: Point3::new(0.0, 0.0, HEIGHT),
        x_dir: Vector3::x_axis(),
        normal: Vector3::z_axis(),
    };
    let faces = (0..4)
        .map(|f| FaceInput {
            body: w,
            face: FaceId(f),
            source: 0,
        })
        .collect();
    let spec = ThickenSpec {
        bodies: vec![],
        faces,
        profiles: vec![square(lid_plane)],
        along: 0.0,
        against: WALL,
        sharp: false,
        source: 1,
    };
    let id = k.thicken_surfaces(&spec).unwrap().bodies[0];
    let v = k.mass_properties(id).unwrap().volume;
    let want = 4.0 * SIDE * HEIGHT * WALL + SIDE * SIDE * WALL;
    assert!(
        (v - want).abs() < tol::POINT * want,
        "volume {v}, want {want}"
    );
    let (lo, hi) = bounds(&k, id);
    assert!(
        (lo.x + WALL).abs() < tol::POINT && (hi.x - SIDE - WALL).abs() < tol::POINT,
        "{lo} {hi}"
    );
    assert!(
        (hi.z - HEIGHT).abs() < tol::POINT,
        "the lid grows down: {hi}"
    );
}

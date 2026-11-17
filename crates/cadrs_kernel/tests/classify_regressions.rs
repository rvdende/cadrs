//! Point classification against bodies that once gave the wrong answer.

use cadrs_kernel::PointClass;
use cadrs_kernel::native::classify;
use cadrs_kernel::native::math::*;
use cadrs_kernel::native::{bodyio, topo::Body};

fn polys(b: &Body) -> Vec<Vec<Vec<P2>>> {
    b.faces
        .iter()
        .map(|f| classify::face_polygons(b, f))
        .collect()
}

/// The blend tool of OCCT's `tolblend_simple/C9` (two B-spline faces, a third surface and two
/// planes): a point 0.29 behind its nearest faces is inside, though the first ray from it
/// counts an even number of crossings (one missed near a face's boundary). Regression: the
/// small piece of the box face it lies on was kept by the blend's subtraction.
#[test]
fn inside_the_tolblend_c9_tool() {
    let b = bodyio::read(include_bytes!("data/blend_tool_tolblend_c9.body")).unwrap();
    let p = P3::new(490.4947963950842, 3.3560290826370736, 26.104651656457563);
    assert_eq!(
        classify::classify_point(&b, &polys(&b), &p, 1e-6),
        PointClass::Inside
    );
}

/// A plate with a hole of radius 100 bored through it: a point of the hole a tenth of a micron
/// in from its wall is off the plate's top face, and one as far into the wall on it. The face
/// tests points against its loops as polygons, the hole's circle stepped in 805 chords a half
/// turn, which run up to 1.9e-4 inside it: a point between the circle and a chord was taken
/// for one of the face.
#[test]
fn beside_a_wide_holes_wall() {
    use cadrs_kernel::native::NativeKernel;
    use cadrs_kernel::{BoolOp, Kernel, Primitive};
    use nalgebra::{Point3, Vector3};
    let mut k = NativeKernel::new();
    let plate = k
        .primitive(&Primitive::block(
            Point3::new(-200.0, -200.0, 0.0),
            Point3::new(200.0, 200.0, 10.0),
        ))
        .unwrap()
        .first()
        .unwrap();
    let rod = k
        .primitive(&Primitive::cylinder(
            Point3::new(0.0, 0.0, -5.0),
            Vector3::z(),
            100.0,
            20.0,
        ))
        .unwrap()
        .first()
        .unwrap();
    let bored = k
        .boolean(BoolOp::Subtract, plate, &[rod])
        .unwrap()
        .first()
        .unwrap();
    let b = k.get(bored).unwrap();
    // The plate's top: the flat face at z = 10, the hole's circle one of its loops.
    let top = b
        .faces
        .iter()
        .find(|f| f.loops.len() == 2 && (f.surf.eval(0.0, 0.0).z - 10.0).abs() < 1e-9)
        .expect("the top face");
    let kept = classify::face_polygons_kept(b, top);
    let sampled = classify::face_polygons(b, top);
    let step = std::f64::consts::PI / 805.0;
    let mut were_wrong = 0;
    for i in 0..400 {
        // (Across the chords, wherever the circle's halves start.)
        let a = i as f64 * 0.05 * step;
        for (r, want) in [
            (100.0 - 1e-4, classify::InFace::Outside),
            (100.0 + 1e-4, classify::InFace::Inside),
        ] {
            let (u, v) = top
                .surf
                .project(&P3::new(r * a.cos(), r * a.sin(), 10.0), None);
            let uv = P2::new(u, v);
            assert_eq!(
                classify::in_face_boxed(&kept.0, &kept.1, &top.surf, uv, 1e-7),
                want,
                "at {a} rad, radius {r}"
            );
            if classify::in_face(&sampled, &top.surf, uv, 1e-7) != want {
                were_wrong += 1;
            }
        }
    }
    assert!(were_wrong > 0, "the polygons alone have every point right");
}

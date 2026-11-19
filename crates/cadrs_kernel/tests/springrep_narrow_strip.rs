//! Ported from springrep's tests/narrow_strip.rs (MIT, https://github.com/springcraft-ai/springrep):
//! a face a hundred times narrower than a tolerance of 1e-5 mm, but wider than springrep's 1e-7
//! mm, is kept as a face: swept from an outline edge 90 tolerances long, measured, and left alone
//! when faces are unified (springrep's `unify` is cadrs's `Kernel::simplify`).

mod springrep_port;

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// A 3 by 1 + `step` mm rect on a circle of radius 4 at the origin, extruded 6 mm: the rect's
/// top runs `step` past the circle's top point, so its left side leaves a strip `step` wide.
fn rect_past_a_circle(k: &mut NativeKernel, step: f64) -> BodyId {
    let r7 = 7f64.sqrt();
    let top = 4.0 + step;
    let outline = Loop::new(vec![
        Curve2::arc(
            [0.0, 0.0],
            4.0,
            FRAC_PI_2,
            TAU - (FRAC_PI_2 - 3f64.atan2(r7)),
        ),
        Curve2::line([r7, 3.0], [3.0, 3.0]),
        Curve2::line([3.0, 3.0], [3.0, top]),
        Curve2::line([3.0, top], [0.0, top]),
        Curve2::line([0.0, top], [0.0, 4.0]),
    ]);
    let b = extrude(k, world(), outline, 6.0);
    k.simplify(b).unwrap().first().unwrap()
}

#[test]
fn a_strip_90_tolerances_wide_stays_a_face() {
    let mut k = NativeKernel::new();
    let s = 8.936e-6;
    let b = rect_past_a_circle(&mut k, s);
    let r7 = 7f64.sqrt();
    let outside = r7 * (4.0 + s) - 1.5 * r7 - 8.0 * (r7 / 4.0).asin() + (3.0 - r7) * (1.0 + s);
    let want = 6.0 * (16.0 * PI + outside);
    let got = volume(&k, b);
    assert!((got - want).abs() < 1e-9 * want, "volume {got} vs {want}");
    assert_eq!(faces(&k, b), 7);
    // springrep's `x.body_u_min`: the face swept from the line x = 0, y 4..4 + s (outward −x).
    let strip: Vec<FaceInfo> = k
        .faces(b)
        .unwrap()
        .into_iter()
        .filter(|f| {
            f.plane
                .is_some_and(|p| p.normal.dot(&-V3::x()) > 1.0 - 1e-9)
                && f.center.x.abs() < 1e-9
        })
        .collect();
    assert_eq!(strip.len(), 1);
    assert!(
        (strip[0].area - 6.0 * s).abs() < 1e-9,
        "strip area {} vs {}",
        strip[0].area,
        6.0 * s
    );
    let again = k.simplify(b).unwrap().first().unwrap();
    assert_eq!(faces(&k, again), 7);
}

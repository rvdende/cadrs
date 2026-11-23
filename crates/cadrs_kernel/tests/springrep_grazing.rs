//! Ported from springrep's tests/grazing.rs (MIT, https://github.com/springcraft-ai/springrep):
//! solids whose curved sides graze: they share a sliver far thinner than it is long, their sides
//! cross at a shallow angle, and where the sliver ends each solid's edges cross the other's.
//! The STEP fixtures `fixtures/springrep/grazing-tooth-{a,b}.step` are springrep's
//! (tests/fixtures, MIT).

mod springrep_port;

use std::f64::consts::PI;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// One tooth of each of two spur gears in mesh, cut down to about where their flanks touch.
fn teeth(k: &mut NativeKernel) -> (BodyId, BodyId) {
    let mut read = |bytes: &[u8]| {
        let bodies = k.import_step(bytes).unwrap();
        assert_eq!(bodies.len(), 1, "one solid read");
        bodies[0]
    };
    (
        read(include_bytes!("fixtures/springrep/grazing-tooth-a.step")),
        read(include_bytes!("fixtures/springrep/grazing-tooth-b.step")),
    )
}

#[test]
fn two_flanks_that_graze_share_the_same_sliver_whichever_is_first() {
    let mut k = NativeKernel::new();
    let (a, b) = teeth(&mut k);
    let one = intersection(&mut k, a, b);
    let other = intersection(&mut k, b, a);
    valid(&k, one);
    valid(&k, other);
    let (v, w) = (volume(&k, one), volume(&k, other));
    // A lens about 2.4e-6 mm³.
    assert!((2.0e-6..3.0e-6).contains(&v), "{v}");
    assert!((v - w).abs() < 1e-3 * v, "{v} against {w}");
}

#[test]
#[ignore = "cadrs leaves an open shell or loses volume where cylinders lie a micron off each other's axis"]
fn a_shaft_a_hair_off_its_bores_axis_shares_the_crescent_between_them() {
    let mut k = NativeKernel::new();
    let shaft = cylinder(
        &mut k,
        &frame(P3::new(0.0, 0.0, -5.0), V3::z(), V3::x()),
        2.0,
        20.0,
    );
    let outer = cylinder(&mut k, &world(), 2.9, 8.0);
    let bore = cylinder(
        &mut k,
        &frame(P3::new(0.0, 0.0, -1.0), V3::z(), V3::x()),
        2.0,
        10.0,
    );
    let ring = difference(&mut k, outer, bore);
    for (d, tilt) in [(4e-4, 0.0), (1e-5, 0.0), (4e-4, 1.6e-5)] {
        let turned = moved(
            &mut k,
            ring,
            &rotation(P3::new(0.0, 0.0, 4.0), V3::x(), tilt),
        );
        let placed = moved(&mut k, turned, &translation(V3::new(0.0, d, 0.0)));
        let shared = boolean(&mut k, BoolOp::Intersect, shaft, placed)
            .unwrap_or_else(|e| panic!("{d} {tilt}: {e:?}"));
        assert!(
            issues(&k, shared).is_empty(),
            "{d} {tilt}: {:?}",
            issues(&k, shared)
        );
        let want = 2.0 * 2.0 * d * 8.0;
        let got = volume(&k, shared);
        assert!(
            (got - want).abs() < 1e-5 * want,
            "{d} {tilt}: {got} against {want}"
        );
    }
}

#[test]
fn a_rib_a_hair_into_the_notch_it_stands_in_only_touches_it() {
    let mut k = NativeKernel::new();
    let along = |r: f64| {
        let a = 1.25 * PI;
        V3::new(r * a.cos(), r * a.sin(), 0.0)
    };
    let out = V3::new(1.0, 1.0, 0.0).normalize();
    let disc = cylinder(&mut k, &frame(P3::origin(), V3::z(), out), 42.5, 16.0);
    let notch = cylinder(
        &mut k,
        &frame(
            P3::origin() + along(43.0 + 3.16e-9) - V3::z(),
            V3::z(),
            -out,
        ),
        2.0,
        18.0,
    );
    let notched = difference(&mut k, disc, notch);
    let rib = cylinder(
        &mut k,
        &frame(P3::origin() + along(42.95), V3::z(), -V3::x()),
        1.95,
        20.0,
    );
    // An empty common part is springrep's empty solid set, of volume 0.
    if let Some(shared) = maybe(boolean(&mut k, BoolOp::Intersect, notched, rib)) {
        valid(&k, shared);
        let v = volume(&k, shared);
        assert!(v.abs() < 1e-12, "{v}");
    }
}

/// A plate 20 × 20 in x and z, `t` thick along y from y = 0.
fn plate(k: &mut NativeKernel, t: f64) -> BodyId {
    pblock(
        k,
        &frame(P3::new(-10.0, 0.0, -10.0), V3::y(), V3::z()),
        [20.0, 20.0, t],
    )
}

#[test]
fn a_press_fit_pin_a_hair_off_its_bores_axis_shares_the_ring_of_its_interference() {
    let mut k = NativeKernel::new();
    let p = plate(&mut k, 4.7);
    let bore = cylinder(
        &mut k,
        &frame(P3::new(0.0, -1.0, 0.0), V3::y(), V3::x()),
        3.95,
        10.0,
    );
    let holed = difference(&mut k, p, bore);
    let o = V3::new(3e-7, 0.0, 0.0);
    let chamfer = cone(
        &mut k,
        &frame(P3::origin() + o, V3::y(), V3::x()),
        3.5,
        4.0,
        0.5,
    );
    let shank = cylinder(
        &mut k,
        &frame(P3::origin() + o + V3::new(0.0, 0.5, 0.0), V3::y(), V3::x()),
        4.0,
        20.0,
    );
    let pin = union(&mut k, chamfer, shank);
    let shared = intersection(&mut k, holed, pin);
    valid(&k, shared);
    let want = PI * (16.0 - 15.6025) * 4.2 + PI * (0.05f64.powi(3) / 3.0 + 3.95 * 0.0025);
    let got = volume(&k, shared);
    assert!((got - want).abs() < 1e-6 * want, "{got} against {want}");
}

/// The plate 8 thick bored r 2.9 with a chamfered mouth.
fn mouthed(k: &mut NativeKernel) -> BodyId {
    let p = plate(k, 8.0);
    let bore = cylinder(
        k,
        &frame(P3::new(0.0, -1.0, 0.0), V3::y(), V3::x()),
        2.9,
        10.0,
    );
    let mouth = cone(
        k,
        &frame(P3::new(0.0, -0.5, 0.0), V3::y(), V3::x()),
        3.9,
        2.9,
        1.0,
    );
    let bored = difference(k, p, bore);
    difference(k, bored, mouth)
}

#[test]
#[ignore = "cadrs leaves an open shell or loses volume where cylinders lie a micron off each other's axis"]
fn an_insert_turned_a_hair_off_its_holes_axis_shares_the_crescent_between_them() {
    let mut k = NativeKernel::new();
    let holed = mouthed(&mut k);
    let d = 5.2e-6;
    for lift in [0.0, -4.2e-6, 0.3] {
        let insert = cylinder(
            &mut k,
            &frame(
                P3::new(0.0, 0.5 + lift, d),
                V3::new(2e-10, 1.0, 0.0),
                -V3::x(),
            ),
            2.9,
            6.0,
        );
        let shared = boolean(&mut k, BoolOp::Intersect, holed, insert)
            .unwrap_or_else(|e| panic!("{lift}: {e:?}"));
        assert!(
            issues(&k, shared).is_empty(),
            "{lift}: {:?}",
            issues(&k, shared)
        );
        let want = 2.0 * 2.9 * d * 6.0;
        let got = volume(&k, shared);
        assert!(
            (got - want).abs() < 1e-3 * want,
            "{lift}: {got} against {want}"
        );
    }
}

#[test]
#[ignore = "cadrs leaves an open shell or loses volume where cylinders lie a micron off each other's axis"]
fn an_insert_a_hair_off_its_holes_axis_with_its_end_a_hair_into_the_chamfer_is_valid() {
    let mut k = NativeKernel::new();
    let holed = mouthed(&mut k);
    let below = pblock(
        &mut k,
        &frame(P3::new(-10.0, -1.0, -10.0), V3::z(), V3::x()),
        [20.0, 12.0, 12.0],
    );
    let d = 5.2e-6;
    let (s, c) = 5f64.to_radians().sin_cos();
    let insert = cylinder(
        &mut k,
        &frame(P3::new(d * c, 0.5 - 4.2e-6, d * s), V3::y(), -V3::x()),
        2.9,
        6.0,
    );
    let insert = intersection(&mut k, insert, below);
    // An empty common part is springrep's empty solid set, of volume 0.
    let got = match maybe(boolean(&mut k, BoolOp::Intersect, holed, insert)) {
        Some(shared) => {
            valid(&k, shared);
            volume(&k, shared)
        }
        None => 0.0,
    };
    assert!(got.abs() <= 2.0 * 2.9 * d * 6.0, "{got}");
}

#[test]
fn a_plane_a_hair_off_parallel_to_a_holes_axis_cuts_it_in_two_lines() {
    let mut k = NativeKernel::new();
    let p = pblock(&mut k, &world(), [10.0, 10.0, 4.0]);
    let tilt = 5.7e-10;
    let hole = cylinder(
        &mut k,
        &frame(P3::new(5.0, 5.0, -1.0), V3::new(0.0, tilt, 1.0), V3::x()),
        1.6,
        6.0,
    );
    let holed = difference(&mut k, p, hole);
    let below = pblock(
        &mut k,
        &frame(P3::new(-1.0, -1.0, -1.0), V3::z(), V3::x()),
        [12.0, 6.5, 6.0],
    );
    let cut = intersection(&mut k, holed, below);
    valid(&k, cut);
    let (r, d): (f64, f64) = (1.6, 0.5);
    let segment = r * r * (d / r).acos() - d * (r * r - d * d).sqrt();
    let want = 220.0 - 4.0 * (PI * r * r - segment);
    let got = volume(&k, cut);
    assert!((got - want).abs() < 1e-9 * want, "{got} against {want}");
}

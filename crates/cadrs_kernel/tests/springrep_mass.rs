//! Ported from springrep's tests/mass.rs (MIT, https://github.com/springcraft-ai/springrep): mass
//! properties against closed forms: volume, centre and the inertia tensor about the centre (both
//! kernels' tensors carry the products of inertia with their minus sign).

mod springrep_port;

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use springrep_port::*;

/// The diagonal tensor with these moments about the axes.
fn diagonal(i: [f64; 3]) -> [[f64; 3]; 3] {
    [[i[0], 0.0, 0.0], [0.0, i[1], 0.0], [0.0, 0.0, i[2]]]
}

fn assert_mass(k: &NativeKernel, b: BodyId, vol: f64, centre: [f64; 3], inertia: [[f64; 3]; 3]) {
    let m = k.mass_properties(b).unwrap();
    let (v, c, i) = (m.volume, m.center_of_mass, m.inertia);
    assert!((v - vol).abs() <= 1e-12 * vol, "volume {v} vs {vol}");
    let size = vol.cbrt();
    let d = (c - p3(centre)).amax();
    assert!(d <= 1e-12 * size, "centre {c:?} vs {centre:?}");
    let scale = inertia.iter().flatten().fold(0.0f64, |m, x| m.max(x.abs()));
    for r in 0..3 {
        for q in 0..3 {
            assert!(
                (i[(r, q)] - inertia[r][q]).abs() <= 1e-11 * scale,
                "inertia {i:?} vs {inertia:?}"
            );
        }
    }
}

fn xz() -> Plane {
    frame(P3::origin(), -V3::y(), V3::x())
}

#[test]
fn a_box_off_the_origin_has_the_box_formulas() {
    let mut k = NativeKernel::new();
    let f = frame(P3::new(0.0, 0.0, 3.0), V3::z(), V3::x());
    let b = extrude(&mut k, f, rect(6.0, 5.0, 10.0, 6.0), 3.0);
    let v = 180.0;
    let i = [v * 45.0 / 12.0, v * 109.0 / 12.0, v * 136.0 / 12.0];
    assert_mass(&k, b, v, [6.0, 5.0, 4.5], diagonal(i));
}

#[test]
fn a_cylinder_off_the_axis_has_the_cylinder_formulas() {
    let mut k = NativeKernel::new();
    let b = extrude(&mut k, world(), circle(2.0, -1.0, 4.0), 5.0);
    let v = 80.0 * PI;
    let across = v * (48.0 + 25.0) / 12.0;
    assert_mass(
        &k,
        b,
        v,
        [2.0, -1.0, 2.5],
        diagonal([across, across, v * 8.0]),
    );
}

#[test]
fn a_ball_has_two_fifths_v_r_squared_about_every_axis() {
    let mut k = NativeKernel::new();
    let r = 3.0;
    let b = revolve(&mut k, xz(), half_disc(r), P3::origin(), V3::z());
    let v = 4.0 / 3.0 * PI * r * r * r;
    let i = 0.4 * v * r * r;
    assert_mass(&k, b, v, [0.0; 3], diagonal([i, i, i]));
}

#[test]
fn a_torus_has_the_torus_formulas() {
    let mut k = NativeKernel::new();
    let (big, a) = (10.0, 2.0);
    let b = revolve(&mut k, xz(), circle(big, 0.0, a), P3::origin(), V3::z());
    let v = 2.0 * PI * PI * big * a * a;
    let across = v * (big * big / 2.0 + 5.0 * a * a / 8.0);
    let along = v * (big * big + 0.75 * a * a);
    assert_mass(&k, b, v, [0.0; 3], diagonal([across, across, along]));
}

#[test]
fn an_l_shaped_plate_has_products_of_inertia() {
    let mut k = NativeKernel::new();
    let pts = [
        [0.0, 0.0],
        [4.0, 0.0],
        [4.0, 2.0],
        [2.0, 2.0],
        [2.0, 6.0],
        [0.0, 6.0],
    ];
    let b = extrude(&mut k, world(), Loop::polygon(&pts), 1.0);
    let centre = [1.5, 2.5, 0.5];
    let mut want = [[0.0; 3]; 3];
    for (size, at) in [
        ([4.0, 2.0, 1.0], [2.0, 1.0, 0.5]),
        ([2.0, 4.0, 1.0], [1.0, 4.0, 0.5]),
    ] {
        let v: f64 = size.iter().product();
        let s2 = size.map(|x: f64| x * x);
        let own = [
            v * (s2[1] + s2[2]) / 12.0,
            v * (s2[0] + s2[2]) / 12.0,
            v * (s2[0] + s2[1]) / 12.0,
        ];
        let d = [at[0] - centre[0], at[1] - centre[1], at[2] - centre[2]];
        let dd: f64 = d.iter().map(|x| x * x).sum();
        for r in 0..3 {
            for q in 0..3 {
                let shift = v * (if r == q { dd } else { 0.0 } - d[r] * d[q]);
                want[r][q] += shift + if r == q { own[r] } else { 0.0 };
            }
        }
    }
    assert!(want[0][1] != 0.0);
    assert_mass(&k, b, 16.0, centre, want);
}

#[test]
fn a_tilted_cylinder_turns_its_tensor_with_it() {
    let mut k = NativeKernel::new();
    let z = V3::new(1.0, 2.0, 2.0) * (1.0 / 3.0);
    let x = V3::new(2.0, -2.0, 1.0) * (1.0 / 3.0);
    let o = P3::new(1.0, -3.0, 2.0);
    let (r, h) = (1.5, 6.0);
    let b = extrude(&mut k, frame(o, z, x), circle(0.0, 0.0, r), h);
    let v = PI * r * r * h;
    let (across, along) = (v * (3.0 * r * r + h * h) / 12.0, v * r * r / 2.0);
    let zs = [z.x, z.y, z.z];
    let mut want = [[0.0; 3]; 3];
    for i in 0..3 {
        for q in 0..3 {
            want[i][q] = if i == q { across } else { 0.0 } + (along - across) * zs[i] * zs[q];
        }
    }
    let c = o + z * (h / 2.0);
    assert_mass(&k, b, v, [c.x, c.y, c.z], want);
}

#[test]
fn a_lofted_cylinder_cut_on_a_slant_measures_its_closed_form_every_way() {
    // A smooth loft through equal circles, below the plane z = h + x·tan θ: πr²h, and its area the
    // disc, the ellipse πr²/cos θ and the side 2πrh.
    let mut k = NativeKernel::new();
    let (r, h, theta) = (5.0, 7.0, 0.3);
    let sections: Vec<Profile> = [0.0, 5.0, 10.0, 15.0]
        .iter()
        .map(|&z| Profile::of(Plane::xy(z), vec![circle(0.0, 0.0, r)]))
        .collect();
    let can = k
        .loft_with(&LoftSpec::through(sections))
        .unwrap()
        .first()
        .unwrap();
    // (springrep checks its loft is a NURBS surface, and that the cut's edge on it is a fitted
    // intersection curve with projected pcurves: its own representation, no counterpart.)
    let below = frame(P3::new(0.0, 0.0, -100.0), V3::z(), V3::x());
    let cut0 = extrude(&mut k, below, rect(0.0, 0.0, 100.0, 100.0), 100.0);
    let t = translation(V3::new(0.0, 0.0, h)) * rotation(P3::origin(), V3::y(), -theta);
    let cut = moved(&mut k, cut0, &t);
    let b = intersection(&mut k, can, cut);
    // springrep measures the volume four ways (rough sign, to 1e-10, exact, moments); cadrs has
    // one, its mass properties.
    let want = PI * r * r * h;
    let got = volume(&k, b);
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    let ar = PI * r * r * (1.0 + 1.0 / theta.cos()) + TAU * r * h;
    let got = area(&k, b);
    assert!((got - ar).abs() <= 1e-9 * ar, "area {got} vs {ar}");
}

#[test]
fn a_ruled_loft_cylinder_cut_on_a_slant_measures_its_closed_form() {
    // The lofted cylinder above through two circles only: a ruled B-spline (degree 1 along its
    // rulings), whose trimmed side mass properties integrate along the rulings.
    let mut k = NativeKernel::new();
    let (r, h, theta) = (5.0, 7.0, 0.3);
    let sections: Vec<Profile> = [0.0, 15.0]
        .iter()
        .map(|&z| Profile::of(Plane::xy(z), vec![circle(0.0, 0.0, r)]))
        .collect();
    let can = k
        .loft_with(&LoftSpec::through(sections))
        .unwrap()
        .first()
        .unwrap();
    let below = frame(P3::new(0.0, 0.0, -100.0), V3::z(), V3::x());
    let cut0 = extrude(&mut k, below, rect(0.0, 0.0, 100.0, 100.0), 100.0);
    let t = translation(V3::new(0.0, 0.0, h)) * rotation(P3::origin(), V3::y(), -theta);
    let cut = moved(&mut k, cut0, &t);
    let b = intersection(&mut k, can, cut);
    let want = PI * r * r * h;
    let got = volume(&k, b);
    assert!((got - want).abs() <= 1e-9 * want, "volume {got} vs {want}");
    let ar = PI * r * r * (1.0 + 1.0 / theta.cos()) + TAU * r * h;
    let got = area(&k, b);
    assert!((got - ar).abs() <= 1e-9 * ar, "area {got} vs {ar}");
    // The centre: below z = h + x·tan θ, x̄ = r² tan θ / (4h), z̄ = h/2 + r² tan² θ / (8h).
    let m = k.mass_properties(b).unwrap();
    let tn = theta.tan();
    let c = P3::new(
        r * r * tn / (4.0 * h),
        0.0,
        h / 2.0 + r * r * tn * tn / (8.0 * h),
    );
    assert!(
        (m.center_of_mass - c).amax() <= 1e-9 * h,
        "centre {:?} vs {c:?}",
        m.center_of_mass
    );
}

//! Ported from springrep's tests/transforms.rs (MIT, https://github.com/springcraft-ai/springrep):
//! solids mirrored and scaled uniformly: a similarity, a uniform scale times a rotation or a
//! reflection, then a translation. Each result is checked valid and measured against the original
//! moved by hand: volume times s³, area times s², the centre moved, and the inertia turned by the
//! rotation and scaled by s⁵.
//!
//! springrep's `Brep::transform_similar(m, t)` is here cadrs's `scale` by s about the origin then
//! `transform_motion` by m / s (a rotation or a reflection) and t.

mod springrep_port;

use std::f64::consts::{PI, TAU};

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use nalgebra::Matrix3;
use springrep_port::*;

/// A `2n`-sided mesh of a cylinder about z, its upper ring half a side round from its lower, as
/// a tool that turns each ring on its own writes one: every side triangle a hair off the axis.
fn twisted_prism(k: &mut NativeKernel, n: u32, r: f64, h: f64) -> BodyId {
    let mut points = Vec::new();
    for i in 0..n {
        let a = TAU * f64::from(i) / f64::from(n);
        points.push(P3::new(r * a.cos(), r * a.sin(), 0.0));
        let a = a + PI / f64::from(n);
        points.push(P3::new(r * a.cos(), r * a.sin(), h));
    }
    points.extend([P3::new(0.0, 0.0, 0.0), P3::new(0.0, 0.0, h)]);
    let (low, high) = (2 * n, 2 * n + 1);
    let mut triangles = Vec::new();
    for i in 0..n {
        let j = (i + 1) % n;
        let (a, b, c, d) = (2 * i, 2 * j, 2 * j + 1, 2 * i + 1);
        triangles.extend([[a, b, d], [b, c, d], [low, b, a], [high, d, c]]);
    }
    let tris: Vec<[P3; 3]> = triangles
        .iter()
        .map(|t| t.map(|i| points[i as usize]))
        .collect();
    k.mesh_solid(&tris, 1e-7).unwrap()
}

/// A turn about z then a quarter turn about x, as another program writes it: to nine digits,
/// which leaves it 3e-10 from a rotation.
const NINE_DIGITS: [[f64; 3]; 3] = [
    [0.963144594, 0.268984183, 0.0],
    [0.0, 0.0, 1.0],
    [0.268984183, -0.963144594, 0.0],
];

fn mat(m: [[f64; 3]; 3]) -> Matrix3<f64> {
    Matrix3::from_fn(|i, j| m[i][j])
}

/// springrep's `transform_similar(m, t)`: scaled by s = |det m|^⅓ about the origin, then moved by
/// m / s and t.
fn similar(k: &mut NativeKernel, b: BodyId, m: [[f64; 3]; 3], t: V3) -> Result<BodyId> {
    let m = mat(m);
    let s = m.determinant().abs().cbrt();
    let scaled = if (s - 1.0).abs() > 1e-15 {
        k.scale(b, P3::origin(), s)?.first()?
    } else {
        b
    };
    let motion = Motion {
        linear: m / s,
        translation: t,
    };
    k.transform_motion(scaled, &motion)?.first()
}

#[test]
fn a_matrix_written_to_nine_digits_moves_a_solid_as_the_rotation_it_stands_for() {
    // The prism's axis lands on y; a cylinder along y, 1 mm aside, shares a lens with it.
    let mut k = NativeKernel::new();
    let p0 = twisted_prism(&mut k, 45, 2.0, 18.0);
    let prism = similar(&mut k, p0, NINE_DIGITS, V3::zeros()).unwrap();
    valid(&k, prism);
    let f = frame(P3::new(1.0, 5.0, 0.0), V3::y(), V3::x());
    let tool = cylinder(&mut k, &f, 2.0, 5.0);
    let shared = intersection(&mut k, tool, prism);
    valid(&k, shared);
    let lens = 8.0 * (0.25_f64).acos() - 0.5 * 15.0_f64.sqrt();
    let v = volume(&k, shared);
    assert!((5.0 * lens - 0.13..5.0 * lens).contains(&v), "{v}");
}

#[test]
fn a_rigid_motion_from_outside_is_the_nearest_rotation() {
    // springrep's `Transform::rigid` (the nearest rotation to a matrix read from outside, and its
    // refusal of a mirror or a stretch) has no counterpart in cadrs's API; cadrs's `Motion` takes
    // its linear part as given.
}

/// springrep's test `part`: a block with a ball on it and a rod bored through both.
fn part(k: &mut NativeKernel) -> BodyId {
    let a = block(k, [1.0, 2.0, 0.0], [9.0, 7.0, 4.0]);
    let b = ball(k, [4.0, 4.0, 4.0], 2.5);
    let base = union(k, a, b);
    let c = rod(k, [6.0, 4.5, -1.0], [0.0, 0.0, 1.0], 1.2, 9.0);
    difference(k, base, c)
}

/// A torus of major radius 5 and minor 1.5 about (2, 1, 0), and a prism on a closed spline.
fn curved(k: &mut NativeKernel) -> BodyId {
    let f = frame(P3::new(2.0, 1.0, 0.0), -V3::y(), V3::x());
    let torus = revolve(k, f, circle(5.0, 0.0, 1.5), P3::new(2.0, 1.0, 0.0), V3::z());
    let pts =
        [(0.0, 0.0), (4.0, -1.0), (6.0, 2.0), (3.0, 5.0), (-1.0, 3.0)].map(|(x, y)| [x + 12.0, y]);
    let prism = extrude(k, world(), periodic_spline(&pts), 3.0);
    union(k, torus, prism)
}

/// `b` moved by the similarity `m`, `t`, of uniform scale `s`, checked against its own measures
/// moved by hand.
fn assert_moved(k: &mut NativeKernel, b: BodyId, m: [[f64; 3]; 3], t: V3, s: f64) -> BodyId {
    let out = similar(k, b, m, t).unwrap();
    valid(k, out);
    let (before, after) = (
        k.mass_properties(b).unwrap(),
        k.mass_properties(out).unwrap(),
    );
    let rel = |got: f64, want: f64| (got - want).abs() <= 1e-9 * want.abs().max(1.0);
    assert!(
        rel(after.volume, before.volume * s.powi(3)),
        "volume {} vs {}",
        after.volume,
        before.volume * s.powi(3)
    );
    assert!(
        rel(after.surface_area, before.surface_area * s * s),
        "area {} vs {}",
        after.surface_area,
        before.surface_area * s * s
    );
    let centre = mat(m) * before.center_of_mass + t;
    assert!(
        (after.center_of_mass - centre).norm() <= 1e-9 * (1.0 + centre.coords.norm()),
        "{:?} vs {centre:?}",
        after.center_of_mass
    );
    let q = mat(m) / s;
    let want = q * before.inertia * q.transpose() * s.powi(5);
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                rel(after.inertia[(i, j)], want[(i, j)]),
                "inertia {i}{j}: {} vs {}",
                after.inertia[(i, j)],
                want[(i, j)]
            );
        }
    }
    out
}

const MIRROR_X: [[f64; 3]; 3] = [[-1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn a_part_mirrored_across_a_plane_is_its_mirror_image() {
    let mut k = NativeKernel::new();
    let p = part(&mut k);
    assert_moved(&mut k, p, MIRROR_X, V3::zeros(), 1.0);
    // Across the plane x + y = 3.
    let h = std::f64::consts::FRAC_1_SQRT_2;
    let m = [[0.0, -1.0, 0.0], [-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]];
    assert_moved(&mut k, p, m, V3::new(3.0, 3.0, 0.0) * (2.0 * h * h), 1.0);
}

#[test]
fn a_torus_and_a_spline_prism_mirror_and_scale() {
    let mut k = NativeKernel::new();
    let c = curved(&mut k);
    assert_moved(&mut k, c, MIRROR_X, V3::new(1.0, 0.0, 0.0), 1.0);
    let s = 1.7;
    let m = MIRROR_X.map(|row| row.map(|x| x * s));
    assert_moved(&mut k, c, m, V3::new(0.0, -2.0, 3.0), s);
}

#[test]
fn a_part_scaled_turned_and_mirrored_at_once() {
    let mut k = NativeKernel::new();
    let p = part(&mut k);
    let s = 1.5;
    let m = [[0.0, -s, 0.0], [s, 0.0, 0.0], [0.0, 0.0, -s]];
    assert_moved(&mut k, p, m, V3::new(4.0, -1.0, 2.0), s);
    let s = 0.4;
    let m = [[0.0, 0.0, s], [s, 0.0, 0.0], [0.0, s, 0.0]];
    assert_moved(&mut k, p, m, V3::zeros(), s);
}

#[test]
fn a_part_mirrored_and_joined_to_itself() {
    let mut k = NativeKernel::new();
    let a = part(&mut k);
    let b = similar(&mut k, a, MIRROR_X, V3::new(4.0, 0.0, 0.0)).unwrap();
    let u = union(&mut k, a, b);
    let i = intersection(&mut k, a, b);
    for x in [u, i] {
        valid(&k, x);
    }
    let v = volume(&k, a);
    let sum = volume(&k, u) + volume(&k, i);
    assert!(
        (sum - 2.0 * v).abs() <= 1e-7 * area(&k, a),
        "{sum} vs {}",
        2.0 * v
    );
}

#[test]
fn a_stretch_or_shear_is_refused() {
    // springrep refuses with `InvalidInput`; any refusal counts here.
    let mut k = NativeKernel::new();
    let b = part(&mut k);
    let stretch = [[2.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(similar(&mut k, b, stretch, V3::zeros()).is_err());
    let shear = [[1.0, 0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    assert!(similar(&mut k, b, shear, V3::zeros()).is_err());
}

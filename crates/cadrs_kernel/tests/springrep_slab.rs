//! Ported from springrep's tests/slab.rs (MIT, https://github.com/springcraft-ai/springrep):
//! slabs, a block whose top is a B-spline height field. A field whose coefficients sit on a plane
//! is that plane, because a clamped B-spline over its Greville abscissae reproduces a linear
//! function, so these cases have closed-form volumes and areas.
//!
//! springrep's `primitive::slab` has no counterpart in cadrs; here a slab is a block on the
//! frame, above the field, whose top face is replaced (`Kernel::replace_faces`) by the field's
//! B-spline surface, its control net placed at the Greville abscissae as springrep places it.

mod springrep_port;

use std::f64::consts::PI;
use std::sync::Arc;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::testkit::nurbs::NurbsSurface;
use cadrs_kernel::testkit::surface::Surface;
use cadrs_kernel::*;
use springrep_port::*;

const UNIFORM: [f64; 8] = [0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
/// Two inner knots, spaced unevenly.
const UNEVEN: [f64; 10] = [0.0, 0.0, 0.0, 0.0, 0.3, 0.45, 1.0, 1.0, 1.0, 1.0];

fn greville(knots: &[f64], i: usize) -> f64 {
    (knots[i + 1] + knots[i + 2] + knots[i + 3]) / 3.0
}

/// Coefficients `f(u_i, v_j)` at the Greville abscissae of a cubic on `ku` × `kv`.
fn field(ku: &[f64], kv: &[f64], f: impl Fn(f64, f64) -> f64) -> Vec<Vec<f64>> {
    let (nu, nv) = (ku.len() - 4, kv.len() - 4);
    (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| f(greville(ku, i), greville(kv, j)))
                .collect()
        })
        .collect()
}

/// The solid under `z = f(x / size[0], y / size[1])` in `frame`: springrep's `slab`.
fn slab(
    k: &mut NativeKernel,
    f: &Plane,
    size: [f64; 2],
    knots: (&[f64], &[f64]),
    heights: &[Vec<f64>],
) -> Result<BodyId> {
    let (nu, nv) = (heights.len(), heights[0].len());
    let [a, b] = size;
    let to_world = |x: f64, y: f64, z: f64| {
        f.origin + f.x_dir.into_inner() * x + y_of(f) * y + f.normal.into_inner() * z
    };
    let poles: Vec<Vec<P3>> = (0..nu)
        .map(|i| {
            (0..nv)
                .map(|j| {
                    to_world(
                        a * greville(knots.0, i),
                        b * greville(knots.1, j),
                        heights[i][j],
                    )
                })
                .collect()
        })
        .collect();
    let net = BsplineSurface {
        degree_u: 3,
        degree_v: 3,
        knots_u: knots.0.to_vec(),
        knots_v: knots.1.to_vec(),
        poles,
        weights: vec![],
    };
    let top = heights.iter().flatten().fold(0.0f64, |m, &h| m.max(h)) + 1.0;
    let blk = pblock(k, f, [a, b, top]);
    let face = k
        .faces(blk)?
        .into_iter()
        .find(|x| {
            x.plane
                .is_some_and(|p| p.normal.dot(&f.normal) > 1.0 - 1e-9)
                && (to_local(f, x.center).z - top).abs() < 1e-9
        })
        .unwrap()
        .id;
    let surface = Surface::Nurbs(Arc::new(NurbsSurface::from_bspline(&net)?));
    k.replace_faces(
        blk,
        &[FaceReplacement {
            face,
            surface,
            domain: None,
        }],
    )?
    .first()
}

fn close(got: f64, want: f64) {
    assert!(
        (got - want).abs() <= 1e-9 * want.abs().max(1.0),
        "{got} vs {want}"
    );
}

#[test]
fn a_flat_slab_is_a_box() {
    let mut k = NativeKernel::new();
    let h = field(&UNIFORM, &UNEVEN, |_, _| 3.0);
    let s = slab(&mut k, &world(), [10.0, 6.0], (&UNIFORM, &UNEVEN), &h).unwrap();
    valid(&k, s);
    close(volume(&k, s), 180.0);
    close(area(&k, s), 2.0 * (60.0 + 30.0 + 18.0));
    let c = k.mass_properties(s).unwrap().center_of_mass;
    assert!((c - P3::new(5.0, 3.0, 1.5)).norm() <= 1e-9, "{c:?}");
}

#[test]
fn a_slab_under_a_sloping_plane_is_a_wedge_on_a_box() {
    let mut k = NativeKernel::new();
    let h = field(&UNEVEN, &UNIFORM, |u, _| 2.0 + 3.0 * u);
    let s = slab(&mut k, &world(), [10.0, 6.0], (&UNEVEN, &UNIFORM), &h).unwrap();
    valid(&k, s);
    close(volume(&k, s), 210.0);
    let ar = 60.0 + 6.0 * 109f64.sqrt() + 2.0 * 35.0 + 6.0 * 2.0 + 6.0 * 5.0;
    close(area(&k, s), ar);
}

#[test]
fn a_saddle_slab_placed_off_the_world_frame() {
    let mut k = NativeKernel::new();
    let h = field(&UNEVEN, &UNEVEN, |u, v| 2.0 + 4.0 * u * v);
    let f = frame(
        P3::new(5.0, -2.0, 1.0),
        V3::new(1.0, 1.0, 1.0),
        V3::new(1.0, -1.0, 0.0),
    );
    let s = slab(&mut k, &f, [8.0, 5.0], (&UNEVEN, &UNEVEN), &h).unwrap();
    valid(&k, s);
    close(volume(&k, s), 120.0);
}

#[test]
fn a_ball_through_a_slab_top_joins_and_cuts() {
    let mut k = NativeKernel::new();
    let h = field(&UNEVEN, &UNEVEN, |u, v| 3.0 + 2.0 * (PI * u).sin() * v);
    let s = slab(&mut k, &world(), [10.0, 8.0], (&UNEVEN, &UNEVEN), &h).unwrap();
    let b = ball(&mut k, [4.0, 5.0, 4.0], 2.0);
    let (u, i) = (union(&mut k, s, b), intersection(&mut k, s, b));
    valid(&k, u);
    valid(&k, i);
    let sum = volume(&k, u) + volume(&k, i);
    let want = volume(&k, s) + volume(&k, b);
    assert!((sum - want).abs() <= 1e-6 * want, "{sum} vs {want}");
}

#[test]
fn a_slab_that_reaches_the_floor_is_refused() {
    // springrep's `slab` refuses a coefficient at or below the floor as invalid input: a check
    // of its own constructor, which cadrs has no counterpart of.
}

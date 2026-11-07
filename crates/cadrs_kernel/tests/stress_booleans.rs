//! Boolean stress (research/PAPER_SCAN.md item 19), no reference kernel needed:
//! - a cube united with (and intersected by) copies of itself rotated by tiny angles about each
//!   axis and a skew one, sweeping across the modelling tolerance;
//! - iterated unions (12 by default) checked after each step for validity, volume against the
//!   closed form, and tolerance drift;
//! - an iterated drill-bit cut chain (EMBER's benchmark): blind 118° drills, each overlapping the
//!   last, cut one at a time; the result must equal the plate less the drills' union cut at once.
//!   Each cut's time is printed (`--nocapture`).
//!
//! Soak knobs: `CADRS_STRESS_ANGLES=n` adds n random angles (log-uniform, 1e-13 to 1e-2) about
//! random axes; `CADRS_STRESS_UNIONS`, `CADRS_STRESS_DRILLS` lengthen the chains;
//! `CADRS_META_SEED` seeds them (`random` for a fresh seed).

mod meta_common;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use meta_common::*;

/// The largest vertex or edge tolerance a result of exact inputs may have.
const TOL_CAP: f64 = 1e-5;

/// The volume of a union of axis-aligned boxes (min, size), exactly: over the cells of the grid
/// their faces make.
fn boxes_union_volume(boxes: &[([f64; 3], [f64; 3])]) -> f64 {
    let axis = |i: usize| {
        let mut c: Vec<f64> = boxes
            .iter()
            .flat_map(|(m, s)| [m[i], m[i] + s[i]])
            .collect();
        c.sort_by(f64::total_cmp);
        c.dedup();
        c
    };
    let (xs, ys, zs) = (axis(0), axis(1), axis(2));
    let mut v = 0.0;
    for x in xs.windows(2) {
        for y in ys.windows(2) {
            for z in zs.windows(2) {
                let c = [
                    (x[0] + x[1]) / 2.0,
                    (y[0] + y[1]) / 2.0,
                    (z[0] + z[1]) / 2.0,
                ];
                let inside = boxes
                    .iter()
                    .any(|(m, s)| (0..3).all(|i| c[i] > m[i] && c[i] < m[i] + s[i]));
                if inside {
                    v += (x[1] - x[0]) * (y[1] - y[0]) * (z[1] - z[0]);
                }
            }
        }
    }
    v
}

/// The volumes of cube ∪ turned cube and cube ∩ turned cube to first order in the angle, for the
/// cube [0, 10]³ turned by `angle` about `axis` through `about`: each face of the turned cube
/// lies off the cube's face by δ(x) = angle (ω × (x − c))·n, linear over the face, so the union
/// gains ∫ max(δ, 0) over every face and the common part loses ∫ max(−δ, 0). The error is
/// second order (about angle² × 6·10⁴ here).
fn first_order_volumes(axis: [f64; 3], angle: f64, about: [f64; 3]) -> (f64, f64) {
    let w = nalgebra::Vector3::from(axis).normalize() * angle;
    let c = nalgebra::Vector3::from(about);
    let (mut gain, mut loss) = (0.0, 0.0);
    for i in 0..3 {
        for side in [0.0, 10.0] {
            let mut n = nalgebra::Vector3::zeros();
            n[i] = if side == 0.0 { -1.0 } else { 1.0 };
            let (j, k) = ((i + 1) % 3, (i + 2) % 3);
            // δ at a face point (s, t): linear, so exact over the square clipped where δ > 0.
            let delta = |s: f64, t: f64| {
                let mut x = nalgebra::Vector3::zeros();
                x[i] = side;
                x[j] = s;
                x[k] = t;
                w.cross(&(x - c)).dot(&n)
            };
            let square = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
            for sign in [1.0, -1.0] {
                // Clip the square to sign·δ > 0 (one half-plane: Sutherland–Hodgman).
                let f = |p: (f64, f64)| sign * delta(p.0, p.1);
                let mut poly = Vec::new();
                for e in 0..4 {
                    let (p, q) = (square[e], square[(e + 1) % 4]);
                    let (fp, fq) = (f(p), f(q));
                    if fp > 0.0 {
                        poly.push(p);
                    }
                    if (fp > 0.0) != (fq > 0.0) {
                        let u = fp / (fp - fq);
                        poly.push((p.0 + u * (q.0 - p.0), p.1 + u * (q.1 - p.1)));
                    }
                }
                // ∫ over the polygon of a linear function: by triangles, area × the mean of
                // its corners' values.
                let mut v = 0.0;
                for e in 1..poly.len().saturating_sub(1) {
                    let (p, q, r) = (poly[0], poly[e], poly[e + 1]);
                    let area = ((q.0 - p.0) * (r.1 - p.1) - (r.0 - p.0) * (q.1 - p.1)).abs() / 2.0;
                    v += area * (f(p) + f(q) + f(r)) / 3.0;
                }
                if sign > 0.0 {
                    gain += v;
                } else {
                    loss += v;
                }
            }
        }
    }
    (1000.0 + gain, 1000.0 - loss)
}

/// One tiny-rotation case: the failures of cube ∪ turned cube and cube ∩ turned cube, in both
/// tool orders. Up to an angle of 1e-4 each volume must match the first-order volumes within
/// the result's own tolerance band; beyond, the union and the common part must still add up.
fn tiny_rotation(axis: [f64; 3], angle: f64, about: [f64; 3]) -> Vec<String> {
    let mut k = NativeKernel::new();
    let cube = Shape::Box {
        min: [0.0; 3],
        size: [10.0; 3],
    };
    let a = build(&mut k, &Placed::fixed(cube)).unwrap();
    let rot = Placed {
        shape: cube,
        axis,
        angle,
        about,
    };
    let b = build(&mut k, &rot).unwrap();
    // How far any point of the cube moves: |angle| × the farthest corner from the centre.
    let reach =
        angle.abs() * 3f64.sqrt() * (10.0 + about.iter().map(|c| c.abs()).fold(0.0, f64::max));
    let slack = 600.0 * reach + 1e-7 * 1000.0;
    let truth = (angle.abs() <= 1e-4).then(|| first_order_volumes(axis, angle, about));
    let mut bad = Vec::new();
    let mut vols = [f64::NAN; 2];
    let mut bands = [0.0; 2];
    for (x, y, order) in [(a, b, ""), (b, a, " (turned cube first)")] {
        for (n, (op, what)) in [(BoolOp::Union, "∪"), (BoolOp::Intersect, "∩")]
            .into_iter()
            .enumerate()
        {
            let what = format!("{what}{order}");
            match boolean(&mut k, op, x, &[y]) {
                Ok(r) => {
                    if let Some(why) = invalid(&k, &r) {
                        bad.push(format!("{what}: invalid: {why}"));
                    }
                    if r.len() != 1 {
                        bad.push(format!("{what}: {} bodies", r.len()));
                    }
                    let t = max_tol(&k, &r);
                    if t > TOL_CAP.max(reach) {
                        bad.push(format!("{what}: tolerance {t:e}"));
                    }
                    let v = volume(&k, &r).unwrap_or(f64::NAN);
                    let bd = band(&k, &r);
                    if !((v - 1000.0).abs() <= slack + bd) {
                        bad.push(format!("{what}: volume {v} (1000 ± {slack:e})"));
                    }
                    if let Some((tu, ti)) = truth {
                        let want = if n == 0 { tu } else { ti };
                        let allow = bd + 1e-7 * 1000.0 + 6e4 * angle * angle;
                        if !((v - want).abs() <= allow) {
                            bad.push(format!(
                                "{what}: volume {v}, first-order {want} (off {:.2e}, tolerance band {bd:.1e})",
                                v - want
                            ));
                        }
                    }
                    if order.is_empty() {
                        vols[n] = v;
                        bands[n] = bd;
                    }
                }
                Err(e) => bad.push(format!("{what}: {e:?}")),
            }
        }
    }
    if !((vols[0] + vols[1] - 2000.0).abs() <= 1e-7 * 2000.0 + bands[0] + bands[1]) {
        bad.push(format!("vol(∪) + vol(∩) = {} (2000)", vols[0] + vols[1]));
    }
    bad
}

#[test]
fn cube_and_tiny_rotations() {
    tiny_rotations(
        &[1e-12, 1e-10, 1e-8, 1e-7, 1e-4, 1e-2],
        env_num("CADRS_STRESS_ANGLES", 0),
    );
}

/// Tilts of 1e-6 and 1e-5 (the cube's corners move 1e-5 to 1e-4): the boolean takes the
/// near-coincident faces as one and returns the cube itself, in both tool orders, the union
/// ~500·angle of volume short and the common part as much over; its faces are up to ~9e-5 from
/// where the inputs had them, and its tolerances must say so (they were left at 1e-7).
#[test]
fn cube_tilted_into_coincidence() {
    tiny_rotations(&[1e-6, 1e-5], 0);
}

/// The tiny-rotation cases at `angles` about each axis and a skew one through the cube's centre
/// (and the skew one through its corner), plus `random` random ones.
fn tiny_rotations(angles: &[f64], random: usize) {
    let axes = [
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [1.0, 1.0, 1.0],
    ];
    let mut cases: Vec<([f64; 3], f64, [f64; 3])> = Vec::new();
    for &angle in angles {
        for axis in axes {
            // About the cube's centre, and (for a tilt on top of the shift) about its corner.
            cases.push((axis, angle, [5.0; 3]));
        }
        cases.push(([1.0, 1.0, 1.0], angle, [0.0; 3]));
    }
    let mut r = Rng::new(seed(7));
    for _ in 0..random {
        let axis = [r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)];
        let angle = 10f64.powf(r.range(-13.0, -2.0)) * if r.below(2) == 0 { 1.0 } else { -1.0 };
        let about = [r.range(0.0, 10.0), r.range(0.0, 10.0), r.range(0.0, 10.0)];
        cases.push((axis, angle, about));
    }
    let mut fails = Vec::new();
    for (axis, angle, about) in &cases {
        let bad = tiny_rotation(*axis, *angle, *about);
        if !bad.is_empty() {
            fails.push(format!(
                "axis {axis:?} angle {angle:e} about {about:?}: {}",
                bad.join("; ")
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} of {} cases fail:\n{}",
        fails.len(),
        cases.len(),
        fails.join("\n")
    );
}

/// Boxes in a staircase, each overlapping the last and some earlier ones, united one at a time:
/// after each union the result is valid, its volume the exact union's, its tolerance exact.
#[test]
fn iterated_box_unions() {
    let n = env_num("CADRS_STRESS_UNIONS", 12usize);
    let mut r = Rng::new(seed(11));
    let boxes: Vec<([f64; 3], [f64; 3])> = (0..n)
        .map(|i| {
            let jitter = if i == 0 {
                0.0
            } else {
                r.snapped(-0.5, 0.5, 0.25)
            };
            let min = [
                i as f64 * 2.5 + jitter,
                (i % 3) as f64 * 1.5,
                (i % 2) as f64 * 1.25,
            ];
            (min, [5.0, 4.0 + (i % 4) as f64 * 0.5, 3.0])
        })
        .collect();
    let mut k = NativeKernel::new();
    let mut acc = build(
        &mut k,
        &Placed::fixed(Shape::Box {
            min: boxes[0].0,
            size: boxes[0].1,
        }),
    )
    .unwrap();
    let mut fails = Vec::new();
    for i in 1..n {
        let b = build(
            &mut k,
            &Placed::fixed(Shape::Box {
                min: boxes[i].0,
                size: boxes[i].1,
            }),
        )
        .unwrap();
        let r = match boolean(&mut k, BoolOp::Union, acc, &[b]) {
            Ok(r) if r.len() == 1 => r,
            other => {
                fails.push(format!("union {i}: {other:?}"));
                break;
            }
        };
        acc = r[0];
        let want = boxes_union_volume(&boxes[..=i]);
        let v = volume(&k, &r).unwrap_or(f64::NAN);
        if !((v - want).abs() <= 1e-9 * want) {
            fails.push(format!("union {i}: volume {v}, exact {want}"));
        }
        if let Some(why) = invalid(&k, &r) {
            fails.push(format!("union {i}: invalid: {why}"));
        }
        let t = max_tol(&k, &r);
        if t > TOL_CAP {
            fails.push(format!("union {i}: tolerance {t:e}"));
        }
    }
    assert!(fails.is_empty(), "boxes {boxes:?}:\n{}", fails.join("\n"));
}

/// A cube united with copies of itself turned about its vertical axis by growing small angles:
/// a fan of nearly coincident faces. Each step's volume grows, by no more than the turn sweeps.
#[test]
fn iterated_rotated_unions() {
    let n = env_num("CADRS_STRESS_UNIONS", 12usize);
    let cube = Shape::Box {
        min: [-5.0, -5.0, 0.0],
        size: [10.0; 3],
    };
    let mut k = NativeKernel::new();
    let mut acc = build(&mut k, &Placed::fixed(cube)).unwrap();
    let mut last = 1000.0;
    let mut fails = Vec::new();
    for i in 1..n {
        let angle = 1e-3 * i as f64;
        let b = build(
            &mut k,
            &Placed {
                shape: cube,
                axis: [0.0, 0.0, 1.0],
                angle,
                about: [0.0; 3],
            },
        )
        .unwrap();
        let r = match boolean(&mut k, BoolOp::Union, acc, &[b]) {
            Ok(r) if r.len() == 1 => r,
            other => {
                fails.push(format!("union {i} (angle {angle}): {other:?}"));
                break;
            }
        };
        acc = r[0];
        let v = volume(&k, &r).unwrap_or(f64::NAN);
        // A square of side 10 turned by δ about its centre sticks out of the last one by 4
        // slivers of about ½·5·5δ each: 500δ of volume over the height 10 (twice that allowed).
        if !(v >= last - 1e-9 * v && v <= last + 2.0 * 500.0 * 1e-3 + 1e-9 * v) {
            fails.push(format!("union {i}: volume {v} after {last}"));
        }
        last = v;
        if let Some(why) = invalid(&k, &r) {
            fails.push(format!("union {i}: invalid: {why}"));
        }
        let t = max_tol(&k, &r);
        if t > TOL_CAP {
            fails.push(format!("union {i}: tolerance {t:e}"));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// The drill positions of the chain: along x, each 1.7 past the last (drills of radius 2.5
/// overlap), wandering a little in y.
fn drills(n: usize) -> Vec<Placed> {
    (0..n)
        .map(|i| {
            Placed::fixed(Shape::Drill {
                tip: [
                    5.0 + 1.7 * i as f64,
                    10.0 + 1.5 * (i as f64 * 0.7).sin(),
                    3.0,
                ],
                r: 2.5,
                len: 20.0,
            })
        })
        .collect()
}

#[test]
fn drill_chain() {
    let n = env_num("CADRS_STRESS_DRILLS", 12usize);
    let plate = Placed::fixed(Shape::Box {
        min: [0.0; 3],
        size: [10.0 + 1.7 * n as f64, 20.0, 10.0],
    });
    let ds = drills(n);
    let mut k = NativeKernel::new();
    let mut acc = build(&mut k, &plate).unwrap();
    let mut last = volume(&k, &[acc]).unwrap();
    let mut fails = Vec::new();
    let t0 = std::time::Instant::now();
    for (i, d) in ds.iter().enumerate() {
        let t = std::time::Instant::now();
        let tool = build(&mut k, d).unwrap();
        let r = match boolean(&mut k, BoolOp::Subtract, acc, &[tool]) {
            Ok(r) if r.len() == 1 => r,
            other => {
                fails.push(format!("cut {i}: {other:?}"));
                break;
            }
        };
        acc = r[0];
        let v = volume(&k, &r).unwrap_or(f64::NAN);
        println!(
            "drill_chain: cut {i} in {:.1} ms, {} faces",
            t.elapsed().as_secs_f64() * 1e3,
            k.get(acc).map(|b| b.faces().len()).unwrap_or(0)
        );
        if !(v < last && v > last - exact_volume(&d.shape)) {
            fails.push(format!("cut {i}: volume {v} after {last}"));
        }
        last = v;
        if let Some(why) = invalid(&k, &r) {
            fails.push(format!("cut {i}: invalid: {why}"));
        }
        let t = max_tol(&k, &r);
        if t > TOL_CAP {
            fails.push(format!("cut {i}: tolerance {t:e}"));
        }
    }
    println!(
        "drill_chain: {n} cuts in {:.1} ms",
        t0.elapsed().as_secs_f64() * 1e3
    );
    // The same holes cut at once: the plate less the drills' union.
    if fails.is_empty() {
        let mut u = build(&mut k, &ds[0]).unwrap();
        for d in &ds[1..] {
            let tool = build(&mut k, d).unwrap();
            match boolean(&mut k, BoolOp::Union, u, &[tool]) {
                Ok(r) if r.len() == 1 => u = r[0],
                other => {
                    fails.push(format!("drills' union: {other:?}"));
                    break;
                }
            }
        }
        if fails.is_empty() {
            let p = build(&mut k, &plate).unwrap();
            match boolean(&mut k, BoolOp::Subtract, p, &[u]) {
                Ok(r) => {
                    let v = volume(&k, &r).unwrap_or(f64::NAN);
                    if !((v - last).abs() <= 1e-7 * last) {
                        fails.push(format!("cut one at a time: {last}; at once: {v}"));
                    }
                }
                Err(e) => fails.push(format!("plate − union: {e:?}")),
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// A tilt of about 1e-5 about a skew axis (the cube's corners move ~1e-4, about the edges'
/// tolerance band): the union and the common part left open shells at every tolerance of the
/// ladder (found by the soak; the tilts 1.1e-5 to 1.5e-5, some with an edge used by three
/// faces), the faces' gap above the tolerance in places and below it in others.
#[test]
fn cube_tilted_near_tolerance() {
    let bad = tiny_rotation(
        [0.5976071051500398, 0.511301556512727, -0.5102939961996196],
        -1.161043810954499e-5,
        [2.809667712018844, 9.873518856878142, 3.249429577303461],
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// A tilt of 1.2e-6 about a skew axis (soak): with the turned cube first, the union and the
/// common part were the turned cube itself, tolerance 1e-7, 9e-4 off the first-order volumes
/// (with the cube first, one merged cube of tolerance 1e-5, within its band).
#[test]
fn cube_tilted_volume_sum() {
    let bad = tiny_rotation(
        [0.4700611935019128, -0.9620889946033548, -0.8509348303880513],
        1.2181008428462193e-6,
        [4.626890525898524, 6.837536705820911, 2.1387233918948976],
    );
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

// --- Soak findings (the build box's worker), replayed at kernel-perf d0bcb72 and still failing,
// pinned by class: each test asserts every tilt of its class passes `tiny_rotation`, and is a
// known failure until they all do.

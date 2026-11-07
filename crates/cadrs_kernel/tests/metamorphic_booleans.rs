//! Metamorphic booleans (research/PAPER_SCAN.md item 18): relations every correct boolean keeps,
//! checked on generated solids without a reference kernel.
//!
//! For each random pair A, B (boxes, cylinders along an axis, spheres, some rotated, half their
//! coordinates on a grid so faces and edges coincide):
//! - vol(A ∪ B) + vol(A ∩ B) = vol(A) + vol(B), and vol(A − B) + vol(A ∩ B) = vol(A);
//! - tool order: vol(B ∪ A) = vol(A ∪ B), vol(B ∩ A) = vol(A ∩ B);
//! - every result valid (`CheckLevel::Full`) and a solid;
//! - the union written to STEP and read back keeps its volume and validity.
//!
//! For each shape on its own: A − A = ∅, A ∪ A = A, A ∩ A = A.
//!
//! `CADRS_META_CASES` sets the number of pairs (default 16, a few seconds), `CADRS_META_SEED` the
//! seed (`random` for a fresh one, printed), `CADRS_META_CASE=i` runs case i alone. A failing case prints its seed, index and shapes;
//! a failure the kernel has today is pinned below as its own `#[ignore = "known failure…"]` test
//! and skipped by the random run (`KNOWN`).

mod meta_common;

use cadrs_kernel::native::NativeKernel;
use cadrs_kernel::*;
use meta_common::*;

/// Relative volume tolerance: mass properties are exact for these surfaces to about 1e-12.
const REL: f64 = 1e-7;

/// (seed, case) pairs of the default run that fail today: each has a pinned test below.
const KNOWN: &[(u64, usize)] = &[];

/// A random shape about the cube [0, 10]³.
fn shape(r: &mut Rng) -> Placed {
    let s = match r.below(3) {
        0 => Shape::Box {
            min: [
                r.snapped(-2.0, 8.0, 1.0),
                r.snapped(-2.0, 8.0, 1.0),
                r.snapped(-2.0, 8.0, 1.0),
            ],
            size: [
                r.snapped(1.0, 8.0, 1.0),
                r.snapped(1.0, 8.0, 1.0),
                r.snapped(1.0, 8.0, 1.0),
            ],
        },
        1 => Shape::Cyl {
            axis: r.below(3),
            base: [
                r.snapped(0.0, 10.0, 1.0),
                r.snapped(0.0, 10.0, 1.0),
                r.snapped(-2.0, 4.0, 1.0),
            ],
            r: r.snapped(0.5, 4.0, 0.5),
            len: r.snapped(1.0, 10.0, 1.0),
        },
        _ => Shape::Sphere {
            c: [
                r.snapped(0.0, 10.0, 1.0),
                r.snapped(0.0, 10.0, 1.0),
                r.snapped(0.0, 10.0, 1.0),
            ],
            r: r.snapped(0.5, 5.0, 0.5),
        },
    };
    let mut p = Placed::fixed(s);
    // A third of them rotated about a random axis through the middle of the cube.
    if r.below(3) == 0 {
        p.axis = [r.range(-1.0, 1.0), r.range(-1.0, 1.0), r.range(-1.0, 1.0)];
        p.angle = r.range(-3.0, 3.0);
        p.about = [5.0; 3];
    }
    p
}

fn close(a: f64, b: f64, scale: f64) -> bool {
    (a - b).abs() <= REL * scale.max(1.0)
}

/// The failures of the relations on one pair (empty when they all hold).
fn check_pair(a: &Placed, b: &Placed) -> Vec<String> {
    let mut k = NativeKernel::new();
    let mut bad = Vec::new();
    let (ia, ib) = match (build(&mut k, a), build(&mut k, b)) {
        (Ok(x), Ok(y)) => (x, y),
        (x, y) => return vec![format!("build: {:?} {:?}", x.err(), y.err())],
    };
    let va = volume(&k, &[ia]).unwrap_or(f64::NAN);
    let vb = volume(&k, &[ib]).unwrap_or(f64::NAN);
    let scale = va + vb;
    for (v, s) in [(va, &a.shape), (vb, &b.shape)] {
        if !close(v, exact_volume(s), scale) {
            bad.push(format!("vol({s:?}) = {v}, closed form {}", exact_volume(s)));
        }
    }
    // The results' tolerance bands, summed: what the relations may be off by beyond rounding.
    let mut bands = 0.0;
    let mut run = |op: BoolOp, x: BodyId, y: BodyId, what: &str| -> Option<(Vec<BodyId>, f64)> {
        match boolean(&mut k, op, x, &[y]) {
            Ok(r) => {
                if let Some(why) = invalid(&k, &r) {
                    bad.push(format!("{what}: invalid: {why}"));
                }
                bands += band(&k, &r);
                match volume(&k, &r) {
                    Ok(v) => Some((r, v)),
                    Err(e) => {
                        bad.push(format!("{what}: volume: {e:?}"));
                        None
                    }
                }
            }
            Err(e) => {
                bad.push(format!("{what}: {e:?}"));
                None
            }
        }
    };
    let u = run(BoolOp::Union, ia, ib, "A ∪ B");
    let i = run(BoolOp::Intersect, ia, ib, "A ∩ B");
    let d = run(BoolOp::Subtract, ia, ib, "A − B");
    let u2 = run(BoolOp::Union, ib, ia, "B ∪ A");
    let i2 = run(BoolOp::Intersect, ib, ia, "B ∩ A");
    let close = |a: f64, b: f64, scale: f64| (a - b).abs() <= REL * scale.max(1.0) + bands;
    if let (Some(u), Some(i)) = (&u, &i)
        && !close(u.1 + i.1, va + vb, scale)
    {
        bad.push(format!(
            "vol(A∪B) + vol(A∩B) = {} + {} = {}, vol(A) + vol(B) = {}",
            u.1,
            i.1,
            u.1 + i.1,
            va + vb
        ));
    }
    if let (Some(d), Some(i)) = (&d, &i)
        && !close(d.1 + i.1, va, scale)
    {
        bad.push(format!(
            "vol(A−B) + vol(A∩B) = {}, vol(A) = {va}",
            d.1 + i.1
        ));
    }
    if let (Some(x), Some(y)) = (&u, &u2)
        && !close(x.1, y.1, scale)
    {
        bad.push(format!("vol(A∪B) = {} but vol(B∪A) = {}", x.1, y.1));
    }
    if let (Some(x), Some(y)) = (&i, &i2)
        && !close(x.1, y.1, scale)
    {
        bad.push(format!("vol(A∩B) = {} but vol(B∩A) = {}", x.1, y.1));
    }
    // The union through STEP and back.
    if let Some((r, v)) = &u {
        let parts: Vec<(BodyId, String)> = r.iter().map(|&b| (b, format!("u{}", b.0))).collect();
        let back = k
            .export_model(exchange::ExchangeFormat::Step, "metamorphic", &parts, &[])
            .and_then(|s| k.import_model(exchange::ExchangeFormat::Step, &s));
        match back {
            Ok(m) => {
                let ids: Vec<BodyId> = m.parts.iter().map(|p| p.body).collect();
                if ids.len() != r.len() {
                    bad.push(format!(
                        "STEP: {} bodies read back of {}",
                        ids.len(),
                        r.len()
                    ));
                }
                if let Some(why) = invalid(&k, &ids) {
                    bad.push(format!("STEP: read back invalid: {why}"));
                }
                match volume(&k, &ids) {
                    Ok(w) if close(w, *v, scale) => {}
                    w => bad.push(format!("STEP: volume {w:?} read back, {v} written")),
                }
            }
            Err(e) => bad.push(format!("STEP round trip: {e:?}")),
        }
    }
    bad
}

/// The failures of A − A = ∅, A ∪ A = A, A ∩ A = A.
fn check_self(a: &Placed) -> Vec<String> {
    let mut k = NativeKernel::new();
    let mut bad = Vec::new();
    let Ok(ia) = build(&mut k, a) else {
        return vec!["build failed".into()];
    };
    // A second, separate body of the same shape.
    let Ok(ib) = build(&mut k, a) else {
        return vec!["build failed".into()];
    };
    let va = volume(&k, &[ia]).unwrap_or(f64::NAN);
    match boolean(&mut k, BoolOp::Subtract, ia, &[ib]) {
        Ok(r) => {
            let v = volume(&k, &r).unwrap_or(f64::NAN);
            if !(v.abs() <= REL * va) {
                bad.push(format!("A − A has volume {v} ({} bodies)", r.len()));
            }
        }
        Err(e) => bad.push(format!("A − A: {e:?}")),
    }
    for (op, what) in [(BoolOp::Union, "A ∪ A"), (BoolOp::Intersect, "A ∩ A")] {
        match boolean(&mut k, op, ia, &[ib]) {
            Ok(r) => {
                if let Some(why) = invalid(&k, &r) {
                    bad.push(format!("{what}: invalid: {why}"));
                }
                let v = volume(&k, &r).unwrap_or(f64::NAN);
                if !close(v, va, va) || r.len() != 1 {
                    bad.push(format!(
                        "{what}: volume {v} in {} bodies, vol(A) = {va}",
                        r.len()
                    ));
                }
            }
            Err(e) => bad.push(format!("{what}: {e:?}")),
        }
    }
    bad
}

/// Runs `n` random cases from `seed`, skipping `KNOWN`; panics listing every failure.
fn random_cases(seed: u64, n: usize) {
    let mut fails = Vec::new();
    let only: Option<usize> = std::env::var("CADRS_META_CASE")
        .ok()
        .and_then(|v| v.parse().ok());
    for case in 0..n {
        if only.is_some_and(|o| o != case) {
            continue;
        }
        // Each case its own stream, so a case reruns alone from (seed, case).
        let mut r = Rng::new(seed ^ (case as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
        let a = shape(&mut r);
        let b = shape(&mut r);
        if KNOWN.contains(&(seed, case)) {
            continue;
        }
        let mut bad = check_self(&a);
        bad.extend(check_pair(&a, &b));
        if !bad.is_empty() {
            fails.push(format!(
                "seed {seed} case {case}:\n  A = {a:?}\n  B = {b:?}\n  {}",
                bad.join("\n  ")
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} of {n} cases fail:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

#[test]
fn random_pairs() {
    let only = env_num("CADRS_META_CASE", 0usize);
    random_cases(seed(1), env_num("CADRS_META_CASES", 16).max(only + 1));
}

/// Pairs that share faces exactly: a box and the same box moved by a whole step along each axis
/// and diagonal (faces, edges and corners in contact), and boxes overlapping by half.
#[test]
fn touching_boxes() {
    let a = Placed::fixed(Shape::Box {
        min: [0.0; 3],
        size: [4.0, 4.0, 4.0],
    });
    let mut fails = Vec::new();
    for d in [
        [4.0, 0.0, 0.0],
        [4.0, 4.0, 0.0],
        [4.0, 4.0, 4.0],
        [2.0, 0.0, 0.0],
        [2.0, 2.0, 2.0],
        [0.0, 0.0, 2.0],
    ] {
        let b = Placed::fixed(Shape::Box {
            min: d,
            size: [4.0, 4.0, 4.0],
        });
        let bad = check_pair(&a, &b);
        if !bad.is_empty() {
            fails.push(format!("B at {d:?}: {}", bad.join("; ")));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// One extrude-sequence step: the shape and how it combines with the model so far.
#[derive(Debug, Clone, Copy)]
struct Step {
    op: BoolOp,
    shape: Shape,
}

/// A random axis-aligned sketch extruded from one of the three planes, as a CAD model's
/// features are (Fusion 360 Gallery's reconstruction sequences: sketch, extrude, join / cut /
/// intersect), on a grid of 0.5 so faces of different steps coincide.
fn prism(r: &mut Rng) -> Shape {
    let g = |r: &mut Rng, lo: f64, hi: f64| (r.range(lo, hi) * 2.0).round() / 2.0;
    let (x0, y0) = (g(r, 0.0, 6.0), g(r, 0.0, 6.0));
    let (x1, y1) = (x0 + g(r, 1.5, 6.0), y0 + g(r, 1.5, 6.0));
    // A third with a hole: a rectangle well inside (a wall at least 0.5 thick).
    let hole = (r.below(3) == 0 && x1 - x0 >= 2.0 && y1 - y0 >= 2.0).then(|| {
        let (hx0, hy0) = (x0 + 0.5, y0 + 0.5);
        [
            hx0,
            hy0,
            (hx0 + g(r, 0.5, x1 - x0 - 1.0)).min(x1 - 0.5),
            (hy0 + g(r, 0.5, y1 - y0 - 1.0)).min(y1 - 0.5),
        ]
    });
    let axis = r.below(3);
    let mut o = [0.0; 3];
    o[axis] = g(r, 0.0, 6.0);
    Shape::Prism {
        axis,
        o,
        outer: [x0, y0, x1, y1],
        hole: hole.filter(|h| h[2] > h[0] && h[3] > h[1]),
        depth: g(r, 1.0, 6.0),
    }
}

/// The exact volume of the model a sequence of steps builds: over the cells of the grid its
/// faces make, each cell's centre classified through the steps.
fn sequence_volume(steps: &[Step]) -> f64 {
    let mut axes: [Vec<f64>; 3] = Default::default();
    for s in steps {
        for (i, c) in s.shape.planes().into_iter().enumerate() {
            axes[i].extend(c);
        }
    }
    for a in &mut axes {
        a.sort_by(f64::total_cmp);
        a.dedup();
    }
    let mut v = 0.0;
    for x in axes[0].windows(2) {
        for y in axes[1].windows(2) {
            for z in axes[2].windows(2) {
                let c = [
                    (x[0] + x[1]) / 2.0,
                    (y[0] + y[1]) / 2.0,
                    (z[0] + z[1]) / 2.0,
                ];
                let inside = steps.iter().fold(false, |acc, s| {
                    let p = s.shape.contains(c);
                    match s.op {
                        BoolOp::Union => acc || p,
                        BoolOp::Subtract => acc && !p,
                        BoolOp::Intersect => acc && p,
                    }
                });
                if inside {
                    v += (x[1] - x[0]) * (y[1] - y[0]) * (z[1] - z[0]);
                }
            }
        }
    }
    v
}

/// The failures of one extrude sequence: after each step the model is valid and has the exact
/// volume (an empty model ends the sequence).
fn check_sequence(steps: &[Step], done: &mut usize) -> Vec<String> {
    let mut k = NativeKernel::new();
    let mut bad = Vec::new();
    let mut model: Vec<BodyId> = Vec::new();
    for (i, s) in steps.iter().enumerate() {
        let tool = match build(&mut k, &Placed::fixed(s.shape)) {
            Ok(t) => t,
            Err(e) => return vec![format!("step {i}: build: {e:?}")],
        };
        model = if i == 0 {
            vec![tool]
        } else {
            // A model of several solids: each combined with the tool (a union joins them all).
            let r = match s.op {
                BoolOp::Union => {
                    let (first, rest) = model
                        .split_first()
                        .map(|(f, r)| (*f, r.to_vec()))
                        .unwrap_or((tool, vec![]));
                    let mut tools = rest;
                    tools.push(tool);
                    boolean(&mut k, BoolOp::Union, first, &tools)
                }
                op => model.iter().try_fold(Vec::new(), |mut acc, &m| {
                    acc.extend(boolean(&mut k, op, m, &[tool])?);
                    Ok(acc)
                }),
            };
            match r {
                Ok(r) => r,
                Err(e) => {
                    bad.push(format!("step {i} ({:?}): {e:?}", s.op));
                    break;
                }
            }
        };
        if let Some(why) = invalid(&k, &model) {
            bad.push(format!("step {i} ({:?}): invalid: {why}", s.op));
            break;
        }
        let want = sequence_volume(&steps[..=i]);
        let v = volume(&k, &model).unwrap_or(f64::NAN);
        // (Within the model's tolerance band: a result of tolerance t stands for any solid
        // within t of it.)
        if !((v - want).abs() <= 1e-9 * want.max(1.0) + band(&k, &model)) {
            bad.push(format!("step {i} ({:?}): volume {v}, exact {want}", s.op));
            break;
        }
        *done += 1;
        if model.is_empty() {
            break;
        }
    }
    bad
}

/// Random extrude sequences (3 to 8 steps of join, cut and intersect) checked against their
/// exact volumes after every step. `CADRS_META_CASES` scales the count as for `random_pairs`.
#[test]
fn extrude_sequences() {
    let s = seed(1);
    let n = env_num("CADRS_META_CASES", 16usize);
    let mut fails = Vec::new();
    let mut done = 0;
    for case in 0..n {
        let mut r = Rng::new(s ^ (case as u64).wrapping_mul(0xd1b5_4a32_d192_ed03));
        let len = 3 + r.below(6);
        let steps: Vec<Step> = (0..len)
            .map(|i| Step {
                op: match (i, r.below(20)) {
                    (0, _) => BoolOp::Union,
                    (_, 0..=9) => BoolOp::Union,
                    (_, 10..=17) => BoolOp::Subtract,
                    _ => BoolOp::Intersect,
                },
                shape: prism(&mut r),
            })
            .collect();
        let bad = check_sequence(&steps, &mut done);
        if !bad.is_empty() {
            fails.push(format!(
                "seed {s} sequence {case}: {steps:?}\n  {}",
                bad.join("\n  ")
            ));
        }
    }
    assert!(
        fails.is_empty(),
        "{} of {n} sequences fail:\n{}",
        fails.len(),
        fails.join("\n")
    ); // (Not vacuous: most sequences run several steps before an intersection can empty them.)
    println!("extrude_sequences: {done} steps checked");
    assert!(done >= 3 * n, "only {done} steps checked");
}

/// Runs the pair relations on one pinned pair, panicking with its failures.
fn pinned(a: Placed, b: Placed) {
    let bad = check_pair(&a, &b);
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Two cylinders crossing at skew angles (seed 12345 case 2): every boolean of the pair left
/// an open shell (the small cylinder's cap box, from samples of its rim, fell short of the rim
/// where the cylinders' curve leaves it, and the cap's own curve lost its end there).
#[test]
fn skew_cylinders() {
    pinned(
        Placed {
            shape: Shape::Cyl {
                axis: 2,
                base: [7.0, 7.0, -0.3707804499725267],
                r: 1.1004411956127174,
                len: 2.269831665071858,
            },
            axis: [-0.8516050882627577, -0.978177783068829, -0.3816516819404845],
            angle: 1.3877809304771738,
            about: [5.0; 3],
        },
        Placed {
            shape: Shape::Cyl {
                axis: 2,
                base: [7.0, 3.817329177849694, 3.5352404522767253],
                r: 3.753280953966347,
                len: 6.782899416459303,
            },
            axis: [
                0.19230537965956995,
                -0.9084555615490686,
                -0.3459386681894472,
            ],
            angle: -1.1134832725489603,
            about: [5.0; 3],
        },
    );
}

/// A tilted box and a tilted cylinder (seed 12345 case 75): every boolean left an open shell
/// (as [`skew_cylinders`]).
#[test]
fn tilted_box_and_cylinder() {
    pinned(
        Placed {
            shape: Shape::Box {
                min: [-1.7287351321660231, 2.0, 7.0],
                size: [6.524599352812511, 5.962324057922664, 2.308564273937577],
            },
            axis: [0.5530960115474062, 0.6896281828475401, 0.3117214767784895],
            angle: -2.9200964712114432,
            about: [5.0; 3],
        },
        Placed {
            shape: Shape::Cyl {
                axis: 2,
                base: [6.5860680206798445, 6.931195387961152, -1.418602229435946],
                r: 3.0,
                len: 4.0,
            },
            axis: [
                -0.6386842108454478,
                0.14934690994994448,
                0.21526908347248686,
            ],
            angle: -1.8272343669399191,
            about: [5.0; 3],
        },
    );
}

/// Two crossing cylinders on the axes (seed 12345 case 142): the union's STEP file read back
/// without one of the small cylinder's faces. An edge of the closed intersection curve ends where
/// the curve closes; the reader took that point as the curve's start, ran the edge the other,
/// long way round, and the face's loop then wrapped round the cylinder.
#[test]
fn step_round_trip_seamed_cylinder() {
    pinned(
        Placed::fixed(Shape::Cyl {
            axis: 1,
            base: [1.1684983475486566, 0.5293421570789636, 2.0824245633488534],
            r: 2.736774377748768,
            len: 7.323229369549773,
        }),
        Placed::fixed(Shape::Cyl {
            axis: 2,
            base: [0.0, 3.0790339291622146, -0.019167617471158582],
            r: 1.2504993337665509,
            len: 5.269151724617697,
        }),
    );
}

/// Every boolean of `x` and `y` (both ways round) that succeeds gives a valid body: one the
/// boolean can't get right is an error (strictly, as the helper takes it: a result handed back
/// flagged as having crossing faces is one), never an unflagged invalid result.
fn never_invalid(x: Placed, y: Placed) {
    let mut k = NativeKernel::new();
    let a = build(&mut k, &x).unwrap();
    let b = build(&mut k, &y).unwrap();
    let mut bad = Vec::new();
    for (p, q, order) in [(a, b, ""), (b, a, " (B first)")] {
        for op in [BoolOp::Union, BoolOp::Intersect, BoolOp::Subtract] {
            if let Ok(r) = boolean(&mut k, op, p, &[q])
                && let Some(why) = invalid(&k, &r)
            {
                bad.push(format!("{op:?}{order}: {why}"));
            }
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// [`cylinder_tangent_to_box_face`] and [`spheres_loops_cross_at_seam`]: until they are
/// computed right, flagged (the boolean finds the faces crossing, the loops crossing in
/// (u, v)), never invalid bodies handed back as sound.
#[test]
fn known_failures_are_never_invalid() {
    never_invalid(
        Placed::fixed(Shape::Box {
            min: [4.0, 4.923492988846983, 2.0],
            size: [4.610421114922351, 4.0648740291358285, 3.0],
        }),
        Placed::fixed(Shape::Cyl {
            axis: 0,
            base: [8.0, 6.9670406723616765, 4.0],
            r: 2.0,
            len: 5.0,
        }),
    );
    never_invalid(
        Placed {
            shape: Shape::Sphere {
                c: [7.0, 6.62966283372091, 0.9659503482469911],
                r: 4.0,
            },
            axis: [0.7765431131320071, 0.5361794883583875, 0.5889070314711073],
            angle: -0.3773634823933971,
            about: [5.0; 3],
        },
        Placed::fixed(Shape::Sphere {
            c: [7.0, 4.0, 9.0],
            r: 4.532638031669147,
        }),
    );
}

/// A cylinder along x and a tilted box (seed 12345 case 107): the union's STEP file read back
/// with two planar faces' conic edges 0.2 mm off their p-curves. The edges use part of a
/// rational arc whose p-curve covers the whole arc, and the reader's gap closing moved the
/// arc's end control point to the edge's end.
#[test]
fn step_round_trip_conic_edge() {
    pinned(
        Placed::fixed(Shape::Cyl {
            axis: 0,
            base: [6.760270809297007, 2.0, 2.0],
            r: 3.347707988538065,
            len: 3.87749651721185,
        }),
        Placed {
            shape: Shape::Box {
                min: [3.0, 1.0, 4.0],
                size: [5.0, 4.0, 4.0],
            },
            axis: [
                -0.8011568824086492,
                0.16992539633470694,
                -0.6291828619687938,
            ],
            angle: -0.09767065517102713,
            about: [5.0; 3],
        },
    );
}

/// Four boxes united in turn (an extrude sequence of the soak, minimised): the first two touch
/// along an edge only (x = 4.5, z = 5.5), the third bridges them, and the fourth's union left
/// an open shell: the model keeps the two boxes' corners there as two vertices (and the edge as
/// two), which the boolean pooled for the tool (by position) but not for the target.
#[test]
fn boxes_touching_along_an_edge() {
    let bx = |x: [f64; 2], y: [f64; 2], z: [f64; 2]| Step {
        op: BoolOp::Union,
        shape: Shape::Box {
            min: [x[0], y[0], z[0]],
            size: [x[1] - x[0], y[1] - y[0], z[1] - z[0]],
        },
    };
    let steps = [
        bx([2.5, 4.5], [3.0, 5.0], [5.5, 11.0]),
        bx([4.5, 8.0], [3.0, 5.5], [3.5, 5.5]),
        bx([4.5, 6.5], [4.5, 6.0], [3.5, 8.0]),
        bx([4.5, 9.5], [3.0, 7.0], [4.0, 6.0]),
    ];
    let mut bad = check_sequence(&steps, &mut 0);
    // And a cut (seed 955695009267410166 sequence 500), the same boxes as sketches extruded
    // along y and z (as boxes along z alone it passes): two touching along the edge x = 5.5,
    // y = 5, a third on both, then a block cut through them all.
    let prism = |op, axis, o, outer, depth| Step {
        op,
        shape: Shape::Prism {
            axis,
            o,
            outer,
            hole: None,
            depth,
        },
    };
    let steps = [
        prism(BoolOp::Union, 1, [0.0, 2.0, 0.0], [1.0, 3.5, 5.0, 5.5], 3.0),
        prism(BoolOp::Union, 1, [0.0, 5.0, 0.0], [2.5, 5.5, 5.0, 7.5], 2.5),
        prism(
            BoolOp::Union,
            2,
            [0.0, 0.0, 5.0],
            [2.0, 5.0, 6.5, 10.5],
            2.0,
        ),
        prism(
            BoolOp::Subtract,
            1,
            [0.0, 2.5, 0.0],
            [1.0, 1.0, 5.0, 6.0],
            5.0,
        ),
    ];
    bad.extend(check_sequence(&steps, &mut 0));
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

/// Two spheres, one turned (soak seed 16150658447098336548 case 2323): the union and the
/// difference came back invalid, a sphere face's intersection loop crossing its outer loop in
/// (u, v) at the seam near the pole: the seam's crossings of the other sphere, close together
/// there, fell between two samples of the edge–face search.
#[test]
fn spheres_loops_cross_at_seam() {
    pinned(
        Placed {
            shape: Shape::Sphere {
                c: [7.0, 6.62966283372091, 0.9659503482469911],
                r: 4.0,
            },
            axis: [0.7765431131320071, 0.5361794883583875, 0.5889070314711073],
            angle: -0.3773634823933971,
            about: [5.0; 3],
        },
        Placed::fixed(Shape::Sphere {
            c: [7.0, 4.0, 9.0],
            r: 4.532638031669147,
        }),
    );
}

// --- Soak findings (the build box's worker, research/SMLIB_COVERAGE.md item 1 of review 5), replayed
// at kernel-perf d0bcb72 and still failing, pinned by class: each test asserts every pair of its
// class keeps the metamorphic relations (`check_pair`), and is a known failure until they all do.

fn sequences_all(cases: &[Vec<Step>]) {
    let bad: Vec<String> = cases
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            let mut done = 0;
            let v = check_sequence(s, &mut done);
            (!v.is_empty()).then(|| format!("sequence {i}: {}", v.join("; ")))
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} sequences fail:\n{}",
        bad.len(),
        cases.len(),
        bad.join("\n")
    );
}

/// Soak: 3 extrude sequences where a step fails at the boolean's validity stage: an edge used by three or more faces.
#[test]
#[ignore = "known failure: a step fails at the boolean's validity stage: an edge used by three or more faces"]
fn soak_sequence_edge_three_faces() {
    sequences_all(&[
        vec![
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 0,
                    o: [0.5, 0.0, 0.0],
                    outer: [2.0, 2.0, 4.5, 7.0],
                    hole: Some([2.5, 2.5, 4.0, 6.0]),
                    depth: 6.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 1.0],
                    outer: [3.0, 0.5, 5.0, 3.5],
                    hole: None,
                    depth: 3.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 4.5],
                    outer: [5.0, 1.5, 9.5, 6.5],
                    hole: None,
                    depth: 1.5,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 0,
                    o: [0.0, 0.0, 0.0],
                    outer: [3.0, 1.5, 8.0, 4.0],
                    hole: Some([3.5, 2.0, 7.5, 3.0]),
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 2.5],
                    outer: [1.5, 2.5, 6.5, 7.0],
                    hole: None,
                    depth: 2.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 4.5, 0.0],
                    outer: [4.5, 3.0, 8.0, 7.0],
                    hole: None,
                    depth: 1.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 0.5],
                    outer: [5.5, 2.5, 7.5, 4.0],
                    hole: None,
                    depth: 4.5,
                },
            },
        ],
        vec![
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 2.5],
                    outer: [2.0, 2.5, 5.0, 5.5],
                    hole: Some([2.5, 3.0, 3.5, 4.5]),
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 2.0],
                    outer: [1.5, 5.5, 5.0, 8.0],
                    hole: None,
                    depth: 5.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [0.5, 0.0, 0.0],
                    outer: [4.5, 1.0, 10.5, 6.0],
                    hole: None,
                    depth: 3.5,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 0,
                    o: [2.0, 0.0, 0.0],
                    outer: [6.0, 2.5, 9.0, 4.5],
                    hole: None,
                    depth: 1.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [3.5, 0.0, 0.0],
                    outer: [1.0, 6.0, 2.5, 9.5],
                    hole: None,
                    depth: 3.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 5.0, 0.0],
                    outer: [3.5, 3.0, 6.5, 7.5],
                    hole: None,
                    depth: 4.5,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 5.5, 0.0],
                    outer: [2.0, 5.0, 7.0, 8.0],
                    hole: None,
                    depth: 3.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 5.5, 0.0],
                    outer: [0.5, 1.5, 5.5, 5.0],
                    hole: None,
                    depth: 1.5,
                },
            },
        ],
        vec![
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 3.0, 0.0],
                    outer: [4.0, 5.0, 6.0, 8.5],
                    hole: Some([4.5, 5.5, 5.5, 7.0]),
                    depth: 1.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [1.0, 0.0, 0.0],
                    outer: [5.5, 3.0, 11.0, 5.5],
                    hole: None,
                    depth: 3.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [0.5, 0.0, 0.0],
                    outer: [2.5, 5.0, 7.5, 8.5],
                    hole: None,
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 0.5, 0.0],
                    outer: [5.0, 4.0, 8.5, 6.5],
                    hole: None,
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 4.0, 0.0],
                    outer: [3.5, 5.5, 8.5, 9.5],
                    hole: None,
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 1.5, 0.0],
                    outer: [2.0, 4.0, 5.0, 6.5],
                    hole: Some([2.5, 4.5, 4.0, 5.5]),
                    depth: 2.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 2.5],
                    outer: [3.5, 4.0, 8.5, 6.0],
                    hole: None,
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 3.0],
                    outer: [4.0, 5.0, 7.0, 8.0],
                    hole: None,
                    depth: 6.0,
                },
            },
        ],
    ]);
}

/// Soak: 2 extrude sequences where a step leaves an open shell.
#[test]
#[ignore = "known failure: a step leaves an open shell"]
fn soak_sequence_open_shell() {
    sequences_all(&[
        vec![
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 1.0, 0.0],
                    outer: [0.0, 2.5, 4.5, 6.5],
                    hole: Some([0.5, 3.0, 3.0, 4.5]),
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 3.0],
                    outer: [4.5, 0.5, 8.0, 6.0],
                    hole: None,
                    depth: 3.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [4.5, 0.0, 0.0],
                    outer: [5.5, 0.5, 8.0, 3.0],
                    hole: None,
                    depth: 3.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 0.5],
                    outer: [3.0, 3.5, 8.5, 9.0],
                    hole: Some([3.5, 4.0, 4.5, 5.5]),
                    depth: 4.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 3.0],
                    outer: [3.5, 2.5, 7.0, 7.5],
                    hole: None,
                    depth: 4.5,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 2.5],
                    outer: [0.5, 2.5, 3.0, 4.5],
                    hole: Some([1.0, 3.0, 1.5, 3.5]),
                    depth: 3.5,
                },
            },
        ],
        vec![
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 4.0],
                    outer: [3.0, 1.0, 7.5, 4.5],
                    hole: None,
                    depth: 3.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 2.0, 0.0],
                    outer: [5.5, 4.5, 8.5, 10.5],
                    hole: Some([6.0, 5.0, 6.5, 5.5]),
                    depth: 5.0,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 3.5, 0.0],
                    outer: [5.0, 2.0, 7.0, 5.0],
                    hole: Some([5.5, 2.5, 6.5, 4.0]),
                    depth: 3.5,
                },
            },
            Step {
                op: BoolOp::Union,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 1.5],
                    outer: [2.0, 6.0, 4.0, 11.0],
                    hole: None,
                    depth: 2.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [4.0, 0.0, 0.0],
                    outer: [5.0, 2.0, 9.5, 5.5],
                    hole: None,
                    depth: 2.5,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 2,
                    o: [0.0, 0.0, 5.5],
                    outer: [5.0, 4.0, 8.0, 9.0],
                    hole: None,
                    depth: 6.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 1,
                    o: [0.0, 2.5, 0.0],
                    outer: [2.5, 2.0, 6.5, 6.5],
                    hole: Some([3.0, 2.5, 5.5, 6.0]),
                    depth: 5.0,
                },
            },
            Step {
                op: BoolOp::Subtract,
                shape: Shape::Prism {
                    axis: 0,
                    o: [5.5, 0.0, 0.0],
                    outer: [2.5, 4.0, 8.5, 6.5],
                    hole: Some([3.0, 4.5, 4.0, 5.0]),
                    depth: 4.0,
                },
            },
        ],
    ]);
}

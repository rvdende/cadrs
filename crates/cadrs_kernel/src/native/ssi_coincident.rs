//! Coincident surfaces: where two surfaces are one over a region, their intersection is that
//! region's boundary (SMLib's `SM_TC_REGION_BOUNDARY` curves): the sides of each surface's
//! parameter box that lie on the other surface inside its box.

use std::sync::Arc;

use super::curve::Curve;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::ssi::IntCurve;
use super::surface::Surface;
use super::tol;

type UvBox = ((f64, f64), (f64, f64));

/// Whether (u, v) lies in the box (angles modulo a turn), within a relative `e`.
fn in_box(s: &Surface, bx: UvBox, (u, v): (f64, f64), e: f64) -> bool {
    let ok = |x: f64, (lo, hi): (f64, f64), periodic: bool| {
        let m = (hi - lo) * e + tol::REL_FINE * (1.0 + lo.abs().max(hi.abs()));
        if periodic {
            hi - lo >= TAU - tol::ANGULAR || wrap(x, lo - m, TAU) <= hi + m
        } else {
            x >= lo - m && x <= hi + m
        }
    };
    ok(u, bx.0, s.u_periodic()) && ok(v, bx.1, s.v_periodic())
}

/// A box sure to hold `s` over every (u, v) [`in_box`] takes for `bx` with `e`: the box grown by
/// its margin (a closed direction's whole turn when the box spans one).
fn in_box_bbox(s: &Surface, bx: UvBox, e: f64) -> Bbox {
    let grown = |(lo, hi): (f64, f64), periodic: bool| {
        if periodic && hi - lo >= TAU - tol::ANGULAR {
            return (lo, lo + TAU);
        }
        let m = (hi - lo) * e + tol::REL_FINE * (1.0 + lo.abs().max(hi.abs()));
        (lo - m, hi + m)
    };
    s.patch_bbox(grown(bx.0, s.u_periodic()), grown(bx.1, s.v_periodic()))
}

/// Whether `p` lies farther than `tol` outside box `b` (to rounding): no point in the box is
/// within `tol` of it.
fn farther_than(b: &Bbox, p: &P3, tol: f64) -> bool {
    if b.is_empty() {
        return false;
    }
    let out = (b.min - p).sup(&(p - b.max)).sup(&V3::zeros());
    out.norm() > tol * (1.0 + tol::REL) + tol::ROUNDOFF * (1.0 + p.coords.abs().max())
}

/// Whether all of `s` over `bx` lies on `o` (within `tol`): a 9 × 9 grid of its points, each
/// projected from a coarse start.
pub fn lies_on(s: &Surface, bx: UvBox, o: &Surface, tol: f64) -> bool {
    let ((u0, u1), (v0, v1)) = bx;
    if ![u0, u1, v0, v1].iter().all(|x| x.is_finite())
        || (u1 - u0).abs() > 1e3
        || (v1 - v0).abs() > 1e3
    {
        return false;
    }
    let mut hint = None;
    (0..9).all(|i| {
        (0..9).all(|j| {
            let p = s.eval(
                u0 + (u1 - u0) * i as f64 / 8.0,
                v0 + (v1 - v0) * j as f64 / 8.0,
            );
            let near = |uv: (f64, f64)| (o.eval(uv.0, uv.1) - p).norm() <= tol;
            let uv = o.project(&p, hint);
            let ok = near(uv) || {
                let c = o.project_coarse(&p, 16);
                near(o.project(&p, Some(c)))
            };
            hint = Some(uv);
            ok
        })
    })
}

/// The boundary of the region where `a` over `ua` and `b` over `ub` coincide, within `tol`.
pub fn region_boundary(a: &Surface, ua: UvBox, b: &Surface, ub: UvBox, tol: f64) -> Vec<IntCurve> {
    let mut out: Vec<IntCurve> = Vec::new();
    for (s, bx, o, obx) in [(a, ua, b, ub), (b, ub, a, ua)] {
        for c in sides_on(s, bx, o, obx, tol) {
            // A side found from both surfaces (shared edges of the boxes), or the two sides of
            // a full turn's seam: once.
            let dup = out.iter().any(|d| {
                [0.2, 0.5, 0.8].iter().all(|f| {
                    let p = c.c.point(c.t0 + (c.t1 - c.t0) * f);
                    (d.c.point(d.c.closest(&p, d.t0, d.t1)) - p).norm() < 10.0 * tol
                })
            });
            if std::env::var_os("CADRS_SSI_DEBUG").is_some() {
                eprintln!(
                    "coincident side: {:?} .. {:?} length {:.4}{}",
                    c.c.point(c.t0),
                    c.c.point(c.t1),
                    c.c.length(c.t0, c.t1),
                    if dup { " (again)" } else { "" }
                );
            }
            if !dup {
                out.push(c);
            }
        }
    }
    out
}

/// The parts of the sides of `s`'s box `bx` that lie on `o` inside its box `obx`.
fn sides_on(s: &Surface, bx: UvBox, o: &Surface, obx: UvBox, tol: f64) -> Vec<IntCurve> {
    let ((u0, u1), (v0, v1)) = bx;
    let mut out = Vec::new();
    for (fix_u, val, range) in [
        (true, u0, (v0, v1)),
        (true, u1, (v0, v1)),
        (false, v0, (u0, u1)),
        (false, v1, (u0, u1)),
    ] {
        out.extend(
            side_runs(s, fix_u, val, range, o, obx, tol)
                .into_iter()
                .map(|(c, _)| c),
        );
    }
    out
}

/// The distance from `p` to `o` and the foot's (u, v), projected from `hint`. A foot not square
/// to the surface (held on the edge of its domain, or short of a fold) is checked against one
/// projected from a coarse grid, the nearer kept: a hinted projection alone would leave a run of
/// points on `o` early.
pub fn foot(o: &Surface, p: &P3, hint: Option<(f64, f64)>) -> (f64, (f64, f64)) {
    let uv = o.project(p, hint);
    let (q, su, sv) = o.d1(uv.0, uv.1);
    let r = p - q;
    let d = r.norm();
    let tang =
        (r.dot(&su) / su.norm().max(1e-300)).abs() + (r.dot(&sv) / sv.norm().max(1e-300)).abs();
    if tang <= 1e-7 * (1.0 + d) || o.signed_distance(p).is_some() {
        return (d, uv);
    }
    let c = o.project(p, Some(o.project_coarse(p, 12)));
    let dc = (o.eval(c.0, c.1) - p).norm();
    if dc < d { (dc, c) } else { (d, uv) }
}

/// The runs of the side u = `val` (v = `val` when not `fix_u`) of `s` over `range` that lie on
/// `o` inside its box `obx` (within `tol`), each as a curve with its parameter range on the side.
fn side_runs(
    s: &Surface,
    fix_u: bool,
    val: f64,
    (t0, t1): (f64, f64),
    o: &Surface,
    obx: UvBox,
    tol: f64,
) -> Vec<(IntCurve, (f64, f64))> {
    let mut out = Vec::new();
    if !(t1 > t0) || !val.is_finite() {
        return out;
    }
    let at = |t: f64| {
        if fix_u {
            s.eval(val, t)
        } else {
            s.eval(t, val)
        }
    };
    let on = |t: f64, hint: Option<(f64, f64)>| -> (bool, (f64, f64)) {
        let (d, uv) = foot(o, &at(t), hint);
        (d <= tol && in_box(o, obx, uv, tol::ROUNDOFF), uv)
    };
    const N: usize = 64;
    let mut hint = None;
    let flags: Vec<(f64, bool, (f64, f64))> = (0..=N)
        .map(|i| {
            let t = t0 + (t1 - t0) * i as f64 / N as f64;
            let (f, uv) = on(t, hint);
            hint = Some(uv);
            (t, f, uv)
        })
        .collect();
    // The runs of samples on `o`, their ends found by bisection (each projection from the
    // last one on `o`).
    let edge = |inside: f64, outside: f64, mut hint: (f64, f64)| {
        let (mut a, mut b) = (inside, outside);
        for _ in 0..50 {
            let m = 0.5 * (a + b);
            let (f, uv) = on(m, Some(hint));
            if f {
                a = m;
                hint = uv;
            } else {
                b = m;
            }
        }
        a
    };
    let mut k = 0;
    while k <= N {
        if !flags[k].1 {
            k += 1;
            continue;
        }
        let mut e = k;
        while e < N && flags[e + 1].1 {
            e += 1;
        }
        let ra = if k == 0 {
            flags[0].0
        } else {
            edge(flags[k].0, flags[k - 1].0, flags[k].2)
        };
        let rb = if e == N {
            flags[N].0
        } else {
            edge(flags[e].0, flags[e + 1].0, flags[e].2)
        };
        k = e + 1;
        if (at(rb) - at(ra)).norm() <= 10.0 * tol
            && (at(0.5 * (ra + rb)) - at(ra)).norm() <= 10.0 * tol
        {
            continue; // a point (a pole), or nothing
        }
        let iso = match s {
            Surface::Nurbs(n) => Some(Curve::Nurbs(Arc::new(nurbs_iso(n, fix_u, val)))),
            _ if fix_u => s.iso_u(val),
            _ => s.iso_v(val),
        };
        let c = match iso {
            Some(c) => IntCurve {
                c,
                t0: ra,
                t1: rb,
                closed: (at(ra) - at(rb)).norm() <= tol,
                pcs: None,
            },
            None => {
                // A side with no iso curve of its own (a surface of revolution's): interpolated
                // through its points (exact on `s`).
                let pts: Vec<P3> = (0..=128)
                    .map(|i| at(ra + (rb - ra) * i as f64 / 128.0))
                    .collect();
                let ts: Vec<f64> = (0..=128)
                    .map(|i| ra + (rb - ra) * i as f64 / 128.0)
                    .collect();
                let n = NurbsCurve::interpolate(&pts, &ts, 3, None, None);
                let r = n.range();
                IntCurve {
                    c: Curve::Nurbs(Arc::new(n)),
                    t0: r.0,
                    t1: r.1,
                    closed: false,
                    pcs: None,
                }
            }
        };
        out.push((c, (ra, rb)));
    }
    out
}

/// A side of a parameter box lying on the other surface: the surface it is a side of (a or b),
/// which coordinate is fixed and at what value, the run's range of the other, and the curve.
pub struct Side {
    pub on_a: bool,
    pub fix_u: bool,
    pub val: f64,
    pub range: (f64, f64),
    pub curve: IntCurve,
}

/// The runs of the sides of either box lying on the other surface inside its box (within
/// `tol`) where the surfaces cross there (a face whose edge lies on the other surface, the two
/// at an angle: a blend tool's end on a wall, a wall through a tube): the intersection runs
/// exactly along them, where a march only stays inside the box by rounding. (Sides of a whole
/// turn are its seam, not an end.) As the march's curves, a's sides run on over all of b's
/// surface, b's only inside a's box. Each side is first sampled at five points, so one wholly
/// off costs five projections.
pub fn crossing_sides(
    a: &Surface,
    ua: UvBox,
    b: &Surface,
    ub: UvBox,
    tol: f64,
) -> (Vec<Side>, u64) {
    let mut out: Vec<Side> = Vec::new();
    let mut work = 0u64;
    let ((pu0, pu1), (pv0, pv1)) = b.param_box();
    let all_b = ((pu0.max(-1e4), pu1.min(1e4)), (pv0.max(-1e4), pv1.min(1e4)));
    for (on_a, s, bx, o, obx) in [(true, a, ua, b, all_b), (false, b, ub, a, ua)] {
        let ((u0, u1), (v0, v1)) = bx;
        // (A side's point farther than `tol` from all of `o` over its box has no foot there
        // within `tol`: no projection asked. The box once, when a side is sampled.)
        let mut near: Option<Bbox> = None;
        let whole =
            |periodic: bool, (lo, hi): (f64, f64)| periodic && hi - lo >= TAU - tol::ANGULAR;
        for (fix_u, val, range) in [
            (true, u0, (v0, v1)),
            (true, u1, (v0, v1)),
            (false, v0, (u0, u1)),
            (false, v1, (u0, u1)),
        ] {
            if (fix_u && whole(s.u_periodic(), (u0, u1)))
                || (!fix_u && whole(s.v_periodic(), (v0, v1)))
            {
                continue;
            }
            let at = |t: f64| {
                if fix_u {
                    s.eval(val, t)
                } else {
                    s.eval(t, val)
                }
            };
            work += 5;
            let any = (0..5).any(|i| {
                let p = at(range.0 + (range.1 - range.0) * (0.1 + 0.2 * i as f64));
                let near = near.get_or_insert_with(|| in_box_bbox(o, obx, tol::ROUNDOFF));
                if farther_than(near, &p, tol) {
                    return false;
                }
                let (d, uv) = foot(o, &p, None);
                d <= tol && in_box(o, obx, uv, tol::ROUNDOFF)
            });
            if !any {
                continue;
            }
            work += 2 * 64;
            for (c, r) in side_runs(s, fix_u, val, range, o, obx, tol) {
                // Crossing: the normals apart at the run's middle.
                let p = at(0.5 * (r.0 + r.1));
                let (su, sv) = if fix_u {
                    (val, 0.5 * (r.0 + r.1))
                } else {
                    (0.5 * (r.0 + r.1), val)
                };
                let (_, ouv) = foot(o, &p, None);
                if s.normal(su, sv).cross(&o.normal(ouv.0, ouv.1)).norm() <= 1e-3 {
                    continue;
                }
                // (A side shared by both boxes, found from each: once.)
                let dup = out.iter().any(|d| {
                    [0.2, 0.5, 0.8].iter().all(|f| {
                        let q = c.c.point(c.t0 + (c.t1 - c.t0) * f);
                        (d.curve
                            .c
                            .point(d.curve.c.closest(&q, d.curve.t0, d.curve.t1))
                            - q)
                            .norm()
                            < 10.0 * tol
                    })
                });
                if !dup {
                    out.push(Side {
                        on_a,
                        fix_u,
                        val,
                        range: r,
                        curve: c,
                    });
                }
            }
        }
    }
    (out, work)
}

/// The boundary of the region where `a` over `ua` lies on `b` inside `ub` (within `tol`) that
/// runs inside a's box: where the surfaces part (the sides of the boxes that lie on the other
/// surface are [`region_boundary`]'s). A grid of a's parameters is marked on or off `b`, and
/// each grid edge between a point on and one off is cut where the two part (by bisection);
/// the cuts of each grid cell pair up (marching squares, a saddle settled by the cell's middle)
/// and chain into curves, fitted through points of `a` refined where the fit strays from it. A
/// curve ending inside a's box, by where `b` ends or leaves its box, runs on to the nearest of
/// `sides` (the region's other edges) when that is within two grid cells. With the work it took
/// (points projected onto `b`).
pub fn interior_boundary(
    a: &Surface,
    ua: UvBox,
    b: &Surface,
    ub: UvBox,
    tol: f64,
    sides: &[IntCurve],
) -> (Vec<IntCurve>, u64) {
    use rayon::prelude::*;
    use std::collections::HashMap;
    const N: usize = 48;
    let ((u0, u1), (v0, v1)) = ua;
    if ![u0, u1, v0, v1].iter().all(|x| x.is_finite()) || !(u1 > u0) || !(v1 > v0) {
        return (Vec::new(), 0);
    }
    let uv_at = |i: usize, j: usize| {
        (
            u0 + (u1 - u0) * i as f64 / N as f64,
            v0 + (v1 - v0) * j as f64 / N as f64,
        )
    };
    // 0: off b, 1: on b inside its box, 2: on b outside its box, or past where b ends (its
    // foot on the edge of b's domain: a side of b there, not where the two part).
    let ((pu0, pu1), (pv0, pv1)) = b.param_box();
    let rim = |(u, v): (f64, f64)| {
        let at = |x: f64, (lo, hi): (f64, f64), periodic: bool| {
            !periodic
                && ((x - lo).abs() <= tol::REL * (hi - lo).abs().max(1.0)
                    || (x - hi).abs() <= tol::REL * (hi - lo).abs().max(1.0))
        };
        at(u, (pu0, pu1), b.u_periodic()) || at(v, (pv0, pv1), b.v_periodic())
    };
    let state = |uv: (f64, f64), hint: Option<(f64, f64)>| -> (u8, (f64, f64)) {
        let (d, f) = foot(b, &a.eval(uv.0, uv.1), hint);
        (
            if d > tol && !rim(f) {
                0
            } else if d <= tol && in_box(b, ub, f, tol::POINT) {
                1
            } else {
                2
            },
            f,
        )
    };
    let nodes: Vec<Vec<(u8, (f64, f64))>> = (0..=N)
        .into_par_iter()
        .map(|i| {
            let mut hint = None;
            (0..=N)
                .map(|j| {
                    let s = state(uv_at(i, j), hint);
                    hint = Some(s.1);
                    s
                })
                .collect()
        })
        .collect();
    let mut work = ((N + 1) * (N + 1)) as u64;
    if !nodes.iter().flatten().any(|n| n.0 == 1) || nodes.iter().flatten().all(|n| n.0 != 0) {
        return (Vec::new(), work);
    }
    // A grid edge: horizontal (node (i, j) to (i + 1, j)) or vertical ((i, j) to (i, j + 1)).
    type Key = (bool, usize, usize);
    let ends = |k: Key| {
        if k.0 {
            ((k.1, k.2), (k.1 + 1, k.2))
        } else {
            ((k.1, k.2), (k.1, k.2 + 1))
        }
    };
    let crosses = |k: Key| {
        let ((i0, j0), (i1, j1)) = ends(k);
        let (s0, s1) = (nodes[i0][j0].0, nodes[i1][j1].0);
        (s0 == 1 && s1 == 0) || (s0 == 0 && s1 == 1)
    };
    // Where a's points along `from` (on) to `to` (off) part from b.
    let on = |uv: (f64, f64), hint: (f64, f64)| -> (bool, (f64, f64)) {
        let (d, f) = foot(b, &a.eval(uv.0, uv.1), Some(hint));
        (d <= tol, f)
    };
    let part = |from: (f64, f64), to: (f64, f64), mut hint: (f64, f64)| -> (f64, f64) {
        let (mut lo, mut hi) = (0.0, 1.0);
        let at = |f: f64| (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f);
        for _ in 0..48 {
            let m = 0.5 * (lo + hi);
            let (f, h) = on(at(m), hint);
            if f {
                lo = m;
                hint = h;
            } else {
                hi = m;
            }
        }
        at(lo)
    };
    // The cells' segments, between cut edges.
    let mut segs: Vec<(Key, Key)> = Vec::new();
    #[allow(
        clippy::needless_range_loop,
        reason = "a cell's corners are nodes of both indices"
    )]
    for i in 0..N {
        for j in 0..N {
            let es: [Key; 4] = [
                (true, i, j),
                (false, i + 1, j),
                (true, i, j + 1),
                (false, i, j),
            ];
            let cut: Vec<usize> = (0..4).filter(|&e| crosses(es[e])).collect();
            match cut.len() {
                2 | 3 => segs.push((es[cut[0]], es[cut[1]])),
                4 => {
                    // A saddle: corners (i, j) and (i + 1, j + 1) alike, the other two alike.
                    // When the middle is like the first pair those are joined through it and the
                    // others cut off (edges 0–1 round (i + 1, j), 2–3 round (i, j + 1)).
                    work += 1;
                    let (m, _) = state(
                        (
                            (uv_at(i, j).0 + uv_at(i + 1, j).0) * 0.5,
                            (uv_at(i, j).1 + uv_at(i, j + 1).1) * 0.5,
                        ),
                        Some(nodes[i][j].1),
                    );
                    let joined = (m == 1) == (nodes[i][j].0 == 1);
                    if joined {
                        segs.push((es[0], es[1]));
                        segs.push((es[2], es[3]));
                    } else {
                        segs.push((es[3], es[0]));
                        segs.push((es[1], es[2]));
                    }
                }
                _ => {}
            }
        }
    }
    if segs.is_empty() {
        return (Vec::new(), work);
    }
    // Chains of segments (open ones from an end first).
    let mut by_key: HashMap<Key, Vec<usize>> = HashMap::new();
    for (k, s) in segs.iter().enumerate() {
        by_key.entry(s.0).or_default().push(k);
        by_key.entry(s.1).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut chains: Vec<(Vec<Key>, bool)> = Vec::new();
    let mut starts: Vec<Key> = segs
        .iter()
        .flat_map(|s| [s.0, s.1])
        .filter(|k| by_key[k].len() == 1)
        .collect();
    starts.extend(segs.iter().map(|s| s.0));
    for start in starts {
        let Some(&first) = by_key[&start].iter().find(|&&s| !used[s]) else {
            continue;
        };
        let mut chain = vec![start];
        let mut cur = start;
        let mut seg = first;
        let mut closed = false;
        loop {
            used[seg] = true;
            let next = if segs[seg].0 == cur {
                segs[seg].1
            } else {
                segs[seg].0
            };
            if next == start {
                closed = true;
                break;
            }
            chain.push(next);
            cur = next;
            match by_key[&cur].iter().find(|&&s| !used[s]) {
                Some(&s) => seg = s,
                None => break,
            }
        }
        chains.push((chain, closed));
    }
    // A chain running along a side of a's box, within its first row of cells, follows the
    // surfaces' noise where a's side lies on b only about as near as the tolerance (that side
    // on b is a side curve): those stretches go, and a chain ending by one ends on the side.
    // (Sides: 0 u = u0, 1 u = u1, 2 v = v0, 3 v = v1.)
    let ring = |k: Key| -> [bool; 4] {
        let (h, i, j) = k;
        if h {
            [i == 0, i + 1 == N, j <= 1, j + 1 >= N]
        } else {
            [i <= 1, i + 1 >= N, j == 0, j + 1 == N]
        }
    };
    let along = |x: Key, y: Key| (0..4).find(|&sd| ring(x)[sd] && ring(y)[sd]);
    let mut pieces: Vec<(Vec<Key>, bool, [Option<usize>; 2])> = Vec::new();
    for (chain, closed) in chains {
        let n = chain.len();
        let segs = if closed { n } else { n.saturating_sub(1) };
        let cut: Vec<Option<usize>> = (0..segs)
            .map(|k| along(chain[k], chain[(k + 1) % n]))
            .collect();
        if cut.iter().all(Option::is_none) {
            pieces.push((chain, closed, [None, None]));
            continue;
        }
        // Opened after a stretch along a side (a closed chain), then cut at each.
        let first = cut.iter().position(Option::is_some).unwrap_or(0);
        let order: Vec<usize> = if closed {
            (0..=n).map(|k| (first + 1 + k) % n).collect()
        } else {
            (0..n).collect()
        };
        let seg_after = |pos: usize| -> Option<usize> {
            let k = order[pos];
            if closed || k + 1 < n {
                cut[k % segs]
            } else {
                None
            }
        };
        let mut cur: Vec<Key> = Vec::new();
        let mut start: Option<usize> = if closed { cut[first] } else { None };
        for pos in 0..order.len() {
            cur.push(chain[order[pos]]);
            let last = pos + 1 == order.len();
            match if last { None } else { seg_after(pos) } {
                Some(sd) => {
                    if cur.len() >= 2 {
                        pieces.push((std::mem::take(&mut cur), false, [start, Some(sd)]));
                    }
                    cur.clear();
                    start = Some(sd);
                }
                None if last && cur.len() >= 2 => {
                    pieces.push((std::mem::take(&mut cur), false, [start, None]))
                }
                None => {}
            }
        }
    }
    // (An open chain's end one cell from a side, where the grid's last row is past b's end or
    // box, ends on the side.)
    for (chain, closed, snap) in pieces.iter_mut() {
        if *closed {
            continue;
        }
        for (end, k) in [(0, chain[0]), (1, chain[chain.len() - 1])] {
            let on_side = if k.0 {
                k.2 == 0 || k.2 == N
            } else {
                k.1 == 0 || k.1 == N
            };
            if snap[end].is_none() && !on_side {
                snap[end] = (0..4).find(|&sd| ring(k)[sd]);
            }
        }
    }
    // The cuts.
    let mut curves = Vec::new();
    for (chain, closed, snap) in pieces {
        let mut uvs: Vec<(f64, f64)> = chain
            .iter()
            .map(|&k| {
                let ((i0, j0), (i1, j1)) = ends(k);
                let (from, to, hint) = if nodes[i0][j0].0 == 1 {
                    ((i0, j0), (i1, j1), nodes[i0][j0].1)
                } else {
                    ((i1, j1), (i0, j0), nodes[i1][j1].1)
                };
                part(uv_at(from.0, from.1), uv_at(to.0, to.1), hint)
            })
            .collect();
        let last = uvs.len() - 1;
        for (end, sd) in [(0, snap[0]), (last, snap[1])] {
            match sd {
                Some(0) => uvs[end].0 = u0,
                Some(1) => uvs[end].0 = u1,
                Some(2) => uvs[end].1 = v0,
                Some(3) => uvs[end].1 = v1,
                _ => {}
            }
        }
        // An end inside the box (not on a side of a's box): on to the nearest side curve.
        if !closed {
            let on_edge = |k: Key| {
                if k.0 {
                    k.2 == 0 || k.2 == N
                } else {
                    k.1 == 0 || k.1 == N
                }
            };
            let reach = 2.0
                * (a.eval(u0, v0) - a.eval(u0 + (u1 - u0) / N as f64, v0 + (v1 - v0) / N as f64))
                    .norm()
                    .max(
                        (a.eval(u1, v1)
                            - a.eval(u1 - (u1 - u0) / N as f64, v1 - (v1 - v0) / N as f64))
                        .norm(),
                    );
            for (end, k) in [(0usize, chain[0]), (1, chain[chain.len() - 1])] {
                if snap[end].is_some() || on_edge(k) {
                    continue;
                }
                let at = if end == 0 { 0 } else { uvs.len() - 1 };
                let p = a.eval(uvs[at].0, uvs[at].1);
                let best = sides
                    .iter()
                    .map(|c| c.c.point(c.c.closest(&p, c.t0.min(c.t1), c.t0.max(c.t1))))
                    .min_by(|x, y| (x - p).norm().total_cmp(&(y - p).norm()));
                if let Some(q) = best.filter(|q| (q - p).norm() > tol && (q - p).norm() <= reach) {
                    let uv = a.project(&q, Some(uvs[at]));
                    if end == 0 {
                        uvs.insert(0, uv);
                    } else {
                        uvs.push(uv);
                    }
                }
            }
        }
        work += 48 * uvs.len() as u64;
        // Between two points of the boundary: the point where a parts from b across the chord
        // through its middle (within the chord's length either side), else the middle.
        let between = |p: (f64, f64), q: (f64, f64)| -> (f64, f64) {
            let m = (0.5 * (p.0 + q.0), 0.5 * (p.1 + q.1));
            let n = (-(q.1 - p.1), q.0 - p.0);
            let (x, y) = ((m.0 - n.0).clamp(u0, u1), (m.1 - n.1).clamp(v0, v1));
            let (x2, y2) = ((m.0 + n.0).clamp(u0, u1), (m.1 + n.1).clamp(v0, v1));
            let (fa, ha) = state((x, y), None);
            let (fb, hb) = state((x2, y2), None);
            match (fa, fb) {
                (1, 0) => part((x, y), (x2, y2), ha),
                (0, 1) => part((x2, y2), (x, y), hb),
                _ => m,
            }
        };
        let fit_tol = {
            let mut bb = Bbox::empty();
            uvs.iter().for_each(|q| bb.add(&a.eval(q.0, q.1)));
            1e-9f64.max(2e-10 * bb.diagonal())
        };
        let (c, w) = fit_on(a, uvs, closed, &between, fit_tol);
        work += w;
        curves.extend(c);
    }
    (curves, work)
}

/// A cubic B-spline through the points of `s` at `uvs` (a closed curve back to the first),
/// with a point from `between` inserted between two neighbours wherever the fit's middle there
/// is farther than `tol` from it. With the points inserted.
fn fit_on(
    s: &Surface,
    mut uvs: Vec<(f64, f64)>,
    closed: bool,
    between: &dyn Fn((f64, f64), (f64, f64)) -> (f64, f64),
    tol: f64,
) -> (Option<IntCurve>, u64) {
    let closed = closed && uvs.len() > 2;
    if closed {
        uvs.push(uvs[0]);
    }
    uvs.dedup_by(|x, y| (s.eval(x.0, x.1) - s.eval(y.0, y.1)).norm() < tol::DEGENERATE);
    if uvs.len() < 2 {
        return (None, 0);
    }
    let mut work = 0u64;
    let fit = |uvs: &[(f64, f64)]| {
        let pts: Vec<P3> = uvs.iter().map(|q| s.eval(q.0, q.1)).collect();
        let mut t = vec![0.0];
        for w in pts.windows(2) {
            t.push(t[t.len() - 1] + (w[1] - w[0]).norm());
        }
        (
            NurbsCurve::interpolate(&pts, &t, 3.min(pts.len() - 1), None, None),
            t,
        )
    };
    for _ in 0..8 {
        let (c, t) = fit(&uvs);
        let mut ins: Vec<(usize, (f64, f64))> = Vec::new();
        for k in 0..uvs.len() - 1 {
            let (p, q) = (uvs[k], uvs[k + 1]);
            let chord = (s.eval(p.0, p.1) - s.eval(q.0, q.1)).norm();
            if chord <= 1e-7 {
                continue;
            }
            let m = between(p, q);
            work += 50;
            let pm = s.eval(m.0, m.1);
            if (c.point(0.5 * (t[k] + t[k + 1])) - pm).norm() > tol {
                ins.push((k + 1, m));
            }
        }
        if ins.is_empty() || uvs.len() > 4000 {
            let r = c.range();
            return (
                Some(IntCurve {
                    c: Curve::Nurbs(Arc::new(c)),
                    t0: r.0,
                    t1: r.1,
                    closed,
                    pcs: None,
                }),
                work,
            );
        }
        for (k, m) in ins.into_iter().rev() {
            uvs.insert(k, m);
        }
    }
    let (c, _) = fit(&uvs);
    let r = c.range();
    (
        Some(IntCurve {
            c: Curve::Nurbs(Arc::new(c)),
            t0: r.0,
            t1: r.1,
            closed,
            pcs: None,
        }),
        work,
    )
}

/// The iso curve u = `val` (v = `val` when not `fix_u`) of a B-spline surface, exactly: the
/// control net's rows (columns) combined by the basis functions at `val`, in homogeneous
/// coordinates.
fn nurbs_iso(n: &super::nurbs::NurbsSurface, fix_u: bool, val: f64) -> NurbsCurve {
    use super::nurbs::{H, basis_ders, find_span};
    let (p, knots, count) = if fix_u {
        (n.du, &n.ku, n.cps.len())
    } else {
        (n.dv, &n.kv, n.cps[0].len())
    };
    let span = find_span(count - 1, p, val, knots);
    let basis = &basis_ders(span, val, p, 0, knots)[0];
    let cps: Vec<H> = if fix_u {
        (0..n.cps[0].len())
            .map(|j| (0..=p).map(|k| n.cps[span - p + k][j] * basis[k]).sum())
            .collect()
    } else {
        (0..n.cps.len())
            .map(|i| (0..=p).map(|k| n.cps[i][span - p + k] * basis[k]).sum())
            .collect()
    };
    let (degree, knots) = if fix_u {
        (n.dv, n.kv.clone())
    } else {
        (n.du, n.ku.clone())
    };
    NurbsCurve { degree, knots, cps }
}

//! What any solid passes through as it slides, turns or screws: the solid where it starts, with
//! what each of its *leading* faces sweeps.
//!
//! A point the moving solid covers at some instant is inside it where it starts, or enters it
//! later across the part of its boundary moving towards it: where the face's outward normal leads
//! the motion (an acute angle with the velocity). Split along its *contact* (where the normal is
//! square to the velocity), each piece of a face leads all over, trails all over, or runs along
//! itself (a flat face sliding in its plane, a drum's end turning in its own): a leading piece
//! never moves back over where it has been, so it sweeps a solid without folding
//! ([`super::sweep::sweep_body`]; a screw's sides skinned through sections, see
//! [`super::sweep::Screw`]). The solid swept is the start united with those solids.
//!
//! A face is split along the iso-lines its contact runs along (a torus's meridians and its top
//! and bottom circles slid across its axis), else along the plane the contact lies in (a plane
//! fitted to contact points found on the surface), else along contact curves traced over its
//! parameters (marching squares, the points pulled onto the contact by Newton steps); a contact
//! closed round the face's seam is split after the face is halved.
//!
//! Where no point enters the solid twice (a slide of a convex solid, most turns short of a whole
//! one), the pieces only touch: the faces two of them share are dropped and the rest sewn, which
//! no boolean need do. Where they overlap (a bar's outer corners turned past its end face), they
//! are united, at once or one at a time. A sweep that takes more than a few seconds is given up
//! for [`crate::SolidMotion::Rigid`]'s meshed one.

use super::curve::Curve;
use super::kernel::NativeKernel;
use super::math::*;
use super::topo::Body;
use crate::*;

/// The motion from where the solid starts.
#[derive(Clone, Copy)]
enum Move {
    Slide(V3),
    Turn {
        o: P3,
        z: V3,
        angle: f64,
    },
    /// Turned by `angle` about the axis through `o` along the unit `z` while rising `rise`
    /// along it, uniformly.
    Screw {
        o: P3,
        z: V3,
        angle: f64,
        rise: f64,
    },
}

impl Move {
    /// The velocity of a point (unit speed per radian for a turn), its sign the motion's.
    fn velocity(&self, p: &P3) -> V3 {
        match *self {
            Move::Slide(v) => v,
            Move::Turn { o, z, angle } => z.cross(&(p - o)) * angle.signum(),
            Move::Screw { o, z, angle, rise } => z.cross(&(p - o)) * angle + z * rise,
        }
    }
}

/// The rotation by `angle` about the axis through `o` along the unit `z`.
fn turned(o: P3, z: V3, angle: f64) -> Affine {
    let r = nalgebra::Rotation3::from_axis_angle(&nalgebra::Unit::new_normalize(z), angle);
    let m = *r.matrix();
    Affine {
        m,
        t: o.coords - m * o.coords,
    }
}

/// How a face stands to the motion.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Stand {
    Leads,
    Trails,
    Along,
    Both,
}

/// How far a screw's rails and side surfaces may stray from the true sweep, per unit of the body's
/// size.
const SCREW_TOL: f64 = 1e-7;

/// A screw's piece with its faces cut so none wraps more than a quarter turn about the axis
/// (the faces a screw of several turns sweeps wind round it, which a boolean then traces whole):
/// its turned faces across their angle, its skinned sides (v the screw's turn, from 0 to `s1`)
/// at each quarter turn, both alike in every piece so faces two pieces share still match.
fn quarter_turns(b: &mut Body, s1: f64) {
    use super::heal_split::{Dir, face_uv_box, split_angle, split_face_iso};
    let quarter = std::f64::consts::FRAC_PI_2;
    if s1 <= quarter {
        return;
    }
    let clock = web_time::Instant::now();
    let lap = |what: &str| {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "  quarter turns: {what} by {:.0} ms",
                clock.elapsed().as_secs_f64() * 1e3
            );
        }
    };
    let _ = split_angle(b, quarter);
    lap("turned faces split");
    let cuts: Vec<f64> = (1..)
        .map(|k| k as f64 * quarter)
        .take_while(|&v| v < s1 - 1e-9)
        .collect();
    let n = b.faces.len();
    for fi in 0..n {
        if !matches!(b.faces[fi].surf, super::surface::Surface::Nurbs(_)) {
            continue;
        }
        let Some((_, (v0, v1))) = face_uv_box(b, fi) else {
            continue;
        };
        let inside: Vec<f64> = cuts
            .iter()
            .copied()
            .filter(|&v| v > v0 + 1e-9 && v < v1 - 1e-9)
            .collect();
        if !inside.is_empty() {
            let _ = split_face_iso(b, fi, Dir::V, &inside);
        }
    }
    lap("skinned faces split");
    // Each piece on its own part of the surface: the faces cut from one share it, so each would
    // carry the whole winding surface (into every boolean, mass and file written). (The parts
    // cut out side by side: each is a knot insertion over the whole surface.)
    let parts: Vec<Option<super::nurbs::NurbsSurface>> = {
        use rayon::prelude::*;
        let boxes: Vec<_> = (0..b.faces.len()).map(|fi| face_uv_box(b, fi)).collect();
        b.faces
            .par_iter()
            .zip(boxes)
            .map(|(f, uv)| {
                let super::surface::Surface::Nurbs(n) = &f.surf else {
                    return None;
                };
                let ((u0, u1), (v0, v1)) = uv?;
                let ((a0, a1), (c0, c1)) = n.range();
                let pad = |lo: f64, hi: f64, a: f64, z: f64| {
                    let m = 1e-3 * (z - a);
                    ((lo - m).max(a), (hi + m).min(z))
                };
                let ((u0, u1), (v0, v1)) = (pad(u0, u1, a0, a1), pad(v0, v1, c0, c1));
                (u1 - u0 < 0.9 * (a1 - a0) || v1 - v0 < 0.9 * (c1 - c0))
                    .then(|| n.restricted((u0, u1), (v0, v1)))
            })
            .collect()
    };
    for (f, part) in b.faces.iter_mut().zip(parts) {
        if let Some(part) = part {
            f.surf = super::surface::Surface::Nurbs(std::sync::Arc::new(part));
        }
    }
    lap("each on its own part");
}

/// How long (seconds) a sweep by faces may split and unite before it gives up.
const EFFORT_SECONDS: f64 = 4.0;

/// [`EFFORT_SECONDS`], or `CADRS_SWEEP_EFFORT` seconds (for debugging).
fn effort() -> f64 {
    std::env::var("CADRS_SWEEP_EFFORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(EFFORT_SECONDS)
}

/// A screw's turn (radians, above 0).
fn mv_angle(mv: &Move) -> f64 {
    match *mv {
        Move::Screw { angle, .. } | Move::Turn { angle, .. } => angle.abs(),
        Move::Slide(v) => v.norm(),
    }
}

/// The cosine below which a face's normal counts as square to the motion (a face split along a
/// traced contact curve meets it within about this along its new edge).
const SQUARE: f64 = 1e-7;

/// Points of face `fi` with their outward normals: a 24 × 24 grid over its parameters' box, the
/// points inside it, and its outline's own points (no mesh: a triangulation of a sliver of a
/// face can stall), each with its surface's normal there, those where it degenerates (a pole)
/// left out.
pub(crate) fn face_points(b: &Body, fi: usize, _size: f64) -> Vec<(P3, V3)> {
    let f = &b.faces[fi];
    let polys = super::classify::face_polygons(b, f);
    let sign = if f.sense { 1.0 } else { -1.0 };
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for p in polys.iter().flatten() {
        (u0, u1, v0, v1) = (u0.min(p.x), u1.max(p.x), v0.min(p.y), v1.max(p.y));
    }
    if !(u1 >= u0 && v1 >= v0) {
        return Vec::new();
    }
    let n = 24;
    let tol = 1e-9 * ((u1 - u0) + (v1 - v0)).max(1e-12);
    // (Points within a fiftieth of the box of the outline are left out: the outline is a
    // chord's way off the face's own edge, and beside a contact a point across it reads the
    // other way.)
    let (su, sv) = ((u1 - u0).max(1e-300), (v1 - v0).max(1e-300));
    let near_outline = |q: P2| {
        polys.iter().any(|poly| {
            (0..poly.len()).any(|k| {
                let (a, b2) = (poly[k], poly[(k + 1) % poly.len()]);
                let (ax, ay, bx, by, qx, qy) =
                    (a.x / su, a.y / sv, b2.x / su, b2.y / sv, q.x / su, q.y / sv);
                let (dx, dy) = (bx - ax, by - ay);
                let l2 = dx * dx + dy * dy;
                let t = if l2 > 0.0 {
                    (((qx - ax) * dx + (qy - ay) * dy) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (ax + dx * t - qx).hypot(ay + dy * t - qy) < 0.02
            })
        })
    };
    let mut uvs: Vec<P2> = Vec::new();
    for i in 0..=n {
        for j in 0..=n {
            let q = P2::new(
                u0 + (u1 - u0) * (i as f64 + 0.5) / (n + 1) as f64,
                v0 + (v1 - v0) * (j as f64 + 0.5) / (n + 1) as f64,
            );
            if super::classify::in_face(&polys, &f.surf, q, tol) == super::classify::InFace::Inside
                && !near_outline(q)
            {
                uvs.push(q);
            }
        }
    }
    // A sliver with no point clear of its outline: the outline's own points.
    if uvs.is_empty() {
        uvs = polys.iter().flatten().copied().collect();
    }
    uvs.into_iter()
        .filter_map(|q| {
            let (p, su, sv) = f.surf.d1(q.x, q.y);
            let n = su.cross(&sv);
            (n.norm() > 1e-9 * su.norm() * sv.norm() && n.norm() > 1e-300)
                .then(|| (p, n.normalize() * sign))
        })
        .collect()
}

fn stand(points: &[(P3, V3)], mv: &Move) -> Stand {
    // The most the face leads and trails by (cosines).
    let (mut lead, mut trail) = (0.0f64, 0.0f64);
    for (p, n) in points {
        let v = mv.velocity(p);
        let s = v.norm() * n.norm();
        if s <= tol::TINY {
            continue;
        }
        let c = n.dot(&v) / s;
        lead = lead.max(c);
        trail = trail.max(-c);
    }
    // Both ways only past [`FOLD`] each (a face split along a fitted contact strays across it
    // by less beside its new edge, as a body sweep's fold test allows); else the way it mostly
    // goes, past [`SQUARE`].
    if lead > FOLD && trail > FOLD {
        Stand::Both
    } else if lead > SQUARE && lead >= trail {
        Stand::Leads
    } else if trail > SQUARE {
        Stand::Trails
    } else {
        Stand::Along
    }
}

/// The cosine past which a face counts as leading and trailing both.
const FOLD: f64 = 1e-6;

/// The plane face `fi`'s contact lies in, from contact points found on its surface (sign changes
/// of the normal's component along the velocity across a grid of the face's parameters, each
/// refined by bisection): None when they don't all lie in one.
fn contact_plane(b: &Body, fi: usize, mv: &Move, size: f64) -> Option<Plane> {
    let f = &b.faces[fi];
    let s = &f.surf;
    let sign = if f.sense { 1.0 } else { -1.0 };
    let g = |u: f64, v: f64| {
        let p = s.eval(u, v);
        let n = s.normal(u, v) * sign;
        let w = mv.velocity(&p);
        let l = w.norm().max(tol::TINY);
        n.dot(&w) / l
    };
    // The face's parameter box, from its p-curves.
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in f.loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for k in 0..=16 {
            let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 16.0);
            (u0, u1, v0, v1) = (u0.min(q.x), u1.max(q.x), v0.min(q.y), v1.max(q.y));
        }
    }
    if !(u1 > u0 && v1 > v0) {
        return None;
    }
    let n = 24;
    let at = |i: usize, j: usize| {
        (
            u0 + (u1 - u0) * i as f64 / n as f64,
            v0 + (v1 - v0) * j as f64 / n as f64,
        )
    };
    let mut pts: Vec<P3> = Vec::new();
    let zero = 1e-12;
    let mut refine = |a: (f64, f64), b2: (f64, f64)| {
        let (ga, gb) = (g(a.0, a.1), g(b2.0, b2.1));
        // (A grid point on the contact is one of its points, and its neighbours either side
        // change sign across it only through it.)
        if ga.abs() <= zero {
            if gb.abs() > zero {
                pts.push(s.eval(a.0, a.1));
            }
            return;
        }
        if gb.abs() <= zero || ga * gb > 0.0 {
            return;
        }
        let (mut lo, mut hi, mut glo) = (0.0f64, 1.0f64, ga);
        for _ in 0..60 {
            let m = 0.5 * (lo + hi);
            let gm = g(a.0 + (b2.0 - a.0) * m, a.1 + (b2.1 - a.1) * m);
            if (gm < 0.0) == (glo < 0.0) {
                lo = m;
                glo = gm;
            } else {
                hi = m;
            }
        }
        let m = 0.5 * (lo + hi);
        pts.push(s.eval(a.0 + (b2.0 - a.0) * m, a.1 + (b2.1 - a.1) * m));
    };
    for i in 0..=n {
        for j in 0..=n {
            if i < n {
                refine(at(i, j), at(i + 1, j));
            }
            if j < n {
                refine(at(i, j), at(i, j + 1));
            }
        }
    }
    if pts.len() < 2 {
        return None;
    }
    let close = 1e-7 * size;
    // The plane through `on` (its points' fit: a line's, square to the face there): Some when
    // they all lie on it.
    let fit = |on: &[P3]| -> Option<(P3, V3)> {
        let c = P3::from(on.iter().map(|p| p.coords).sum::<V3>() / on.len() as f64);
        let mut cov = nalgebra::Matrix3::zeros();
        for p in on {
            let d = p - c;
            cov += d * d.transpose();
        }
        let e = cov.symmetric_eigen();
        let mut order = [0usize, 1, 2];
        order.sort_by(|&x, &y| e.eigenvalues[x].total_cmp(&e.eigenvalues[y]));
        let line = e.eigenvalues[order[1]] <= close * close * on.len() as f64;
        let normal = if line {
            let d: V3 = e.eigenvectors.column(order[2]).into_owned();
            let (u, v) = s.project(&c, None);
            d.cross(&s.normal(u, v)).try_normalize(tol::TINY)?
        } else {
            e.eigenvectors.column(order[0]).into_owned()
        };
        on.iter()
            .all(|p| (p - c).dot(&normal).abs() <= close)
            .then_some((c, normal))
    };
    // All on one plane; else the plane through three of them that holds the most (a rod turned
    // about a line crossing it meets the turn along two planes: one split, then the other).
    let (c, normal) = fit(&pts).or_else(|| {
        let m = pts.len();
        let step = (m / 24).max(1);
        let mut best: Option<(usize, Vec<P3>)> = None;
        for i in (0..m).step_by(step) {
            for j in (i + 1..m).step_by(step) {
                for l in (j + 1..m).step_by(step) {
                    let nrm = (pts[j] - pts[i]).cross(&(pts[l] - pts[i]));
                    let Some(nrm) = nrm.try_normalize(tol::TINY) else {
                        continue;
                    };
                    let on: Vec<P3> = pts
                        .iter()
                        .copied()
                        .filter(|p| (p - pts[i]).dot(&nrm).abs() <= close)
                        .collect();
                    // (A plane holding a few points by chance isn't a contact's: it must hold
                    // a good share of them.)
                    if on.len() >= (m / 3).max(8)
                        && best.as_ref().is_none_or(|(n, _)| on.len() > *n)
                    {
                        best = Some((on.len(), on));
                    }
                }
            }
        }
        fit(&best?.1)
    })?;
    let x = any_perpendicular(&normal);
    Some(Plane {
        origin: c,
        x_dir: nalgebra::Unit::new_normalize(x),
        normal: nalgebra::Unit::new_normalize(normal),
    })
}

/// The contact of face `fi` with the motion (where its normal is square to the velocity) as
/// curves across it, boundary to boundary: the zero set of the normal's component along the
/// velocity traced over a grid of the face's parameters (marching squares, each grid crossing
/// found by bisection), each run of crossings filled in and every point pulled onto the zero set
/// by Newton steps, the points interpolated in space. Closed runs inside the face are left out.
fn contact_curves(b: &Body, fi: usize, mv: &Move) -> Vec<Curve> {
    let f = &b.faces[fi];
    let s = &f.surf;
    let sign = if f.sense { 1.0 } else { -1.0 };
    let g = |u: f64, v: f64| {
        let p = s.eval(u, v);
        let n = s.normal(u, v) * sign;
        let w = mv.velocity(&p);
        n.dot(&w) / w.norm().max(tol::TINY)
    };
    let Some((u0, u1, v0, v1)) = uv_box(b, fi) else {
        return Vec::new();
    };
    let n = 48;
    let at = |i: usize, j: usize| {
        (
            u0 + (u1 - u0) * i as f64 / n as f64,
            v0 + (v1 - v0) * j as f64 / n as f64,
        )
    };
    let vals: Vec<Vec<f64>> = (0..=n)
        .map(|i| {
            (0..=n)
                .map(|j| {
                    let (u, v) = at(i, j);
                    g(u, v)
                })
                .collect()
        })
        .collect();
    let pos = |x: f64| x > 0.0;
    let corner = |i: usize, j: usize| vals[i][j];
    // A grid edge's crossing: (i, j, 0) from (i, j) to (i + 1, j), (i, j, 1) to (i, j + 1).
    let mut crossings: std::collections::HashMap<(usize, usize, u8), (f64, f64)> =
        std::collections::HashMap::new();
    let mut cross = |i: usize, j: usize, d: u8| -> Option<(usize, usize, u8)> {
        let (i2, j2) = if d == 0 { (i + 1, j) } else { (i, j + 1) };
        let (ga, gb) = (vals[i][j], vals[i2][j2]);
        if pos(ga) == pos(gb) {
            return None;
        }
        crossings.entry((i, j, d)).or_insert_with(|| {
            let (a, z) = (at(i, j), at(i2, j2));
            let (mut lo, mut hi) = (0.0f64, 1.0f64);
            for _ in 0..60 {
                let m = 0.5 * (lo + hi);
                if pos(g(a.0 + (z.0 - a.0) * m, a.1 + (z.1 - a.1) * m)) == pos(ga) {
                    lo = m;
                } else {
                    hi = m;
                }
            }
            let m = 0.5 * (lo + hi);
            (a.0 + (z.0 - a.0) * m, a.1 + (z.1 - a.1) * m)
        });
        Some((i, j, d))
    };
    // Each cell's pieces of the zero set, between the crossings of its sides.
    type Key = (usize, usize, u8);
    let mut segs: Vec<(Key, Key)> = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let sides = [
                cross(i, j, 0),
                cross(i + 1, j, 1),
                cross(i, j + 1, 0),
                cross(i, j, 1),
            ];
            let on: Vec<Key> = sides.iter().flatten().copied().collect();
            match on.len() {
                2 => segs.push((on[0], on[1])),
                4 => {
                    // A saddle: the centre's sign says which corners connect.
                    let (a, z) = (at(i, j), at(i + 1, j + 1));
                    let c = g(0.5 * (a.0 + z.0), 0.5 * (a.1 + z.1));
                    let [bottom, right, top, left] = sides.map(|k| k.unwrap());
                    if pos(c) == pos(corner(i, j)) {
                        segs.push((bottom, right));
                        segs.push((top, left));
                    } else {
                        segs.push((bottom, left));
                        segs.push((right, top));
                    }
                }
                _ => {}
            }
        }
    }
    // The pieces chained, from the ends that touch only one piece.
    let mut touching: std::collections::HashMap<Key, Vec<usize>> = Default::default();
    for (k, (a, z)) in segs.iter().enumerate() {
        touching.entry(*a).or_default().push(k);
        touching.entry(*z).or_default().push(k);
    }
    let mut used = vec![false; segs.len()];
    let mut chains: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut starts: Vec<Key> = touching
        .iter()
        .filter(|(_, v)| v.len() == 1)
        .map(|(k, _)| *k)
        .collect();
    starts.sort();
    for st in starts {
        let mut key = st;
        let mut chain = vec![crossings[&key]];
        while let Some(&k) = touching[&key].iter().find(|&&k| !used[k]) {
            used[k] = true;
            key = if segs[k].0 == key {
                segs[k].1
            } else {
                segs[k].0
            };
            chain.push(crossings[&key]);
        }
        if chain.len() >= 2 {
            chains.push(chain);
        }
    }
    // Filled in and pulled onto the zero set.
    let (du, dv) = ((u1 - u0) * 1e-7, (v1 - v0) * 1e-7);
    let onto = |mut q: (f64, f64)| {
        for _ in 0..8 {
            let gq = g(q.0, q.1);
            let gu = (g(q.0 + du, q.1) - g(q.0 - du, q.1)) / (2.0 * du);
            let gv = (g(q.0, q.1 + dv) - g(q.0, q.1 - dv)) / (2.0 * dv);
            let gg = gu * gu + gv * gv;
            if gg <= 1e-300 || gq.abs() < 1e-15 {
                break;
            }
            q = (q.0 - gq * gu / gg, q.1 - gq * gv / gg);
        }
        q
    };
    let mut out = Vec::new();
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        for c in &chains {
            eprintln!(
                "sweep by faces: box u {u0:.4}..{u1:.4} v {v0:.4}..{v1:.4}; chain of {} from {:?} to {:?}",
                c.len(),
                c[0],
                c[c.len() - 1]
            );
        }
    }
    // (Interpolated through the grid's crossings, then through the contact halfway between two
    // points wherever the curve strays from it there by more than `fit`: as few poles as that
    // takes, since the sweep's sides carry a row of them for each.)
    let fit = CONTACT_FIT * b.bbox().diagonal().max(1.0);
    for chain in chains {
        let mut uv = chain;
        let mut curve = None;
        for round in 0..=CONTACT_ROUNDS {
            let pts: Vec<P3> = uv.iter().map(|q| s.eval(q.0, q.1)).collect();
            let mut params = vec![0.0];
            for w in pts.windows(2) {
                params.push(params.last().unwrap() + (w[1] - w[0]).norm());
            }
            if *params.last().unwrap() <= tol::POINT {
                break;
            }
            let c = Curve::Nurbs(std::sync::Arc::new(super::nurbs::NurbsCurve::interpolate(
                &pts, &params, 3, None, None,
            )));
            let halves: Vec<Option<(f64, f64)>> = if round == CONTACT_ROUNDS {
                Vec::new()
            } else {
                (0..uv.len() - 1)
                    .map(|i| {
                        let q =
                            onto((0.5 * (uv[i].0 + uv[i + 1].0), 0.5 * (uv[i].1 + uv[i + 1].1)));
                        let p = s.eval(q.0, q.1);
                        let t = c.closest(&p, params[i], params[i + 1]);
                        ((c.point(t) - p).norm() > fit).then_some(q)
                    })
                    .collect()
            };
            curve = Some(c);
            if halves.iter().all(Option::is_none) {
                break;
            }
            let mut more = Vec::with_capacity(2 * uv.len());
            for (i, q) in uv.iter().enumerate() {
                more.push(*q);
                if let Some(Some(h)) = halves.get(i) {
                    more.push(*h);
                }
            }
            uv = more;
        }
        out.extend(curve);
    }
    out
}

/// How far (per unit of the body's size) a contact curve may stray from the contact.
const CONTACT_FIT: f64 = 1e-9;

/// The most times a contact curve's points are doubled where it strays (sixteen to a grid step,
/// as many as it ever needs).
const CONTACT_ROUNDS: usize = 4;

/// The iso-parameter lines of face `fi` the contact runs along (a torus's meridians and its top
/// and bottom circles slid across its axis, a revolved face's): (u values, v values), each a
/// value where the normal's component along the velocity changes sign across a grid of the
/// face's parameters along many rows, and vanishes all along its line.
fn contact_isolines(b: &Body, fi: usize, mv: &Move) -> (Vec<f64>, Vec<f64>) {
    let f = &b.faces[fi];
    let s = &f.surf;
    let sign = if f.sense { 1.0 } else { -1.0 };
    let g = |u: f64, v: f64| {
        let p = s.eval(u, v);
        let n = s.normal(u, v) * sign;
        let w = mv.velocity(&p);
        n.dot(&w) / w.norm().max(tol::TINY)
    };
    let Some((u0, u1, v0, v1)) = uv_box(b, fi) else {
        return (Vec::new(), Vec::new());
    };
    let n = 32;
    let along = |a0: f64, a1: f64, c: f64, by_u: bool| -> Option<f64> {
        // The sign change along the row at c (u = c when by_u is false: v varies), refined.
        let at = |t: f64| if by_u { g(t, c) } else { g(c, t) };
        let mut out = None;
        for i in 0..n {
            let (a, z) = (
                a0 + (a1 - a0) * i as f64 / n as f64,
                a0 + (a1 - a0) * (i + 1) as f64 / n as f64,
            );
            let (ga, gz) = (at(a), at(z));
            if (ga > 0.0) != (gz > 0.0) {
                let (mut lo, mut hi) = (a, z);
                for _ in 0..60 {
                    let m = 0.5 * (lo + hi);
                    if (at(m) > 0.0) == (ga > 0.0) {
                        lo = m;
                    } else {
                        hi = m;
                    }
                }
                out = Some(0.5 * (lo + hi));
                let _ = out;
            }
        }
        out
    };
    let lines = |a0: f64, a1: f64, c0: f64, c1: f64, by_u: bool| -> Vec<f64> {
        // Crossings of each row (the rows at n values of the other parameter), clustered.
        let mut xs: Vec<f64> = Vec::new();
        for j in 1..n {
            let c = c0 + (c1 - c0) * j as f64 / n as f64;
            let at = |t: f64| if by_u { g(t, c) } else { g(c, t) };
            for i in 0..n {
                let (a, z) = (
                    a0 + (a1 - a0) * i as f64 / n as f64,
                    a0 + (a1 - a0) * (i + 1) as f64 / n as f64,
                );
                let ga = at(a);
                if (ga > 0.0) != (at(z) > 0.0) {
                    let (mut lo, mut hi) = (a, z);
                    for _ in 0..60 {
                        let m = 0.5 * (lo + hi);
                        if (at(m) > 0.0) == (ga > 0.0) {
                            lo = m;
                        } else {
                            hi = m;
                        }
                    }
                    xs.push(0.5 * (lo + hi));
                }
            }
        }
        xs.sort_by(f64::total_cmp);
        let close = 1e-9 * (a1 - a0).max(1.0);
        let mut out: Vec<f64> = Vec::new();
        let mut k = 0;
        while k < xs.len() {
            let mut l = k;
            while l + 1 < xs.len() && xs[l + 1] - xs[k] <= close {
                l += 1;
            }
            let x = xs[k];
            // On many rows, and the normal square to the velocity all along it.
            let on_line = (0..=16).all(|m| {
                let c = c0 + (c1 - c0) * m as f64 / 16.0;
                (if by_u { g(x, c) } else { g(c, x) }).abs() <= 1e-9
            });
            if l + 1 - k >= n / 4 && on_line {
                out.push(x);
            }
            k = l + 1;
        }
        out
    };
    let _ = along;
    // (u lines: crossings along rows of constant v; v lines: along rows of constant u.)
    (lines(u0, u1, v0, v1, true), lines(v0, v1, u0, u1, false))
}

/// Splits `b`'s edges where they cross the axis through `o` along the unit `z` (not at their
/// ends): where an edge's distance from the axis falls to nothing.
fn split_at_axis(b: &mut Body, o: P3, z: V3, size: f64) {
    let off = |p: P3| {
        let w = p - o;
        (w - z * w.dot(&z)).norm()
    };
    let near = 1e-9 * size;
    let n = b.edges.len();
    for e in 0..n {
        if b.edges[e].degenerate {
            continue;
        }
        let (t0, t1) = (b.edges[e].t[0], b.edges[e].t[1]);
        let span = t1 - t0;
        let m = 64;
        let d: Vec<f64> = (0..=m)
            .map(|i| off(b.edges[e].point(t0 + span * i as f64 / m as f64)))
            .collect();
        let mut at: Vec<f64> = Vec::new();
        for i in 1..m {
            if d[i] > d[i - 1] || d[i] > d[i + 1] {
                continue;
            }
            // The least distance about sample i, by golden section.
            let f = |t: f64| off(b.edges[e].point(t));
            let (mut lo, mut hi) = (
                t0 + span * (i - 1) as f64 / m as f64,
                t0 + span * (i + 1) as f64 / m as f64,
            );
            let g = 0.5 * (5f64.sqrt() - 1.0);
            for _ in 0..200 {
                if hi - lo <= 1e-15 * (1.0 + lo.abs()) {
                    break;
                }
                let (a, c) = (hi - g * (hi - lo), lo + g * (hi - lo));
                if f(a) < f(c) {
                    hi = c;
                } else {
                    lo = a;
                }
            }
            let t = 0.5 * (lo + hi);
            let end = 1e-6 * span.abs();
            if f(t) <= near && (t - t0).abs() > end && (t1 - t).abs() > end {
                at.push(t);
            }
        }
        // In the edge's own direction: each cut splits off the rest of the edge, which the next
        // cut splits again.
        at.sort_by(|a, b| ((a - t0) * span).total_cmp(&((b - t0) * span)));
        at.dedup_by(|a, b| (*a - *b).abs() <= 1e-9 * span.abs());
        let mut cur = e;
        for &t in &at {
            super::unify::split_edge(b, cur, t);
            cur = b.edges.len() - 1;
        }
    }
}

/// Face `fi`'s parameter box, from its p-curves.
fn uv_box(b: &Body, fi: usize) -> Option<(f64, f64, f64, f64)> {
    let (mut u0, mut u1, mut v0, mut v1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
    for c in b.faces[fi].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        for k in 0..=16 {
            let q = c.pc.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 16.0);
            (u0, u1, v0, v1) = (u0.min(q.x), u1.max(q.x), v0.min(q.y), v1.max(q.y));
        }
    }
    (u1 > u0 && v1 > v0).then_some((u0, u1, v0, v1))
}

/// The solid split along its faces' contacts with the motion, and its leading faces swept.
fn prepared(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Result<Prepared> {
    prepared_rising(k, b, steps, None)
}

/// A screw's rise as the turn goes (radians from the motion's start ↦ the rise from there), where
/// its lead changes.
pub type Rise = std::sync::Arc<dyn Fn(f64) -> f64 + Send + Sync>;

/// [`prepared`], a screw rising by `profile` (where given) rather than at its steps' one rate.
fn prepared_rising(
    k: &mut NativeKernel,
    b: &Body,
    steps: &[RigidStep],
    profile: Option<Rise>,
) -> Result<Prepared> {
    let clock = web_time::Instant::now();
    let lap = |what: &str| {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "sweep by faces: {what} at {:.0} ms",
                clock.elapsed().as_secs_f64() * 1e3
            );
        }
    };
    let along = |d: V3, z: V3| d.cross(&z).norm() <= 1e-12 * d.norm().max(1.0);
    let (start, mv) = match *steps {
        [RigidStep::Slide { from, to }] => (
            b.transformed(&Affine::translation(from)),
            Move::Slide(to - from),
        ),
        [RigidStep::Turn { axis, from, to }] => {
            let (o, z) = (axis.origin, axis.dir.into_inner());
            (
                b.transformed(&turned(o, z, from)),
                Move::Turn {
                    o,
                    z,
                    angle: to - from,
                },
            )
        }
        // A turn and a slide along its axis: a screw. Turned first, then slid, the axis moves
        // with the slide where it starts.
        [
            RigidStep::Turn { axis, from, to },
            RigidStep::Slide { from: d0, to: d1 },
        ] if along(d1 - d0, axis.dir.into_inner()) => {
            let (o, z) = (axis.origin, axis.dir.into_inner());
            let place = turned(o, z, from);
            (
                b.transformed(&Affine {
                    m: place.m,
                    t: place.t + d0,
                }),
                Move::Screw {
                    o: o + d0,
                    z,
                    angle: to - from,
                    rise: (d1 - d0).dot(&z),
                },
            )
        }
        [
            RigidStep::Slide { from: d0, to: d1 },
            RigidStep::Turn { axis, from, to },
        ] if along(d1 - d0, axis.dir.into_inner()) => {
            let (o, z) = (axis.origin, axis.dir.into_inner());
            (
                b.transformed(&Affine::translation(d0))
                    .transformed(&turned(o, z, from)),
                Move::Screw {
                    o,
                    z,
                    angle: to - from,
                    rise: (d1 - d0).dot(&z),
                },
            )
        }
        _ => {
            return Err(KernelError::Unsupported(
                "sweeping a solid by a motion other than a slide, a turn or a screw",
            ));
        }
    };
    // (A screw whose slide is nothing is a turn.)
    let mv = match mv {
        Move::Screw {
            o,
            z,
            angle,
            rise: 0.0,
        } => Move::Turn { o, z, angle },
        mv => mv,
    };
    let size = start.bbox().diagonal().max(tol::POINT);
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    // The faces that both lead and trail split along their contacts' planes, one at a time (the
    // faces after a split are numbered anew).
    let mut body = start.clone();
    let mut iso_splits = 0;
    // (Work past a few seconds is a split that isn't converging: given up, for a meshed sweep.)
    let over = || clock.elapsed().as_secs_f64() > effort();
    // The contact curves of every face that both leads and trails, traced side by side up front
    // (most of the work where many faces need them: a coil's). A curve split keeps the other
    // faces' numbers, so they stay good until a split of another kind renumbers the faces.
    let mut traced: Vec<Option<Vec<Curve>>> = {
        use rayon::prelude::*;
        super::par::start();
        let both: Vec<bool> = (0..body.faces.len())
            .map(|fi| stand(&face_points(&body, fi, size), &mv) == Stand::Both)
            .collect();
        if both.iter().filter(|b| **b).count() > 1 {
            let b = &body;
            (0..b.faces.len())
                .into_par_iter()
                .map(|fi| both[fi].then(|| contact_curves(b, fi, &mv)))
                .collect()
        } else {
            Vec::new()
        }
    };
    let mut stands: Vec<Stand> = Vec::new();
    for _ in 0..4 * start.faces.len() + 12 {
        if over() {
            return Err(KernelError::Unsupported(
                "sweeping a solid whose faces' contacts take too long to split along",
            ));
        }
        let tick = web_time::Instant::now();
        // Each face's stand, kept from one split to the next for the faces it left alone.
        if stands.len() != body.faces.len() {
            use rayon::prelude::*;
            let b = &body;
            let have = stands.len();
            let more: Vec<Stand> = (have..b.faces.len())
                .into_par_iter()
                .map(|fi| stand(&face_points(b, fi, size), &mv))
                .collect();
            stands.extend(more);
        }
        let both = (0..body.faces.len()).find(|&fi| stands[fi] == Stand::Both);
        let Some(fi) = both else { break };
        let t_stand = tick.elapsed();
        // Along iso-parameter lines: split there, exactly.
        let (us, vs) = contact_isolines(&body, fi, &mv);
        let t_iso = tick.elapsed();
        if !us.is_empty() || !vs.is_empty() {
            use super::heal_split::{Dir, split_face_iso};
            let mut nb = body.clone();
            let mut faces = vec![fi];
            for (dir, vals) in [(Dir::U, &us), (Dir::V, &vs)] {
                if vals.is_empty() {
                    continue;
                }
                let mut next_faces = Vec::new();
                for &g in &faces {
                    match split_face_iso(&mut nb, g, dir, vals) {
                        Ok(out) => next_faces.extend(out),
                        Err(_) => next_faces.push(g),
                    }
                }
                faces = next_faces;
            }
            if debug {
                eprintln!(
                    "sweep by faces: face {fi} {:?} meets the motion along iso-lines u {us:?} v {vs:?}: {} pieces",
                    body.faces[fi].surf.kind(),
                    faces.len()
                );
            }
            if nb.faces.len() > body.faces.len() {
                body = nb;
                traced.clear();
                stands.clear();
                continue;
            }
        }
        let plane = contact_plane(&body, fi, &mv, size);
        let t_plane = tick.elapsed();
        if debug {
            eprintln!(
                "sweep by faces: face {fi} {:?} leads and trails; contact plane {:?}",
                body.faces[fi].surf.kind(),
                plane.map(|p| (p.origin, p.normal.into_inner()))
            );
        }
        // Split along the plane; else along the contact curves traced on the face.
        let mut next: Option<Body> = None;
        let mut by_curve = false;
        if let Some(plane) = plane {
            let id = k.insert(body.clone());
            let split = k.split_faces(id, &[FaceId(fi as u64)], &SplitTool::Plane(plane));
            k.release(id);
            match split {
                Ok(r) => {
                    if let Some(&nid) = r.bodies.first() {
                        let nb = (**k.get(nid)?).clone();
                        k.release(nid);
                        if nb.faces.len() > body.faces.len() {
                            next = Some(nb);
                        }
                    }
                }
                Err(e) if debug => eprintln!("sweep by faces: the split failed: {e}"),
                Err(_) => {}
            }
        }
        if next.is_none() {
            let curves = match traced.get_mut(fi).and_then(Option::take) {
                Some(c) => c,
                None => contact_curves(&body, fi, &mv),
            };
            if debug {
                eprintln!("sweep by faces: {} contact curves traced", curves.len());
            }
            for c in &curves {
                let Curve::Nurbs(n) = c else { continue };
                let (t0, t1) = n.range();
                if debug {
                    let near = |p: P3| {
                        body.faces[fi]
                            .loops
                            .iter()
                            .flatten()
                            .map(|co| {
                                let e = &body.edges[co.edge];
                                (0..=2000)
                                    .map(|i| {
                                        let t = e.t[0] + (e.t[1] - e.t[0]) * i as f64 / 2000.0;
                                        (e.point(t) - p).norm()
                                    })
                                    .fold(f64::INFINITY, f64::min)
                            })
                            .fold(f64::INFINITY, f64::min)
                    };
                    let (a, z) = (c.point(t0), c.point(t1));
                    let (u, v) = body.faces[fi].surf.project(&c.point(0.5 * (t0 + t1)), None);
                    eprintln!(
                        "sweep by faces: curve {a:?} to {z:?}, ends {:.2e} and {:.2e} off the boundary, middle {:.2e} off the surface",
                        near(a),
                        near(z),
                        (body.faces[fi].surf.eval(u, v) - c.point(0.5 * (t0 + t1))).norm()
                    );
                }
                let mut nb = body.clone();
                if super::curve_split::split_face_curve(&mut nb, fi, c, t0, t1).is_some() {
                    nb.fit_tolerances(1e-3);
                    next = Some(nb);
                    by_curve = true;
                    break;
                }
            }
        }
        // A contact closed round the face's seam (an ellipsoid's, a ring's) meets its boundary
        // at one point only: the face halved along an iso-line first, then traced again.
        if next.is_none()
            && iso_splits < 8
            && let Some((u0, u1, v0, v1)) = uv_box(&body, fi)
        {
            use super::heal_split::{Dir, split_face_iso};
            for (dir, mid) in [(Dir::U, 0.5 * (u0 + u1)), (Dir::V, 0.5 * (v0 + v1))] {
                let mut nb = body.clone();
                if split_face_iso(&mut nb, fi, dir, &[mid]).is_ok_and(|f| f.len() > 1) {
                    if debug {
                        eprintln!("sweep by faces: face {fi} halved along an iso-line");
                    }
                    iso_splits += 1;
                    next = Some(nb);
                    break;
                }
            }
        }
        if debug {
            eprintln!(
                "sweep by faces: split of face {fi}: stand {:?} iso {:?} plane {:?} all {:?}",
                t_stand,
                t_iso - t_stand,
                t_plane - t_iso,
                tick.elapsed()
            );
        }
        let nb = next.ok_or(KernelError::Unsupported(
            "sweeping a solid whose face meets the motion along a curve it can't be split along",
        ))?;
        body = nb;
        if by_curve {
            // (The split face measured again, and the faces appended at the end.)
            if fi < stands.len() {
                stands[fi] = stand(&face_points(&body, fi, size), &mv);
            }
        } else {
            traced.clear();
            stands.clear();
        }
    }
    lap("split");
    // A turn's edges cut where they cross its axis: on either side a point turns the other way
    // round, so each piece of an edge sweeps a face of its own (a ball's meridian through the
    // axis sweeps the apple's cap one side of it and, the other, a sheet inside the ball).
    if let Move::Turn { o, z, .. } = mv {
        split_at_axis(&mut body, o, z, size);
    }
    // Each leading face swept, the pieces united with the start.
    let sweep = match mv {
        Move::Slide(v) => Some(BodySweep::Translate(v)),
        Move::Turn { o, z, angle } => Some(BodySweep::Rotate {
            axis: Axis {
                origin: o,
                dir: nalgebra::Unit::new_normalize(z),
            },
            angle,
        }),
        Move::Screw { .. } => None,
    };
    // A screw's sections, for the body's reach from its axis (counter-clockwise about the
    // reversed axis for a negative turn).
    let screw = match mv {
        Move::Screw { o, z, angle, rise } => {
            let zs = z * angle.signum();
            let bb = body.bbox();
            let reach = (0..8)
                .map(|i| {
                    let c = P3::new(
                        if i & 1 == 0 { bb.min.x } else { bb.max.x },
                        if i & 2 == 0 { bb.min.y } else { bb.max.y },
                        if i & 4 == 0 { bb.min.z } else { bb.max.z },
                    );
                    let w = c - o;
                    (w - zs * w.dot(&zs)).norm()
                })
                .fold(0.0, f64::max);
            let s1 = angle.abs();
            let sign = angle.signum();
            Some(super::sweep::Screw::rising(
                o,
                zs,
                rise * sign / s1,
                profile
                    .clone()
                    .map(|f| -> Rise { std::sync::Arc::new(move |s: f64| f(s * sign) * sign) }),
                s1,
                reach,
                SCREW_TOL * size.max(1.0),
            ))
        }
        _ => None,
    };
    // A lead that changes: each face must stand as it does at the least and the greatest lead
    // (else its contact moves as the lead does, which this sweep doesn't follow).
    if let (Some(f), Move::Screw { o, z, angle, .. }) = (&profile, mv) {
        let n = 256;
        let slopes: Vec<f64> = (0..n)
            .map(|i| {
                let (a, b) = (
                    angle * i as f64 / n as f64,
                    angle * (i + 1) as f64 / n as f64,
                );
                (f(b) - f(a)) / (b - a) * angle
            })
            .collect();
        let lo = slopes.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = slopes.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        for fi in 0..body.faces.len() {
            let pts = face_points(&body, fi, size);
            let at = |rise: f64| stand(&pts, &Move::Screw { o, z, angle, rise });
            if at(lo) != at(hi) {
                return Err(KernelError::Unsupported(
                    "screwing a solid at a changing lead that moves its faces' contacts",
                ));
            }
        }
    }
    let mut pieces: Vec<Body> = Vec::new();
    for fi in 0..body.faces.len() {
        let st = stand(&face_points(&body, fi, size), &mv);
        if debug {
            eprintln!(
                "sweep by faces: face {fi} {:?} {st:?}",
                body.faces[fi].surf.kind()
            );
        }
        match st {
            Stand::Leads => {
                if let Some(w) = &screw {
                    let sheet = super::kernel::subset(&body, &[fi], false);
                    let swept = super::sweep::sweep_body(
                        &sheet,
                        &super::sweep::BodyMove::Screw(w),
                        mv_angle(&mv),
                        false,
                        0,
                    );
                    if debug && let Err(e) = &swept {
                        eprintln!("sweep by faces: sweeping face {fi} failed: {e}");
                    }
                    let piece = swept?.body;
                    if debug {
                        eprintln!(
                            "sweep by faces: face {fi} screwed: check {:?}",
                            piece.check().first()
                        );
                    }
                    pieces.push(piece);
                    continue;
                }
                let Some(sweep) = &sweep else { continue };
                let sheet = k.insert(super::kernel::subset(&body, &[fi], false));
                let swept = k.sweep_body(sheet, sweep);
                k.release(sheet);
                if debug && let Err(e) = &swept {
                    eprintln!("sweep by faces: sweeping face {fi} failed: {e}");
                }
                for id in swept?.bodies {
                    pieces.push((**k.get(id)?).clone());
                    k.release(id);
                }
            }
            Stand::Both => {
                return Err(KernelError::Unsupported(
                    "sweeping a solid whose face meets the motion along a curve it can't be split along",
                ));
            }
            Stand::Trails | Stand::Along => {}
        }
    }
    Ok(Prepared {
        start,
        body,
        pieces,
        mv,
        size,
        clock,
    })
}

/// What [`by_faces`] unites: the solid where it starts (its faces split along their contacts)
/// and each of its leading faces' sweeps, with the motion and the solid's size.
struct Prepared {
    start: Body,
    body: Body,
    pieces: Vec<Body>,
    mv: Move,
    size: f64,
    clock: web_time::Instant,
}

/// The solids whose union is the sweep of `b` by one slide, turn or screw ([`by_faces`]): the
/// solid where it starts and each of its leading faces' sweeps. They overlap where a point
/// enters the moving solid twice; what the sweep cuts from a body is the body less each of them
/// in turn, which needs no union.
pub fn face_sweeps(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Result<Vec<Body>> {
    let p = prepared(k, b, steps)?;
    let mut all = vec![p.body];
    all.extend(p.pieces);
    Ok(all)
}

/// The pieces of a screw about the axis through `o` along the unit `z` by `angle` (radians from
/// where `b` is) rising `rise(θ)` at the turn θ (0 at 0): [`face_sweeps`] of a screw whose lead
/// changes, where every face's contact stays put as it does (a face square to the axis, or along
/// it).
pub fn rising_screw_sweeps(
    k: &mut NativeKernel,
    b: &Body,
    o: P3,
    z: V3,
    angle: f64,
    rise: Rise,
) -> Result<Vec<Body>> {
    let steps = [
        RigidStep::Turn {
            axis: Axis {
                origin: o,
                dir: nalgebra::Unit::new_normalize(z),
            },
            from: 0.0,
            to: angle,
        },
        RigidStep::Slide {
            from: V3::zeros(),
            to: z.normalize() * rise(angle),
        },
    ];
    let p = prepared_rising(k, b, &steps, Some(rise))?;
    let mut all = vec![p.body];
    all.extend(p.pieces);
    Ok(all)
}

/// [`crate::Kernel::sweep_solid`] for one slide or one turn of any solid (see the module docs).
pub fn by_faces(k: &mut NativeKernel, b: &Body, steps: &[RigidStep]) -> Result<OpResult> {
    by_faces_or_pieces(k, b, steps, None)
}

/// [`by_faces`], but where its pieces overlap (they would be united), with `pieces` given: the
/// solid where it starts and the leading faces' sweeps put there, unmade, and an error returned
/// (what a cut needs: the target less each in turn, no union).
pub fn by_faces_or_pieces(
    k: &mut NativeKernel,
    b: &Body,
    steps: &[RigidStep],
    pieces_out: Option<&mut Vec<Body>>,
) -> Result<OpResult> {
    let Prepared {
        start,
        body,
        pieces,
        mv,
        size,
        clock,
    } = prepared(k, b, steps)?;
    let lap = |what: &str| {
        if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
            eprintln!(
                "sweep by faces: {what} at {:.0} ms",
                clock.elapsed().as_secs_f64() * 1e3
            );
        }
    };
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    let over = || clock.elapsed().as_secs_f64() > effort();
    if pieces.is_empty() {
        let first = k.insert(start);
        return Ok(OpResult {
            bodies: vec![first],
            history: History::default(),
        });
    }
    // The start and the pieces glued along the faces two of them share (a piece's start cap and
    // the face it swept; the sides two neighbouring leading faces' common edge sweeps): those
    // pairs dropped and the rest sewn. Else united.
    lap("swept");
    let mut all = vec![body.clone()];
    all.extend(pieces.iter().cloned());
    if let Some(r) = glued(k, &all, size) {
        // A screw's faces cut into quarter turns, if the body stays sound.
        if let Move::Screw { .. } = mv
            && let [id] = r.bodies.as_slice()
        {
            let mut cut = (**k.get(*id)?).clone();
            quarter_turns(&mut cut, mv_angle(&mv));
            if cut.is_solid() && cut.check().is_empty() {
                if debug {
                    eprintln!(
                        "sweep by faces: cut into {} faces of a quarter turn",
                        cut.faces.len()
                    );
                }
                k.release(*id);
                return Ok(OpResult {
                    bodies: vec![k.insert(cut)],
                    history: History::default(),
                });
            } else if debug {
                eprintln!(
                    "sweep by faces: cutting into quarter turns left {:?}",
                    cut.check().first()
                );
            }
        }
        return Ok(r);
    }
    if let Some(out) = pieces_out {
        out.push(body);
        out.extend(pieces);
        return Err(KernelError::Unsupported(
            "sweeping a solid whose faces' sweeps overlap, as pieces",
        ));
    }
    if debug {
        eprintln!("sweep by faces: gluing failed; uniting");
    }
    let first = k.insert(body);
    let ids: Vec<BodyId> = pieces.into_iter().map(|p| k.insert(p)).collect();
    let r = k.boolean(BoolOp::Union, first, &ids);
    if debug && let Err(e) = &r {
        eprintln!(
            "sweep by faces: the union of {} pieces failed: {e}",
            ids.len()
        );
    }
    // Else one piece at a time, each into what the ones before made (a piece that fails is
    // tried again after the rest).
    let r = match r {
        Ok(r) => Ok(r),
        Err(e) => {
            let mut acc = first;
            let mut todo: Vec<BodyId> = ids.clone();
            let mut last_err = e;
            let mut stuck = 0;
            while let Some(p) = todo.first().copied() {
                if over() {
                    break;
                }
                todo.remove(0);
                match k.boolean(BoolOp::Union, acc, &[p]) {
                    Ok(r) if r.bodies.len() == 1 => {
                        if acc != first {
                            k.release(acc);
                        }
                        acc = r.bodies[0];
                        stuck = 0;
                    }
                    Ok(r) => {
                        for b in r.bodies {
                            k.release(b);
                        }
                        last_err = KernelError::InvalidResult {
                            op: "solid sweep",
                            why: "uniting a piece made more than one solid".into(),
                        };
                        todo.push(p);
                        stuck += 1;
                    }
                    Err(e) => {
                        if debug {
                            eprintln!("sweep by faces: uniting a piece failed: {e}");
                        }
                        last_err = e;
                        todo.push(p);
                        stuck += 1;
                    }
                }
                if stuck > todo.len() {
                    break;
                }
            }
            if todo.is_empty() {
                Ok(OpResult {
                    bodies: vec![acc],
                    history: History::default(),
                })
            } else {
                if acc != first {
                    k.release(acc);
                }
                Err(last_err)
            }
        }
    };
    k.release(first);
    for p in ids {
        k.release(p);
    }
    let r = r?;
    Ok(OpResult {
        bodies: r.bodies,
        history: History::default(),
    })
}

/// The winding number of the closed triangles `meshes` about `p` (1 inside, 0 outside): their
/// solid angles there summed (Van Oosterom and Strackee), over 4π.
fn winding(meshes: &[super::tess::FaceMesh], p: &P3) -> f64 {
    let mut total = 0.0;
    for m in meshes {
        for t in &m.indices {
            let a = m.positions[t[0] as usize] - p;
            let b = m.positions[t[1] as usize] - p;
            let c = m.positions[t[2] as usize] - p;
            let (la, lb, lc) = (a.norm(), b.norm(), c.norm());
            let num = a.dot(&b.cross(&c));
            let den = la * lb * lc + a.dot(&b) * lc + a.dot(&c) * lb + b.dot(&c) * la;
            total += 2.0 * num.atan2(den);
        }
    }
    total / (4.0 * std::f64::consts::PI)
}

/// Whether faces `fa` of `a` and `fb` of `b` (on one surface) face opposite ways: their outward
/// normals at a point inside the first (a face wrapped round an axis has its centroid off it).
fn facing(a: &Body, fa: usize, b: &Body, fb: usize) -> bool {
    let (f, g) = (&a.faces[fa], &b.faces[fb]);
    let polys = super::classify::face_polygons(a, f);
    let Some(q) = super::boolean::interior_points_of_polys(&polys)
        .into_iter()
        .next()
    else {
        return false;
    };
    let p = f.surf.eval(q.x, q.y);
    let na = f.surf.normal(q.x, q.y) * if f.sense { 1.0 } else { -1.0 };
    let (u, v) = g.surf.project(&p, None);
    let nb = g.surf.normal(u, v) * if g.sense { 1.0 } else { -1.0 };
    na.dot(&nb) < -0.5
}

/// Whether face `fa` of `a` lies on face `fb`'s surface of `b`: the same surface, or (one
/// parametrized another way: the sweeps of a rim from either side) points inside it on the other.
fn coincide(a: &Body, fa: usize, b: &Body, fb: usize, close: f64) -> bool {
    let (sa, sb) = (&a.faces[fa].surf, &b.faces[fb].surf);
    if sa.same_domain(sb, close).is_some() {
        return true;
    }
    let polys = super::classify::face_polygons(a, &a.faces[fa]);
    let pts = super::boolean::interior_points_of_polys(&polys);
    !pts.is_empty()
        && pts.iter().take(6).all(|q| {
            let p = sa.eval(q.x, q.y);
            let (u, v) = sb.project(&p, None);
            (sb.eval(u, v) - p).norm() <= close
        })
}

/// `pieces` (closed bodies, a sweep's: [`face_sweeps`]) glued into as few bodies as gluing
/// makes ([`glued`]): each piece into the first group it makes one sound solid with, else a group
/// of its own. Pieces that only touch (neighbouring faces' sweeps, sharing the face their
/// common edge sweeps) become one, so that cutting the groups one after another meets no
/// surfaces that touch along a curve.
pub fn glued_groups(k: &mut NativeKernel, pieces: Vec<Body>) -> Vec<Body> {
    let size = pieces
        .iter()
        .map(|p| p.bbox().diagonal())
        .fold(0.0, f64::max)
        .max(tol::POINT);
    // All of them at once, where they make one solid so (sewn and simplified the once: one at
    // a time, each joins a group the ones before have grown).
    if pieces.len() > 2
        && let Some(r) = glued(k, &pieces, size)
    {
        let one = match r.bodies.as_slice() {
            [id] => k
                .get(*id)
                .ok()
                .filter(|b| b.solids.len() == 1)
                .map(|b| (**b).clone()),
            _ => None,
        };
        for id in r.bodies {
            k.release(id);
        }
        if let Some(b) = one {
            return vec![b];
        }
    }
    let mut groups: Vec<Body> = Vec::new();
    'piece: for p in pieces {
        for g in groups.iter_mut().rev() {
            // (Only bodies whose boxes meet can share a face.)
            let (gb, pb) = (g.bbox(), p.bbox());
            let apart = (0..3).any(|i| {
                gb.min[i] > pb.max[i] + 1e-9 * size || pb.min[i] > gb.max[i] + 1e-9 * size
            });
            if apart {
                continue;
            }
            if let Some(r) = glued(k, &[g.clone(), p.clone()], size) {
                let one = match r.bodies.as_slice() {
                    [id] => k
                        .get(*id)
                        .ok()
                        .filter(|b| b.solids.len() == 1)
                        .map(|b| (**b).clone()),
                    _ => None,
                };
                for id in r.bodies {
                    k.release(id);
                }
                if let Some(b) = one {
                    *g = b;
                    continue 'piece;
                }
            }
        }
        groups.push(p);
    }
    groups
}

/// A face's boundary by its length and its middle (∫ p ds over its loops' edges, by the
/// length): what two faces with one boundary have alike, read off their edges alone (a face's
/// area and centroid tell as much, but take the whole surface: on a screw's long sides, most
/// of a sweep's time).
fn boundary_measure(b: &Body, fi: usize) -> (f64, P3) {
    // (Gauss–Legendre, five points, on short steps of each smooth piece.)
    const X: [f64; 5] = [
        -0.906_179_845_938_664,
        -0.538_469_310_105_683_1,
        0.0,
        0.538_469_310_105_683_1,
        0.906_179_845_938_664,
    ];
    const W: [f64; 5] = [
        0.236_926_885_056_189_08,
        0.478_628_670_499_366_47,
        0.568_888_888_888_888_9,
        0.478_628_670_499_366_47,
        0.236_926_885_056_189_08,
    ];
    let (mut length, mut moment) = (0.0, V3::zeros());
    for c in b.faces[fi].loops.iter().flatten() {
        let e = &b.edges[c.edge];
        if e.degenerate {
            continue;
        }
        let (lo, hi) = (e.t[0].min(e.t[1]), e.t[0].max(e.t[1]));
        let steps = match e.curve {
            Curve::Line { .. } => 1,
            Curve::Nurbs(_) => 4,
            _ => (((hi - lo) / 0.1).ceil() as usize).max(1),
        };
        for w in e.curve.pieces(lo, hi).windows(2) {
            for k in 0..steps {
                let (a, z) = (
                    w[0] + (w[1] - w[0]) * k as f64 / steps as f64,
                    w[0] + (w[1] - w[0]) * (k + 1) as f64 / steps as f64,
                );
                let (m, r) = (0.5 * (a + z), 0.5 * (z - a));
                for (x, gw) in X.iter().zip(W) {
                    let t = m + r * x;
                    let ds = gw * r * e.curve.tangent(t).norm();
                    length += ds;
                    moment += e.curve.point(t).coords * ds;
                }
            }
        }
    }
    if length > tol::TINY {
        (length, P3::from(moment / length))
    } else {
        (0.0, b.face_bbox(fi).center())
    }
}

/// The solid the closed bodies `all` make together where they only touch along whole faces: each
/// pair of faces from two of them on one surface with the same boundary (the same measure and
/// middle) and opposite outward normals dropped, the rest sewn into one closed solid. None when
/// that fails.
pub(super) fn glued(k: &mut NativeKernel, all: &[Body], size: f64) -> Option<OpResult> {
    glued_with(k, all, size, false)
}

/// [`glued`]; with `apart`, of bodies known not to overlap (a body's own copies set side by
/// side), which then needn't be shown.
pub(super) fn glued_with(
    k: &mut NativeKernel,
    all: &[Body],
    size: f64,
    apart: bool,
) -> Option<OpResult> {
    let close = 1e-7 * size.max(1.0);
    // Each face's measure and middle, and its outward normal near there: its area and
    // centroid, or on a surface that must be walked over to have them (a B-spline, a swept or
    // a revolved one) its boundary's length and middle. (A face closed on itself by a seam has
    // no outline to go by; the surfaces that have such integrate along their edges alone.)
    let walked = |s: &super::surface::Surface| {
        matches!(
            s,
            super::surface::Surface::Nurbs(_)
                | super::surface::Surface::Extrusion { .. }
                | super::surface::Surface::Revolution { .. }
        )
    };
    let mut faces: Vec<(usize, usize, P3, f64, V3)> = Vec::new();
    for (bi, b) in all.iter().enumerate() {
        for fi in 0..b.faces.len() {
            let (a, c) = if walked(&b.faces[fi].surf) {
                boundary_measure(b, fi)
            } else {
                super::mass::face_area(b, fi)
            };
            let f = &b.faces[fi];
            let (u, v) = f.surf.project(&c, None);
            let n = f.surf.normal(u, v) * if f.sense { 1.0 } else { -1.0 };
            faces.push((bi, fi, c, a, n));
        }
    }
    let mut dropped = vec![false; faces.len()];
    for i in 0..faces.len() {
        if dropped[i] {
            continue;
        }
        for j in i + 1..faces.len() {
            let (a, b2) = (&faces[i], &faces[j]);
            if dropped[j] || a.0 == b2.0 {
                continue;
            }
            if (a.2 - b2.2).norm() <= close
                && (a.3 - b2.3).abs() <= close * size.max(1.0)
                && coincide(&all[a.0], a.1, &all[b2.0], b2.1, close)
                && facing(&all[a.0], a.1, &all[b2.0], b2.1)
            {
                dropped[i] = true;
                dropped[j] = true;
                break;
            }
        }
    }
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        for (i, f) in faces.iter().enumerate() {
            eprintln!(
                "  glue: body {} face {} {:?} middle {:.4?} measure {:.6} normal {:.3?}{}",
                f.0,
                f.1,
                all[f.0].faces[f.1].surf.kind(),
                f.2,
                f.3,
                f.4,
                if dropped[i] { " dropped" } else { "" }
            );
        }
    }
    if !dropped.iter().any(|&d| d) {
        return None;
    }
    // Gluing is right only where the bodies don't overlap (the sweeps of two leading faces of a
    // turned tool can): every kept face then bounds the whole, so points just outside it lie
    // outside every body.
    let t_check = web_time::Instant::now();
    if !apart {
        use rayon::prelude::*;
        let polys: Vec<Vec<Vec<Vec<P2>>>> = all
            .par_iter()
            .map(|b| {
                b.faces
                    .iter()
                    .map(|f| super::classify::face_polygons(b, f))
                    .collect()
            })
            .collect();
        // (Judged by ray parity, the majority of a few rays: no mesh, which a face folded along an
        // edge can make huge; an overlap it finds confirmed by the body's winding number over its
        // mesh, as a ray through the long helical B-spline faces a screw sweeps can miss a
        // crossing. Each kept face on its own: the faces are many and the rays dear.)
        let boxes: Vec<Bbox> = all.iter().map(|b| b.bbox()).collect();
        let eps = 1e-3 * size.max(1.0);
        let kept: Vec<(usize, usize)> = faces
            .iter()
            .zip(&dropped)
            .filter(|(_, d)| !**d)
            .map(|(f, _)| (f.0, f.1))
            .collect();
        // (The rays side by side, a kept face each; the meshes after, one thread's work: a
        // mesh is made across the threads itself, and may not be waited for from one of them.)
        let suspects: Vec<(usize, usize, usize, P3)> = kept
            .par_iter()
            .flat_map_iter(|&(bi, fi)| {
                let f = &all[bi].faces[fi];
                let sign = if f.sense { 1.0 } else { -1.0 };
                let mut found = Vec::new();
                for q in super::boolean::interior_points_of_polys(&polys[bi][fi])
                    .into_iter()
                    .take(4)
                {
                    let out = f.surf.eval(q.x, q.y) + f.surf.normal(q.x, q.y) * (sign * eps);
                    for bj in 0..all.len() {
                        if bj != bi
                            && boxes[bj].contains(&out)
                            && super::classify::parity_inside(&all[bj], &polys[bj], &out)
                        {
                            found.push((bi, fi, bj, out));
                        }
                    }
                }
                found
            })
            .collect();
        let mut meshes: Vec<Option<Vec<super::tess::FaceMesh>>> = vec![None; all.len()];
        let overlap = suspects.into_iter().find(|&(_, _, bj, out)| {
            let m = meshes[bj].get_or_insert_with(|| {
                super::tess::mesh_body(&all[bj], 1e-4 * size.max(1.0), 0.3).0
            });
            winding(m, &out) > 0.5
        });
        if let Some((bi, fi, bj, out)) = overlap {
            if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
                eprintln!(
                    "sweep by faces: the bodies overlap (face {fi} of body {bi}, {:?} {:.4?}, inside body {bj}; volumes {:.4?})",
                    all[bi].faces[fi].surf.kind(),
                    out,
                    all.iter().map(super::mass::body_volume).collect::<Vec<_>>()
                );
            }
            return None;
        }
    }
    let ids: Vec<BodyId> = faces
        .iter()
        .zip(&dropped)
        .filter(|(_, d)| !**d)
        .map(|((bi, fi, ..), _)| k.insert(super::kernel::subset(&all[*bi], &[*fi], false)))
        .collect();
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        eprintln!(
            "sweep by faces: overlap check {:.0} ms",
            t_check.elapsed().as_secs_f64() * 1e3
        );
    }
    let t_sew = web_time::Instant::now();
    let r = k.sew_solid(&ids, 1e-6 * size.max(1.0));
    if std::env::var_os("CADRS_SWEEP_DEBUG").is_some() {
        eprintln!(
            "sweep by faces: sewing {:.0} ms",
            t_sew.elapsed().as_secs_f64() * 1e3
        );
    }
    for id in ids {
        k.release(id);
    }
    let debug = std::env::var_os("CADRS_SWEEP_DEBUG").is_some();
    if debug {
        eprintln!(
            "sweep by faces: {} of {} faces dropped in pairs; sewn: {:?}",
            dropped.iter().filter(|&&d| d).count(),
            faces.len(),
            r.as_ref()
                .map(|r| r.bodies.len())
                .map_err(|e| e.to_string())
        );
    }
    let r = r.ok()?;
    let id = *r.bodies.first()?;
    // (Edges sewn from two pieces' copies, one an intersection's fit, take the tolerance their
    // p-curves need, as a split's do.)
    {
        let mut b = (**k.get(id).ok()?).clone();
        // (Each loop's p-curves in one run of periods: pieces' copies may sit a turn apart,
        // also in v about a turned circle.)
        for fi in 0..b.faces.len() {
            let surf = b.faces[fi].surf.clone();
            let pu = surf.u_periodic().then_some(std::f64::consts::TAU);
            let pv = (surf.v_periodic()
                || matches!(&surf, super::surface::Surface::Revolution { m, .. } if m.is_periodic()))
            .then_some(std::f64::consts::TAU);
            let mut loops = std::mem::take(&mut b.faces[fi].loops);
            for l in &mut loops {
                for k in 1..l.len() {
                    let (pe, ps) = {
                        let (prev, cur) = (&l[k - 1], &l[k]);
                        let (_, z) = super::topo::co_range(&b.edges[prev.edge], prev.fwd);
                        let (a, _) = super::topo::co_range(&b.edges[cur.edge], cur.fwd);
                        (prev.pc.point(z), cur.pc.point(a))
                    };
                    let shift = |d: f64, per: Option<f64>| per.map_or(0.0, |p| (d / p).round() * p);
                    let (du, dv) = (shift(pe.x - ps.x, pu), shift(pe.y - ps.y, pv));
                    if du != 0.0 || dv != 0.0 {
                        l[k].pc = l[k]
                            .pc
                            .transformed(&Affine::translation(V3::new(du, dv, 0.0)));
                    }
                }
            }
            b.faces[fi].loops = loops;
        }
        b.fit_tolerances(1e-3);
        k.release(id);
        let nid = k.insert(b);
        glued_checked(k, nid, debug)
    }
}

/// The sewn solid `id` of [`glued`], checked and simplified: first its degenerate edges' p-curves
/// bridged (a cone's apex, where the contact curves the faces were split along arrive at their
/// own u).
fn glued_checked(k: &mut NativeKernel, id: BodyId, debug: bool) -> Option<OpResult> {
    let id = match k.get(id) {
        Ok(b) if b.edges.iter().any(|e| e.degenerate) => {
            let mut b = (**b).clone();
            if super::heal::bridge_degenerate(&mut b) > 0 {
                k.release(id);
                k.insert(b)
            } else {
                id
            }
        }
        _ => id,
    };
    if debug && let Ok(b) = k.get(id) {
        eprintln!(
            "sweep by faces: sewn solid {}, check {:?}, volume {}",
            b.is_solid(),
            b.check().first(),
            super::mass::body_volume(b)
        );
    }
    let ok = k
        .get(id)
        .ok()
        .is_some_and(|b| b.is_solid() && b.check().is_empty());
    if !ok {
        return None;
    }
    // The pieces' faces on one surface (a start face and the sweep of its edge, say) merged.
    let id = match super::surfacing::simplify(k, id) {
        Ok(s)
            if s.bodies.len() == 1
                && k.get(s.bodies[0])
                    .is_ok_and(|b| b.is_solid() && b.check().is_empty()) =>
        {
            k.release(id);
            s.bodies[0]
        }
        _ => id,
    };
    if debug && let Ok(b) = k.get(id) {
        eprintln!("sweep by faces: simplified to {} faces", b.faces.len());
    }
    Some(OpResult {
        bodies: vec![id],
        history: History::default(),
    })
}

//! Face blends (Onshape's Face blend, rolling ball): a blend between two faces that meet along
//! an edge, the walls trimmed to it and joined with it into one sheet.
//!
//! The faces' common edge is the spine. In the plane square to it at each station a ball touches
//! both faces ([`fillet_geom::ball_in_plane`]) on the side where the faces run on from the edge
//! (the sector between them): of the radius, or of the radius whose contacts lie the width
//! apart. The stations run a little past the edge's ends, so the blend surface (the circular
//! sections interpolated along the spine, [`fillet_geom::nurbs_strip`]) reaches past the faces;
//! each rail (contact curve) is cut where it leaves its face. Each wall is split along its rail
//! and what lies between the rail and the edge goes, with the faces beyond it; the blend face
//! is the strip between the rails' cuts, its ends straight across its (u, v); all is sewn.
//!
//! [`fillet_geom::ball_in_plane`]: super::fillet_geom::ball_in_plane
//! [`fillet_geom::nurbs_strip`]: super::fillet_geom::nurbs_strip

use std::sync::Arc;

use super::classify::{InFace, face_polygons, in_face};
use super::curve::Curve;
use super::fillet_geom::{Ball, Spine, ball_in_plane, foot, nurbs_strip};
use super::kernel::NativeKernel;
use super::math::*;
use super::nurbs::NurbsCurve;
use super::surface::Surface;
use super::tol;
use super::topo::{Body, Coedge, Face, Shell};
use crate::*;

fn invalid(why: &str) -> KernelError {
    KernelError::InvalidParameter(why.into())
}

/// Stations along the spine.
const STATIONS: usize = 144;
/// How far past each end of the spine the stations run (a share of its length).
const PAST: f64 = 0.15;

/// The face blend (see the module docs).
pub fn face_blend(k: &mut NativeKernel, spec: &FaceBlendSpec) -> Result<OpResult> {
    let size = match spec.size {
        BlendSize::Radius(r) | BlendSize::Width(r) => r,
    };
    if !(size.is_finite() && size > 0.0) {
        return Err(invalid("A face blend's size must be greater than zero"));
    }
    let (b1, f1) = (spec.side1.0, spec.side1.1.0 as usize);
    let (b2, f2) = (spec.side2.0, spec.side2.1.0 as usize);
    let bodies: Vec<BodyId> = if b1 == b2 { vec![b1] } else { vec![b1, b2] };
    let one = (**k.get(b1)?).clone();
    let two = (**k.get(b2)?).clone();
    if f1 >= one.faces.len() || f2 >= two.faces.len() {
        return Err(invalid("A face to blend no longer exists"));
    }
    // The spine: an edge of each face along the same curve.
    let (e1, e2) = common_edge(&one, f1, &two, f2).ok_or(KernelError::Unsupported(
        "face blends of faces that don't meet along an edge",
    ))?;
    let edge = &one.edges[e1];
    let spine = Spine::new(edge.curve.clone(), edge.t, false);
    let (t0, t1) = (edge.t[0], edge.t[1]);
    let span = t1 - t0;
    let faces = [&one.faces[f1], &two.faces[f2]];
    let polys = [face_polygons(&one, faces[0]), face_polygons(&two, faces[1])];
    let scale = one
        .face_bbox(f1)
        .diagonal()
        .max(two.face_bbox(f2).diagonal())
        .max(1.0);
    let near = 1e-6 * scale;
    // Which way into each face, square to the spine, at `p` with tangent `t`.
    let into = |k: usize, p: &P3, t: &V3, hint: Option<P2>| -> Option<(V3, V3, P2)> {
        let f = faces[k];
        let uv = foot(f, p, hint);
        let n = f.surf.normal(uv.x, uv.y) * if f.sense { 1.0 } else { -1.0 };
        let d = t.cross(&n).try_normalize(tol::DEGENERATE)?;
        let probe = p + d * (0.01 * scale);
        let q = foot(f, &probe, Some(uv));
        let d = if in_face(&polys[k], &f.surf, q, near) == InFace::Outside {
            -d
        } else {
            d
        };
        Some((d, n, uv))
    };
    // The balls along the spine.
    let lo = t0 - PAST * span;
    let hi = t1 + PAST * span;
    let mut ts = Vec::with_capacity(STATIONS + 1);
    let mut balls: Vec<Ball> = Vec::with_capacity(STATIONS + 1);
    let mut guess: Option<([P2; 2], f64)> = None;
    // From the middle out, so the first guess is where the faces surely are.
    let order: Vec<usize> = {
        let mid = STATIONS / 2;
        let mut v = vec![mid];
        for i in 1..=mid {
            if mid + i <= STATIONS {
                v.push(mid + i);
            }
            if i <= mid {
                v.push(mid - i);
            }
        }
        v
    };
    let mut solved: Vec<Option<(f64, Ball)>> = vec![None; STATIONS + 1];
    for &i in &order {
        let t = lo + (hi - lo) * i as f64 / STATIONS as f64;
        let (p, tan) = spine.at(t);
        // The neighbour solved last, else a guess from the wedge.
        let from = if i > STATIONS / 2 {
            solved[i - 1].as_ref()
        } else if i < STATIONS / 2 {
            solved[i + 1].as_ref()
        } else {
            None
        };
        let start = match from {
            Some((r, b)) => Some(([b.q[0].uv, b.q[1].uv], *r)),
            None => guess,
        };
        let (Some((d1, n1, uv1)), Some((d2, n2, uv2))) =
            (into(0, &p, &tan, None), into(1, &p, &tan, None))
        else {
            continue;
        };
        // The ball sits between the faces: on the side of each that faces the other's way on.
        let side = [n1.dot(&d2).signum(), n2.dot(&d1).signum()];
        let alpha = d1.dot(&d2).clamp(-1.0, 1.0).acos();
        if alpha < 1e-3 || alpha > PI - 1e-3 {
            continue;
        }
        let r_of_width = |w: f64| w / (2.0 * (alpha / 2.0).cos());
        let r0 = match spec.size {
            BlendSize::Radius(r) => r,
            BlendSize::Width(w) => r_of_width(w),
        };
        let (g, r_start) = match start {
            Some((g, r)) => (g, r),
            None => {
                let a = r0 / (alpha / 2.0).tan();
                (
                    [
                        foot(faces[0], &(p + d1 * a), Some(uv1)),
                        foot(faces[1], &(p + d2 * a), Some(uv2)),
                    ],
                    r0,
                )
            }
        };
        let solve = |r: f64, g: [P2; 2]| {
            ball_in_plane(faces, [side[0] * r, side[1] * r], &p, &tan, g, false)
        };
        let ball = match spec.size {
            BlendSize::Radius(r) => solve(r, g).map(|b| (r, b)),
            BlendSize::Width(w) => {
                // The radius whose contacts lie the width apart (secant steps).
                let chord = |b: &Ball| (b.q[0].p - b.q[1].p).norm();
                let mut r_a = r_start;
                let Some(mut b_a) = solve(r_a, g) else {
                    continue;
                };
                let mut c_a = chord(&b_a) - w;
                let mut r_b = r_a * w / chord(&b_a).max(tol::DEGENERATE);
                let mut found = None;
                for _ in 0..30 {
                    let Some(b_b) = solve(r_b, [b_a.q[0].uv, b_a.q[1].uv]) else {
                        break;
                    };
                    let c_b = chord(&b_b) - w;
                    if c_b.abs() < 1e-9 * scale {
                        found = Some((r_b, b_b));
                        break;
                    }
                    let next = if (c_b - c_a).abs() > tol::TINY {
                        r_b - c_b * (r_b - r_a) / (c_b - c_a)
                    } else {
                        r_b * w / (c_b + w)
                    };
                    (r_a, c_a, b_a) = (r_b, c_b, b_b);
                    r_b = next.max(0.05 * r0);
                }
                found
            }
        };
        let Some((r, b)) = ball else { continue };
        guess = Some(([b.q[0].uv, b.q[1].uv], r));
        solved[i] = Some((r, b));
    }
    for (i, s) in solved.into_iter().enumerate() {
        if let Some((_, b)) = s {
            ts.push(lo + (hi - lo) * i as f64 / STATIONS as f64);
            balls.push(b);
        }
    }
    if balls.len() < 4 {
        return Err(KernelError::InvalidResult {
            op: "face blend",
            why: "no ball fits between the faces".into(),
        });
    }
    let strip = nurbs_strip(&balls, &ts, false, &[]).ok_or_else(|| KernelError::InvalidResult {
        op: "face blend",
        why: "the blend surface can't be built".into(),
    })?;
    let Surface::Nurbs(surf) = &strip.surf else {
        return Err(KernelError::Unsupported("face blends of exact strips"));
    };
    let (ta, tb) = (ts[0], ts[ts.len() - 1]);
    // Where each rail runs on its face.
    let on_face = |k: usize, t: f64| -> bool {
        let p = strip.rails[k].point(t);
        let f = faces[k];
        let uv = foot(f, &p, None);
        (f.surf.eval(uv.x, uv.y) - p).norm() < 1e-5 * scale
            && in_face(&polys[k], &f.surf, uv, near) != InFace::Outside
    };
    let mut cuts = [(0.0, 0.0); 2];
    for (k, cut) in cuts.iter_mut().enumerate() {
        let n = 400;
        let at = |i: usize| ta + (tb - ta) * i as f64 / n as f64;
        let inside: Vec<bool> = (0..=n).map(|i| on_face(k, at(i))).collect();
        let first = inside
            .iter()
            .position(|x| *x)
            .ok_or_else(|| invalid("The blend doesn't touch a face"))?;
        let last = inside.iter().rposition(|x| *x).unwrap_or(first);
        let edge_of = |out: f64, inn: f64| {
            let (mut a, mut z) = (out, inn);
            for _ in 0..60 {
                let m = 0.5 * (a + z);
                if on_face(k, m) {
                    z = m;
                } else {
                    a = m;
                }
            }
            z
        };
        let s0 = if first == 0 {
            at(0)
        } else {
            edge_of(at(first - 1), at(first))
        };
        let s1 = if last == n {
            at(n)
        } else {
            edge_of(at(last + 1), at(last))
        };
        *cut = (s0, s1);
    }
    // The walls: each split along its rail, what lies between the rail and the edge gone.
    let mut walls: Vec<(Body, Vec<usize>, BodyId)> = Vec::new();
    let mut kept: Vec<Vec<usize>> = Vec::new();
    for (k, (mut b, fi, ei, id)) in [(one, f1, e1, b1), (two, f2, e2, b2)]
        .into_iter()
        .enumerate()
    {
        if k == 1 && b1 == b2 {
            break;
        }
        let mut from: Vec<usize> = (0..b.faces.len()).collect();
        let n_edges = b.edges.len();
        let pad = 0.02 * (tb - ta);
        let (c0, c1) = cuts[k];
        let pieces = super::curve_split::split_face_curve(
            &mut b,
            fi,
            &strip.rails[k],
            (c0 - pad).max(ta),
            (c1 + pad).min(tb),
        )
        .ok_or_else(|| invalid("A wall can't be trimmed to the blend"))?;
        from.extend(pieces[1..].iter().map(|_| fi));
        // The new edge (the rail's cut) is the last one added.
        let rail_edges: Vec<usize> = (n_edges..b.edges.len())
            .filter(|&e| matches!(b.edges[e].curve, Curve::Nurbs(_)) && !b.edges[e].degenerate)
            .filter(|&e| {
                let m = b.edges[e].point(0.5 * (b.edges[e].t[0] + b.edges[e].t[1]));
                let s = strip.rails[k].closest(&m, ta, tb);
                (strip.rails[k].point(s) - m).norm() < 1e-6 * scale
            })
            .collect();
        // The piece along the spine, and everything reached from it but across the rail.
        let ef = b.edge_faces();
        let spine_piece = pieces
            .iter()
            .copied()
            .find(|&g| b.faces[g].loops.iter().flatten().any(|c| c.edge == ei))
            .ok_or_else(|| invalid("A wall's piece along the edge can't be found"))?;
        let mut gone = vec![spine_piece];
        let mut i = 0;
        while i < gone.len() {
            let g = gone[i];
            for c in b.faces[g].loops.iter().flatten() {
                if rail_edges.contains(&c.edge) {
                    continue;
                }
                for &h in &ef[c.edge] {
                    if !gone.contains(&h) {
                        gone.push(h);
                    }
                }
            }
            i += 1;
        }
        let keep: Vec<usize> = (0..b.faces.len()).filter(|g| !gone.contains(g)).collect();
        if keep.is_empty() {
            return Err(invalid("Trimming a wall to the blend leaves nothing of it"));
        }
        // The blend's rail runs where the wall was cut.
        if let Some(&e) = rail_edges.first() {
            let t = b.edges[e].t;
            cuts[k] = (t[0].min(t[1]), t[0].max(t[1]));
        }
        kept.push(keep.iter().map(|&g| from[g]).collect());
        walls.push((b, keep, id));
    }
    // The blend face: the strip between the rails' cuts.
    let blend = blend_face(surf, cuts)?;
    // All sewn into one sheet.
    let mut out = Body::default();
    let mut history = History::default();
    let mut n = 0usize;
    for ((b, keep, id), from) in walls.iter().zip(&kept) {
        let part = super::kernel::subset(b, keep, false);
        out.append(&part);
        for &f in from {
            history.modified.push((
                FaceId(n as u64),
                InputFace {
                    body: *id,
                    face: FaceId(f as u64),
                },
            ));
            n += 1;
        }
    }
    out.append(&blend);
    history.generated.push((
        FaceId(n as u64),
        Origin::ProfileCurve {
            region: spec.source,
            curve: unsourced_curve(0, 0),
        },
    ));
    for b in &bodies {
        let input = k.get(*b)?;
        for f in 0..input.faces.len() {
            let face = InputFace {
                body: *b,
                face: FaceId(f as u64),
            };
            if !history.modified.iter().any(|(_, x)| *x == face) {
                history.deleted.push(face);
            }
        }
    }
    let out = super::surfacing::sew_faces(out, 1e-5 * scale)?;
    let id = k.insert(out);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

/// An edge of face `fa` of `a` and one of face `fb` of `b` along the same curve (ends and
/// middle within a hair).
fn common_edge(a: &Body, fa: usize, b: &Body, fb: usize) -> Option<(usize, usize)> {
    let ends = |body: &Body, e: usize| {
        let ed = &body.edges[e];
        (
            ed.point(ed.t[0]),
            ed.point(ed.t[1]),
            ed.point(0.5 * (ed.t[0] + ed.t[1])),
        )
    };
    let tol = 1e-5 * (1.0 + a.face_bbox(fa).diagonal());
    for ca in a.faces[fa].loops.iter().flatten() {
        if a.edges[ca.edge].degenerate {
            continue;
        }
        let (p0, p1, pm) = ends(a, ca.edge);
        for cb in b.faces[fb].loops.iter().flatten() {
            if b.edges[cb.edge].degenerate {
                continue;
            }
            let (q0, q1, qm) = ends(b, cb.edge);
            let same = ((p0 - q0).norm() < tol && (p1 - q1).norm() < tol)
                || ((p0 - q1).norm() < tol && (p1 - q0).norm() < tol);
            let ed = &b.edges[cb.edge];
            let mid_on = {
                let s = ed.curve.closest(&pm, ed.t[0], ed.t[1]);
                (ed.point(s) - pm).norm() < tol
            };
            if same && (mid_on || (pm - qm).norm() < tol) {
                return Some((ca.edge, cb.edge));
            }
        }
    }
    None
}

/// The blend face on the strip (u the spine's parameter, v from the first rail to the second)
/// between the rails' cuts `cuts[k]`: its sides the rails, its ends straight across (u, v).
fn blend_face(surf: &Arc<super::nurbs::NurbsSurface>, cuts: [(f64, f64); 2]) -> Result<Body> {
    let s = Surface::Nurbs(surf.clone());
    let mut b = Body::default();
    let corner = |u: f64, v: f64| s.eval(u, v);
    let (a0, b0) = cuts[0];
    let (a1, b1) = cuts[1];
    let v00 = b.add_vertex(corner(a0, 0.0));
    let v10 = b.add_vertex(corner(b0, 0.0));
    let v11 = b.add_vertex(corner(b1, 1.0));
    let v01 = b.add_vertex(corner(a1, 1.0));
    let line2 = |o: P2, d: V2| Curve::Line {
        o: P3::new(o.x, o.y, 0.0),
        d: V3::new(d.x, d.y, 0.0),
    };
    let rail = |at_v1: bool| -> Curve {
        Curve::Nurbs(Arc::new(super::loft_build::iso(surf, true, at_v1)))
    };
    // An end: the surface along the straight line in (u, v) from (ua, 0) to (ub, 1), its 3D
    // curve interpolated through points of it, parametrized as the line (v).
    let end = |ua: f64, ub: f64| -> (Curve, Curve) {
        let m = 32;
        let vs: Vec<f64> = (0..=m).map(|i| i as f64 / m as f64).collect();
        let pts: Vec<P3> = vs.iter().map(|&v| s.eval(ua + (ub - ua) * v, v)).collect();
        let c = NurbsCurve::interpolate(&pts, &vs, 3, None, None);
        (
            Curve::Nurbs(Arc::new(c)),
            line2(P2::new(ua, 0.0), V2::new(ub - ua, 1.0)),
        )
    };
    let e0 = b.add_edge(rail(false), a0, b0, v00, v10);
    let (c1, p1) = end(b0, b1);
    let e1 = b.add_edge(c1, 0.0, 1.0, v10, v11);
    let e2 = b.add_edge(rail(true), a1, b1, v01, v11);
    let (c3, p3) = end(a0, a1);
    let e3 = b.add_edge(c3, 0.0, 1.0, v00, v01);
    let lp = vec![
        Coedge {
            edge: e0,
            fwd: true,
            pc: line2(P2::new(0.0, 0.0), V2::new(1.0, 0.0)),
        },
        Coedge {
            edge: e1,
            fwd: true,
            pc: p1,
        },
        Coedge {
            edge: e2,
            fwd: false,
            pc: line2(P2::new(0.0, 1.0), V2::new(1.0, 0.0)),
        },
        Coedge {
            edge: e3,
            fwd: false,
            pc: p3,
        },
    ];
    b.faces.push(Face {
        surf: s,
        sense: true,
        loops: vec![lp],
    });
    super::build::fix_senses(&mut b, 0..1);
    b.shells.push(Shell {
        faces: vec![0],
        closed: false,
    });
    Ok(b)
}

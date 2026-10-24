//! Move face where the faces round the moved ones can't just be extended to meet them (#22: a
//! face moved off a coplanar neighbour, a wall's end moved with the fillets round it): each
//! moved face is swept to where it goes ([`super::offset::slab::face_prism`]) and the sweep is
//! added to the body where it moves out, or cut from it where it moves in. Booleans that merge
//! faces on one surface then extend the neighbours, and the sweep's sides fill the gaps that
//! nothing extends over (the step left where a face moves off its coplanar neighbour).
//!
//! The fillets that Reapply fillet would put back ([`Refillet`]s) go along with the faces they
//! join when their other faces run along the move (a fillet between a moved wall end and a side
//! the move slides along stays the same fillet, moved), and so do the corners between two such
//! fillets. A rotation, or a fillet whose other face the move doesn't slide along, isn't swept.

use super::kernel::NativeKernel;
use super::math::*;
use super::surface::Surface;
use super::tol;
use super::topo::Body;
use crate::*;

fn refused(why: &str) -> KernelError {
    KernelError::InvalidResult {
        op: "move face",
        why: format!("the faces can't be moved there: {why}"),
    }
}

/// Face `fi`'s outward normal at a point inside it, and that point.
fn outward(b: &Body, fi: usize) -> Option<(P3, V3)> {
    let p = super::smb::face_centroids(&super::kernel::subset(b, &[fi], false))
        .first()
        .copied()?;
    let f = &b.faces[fi];
    let (u, v) = f.surf.project(&p, None);
    let n = f.surf.normal(u, v).try_normalize(tol::DEGENERATE)?;
    Some((f.surf.eval(u, v), if f.sense { n } else { -n }))
}

/// Whether face `fi` runs along `v` everywhere (its normal square to it at its middle and along
/// its edges): sliding it along `v` leaves it on its own surface.
fn runs_along(b: &Body, fi: usize, v: &V3) -> bool {
    let d = v.normalize();
    let f = &b.faces[fi];
    let square = |p: &P3| {
        let (u, w) = f.surf.project(p, None);
        f.surf
            .normal(u, w)
            .try_normalize(tol::DEGENERATE)
            .is_none_or(|n| n.dot(&d).abs() < tol::ANGULAR_POINT)
    };
    outward(b, fi).is_some_and(|(p, _)| square(&p))
        && f.loops.iter().flatten().all(|c| {
            let e = &b.edges[c.edge];
            e.degenerate
                || [0.0, 0.5, 1.0]
                    .iter()
                    .all(|s| square(&e.point(e.t[0] + (e.t[1] - e.t[0]) * s)))
        })
}

/// What each face of `b` sweeps by: the faces moved by `how`, the fillets of `fillets` whose
/// other faces the move slides along, and the corners between two of those. `None` when a face
/// can't be swept (a rotation, an offset of a curved face).
fn sweeps(
    b: &Body,
    faces: &[FaceId],
    how: &FaceMove,
    fillets: &[Refillet],
) -> Option<Vec<(usize, V3)>> {
    let mut out: Vec<(usize, V3)> = Vec::new();
    for f in faces {
        let fi = f.0 as usize;
        let v = match how {
            FaceMove::Motion(m) => {
                if (m.linear - nalgebra::Matrix3::identity()).norm() > tol::ANGULAR_POINT {
                    return None;
                }
                m.translation
            }
            FaceMove::Offset(d) => {
                if !matches!(b.faces.get(fi)?.surf, Surface::Plane { .. }) {
                    return None;
                }
                outward(b, fi)?.1 * *d
            }
        };
        if v.norm() < tol::POINT {
            return None;
        }
        out.push((fi, v));
    }
    let moved = out.clone();
    let along = |fi: usize| moved.iter().find(|(f, _)| *f == fi).map(|(_, v)| *v);
    let mut swept_fillets: Vec<usize> = Vec::new();
    for r in fillets {
        let vs: Vec<V3> = r
            .tangent
            .iter()
            .filter_map(|t| along(t.0 as usize))
            .collect();
        let Some(v) = vs.first().copied() else {
            continue;
        };
        if vs.iter().any(|w| (w - v).norm() > tol::POINT) {
            return None;
        }
        let others = r
            .tangent
            .iter()
            .map(|t| t.0 as usize)
            .filter(|t| along(*t).is_none());
        // Its other faces: ones the move slides along, or corners between fillets.
        let slides = others
            .filter(|&t| !fillets.iter().any(|g| g.face.0 as usize == t))
            .filter(|&t| {
                !matches!(
                    b.faces[t].surf,
                    Surface::Sphere { .. } | Surface::Torus { .. }
                )
            })
            .all(|t| runs_along(b, t, &v));
        if !slides {
            return None;
        }
        out.push((r.face.0 as usize, v));
        swept_fillets.push(r.face.0 as usize);
    }
    // A corner between two swept fillets goes with them.
    let ef = b.edge_faces();
    for (fi, face) in b.faces.iter().enumerate() {
        if out.iter().any(|(f, _)| *f == fi) {
            continue;
        }
        let mut next: Vec<usize> = Vec::new();
        for c in face.loops.iter().flatten() {
            for &g in &ef[c.edge] {
                if g != fi && swept_fillets.contains(&g) && !next.contains(&g) {
                    next.push(g);
                }
            }
        }
        if next.len() >= 2 {
            let vs: Vec<V3> = next
                .iter()
                .filter_map(|g| out.iter().find(|(f, _)| f == g).map(|(_, v)| *v))
                .collect();
            if vs.iter().all(|w| (w - vs[0]).norm() <= tol::POINT) {
                out.push((fi, vs[0]));
            }
        }
    }
    Some(out)
}

/// [`Kernel::move_faces_swept`]: see the module docs.
pub fn move_faces_swept(
    k: &mut NativeKernel,
    body: BodyId,
    faces: &[FaceId],
    how: &FaceMove,
    fillets: &[Refillet],
) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
    if let Some(f) = faces.iter().find(|f| f.0 as usize >= b.faces.len()) {
        return Err(KernelError::InvalidParameter(format!("unknown {f:?}")));
    }
    let sweeps =
        sweeps(&b, faces, how, fillets).ok_or_else(|| refused("only straight moves are swept"))?;
    let mut cur = b.clone();
    // Each face of `cur`: the faces of `b` it continues (none: a side of a sweep).
    let mut sources: Vec<Vec<usize>> = (0..b.faces.len()).map(|f| vec![f]).collect();
    // Whether it is (part of) a moved face where it landed.
    let mut landed: Vec<bool> = vec![false; b.faces.len()];
    // Out first, so a sweep in doesn't cut what one out is about to add to.
    let mut order: Vec<(usize, V3, bool)> = Vec::new();
    for &(fi, v) in &sweeps {
        let (_, n) = outward(&b, fi).ok_or_else(|| refused("a face with no area"))?;
        let dot = n.dot(&v.normalize());
        if dot.abs() < tol::ANGULAR_POINT {
            // Sliding along itself: nothing to sweep (its neighbours' sweeps carry it).
            continue;
        }
        order.push((fi, v, dot > 0.0));
    }
    order.sort_by_key(|o| !o.2);
    let swept: Vec<usize> = order.iter().map(|o| o.0).collect();
    for (fi, v, out) in order {
        let prism = super::offset::slab::face_prism(&b, fi, v)?;
        // The prism's face where the face lands: the one around its middle moved.
        let (mid, _) = outward(&b, fi).ok_or_else(|| refused("a face with no area"))?;
        let lands = (0..prism.faces.len()).find(|&g| {
            outward(&prism, g).is_some_and(|(p, _)| {
                let f = &prism.faces[g];
                let target = mid + v;
                f.surf.distance(&target) < 1e-6 * (1.0 + v.norm())
                    && (p - target).norm() < (v.norm() + 1.0) * 1e3
            })
        });
        let op = if out { BoolOp::Union } else { BoolOp::Subtract };
        let r = super::boolean::boolean_bodies_flush(&cur, &prism, op)?;
        landed = r
            .from
            .iter()
            .map(|srcs| {
                srcs.iter().any(|&(o, g)| match o {
                    0 => landed[g],
                    _ => Some(g) == lands,
                })
            })
            .collect();
        sources = r
            .from
            .iter()
            .map(|srcs| {
                let mut s: Vec<usize> = Vec::new();
                for &(o, g) in srcs {
                    let add: Vec<usize> = match o {
                        0 => sources[g].clone(),
                        _ if Some(g) == lands => vec![fi],
                        _ => Vec::new(),
                    };
                    for a in add {
                        if !s.contains(&a) {
                            s.push(a);
                        }
                    }
                }
                s
            })
            .collect();
        cur = r.body;
    }
    if cur.solids.len() != 1 {
        return Err(refused("the body would come apart"));
    }
    // A swept face is now only where it landed: where it was is no longer on the boundary.
    let polys: Vec<_> = cur
        .faces
        .iter()
        .map(|f| super::classify::face_polygons(&cur, f))
        .collect();
    for &f in &swept {
        let (p, _) = outward(&b, f).ok_or_else(|| refused("a face with no area"))?;
        if super::classify::classify_point(&cur, &polys, &p, 1e-6) == PointClass::OnBoundary {
            return Err(refused("part of a moved face is left behind"));
        }
    }
    // A face it merged with elsewhere (a coplanar neighbour) doesn't continue it.
    for (srcs, l) in sources.iter_mut().zip(&landed) {
        if !l {
            srcs.retain(|f| !swept.contains(f));
        }
    }
    let mut history = History::default();
    for (g, srcs) in sources.iter().enumerate() {
        for &s in srcs {
            history.modified.push((
                FaceId(g as u64),
                InputFace {
                    body,
                    face: FaceId(s as u64),
                },
            ));
        }
        if srcs.is_empty() {
            // A side of a sweep: named after the edge of the body it was swept from.
            let edge = sweeps.iter().find_map(|&(fi, v)| {
                b.faces[fi]
                    .loops
                    .iter()
                    .flatten()
                    .map(|c| c.edge)
                    .find(|&e| {
                        let ed = &b.edges[e];
                        let p = ed.point(0.5 * (ed.t[0] + ed.t[1]));
                        let s = &cur.faces[g].surf;
                        let on = |q: P3| s.distance(&q) < 1e-6 * (1.0 + v.norm());
                        !ed.degenerate && on(p) && on(p + v)
                    })
            });
            match edge {
                Some(e) => history.generated.push((
                    FaceId(g as u64),
                    Origin::FromEdge {
                        body,
                        edge: EdgeId(e as u64),
                    },
                )),
                None => return Err(refused("a face of the result comes from nowhere")),
            }
        }
    }
    let id = k.insert(cur);
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

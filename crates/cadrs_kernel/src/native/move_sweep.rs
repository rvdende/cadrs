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
//! fillets. A rotation, or a fillet whose other face the move doesn't slide along, isn't swept;
//! nor a face the move runs along somewhere inside it (it would sweep over itself), nor two faces
//! next to each other that move different ways (an offset top and side: the corner between their
//! sweeps would be left out).

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

/// Points inside a face, with its outward normals there.
type Inside = Vec<(P3, V3)>;

/// Face `fi`'s outward normal at the point of its surface nearest `p`, and that point.
fn outward_at(b: &Body, fi: usize, p: &P3) -> Option<(P3, V3)> {
    let f = &b.faces[fi];
    let (u, v) = f.surf.project(p, None);
    let n = f.surf.normal(u, v).try_normalize(tol::DEGENERATE)?;
    Some((f.surf.eval(u, v), if f.sense { n } else { -n }))
}

/// Points inside face `fi` (the middles of up to 32 of its mesh's triangles, on its surface)
/// with its outward normals there: unlike its area centroid, inside it whatever its shape.
fn inside(k: &mut NativeKernel, b: &Body, fi: usize) -> Result<Inside> {
    let id = k.insert(super::kernel::subset(b, &[fi], false));
    let mesh = k.tessellate(id, Tessellation::default());
    k.release(id);
    let mesh = mesh?;
    let step = mesh.indices.len().div_ceil(32).max(1);
    Ok(mesh
        .indices
        .iter()
        .step_by(step)
        .filter_map(|t| {
            let c = t
                .iter()
                .map(|&i| mesh.positions[i as usize].coords)
                .sum::<V3>()
                / 3.0;
            outward_at(b, fi, &P3::from(c))
        })
        .collect())
}

/// Face `fi`'s outward normal at a point inside it, and that point.
fn outward(k: &mut NativeKernel, b: &Body, fi: usize) -> Option<(P3, V3)> {
    inside(k, b, fi).ok()?.first().copied()
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
    f.loops.iter().flatten().all(|c| {
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
    k: &mut NativeKernel,
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
                outward(k, b, fi)?.1 * *d
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
    // Faces next to each other moving different ways leave the corner between their sweeps out.
    let ef = b.edge_faces();
    let way = |fi: usize| out.iter().find(|(f, _)| *f == fi).map(|(_, v)| *v);
    for fs in &ef {
        if let [a, c] = fs[..]
            && let (Some(va), Some(vc)) = (way(a), way(c))
            && (va - vc).norm() > tol::POINT
        {
            return None;
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
    let sweeps = sweeps(k, &b, faces, how, fillets).ok_or_else(|| {
        refused("only straight moves of faces that don't move apart from each other are swept")
    })?;
    let mut cur = b.clone();
    // Each face of `cur`: the faces of `b` it continues (none: a side of a sweep).
    let mut sources: Vec<Vec<usize>> = (0..b.faces.len()).map(|f| vec![f]).collect();
    // Whether it is (part of) a moved face where it landed.
    let mut landed: Vec<bool> = vec![false; b.faces.len()];
    // For a side of a sweep, the edge of `b` it was swept from.
    let mut swept_from: Vec<Option<usize>> = vec![None; b.faces.len()];
    // Out first, so a sweep in doesn't cut what one out is about to add to; each with points
    // inside it.
    let mut order: Vec<(usize, V3, bool, Inside)> = Vec::new();
    for &(fi, v) in &sweeps {
        let pts = inside(k, &b, fi)?;
        let dots: Vec<f64> = pts.iter().map(|(_, n)| n.dot(&v.normalize())).collect();
        if dots.is_empty() {
            return Err(refused("a face with no area"));
        }
        if dots.iter().all(|d| d.abs() < tol::ANGULAR_POINT) {
            // Sliding along itself: nothing to sweep (its neighbours' sweeps carry it).
            continue;
        }
        // The move must cross it the same way all over (else it sweeps over itself).
        let out = dots[0] > 0.0;
        if dots
            .iter()
            .any(|d| d.abs() < tol::ANGULAR_POINT || (*d > 0.0) != out)
        {
            return Err(refused("the move runs along a face it moves"));
        }
        order.push((fi, v, out, pts));
    }
    order.sort_by_key(|o| !o.2);
    let swept: Vec<(usize, Inside)> = order.iter().map(|o| (o.0, o.3.clone())).collect();
    for (fi, v, out, pts) in order {
        let (prism, edges) = super::offset::slab::face_prism(&b, fi, v, out)?;
        // The prism's face where the face lands: an end (no edge swept to it) through a point
        // inside the face, moved.
        let target = pts[0].0 + v;
        let lands = (0..prism.faces.len()).find(|&g| {
            edges[g].is_none() && prism.faces[g].surf.distance(&target) < 1e-6 * (1.0 + v.norm())
        });
        let op = if out { BoolOp::Union } else { BoolOp::Subtract };
        let r = super::boolean::boolean_bodies_flush(&cur, &prism, op)?;
        swept_from = r
            .from
            .iter()
            .map(|srcs| {
                srcs.iter().find_map(|&(o, g)| match o {
                    0 => swept_from[g],
                    _ => edges[g],
                })
            })
            .collect();
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
    for (_, pts) in &swept {
        if pts.iter().any(|(p, _)| {
            super::classify::classify_point(&cur, &polys, p, 1e-6) == PointClass::OnBoundary
        }) {
            return Err(refused("part of a moved face is left behind"));
        }
    }
    // A face it merged with elsewhere (a coplanar neighbour) doesn't continue it.
    for (srcs, l) in sources.iter_mut().zip(&landed) {
        if !l {
            srcs.retain(|f| !swept.iter().any(|(s, _)| s == f));
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
            let edge = swept_from[g];
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

#[cfg(test)]
mod tests {
    use super::*;

    fn body(s: super::super::draw::Shape) -> BodyId {
        match s {
            super::super::draw::Shape::Body(b) => b,
            _ => unreachable!(),
        }
    }

    /// The face of `id` whose outward normal at its middle is `n`.
    fn facing(k: &mut NativeKernel, id: BodyId, n: [f64; 3]) -> FaceId {
        let b = (**k.get(id).unwrap()).clone();
        let fi = (0..b.faces.len())
            .find(|&f| outward(k, &b, f).is_some_and(|(_, m)| (m - V3::from(n)).norm() < 1e-9))
            .unwrap();
        FaceId(fi as u64)
    }

    #[test]
    fn an_offset_top_is_swept_up() {
        // A 10 mm cube's top offset 2 by sweeping it: 10 × 10 × 12, its sides grown to meet it.
        let mut k = NativeKernel::new();
        let id = body(k.box_at(0.0, 0.0, 0.0, 10.0, 10.0, 10.0).unwrap());
        let top = facing(&mut k, id, [0.0, 0.0, 1.0]);
        let r = move_faces_swept(&mut k, id, &[top], &FaceMove::Offset(2.0), &[]).unwrap();
        let out = k.get(r.bodies[0]).unwrap();
        assert!((super::super::mass::body_volume(out) - 1200.0).abs() < 1e-6);
        assert_eq!(out.faces.len(), 6);
    }

    #[test]
    fn faces_next_to_each_other_moving_apart_are_not_swept() {
        // The cube's top and a side offset together: each goes along its own normal, and the
        // corner between their sweeps would be left out, so the sweep refuses.
        let mut k = NativeKernel::new();
        let id = body(k.box_at(0.0, 0.0, 0.0, 10.0, 10.0, 10.0).unwrap());
        let top = facing(&mut k, id, [0.0, 0.0, 1.0]);
        let side = facing(&mut k, id, [1.0, 0.0, 0.0]);
        let r = move_faces_swept(&mut k, id, &[top, side], &FaceMove::Offset(2.0), &[]);
        assert!(r.is_err(), "{r:?}");
        // Translated together (one vector) they sweep as one: the cube drawn out along
        // (2, 0, 2), 1000 + |v| × the cube seen along v, 2√2 × 100√2.
        let r = move_faces_swept(
            &mut k,
            id,
            &[top, side],
            &FaceMove::Motion(Motion::translation(V3::new(2.0, 0.0, 2.0))),
            &[],
        )
        .unwrap();
        let out = k.get(r.bodies[0]).unwrap();
        assert!((super::super::mass::body_volume(out) - 1400.0).abs() < 1e-6);
    }

    #[test]
    fn a_wall_the_move_runs_along_is_not_swept() {
        // A cylinder's wall moved sideways: the move runs along the wall on two lines, so it
        // would sweep over itself; refused.
        let mut k = NativeKernel::new();
        let id = body(k.pcylinder(5.0, 10.0).unwrap());
        let b = (**k.get(id).unwrap()).clone();
        let wall = (0..b.faces.len())
            .find(|&f| matches!(b.faces[f].surf, Surface::Cylinder { .. }))
            .unwrap();
        let r = move_faces_swept(
            &mut k,
            id,
            &[FaceId(wall as u64)],
            &FaceMove::Motion(Motion::translation(V3::new(2.0, 0.0, 0.0))),
            &[],
        );
        let e = r.expect_err("refused").to_string();
        assert!(e.contains("runs along"), "{e}");
    }
}

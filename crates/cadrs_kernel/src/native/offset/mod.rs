//! Offsets of bodies and of planar wires, by stages:
//!
//! 1. **Surfaces** ([`sharp::offset_surface`]): each face's surface moved along its normal by
//!    its distance (planes, quadrics, tori, extrusions, revolutions, B-splines).
//! 2. **Sharp offsets** ([`sharp`]): the moved faces meeting as they did (a local operation that
//!    keeps the topology), prisms by their profile's 2D offset, crossed faces collapsed.
//! 3. **Rounded offsets** ([`round`]): every face's slab ([`slab`], the face swept along its
//!    normal), tubes round edges and balls round vertices, united (outward) or subtracted
//!    (inward); shells and thick solids from those.
//! 4. **Planar wires** ([`wire`]): exact 2D offsets, trimmed where they cross themselves; with
//!    them the insets of prisms with curved sides ([`prism`]), the outline's offset extruded.
//!
//! [`offset`] is `Kernel::offset`: sharp or rounded by the spec, with the faces' history.

pub mod prism;
pub mod round;
pub mod sharp;
pub mod slab;
pub mod wire;

use crate::native::kernel::NativeKernel;
use crate::native::surface::Surface;
use crate::native::tol;
use crate::*;

/// Faces moved until they run into each other (two holes widened until they merge): the body
/// less what the faces moved into it sweep along their normals ([`slab::face_slab`]), with what
/// those moved out of it sweep, the material between the faces' places. That is the move where
/// each moved face meets its neighbours square, its edges swept along its normals staying on
/// their surfaces (as a hole's wall meets the plate's faces), so that the neighbours are trimmed
/// or extended along their own surfaces as the sharp offset does it; else None, as when the
/// result isn't sound or the body falls apart.
fn moved_by_slabs(
    b: &crate::native::topo::Body,
    faces: &[(FaceId, f64)],
) -> Option<crate::native::topo::Body> {
    let ef = b.edge_faces();
    // (The part's own size, however small: no floor at a millimetre.)
    let size = b.bbox().diagonal().max(tol::POINT);
    let near = tol::REL_POINT * size;
    for &(f, d) in faces {
        let fi = f.0 as usize;
        let face = b.faces.get(fi)?;
        if d == 0.0 {
            continue;
        }
        let sign = if face.sense { 1.0 } else { -1.0 };
        for c in face.loops.iter().flatten() {
            let e = &b.edges[c.edge];
            if e.degenerate {
                continue;
            }
            // (A seam has the face on both sides: nothing to meet.)
            let Some(&other) = ef[c.edge].iter().find(|&&g| g != fi) else {
                continue;
            };
            for k in 0..=4 {
                let p = e.curve.point(e.t[0] + (e.t[1] - e.t[0]) * k as f64 / 4.0);
                let (u, v) = face.surf.project(&p, None);
                let n = face.surf.normal(u, v).try_normalize(tol::TINY)? * sign;
                for s in [0.5, 1.0] {
                    if b.faces[other].surf.distance(&(p + n * d * s)) > near {
                        return None;
                    }
                }
            }
        }
    }
    let mut out = b.clone();
    let mut moved: Vec<(FaceId, f64)> = faces.iter().copied().filter(|(_, d)| *d != 0.0).collect();
    moved.sort_by(|x, y| x.1.total_cmp(&y.1));
    for (f, d) in moved {
        let slab = slab::face_slab(b, f.0 as usize, d).ok()?;
        let op = if d < 0.0 {
            BoolOp::Subtract
        } else {
            BoolOp::Union
        };
        out = crate::native::boolean::boolean_bodies(&out, &slab, op)
            .ok()?
            .body;
    }
    // Faces the move brought together merged (a slot closed by its wall: the tops either side
    // of it one face again, the sliver left where the walls met gone); the union keeps faces of
    // one input apart, as a union must.
    let from: Vec<Vec<(usize, usize)>> = (0..out.faces.len()).map(|i| vec![(0, i)]).collect();
    crate::native::unify::unify_faces(&mut out, from, &|_, _| true);
    // (One piece as it was: a move that breaks the body apart is no Move face.)
    (out.solids.len() == b.solids.len()
        && out
            .check_with(crate::native::topo::CheckLevel::Full)
            .is_empty())
    .then_some(out)
}

/// Whether a flat face of the offset body has shrunk to nothing (a step's riser when its tread
/// is raised level with the one above): the sharp offset keeps it as a sliver of no area, which
/// the slabs' union with its faces merged ([`moved_by_slabs`]) takes away.
fn collapses_a_face(out: &crate::native::topo::Body) -> bool {
    let size = out.bbox().diagonal().max(tol::POINT);
    let small = tol::REL_POINT * size * size;
    out.faces.iter().any(|f| {
        matches!(f.surf, Surface::Plane { .. })
            && crate::native::classify::face_polygons(out, f)
                .first()
                .is_some_and(|l| crate::native::math::polygon_area(l).abs() <= small)
    })
}

/// Whether a moved flat face passes a flat face facing the same way that shares a neighbour
/// with it (a step's tread raised past the one above: the riser between them would turn inside
/// out). Faces moved into each other (a slot's walls pushed across it) are not this: they face
/// each other, and their slabs' union is the move.
fn passes_a_step(b: &crate::native::topo::Body, dist: &[f64]) -> bool {
    let near = tol::REL_POINT * b.bbox().diagonal().max(tol::POINT);
    let ef = b.edge_faces();
    let plane = |i: usize| match &b.faces[i].surf {
        Surface::Plane { f } => Some((f.o, f.z * if b.faces[i].sense { 1.0 } else { -1.0 })),
        _ => None,
    };
    let neighbours = |i: usize| -> Vec<usize> {
        let mut n: Vec<usize> = b.faces[i]
            .loops
            .iter()
            .flatten()
            .flat_map(|c| ef[c.edge].iter().copied())
            .filter(|&g| g != i)
            .collect();
        n.sort_unstable();
        n.dedup();
        n
    };
    (0..b.faces.len()).any(|i| {
        let d = dist.get(i).copied().unwrap_or(0.0);
        let Some((o, n)) = (d != 0.0).then(|| plane(i)).flatten() else {
            return false;
        };
        let mine = neighbours(i);
        (0..b.faces.len()).any(|j| {
            j != i
                && dist.get(j).copied().unwrap_or(0.0) == 0.0
                && plane(j).is_some_and(|(q, m)| {
                    // Same way, beside it, and the move takes it from one side of the other's
                    // plane through to the other.
                    let (before, after) = ((o - q).dot(&m), (o + n * d - q).dot(&m));
                    n.dot(&m) > 1.0 - tol::ANGULAR_POINT
                        && before.abs() > near
                        && after.abs() > near
                        && before.signum() != after.signum()
                        && neighbours(j).iter().any(|g| mine.contains(g))
                })
        })
    })
}

/// Whether a moved flat face lands on another flat face of `b` facing it (a slot's wall pushed
/// across the slot onto the far wall): the sharp offset leaves the two back to back, a body
/// whose check passes but which has the closed slot's walls inside it; the slabs' union
/// ([`moved_by_slabs`]) merges them away.
fn lands_on_opposite(b: &crate::native::topo::Body, dist: &[f64]) -> bool {
    let near = tol::REL_POINT * b.bbox().diagonal().max(tol::POINT);
    let plane = |i: usize| match &b.faces[i].surf {
        Surface::Plane { f } => Some((f.o, f.z * if b.faces[i].sense { 1.0 } else { -1.0 })),
        _ => None,
    };
    (0..b.faces.len()).any(|i| {
        let d = dist.get(i).copied().unwrap_or(0.0);
        let Some((o, n)) = (d != 0.0).then(|| plane(i)).flatten() else {
            return false;
        };
        let o = o + n * d;
        (0..b.faces.len()).any(|j| {
            j != i
                && plane(j).is_some_and(|(q, m)| {
                    // (Where the other face is moved too, its own place after the move.)
                    let q = q + m * dist.get(j).copied().unwrap_or(0.0);
                    n.dot(&m) < -1.0 + tol::REL && (q - o).dot(&n).abs() <= near
                })
        })
    })
}

/// `b` without the faces that moving by `dist` takes past their own radius (a cylinder, a
/// sphere or a torus tube whose offset has none left: [`sharp::offset_surface`]), the faces
/// round them run on to meet ([`crate::native::defeature`]); with each of its faces' sources
/// in `b`. None when no face vanishes, all do, or the faces round them don't close.
pub(crate) fn without_vanishing(
    b: &crate::native::topo::Body,
    dist: &[f64],
) -> Option<(crate::native::topo::Body, Vec<Vec<usize>>)> {
    let gone: Vec<usize> = (0..b.faces.len())
        .filter(|&f| {
            let face = &b.faces[f];
            dist[f] != 0.0
                && matches!(
                    face.surf,
                    Surface::Cylinder { .. } | Surface::Sphere { .. } | Surface::Torus { .. }
                )
                && sharp::offset_surface(&face.surf, face.sense, dist[f]).is_none()
        })
        .collect();
    if gone.is_empty() || gone.len() == b.faces.len() {
        return None;
    }
    match crate::native::defeature::delete_faces(b, &gone) {
        Ok(r) => Some(r),
        Err(e) => {
            if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
                eprintln!("the vanishing faces {gone:?} can't be deleted: {e}");
            }
            None
        }
    }
}

pub fn offset(k: &mut NativeKernel, body: BodyId, spec: &OffsetSpec) -> Result<OpResult> {
    let b = std::sync::Arc::clone(k.get(body)?);
    let mut dist = vec![spec.distance; b.faces.len()];
    for (f, d) in &spec.faces {
        if let Some(x) = dist.get_mut(f.0 as usize) {
            *x = *d;
        }
    }
    // Move face (some faces moved, the rest kept): a move that would change the body's
    // topology isn't one the sharp offset makes (a hole's wall moved onto or past its axis, a
    // face run into one it doesn't border): refused cleanly rather than a wrong body.
    let moving = spec.distance == 0.0 && spec.faces.iter().any(|(_, d)| *d != 0.0);
    let too_far =
        |why: &str| KernelError::InvalidParameter(format!("The faces can't move that far: {why}"));
    if moving
        && spec.faces.iter().any(|(f, d)| {
            b.faces.get(f.0 as usize).is_some_and(|face| {
                *d != 0.0
                    && matches!(
                        face.surf,
                        Surface::Cylinder { .. }
                            | Surface::Cone { .. }
                            | Surface::Sphere { .. }
                            | Surface::Torus { .. }
                    )
                    && sharp::offset_surface(&face.surf, face.sense, *d).is_none()
            })
        })
    {
        return Err(too_far("a face would shrink to nothing"));
    }
    let first = (|| -> Result<crate::native::topo::Body> {
        Ok(if spec.sharp || dist.iter().all(|d| *d == 0.0) {
            let out = sharp::offset_sharp(&b, &dist)?;
            if moving && passes_a_step(&b, &dist) {
                return Err(too_far("a face beside them would turn inside out"));
            }
            if moving
                && (lands_on_opposite(&b, &dist)
                    || collapses_a_face(&out)
                    || !out
                        .check_with(crate::native::topo::CheckLevel::Full)
                        .is_empty())
            {
                // (Faces run into each other: two holes widened until they merge.)
                moved_by_slabs(&b, &spec.faces)
                    .ok_or_else(|| too_far("a face would run into another face"))?
            } else {
                out
            }
        } else if dist.iter().all(|d| *d <= 0.0) {
            round::inset_rounded(&b, &dist)?
        } else {
            round::offset_rounded(&b, &dist)?
        })
    })();
    // Faces the distance takes past their own radius (a round of radius 2 moved in 3) have no
    // offset: they are no part of the result's boundary, which the faces round them make,
    // run on until they meet. So the body without them ([`without_vanishing`]) is offset.
    let (b, dist, out, back) = match first {
        Ok(out) => (b, dist, out, None),
        Err(e) => {
            let Some((b2, from)) = (!moving).then(|| without_vanishing(&b, &dist)).flatten() else {
                return Err(e);
            };
            let d2: Vec<f64> = from.iter().map(|s| dist[s[0]]).collect();
            let out = if spec.sharp {
                sharp::offset_sharp(&b2, &d2)
            } else if d2.iter().all(|d| *d <= 0.0) {
                round::inset_rounded(&b2, &d2)
            } else {
                round::offset_rounded(&b2, &d2)
            }
            .map_err(|again| {
                if crate::native::env_flag!("CADRS_SHELL_DEBUG") {
                    eprintln!("offset without the vanishing faces: {again}");
                }
                e
            })?;
            let n = b.faces.len();
            (std::sync::Arc::new(b2), d2, out, Some((from, n)))
        }
    };
    let v0 = crate::native::mass::body_volume(&b);
    let v1 = crate::native::mass::body_volume(&out);
    let grow = dist.iter().any(|d| *d > 0.0);
    let shrink = dist.iter().any(|d| *d < 0.0);
    // (A volume's round-off: a cubic millimetre's for a part of a millimetre, less below.)
    let within = tol::ROUNDOFF * tol::small(b.bbox().diagonal()).powi(3);
    if (grow && !shrink && v1 < v0 - within) || (shrink && !grow && v1 > v0 + within) || v1 <= 0.0 {
        return Err(KernelError::TooLarge {
            op: "offset",
            why: "the offset turned the body inside out".into(),
        });
    }
    // History: each face the offset of input faces (modified) or a round of an edge or vertex
    // (generated); an input face with no offset left (vanished, or inside the rounds) deleted.
    let srcs = round::offset_sources(
        &b,
        body,
        &out,
        &dist,
        !(spec.sharp || dist.iter().all(|d| *d == 0.0)),
    );
    let id = k.insert(out);
    let mut history = History::default();
    // (The input's own faces, where the body offset was the one without its vanishing faces.)
    let inputs = |i: usize| -> Vec<usize> {
        match &back {
            Some((from, _)) => from[i].clone(),
            None => vec![i],
        }
    };
    let mut kept = vec![false; back.as_ref().map_or(b.faces.len(), |x| x.1)];
    for (f, src) in srcs.iter().enumerate() {
        for s in src {
            match s {
                crate::native::blend::FaceSrc::Input(i) => {
                    for i in inputs(*i) {
                        kept[i] = true;
                        history.modified.push((
                            FaceId(f as u64),
                            InputFace {
                                body,
                                face: FaceId(i as u64),
                            },
                        ));
                    }
                }
                crate::native::blend::FaceSrc::New(o) => {
                    history.generated.push((FaceId(f as u64), *o))
                }
            }
        }
    }
    history.deleted = (0..kept.len())
        .filter(|&i| !kept[i])
        .map(|i| InputFace {
            body,
            face: FaceId(i as u64),
        })
        .collect();
    Ok(OpResult {
        bodies: vec![id],
        history,
    })
}

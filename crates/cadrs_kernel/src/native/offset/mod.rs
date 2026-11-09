//! Offsets of bodies and of planar wires, by stages:
//!
//! 1. **Surfaces** ([`sharp::offset_surface`]): each face's surface moved along its normal by
//!    its distance (planes, quadrics, tori, extrusions, revolutions, B-splines).
//! 2. **Sharp offsets** ([`sharp`]): the moved faces meeting as they did (a local operation that
//!    keeps the topology), prisms by their profile's 2D offset, crossed faces collapsed.
//! 3. **Rounded offsets** ([`round`]): every face's slab ([`slab`], the face swept along its
//!    normal), tubes round edges and balls round vertices, united (outward) or subtracted
//!    (inward); shells and thick solids from those.
//! 4. **Planar wires** ([`wire`]): DRAW's `mkoffset`, as OCCT's `BRepOffsetAPI_MakeOffset`.
//!
//! [`offset`] is `Kernel::offset`: sharp or rounded by the spec, with the faces' history.

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
    let size = b.bbox().diagonal().max(1.0);
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
    // (One piece as it was: a move that breaks the body apart is no Move face.)
    (out.solids.len() == b.solids.len()
        && out
            .check_with(crate::native::topo::CheckLevel::Full)
            .is_empty())
    .then_some(out)
}

pub fn offset(k: &mut NativeKernel, body: BodyId, spec: &OffsetSpec) -> Result<OpResult> {
    let b = (**k.get(body)?).clone();
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
    let out = if spec.sharp || dist.iter().all(|d| *d == 0.0) {
        let out = sharp::offset_sharp(&b, &dist)?;
        if moving
            && !out
                .check_with(crate::native::topo::CheckLevel::Full)
                .is_empty()
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
    };
    let v0 = crate::native::mass::body_volume(&b);
    let v1 = crate::native::mass::body_volume(&out);
    let grow = dist.iter().any(|d| *d > 0.0);
    let shrink = dist.iter().any(|d| *d < 0.0);
    if (grow && !shrink && v1 < v0 - tol::ROUNDOFF)
        || (shrink && !grow && v1 > v0 + tol::ROUNDOFF)
        || v1 <= 0.0
    {
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
    let mut kept = vec![false; b.faces.len()];
    for (f, src) in srcs.iter().enumerate() {
        for s in src {
            match s {
                crate::native::blend::FaceSrc::Input(i) => {
                    kept[*i] = true;
                    history.modified.push((
                        FaceId(f as u64),
                        InputFace {
                            body,
                            face: FaceId(*i as u64),
                        },
                    ));
                }
                crate::native::blend::FaceSrc::New(o) => {
                    history.generated.push((FaceId(f as u64), *o))
                }
            }
        }
    }
    history.deleted = (0..b.faces.len())
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

//! Sweeps of solids made once for every place of one tool: a cutter is one body moved rigidly
//! to each pass of its program, and what it sweeps there, by a motion that sits the same way
//! to it, is what it swept before, moved the same way. A session keeps each body it made by
//! moving another ([`NativeKernel::placed`]: the body first moved, and the motion from it),
//! and each sweep by that first body and the motion as that body sees it; the sweep of a copy
//! is then the kept one, moved to the copy. (A dome's 180 passes along its meridians, the one
//! cutter turned 2° on for each, are three sweeps.)
//!
//! Nothing is compared but the motions' numbers, in the first body's own coordinates, to
//! 1e-10: the bodies are the same by how they were made, not by looking alike.

use std::sync::Arc;

use super::kernel::NativeKernel;
use super::math::*;
use super::topo::Body;
use crate::*;

/// A kept sweep: the motion as the first body sees it, the sweep's bodies in that body's
/// coordinates, its history, and the body it was made of.
pub(crate) struct Swept {
    motion: Vec<f64>,
    bodies: Vec<Body>,
    history: History,
    input: BodyId,
    /// The bodies as first made, and where the swept body stood then: a later copy of the
    /// sweep is those bodies moved on from there ([`NativeKernel::placed`]).
    first: Vec<BodyId>,
    stood: Affine,
}

/// A kept union of several bodies ([`united`]): the bodies first moved of each, how each
/// stands to the first (and the tolerance it was united at), the union in the first's own
/// coordinates, and each of its faces' origins (which body of the list, which face).
pub(crate) struct United {
    roots: Vec<u64>,
    stand: Vec<f64>,
    body: Body,
    origin: Vec<Vec<(usize, usize)>>,
}

/// How many unions a session keeps.
const UNIONS_KEPT: usize = 32;

/// The body first moved of each of `tools`, the numbers of how each stands to the first, and
/// where the first stands.
fn standing(k: &NativeKernel, tools: &[BodyId], point: f64) -> (Vec<u64>, Vec<f64>, Affine) {
    let placed = |t: BodyId| {
        k.placed
            .get(&t.0)
            .copied()
            .unwrap_or((t.0, Affine::identity()))
    };
    let (_, first) = placed(tools[0]);
    let inv = back(&first);
    let mut roots = Vec::with_capacity(tools.len());
    let mut stand = vec![point];
    for &t in tools {
        let (root, at) = placed(t);
        roots.push(root);
        let rel = after(&inv, &at);
        stand.extend_from_slice(rel.m.as_slice());
        stand.extend_from_slice(rel.t.as_slice());
    }
    (roots, stand, first)
}

/// The union of `tools` as a session kept it of the same bodies standing the same way to one
/// another (a program's every batch of passes, each the one before turned on): moved to
/// where these stand, with each face's origin (tool, face). None where none is kept.
pub(crate) fn united(
    k: &NativeKernel,
    tools: &[BodyId],
    point: f64,
) -> Option<super::boolean::Unioned> {
    if tools.len() < 3 || crate::native::env_flag!("CADRS_NO_SWEEP_CACHE") {
        return None;
    }
    let (roots, stand, first) = standing(k, tools, point);
    let hit = k.united.iter().find(|u| {
        u.roots == roots
            && u.stand.len() == stand.len()
            && u.stand
                .iter()
                .zip(&stand)
                .all(|(a, b)| (a - b).abs() <= 1e-10 * (1.0 + a.abs()))
    })?;
    Some((
        hit.body.transformed(&first),
        hit.origin
            .iter()
            .map(|srcs| srcs.iter().map(|&(i, f)| (tools[i], f)).collect())
            .collect(),
    ))
}

/// Keeps the union `body` of `tools` (its faces' origins `origin`) for [`united`].
pub(crate) fn keep_united(
    k: &mut NativeKernel,
    tools: &[BodyId],
    point: f64,
    body: &Body,
    origin: &[Vec<(BodyId, usize)>],
) {
    if tools.len() < 3 || crate::native::env_flag!("CADRS_NO_SWEEP_CACHE") {
        return;
    }
    let (roots, stand, first) = standing(k, tools, point);
    let index = |b: BodyId| tools.iter().position(|t| *t == b);
    let Some(origin) = origin
        .iter()
        .map(|srcs| {
            srcs.iter()
                .map(|&(b, f)| Some((index(b)?, f)))
                .collect::<Option<Vec<_>>>()
        })
        .collect::<Option<Vec<_>>>()
    else {
        return;
    };
    if k.united.len() >= UNIONS_KEPT {
        k.united.remove(0);
    }
    k.united.push(Arc::new(United {
        roots,
        stand,
        body: body.transformed(&back(&first)),
        origin,
    }));
}

/// How many sweeps a session keeps for one body, and for how many bodies (past either, that
/// body's, or all, are dropped: a cache, not a record).
const KEPT: (usize, usize) = (64, 256);

/// `a` after `b`.
pub(crate) fn after(a: &Affine, b: &Affine) -> Affine {
    Affine {
        m: a.m * b.m,
        t: a.m * b.t + a.t,
    }
}

/// The inverse of a rigid motion.
pub(crate) fn back(a: &Affine) -> Affine {
    let m = a.m.transpose();
    Affine { m, t: -(m * a.t) }
}

impl NativeKernel {
    /// The body `id` is a moved copy of, and the motion that takes that body to it (itself
    /// and no motion, for a body never moved).
    pub(crate) fn copy_of(&self, id: BodyId) -> (u64, Affine) {
        self.placed
            .get(&id.0)
            .copied()
            .unwrap_or((id.0, Affine::identity()))
    }

    /// Records that `made` is `of` moved by `motion` (a rotation and a translation; a
    /// reflection is no such placing).
    pub(crate) fn place(&mut self, made: BodyId, of: BodyId, motion: &Affine) {
        if motion.m.determinant() <= 0.0 {
            return;
        }
        let (root, to_of) = self
            .placed
            .get(&of.0)
            .copied()
            .unwrap_or((of.0, Affine::identity()));
        self.placed.insert(made.0, (root, after(motion, &to_of)));
    }
}

/// The motion's numbers as the body it is from sees them (`inv` takes the world to that
/// body's coordinates): its kind, each axis by its direction and its point nearest the
/// body's origin (any point of an axis says the same turn), each slide by its ends. None for
/// the motions that aren't kept (along a path, in a plane, a climb).
fn seen(motion: &SolidMotion, inv: &Affine) -> Option<Vec<f64>> {
    let axis = |a: &Axis, along: bool, out: &mut Vec<f64>| {
        let (o, d) = (inv.point(&a.origin), inv.vector(&a.dir));
        // (A screw's heights count from its axis's own origin: that point, not the nearest.)
        let o = if along { o } else { o - d * o.coords.dot(&d) };
        out.extend_from_slice(o.coords.as_slice());
        out.extend_from_slice(d.as_slice());
    };
    let mut out = Vec::new();
    match motion {
        SolidMotion::Turn { axis: a } => {
            out.push(1.0);
            axis(a, false, &mut out);
        }
        SolidMotion::Screw {
            axis: a,
            rise,
            from,
            to,
        } => {
            out.push(2.0);
            axis(a, true, &mut out);
            out.extend([*rise, *from, *to]);
        }
        SolidMotion::Rigid { steps } => {
            out.push(3.0);
            for s in steps {
                match s {
                    RigidStep::Turn { axis: a, from, to } => {
                        out.push(1.0);
                        axis(a, false, &mut out);
                        out.extend([*from, *to]);
                    }
                    RigidStep::Slide { from, to } => {
                        out.push(2.0);
                        out.extend_from_slice(inv.vector(from).as_slice());
                        out.extend_from_slice(inv.vector(to).as_slice());
                    }
                }
            }
        }
        _ => return None,
    }
    Some(out)
}

/// The history with `from` named where it names `was` (a kept sweep's input body).
fn renamed(h: &History, was: BodyId, from: BodyId) -> History {
    let body = |b: BodyId| if b == was { from } else { b };
    History {
        generated: h
            .generated
            .iter()
            .map(|&(f, o)| {
                let o = match o {
                    Origin::FromEdge { body: b, edge } => Origin::FromEdge {
                        body: body(b),
                        edge,
                    },
                    Origin::FromVertex { body: b, vertex } => Origin::FromVertex {
                        body: body(b),
                        vertex,
                    },
                    o => o,
                };
                (f, o)
            })
            .collect(),
        modified: h
            .modified
            .iter()
            .map(|&(f, i)| {
                (
                    f,
                    InputFace {
                        body: body(i.body),
                        face: i.face,
                    },
                )
            })
            .collect(),
        deleted: h
            .deleted
            .iter()
            .map(|i| InputFace {
                body: body(i.body),
                face: i.face,
            })
            .collect(),
    }
}

/// [`crate::Kernel::sweep_solid`]: the kept sweep of this body's first copy by this motion
/// as the copy sees it, moved here; else made ([`super::sweep_solid::sweep_solid`]) and kept.
pub(crate) fn sweep_solid(
    k: &mut NativeKernel,
    body: BodyId,
    motion: &SolidMotion,
) -> Result<OpResult> {
    if crate::native::env_flag!("CADRS_NO_SWEEP_CACHE") {
        return super::sweep_solid::sweep_solid(k, body, motion);
    }
    let (root, place) = k
        .placed
        .get(&body.0)
        .copied()
        .unwrap_or((body.0, Affine::identity()));
    let inv = back(&place);
    let Some(key) = seen(motion, &inv) else {
        return super::sweep_solid::sweep_solid(k, body, motion);
    };
    let same = |s: &&Arc<Swept>| {
        s.motion.len() == key.len()
            && s.motion
                .iter()
                .zip(&key)
                .all(|(a, b)| (a - b).abs() <= 1e-10 * (1.0 + a.abs()))
    };
    if let Some(hit) = k
        .swept
        .get(&root)
        .and_then(|kept| kept.iter().find(same))
        .cloned()
    {
        // (Each copy is the sweep as first made, moved on from where its body stood then.)
        let on = after(&place, &back(&hit.stood));
        let bodies: Vec<BodyId> = hit
            .bodies
            .iter()
            .map(|b| k.insert(b.transformed(&place)))
            .collect();
        for (&made, &first) in bodies.iter().zip(&hit.first) {
            k.place(made, first, &on);
        }
        return Ok(OpResult {
            bodies,
            history: renamed(&hit.history, hit.input, body),
        });
    }
    // Made here. (Not kept where the sweep left the session something of its own about the
    // result, a swept torus's motion or a pipe's path: a copy moved into place has none.)
    let aside = (k.swept_tori.len(), k.pipes.len());
    let r = super::sweep_solid::sweep_solid(k, body, motion)?;
    if aside == (k.swept_tori.len(), k.pipes.len()) {
        let mut bodies = Vec::with_capacity(r.bodies.len());
        for &id in &r.bodies {
            bodies.push(k.get(id)?.transformed(&inv));
        }
        if k.swept.len() >= KEPT.1 && !k.swept.contains_key(&root) {
            k.swept.clear();
        }
        let kept = k.swept.entry(root).or_default();
        if kept.len() >= KEPT.0 {
            kept.clear();
        }
        kept.push(Arc::new(Swept {
            motion: key,
            bodies,
            history: r.history.clone(),
            input: body,
            first: r.bodies.clone(),
            stood: place,
        }));
    }
    Ok(r)
}

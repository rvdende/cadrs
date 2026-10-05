//! MC1.3: an Extrude or Revolve end **up to** a face, part or vertex of an assembly context. The
//! feature carries the context part frozen ([`crate::assembly::context::ContextTarget`]): its
//! Part Studio is rebuilt in the session (set aside like a Derived source), the part moved to
//! where the context has it, and added to a copy of the state the end is resolved in, under the
//! context part's id and with its source's face names (the names the context's view shows). It is
//! a reference only: the body is released once the feature is built, and never becomes a part.

use super::*;
use cadrs_kernel::{Kernel, Motion};
use nalgebra::{Matrix3, Vector3};

use crate::assembly::context::ContextTarget;

fn motion_of(p: &crate::assembly::Pose) -> Motion {
    Motion { linear: Matrix3::from_row_slice(&p.rotation.concat()), translation: Vector3::from(p.translation) }
}

impl Rebuilder {
    /// `state` with the context parts of `targets` added, and the kernel bodies to release after
    /// the feature is built. No targets: `state` itself.
    pub(in crate::rebuild) fn with_context_targets(&mut self, state: &Arc<State>, targets: &[ContextTarget]) -> Result<(Arc<State>, Vec<BodyId>), String> {
        if targets.is_empty() {
            return Ok((state.clone(), Vec::new()));
        }
        let mut next = (**state).clone();
        let mut owned: Vec<BodyId> = Vec::new();
        for t in targets {
            match self.context_part(t) {
                Ok(Some((ps, body))) => {
                    owned.push(body);
                    next.parts.push(ps);
                }
                // Not in its frozen studio: the end reports its target as gone.
                Ok(None) => {}
                Err(e) => {
                    for b in owned {
                        self.kernel.release(b);
                    }
                    return Err(e);
                }
            }
        }
        Ok((Arc::new(next), owned))
    }

    /// The context part `t` as a part of the state (and its body), or `None` if its source studio
    /// no longer makes it.
    fn context_part(&mut self, t: &ContextTarget) -> Result<Option<(PartState, BodyId)>, String> {
        let (_, src) = self.sub_build(&t.source.features)?;
        let Some(ps) = src.part(t.source_part) else { return Ok(None) };
        let Some(body) = ps.body else { return Ok(None) };
        let r = self.kernel.transform_motion(body, &motion_of(&t.pose)).map_err(|e| format!("The context part could not be placed: {e}"))?;
        let b = r.bodies[0];
        // The source's face names, through the move.
        let n = match self.kernel.faces(b) {
            Ok(f) => f.len(),
            Err(e) => {
                self.kernel.release(b);
                return Err(e.to_string());
            }
        };
        let mut faces: Vec<Option<cadrs_sketch::FaceName>> = vec![None; n];
        for (f, input) in &r.history.modified {
            if let (Some(slot), Some(sn)) = (faces.get_mut(f.0 as usize), ps.names.face(input.face)) {
                *slot = Some(sn);
            }
        }
        let faces = faces
            .into_iter()
            .enumerate()
            .map(|(i, f)| f.unwrap_or_else(|| cadrs_sketch::FaceName::new(t.part.0, cadrs_sketch::FaceOrigin::Unnamed { index: i as u32 })))
            .collect();
        let names = match self.names_from_faces(b, faces) {
            Ok(n) => n,
            Err(e) => {
                self.kernel.release(b);
                return Err(e);
            }
        };
        let id = PartId::new(t.part, 0);
        let part = Part {
            id,
            feature: t.part,
            name: ps.part.name.clone(),
            kind: ps.part.kind,
            palette: ps.part.palette,
            solid: Arc::new(crate::assembly::transform_solid(&ps.part.solid, &t.pose)),
            mass: None,
            features: vec![t.part],
            source: None,
            derived: None,
        };
        Ok(Some((PartState { part, body: Some(b), names: Arc::new(names) }, b)))
    }
}

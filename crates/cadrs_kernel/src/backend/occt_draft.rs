//! Draft, offset and variable-radius fillets of the OCCT backend (P3.10, [`Kernel::draft`],
//! [`Kernel::offset`], [`Kernel::fillet_variable`]).
//!
//! **Draft** (`BRepOffsetAPI_DraftAngle`, fork `Shape::try_draft_h`): each face is turned about
//! its line on the neutral plane. OCCT's positive angle leans a side face in towards the
//! material as it runs along the pull direction, as ours does; the conformance case
//! `draft_cube_sides` pins this with the frustum's volume. Only planes, cylinders and cones can be drafted (OCCT's
//! `Draft_Modification`); a face parallel to the neutral plane is refused.
//!
//! **Offset** (`BRepOffset_MakeOffset` in skin mode, fork `Shape::try_offset_h`): every face
//! moved outward by the distance or its own; sharp edges join the offset faces by intersection.
//!
//! **Variable fillet** (`BRepFilletAPI_MakeFillet` with a law per edge, fork
//! `Shape::try_fillet_variable_h`): OCCT interpolates the `(t, r)` pairs along each edge's
//! parameter; the edge's parameter runs from its `start` to its `end` (the same direction as
//! [`crate::EdgeInfo`]), so `t` is the fraction of the parameter range. On lines and circles the
//! parameter is proportional to length.

use glam::DVec3;

use super::{OcctKernel, face_count, occt, overflows, select_edges, single_input_history, to_glam};
use crate::{BodyId, DraftSpec, EdgeId, FilletLaw, Kernel, KernelError, OffsetSpec, OpResult, Result};

impl OcctKernel {
    pub(super) fn draft_full(&mut self, body: BodyId, spec: &DraftSpec) -> Result<OpResult> {
        if spec.faces.is_empty() {
            return Err(KernelError::InvalidParameter("Select faces to draft".into()));
        }
        if !(spec.angle.is_finite() && spec.angle.abs() < std::f64::consts::FRAC_PI_2 - 1e-6) {
            return Err(KernelError::InvalidParameter("The draft angle must be between -90° and 90°".into()));
        }
        let shape = self.body(body)?;
        let n = face_count(shape)?;
        if spec.faces.iter().any(|f| f.0 as usize >= n) {
            return Err(KernelError::OperationFailed("unknown face".into()));
        }
        // OCCT's DraftAngle without propagation can crash on faces with tangent neighbours
        // (and on a cylinder's own seam), so it always propagates; without Tangent propagation a
        // pick with an unpicked tangent neighbour is refused instead, as the fillet's is.
        if !spec.tangent_propagation {
            let infos = self.edges(body)?;
            let shape = self.body(body)?;
            let mut extra: Vec<crate::FaceId> = Vec::new();
            for e in &infos {
                let [Some(a), Some(b)] = e.faces else { continue };
                if a == b {
                    continue;
                }
                let (mine, other) = match (spec.faces.contains(&a), spec.faces.contains(&b)) {
                    (true, false) => (a, b),
                    (false, true) => (b, a),
                    _ => continue,
                };
                let _ = mine;
                let mid = shape.edge_normals(e.id.0 as usize, 3).map_err(occt)?;
                let tangent = mid.get(1).is_some_and(|m| m.normals[0].dot(m.normals[1]) > crate::TANGENT_ANGLE.cos());
                if tangent && !extra.contains(&other) {
                    extra.push(other);
                }
            }
            if !extra.is_empty() {
                let n = extra.len();
                return Err(KernelError::InvalidParameter(format!(
                    "Without tangent propagation the draft would stop at a smooth edge ({n} tangent face{} not picked); turn on Tangent propagation or pick them",
                    if n == 1 { "" } else { "s" }
                )));
            }
        }
        let before = self.faces(body)?;
        let shape = self.body(body)?;
        let faces: Vec<usize> = spec.faces.iter().map(|f| f.0 as usize).collect();
        let angles = vec![spec.angle; faces.len()];
        let pull: DVec3 = to_glam(spec.pull.into_inner());
        let (result, h) = shape
            .try_draft_h(&faces, &angles, pull, to_glam(spec.neutral.coords), pull, true)
            .map_err(occt)?;
        if !result.is_valid().map_err(occt)? {
            return Err(KernelError::OperationFailed("the drafted body is not a valid solid".into()));
        }
        let mut out = self.insert(result, single_input_history(body, &h))?;
        self.recover_drafted(&mut out, body, &before, spec.angle.abs())?;
        Ok(out)
    }

    /// P3.11: OCCT's draft history reports the drafted faces as deleted, so their persistent
    /// names were lost (a later feature on a drafted face lost it, and the dialog couldn't show
    /// the faces to draft). Each input face the history lost continues as the new face nearest
    /// to it that the history doesn't account for, whose normal is within the draft angle of
    /// its own (a face only turns about its line on the neutral plane).
    fn recover_drafted(&self, out: &mut OpResult, body: BodyId, before: &[crate::FaceInfo], angle: f64) -> Result<()> {
        let after = self.faces(out.bodies[0])?;
        let mut taken: Vec<u64> = out.history.modified.iter().map(|(f, _)| f.0).chain(out.history.generated.iter().map(|(f, _)| f.0)).collect();
        let normal = |f: &crate::FaceInfo| f.plane.map(|p| p.normal.into_inner());
        let mut lost: Vec<crate::InputFace> = Vec::new();
        for input in std::mem::take(&mut out.history.deleted) {
            let Some(was) = before.iter().find(|f| f.id == input.face).filter(|_| input.body == body) else {
                lost.push(input);
                continue;
            };
            let best = after
                .iter()
                .filter(|f| !taken.contains(&f.id.0) && f.kind == was.kind)
                .filter(|f| match (normal(was), normal(f)) {
                    (Some(a), Some(b)) => a.dot(&b) >= (angle + 1e-3).cos() - 1e-9,
                    _ => true,
                })
                .min_by(|a, b| (a.center - was.center).norm().total_cmp(&(b.center - was.center).norm()));
            match best {
                Some(f) => {
                    taken.push(f.id.0);
                    out.history.modified.push((f.id, input));
                }
                None => lost.push(input),
            }
        }
        out.history.deleted = lost;
        Ok(())
    }

    pub(super) fn offset_full(&mut self, body: BodyId, spec: &OffsetSpec) -> Result<OpResult> {
        if !spec.distance.is_finite() || spec.faces.iter().any(|(_, d)| !d.is_finite()) {
            return Err(KernelError::InvalidParameter("The offset distance must be a number".into()));
        }
        let shape = self.body(body)?;
        let n = face_count(shape)?;
        if spec.faces.iter().any(|(f, _)| f.0 as usize >= n) {
            return Err(KernelError::OperationFailed("unknown face".into()));
        }
        let before = self.mass_properties(body)?.volume;
        let shape = self.body(body)?;
        if spec.distance.abs() < 1e-9 && spec.faces.iter().all(|(_, d)| d.abs() < 1e-9) {
            // Nothing moves: a copy.
            let copy = super::clone_shape(shape);
            let h = History::default();
            let mut out = h;
            for i in 0..n {
                out.modified.push((crate::FaceId(i as u64), crate::InputFace { body, face: crate::FaceId(i as u64) }));
            }
            return self.insert_raw(copy, out);
        }
        let faces: Vec<usize> = spec.faces.iter().map(|(f, _)| f.0 as usize).collect();
        let offsets: Vec<f64> = spec.faces.iter().map(|(_, d)| *d).collect();
        let (result, h) = shape.try_offset_h(&faces, &offsets, spec.distance, spec.sharp).map_err(occt)?;
        if !result.is_valid().map_err(occt)? || result.sub_count(opencascade::safe::SubKind::Solid).map_err(occt)? != 1 {
            return Err(KernelError::OperationFailed("the offset body is not a valid solid".into()));
        }
        let out = self.insert(result, single_input_history(body, &h))?;
        let after = self.mass_properties(out.bodies[0])?.volume;
        // An outward offset grows the body; OCCT can return the input's complement or an
        // inside-out solid when it fails quietly.
        let grows = spec.distance > 0.0 || spec.faces.iter().any(|(_, d)| *d > 0.0);
        let shrinks = spec.distance < 0.0 || spec.faces.iter().any(|(_, d)| *d < 0.0);
        if (grows && !shrinks && after <= before) || (shrinks && !grows && after >= before) || after <= 0.0 {
            self.release(out.bodies[0]);
            return Err(KernelError::OperationFailed("the offset went the wrong way".into()));
        }
        Ok(out)
    }

    pub(super) fn fillet_variable_full(&mut self, body: BodyId, laws: &[FilletLaw], allow_overflow: bool) -> Result<OpResult> {
        if laws.is_empty() {
            return Err(KernelError::InvalidParameter("Select edges or faces to fillet".into()));
        }
        for l in laws {
            if l.radii.is_empty() {
                return Err(KernelError::InvalidParameter("A variable fillet needs a radius on every edge".into()));
            }
            if l.radii.iter().any(|(t, r)| !(r.is_finite() && *r > 0.0 && (0.0..=1.0).contains(t))) {
                return Err(KernelError::InvalidParameter("Every radius of a variable fillet must be greater than zero".into()));
            }
        }
        let listed: Vec<EdgeId> = laws.iter().map(|l| l.edge).collect();
        for &e in &listed {
            for c in self.tangent_chain(body, e)? {
                if !listed.contains(&c) {
                    return Err(KernelError::InvalidParameter(
                        "A variable fillet needs radii on every edge tangent to its edges".into(),
                    ));
                }
            }
        }
        let shape = self.body(body)?;
        let selected = select_edges(shape, &listed)?;
        let radii: Vec<Vec<(f64, f64)>> = laws
            .iter()
            .map(|l| {
                let mut r = l.radii.clone();
                r.sort_by(|a, b| a.0.total_cmp(&b.0));
                r.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-9);
                // A law needs both ends; a single radius is constant.
                if r.len() > 1 {
                    if r[0].0 > 1e-9 {
                        r.insert(0, (0.0, r[0].1));
                    }
                    let last = r[r.len() - 1];
                    if last.0 < 1.0 - 1e-9 {
                        r.push((1.0, last.1));
                    }
                }
                r
            })
            .collect();
        let (result, h) = shape
            .try_fillet_variable_h(selected.iter().zip(radii.iter().map(Vec::as_slice)))
            .map_err(occt)?;
        if !allow_overflow {
            let infos = self.edges(body)?;
            let vertices = self.vertices(body)?;
            if overflows(&result, &h, &listed, &infos, &vertices)? {
                return Err(KernelError::InvalidParameter(
                    "The fillet runs over onto a neighbouring face; turn on Allow edge overflow".into(),
                ));
            }
        }
        self.insert(result, single_input_history(body, &h))
    }
}

use crate::History;

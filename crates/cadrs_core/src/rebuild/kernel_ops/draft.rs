//! Rebuilding the Draft feature and the extrude's Draft option (P3.10, PS4.9, X3). A child of
//! `rebuild::kernel_ops`, so it shares its helpers (face lookup, per-part results).

use super::*;
use cadrs_kernel::{DraftSpec, ExtrudeEnd, ExtrudeSpec, Kernel, OpResult, Origin, Profile};
use cadrs_sketch::PlaneFrame;
use nalgebra::{Point3, Unit, Vector3};

use crate::draft::DraftFeature;
use crate::pattern::MirrorPlane;

impl Rebuilder {
    /// The Draft feature: the faces of each part turned about the neutral plane.
    pub(in crate::rebuild) fn draft(
        &mut self,
        before: &[Feature],
        id: FeatureId,
        x: &DraftFeature,
        state: &Arc<State>,
    ) -> Result<Output, String> {
        if let Some(p) = x.problem() {
            return Err(p.into());
        }
        let lost = || "The neutral plane no longer exists".to_string();
        let frame: PlaneFrame = match x.neutral.ok_or("Select a neutral plane")? {
            MirrorPlane::Plane(p) => super::advanced::plane_of(state, &p).ok_or_else(lost)?,
            MirrorPlane::Face(f) => {
                let (part, _) = face_ids(state, &f).ok_or_else(lost)?;
                face_frame(part, &f).ok_or("The neutral plane must be a flat face")?
            }
            MirrorPlane::Connector(c) => super::super::connector_frame(before, state, &c)?,
        };
        let n = frame.normal();
        let normal = Unit::new_normalize(Vector3::new(n[0], n[1], n[2]));
        let pull = if x.flip { -normal } else { normal };
        let neutral = Point3::new(frame.origin[0], frame.origin[1], frame.origin[2]);
        // The faces, by part.
        let mut groups: Vec<(PartId, BodyId, Vec<cadrs_kernel::FaceId>)> = Vec::new();
        let mut missing = 0;
        for f in &x.faces {
            let Some((part, ids)) = face_ids(state, f) else {
                missing += 1;
                continue;
            };
            let body = part.body.ok_or("A part has no body")?;
            match groups.iter_mut().find(|g| g.0 == part.part.id) {
                Some(g) => g.2.extend(ids.into_iter().filter(|i| !g.2.contains(i)).collect::<Vec<_>>()),
                None => groups.push((part.part.id, body, ids)),
            }
        }
        if groups.is_empty() {
            return Err("The faces to draft no longer exist".into());
        }
        let faces: Vec<Vec<cadrs_kernel::FaceId>> = groups.iter().map(|g| g.2.clone()).collect();
        let bodies = groups.iter().map(|g| (g.0, g.1)).collect();
        let (angle, tangent) = (x.angle.to_radians(), x.tangent_propagation);
        let mut out = self.per_part(id, "Draft", state, bodies, |this, i, body| {
            let spec = DraftSpec { faces: faces[i].clone(), angle, pull, neutral, tangent_propagation: tangent };
            this.kernel.draft(body, &spec)
        })?;
        // PS11.1: some faces gone but the rest drafted is a warning, not a failure.
        if missing > 0 {
            out.warning = Some(if missing == 1 {
                "1 face to draft no longer exists; the others are drafted".into()
            } else {
                format!("{missing} faces to draft no longer exist; the others are drafted")
            });
        }
        Ok(out)
    }

    /// An extrude with Draft (PS4.9): each end extruded on its own, its side faces (the faces
    /// the profile's curves made) drafted about the start plane with the pull along that end's
    /// direction, and the ends joined; the result's faces carry the extrude's own names (the far
    /// face of a second end is its start cap, as a two-ended extrude's).
    pub(in crate::rebuild) fn drafted_extrude(&mut self, profile: &Profile, spec: &ExtrudeSpec, angle: f64) -> cadrs_kernel::Result<OpResult> {
        let start = profile.plane.origin + spec.direction.into_inner() * spec.start_offset;
        let one = |end: ExtrudeEnd, direction: Unit<Vector3<f64>>, start_offset: f64| ExtrudeSpec {
            end,
            direction,
            start_offset,
            symmetric: false,
            second: None,
            ..spec.clone()
        };
        let mut halves: Vec<(ExtrudeSpec, bool)> = Vec::new();
        if spec.symmetric {
            let half = match spec.end {
                ExtrudeEnd::Blind(d) => ExtrudeEnd::Blind(d / 2.0),
                e => e,
            };
            halves.push((one(half, spec.direction, spec.start_offset), false));
            halves.push((one(half, -spec.direction, -spec.start_offset), true));
        } else {
            halves.push((one(spec.end, spec.direction, spec.start_offset), false));
            if let Some(second) = spec.second {
                halves.push((one(second, -spec.direction, -spec.start_offset), true));
            }
        }
        let mut made: Vec<(cadrs_kernel::BodyId, Vec<(cadrs_kernel::FaceId, Origin)>)> = Vec::new();
        let release = |k: &mut cadrs_kernel::backend::occt::OcctKernel, made: &[(cadrs_kernel::BodyId, Vec<(cadrs_kernel::FaceId, Origin)>)]| {
            for (b, _) in made {
                k.release(*b);
            }
        };
        for (half, second) in &halves {
            let r = match self.kernel.extrude_with(profile, half) {
                Ok(r) => r,
                Err(e) => {
                    release(&mut self.kernel, &made);
                    return Err(e);
                }
            };
            let body = r.bodies[0];
            let origin_of = |f: cadrs_kernel::FaceId| r.history.generated.iter().find(|(g, _)| *g == f).map(|(_, o)| *o);
            let sides: Vec<cadrs_kernel::FaceId> = r
                .history
                .generated
                .iter()
                .filter(|(_, o)| matches!(o, Origin::ProfileCurve { .. }))
                .map(|(f, _)| *f)
                .collect();
            let drafted = self.kernel.draft(
                body,
                &DraftSpec { faces: sides, angle, pull: half.direction, neutral: start, tangent_propagation: true },
            );
            self.kernel.release(body);
            let d = match drafted {
                Ok(d) => d,
                Err(e) => {
                    release(&mut self.kernel, &made);
                    return Err(e);
                }
            };
            // The drafted body's faces continue the extrude's.
            let mut tags: Vec<(cadrs_kernel::FaceId, Origin)> = Vec::new();
            for (f, input) in &d.history.modified {
                if let Some(mut o) = origin_of(input.face) {
                    if *second {
                        o = match o {
                            Origin::EndCap { region } => Origin::StartCap { region },
                            Origin::StartCap { region } => Origin::EndCap { region },
                            o => o,
                        };
                    }
                    if !tags.iter().any(|(g, _)| g == f) {
                        tags.push((*f, o));
                    }
                }
            }
            made.push((d.bodies[0], tags));
        }
        if made.len() == 1 {
            let (body, tags) = made.pop().expect("one");
            return Ok(OpResult { bodies: vec![body], history: cadrs_kernel::History { generated: tags, ..Default::default() } });
        }
        let (a, b) = (made[0].0, made[1].0);
        let joined = self.kernel.boolean(cadrs_kernel::BoolOp::Union, a, &[b]);
        let result = joined.map(|r| {
            let mut tags: Vec<(cadrs_kernel::FaceId, Origin)> = Vec::new();
            for (f, input) in &r.history.modified {
                let from = made.iter().find(|(m, _)| *m == input.body).and_then(|(_, t)| t.iter().find(|(g, _)| *g == input.face));
                if let Some((_, o)) = from
                    && !tags.iter().any(|(g, _)| g == f)
                {
                    tags.push((*f, *o));
                }
            }
            OpResult { bodies: r.bodies.clone(), history: cadrs_kernel::History { generated: tags, ..Default::default() } }
        });
        release(&mut self.kernel, &made);
        result
    }
}

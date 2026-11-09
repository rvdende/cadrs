//! What the cadrs implementations of the feature cases share: extrudes and revolves whose end is
//! "up to" something rather than a given depth or angle.
use crate::Refusal;
use crate::bench::{V3, unit};
use cadrs_kernel::{
    Axis, BodyId, DefaultKernel, ExtrudeEnd, ExtrudeSpec, Kernel, OpResult, Profile, RevolveEnd,
    RevolveSpec,
};

fn first(r: OpResult) -> Result<BodyId, Refusal> {
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the feature made no body"))
}

/// `profile` extruded along `dir` to `end`, the bodies of `scene` the ones "Up to next"
/// considers.
pub fn extrude_to(
    k: &mut DefaultKernel,
    profile: &Profile,
    dir: V3,
    end: ExtrudeEnd,
    scene: &[BodyId],
) -> Result<BodyId, Refusal> {
    let mut spec = ExtrudeSpec::blind(unit(dir), 0.0);
    spec.end = end;
    spec.scene = scene.to_vec();
    first(k.extrude_with(profile, &spec)?)
}

/// `profile` revolved about `axis` (right-handed) to `end`, the bodies of `scene` the ones "Up to
/// next" considers.
pub fn revolve_to(
    k: &mut DefaultKernel,
    profile: &Profile,
    axis: Axis,
    end: RevolveEnd,
    scene: &[BodyId],
) -> Result<BodyId, Refusal> {
    let mut spec = RevolveSpec::solid(axis, Some(0.0));
    spec.end = end;
    spec.scene = scene.to_vec();
    first(k.revolve_with(profile, &spec)?)
}

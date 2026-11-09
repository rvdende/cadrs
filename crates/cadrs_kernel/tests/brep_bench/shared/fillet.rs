//! What the cadrs implementations of the fillet cases share: the Fillet feature on picked edges.
use crate::Refusal;
use cadrs_kernel::{BodyId, DefaultKernel, EdgeId, FilletLaw, Kernel};

/// `body` with `edges` (and the edges tangent to them) rounded to `radius` in one Fillet
/// feature.
pub fn fillet(
    k: &mut DefaultKernel,
    body: BodyId,
    edges: &[EdgeId],
    radius: f64,
) -> Result<BodyId, Refusal> {
    let r = k.fillet(body, edges, radius)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the fillet made no body"))
}

/// `body` with each law's edge rounded with the radius it gives along the edge (a variable
/// fillet), overflow allowed.
pub fn fillet_variable(
    k: &mut DefaultKernel,
    body: BodyId,
    laws: &[FilletLaw],
) -> Result<BodyId, Refusal> {
    let r = k.fillet_variable(body, laws, true)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the fillet made no body"))
}

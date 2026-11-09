//! What the cadrs implementations of the offset cases share: cadrs's offset of a solid's faces.
use crate::Refusal;
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, OffsetSpec};

/// `body` offset by `distance` (inward when negative). With `sharp`, every face moves and the
/// moved faces are extended until they meet; without it, the result is the solid of the points
/// within `distance` of `body` (or at least −`distance` from its outside), its edges rounded
/// where the distance makes them round.
pub fn offset(
    k: &mut DefaultKernel,
    body: BodyId,
    distance: f64,
    sharp: bool,
) -> Result<BodyId, Refusal> {
    let r = k.offset(
        body,
        &OffsetSpec {
            distance,
            faces: vec![],
            sharp,
        },
    )?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the offset made no body"))
}

/// `body` grown by `distance`: the solid of every point within `distance` of it.
pub fn grown(k: &mut DefaultKernel, body: BodyId, distance: f64) -> Result<BodyId, Refusal> {
    offset(k, body, distance, false)
}

/// `body` shrunk by `distance`: the solid of every point at least `distance` from its outside.
pub fn shrunk(k: &mut DefaultKernel, body: BodyId, distance: f64) -> Result<BodyId, Refusal> {
    offset(k, body, -distance, false)
}

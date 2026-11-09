//! The torus (center (3, 0, 2), axis along z, radii 1 and 0.22) rolled for t from −100° to 100°:
//! turned by 0.7t about the upright line through (2, 0, 0), then by −t about the line through
//! (0, 0.7071 cos 45°, 0.7071 sin 45°) along (cos 20°, sin 20° cos 45°, sin 20° sin 45°).
//!
//! Cadrs's sweep of a solid with the rigid motion of the two turns.
use crate::Refusal;
use crate::shared::swept::crossed_tori;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    Ok(crossed_tori(k)?.0)
}

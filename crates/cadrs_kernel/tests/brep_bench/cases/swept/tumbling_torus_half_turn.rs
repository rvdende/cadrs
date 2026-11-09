//! The torus (center (3, 0, 2), axis along z, radii 1 and 0.16) rolled for t from −90° to 90°:
//! turned by 0.375t about the upright line through (2, 0, 0), then by −t about the line through
//! (0, 0.7071 cos 30°, 0.7071 sin 30°) along (cos 20°, sin 20° cos 30°, sin 20° sin 30°).
//!
//! Cadrs's sweep of a solid with the rigid motion of the two turns.
use crate::Refusal;
use crate::shared::swept::tumbling_torus;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    use crate::bench::p3;
    tumbling_torus(
        k,
        p3(3.0, 0.0, 2.0),
        0.16,
        (30.0, 20.0, 0.7071),
        0.375,
        -1.0,
        (-90.0, 90.0),
    )
}

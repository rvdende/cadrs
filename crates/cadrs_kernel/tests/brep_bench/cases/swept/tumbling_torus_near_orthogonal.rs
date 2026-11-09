//! The torus (center (2.02, 0, 1), axis along z, radii 1 and 0.08) rolled for t from −180° to 180°:
//! turned by 1.01t about the upright line through (2, 0, 0), then by −t about the line through
//! the origin along (cos 89°, sin 89° cos 1°, sin 89° sin 1°), nearly square to the first.
//!
//! Cadrs's sweep of a solid with the rigid motion of the two turns.
use crate::Refusal;
use crate::shared::swept::tumbling_torus;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    use crate::bench::p3;
    tumbling_torus(
        k,
        p3(2.02, 0.0, 1.0),
        0.08,
        (1.0, 89.0, 0.0),
        1.01,
        -1.0,
        (-180.0, 180.0),
    )
}

//! The torus of `swept/tumbling-torus-near-orthogonal` with a tube of radius 0.16.
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
        0.16,
        (1.0, 89.0, 0.0),
        1.01,
        -1.0,
        (-180.0, 180.0),
    )
}

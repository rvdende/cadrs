//! The torus of `swept/tumbling-torus-half-turn` rolled for t from −180° to 180°.
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
        (-180.0, 180.0),
    )
}

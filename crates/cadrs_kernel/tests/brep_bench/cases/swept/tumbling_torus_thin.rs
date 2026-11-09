//! The torus of `swept/tumbling-torus` (center (3, 0, 2), axis along z, major radius 1) with a tube
//! of radius 0.015, rolled for t from −75° to 75°: turned by 0.25t about the upright line through
//! (2, 0, 0), then by −t about the line through (0, ½, ½) along x.
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
        0.015,
        (45.0, 0.0, 0.5f64.sqrt()),
        0.25,
        -1.0,
        (-75.0, 75.0),
    )
}

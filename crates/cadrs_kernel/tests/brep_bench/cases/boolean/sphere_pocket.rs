//! A block from (−10, −10, −10) to (10, 10, 0) less a ball of radius 5 centered on the origin: a
//! hemispherical pocket in its top face. The ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let slab = block(k, p3(-10.0, -10.0, -10.0), p3(10.0, 10.0, 0.0))?;
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    cut(k, slab, b)
}

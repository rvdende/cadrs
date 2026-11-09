//! A ball of radius 5 centered on the origin with everything below the xy plane cut away by a
//! block. The ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let below = block(k, p3(-10.0, -10.0, -10.0), p3(10.0, 10.0, 0.0))?;
    cut(k, b, below)
}

//! A cube of side 10 between the origin and (10, 10, 10), and the ball of radius 5 at its
//! center, which touches each of its faces at one point: their common part, the whole ball. The
//! ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, block, common, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let b = ball(k, p3(5.0, 5.0, 5.0), 5.0)?;
    common(k, cube, b)
}

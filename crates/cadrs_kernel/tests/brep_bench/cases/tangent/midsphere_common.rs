//! A cube of side 10 from (−5, −5, −5) to (5, 5, 5), and the ball about the origin of radius
//! 5√2 that touches each of its twelve edges at the midpoint: their common part, six disks
//! touching at the edge midpoints and eight three-sided sphere patches. The ball is cadrs's
//! sphere primitive.
use crate::Refusal;
use crate::bench::{ball, block, common, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(-5.0, -5.0, -5.0), p3(5.0, 5.0, 5.0))?;
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0 * 2f64.sqrt())?;
    common(k, cube, b)
}

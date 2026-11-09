//! A half ball shelled open at its flat face: a bowl. The lower half of the ball of radius 5 centered
//! on the origin, shelled inward to a wall 1 thick, open at its flat face, z = 0: the half ball less
//! the half ball of radius 4.
//!
//! The half ball is cadrs's sphere primitive less a block above z = 0; cadrs's Shell feature
//! (inward), the flat face picked as the face to remove.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the ball's lower half: the ball less a block above z = 0
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let above = block(k, p3(-6.0, -6.0, 0.0), p3(6.0, 6.0, 6.0))?;
    let half = cut(k, b, above)?;
    shell_inward(k, half, &[p3(2.0, 1.0, 0.0)], 1.0, Joins::Rounded)
}

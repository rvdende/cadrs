//! A block, the box from (−6, −6, −8) to (18, 18, 0), grooved by a ball of radius 3 whose
//! center swings a quarter turn about the z axis in the top face, from (12, 0, 0) to (0, 12, 0):
//! everything the ball passes through cut from the block.
//!
//! The ball is cadrs's sphere primitive; its swing is cadrs's sweep of a solid over part of a
//! turn (`Kernel::cut_swept`, a rigid turn about the z axis from 0° to 90°), cut from the block.
use crate::Refusal;
use crate::bench::{ball, block, p3};
use crate::shared::swept::{cut_swept, swing, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let body = block(k, p3(-6.0, -6.0, -8.0), p3(18.0, 18.0, 0.0))?;
    let tool = ball(k, p3(12.0, 0.0, 0.0), 3.0)?;
    cut_swept(k, body, tool, &swing(z_axis(), 0.0, 90.0))
}

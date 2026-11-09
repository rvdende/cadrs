//!
//! The ball is cadrs's sphere primitive; what it passes through as it is carried along the curve
//! (drawn on the Top plane) is cadrs's sweep of a solid along a path (`Kernel::cut_swept`), cut
//! from the block.
use crate::Refusal;
use crate::bench::{ball, block, p3};
use crate::shared::swept::{bezier_path, carry, cut_swept};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let body = block(k, p3(-6.0, -6.0, -8.0), p3(36.0, 26.0, 0.0))?;
    let tool = ball(k, p3(0.0, 0.0, 0.0), 3.0)?;
    let path = bezier_path([(0.0, 0.0), (10.0, 0.0), (20.0, 20.0), (30.0, 20.0)]);
    cut_swept(k, body, tool, &carry(path))
}

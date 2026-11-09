//! The solid a ball (radius 2 about (1, 0, 0)) fills as it turns 120° about the z axis, which
//! passes through it.
//!
//! The ball is cadrs's sphere primitive; what it passes through is cadrs's sweep of a solid (one
//! turn).
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(1.0, 0.0, 0.0), 2.0)?;
    swung(k, b, z_axis(), 0.0, 120.0)
}

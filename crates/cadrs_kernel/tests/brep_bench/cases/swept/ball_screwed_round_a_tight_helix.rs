//! The solid a ball (radius 2, its centre starting at (1.5, 0, 0)) fills as it is screwed about the
//! z axis, rising 3 a turn, for two whole turns: everything within 2 of its centre's helix.
//!
//! cadrs's sweep of a solid (a turn and a slide along its axis at once: a screw).
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::swept::{screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(1.5, 0.0, 0.0), 2.0)?;
    screwed(k, b, z_axis(), 3.0, 0.0, 720.0)
}

//! A pipe of radius 1 along the helix of radius 10 about the z axis rising 5 a turn, from
//! (10, 0, 0) three right-handed turns, each end closed by a half ball of radius 1, as a ball
//! rolled along the helix would leave.
//!
//! The ball is cadrs's sphere primitive, centred where the helix starts; the pipe is what it
//! fills as it is screwed along the helix, cadrs's sweep of a solid along a screw
//! (`Kernel::sweep_solid`: turned about the z axis from 0° to 1080° while rising 5 a turn).
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::swept::{screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let tool = ball(k, p3(10.0, 0.0, 0.0), 1.0)?;
    screwed(k, tool, z_axis(), 5.0, 0.0, 1080.0)
}

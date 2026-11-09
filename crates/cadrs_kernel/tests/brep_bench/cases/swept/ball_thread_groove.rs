//! A rod, the cylinder of radius 5 about the z axis from z = 0 to z = 20, with a helical groove
//! cut by a ball of radius 1 whose center follows the helix of radius 5 about the z axis from
//! (5, 0, −2), right-handed, rising 4 a turn, six whole turns to (5, 0, 22).
//!
//! The ball is cadrs's sphere primitive, centred where the helix starts; its screw is cadrs's
//! sweep of a solid along a screw (`Kernel::cut_swept`: turned about the z axis from 0° to 2160°
//! while rising 4 a turn), cut from the rod.
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::swept::{cut_swept, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let body = rod(k)?;
    let tool = ball(k, p3(5.0, 0.0, -2.0), 1.0)?;
    cut_swept(k, body, tool, &screw(z_axis(), 4.0, 0.0, 2160.0))
}

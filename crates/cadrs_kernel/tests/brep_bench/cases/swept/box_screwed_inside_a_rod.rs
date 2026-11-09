//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) with a sealed helical
//! tunnel: what a box (2.5 ≤ x ≤ 3.5, |y| ≤ 0.5, 2 ≤ z ≤ 3) passes through screwed about the z
//! axis, rising 4 a turn, from a turn of 0° to 720°.
//!
//! cadrs's sweep of a solid (a turn and a slide along its axis at once: a screw), cut from the rod.
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::swept::{rod, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let tool = block(k, p3(2.5, -0.5, 2.0), p3(3.5, 0.5, 3.0))?;
    let swept = screwed(k, tool, z_axis(), 4.0, 0.0, 720.0)?;
    let r = rod(k)?;
    cut(k, r, swept)
}

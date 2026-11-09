//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) with a thread cut partway
//! up it by a bar (the box 4 ≤ x ≤ 12, |y| ≤ 8, |z| ≤ 1) screwed about the z axis, rising 6 a turn,
//! from a turn of −120° to +600°.
//!
//! What the bar passes through is cadrs's sweep of a solid (a turn and a slide along its axis at
//! once: a screw), cut from the rod; of the bar, its part within 5.5 of the axis (the screw maps
//! the cylinder about its axis onto itself, so only that part reaches the rod).
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::swept::{near_axis, rod, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = block(k, p3(4.0, -8.0, -1.0), p3(12.0, 8.0, 1.0))?;
    let bar = near_axis(k, bar, 5.5)?;
    let swept = screwed(k, bar, z_axis(), 6.0, -120.0, 600.0)?;
    let r = rod(k)?;
    cut(k, r, swept)
}

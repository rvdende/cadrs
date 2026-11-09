//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) with a thread cut partway
//! up it by a bar (the box 4 ≤ x ≤ 12, |y| ≤ 8, |z| ≤ 1) screwed about the z axis, rising 6 a turn,
//! from a turn of −90° to 900°, then slid along −x by 3 out of its groove.
//!
//! Each piece is cadrs's sweep of a solid (the screw: a turn and a slide along its axis at once;
//! then one slide of the bar where the screw leaves it), each cut from the rod in turn; of the bar,
//! its part within 5.5 of the axis (the screw maps the cylinder about its axis onto itself, and the
//! slide carries the bar's points away from the axis, so only that part reaches the rod).
use crate::Refusal;
use crate::bench::{block, cut, moved, p3, turned, v3};
use crate::shared::swept::{near_axis, rod, screwed, slid, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = block(k, p3(4.0, -8.0, -1.0), p3(12.0, 8.0, 1.0))?;
    let bar = near_axis(k, bar, 5.5)?;
    let screw = screwed(k, bar, z_axis(), 6.0, -90.0, 900.0)?;
    // The bar where the screw stops: turned 900° and raised 15.
    let there = turned(k, bar, z_axis(), 900.0)?;
    let there = moved(k, there, v3(0.0, 0.0, 15.0))?;
    let out = slid(k, there, v3(-3.0, 0.0, 0.0))?;
    let r = rod(k)?;
    let r = cut(k, r, screw)?;
    cut(k, r, out)
}

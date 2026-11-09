//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by the flat end
//! of an end mill (a cylinder of radius 1.2 along the x axis from x = 4 to 12) screwed about the z
//! axis, rising 6 a turn, from a turn of −90° to 1350°.
//!
//! What the mill passes through is cadrs's sweep of a solid (a turn and a slide along its axis at
//! once: a screw), cut from the rod; of the mill, its part within 5.5 of the axis (the screw maps
//! the cylinder about its axis onto itself, so only that part reaches the rod).
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{near_axis, rod, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mill = cylinder(k, p3(4.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 1.2, 8.0)?;
    let mill = near_axis(k, mill, 5.5)?;
    let swept = screwed(k, mill, z_axis(), 6.0, -90.0, 1350.0)?;
    let r = rod(k)?;
    cut(k, r, swept)
}

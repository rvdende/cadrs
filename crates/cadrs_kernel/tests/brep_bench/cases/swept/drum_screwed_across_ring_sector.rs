//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! with a helical groove milled by a drum (radius 1 about the upright through (10, 0), from z =
//! −0.5 to 0.5) screwed about the z axis, rising 0.5 a turn, from a turn of −140° to +140°.
//!
//! What the drum passes through is cadrs's sweep of a solid (a turn and a slide along its axis at
//! once: a screw), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{ring_sector, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let drum = cylinder(k, p3(10.0, 0.0, -0.5), v3(0.0, 0.0, 1.0), 1.0, 1.0)?;
    let swept = screwed(k, drum, z_axis(), 0.5, -140.0, 140.0)?;
    cut(k, block, swept)
}

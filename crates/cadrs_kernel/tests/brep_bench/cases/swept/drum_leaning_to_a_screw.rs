//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! with a helical groove milled by a drum (radius 0.8, length 3, its axis through its middle
//! (10, 0, 0) along (0, sin 25°, cos 25°)) screwed about the z axis, rising 1 a turn, from a turn
//! of −140° to +140°.
//!
//! What the drum passes through is cadrs's sweep of a solid (a turn and a slide along its axis at
//! once: a screw), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{ring_sector, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let a = 25f64.to_radians();
    let dir = v3(0.0, a.sin(), a.cos());
    let drum = cylinder(k, p3(10.0, 0.0, 0.0) - dir * 1.5, dir, 0.8, 3.0)?;
    let swept = screwed(k, drum, z_axis(), 1.0, -140.0, 140.0)?;
    cut(k, block, swept)
}

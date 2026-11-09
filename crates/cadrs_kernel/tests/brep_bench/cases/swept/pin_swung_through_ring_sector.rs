//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! slotted by a pin (radius 0.5 about the x axis from x = 6 to 11) turned about the z axis from
//! −140° to +140°.
//!
//! The pin is a circle extruded; what it passes through is cadrs's sweep of a solid (one turn),
//! cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let pin = cylinder(k, p3(6.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 0.5, 5.0)?;
    let swept = swung(k, pin, z_axis(), -140.0, 140.0)?;
    cut(k, block, swept)
}

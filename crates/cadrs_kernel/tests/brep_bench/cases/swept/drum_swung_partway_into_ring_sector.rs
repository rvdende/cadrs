//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a drum (radius 1 about the upright through (10, 0), from z = −0.5 to 0.5) turned
//! about the z axis from −140° to +30°.
//!
//! The block is cadrs's cylindrical box; the drum a circle extruded. What the drum passes
//! through is cadrs's sweep of a solid (one turn: the drum where it starts, with what its leading
//! faces sweep), cut from the block.
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let drum = cylinder(k, p3(10.0, 0.0, -0.5), v3(0.0, 0.0, 1.0), 1.0, 1.0)?;
    let swept = swung(k, drum, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

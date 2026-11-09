//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a ring lying flat (the torus about the upright through (10, 0), radii 1 and 0.4)
//! turned about the z axis from −140° to +30°.
//!
//! The ring is cadrs's torus primitive; what it passes through is cadrs's sweep of a solid (one
//! turn), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, p3, torus, v3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let ring = torus(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 1.0, 0.4)?;
    let swept = swung(k, ring, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

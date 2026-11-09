//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a bar (the box 10 ≤ x ≤ 11, |y| ≤ 0.5, |z| ≤ 0.6) turned about the z axis from
//! −140° to +30°.
//!
//! What the bar passes through is cadrs's sweep of a solid (one turn: the bar where it starts,
//! with what its leading faces sweep, its faces x = 10 and 11 split along y = 0), cut from the
//! block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let sector = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let bar = block(k, p3(10.0, -0.5, -0.6), p3(11.0, 0.5, 0.6))?;
    let swept = swung(k, bar, z_axis(), -140.0, 30.0)?;
    cut(k, sector, swept)
}

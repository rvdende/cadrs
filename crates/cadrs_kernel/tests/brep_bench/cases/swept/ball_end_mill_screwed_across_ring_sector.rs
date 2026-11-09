//! A curved block (the ring sector between radii 8 and 12, heights ±2, polar angles −30° to 30°)
//! with a helical groove milled by a ball-end mill (about the upright through (10, 0): the lower
//! half of the ball of radius 1 about (10, 0, 0) under a cylinder of radius 1 from z = 0 to 3)
//! screwed about the z axis, rising 3 a turn, from a turn of −60° to +60°.
//!
//! The mill is its half section (a quarter circle and three lines) turned about its axis (cadrs's
//! Revolve); what it passes through is cadrs's sweep of a solid (a turn and a slide along its axis
//! at once: a screw), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::{ring_sector, screwed, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-2.0, 2.0), (-30.0, 30.0))?;
    let w = 2f64.sqrt() / 2.0;
    let section = Section::on(xz(0.0))
        .line((10.0, -1.0), (10.0, 3.0))
        .line((10.0, 3.0), (11.0, 3.0))
        .line((11.0, 3.0), (11.0, 0.0))
        .conic((11.0, 0.0), (11.0, -1.0), w, (10.0, -1.0));
    let mill = section.turned(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let swept = screwed(k, mill, z_axis(), 3.0, -60.0, 60.0)?;
    cut(k, block, swept)
}

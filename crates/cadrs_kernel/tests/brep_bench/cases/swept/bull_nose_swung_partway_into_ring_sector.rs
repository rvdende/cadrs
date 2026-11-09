//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a bull-nose mill (a cylinder of radius 1 about the upright through (10, 0) from
//! z = −0.5 to 0.5, its bottom corner rounded to radius 0.3) turned about the z axis from −140°
//! to +30°.
//!
//! The mill is its half section turned about its axis (the round a quarter circle, drawn as the
//! rational quadratic it is); what it passes through is cadrs's sweep of a solid (one turn), cut
//! from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let section = Section::on(xz(0.0))
        .line((10.0, -0.5), (10.7, -0.5))
        .conic((10.7, -0.5), (11.0, -0.5), 2f64.sqrt() / 2.0, (11.0, -0.2))
        .line((11.0, -0.2), (11.0, 0.5))
        .line((11.0, 0.5), (10.0, 0.5))
        .line((10.0, 0.5), (10.0, -0.5));
    let mill = section.turned(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let swept = swung(k, mill, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

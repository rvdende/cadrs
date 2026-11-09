//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a drill (a cylinder of radius 1 about the upright through (10, 0) up to z = 3, ended
//! below by a cone of 118° with its point at (10, 0, −0.8)) turned about the z axis from −140° to
//! +30°.
//!
//! The drill is its half section turned about its axis; what it passes through is cadrs's sweep
//! of a solid (one turn), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let shoulder = -0.8 + 1.0 / 59f64.to_radians().tan();
    let section = Section::on(xz(0.0))
        .line((10.0, -0.8), (11.0, shoulder))
        .line((11.0, shoulder), (11.0, 3.0))
        .line((11.0, 3.0), (10.0, 3.0))
        .line((10.0, 3.0), (10.0, -0.8));
    let drill = section.turned(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let swept = swung(k, drill, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

//! A plate (the box from (0, −5, 0) to (20, 5, 4)) bored by a drill (a cylinder of radius 2 about
//! the upright through (10, 0) up to z = 14, ended below by a cone of 118° with its point at
//! (10, 0, 6)) pushed straight down 20, wholly through it.
//!
//! The drill is its half section turned about its axis; what it passes through is cadrs's sweep
//! of a solid (one slide), cut from the plate.
use crate::Refusal;
use crate::bench::{block, cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, -5.0, 0.0), p3(20.0, 5.0, 4.0))?;
    let shoulder = 6.0 + 2.0 / 59f64.to_radians().tan();
    let section = Section::on(xz(0.0))
        .line((10.0, 6.0), (12.0, shoulder))
        .line((12.0, shoulder), (12.0, 14.0))
        .line((12.0, 14.0), (10.0, 14.0))
        .line((10.0, 14.0), (10.0, 6.0));
    let drill = section.turned(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let swept = slid(k, drill, v3(0.0, 0.0, -20.0))?;
    cut(k, plate, swept)
}

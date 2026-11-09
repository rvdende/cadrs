//! A block (the box from (−10, −10, −5) to (10, 10, 0)) pocketed by an end mill (radius 3 about
//! the upright through (1.5, 0), from z = −2 to 4) turned 120° about the z axis, which passes
//! through its flat end.
//!
//! The mill is a circle extruded; what it passes through is cadrs's sweep of a solid (one turn),
//! cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, cylinder, p3, v3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-10.0, -10.0, -5.0), p3(10.0, 10.0, 0.0))?;
    let mill = cylinder(k, p3(1.5, 0.0, -2.0), v3(0.0, 0.0, 1.0), 3.0, 6.0)?;
    let swept = swung(k, mill, z_axis(), 0.0, 120.0)?;
    cut(k, b, swept)
}

//! A plate (the box from (0, −5, 0) to (20, 5, 4)) slotted by a drum (radius 2 about the line y = 0,
//! z = −5, from x = 6 to 14) pushed straight up 16, wholly through it.
//!
//! The drum is a circle extruded; what it passes through is cadrs's sweep of a solid (one slide),
//! cut from the plate.
use crate::Refusal;
use crate::bench::{block, cut, cylinder, p3, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, -5.0, 0.0), p3(20.0, 5.0, 4.0))?;
    let drum = cylinder(k, p3(6.0, 0.0, -5.0), v3(1.0, 0.0, 0.0), 2.0, 8.0)?;
    let swept = slid(k, drum, v3(0.0, 0.0, 16.0))?;
    cut(k, plate, swept)
}

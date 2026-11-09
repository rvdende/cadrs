//! A block (the box from (0, −5, 0) to (20, 5, 10)) slotted across its top by an end mill (radius 2
//! about the upright through (−5, 0), from z = 7 to 15) slid 30 along x.
//!
//! The mill is a circle extruded; what it passes through is cadrs's sweep of a solid (one slide),
//! cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, cylinder, p3, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, -5.0, 0.0), p3(20.0, 5.0, 10.0))?;
    let mill = cylinder(k, p3(-5.0, 0.0, 7.0), v3(0.0, 0.0, 1.0), 2.0, 8.0)?;
    let swept = slid(k, mill, v3(30.0, 0.0, 0.0))?;
    cut(k, b, swept)
}

//! A block (the box from (0, −10, −10) to (30, 10, 0)) engraved by a V bit (a cone about the
//! upright through (5, 0), its point at (5, 0, −2), opening at 90° to a top of radius 5 at z = 3)
//! slid 15 along x, stopping inside it at both ends.
//!
//! The bit is cadrs's cone primitive; what it passes through is cadrs's sweep of a solid (one
//! slide), cut from the block.
use crate::Refusal;
use crate::bench::{block, cone, cut, p3, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, -10.0, -10.0), p3(30.0, 10.0, 0.0))?;
    let bit = cone(k, p3(5.0, 0.0, -2.0), v3(0.0, 0.0, 1.0), 0.0, 5.0, 5.0)?;
    let swept = slid(k, bit, v3(15.0, 0.0, 0.0))?;
    cut(k, b, swept)
}

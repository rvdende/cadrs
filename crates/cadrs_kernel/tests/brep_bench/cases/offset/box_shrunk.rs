//! The box from the origin to (10, 8, 6), shrunk by 1: the solid of every point at least 1 from
//! the box's outside. Each face moves 1 in and the edges stay sharp: the box from (1, 1, 1) to
//! (9, 7, 5).
//!
//! The rectangle extruded, then cadrs's offset inward.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::offset::shrunk;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shrunk(k, b, 1.0)
}

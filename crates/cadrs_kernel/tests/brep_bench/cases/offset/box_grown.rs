//! The box from the origin to (10, 8, 6), grown by 1: the solid of every point within 1 of the
//! box. Each face moves 1 out, each edge becomes a quarter cylinder of radius 1, each corner an
//! eighth of a ball.
//!
//! The rectangle extruded, then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    grown(k, b, 1.0)
}

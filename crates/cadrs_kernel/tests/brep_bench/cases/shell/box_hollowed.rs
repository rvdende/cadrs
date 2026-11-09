//! A box hollowed, its cavity closed. The box from the origin to (10, 8, 6), hollowed to a wall 1
//! thick with no opening: the box less the box from (1, 1, 1) to (9, 7, 5), one solid bounded by two
//! shells, the cavity's facing in.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature with Hollow set (no face
//! removed).
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::hollow;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    hollow(k, b, 1.0)
}

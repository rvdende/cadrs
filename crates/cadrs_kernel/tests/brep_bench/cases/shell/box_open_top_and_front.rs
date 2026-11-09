//! A box shelled open at two faces that share an edge. The box from the origin to (10, 8, 6), shelled
//! inward to a wall 1 thick, open at its top, z = 6, and its front, y = 0: the box less the cavity from
//! (1, 0, 1) to (9, 7, 6), a bottom, a back and two sides, each 1 thick.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top and
//! front picked as the faces to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(
        k,
        b,
        &[p3(5.0, 4.0, 6.0), p3(5.0, 0.0, 3.0)],
        1.0,
        Joins::Rounded,
    )
}

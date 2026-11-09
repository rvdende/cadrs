//! A box shelled open at two opposite faces. The box from the origin to (10, 8, 6), shelled inward to a
//! wall 1 thick, open at its top, z = 6, and its bottom, z = 0: a rectangular tube, the box less the
//! box from (1, 1, 0) to (9, 7, 6), with a rim at each end.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top and
//! bottom picked as the faces to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(
        k,
        b,
        &[p3(5.0, 4.0, 6.0), p3(5.0, 4.0, 0.0)],
        1.0,
        Joins::Rounded,
    )
}

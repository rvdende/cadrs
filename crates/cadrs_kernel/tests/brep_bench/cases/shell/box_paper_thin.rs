//! A box shelled to a wall 0.001 thick. The box from the origin to (10, 8, 6), shelled inward to a
//! wall 0.001 thick, open at its top face, z = 6: the box less the box from (0.001, 0.001, 0.001) to
//! (9.999, 7.999, 6).
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(k, b, &[p3(5.0, 4.0, 6.0)], 0.001, Joins::Rounded)
}

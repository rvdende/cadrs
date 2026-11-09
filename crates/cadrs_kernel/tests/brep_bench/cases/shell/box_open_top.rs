//! A box shelled with its top open. The box from the origin to (10, 8, 6), shelled inward to a wall 1
//! thick, open at its top face, z = 6: the outside the box's other five faces, the inside each of them
//! moved 1 inward, the moved faces extended or trimmed where they meet, and running on to the opening,
//! which is left as a rim.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(k, b, &[p3(5.0, 4.0, 6.0)], 1.0, Joins::Rounded)
}

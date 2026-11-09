//! A box shelled nearly to half its width, leaving a cavity 0.02 wide. The box from the origin to
//! (10, 8, 6), shelled inward to a wall 3.99 thick, open at its top face, z = 6: the walls at y = 0 and
//! y = 8, moved, come within 0.02 of each other, so the cavity is a slot from (3.99, 3.99, 3.99) to
//! (6.01, 4.01, 6).
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(k, b, &[p3(5.0, 4.0, 6.0)], 3.99, Joins::Rounded)
}

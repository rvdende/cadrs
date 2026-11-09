//! A box shelled open at every face but one. The box from the origin to (10, 8, 6), shelled inward to
//! a wall 1 thick, open at five of its faces, every face but its bottom, z = 0: the bottom moved 1
//! inward runs on to the planes of the four sides, so the result is the slab from the origin to
//! (10, 8, 1).
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), the top and
//! the four sides picked as the faces to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(
        k,
        b,
        &[
            p3(5.0, 4.0, 6.0),
            p3(5.0, 0.0, 3.0),
            p3(5.0, 8.0, 3.0),
            p3(0.0, 4.0, 3.0),
            p3(10.0, 4.0, 3.0),
        ],
        1.0,
        Joins::Rounded,
    )
}

//! A box shelled to half its width: the walls meet and nothing is hollowed. The box from the origin to
//! (10, 8, 6), shelled inward to a wall 4 thick, open at its top face, z = 6: the walls at y = 0 and
//! y = 8, moved 4 inward, meet in the plane y = 4, the cavity is flat, and the result is the box
//! itself, its six faces as they were.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature (inward), its top picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_inward(k, b, &[p3(5.0, 4.0, 6.0)], 4.0, Joins::Rounded)
}

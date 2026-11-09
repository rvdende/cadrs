//! A box shelled outward with its top open, its edges kept sharp. The box from the origin to
//! (10, 8, 6), shelled outward to a wall 1 thick, open at its top face, z = 6: the box's other faces
//! are the inside, the outside each of them moved 1 outward and extended until they meet, cut off by
//! the plane z = 6, the rim. The box from (−1, −1, −1) to (11, 9, 6) less the box.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature, outward, its corners
//! sharp, its top picked as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_outward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_outward(k, b, &[p3(5.0, 4.0, 6.0)], 1.0, Joins::Sharp)
}

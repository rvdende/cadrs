//! A box shelled outward with its top open, its edges rounded. The box from the origin to (10, 8, 6),
//! shelled outward to a wall 1 thick, open at its top face, z = 6: the wall is every point outside the
//! box within 1 of its faces other than the top, up to the plane z = 6, so its outside is each face
//! moved 1 outward, joined at the convex edges by quarter cylinders of radius 1 and at the bottom
//! corners by eighths of a ball.
//!
//! The box is a rectangle on the Top plane extruded 6; cadrs's Shell feature, outward, its corners
//! rounded (its default joins), its top picked as the face to remove.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::shell::{Joins, shell_outward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 8.0, 6.0))?;
    shell_outward(k, b, &[p3(5.0, 4.0, 6.0)], 1.0, Joins::Rounded)
}

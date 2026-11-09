//! A ball of radius 10 about the origin and a solid cylinder of radius 5 about the line x = 5,
//! y = 0, from z = −12 to 12, whose wall touches the sphere from inside at (10, 0, 0): the two
//! united; above and below that point the cylinder comes out through the sphere.
//! The ball is cadrs's sphere primitive, its poles at (0, ±10, 0) and its seam through
//! (−10, 0, 0); the cylinder a circle extruded, its seam on the line x = 0, y = 0.
use crate::Refusal;
use crate::bench::union;
use crate::shared::tangent::{viviani_ball, viviani_cylinder};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let ball = viviani_ball(k)?;
    let bore = viviani_cylinder(k)?;
    union(k, ball, bore)
}

//! A ball cut by a plane and shelled open there: the rim meets the walls aslant. The part of the ball
//! of radius 5 centered on the origin below the plane z = 3, shelled inward to a wall 1 thick, open at
//! its flat face: the sphere moved 1 inward, the sphere of radius 4, runs on to meet the plane z = 3
//! in the circle of radius √7.
//!
//! The ball is cadrs's sphere primitive, its poles on the z axis, less the block above the plane
//! (a rectangle on that plane extruded up); cadrs's Shell feature (inward), the flat face picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

const H: f64 = 3.0;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let above = block(k, p3(-6.0, -6.0, H), p3(6.0, 6.0, 6.0))?;
    let capped = cut(k, b, above)?;
    shell_inward(k, capped, &[p3(0.5, 0.5, H)], 1.0, Joins::Rounded)
}

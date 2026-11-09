//! A ball cut by a plane and shelled open there, the cavity never reaching it. The part of the ball of
//! radius 5 centered on the origin below the plane z = 4.5, shelled inward to a wall 1 thick, open at
//! its flat face: the sphere moved 1 inward, of radius 4, lies wholly below the plane, so the cavity
//! never reaches the opening, the flat face stays, and the cavity is the closed ball of radius 4.
//!
//! The ball is cadrs's sphere primitive, its poles on the z axis, less the block above the plane
//! (a rectangle on that plane extruded up); cadrs's Shell feature (inward), the flat face picked
//! as the face to remove.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

const H: f64 = 4.5;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let above = block(k, p3(-6.0, -6.0, H), p3(6.0, 6.0, 6.0))?;
    let capped = cut(k, b, above)?;
    shell_inward(k, capped, &[p3(0.5, 0.5, H)], 1.0, Joins::Rounded)
}

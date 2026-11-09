//! A capsule's end shelled open at its dome: the walls run on past their face to meet it. A cylinder of
//! radius 5 on the z axis from z = 0 to z = 10, capped by the half ball of radius 5 about (0, 0, 10),
//! shelled inward to a wall 1 thick, open at its hemisphere: the side moved 1 inward runs on up into
//! the dome until it meets the sphere at z = 13, the bottom moved is z = 1.
//!
//! The capsule's end is cadrs's cylinder primitive united with its sphere primitive;
//! cadrs's Shell feature (inward), the dome picked as the face to remove.
use crate::Refusal;
use crate::bench::{V3, ball, cylinder, p3, union};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

/// The capsule's end: cadrs's cylinder of radius 5 from z = 0 to z = 10 united with its ball of
/// radius 5 about (0, 0, 10), the dome the ball's upper half (its lower half within the
/// cylinder).
fn capsule_end(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let side = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 5.0, 10.0)?;
    let dome = ball(k, p3(0.0, 0.0, 10.0), 5.0)?;
    union(k, side, dome)
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = capsule_end(k)?;
    shell_inward(k, c, &[p3(0.0, 0.0, 15.0)], 1.0, Joins::Rounded)
}

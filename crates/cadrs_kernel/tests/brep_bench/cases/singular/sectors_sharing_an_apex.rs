//! Two spherical sectors of a ball of radius 10, each the part of the ball within 45° of its
//! cone's axis, both with their apex at the origin: the first about the z axis toward +z, the
//! second the first turned 60° about the y axis. The solid common to both. Their caps lie on the
//! same sphere, and the two cones' sides meet in two straight lines from the apex.
//!
//! The first sector is cadrs's sphere primitive in common with its cone primitive (apex at the
//! centre, 10 up z, its rim of radius 10 clear of the ball's cap). The second is a copy turned
//! 60° about y; the two are intersected.
use crate::Refusal;
use crate::bench::{axis, ball, common, cone, p3, turned, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the sector: the ball within the cone of half-angle 45° up z from its centre (the cone
    // 10 high, its rim of radius 10 above the ball's cap)
    let origin = p3(0.0, 0.0, 0.0);
    let b = ball(k, origin, 10.0)?;
    let within = cone(k, origin, v3(0.0, 0.0, 1.0), 0.0, 10.0, 10.0)?;
    let first = common(k, b, within)?;
    let second = turned(k, first, axis(origin, v3(0.0, 1.0, 0.0)), 60.0)?;
    common(k, first, second)
}

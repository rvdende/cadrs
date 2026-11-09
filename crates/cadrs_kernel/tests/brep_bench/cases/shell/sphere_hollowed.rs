//! A ball hollowed, its cavity closed. A ball of radius 5 centered on the origin, hollowed to a wall 1
//! thick with no opening: the ball less the ball of radius 4, one solid bounded by two spheres.
//!
//! The ball is cadrs's sphere primitive, its poles on the z axis; cadrs's Shell feature with
//! Hollow set (no face removed).
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::shell::hollow;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    hollow(k, b, 1.0)
}

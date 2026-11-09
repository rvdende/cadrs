//! The solid a thick disc (radius 2 about the line through (2, 0, 0) along y, from y = 0 to 0.5)
//! fills as it turns 90° about the z axis, which passes through its rim.
//!
//! The disc is a circle extruded; what it passes through is cadrs's sweep of a solid (one
//! turn).
use crate::Refusal;
use crate::bench::{cylinder, p3, v3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let coin = cylinder(k, p3(2.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), 2.0, 0.5)?;
    swung(k, coin, z_axis(), 0.0, 90.0)
}

//! The solid a rod (radius 0.5 about the x axis from x = −3 to 5) fills as it turns 120° about
//! the z axis, which crosses it.
//!
//! The rod is a circle extruded; what it passes through is cadrs's sweep of a solid (one turn).
use crate::Refusal;
use crate::bench::{cylinder, p3, v3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let rod = cylinder(k, p3(-3.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 0.5, 8.0)?;
    swung(k, rod, z_axis(), 0.0, 120.0)
}

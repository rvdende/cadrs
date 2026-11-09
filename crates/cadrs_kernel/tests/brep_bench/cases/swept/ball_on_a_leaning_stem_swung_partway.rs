//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a ball (radius 0.8 about (10, 0, 0)) on a stem (a cylinder of radius 0.4 from the
//! ball's centre 3 along (0, sin 20°, cos 20°)) turned about the z axis from −140° to +30°.
//!
//! The tool is the ball and the stem united; what it passes through is cadrs's sweep of a solid
//! (one turn), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{ball, cut, cylinder, p3, union, v3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let a = 20f64.to_radians();
    let b = ball(k, p3(10.0, 0.0, 0.0), 0.8)?;
    let stem = cylinder(k, p3(10.0, 0.0, 0.0), v3(0.0, a.sin(), a.cos()), 0.4, 3.0)?;
    let tool = union(k, b, stem)?;
    let swept = swung(k, tool, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

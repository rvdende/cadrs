//! A ring (radii 3 to 8 about the z axis, heights ±2.5) with a channel cut inside it by a drum
//! (radius 1, length 3, its axis along (sin 25°, 0, cos 25°) through its middle (5, 1.2, 0)) turned
//! about the z axis by 100°.
//!
//! The ring is a cylinder less a cylinder; what the drum passes through is cadrs's sweep of a
//! solid (one turn), cut from it.
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outer = cylinder(k, p3(0.0, 0.0, -2.5), v3(0.0, 0.0, 1.0), 8.0, 5.0)?;
    let inner = cylinder(k, p3(0.0, 0.0, -2.5), v3(0.0, 0.0, 1.0), 3.0, 5.0)?;
    let ring = cut(k, outer, inner)?;
    let a = 25f64.to_radians();
    let dir = v3(a.sin(), 0.0, a.cos());
    let drum = cylinder(k, p3(5.0, 1.2, 0.0) - dir * 1.5, dir, 1.0, 3.0)?;
    let swept = swung(k, drum, z_axis(), 0.0, 100.0)?;
    cut(k, ring, swept)
}

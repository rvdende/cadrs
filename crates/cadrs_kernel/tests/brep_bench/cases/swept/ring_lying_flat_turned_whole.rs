//! The solid a ring (the torus about the upright through (8, 0), radii 3 and 1, centered on z = 0)
//! fills as it turns a whole turn about the z axis.
//!
//! The ring is cadrs's torus primitive; what it passes through is cadrs's sweep of a solid (one
//! whole turn).
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let ring = torus(k, p3(8.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0, 1.0)?;
    swung(k, ring, z_axis(), 0.0, 360.0)
}

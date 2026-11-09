//! The solid a ring (the torus about the z axis, radii 6 and 2) fills as it slides along the x axis
//! by 15, farther than its hole is wide.
//!
//! cadrs's sweep of a solid (one slide).
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let ring = torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 6.0, 2.0)?;
    slid(k, ring, v3(15.0, 0.0, 0.0))
}

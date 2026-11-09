//! A torus about the z axis centered on the origin, its tube's center circle of radius 10 and its
//! tube of radius 2, with everything where x < 0 cut away by a block. The torus is cadrs's
//! torus primitive.
use crate::Refusal;
use crate::bench::{V3, block, cut, p3, torus};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let ring = torus(k, p3(0.0, 0.0, 0.0), V3::z(), 10.0, 2.0)?;
    let left = block(k, p3(-20.0, -20.0, -10.0), p3(0.0, 20.0, 10.0))?;
    cut(k, ring, left)
}

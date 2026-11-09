//! A torus about the z axis (the middle circle of its tube of radius 10, the tube of radius 3),
//! turned 45° about z so that its seam is off the points of contact, united with a slab from
//! (−13, −13, −1) to (13, 13, 1) whose four sides its outer equator touches. The torus is
//! cadrs's torus primitive.
use crate::Refusal;
use crate::bench::{V3, axis, block, p3, torus, turned, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let o = p3(0.0, 0.0, 0.0);
    let ring = torus(k, o, V3::z(), 10.0, 3.0)?;
    let ring = turned(k, ring, axis(o, V3::z()), 45.0)?;
    let slab = block(k, p3(-13.0, -13.0, -1.0), p3(13.0, 13.0, 1.0))?;
    union(k, slab, ring)
}

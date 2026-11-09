//! A solid horn torus centered on the origin, about the z axis: the circle through the middle of
//! its tube has radius 5, and the tube has radius 5 too, so the tube closes its hole to a single
//! point, the origin.
//!
//! cadrs's torus primitive (`Primitive::Torus`), its tube as wide as its ring.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 5.0)
}

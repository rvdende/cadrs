//! A solid torus centered on the origin, about the z axis: the circle through the middle of its
//! tube has radius 5, and the tube has radius 4.99, so its hole is a circle of radius 0.01 about
//! the axis.
//!
//! The torus is cadrs's torus primitive, its seam in the plane x = 0 on the side y > 0.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 4.99)
}

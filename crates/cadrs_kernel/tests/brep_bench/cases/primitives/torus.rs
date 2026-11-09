//! A solid torus centered on the origin, about the z axis, of radius 10 to the middle of its tube
//! and 2 across the tube: cadrs's torus primitive, its seam in the plane x = 0 on the side y > 0.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 10.0, 2.0)
}

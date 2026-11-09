//! A truncated cone on the z axis, from z = 0 to z = 6, of radius 5 at its base and 2 at its top:
//! cadrs's cone primitive.
use crate::Refusal;
use crate::bench::{cone, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    cone(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 2.0, 6.0)
}

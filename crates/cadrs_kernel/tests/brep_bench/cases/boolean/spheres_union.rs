//! Two balls of radius 5, centered on the origin and at (6, 0, 0), united. Each ball is cadrs's
//! sphere primitive.
use crate::Refusal;
use crate::bench::{ball, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let b = ball(k, p3(6.0, 0.0, 0.0), 5.0)?;
    union(k, a, b)
}

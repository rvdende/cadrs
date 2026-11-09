//! Two balls of radius 5, centered on the origin and at (6, 0, 0): the lens common to both. Each
//! ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, common, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let b = ball(k, p3(6.0, 0.0, 0.0), 5.0)?;
    common(k, a, b)
}

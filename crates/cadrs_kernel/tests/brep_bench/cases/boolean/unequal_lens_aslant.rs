//! The lens common to a ball of radius 5.511 about the origin and one of radius 3.317 about
//! (4.231, 3.467, 2.653). Each ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, common, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = ball(k, p3(0.0, 0.0, 0.0), 5.511)?;
    let b = ball(k, p3(4.231, 3.467, 2.653), 3.317)?;
    common(k, a, b)
}

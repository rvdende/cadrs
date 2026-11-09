//! A solid ball of radius 5 centered on the origin: cadrs's sphere primitive, its poles on the z
//! axis and its seam toward +x.
use crate::Refusal;
use crate::bench::{ball, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    ball(k, p3(0.0, 0.0, 0.0), 5.0)
}

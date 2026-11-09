//! A ball of radius 5 about the origin, its poles on the z axis, with everything below the plane
//! z = 4.9999 cut away by a block on that plane: the cap 10⁻⁴ high around the top pole. The ball
//! is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{V3, ball, p3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    cut_beyond(k, b, p3(0.0, 0.0, 4.9999), -V3::z())
}

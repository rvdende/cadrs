//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, less a ball of radius 5 centered
//! at (0, 0, 10), the middle of its top: the hemispherical dish meets the wall tangentially
//! along the top rim, a knife edge. The ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, cut, cylinder, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let b = ball(k, p3(0.0, 0.0, 10.0), 5.0)?;
    cut(k, drum, b)
}

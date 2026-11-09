//! A ball of radius 5 centered on the origin, bored right through along the z axis by a hole of
//! radius 3: a circle extruded from z = −6 to z = 6, cut away. The ball is cadrs's sphere
//! primitive.
use crate::Refusal;
use crate::bench::{V3, ball, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let bore = cylinder(k, p3(0.0, 0.0, -6.0), V3::z(), 3.0, 12.0)?;
    cut(k, b, bore)
}

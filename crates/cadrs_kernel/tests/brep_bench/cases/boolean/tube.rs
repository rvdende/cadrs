//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, bored along its axis by a hole of
//! radius 3: a cylinder from z = −1 to z = 11, past both ends, cut away.
use crate::Refusal;
use crate::bench::{V3, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outer = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 5.0, 10.0)?;
    let bore = cylinder(k, p3(0.0, 0.0, -1.0), V3::z(), 3.0, 12.0)?;
    cut(k, outer, bore)
}

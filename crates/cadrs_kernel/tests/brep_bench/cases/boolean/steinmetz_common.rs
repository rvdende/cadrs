//! Two cylinders of radius 5 and length 30 crossing at right angles, one along the x axis from
//! x = −15 to 15 and one along the y axis from y = −15 to 15: the solid common to both. Each is a
//! circle extruded.
use crate::Refusal;
use crate::bench::{V3, common, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = cylinder(k, p3(-15.0, 0.0, 0.0), V3::x(), 5.0, 30.0)?;
    let b = cylinder(k, p3(0.0, -15.0, 0.0), V3::y(), 5.0, 30.0)?;
    common(k, a, b)
}

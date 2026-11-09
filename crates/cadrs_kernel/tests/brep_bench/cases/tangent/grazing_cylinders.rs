//! Two solid cylinders of radius 5 and length 30 crossing at right angles and barely
//! overlapping: the first along the x axis from x = −15 to 15, the second parallel to the y axis
//! from y = −15 to 15 with its axis on the line x = 0, z = 9.99. Their common part, a cushion
//! 0.01 thick. Each cylinder is a circle extruded.
use crate::Refusal;
use crate::bench::{common, cylinder, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let first = cylinder(k, p3(-15.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 5.0, 30.0)?;
    let second = cylinder(k, p3(0.0, -15.0, 9.99), v3(0.0, 1.0, 0.0), 5.0, 30.0)?;
    common(k, first, second)
}

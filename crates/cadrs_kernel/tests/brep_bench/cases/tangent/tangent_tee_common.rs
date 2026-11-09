//! Two solid cylinders crossing at right angles: the first, of radius 5, along the x axis from
//! x = −15 to 15; the second, of radius 3, parallel to the y axis from y = −15 to 15, its axis on
//! the line x = 0, z = 2, so that its top touches the first's from inside at (0, 0, 5). Their
//! common part. Each cylinder is a circle extruded.
use crate::Refusal;
use crate::bench::{common, cylinder, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let main = cylinder(k, p3(-15.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 5.0, 30.0)?;
    let branch = cylinder(k, p3(0.0, -15.0, 2.0), v3(0.0, 1.0, 0.0), 3.0, 30.0)?;
    common(k, main, branch)
}

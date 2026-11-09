//! A cube of side 10 between the origin and (10, 10, 10), drilled right through along z by a hole
//! of radius 3 about x = 5, y = 5: the cylinder that cuts it is a circle extruded from z = −1 to
//! z = 11, past both faces.
use crate::Refusal;
use crate::bench::{V3, block, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let drill = cylinder(k, p3(5.0, 5.0, -1.0), V3::z(), 3.0, 12.0)?;
    cut(k, cube, drill)
}

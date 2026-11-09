//! A cube of side 10 from (−5, −5, 0) to (5, 5, 10), united with a copy of itself turned 0.001°
//! about the z axis: a prism over a sixteen-sided polygon, its sides crossing at 0.001°.
use crate::Refusal;
use crate::bench::{axis, block, p3, turned, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(-5.0, -5.0, 0.0), p3(5.0, 5.0, 10.0))?;
    let hair = turned(k, cube, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), 0.001)?;
    union(k, cube, hair)
}

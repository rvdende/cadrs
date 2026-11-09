//! A cube from (−5, −5, −5) to (5, 5, 5) united with a cylinder of radius 5 on the x axis from
//! x = −10 to x = 10, inscribed in the cube between its faces x = ±5. The cylinder is a circle on
//! the plane x = −10 extruded 20; the sketch's x axis runs along (0, 1, 1), so the circle's seam
//! lies on the line y = z of the wall, 45° from the lines where it touches the cube.
use crate::Refusal;
use crate::bench::{V3, block, extrude, p3, plane, round, sketch, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(-5.0, -5.0, -5.0), p3(5.0, 5.0, 5.0))?;
    let on = plane(p3(-10.0, 0.0, 0.0), v3(0.0, 1.0, 1.0), V3::x());
    let rod = extrude(k, &sketch(on, vec![round((0.0, 0.0), 5.0)]), 20.0)?;
    union(k, cube, rod)
}

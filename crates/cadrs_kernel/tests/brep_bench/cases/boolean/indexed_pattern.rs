//! A round disk of radius 20 on the z axis from z = −5 to z = 0, with six holes of radius 2
//! through it along z, cut one after another: the first a circle about (12, 0) extruded from
//! z = −6 to z = 1, past both faces, and the others that cylinder turned about the z axis by 60°,
//! 120°, … 300°.
use crate::Refusal;
use crate::bench::{V3, axis, cut, cylinder, extrude, p3, round, sketch, turned, xy};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut disk = extrude(k, &sketch(xy(-5.0), vec![round((0.0, 0.0), 20.0)]), 5.0)?;
    let first = cylinder(k, p3(12.0, 0.0, -6.0), V3::z(), 2.0, 7.0)?;
    for i in 0..6 {
        let hole = if i == 0 {
            first
        } else {
            turned(k, first, axis(p3(0.0, 0.0, 0.0), V3::z()), 60.0 * i as f64)?
        };
        disk = cut(k, disk, hole)?;
    }
    Ok(disk)
}

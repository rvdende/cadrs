//! A plate from the origin to (100, 20, 10), drilled through along z by nine holes of radius 2
//! at y = 10 and x = 10, 20, … 90, one cut after another: each hole a circle extruded from
//! z = −1 to z = 11, past both faces.
use crate::Refusal;
use crate::bench::{V3, block, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut plate = block(k, p3(0.0, 0.0, 0.0), p3(100.0, 20.0, 10.0))?;
    for i in 1..=9 {
        let drill = cylinder(k, p3(10.0 * i as f64, 10.0, -1.0), V3::z(), 2.0, 12.0)?;
        plate = cut(k, plate, drill)?;
    }
    Ok(plate)
}

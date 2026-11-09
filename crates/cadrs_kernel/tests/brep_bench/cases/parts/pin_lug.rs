//! A flat lug, the plate from (0, −5, 0) to (80, 5, 40), with a pin hole of radius 8 straight
//! through its thickness along y about the line x = 60, z = 20: a circle extruded from y = −6 to
//! y = 6, past both faces, cut away.
use crate::Refusal;
use crate::bench::{V3, block, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, -5.0, 0.0), p3(80.0, 5.0, 40.0))?;
    let pin = cylinder(k, p3(60.0, -6.0, 20.0), V3::y(), 8.0, 12.0)?;
    cut(k, plate, pin)
}

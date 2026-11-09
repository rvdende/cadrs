//! A rod, the cylinder of radius 5 about the z axis from z = 0 to z = 20, grooved by a wheel slid
//! past it: the torus of radii 6 and 2, its axis parallel to y, slid without turning from center
//! (11.5, 0, −9) up to (11.5, 0, 29), cut from the rod.
//!
//! The wheel is cadrs's torus primitive; its slide is cadrs's sweep of a solid
//! (`Kernel::cut_swept`, one slide of 38 up the z axis), cut from the rod.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use crate::shared::swept::{cut_swept, rod, slide};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let body = rod(k)?;
    let wheel = torus(k, p3(11.5, 0.0, -9.0), v3(0.0, 1.0, 0.0), 6.0, 2.0)?;
    cut_swept(k, body, wheel, &slide(v3(0.0, 0.0, 38.0)))
}

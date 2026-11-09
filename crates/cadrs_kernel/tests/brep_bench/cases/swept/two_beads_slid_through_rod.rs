//! A rod, the cylinder of radius 5 about the z axis from z = 0 to z = 20, grooved by a wheel of
//! two beads slid past it: tori on one axis parallel to y, the first centered (11.5, 0, −9) of
//! radii 6 and 2, the second centered (11.5, 1.5, −9) of radii 6 and 1.5, their union slid
//! without turning 40 up the z axis, cut from the rod.
//!
//! Each bead is cadrs's torus primitive, the wheel their union; its slide is cadrs's sweep of a
//! solid (`Kernel::cut_swept`, one slide of 40 up the z axis), cut from the rod.
use crate::Refusal;
use crate::bench::{p3, torus, union, v3};
use crate::shared::swept::{cut_swept, rod, slide};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let body = rod(k)?;
    let first = torus(k, p3(11.5, 0.0, -9.0), v3(0.0, 1.0, 0.0), 6.0, 2.0)?;
    let second = torus(k, p3(11.5, 1.5, -9.0), v3(0.0, 1.0, 0.0), 6.0, 1.5)?;
    let wheel = union(k, first, second)?;
    cut_swept(k, body, wheel, &slide(v3(0.0, 0.0, 40.0)))
}

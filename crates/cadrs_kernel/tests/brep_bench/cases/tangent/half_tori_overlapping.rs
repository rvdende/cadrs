//! Two halves of the torus about the z axis (the middle circle of its tube of radius 10, the tube
//! of radius 3), united: the half with z ≥ 0, and the half with x ≥ 0 of the torus turned a quarter
//! turn about z, so that its seam is elsewhere. Each torus is cadrs's torus primitive, each half
//! cut from it by a block on a plane through the origin.
use crate::Refusal;
use crate::bench::{V3, axis, p3, torus, turned, union};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let o = p3(0.0, 0.0, 0.0);
    let first = torus(k, o, V3::z(), 10.0, 3.0)?;
    let first = cut_beyond(k, first, o, -V3::z())?;
    let second = torus(k, o, V3::z(), 10.0, 3.0)?;
    let second = turned(k, second, axis(o, V3::z()), 90.0)?;
    let second = cut_beyond(k, second, o, -V3::x())?;
    union(k, first, second)
}

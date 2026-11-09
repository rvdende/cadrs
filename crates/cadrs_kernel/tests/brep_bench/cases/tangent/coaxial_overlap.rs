//! Two cylinders of radius 5 on the z axis, the first from z = 0 to z = 10 and the second from
//! z = 6 to z = 16, united: their walls coincide between z = 6 and z = 10. Each is a circle
//! extruded; the second is turned a quarter turn about z so that the two walls' seams are not
//! in the same place.
use crate::Refusal;
use crate::bench::{axis, cylinder, p3, turned, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let up = v3(0.0, 0.0, 1.0);
    let first = cylinder(k, p3(0.0, 0.0, 0.0), up, 5.0, 10.0)?;
    let second = cylinder(k, p3(0.0, 0.0, 6.0), up, 5.0, 10.0)?;
    let second = turned(k, second, axis(p3(0.0, 0.0, 0.0), up), 90.0)?;
    union(k, first, second)
}

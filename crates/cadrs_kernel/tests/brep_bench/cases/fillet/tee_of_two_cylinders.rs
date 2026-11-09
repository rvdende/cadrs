//! A tee of two solid cylinders (the bar of radius 6 about the x axis from x = −20 to 20, the
//! stem of radius 3 about the z axis from z = 0 to 15) united, the saddle-shaped junction rounded
//! to radius 1.
//!
//! Two circles extruded and united; the junction picked by its point over (3, 0), at z = 6, and
//! filleted.
use crate::Refusal;
use crate::bench::{cylinder, edge_at, p3, union, v3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = cylinder(k, p3(-20.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), 6.0, 40.0)?;
    let stem = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0, 15.0)?;
    let tee = union(k, bar, stem)?;
    let junction = edge_at(k, tee, p3(3.0, 0.0, 6.0))?;
    fillet(k, tee, &[junction], 1.0)
}

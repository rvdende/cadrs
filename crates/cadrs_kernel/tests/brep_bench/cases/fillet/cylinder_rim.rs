//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, the circular edge around its top
//! rounded to radius 1.
//!
//! The cylinder is a circle extruded; its top rim is picked by a point on it and filleted,
//! radius 1.
use crate::Refusal;
use crate::bench::{cylinder, edge_at, p3, v3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let rim = edge_at(k, c, p3(5.0, 0.0, 10.0))?;
    fillet(k, c, &[rim], 1.0)
}

//! A plate from the origin to (20, 10, 2), the two edges of its face y = 10 that run along x (at
//! z = 0 and z = 2) rounded to radius 0.9999: what is left of the face y = 10 is a strip 20 long
//! and 0.0002 tall between the two rounds, tangent to both.
//!
//! The plate is a rectangle extruded; both edges are picked by a point on each and rounded in
//! one Fillet feature, radius 0.9999.
use crate::Refusal;
use crate::bench::{block, edges_at, p3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 10.0, 2.0))?;
    let edges = edges_at(k, plate, &[p3(10.0, 10.0, 0.0), p3(10.0, 10.0, 2.0)])?;
    fillet(k, plate, &edges, 0.9999)
}

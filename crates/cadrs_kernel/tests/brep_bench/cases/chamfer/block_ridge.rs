//! A block from the origin to (10, 4, 6), the two edges of its top face that run along x (at
//! y = 0 and y = 4) chamfered with the same distance 2 on both faces: half the top's width, so
//! the top vanishes and the two chamfers meet in a ridge along y = 2, z = 6.
//!
//! The block is a rectangle extruded; both edges are picked by a point on each and chamfered in
//! one Chamfer feature, equal distance 2.
use crate::Refusal;
use crate::bench::{block, edges_at, p3};
use crate::shared::chamfer::chamfer;
use cadrs_kernel::{BodyId, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 4.0, 6.0))?;
    let edges = edges_at(k, b, &[p3(5.0, 0.0, 6.0), p3(5.0, 4.0, 6.0)])?;
    chamfer(k, b, &edges, ChamferSpec::EqualDistance(2.0))
}

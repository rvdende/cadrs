//! A cube of side 10 with all twelve of its edges rounded to radius 1, three rounds meeting at
//! each corner.
//!
//! The cube, from the origin to (10, 10, 10), is a square extruded; its twelve edges are rounded
//! in one Fillet feature, radius 1, which closes each corner with a patch of a ball.
use crate::Refusal;
use crate::bench::{all_edges, block, p3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let edges = all_edges(k, cube)?;
    fillet(k, cube, &edges, 1.0)
}

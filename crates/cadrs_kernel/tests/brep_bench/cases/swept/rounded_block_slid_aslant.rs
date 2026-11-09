//! The solid a rounded block (the box from the origin to (10, 6, 4), every edge rounded to radius
//! 1 and every corner to a ball) fills as it slides along (8, 5, 3).
//!
//! The block's twelve edges are filleted together (its corners balls); what it passes through is
//! cadrs's sweep of a solid (one slide).
use crate::Refusal;
use crate::bench::{all_edges, block, p3, v3};
use crate::shared::fillet::fillet;
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 6.0, 4.0))?;
    let edges = all_edges(k, b)?;
    let rounded = fillet(k, b, &edges, 1.0)?;
    slid(k, rounded, v3(8.0, 5.0, 3.0))
}

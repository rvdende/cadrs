//! The cube from the origin to (20, 20, 20) less the cube from (10, 10, 10) to (20, 20, 20), the
//! notch's three concave edges rounded to radius 3 in one fillet (the inside corner an eighth of
//! a ball of radius 3).
//!
//! The cube less the notch's box; the three edges picked by their middles and filleted together.
use crate::Refusal;
use crate::bench::{block, cut, edges_at, p3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 20.0, 20.0))?;
    let notch = block(k, p3(10.0, 10.0, 10.0), p3(20.0, 20.0, 20.0))?;
    let notched = cut(k, cube, notch)?;
    let edges = edges_at(
        k,
        notched,
        &[
            p3(15.0, 10.0, 10.0),
            p3(10.0, 15.0, 10.0),
            p3(10.0, 10.0, 15.0),
        ],
    )?;
    fillet(k, notched, &edges, 3.0)
}

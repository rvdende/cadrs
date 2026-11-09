//! A sheet: the half of the cylinder of radius 5 about the z axis where y ≥ 0, from z = 0 to
//! z = 10, thickened by 2 toward the axis.
//!
//! The half circle of radius 5 about the origin from (5, 0) to (−5, 0) through (0, 5) sketched on
//! the Top plane as an open chain and surface-extruded 10 along +z; cadrs's Thicken thickens it
//! with its arrow flipped to point toward the axis.
use crate::Refusal;
use crate::bench::{arc, p2, p3, v3, xy};
use crate::shared::thicken::{open_chain, surface_extrude, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xy(0.0), vec![arc(p2(0.0, 0.0), 5.0, 0.0, 180.0)]);
    let sheet = surface_extrude(k, &s, v3(0.0, 0.0, 1.0), 10.0)?;
    thicken_toward(k, sheet, p3(0.0, 5.0, 5.0), v3(0.0, -1.0, 0.0), 2.0)
}

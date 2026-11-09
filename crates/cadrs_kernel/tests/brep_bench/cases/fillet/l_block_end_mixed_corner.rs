//! An L-block (the foot from the origin to (20, 10, 6) and the upright from the origin to
//! (6, 10, 20), united): its concave edge along y at x = 6, z = 6 rounded to radius 2, then the
//! tangent chain that makes of the end face's edges (up the upright, round the concave round's
//! quarter circle, along the foot) rounded to radius 2, convex.
//!
//! Two Fillet features one after another, as the case composes the corner: the concave edge,
//! then the chain, picked by its straight edge up the upright (tangent propagation takes the
//! rest).
use crate::Refusal;
use crate::bench::{block, edge_at, p3, union};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let foot = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 10.0, 6.0))?;
    let upright = block(k, p3(0.0, 0.0, 0.0), p3(6.0, 10.0, 20.0))?;
    let l = union(k, foot, upright)?;
    let inside = edge_at(k, l, p3(6.0, 5.0, 6.0))?;
    let rounded = fillet(k, l, &[inside], 2.0)?;
    let chain = edge_at(k, rounded, p3(6.0, 0.0, 14.0))?;
    fillet(k, rounded, &[chain], 2.0)
}

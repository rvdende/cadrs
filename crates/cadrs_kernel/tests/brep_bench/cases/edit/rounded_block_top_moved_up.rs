//! A block from the origin to (40, 30, 10), the edge where its top (z = 10) meets its back
//! (y = 30) rounded to radius 3; then its top moved up by 6 to z = 16, carrying the round with
//! it unstretched (its axis now the line y = 27, z = 13), the back and the ends running on up to
//! meet them: the box to (40, 30, 16) with that edge rounded to radius 3.
//!
//! The block is a rectangle extruded and the edge filleted, radius 3; then Move face translates
//! the top and the round together by 6 along +z.
use crate::Refusal;
use crate::bench::{block, edge_at, faces_at, p3, v3};
use crate::shared::edit::move_faces;
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 10.0))?;
    let edge = edge_at(k, b, p3(20.0, 30.0, 10.0))?;
    let rounded = fillet(k, b, &[edge], 3.0)?;
    // the top, and the round at its middle, 3 from its axis at 45°
    let s = 3.0 * std::f64::consts::FRAC_1_SQRT_2;
    let faces = faces_at(
        k,
        rounded,
        &[p3(20.0, 10.0, 10.0), p3(20.0, 27.0 + s, 7.0 + s)],
    )?;
    move_faces(k, rounded, &faces, v3(0.0, 0.0, 6.0))
}

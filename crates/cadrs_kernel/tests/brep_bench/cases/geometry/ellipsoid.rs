//! A solid ellipsoid centered on the origin, its semi-axes 6 along x, 4 along y and 3 along z:
//! every point with x²/36 + y²/16 + z²/9 ≤ 1. No two semi-axes are equal, so it is no surface of
//! revolution.
//!
//! A ball of radius 1 (cadrs's sphere primitive) stretched by 6, 4 and 3 along x, y and z with
//! cadrs's `Kernel::scale_axes` (Onshape's Transform, Scale non-uniformly): the sphere becomes
//! its exact rational B-spline, stretched.
use crate::Refusal;
use crate::bench::{ball, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 1.0)?;
    let r = k.scale_axes(b, p3(0.0, 0.0, 0.0), v3(6.0, 4.0, 3.0))?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the scale made no body"))
}

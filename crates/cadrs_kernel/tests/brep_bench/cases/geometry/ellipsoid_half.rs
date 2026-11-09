//! The ellipsoid x²/36 + y²/16 + z²/9 ≤ 1 (semi-axes 6 along x, 4 along y and 3 along z) with
//! everything below the plane z = 0 cut away: a dome on the flat ellipse x²/36 + y²/16 ≤ 1,
//! rising to 3.
//!
//! The upper half of a ball of radius 1 (the ball cut by a block below z = 0), stretched by 6, 4
//! and 3 along x, y and z with cadrs's `Kernel::scale_axes`: the floor stays in z = 0.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3, v3};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 1.0)?;
    let below = block(k, p3(-2.0, -2.0, -2.0), p3(2.0, 2.0, 0.0))?;
    let half = cut(k, b, below)?;
    let r = k.scale_axes(half, p3(0.0, 0.0, 0.0), v3(6.0, 4.0, 3.0))?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the scale made no body"))
}

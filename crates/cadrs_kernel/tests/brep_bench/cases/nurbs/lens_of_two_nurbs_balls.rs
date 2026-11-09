//! The common part of two balls of radius 5 centered at (∓3, 0, 0), each bounded by one rational
//! B-spline surface, the standard biquadratic sphere of `geometry/nurbs-sphere` (poles on the
//! vertical through its center, its seam on the +x side): a lens of two caps of height 2.
//!
//! Each ball is made as `geometry/nurbs-sphere` makes its own: cadrs's sphere primitive healed
//! with `HealOp::ToBSpline` (the same net, its knots in radians). The two are intersected.
use crate::Refusal;
use crate::bench::{ball, common, p3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, HealOp, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut balls = Vec::new();
    for x in [-3.0, 3.0] {
        let b = ball(k, p3(x, 0.0, 0.0), 5.0)?;
        balls.push(first(Kernel::heal(k, b, &[HealOp::ToBSpline])?)?);
    }
    common(k, balls[0], balls[1])
}

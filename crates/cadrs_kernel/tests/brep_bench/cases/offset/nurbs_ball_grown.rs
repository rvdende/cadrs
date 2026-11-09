//! The ball of radius 5 about the origin bounded by one rational B-spline surface (the standard
//! biquadratic sphere of `geometry/nurbs-sphere`) grown by 2: its face offset out along its
//! normal, through its poles, to the ball of radius 7.
//!
//! The ball as `geometry/nurbs-sphere` makes it (cadrs's sphere primitive healed with
//! `HealOp::ToBSpline`), then offset outward by 2 (cadrs's offset of a solid).
use crate::Refusal;
use crate::bench::{ball, p3};
use crate::shared::offset::grown;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, HealOp, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let spline = first(Kernel::heal(k, b, &[HealOp::ToBSpline])?)?;
    grown(k, spline, 2.0)
}

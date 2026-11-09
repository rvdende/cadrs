//! What the cadrs implementations of the tangent cases share: the ball and the cylinder of the
//! three Viviani cases, each sketched so that its seam (and the ball's poles) stay off Viviani's
//! curve, as the cases ask.
use crate::Refusal;
use crate::bench::{extrude, p3, plane, round, sketch, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

/// The ball of radius 10 about the origin with its poles on the y axis and its seam through
/// (−10, 0, 0) (cadrs's sphere primitive).
pub fn viviani_ball(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    crate::bench::ball_oriented(
        k,
        p3(0.0, 0.0, 0.0),
        10.0,
        v3(0.0, 1.0, 0.0),
        v3(-1.0, 0.0, 0.0),
    )
}

/// The cylinder of radius 5 about the line x = 5, y = 0, from z = −12 to z = 12: the circle on
/// the plane z = −12, its sketch x axis along −x so that the circle's seam is on the line
/// x = 0, y = 0, extruded 24 up.
pub fn viviani_cylinder(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let on = plane(p3(5.0, 0.0, -12.0), v3(-1.0, 0.0, 0.0), v3(0.0, 0.0, 1.0));
    extrude(k, &sketch(on, vec![round((0.0, 0.0), 5.0)]), 24.0)
}

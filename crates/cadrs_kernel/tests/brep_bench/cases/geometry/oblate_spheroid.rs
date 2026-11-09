//! A solid oblate spheroid centered on the origin, about the z axis: the ellipsoid
//! (x² + y²)/36 + z²/9 ≤ 1, of radius 6 at its equator and reaching to z = ±3 at its poles.
//!
//! Built as the description puts it: on the Front (xz) plane, the half of the ellipse
//! x²/36 + z²/9 = 1 with x ≥ 0 (an ellipse arc, semi-axes 6 along x and 3 along z, from the
//! south pole to the north) closed by the z axis, revolved a whole turn about z.
use crate::Refusal;
use crate::bench::{axis, chain, line, p2, p3, revolve, sketch, v3, xz};
use crate::shared::geometry::ellipse_arc;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let half = chain(vec![
        ellipse_arc((0.0, 0.0), 6.0, 3.0, -90.0, 180.0),
        line(p2(0.0, 3.0), p2(0.0, -3.0)),
    ]);
    let s = sketch(xz(0.0), vec![half]);
    revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)
}

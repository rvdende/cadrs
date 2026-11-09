//! A round drum of radius 8 about the z axis, its floor flat at z = 0 and its top the saddle
//! z = 10 + xy/8: every point with x² + y² ≤ 64 and 0 ≤ z ≤ 10 + xy/8. The saddle is the B-spline
//! surface of degree 1 each way whose four control points are (−10, −10, 22.5),
//! (10, −10, −2.5), (−10, 10, −2.5) and (10, 10, 22.5); the drum's top is the part of it inside
//! the circle of radius 8, trimmed by the drum's side.
//!
//! A circle of radius 8 on the Top plane extruded 20, past the saddle everywhere above it; its top
//! face then replaced by the saddle, made from its control net (cadrs's `NurbsSurface`), which
//! trims the drum's side where the saddle meets it.
use crate::Refusal;
use crate::shared::geometry::saddle_drum;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    saddle_drum(k)
}

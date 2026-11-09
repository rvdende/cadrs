//! The drum of `geometry/saddle-drum`, bored through along its axis: every point with
//! 9 ≤ x² + y² ≤ 64 and 0 ≤ z ≤ 10 + xy/8. Its top is the saddle, the bilinear B-spline patch on
//! (−10, −10, 22.5), (10, −10, −2.5), (−10, 10, −2.5) and (10, 10, 22.5), between the circles of
//! radius 3 and 8: a B-spline face trimmed by an outer loop and with a hole.
//!
//! The drum as in `geometry/saddle-drum` (a circle of radius 8 extruded, its top face replaced by
//! the saddle made from its control net), then the bore cut with a cylinder of radius 3 about the
//! z axis from z = −1 to z = 21, past the floor and the saddle.
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::geometry::saddle_drum;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = saddle_drum(k)?;
    let bore = cylinder(k, p3(0.0, 0.0, -1.0), v3(0.0, 0.0, 1.0), 3.0, 22.0)?;
    cut(k, drum, bore)
}

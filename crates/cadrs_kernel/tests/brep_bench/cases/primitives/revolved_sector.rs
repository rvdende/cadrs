//! The rectangle with x from 6 to 10 and z from 0 to 4, sketched on the Front (xz) plane, revolved
//! 90° about the z axis, right-handed: from +x toward +y.
use crate::Refusal;
use crate::bench::{axis, p3, rectangle, revolve, sketch, v3, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch(xz(0.0), vec![rectangle(6.0, 0.0, 10.0, 4.0)]);
    revolve(
        k,
        &s,
        axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),
        Some(90.0),
    )
}

//!
//! cadrs's solid Loft between two sections, each the square sketched on its plane square to the y
//! axis with its square hole (a region with a hole): the loft through the outer squares less the
//! loft through the holes (two sections give ruled faces).
use crate::Refusal;
use crate::bench::{p3, plane, rectangle, sketch_with_holes, v3};
use crate::shared::loft::loft;
use cadrs_kernel::{BodyId, DefaultKernel, Profile};

/// The square of half-side `half` with its hole of half-side `hole` about the y axis, on the plane
/// at `y`.
fn section(y: f64, half: f64, hole: f64) -> Profile {
    let across = plane(p3(0.0, y, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0));
    sketch_with_holes(
        across,
        rectangle(-half, -half, half, half),
        vec![rectangle(-hole, -hole, hole, hole)],
    )
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    loft(k, vec![section(0.0, 12.0, 10.0), section(40.0, 6.0, 4.0)])
}

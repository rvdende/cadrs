//! A pipe of radius 8 along the cubic Bézier curve in space with control points (0, 0, 0),
//! (12, 16, 0), (26, 8, 0) and (24, 0, 16), the section square to the curve, the ends flat: the
//! circle drawn on the plane through the origin square to the curve there, along (3, 4, 0), swept
//! along the Bézier curve (a curve in space, as cadrs's sweep takes one).
use crate::Refusal;
use crate::bench::{p3, round, sketch, square_to, v3};
use crate::shared::sweep::{bezier, sweep};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let curve = bezier(&[
        p3(0.0, 0.0, 0.0),
        p3(12.0, 16.0, 0.0),
        p3(26.0, 8.0, 0.0),
        p3(24.0, 0.0, 16.0),
    ]);
    let disk = sketch(
        square_to(p3(0.0, 0.0, 0.0), v3(3.0, 4.0, 0.0)),
        vec![round((0.0, 0.0), 8.0)],
    );
    sweep(k, &disk, vec![curve])
}

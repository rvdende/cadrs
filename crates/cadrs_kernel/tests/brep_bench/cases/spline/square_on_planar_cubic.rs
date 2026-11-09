//! A bar of square section 20 × 20 along the planar cubic Bézier curve with control points
//! (0, 0, 0), (16, 12, 0), (18, 26, 0) and (12, 34, 0), the section square to the curve and
//! centered on it, two sides horizontal, the ends flat: the square drawn on the plane through
//! the origin square to the curve there, along (4, 3, 0), its sketch x axis horizontal, swept
//! along the Bézier curve drawn on the Top plane.
use crate::Refusal;
use crate::bench::{p2, p3, plane, rectangle, sketch, v3, xy};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, Curve2, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let curve = Curve2::Bezier {
        poles: [p2(0.0, 0.0), p2(16.0, 12.0), p2(18.0, 26.0), p2(12.0, 34.0)],
        source: None,
    };
    let across = plane(p3(0.0, 0.0, 0.0), v3(-3.0, 4.0, 0.0), v3(4.0, 3.0, 0.0));
    let square = sketch(across, vec![rectangle(-10.0, -10.0, 10.0, 10.0)]);
    sweep(k, &square, vec![on(xy(0.0), curve)])
}

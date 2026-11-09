//! A pipe of radius 20 along the planar cubic Bézier curve with control points (0, 0, 0),
//! (12, −16, 0), (32, −16, 0) and (44, 0, 0), the section square to the curve, the ends flat:
//! the circle drawn on the plane through the origin square to the curve there, along (3, −4, 0),
//! swept along the Bézier curve drawn on the Top plane.
use crate::Refusal;
use crate::bench::{p2, p3, round, sketch, square_to, v3, xy};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, Curve2, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let curve = Curve2::Bezier {
        poles: [
            p2(0.0, 0.0),
            p2(12.0, -16.0),
            p2(32.0, -16.0),
            p2(44.0, 0.0),
        ],
        source: None,
    };
    let disk = sketch(
        square_to(p3(0.0, 0.0, 0.0), v3(3.0, -4.0, 0.0)),
        vec![round((0.0, 0.0), 20.0)],
    );
    sweep(k, &disk, vec![on(xy(0.0), curve)])
}

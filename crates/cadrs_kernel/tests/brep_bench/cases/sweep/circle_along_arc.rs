//! A circle of radius 2 swept along a quarter turn of the arc of radius 10 about the z axis in
//! the xy plane, from (10, 0, 0) to (0, 10, 0), the section square to the path, the ends flat:
//! the circle drawn on the plane through (10, 0, 0) square to the arc there (its normal +y),
//! swept along the arc drawn on the Top plane.
use crate::Refusal;
use crate::bench::{arc, p2, p3, plane, round, sketch, v3, xy};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let across = plane(p3(10.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, 1.0, 0.0));
    let section = sketch(across, vec![round((0.0, 0.0), 2.0)]);
    sweep(
        k,
        &section,
        vec![on(xy(0.0), arc(p2(0.0, 0.0), 10.0, 0.0, 90.0))],
    )
}

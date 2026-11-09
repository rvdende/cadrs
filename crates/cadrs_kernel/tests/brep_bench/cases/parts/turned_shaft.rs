//! A stepped shaft on the z axis: a section sketched on the Front plane (sketch (x, y) the
//! model's (x, z)) revolved a whole turn about the z axis. The section runs (0, 0), (20, 0),
//! (20, 60), (16, 60), a quarter circle of radius 4 about (16, 64) to (12, 64) bulging toward the
//! corner (12, 60), then (12, 160), (0, 160) and down the axis back to (0, 0).
use crate::Refusal;
use crate::bench::{V3, arc, axis, chain, line, p2, p3, revolve, sketch, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = chain(vec![
        line(p2(0.0, 0.0), p2(20.0, 0.0)),
        line(p2(20.0, 0.0), p2(20.0, 60.0)),
        line(p2(20.0, 60.0), p2(16.0, 60.0)),
        // from −90° clockwise to −180°: through −135°, toward the corner
        arc(p2(16.0, 64.0), 4.0, -90.0, -90.0),
        line(p2(12.0, 64.0), p2(12.0, 160.0)),
        line(p2(12.0, 160.0), p2(0.0, 160.0)),
        line(p2(0.0, 160.0), p2(0.0, 0.0)),
    ]);
    revolve(
        k,
        &sketch(xz(0.0), vec![section]),
        axis(p3(0.0, 0.0, 0.0), V3::z()),
        None,
    )
}

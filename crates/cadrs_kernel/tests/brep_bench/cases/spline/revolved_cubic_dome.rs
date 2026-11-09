//! A dome: the region in the xz plane bounded by the z axis from (0, 0, 0) to (0, 0, 10), the x
//! axis from the origin to (9, 0, 0), and the cubic Bézier curve with control points (9, 0, 0),
//! (12, 0, 4), (9, 0, 10) and (0, 0, 10), turned a whole turn about the z axis: the region
//! sketched on the Front plane (a sketch's Bézier curve), revolved.
use crate::Refusal;
use crate::bench::{axis, chain, line, p2, p3, revolve, sketch, v3, xz};
use cadrs_kernel::{BodyId, Curve2, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let curve = Curve2::Bezier {
        poles: [p2(9.0, 0.0), p2(12.0, 4.0), p2(9.0, 10.0), p2(0.0, 10.0)],
        source: None,
    };
    let region = chain(vec![
        line(p2(0.0, 0.0), p2(9.0, 0.0)),
        curve,
        line(p2(0.0, 10.0), p2(0.0, 0.0)),
    ]);
    let s = sketch(xz(0.0), vec![region]);
    revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)
}

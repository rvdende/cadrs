//! An L-shaped shelf bracket 10 thick: its outline sketched on the Front plane (sketch (x, y) the
//! model's (x, z)) and extruded symmetrically from y = −5 to y = 5. The outline runs (0, 0),
//! (20, 0), (20, 92), a quarter circle of radius 8 about (28, 92) to (28, 100) bulging toward the
//! corner (20, 100), then (100, 100), (100, 120), (0, 120) and back to (0, 0).
use crate::Refusal;
use crate::bench::{arc, chain, extrude_symmetric, line, p2, sketch, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outline = chain(vec![
        line(p2(0.0, 0.0), p2(20.0, 0.0)),
        line(p2(20.0, 0.0), p2(20.0, 92.0)),
        // from 180° clockwise to 90°: through 135°, toward the corner
        arc(p2(28.0, 92.0), 8.0, 180.0, -90.0),
        line(p2(28.0, 100.0), p2(100.0, 100.0)),
        line(p2(100.0, 100.0), p2(100.0, 120.0)),
        line(p2(100.0, 120.0), p2(0.0, 120.0)),
        line(p2(0.0, 120.0), p2(0.0, 0.0)),
    ]);
    extrude_symmetric(k, &sketch(xz(0.0), vec![outline]), 10.0)
}

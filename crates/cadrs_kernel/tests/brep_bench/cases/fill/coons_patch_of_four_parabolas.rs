//! The bilinearly blended Coons patch of four parabolic arcs over the square from (0, 0) to
//! (10, 10), each a quadratic Bézier curve in its upright plane (arching to 12 on y = 0 and x = 0,
//! sagging to 8 on x = 10 and y = 10), capping the solid under it: four upright walls down to the
//! floor z = 0 and the floor.
//!
//! Built as Onshape's surfacing builds it: each arc a quadratic Bézier spline sketched on its
//! upright plane, the four filled (cadrs's Fill: the bilinearly blended Coons patch of four
//! curves), each wall the Fill of its arc and the lines down to the floor, the floor the Fill of
//! its square, and the six sheets sewn into a solid.
use crate::Refusal;
use crate::bench::{xz, yz};
use crate::shared::fill::{bezier, fill, wall};
use crate::shared::loft::{flat, sewn};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // Each arc on its upright plane, its three control points in that plane's sketch
    // coordinates: (x, z) on y = c, (y, z) on x = c.
    let arcs = [
        (xz(0.0), [(0.0, 10.0), (5.0, 14.0), (10.0, 10.0)]),
        (yz(10.0), [(0.0, 10.0), (5.0, 6.0), (10.0, 10.0)]),
        (xz(10.0), [(10.0, 10.0), (5.0, 6.0), (0.0, 10.0)]),
        (yz(0.0), [(10.0, 10.0), (5.0, 14.0), (0.0, 10.0)]),
    ];
    let patch = fill(k, &arcs.map(|(plane, poles)| (plane, bezier(&poles))))?;
    let mut sheets = vec![
        patch,
        flat(
            k,
            0.0,
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        )?,
    ];
    for (plane, poles) in arcs {
        sheets.push(wall(k, plane, bezier(&poles), poles[0], poles[2])?);
    }
    sewn(k, &sheets)
}

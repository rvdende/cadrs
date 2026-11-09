//! The bilinearly blended Coons patch of the skew quadrilateral (0, 0, 5), (10, 0, 5),
//! (10, 10, 15), (0, 10, 5), the saddle z = 5 + xy/10 over the square from (0, 0) to (10, 10),
//! capping the solid under it: four upright walls down to the floor z = 0 and the floor.
//!
//! Built as Onshape's surfacing builds it: each edge a line sketched on its upright plane, the
//! four filled (cadrs's Fill: the bilinearly blended Coons patch of four curves), each wall the
//! Fill of its edge and the lines down to the floor, the floor the Fill of its square, and the
//! six sheets sewn into a solid.
use crate::Refusal;
use crate::bench::{xz, yz};
use crate::shared::fill::{fill, seg, wall};
use crate::shared::loft::{flat, sewn};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // Each edge on its upright plane, in that plane's sketch coordinates: (x, z) on y = c,
    // (y, z) on x = c.
    let edges = [
        (xz(0.0), (0.0, 5.0), (10.0, 5.0)),
        (yz(10.0), (0.0, 5.0), (10.0, 15.0)),
        (xz(10.0), (10.0, 15.0), (0.0, 5.0)),
        (yz(0.0), (10.0, 5.0), (0.0, 5.0)),
    ];
    let patch = fill(k, &edges.map(|(plane, a, b)| (plane, seg(a, b))))?;
    let mut sheets = vec![
        patch,
        flat(
            k,
            0.0,
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)],
        )?,
    ];
    for (plane, a, b) in edges {
        sheets.push(wall(k, plane, seg(a, b), a, b)?);
    }
    sewn(k, &sheets)
}

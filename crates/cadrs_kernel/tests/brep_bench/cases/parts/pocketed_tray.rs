//! An open tray lying on its back, opening toward −y, made in three steps: the block from
//! (−36, 0, −24) to (36, 16, 24); less the pocket from (−33, 0, −21) to (33, 13, 21); joined to
//! four standoffs on the floor's inside face (y = 13), each a tube along y of outer radius 5 and
//! inner radius 2 from y = 5 to y = 13 about (x, z) = (±26, ±14): an annulus sketched on the plane
//! y = 13 and extruded 8 toward the opening, its hole blind where it meets the floor.
use crate::Refusal;
use crate::bench::{block, cut, extrude, p3, round, sketch_with_holes, union, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let solid = block(k, p3(-36.0, 0.0, -24.0), p3(36.0, 16.0, 24.0))?;
    let pocket = block(k, p3(-33.0, 0.0, -21.0), p3(33.0, 13.0, 21.0))?;
    let mut tray = cut(k, solid, pocket)?;
    for (x, z) in [(26.0, 14.0), (-26.0, 14.0), (-26.0, -14.0), (26.0, -14.0)] {
        // the plane y = 13, its normal −y toward the opening, sketch (x, y) the model's (x, z)
        let ring = sketch_with_holes(xz(13.0), round((x, z), 5.0), vec![round((x, z), 2.0)]);
        let standoff = extrude(k, &ring, 8.0)?;
        tray = union(k, tray, standoff)?;
    }
    Ok(tray)
}

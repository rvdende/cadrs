//! A dumbbell plate shelled thicker than its neck: the inside walls cross and the neck fills. The
//! outline through (0, 0), (10, 0), (10, 4), (14, 4), (14, 0), (24, 0), (24, 10), (14, 10), (14, 6),
//! (10, 6), (10, 10) and (0, 10), extruded 6 along +z, shelled inward to a wall 1.5 thick, open at its
//! top: the wall is every point within 1.5 of the faces other than the top, so the neck, 2 wide, is
//! left solid and each square holds a pocket bulging toward the neck in arcs about its corners.
//!
//! The plate is the outline on the Top plane extruded 6; cadrs's Shell feature (inward, rounded
//! joins: the cavity rounded at the concave edges), the top picked as the face to remove.
use crate::Refusal;
use crate::bench::{extrude, p3, polygon, sketch, xy};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outline = polygon(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 4.0),
        (14.0, 4.0),
        (14.0, 0.0),
        (24.0, 0.0),
        (24.0, 10.0),
        (14.0, 10.0),
        (14.0, 6.0),
        (10.0, 6.0),
        (10.0, 10.0),
        (0.0, 10.0),
    ]);
    let plate = extrude(k, &sketch(xy(0.0), vec![outline]), 6.0)?;
    shell_inward(k, plate, &[p3(5.0, 5.0, 6.0)], 1.5, Joins::Rounded)
}

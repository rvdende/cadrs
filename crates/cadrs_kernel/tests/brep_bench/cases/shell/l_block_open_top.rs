//! An L-shaped plate shelled with its top open, the wall at its inside corner rounded. The outline
//! through (0, 0), (20, 0), (20, 5), (5, 5), (5, 15) and (0, 15), extruded 8 along +z, shelled inward
//! to a wall 1 thick, open at its top: the wall is every point within 1 of the faces other than the
//! top, so at the concave edge x = 5, y = 5 the inside is a quarter cylinder of radius 1 about it.
//!
//! The plate is the outline on the Top plane extruded 8; cadrs's Shell feature (inward, rounded
//! joins: the cavity rounded at the concave edges), the top picked as the face to remove.
use crate::Refusal;
use crate::bench::{extrude, p3, polygon, sketch, xy};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outline = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 5.0),
        (5.0, 5.0),
        (5.0, 15.0),
        (0.0, 15.0),
    ]);
    let plate = extrude(k, &sketch(xy(0.0), vec![outline]), 8.0)?;
    shell_inward(k, plate, &[p3(2.0, 2.0, 8.0)], 1.0, Joins::Rounded)
}

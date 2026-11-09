//! The L-shaped plate whose outline is the polygon through (0, 0), (20, 0), (20, 5), (5, 5),
//! (5, 15) and (0, 15), extruded 8 along +z, grown by 1 with its edges kept sharp: each face moved
//! 1 out along its normal and the moved faces extended until they meet.
//!
//! The outline extruded, then cadrs's offset with sharp edges.
use crate::Refusal;
use crate::bench::{extrude, polygon, sketch, xy};
use crate::shared::offset::offset;
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
    offset(k, plate, 1.0, true)
}

//! The L-shaped plate whose outline is the polygon through (0, 0), (20, 0), (20, 5), (5, 5),
//! (5, 15) and (0, 15), extruded 8 along +z, shrunk by 1: the solid of every point at least 1
//! from its outside. Each face moves 1 in, the convex edges stay sharp, and the concave edge
//! along (5, 5) becomes a concave quarter cylinder of radius 1 about it.
//!
//! The outline extruded, then cadrs's offset inward with rounded edges.
use crate::Refusal;
use crate::bench::{extrude, polygon, sketch, xy};
use crate::shared::offset::shrunk;
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
    shrunk(k, plate, 1.0)
}

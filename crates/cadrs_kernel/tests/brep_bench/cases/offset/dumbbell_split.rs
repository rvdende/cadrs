//! A dumbbell-shaped plate, the polygon through (0, 0), (10, 0), (10, 4), (14, 4), (14, 0),
//! (24, 0), (24, 10), (14, 10), (14, 6), (10, 6), (10, 10), (0, 10) extruded 6 along +z, shrunk
//! by 1.5: every point at least 1.5 from its outside. The neck is lost and the plate splits in
//! two; both are kept.
//!
//! The plate is the polygon sketched on the Top plane and extruded; the shrink is cadrs's offset
//! of a solid (`Kernel::offset`, rounded, inward), which gives each piece it leaves as a body of
//! its own; their union is the one body of both.
use crate::Refusal;
use crate::bench::{extrude, polygon, sketch, union, xy};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, OffsetSpec};

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
    let r = k.offset(
        plate,
        &OffsetSpec {
            distance: -1.5,
            faces: vec![],
            sharp: false,
        },
    )?;
    let mut pieces = r.bodies.into_iter();
    let first = pieces
        .next()
        .ok_or_else(|| Refusal::from("the offset made no body"))?;
    pieces.try_fold(first, |all, piece| union(k, all, piece))
}

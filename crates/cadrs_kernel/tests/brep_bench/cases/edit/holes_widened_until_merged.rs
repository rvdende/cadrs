//! A plate from the origin to (40, 30, 8), drilled right through along z by two holes of radius 3
//! about the vertical lines through (15, 15) and (25, 15); then both holes' walls offset by 3 into
//! the plate, away from their axes, to cylinders of radius 6, which overlap: one opening, the
//! walls trimmed where each runs into the other hole.
//!
//! The plate is a rectangle extruded and each hole a circle extruded through it and cut away;
//! then Move face offsets both walls by 3 into the material at once (−3 along their outward
//! normals, which point into the holes).
use crate::Refusal;
use crate::bench::{block, cut, cylinder, face_at, p3, v3};
use crate::shared::edit::offset_faces;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 8.0))?;
    // the holes' cylinders run past both faces
    let left = cylinder(k, p3(15.0, 15.0, -1.0), v3(0.0, 0.0, 1.0), 3.0, 10.0)?;
    let right = cylinder(k, p3(25.0, 15.0, -1.0), v3(0.0, 0.0, 1.0), 3.0, 10.0)?;
    let drilled = cut(k, plate, left)?;
    let drilled = cut(k, drilled, right)?;
    let walls = [
        face_at(k, drilled, p3(12.0, 15.0, 4.0))?,
        face_at(k, drilled, p3(28.0, 15.0, 4.0))?,
    ];
    offset_faces(k, drilled, &walls, -3.0)
}

//! A plate from the origin to (40, 30, 8), drilled right through along z by a hole of radius 5
//! about the vertical line through (20, 15); then the hole's wall offset by 2 into the plate,
//! away from its axis, to the cylinder of radius 7, the top and bottom trimmed back to meet it.
//!
//! The plate is a rectangle extruded and the hole a circle extruded through it and cut away;
//! then Move face offsets the hole's wall by 2 into the material (−2 along its outward normal,
//! which points into the hole).
use crate::Refusal;
use crate::bench::{block, cut, cylinder, face_at, p3, v3};
use crate::shared::edit::offset_face;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 8.0))?;
    // the hole's cylinder runs past both faces
    let drill = cylinder(k, p3(20.0, 15.0, -1.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let drilled = cut(k, plate, drill)?;
    let wall = face_at(k, drilled, p3(25.0, 15.0, 4.0))?;
    offset_face(k, drilled, wall, -2.0)
}

//! A block from the origin to (40, 30, 10), drilled right through along z by a hole of radius 5
//! about the vertical line through (20, 15); then its top face (the plane z = 10, the hole's
//! circle in it) moved up by 6 to z = 16, the sides and the hole's wall running on up to meet
//! it: the box to (40, 30, 16) with the same hole.
//!
//! The block is a rectangle extruded and the hole a circle extruded through it and cut away;
//! then Move face translates the top by 6 along +z.
use crate::Refusal;
use crate::bench::{block, cut, cylinder, face_at, p3, v3};
use crate::shared::edit::move_faces;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 10.0))?;
    // the hole's cylinder runs past both faces
    let drill = cylinder(k, p3(20.0, 15.0, -1.0), v3(0.0, 0.0, 1.0), 5.0, 12.0)?;
    let drilled = cut(k, b, drill)?;
    let top = face_at(k, drilled, p3(5.0, 5.0, 10.0))?;
    move_faces(k, drilled, &[top], v3(0.0, 0.0, 6.0))
}

//! A sheet: five faces of the cube from the origin to (10, 10, 10), its floor at z = 0 and its four
//! walls, with no top (an open tray), thickened by 1 out of the cube with its edges kept sharp: the
//! moved faces extended until they meet.
//!
//! The tray is the cube (a square on the Top plane extruded 10) with its floor and four walls
//! picked; cadrs's Thicken of them, 1 along their normals (out of the cube), its corners mitred
//! (`sharp`: the moved faces extended until they meet).
use crate::Refusal;
use crate::shared::thicken::{thicken_faces_with, tray};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let (cube, faces) = tray(k)?;
    thicken_faces_with(k, cube, &faces, 1.0, 0.0, true)
}

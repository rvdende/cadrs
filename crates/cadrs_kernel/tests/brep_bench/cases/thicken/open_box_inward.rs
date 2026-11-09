//! A sheet: five faces of the cube from the origin to (10, 10, 10), its floor at z = 0 and its four
//! walls, with no top (an open tray), thickened by 1 into the cube.
//!
//! The cube is a square on the Top plane extruded 10; its floor and walls are picked and thickened
//! by cadrs's Thicken against their normals (which point out of the cube).
use crate::Refusal;
use crate::shared::thicken::{thicken_faces, tray};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let (cube, faces) = tray(k)?;
    thicken_faces(k, cube, &faces, 0.0, 1.0)
}

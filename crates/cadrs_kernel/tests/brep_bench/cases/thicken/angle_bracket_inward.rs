//! A sheet of two flat faces meeting at a right angle along a crease, the floor (the rectangle in
//! the xy plane from (0, 0, 0) to (10, 8, 0)) and the wall (the rectangle in the yz plane from
//! (0, 0, 0) to (0, 8, 10)), thickened by 1 to the inside of the crease, above the floor and in
//! front of the wall (+z and +x), the moved faces trimmed where they cross.
//!
//! The sheet is the open polyline (10, 0), (0, 0), (0, 10) sketched on the Front plane and
//! surface-extruded 8 along +y; cadrs's Thicken thickens it with its arrow flipped to point up
//! from the floor.
use crate::Refusal;
use crate::bench::{p3, v3};
use crate::shared::thicken::{angle_sheet, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let sheet = angle_sheet(k)?;
    thicken_toward(k, sheet, p3(5.0, 4.0, 0.0), v3(0.0, 0.0, 1.0), 1.0)
}

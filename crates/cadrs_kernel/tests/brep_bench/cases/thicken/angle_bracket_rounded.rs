//! A sheet of two flat faces meeting at a right angle along a crease, the floor (the rectangle in
//! the xy plane from (0, 0, 0) to (10, 8, 0)) and the wall (the rectangle in the yz plane from
//! (0, 0, 0) to (0, 8, 10)), thickened by 1 to the outside of the crease (−z and −x), the gap the
//! moved faces leave at the crease closed by the quarter cylinder of radius 1 about it.
//!
//! The sheet is the L (10, 0), (0, 0), (0, 10) on the Front plane surface-extruded 8 along +y;
//! cadrs's Thicken of it, 1 toward −z from the floor (and so −x from the wall), its bend rounded (the cylinder of radius 1 about the crease).
use crate::Refusal;
use crate::bench::{p3, v3};
use crate::shared::thicken::{angle_sheet, thicken_toward_with};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let sheet = angle_sheet(k)?;
    thicken_toward_with(k, sheet, p3(5.0, 4.0, 0.0), v3(0.0, 0.0, -1.0), 1.0, false)
}

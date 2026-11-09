//! The bent sheet of `thicken/bent-flange` (on the xz plane, the line from (−10, 0, 0) to
//! (0, 0, 0), the quarter circle of radius 2 about (0, 0, 2) to (2, 0, 2) and the line up to
//! (2, 0, 12), swept along +y from y = 0 to 8), thickened by 3 to the inside of the bend, above
//! the floor and in front of the wall: more than the bend's radius, so the bend's moved face
//! would turn inside out and the moved floor and wall meet instead.
//!
//! The profile sketched on the Front plane as an open chain and surface-extruded 8 along +y;
//! cadrs's Thicken thickens it with its arrow flipped to point up from the floor. cadrs declines
//! it: its slab of the bend's face has no offset surface for a cylinder moved past its axis (and
//! its thin extrude of the profile, the other way to build the bracket, refuses a wall thicker
//! than an arc's radius).
use crate::Refusal;
use crate::bench::{p3, v3};
use crate::shared::thicken::{flange_sheet, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let sheet = flange_sheet(k)?;
    thicken_toward(k, sheet, p3(-5.0, 4.0, 0.0), v3(0.0, 0.0, 1.0), 3.0)
}

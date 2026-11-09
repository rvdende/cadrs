//! The solid the box from (0, 0, 0) to (10, 6, 4) fills as it slides without turning along
//! (8, 5, 3), to the box from (8, 5, 3) to (18, 11, 7).
//!
//! The box is a rectangle on the Top plane extruded 4; its slide is cadrs's sweep of a solid
//! (`Kernel::sweep_solid`, one slide along (8, 5, 3)).
use crate::Refusal;
use crate::bench::{block, p3, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let start = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 6.0, 4.0))?;
    slid(k, start, v3(8.0, 5.0, 3.0))
}

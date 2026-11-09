//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by a bar (the
//! box 4.5 ≤ x ≤ 12, |y| ≤ 8, |z| ≤ 0.45) screwed about the z axis, rising 1 a turn, from a turn of
//! −180° to 7380°: the groove's turns meet in sharp crests.
//!
//! cadrs's cut by a swept solid: the screw (a turn and a slide along its axis at once) in pieces of
//! half a turn, each swept exactly and cut from the rod in turn (a whole turn's sweep would overlap
//! itself, the groove being wider than the lead out past the crests); of the bar, its part within
//! 5.5 of the axis (the screw maps the cylinder about its axis onto itself, so only that part
//! reaches the rod).
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::swept::{cut_swept, near_axis, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = block(k, p3(4.5, -8.0, -0.45), p3(12.0, 8.0, 0.45))?;
    let bar = near_axis(k, bar, 5.5)?;
    let r = rod(k)?;
    cut_swept(k, r, bar, &screw(z_axis(), 1.0, -180.0, 7380.0))
}

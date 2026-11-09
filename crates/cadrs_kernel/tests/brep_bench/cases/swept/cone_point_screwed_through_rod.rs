//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by a pointed cone
//! (its point at (4, 0, 0), its axis along +x, opening at 60° to a flat base at x = 12) screwed
//! about the z axis, rising 6 a turn, from a turn of −90° to 1350°.
//!
//! The cone is cadrs's cone primitive; what it passes through (a turn
//! and a slide along its axis at once: a screw) is cut from the rod by cadrs's cut by a swept solid
//! (exactly half a turn at a time where it can be, else meshed); of the cone, its part within 5.5
//! of the axis (the screw maps the cylinder about its axis onto itself, so only that part reaches
//! the rod).
use crate::Refusal;
use crate::bench::{cone, p3, v3};
use crate::shared::swept::{cut_swept, near_axis, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let r = 8.0 * 30f64.to_radians().tan();
    let tool = cone(k, p3(12.0, 0.0, 0.0), v3(-1.0, 0.0, 0.0), r, 0.0, 8.0)?;
    let tool = near_axis(k, tool, 5.5)?;
    let rd = rod(k)?;
    cut_swept(k, rd, tool, &screw(z_axis(), 6.0, -90.0, 1350.0))
}

//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by a bar (the box
//! 4 ≤ x ≤ 12, |y| ≤ 8, |z| ≤ 1) turned by θ about the z axis while rising 6s + 0.3s², s = θ/360°,
//! as θ runs from −90° to 1260°.
//!
//! That motion is cadrs's climb (a turn about the axis rising by a polynomial in the turn: here
//! 6θ/2π + 0.3θ²/4π², θ in radians); what the bar passes through is cut from the rod by cadrs's
//! cut by a swept solid (half a turn at a time, the rod less the bar where each half starts and
//! less its leading faces' sweeps); of the bar, its part within 5.5 of the axis (the motion's
//! turns and rises map the cylinder about the axis onto itself).
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::swept::{cut_swept, near_axis, rod, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel, SolidMotion};
use std::f64::consts::PI;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = block(k, p3(4.0, -8.0, -1.0), p3(12.0, 8.0, 1.0))?;
    let bar = near_axis(k, bar, 5.5)?;
    let r = rod(k)?;
    let motion = SolidMotion::Climb {
        axis: z_axis(),
        from: (-90f64).to_radians(),
        to: 1260f64.to_radians(),
        rise: vec![0.0, 6.0 / (2.0 * PI), 0.3 / (4.0 * PI * PI)],
    };
    cut_swept(k, r, bar, &motion)
}

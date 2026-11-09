//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) with a helical groove
//! ground by an oblate spheroid (semi-axes 6, 6 and 1.5 about (10.5, 0, 0), its short axis along
//! (0, cos h, −sin h), h = atan(2π·4.75/30)) screwed about the z axis, rising 30 a turn, from a
//! turn of −90° to +330°.
//!
//! The spheroid is the unit ball scaled and turned (cadrs's affine transform); what it passes
//! through (a turn and a slide along its axis at once: a screw) is cut from the rod by cadrs's cut by
//! a swept solid (exactly half a turn at a time where it can be, else meshed); of the wheel, its part within 5.5 of the axis (the screw maps the cylinder about
//! its axis onto itself, so only that part reaches the rod).
use crate::Refusal;
use crate::bench::{ball, p3, v3};
use crate::shared::sweep::first;
use crate::shared::swept::{cut_swept, near_axis, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let unit = ball(k, p3(0.0, 0.0, 0.0), 1.0)?;
    let h = (std::f64::consts::TAU * 4.75 / 30.0).atan();
    // Its axes: x stays, the short axis along (0, cos h, −sin h), the third square to both.
    let short = v3(0.0, h.cos(), -h.sin());
    let third = short.cross(&v3(1.0, 0.0, 0.0));
    let m = nalgebra::Matrix3::from_columns(&[v3(6.0, 0.0, 0.0), third * 6.0, short * 1.5]);
    let wheel = first(k.transform_affine(unit, m, v3(10.5, 0.0, 0.0))?)?;
    let wheel = near_axis(k, wheel, 5.5)?;
    let r = rod(k)?;
    cut_swept(k, r, wheel, &screw(z_axis(), 30.0, -90.0, 330.0))
}

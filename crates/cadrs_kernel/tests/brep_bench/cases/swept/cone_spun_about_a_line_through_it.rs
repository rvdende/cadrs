//! The solid a cone (its axis along w = (cos 30°, 0, sin 30°) through the origin, its point at −2w,
//! opening at 50° to a flat base at 5w) fills as it turns 150° about the z axis, which crosses it.
//!
//! The cone is cadrs's cone primitive; what it passes through is cadrs's sweep of a solid (one
//! turn).
use crate::Refusal;
use crate::bench::{P3, V3, cone};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let w = V3::new(30f64.to_radians().cos(), 0.0, 30f64.to_radians().sin());
    let point = P3::from(w * -2.0);
    let c = cone(k, point, w, 0.0, 7.0 * 25f64.to_radians().tan(), 7.0)?;
    swung(k, c, z_axis(), 0.0, 150.0)
}

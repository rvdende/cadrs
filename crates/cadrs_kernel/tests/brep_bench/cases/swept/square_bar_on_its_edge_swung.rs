//! The solid a square bar (the square with corners (4, 0), (6, 2), (8, 0), (6, −2), from z = 0 to
//! 3) fills as it turns a quarter turn about the z axis.
//!
//! The square sketched and extruded; what it passes through is cadrs's sweep of a solid (one
//! turn).
use crate::Refusal;
use crate::bench::{extrude, polygon, sketch, xy};
use crate::shared::swept::{swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let square = polygon(&[(4.0, 0.0), (6.0, 2.0), (8.0, 0.0), (6.0, -2.0)]);
    let bar = extrude(k, &sketch(xy(0.0), vec![square]), 3.0)?;
    swung(k, bar, z_axis(), 0.0, 90.0)
}

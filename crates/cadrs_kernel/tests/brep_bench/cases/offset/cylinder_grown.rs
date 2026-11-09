//! A cylinder of radius 5 on the z axis, from z = 0 to z = 10, grown by 1: the solid of every
//! point within 1 of it. The side moves out to radius 6, the ends to z = −1 and z = 11, and each
//! rim becomes the outer quarter of a torus's tube of radius 1.
//!
//! The circle extruded, then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{cylinder, p3, v3};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    grown(k, c, 1.0)
}

//! A solid torus about the z axis, centered on the origin, its tube of radius 2 about a middle
//! circle of radius 4, grown by 3: every point within 5 of the middle circle. The grown tube
//! would cross the axis, so the solid is an apple, one face meeting the axis in two dimples at
//! (0, 0, ±3).
//!
//! The torus is cadrs's torus primitive. Then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let t = torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 4.0, 2.0)?;
    grown(k, t, 3.0)
}

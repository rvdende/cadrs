//! A groin vault: the common part of two blocks with barrel-vaulted tops, the second the first
//! turned a quarter turn about the z axis.
//!
//! The first block stands on the square −12 ≤ x, y ≤ 12 from the floor z = 0; its top is the
//! rational B-spline surface of degree 1 along x and 2 along y, knots (0, 0, 1, 1) and
//! (0, 0, 0, 1, 1, 1), whose rows are the rational quadratic arcs through (x, −12, 10),
//! (x, 0, 26) and (x, 12, 10) with weights 1, 3/5 and 1, for x = −12 and 12: part of the
//! cylinder of radius 15 about the line y = 0, z = 1. It is the square extruded 10, its flat
//! top replaced by that surface (Replace face). The second is a copy turned 90° about z.
use crate::Refusal;
use crate::bench::{axis, common, p3, turned, v3};
use crate::shared::nurbs::{barrel_vault_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = barrel_vault_surface()?;
    let first = domed_block(k, (-12.0, -12.0), (12.0, 12.0), 10.0, top)?;
    let second = turned(k, first, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), 90.0)?;
    common(k, first, second)
}

//! A sheet with no boundary: the sphere of radius 5 about the origin as a surface alone, one closed
//! face with no solid inside it, thickened by 1 outward into a hollow ball.
//!
//! The half circle of radius 5 about the origin from (0, −5) to (0, 5) through (5, 0) sketched on
//! the Front plane (its (x, z)) and surface-revolved a whole turn about z; cadrs's Thicken
//! thickens it with its arrow flipped to point away from the center.
use crate::Refusal;
use crate::bench::{arc, axis, p2, p3, v3, xz};
use crate::shared::thicken::{open_chain, surface_revolve, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xz(0.0), vec![arc(p2(0.0, 0.0), 5.0, -90.0, 180.0)]);
    let sheet = surface_revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)?;
    thicken_toward(k, sheet, p3(0.0, 5.0, 0.0), v3(0.0, 1.0, 0.0), 1.0)
}

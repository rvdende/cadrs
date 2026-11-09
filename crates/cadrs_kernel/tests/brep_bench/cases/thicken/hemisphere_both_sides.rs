//! A sheet: the upper half of the sphere of radius 5 about the origin, where z ≥ 0, one face bounded
//! by its equator, thickened by 1 to each side.
//!
//! The quarter circle of radius 5 about the origin from (5, 0) to (0, 5) sketched on the Front
//! plane (its (x, z)) and surface-revolved a whole turn about z; cadrs's Thicken thickens it 1
//! along its normal and 1 against it.
use crate::Refusal;
use crate::bench::{arc, axis, p2, p3, v3, xz};
use crate::shared::thicken::{open_chain, surface_revolve, thicken};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xz(0.0), vec![arc(p2(0.0, 0.0), 5.0, 0.0, 90.0)]);
    let sheet = surface_revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)?;
    thicken(k, sheet, 1.0, 1.0)
}

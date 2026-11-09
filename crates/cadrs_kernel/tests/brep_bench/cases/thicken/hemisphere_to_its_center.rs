//! A sheet: the upper half of the sphere of radius 5 about the origin, where z ≥ 0, one face bounded
//! by its equator, thickened by 5 toward the sphere's center: by its whole radius, so the moved
//! face shrinks to the center and the thickened sheet is the half ball.
//!
//! The quarter circle of radius 5 about the origin from (5, 0) to (0, 5) sketched on the Front
//! plane (its (x, z)) and surface-revolved a whole turn about z; cadrs's Thicken thickens it with
//! its arrow flipped to point toward the center. cadrs declines it: its slab of the face has no
//! offset surface for a sphere moved all the way to its center (and its thin revolve of the arc
//! refuses a wall as thick as the arc's radius).
use crate::Refusal;
use crate::bench::{arc, axis, p2, p3, v3, xz};
use crate::shared::thicken::{open_chain, surface_revolve, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xz(0.0), vec![arc(p2(0.0, 0.0), 5.0, 0.0, 90.0)]);
    let sheet = surface_revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)?;
    // a point of the sheet on the +y side, 45° up; toward the center from it
    let at = p3(0.0, 1.0, 1.0) * (5.0 / 2f64.sqrt());
    thicken_toward(k, sheet, at, -at.coords, 5.0)
}

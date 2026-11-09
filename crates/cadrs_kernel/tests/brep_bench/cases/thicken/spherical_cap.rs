//! A sheet: the cap of the sphere of radius 10 about the origin where z ≥ 6, one face bounded by
//! the circle of radius 8 at z = 6, thickened by 2 away from the sphere's center.
//!
//! The arc of radius 10 about the origin from (8, 6) to (0, 10) sketched on the Front plane (its
//! (x, z)) and surface-revolved a whole turn about z; cadrs's Thicken thickens it with its arrow
//! flipped to point away from the center.
use crate::Refusal;
use crate::bench::{arc, axis, p2, p3, v3, xz};
use crate::shared::thicken::{open_chain, surface_revolve, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the arc starts at (8, 6), its angle from the sketch's x axis atan2(6, 8)
    let start = 6f64.atan2(8.0).to_degrees();
    let s = open_chain(xz(0.0), vec![arc(p2(0.0, 0.0), 10.0, start, 90.0 - start)]);
    let sheet = surface_revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)?;
    // a point of the sheet on the +y side, 20° from the pole; away from the center from it
    let at = p3(0.0, 20f64.to_radians().sin(), 20f64.to_radians().cos()) * 10.0;
    thicken_toward(k, sheet, at, at.coords, 2.0)
}

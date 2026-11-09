//! A square plate from (−20, −20, 0) to (20, 20, 4) and a round boss on it, a cylinder of radius
//! 5 on the z axis from z = 4 to z = 8, joined into one solid; both circular edges of the boss
//! rounded to radius 2: its rim at z = 8 (convex) and its root at z = 4 (concave). Each round
//! takes half the boss's height, so its side vanishes and its profile is an S.
//!
//! The plate is a square extruded and the boss a circle extruded from the plate's top, the two
//! united; both circles are picked by a point on each and rounded in one Fillet feature, radius 2.
use crate::Refusal;
use crate::bench::{block, cylinder, edges_at, p3, union, v3};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-20.0, -20.0, 0.0), p3(20.0, 20.0, 4.0))?;
    let boss = cylinder(k, p3(0.0, 0.0, 4.0), v3(0.0, 0.0, 1.0), 5.0, 4.0)?;
    let body = union(k, plate, boss)?;
    let edges = edges_at(k, body, &[p3(5.0, 0.0, 8.0), p3(5.0, 0.0, 4.0)])?;
    fillet(k, body, &edges, 2.0)
}

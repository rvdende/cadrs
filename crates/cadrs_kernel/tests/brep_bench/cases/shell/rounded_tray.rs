//! A plate with rounded corners shelled thinner than its rounds. The box from the origin to
//! (20, 14, 10) with its four vertical edges rounded to radius 2, shelled inward to a wall 1 thick,
//! open at its top: each round moved 1 inward is the cylinder of radius 1 on its axis, so the cavity
//! is the 18 × 12 rectangle from (1, 1) to (19, 13) with its corners rounded to radius 1, from z = 1
//! up.
//!
//! The plate is the box (a rectangle on the Top plane extruded 10) with its four vertical edges
//! filleted to radius 2; cadrs's Shell feature (inward), its top picked as the face to remove.
use crate::Refusal;
use crate::bench::{block, edges_at, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 14.0, 10.0))?;
    let corners = [
        p3(0.0, 0.0, 5.0),
        p3(20.0, 0.0, 5.0),
        p3(20.0, 14.0, 5.0),
        p3(0.0, 14.0, 5.0),
    ];
    let edges = edges_at(k, b, &corners)?;
    let plate = k.fillet(b, &edges, 2.0)?.bodies[0];
    shell_inward(k, plate, &[p3(10.0, 7.0, 10.0)], 1.0, Joins::Rounded)
}

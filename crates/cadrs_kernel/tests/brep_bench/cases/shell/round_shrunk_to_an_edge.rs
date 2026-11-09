//! A plate with rounded corners shelled as thick as its rounds: the inside rounds shrink to edges. The
//! box from the origin to (20, 14, 10) with its four vertical edges rounded to radius 2, shelled inward
//! to a wall 2 thick, open at its top: each round moved 2 inward has shrunk to its axis, so the cavity
//! is the sharp-cornered box from (2, 2, 2) to (18, 12, 10).
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
    shell_inward(k, plate, &[p3(10.0, 7.0, 10.0)], 2.0, Joins::Rounded)
}

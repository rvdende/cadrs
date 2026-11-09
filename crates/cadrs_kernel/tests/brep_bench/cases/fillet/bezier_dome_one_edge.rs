//! The block under the bicubic Bézier dome of `nurbs/bezier-dome` (0 ≤ x, y ≤ 20, the floor at
//! z = 0), the edge where the dome meets the side y = 0 rounded to radius 1.
//!
//! The block is the square extruded with its top replaced by the patch (as `nurbs/bezier-dome`);
//! the edge is picked by its point over x = 10, where the patch's edge v = 0 is at z = 5, and
//! filleted.
use crate::Refusal;
use crate::bench::{edge_at, p3};
use crate::shared::fillet::fillet;
use crate::shared::nurbs::{bezier_dome_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = bezier_dome_surface()?;
    let b = domed_block(k, (0.0, 0.0), (20.0, 20.0), 10.0, top)?;
    let edge = edge_at(k, b, p3(10.0, 0.0, 5.0))?;
    fillet(k, b, &[edge], 1.0)
}

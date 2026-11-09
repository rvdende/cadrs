//! The block under the bicubic Bézier dome of `nurbs/bezier-dome`, drilled straight down
//! through by the vertical cylinder of radius 3 about the line x = 12, y = 8.
//!
//! The domed block is built as in `nurbs/bezier-dome` (the square extruded, its top replaced by
//! the patch); the hole is a circle on the plane z = −1 extruded 30, past the floor and the top,
//! and cut from it.
use crate::Refusal;
use crate::bench::{cut, cylinder, p3, v3};
use crate::shared::nurbs::{bezier_dome_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = bezier_dome_surface()?;
    let dome = domed_block(k, (0.0, 0.0), (20.0, 20.0), 10.0, top)?;
    let drill = cylinder(k, p3(12.0, 8.0, -1.0), v3(0.0, 0.0, 1.0), 3.0, 30.0)?;
    cut(k, dome, drill)
}

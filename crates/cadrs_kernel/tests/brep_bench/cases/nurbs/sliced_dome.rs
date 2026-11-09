//! The block under the bicubic Bézier dome of `nurbs/bezier-dome`, with everything above the
//! inclined plane z = 4 + x/2 cut away; the plane passes exactly through the block's corner
//! (0, 0, 4).
//!
//! The domed block is built as in `nurbs/bezier-dome` (the square extruded, its top replaced by
//! the patch). What lies above the plane is cut away with a block: a square of side 80 sketched
//! on the plane itself (through (0, 0, 4), sketch x along (2, 0, 1)), extruded 40 along the
//! plane's upward normal (−1, 0, 2)/√5, reaching past the dome.
use crate::Refusal;
use crate::bench::{cut, extrude, p3, plane, rectangle, sketch, v3};
use crate::shared::nurbs::{bezier_dome_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = bezier_dome_surface()?;
    let dome = domed_block(k, (0.0, 0.0), (20.0, 20.0), 10.0, top)?;
    let on = plane(p3(0.0, 0.0, 4.0), v3(2.0, 0.0, 1.0), v3(-1.0, 0.0, 2.0));
    let above = extrude(
        k,
        &sketch(on, vec![rectangle(-40.0, -40.0, 40.0, 40.0)]),
        40.0,
    )?;
    cut(k, dome, above)
}

//! A block standing on the square 0 ≤ x, y ≤ 20, its floor flat at z = 0, its four sides
//! upright, and its top the bicubic Bézier patch whose sixteen control points stand over the
//! even 4 × 4 grid of the square at the case's heights (4 6 7 5 / 6 14 12 7 / 5 13 16 8 /
//! 3 6 8 6, row i at x = 20i/3).
//!
//! The patch is the kernel's `NurbsSurface` of degree 3 × 3 on the knots (0, 0, 0, 0, 1, 1, 1,
//! 1). The block is the square extruded 10, and its flat top replaced by the patch (Replace
//! face), which trims the four sides to the patch's edges.
use crate::Refusal;
use crate::shared::nurbs::{bezier_dome_surface, domed_block};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let top = bezier_dome_surface()?;
    domed_block(k, (0.0, 0.0), (20.0, 20.0), 10.0, top)
}

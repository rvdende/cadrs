//! A block −12 ≤ x ≤ 12, −8 ≤ y ≤ 8, 0 ≤ z ≤ 8, with a hole of elliptic section bored straight
//! through it along the z axis: everything with x²/36 + y²/9 < 1 (semi-axes 6 along x and 3 along
//! y) is removed, from the bottom face to the top.
//!
//! One sketch on the Top plane: the rectangle with the ellipse (semi-axes 6 along x, 3 along y)
//! inside it as a hole, one region, extruded 8. (Cutting an elliptic cylinder that runs past both
//! faces from the extruded block is the other way; cadrs's boolean gives that solid's volume and
//! area off by about 1e-6 of them and leaves its mesh open.)
use crate::Refusal;
use crate::bench::{extrude, rectangle, sketch_with_holes, xy};
use crate::shared::geometry::ellipse;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch_with_holes(
        xy(0.0),
        rectangle(-12.0, -8.0, 12.0, 8.0),
        vec![ellipse((0.0, 0.0), 6.0, 3.0)],
    );
    extrude(k, &s, 8.0)
}

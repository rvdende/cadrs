//! The elliptic cylinder of `geometry/elliptic-cylinder` (x²/25 + y²/9 ≤ 1 from z = 0 to 10)
//! shrunk by 1: its side moved in along its normals (less than its least radius of curvature,
//! 1.8, so it stays smooth), its ends 1 nearer, its edges sharp.
//!
//! The ellipse sketched and extruded (as `geometry/elliptic-cylinder`), then offset inward by 1
//! (cadrs's offset of a solid).
use crate::Refusal;
use crate::bench::{extrude, sketch, xy};
use crate::shared::geometry::ellipse;
use crate::shared::offset::shrunk;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch(xy(0.0), vec![ellipse((0.0, 0.0), 5.0, 3.0)]);
    let c = extrude(k, &s, 10.0)?;
    shrunk(k, c, 1.0)
}

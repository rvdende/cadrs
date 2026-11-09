//! A square plate from (−10, −10, 0) to (10, 10, 2) with a hole of radius 3 through it on the
//! z axis, both circular edges of the hole (on the top and on the bottom face) rounded to radius
//! 1, half the plate's thickness: the hole's wall vanishes and its whole surface is the inner
//! half of a torus whose centre circle has radius 4 at z = 1.
//!
//! The plate is the square less the circle, sketched on the Top plane and extruded 2; both rims
//! of the hole are picked by a point on each and rounded in one Fillet feature, radius 1.
use crate::Refusal;
use crate::bench::{edges_at, extrude, p3, rectangle, round, sketch_with_holes, xy};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch_with_holes(
        xy(0.0),
        rectangle(-10.0, -10.0, 10.0, 10.0),
        vec![round((0.0, 0.0), 3.0)],
    );
    let plate = extrude(k, &s, 2.0)?;
    let rims = edges_at(k, plate, &[p3(3.0, 0.0, 2.0), p3(3.0, 0.0, 0.0)])?;
    fillet(k, plate, &rims, 1.0)
}

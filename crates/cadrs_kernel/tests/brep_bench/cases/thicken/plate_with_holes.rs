//! A sheet: the square in the xy plane from (−10, −10, 0) to (10, 10, 0) with two round holes of
//! radius 3 through it, centered on (−5, 0, 0) and (5, 0, 0), thickened by 1 to each side. The
//! square and the two circles sketched on the Top plane, the region between them thickened 1
//! along the plane's normal and 1 against it.
use crate::Refusal;
use crate::bench::{rectangle, round, sketch_with_holes, xy};
use crate::shared::thicken::thicken_sketch;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch_with_holes(
        xy(0.0),
        rectangle(-10.0, -10.0, 10.0, 10.0),
        vec![round((-5.0, 0.0), 3.0), round((5.0, 0.0), 3.0)],
    );
    thicken_sketch(k, s, 1.0, 1.0)
}

//! A plate (the box from (−10, −10, 0) to (10, 10, 4)) cut right through by a core drill (the tube
//! between 1.5 and 2.5 from the line through (0, 0, 2) along u = (sin 30°, 0, cos 30°), from 18 to 6
//! before (0, 0, 2)) pushed 24 along u: the plate with an oblique annular hole, and its core cut
//! free.
//!
//! The tube is its annulus sketched square to u and extruded 12; what it passes through is cadrs's
//! sweep of a solid (one slide), cut from the plate (two solids).
use crate::Refusal;
use crate::bench::{P3, block, cut, extrude, p3, round, sketch_with_holes, square_to, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-10.0, -10.0, 0.0), p3(10.0, 10.0, 4.0))?;
    let u = v3(30f64.to_radians().sin(), 0.0, 30f64.to_radians().cos());
    let from = P3::new(0.0, 0.0, 2.0) - u * 18.0;
    let ring = sketch_with_holes(
        square_to(from, u),
        round((0.0, 0.0), 2.5),
        vec![round((0.0, 0.0), 1.5)],
    );
    let tube = extrude(k, &ring, 12.0)?;
    let swept = slid(k, tube, u * 24.0)?;
    cut(k, plate, swept)
}

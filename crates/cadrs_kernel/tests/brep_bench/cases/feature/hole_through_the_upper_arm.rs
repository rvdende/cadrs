//! A C-shaped bracket, the polygon through (x, z) = (0, 0), (40, 0), (40, 6), (6, 6), (6, 24),
//! (40, 24), (40, 30), (0, 30) in the plane y = −10 extruded along +y to y = 10, with a hole of
//! radius 4 on the vertical axis through (28, 0) drilled down from the upper arm's top through
//! the first wall it meets, the upper arm, ending where it comes out into the gap; the lower arm
//! is not drilled.
//!
//! The C is sketched on a plane parallel to the Front plane and extruded along +y. The hole is a
//! circle sketched on the upper arm's top, extruded down "up to next" (to the first faces of the
//! bracket the extrude meets past its start, the arm's underside) and cut away.
use crate::Refusal;
use crate::bench::{cut, extrude, polygon, round, sketch, v3, xy, xz};
use crate::shared::feature::extrude_to;
use cadrs_kernel::{BodyId, DefaultKernel, ExtrudeEnd};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = polygon(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 6.0),
        (6.0, 6.0),
        (6.0, 24.0),
        (40.0, 24.0),
        (40.0, 30.0),
        (0.0, 30.0),
    ]);
    // the plane's normal is −y: extrude against it, along +y
    let bracket = extrude(k, &sketch(xz(-10.0), vec![c]), -20.0)?;
    let circle = sketch(xy(30.0), vec![round((28.0, 0.0), 4.0)]);
    let drill = extrude_to(
        k,
        &circle,
        v3(0.0, 0.0, -1.0),
        ExtrudeEnd::UpToNext { offset: 0.0 },
        &[bracket],
    )?;
    cut(k, bracket, drill)
}

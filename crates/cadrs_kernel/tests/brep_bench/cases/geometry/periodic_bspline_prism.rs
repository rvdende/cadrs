//! A prism standing on the xy plane, from z = 0 to z = 10, whose section is the closed, periodic
//! uniform quadratic B-spline whose control polygon is the square of corners (10, 10),
//! (−10, 10), (−10, −10) and (10, −10): four parabolic arcs, each from the middle of one side to
//! the middle of the next, (10, 0) to (0, 10) and so on, with the square's corner as its control
//! point. The prism's side is that curve swept straight up; its ends are flat.
//!
//! The section is that periodic B-spline, one sketch spline on the Top plane (`Curve2::Spline`,
//! its four poles and the first two again, its knots evenly spaced), extruded 10 up.
use crate::Refusal;
use crate::bench::{v3, xy};
use crate::shared::geometry::Section;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = Section::on(xy(0.0)).periodic(
        2,
        &[(10.0, 10.0), (-10.0, 10.0), (-10.0, -10.0), (10.0, -10.0)],
    );
    section.slid(k, v3(0.0, 0.0, 10.0))
}

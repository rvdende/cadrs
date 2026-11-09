//! A prism 10 tall on the xy plane, from z = 0 to z = 10, whose section is the region inside the
//! closed, uniform, periodic cubic B-spline with the twelve control points (14, 0), (12, 8),
//! (4, 6), (0, −21/8), (−4, 6), (−12, 8), (−14, 0), (−12, −8), (−4, −6), (0, 21/8), (4, −6),
//! (12, −8): a peanut with a waist 0.5 wide.
//!
//! The curve is a sketch spline on the Top plane (`Curve2::Spline`): the B-spline whose poles are
//! the twelve control points followed again by the first three, on uniform knots 0, 1, …, 18,
//! over its range from knot 3 to knot 15, each of its twelve spans the uniform cubic segment of
//! four consecutive control points, wrapping around, as the description defines the curve. The
//! sketch is extruded 10 up.
use crate::Refusal;
use crate::bench::{v3, xy};
use crate::shared::geometry::Section;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let points = [
        (14.0, 0.0),
        (12.0, 8.0),
        (4.0, 6.0),
        (0.0, -21.0 / 8.0),
        (-4.0, 6.0),
        (-12.0, 8.0),
        (-14.0, 0.0),
        (-12.0, -8.0),
        (-4.0, -6.0),
        (0.0, 21.0 / 8.0),
        (4.0, -6.0),
        (12.0, -8.0),
    ];
    Section::on(xy(0.0))
        .periodic(3, &points)
        .slid(k, v3(0.0, 0.0, 10.0))
}

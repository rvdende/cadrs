//! A spheroid: the region in the xz plane between the z axis from (0, 0, −10) to (0, 0, 10) and
//! the rational quadratic B-spline with control points (0, 0, −10), (6, 0, −10), (6, 0, 0),
//! (6, 0, 10), (0, 0, 10), weights 1, √2/2, 1, √2/2, 1 and knots (0, 0, 0, ½, ½, 1, 1, 1),
//! turned a whole turn about the z axis.
//!
//! The boundary is sketched on the Front plane: the B-spline, a sketch spline (`Curve2::Spline`),
//! and the axis segment, revolved a whole turn about z.
use crate::Refusal;
use crate::bench::xz;
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};
use std::f64::consts::FRAC_1_SQRT_2;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let w = FRAC_1_SQRT_2;
    // the Front plane's sketch is the model's (x, z)
    let section = Section::on(xz(0.0))
        .spline(
            2,
            &[0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
            &[
                (0.0, -10.0),
                (6.0, -10.0),
                (6.0, 0.0),
                (6.0, 10.0),
                (0.0, 10.0),
            ],
            &[1.0, w, 1.0, w, 1.0],
        )
        .line((0.0, 10.0), (0.0, -10.0));
    turned_about_z(k, &section)
}

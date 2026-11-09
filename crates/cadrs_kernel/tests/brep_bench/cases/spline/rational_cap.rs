//! A ball's cap: the region in the xz plane bounded by the z axis from (0, 0, −5) to (0, 0, 10),
//! the segment from (0, 0, −5) to (5√3, 0, −5), and the rational quadratic Bézier curve from
//! (5√3, 0, −5) through the control point (10√3, 0, 10) to (0, 0, 10), weights 1, ½ and 1,
//! turned a whole turn about the z axis.
//!
//! The boundary is sketched on the Front plane: the two segments and the weighted Bézier curve, a
//! sketch spline (`Curve2::Spline`), revolved a whole turn about z.
use crate::Refusal;
use crate::bench::xz;
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let r = 5.0 * 3f64.sqrt();
    // the Front plane's sketch is the model's (x, z)
    let section = Section::on(xz(0.0))
        .line((0.0, 10.0), (0.0, -5.0))
        .line((0.0, -5.0), (r, -5.0))
        .bezier(&[(r, -5.0), (2.0 * r, 10.0), (0.0, 10.0)], &[1.0, 0.5, 1.0]);
    turned_about_z(k, &section)
}

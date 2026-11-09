//! A solid paraboloid of revolution about the z axis, its vertex at the origin, filled up to the
//! plane z = 8: every point with x² + y² ≤ 8z and z ≤ 8. It is the parabola x² = 8z of the xz
//! plane (from its vertex to x = 8) turned a whole turn about the z axis.
//!
//! The section is sketched on the Front (xz) plane: the parabola's arc as the quadratic Bézier it
//! is, a sketch spline (`Curve2::Spline`: its ends (0, 0) and (8, 8) and the point (4, 0) where
//! their tangents meet), the top edge from (8, 8) to (0, 8) and the axis back down, revolved a
//! whole turn about z.
use crate::Refusal;
use crate::bench::xz;
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = Section::on(xz(0.0))
        .bezier(&[(0.0, 0.0), (4.0, 0.0), (8.0, 8.0)], &[])
        .line((8.0, 8.0), (0.0, 8.0))
        .line((0.0, 8.0), (0.0, 0.0));
    turned_about_z(k, &section)
}

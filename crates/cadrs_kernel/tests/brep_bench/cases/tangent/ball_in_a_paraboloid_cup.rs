//! A solid paraboloid of revolution about the z axis, x² + y² ≤ 10z up to z = 5, united with a ball
//! of radius 5 about (0, 0, 5) whose bottom pole touches the cup's vertex.
//!
//! The cup is the parabola x² = 10z of the xz plane, from its vertex to (√50, 5), drawn as the
//! quadratic Bézier it is (its tangents meet at (√50/2, 0)), closed by the top edge and the axis,
//! and turned a whole turn about z. The ball is cadrs's sphere primitive, its poles on the z axis
//! and its seam toward +y: away from the cup's (the parabola turned, in the xz plane), whose seam
//! it would otherwise touch to fourth order out of the point of contact, the two within 10⁻⁷ of
//! each other for a tenth of a unit (a boolean then sees them overlap there).
use crate::Refusal;
use crate::bench::{ball_oriented, p3, union, v3, xz};
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = 50f64.sqrt();
    let section = Section::on(xz(0.0))
        .bezier(&[(0.0, 0.0), (a / 2.0, 0.0), (a, 5.0)], &[])
        .line((a, 5.0), (0.0, 5.0))
        .line((0.0, 5.0), (0.0, 0.0));
    let cup = turned_about_z(k, &section)?;
    let b = ball_oriented(
        k,
        p3(0.0, 0.0, 5.0),
        5.0,
        v3(0.0, 0.0, 1.0),
        v3(0.0, 1.0, 0.0),
    )?;
    union(k, cup, b)
}

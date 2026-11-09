//! The solid inside one sheet of a hyperboloid of two sheets about the z axis, below the plane
//! z = 6: every point with z ≥ 3√(1 + (x² + y²)/16) and z ≤ 6. It is the hyperbola
//! z²/9 − x²/16 = 1 of the xz plane (its branch with z > 0, from its vertex (0, 0, 3) to
//! x = 4√3) turned a whole turn about the z axis, which it meets at its vertex.
//!
//! The section is sketched on the Front (xz) plane: the hyperbola's arc from its vertex (0, 3) to
//! (4√3, 6) as the rational quadratic it is (a sketch spline, `Curve2::Spline`), the top edge back
//! to the axis and the axis down to the vertex. The arc x = 4 sinh t, z = 3 cosh t for
//! 0 ≤ t ≤ T, cosh T = 2, has its end tangents meeting at (4/√3, 3), weighted cosh(T/2) = √(3/2)
//! there. The section is revolved a whole turn about z.
use crate::Refusal;
use crate::bench::xz;
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let rim = 4.0 * 3f64.sqrt();
    let section = Section::on(xz(0.0))
        .conic(
            (0.0, 3.0),
            (4.0 / 3f64.sqrt(), 3.0),
            1.5f64.sqrt(),
            (rim, 6.0),
        )
        .line((rim, 6.0), (0.0, 6.0))
        .line((0.0, 6.0), (0.0, 3.0));
    turned_about_z(k, &section)
}

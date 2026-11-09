//! The solid inside a hyperboloid of one sheet about the z axis, between the planes z = −6 and
//! z = 6: every point with x² + y² ≤ 16(1 + z²/36) and −6 ≤ z ≤ 6. Its side is the hyperbola
//! x²/16 − z²/36 = 1 of the xz plane (its branch with x > 0) turned a whole turn about z.
//!
//! The section is sketched on the Front (xz) plane: the bottom edge from the axis to (4√2, −6),
//! the hyperbola's arc up to (4√2, 6) as the rational quadratic it is (a sketch spline,
//! `Curve2::Spline`), the top edge back to the axis and the axis down. The arc x = 4 cosh t,
//! z = 6 sinh t for |t| ≤ T, sinh T = 1, has its end tangents meeting at (2√2, 0), weighted
//! cosh T = √2 there. The section is revolved a whole turn about z.
use crate::Refusal;
use crate::bench::xz;
use crate::shared::geometry::{Section, turned_about_z};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let r = 4.0 * 2f64.sqrt();
    let section = Section::on(xz(0.0))
        .line((0.0, -6.0), (r, -6.0))
        .conic((r, -6.0), (r / 2.0, 0.0), 2f64.sqrt(), (r, 6.0))
        .line((r, 6.0), (0.0, 6.0))
        .line((0.0, 6.0), (0.0, -6.0));
    turned_about_z(k, &section)
}

//! The solid paraboloid of `geometry/paraboloid` (x² + y² ≤ 8z up to z = 8: the parabola's arc as
//! the quadratic Bézier it is, closed and turned a whole turn about z), with everything above the
//! plane z = 10⁻⁴ cut away by a block on that plane: the tip, a disc 0.057 across over a bowl
//! 10⁻⁴ deep.
use crate::Refusal;
use crate::bench::{V3, p3, xz};
use crate::shared::geometry::{Section, turned_about_z};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = Section::on(xz(0.0))
        .bezier(&[(0.0, 0.0), (4.0, 0.0), (8.0, 8.0)], &[])
        .line((8.0, 8.0), (0.0, 8.0))
        .line((0.0, 8.0), (0.0, 0.0));
    let paraboloid = turned_about_z(k, &section)?;
    cut_beyond(k, paraboloid, p3(0.0, 0.0, 1e-4), V3::z())
}

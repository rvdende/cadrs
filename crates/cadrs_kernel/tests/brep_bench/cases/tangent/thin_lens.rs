//! Two balls of radius 5, one about the origin and one about (9.999999, 0, 0), overlapping to a
//! depth of 10⁻⁶: their common part, a lens a millionth thick. Each ball is cadrs's sphere
//! primitive.
//!
//! The lens is as thick as cadrs's default merge distance (10⁻⁶ mm, under which two points are
//! one), so the session is set to model a hundred times finer first, as a user does for a part
//! with features this small.
use crate::Refusal;
use crate::bench::{ball, common, p3};
use cadrs_kernel::native::tol::Tolerances;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    k.set_tolerances(Tolerances {
        confusion: 1e-9,
        point: 1e-8,
        retries: [1e-7, 1e-6],
    })?;
    let a = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let b = ball(k, p3(9.999999, 0.0, 0.0), 5.0)?;
    common(k, a, b)
}

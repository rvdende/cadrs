//! A cube of side 10 between the origin and (10, 10, 10), with a saw cut a millionth wide down
//! into it from the top: the slot between x = 5 and x = 5.000001, through the cube in y, from
//! z = 5 up through the top. It is cut with a block running past the cube's front, back and top.
//!
//! The slot is as wide as cadrs's default merge distance (10⁻⁶ mm, under which two points are
//! one), so the session is set to model a hundred times finer first, as a user does for a part
//! with features this small.
use crate::Refusal;
use crate::bench::{block, cut, p3};
use cadrs_kernel::native::tol::Tolerances;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    k.set_tolerances(Tolerances {
        confusion: 1e-9,
        point: 1e-8,
        retries: [1e-7, 1e-6],
    })?;
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let saw = block(k, p3(5.0, -1.0, 5.0), p3(5.000001, 11.0, 11.0))?;
    cut(k, cube, saw)
}

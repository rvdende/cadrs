//! Two cubes of side 10 united: the first between the origin and (10, 10, 10), the second between
//! (5, 0, 10) and (15, 10, 20), standing on the first with its bottom over half the first's top.
use crate::Refusal;
use crate::bench::{block, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let b = block(k, p3(5.0, 0.0, 10.0), p3(15.0, 10.0, 20.0))?;
    union(k, a, b)
}

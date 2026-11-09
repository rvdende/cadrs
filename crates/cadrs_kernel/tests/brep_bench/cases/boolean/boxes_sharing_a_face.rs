//! Two cubes of side 10 side by side, the first between the origin and (10, 10, 10) and the
//! second between (10, 0, 0) and (20, 10, 10), united: the whole face they share disappears.
use crate::Refusal;
use crate::bench::{block, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let b = block(k, p3(10.0, 0.0, 0.0), p3(20.0, 10.0, 10.0))?;
    union(k, a, b)
}

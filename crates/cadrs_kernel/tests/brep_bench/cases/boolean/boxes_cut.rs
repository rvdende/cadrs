//! Two blocks 10 high standing on the xy plane, the first over [0, 10]² and the second over
//! [5, 15]²: the second cut away from the first.
use crate::Refusal;
use crate::bench::{block, cut, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let b = block(k, p3(5.0, 5.0, 0.0), p3(15.0, 15.0, 10.0))?;
    cut(k, a, b)
}

//! Two solid rings linked through each other, not touching: the torus of radii 3 and 0.5 about
//! the z axis at the origin, and the same torus about the line through (3, 0, 0) parallel to y,
//! united, both bodies kept.
//!
//! Each ring is cadrs's torus primitive; their union is cadrs's boolean, which keeps the two
//! solids apart in one body.
use crate::Refusal;
use crate::bench::{p3, torus, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0, 0.5)?;
    let b = torus(k, p3(3.0, 0.0, 0.0), v3(0.0, 1.0, 0.0), 3.0, 0.5)?;
    union(k, a, b)
}

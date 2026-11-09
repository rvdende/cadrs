//! Two solid tori about the z axis, each of radii 5 and 2, the first's middle circle in z = −1
//! and the second's in z = 1: their common part, a ring whose section is the lens of the two
//! tubes' disks.
//!
//! Each torus is cadrs's torus primitive; their common part is cadrs's boolean.
use crate::Refusal;
use crate::bench::{common, p3, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = torus(k, p3(0.0, 0.0, -1.0), v3(0.0, 0.0, 1.0), 5.0, 2.0)?;
    let b = torus(k, p3(0.0, 0.0, 1.0), v3(0.0, 0.0, 1.0), 5.0, 2.0)?;
    common(k, a, b)
}

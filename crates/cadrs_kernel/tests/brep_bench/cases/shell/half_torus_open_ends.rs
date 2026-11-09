//! A half torus shelled open at both ends: a curved pipe. The half with y ≥ 0 of the solid torus about
//! the z axis whose tube has radius 2 about a middle circle of radius 6, shelled inward to a wall 0.5
//! thick, open at both flat faces (the disks about (6, 0, 0) and (−6, 0, 0) in y = 0): the half torus
//! less the half torus of tube radius 1.5.
//!
//! The half torus is cadrs's torus primitive less a block below y = 0; cadrs's Shell feature
//! (inward), the two end disks picked as the faces to remove.
use crate::Refusal;
use crate::bench::{V3, block, cut, p3, torus};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the torus's half with y ≥ 0: the torus less a block below y = 0
    let ring = torus(k, p3(0.0, 0.0, 0.0), V3::z(), 6.0, 2.0)?;
    let below = block(k, p3(-9.0, -9.0, -3.0), p3(9.0, 0.0, 3.0))?;
    let half = cut(k, ring, below)?;
    shell_inward(
        k,
        half,
        &[p3(6.0, 0.0, 0.5), p3(-6.0, 0.0, 0.5)],
        0.5,
        Joins::Rounded,
    )
}

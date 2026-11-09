//! A cylinder shelled open at its side: two separate disks. A cylinder of radius 5 on the z axis from
//! z = 0 to z = 10, shelled inward to a wall 1 thick, open at its curved side: the ends moved 1 inward
//! are z = 1 and z = 9, each running out to the cylinder, so the result is two separate solids, the
//! disks from z = 0 to 1 and from z = 9 to 10.
//!
//! The cylinder is a circle on the Top plane extruded 10; cadrs's Shell feature (inward), its side
//! picked as the face to remove.
use crate::Refusal;
use crate::bench::{V3, cylinder, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 5.0, 10.0)?;
    shell_inward(k, c, &[p3(5.0, 0.0, 5.0)], 1.0, Joins::Rounded)
}

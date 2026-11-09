//! A cone shelled open at its base, its apex moved in. A solid cone on the z axis, its base the disk of
//! radius 6 at z = 0 and its apex at (0, 0, 8), shelled inward to a wall 1 thick, open at its base:
//! the cone less the cone of the same half-angle whose apex is 5/3 lower.
//!
//! The cone is cadrs's cone primitive; cadrs's Shell feature (inward), the base picked as the face
//! to remove.
use crate::Refusal;
use crate::bench::{V3, cone, p3};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cone(k, p3(0.0, 0.0, 0.0), V3::z(), 6.0, 0.0, 8.0)?;
    shell_inward(k, c, &[p3(2.0, 1.0, 0.0)], 1.0, Joins::Rounded)
}

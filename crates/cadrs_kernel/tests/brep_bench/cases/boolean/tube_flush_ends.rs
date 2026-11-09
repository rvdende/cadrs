//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, less a cylinder of radius 3 on the
//! same axis running exactly from z = 0 to z = 10: the cutter's ends lie in the first's ends.
use crate::Refusal;
use crate::bench::{V3, cut, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outer = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 5.0, 10.0)?;
    let inner = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 3.0, 10.0)?;
    cut(k, outer, inner)
}

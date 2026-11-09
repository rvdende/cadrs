//! A cube of side 10 between the origin and (10, 10, 10) united with a cylinder of radius 5 and
//! height 10 standing on the xy plane about x = 10, y = 5: half of it inside the cube, its side
//! touching the cube's faces y = 0 and y = 10 along their edges at x = 10.
use crate::Refusal;
use crate::bench::{V3, block, cylinder, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let boss = cylinder(k, p3(10.0, 5.0, 0.0), V3::z(), 5.0, 10.0)?;
    union(k, cube, boss)
}

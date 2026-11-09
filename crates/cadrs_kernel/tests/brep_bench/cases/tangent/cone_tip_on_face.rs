//! A block from (−10, −10, 0) to (10, 10, 10), united with a solid cone on the z axis whose apex
//! is at the origin, the middle of the block's bottom face, and whose base is the disk of radius
//! 5 in the plane z = 15: the tip touches the bottom face from inside at one point. The cone is
//! cadrs's cone primitive.
use crate::Refusal;
use crate::bench::{block, cone, p3, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-10.0, -10.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let c = cone(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 0.0, 5.0, 15.0)?;
    union(k, b, c)
}

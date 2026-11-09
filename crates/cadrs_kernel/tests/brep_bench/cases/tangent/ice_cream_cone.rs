//! A ball of radius 15 centered on the origin, united with a solid cone on the z axis whose apex
//! is at (0, 0, −25) and whose base is the disk of radius 12 in the plane z = −9: the cone's side
//! is tangent to the sphere along the rim of its base. The ball is cadrs's sphere primitive, its
//! poles on the z axis, and the cone cadrs's cone primitive.
use crate::Refusal;
use crate::bench::{ball, cone, p3, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 15.0)?;
    let c = cone(k, p3(0.0, 0.0, -25.0), v3(0.0, 0.0, 1.0), 0.0, 12.0, 16.0)?;
    union(k, b, c)
}

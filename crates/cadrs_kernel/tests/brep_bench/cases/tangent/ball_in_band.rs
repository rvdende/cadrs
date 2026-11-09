//! A ball of radius 5 centered on the origin, united with a cylinder of the same radius on the z
//! axis, from z = −3 to z = 3: the ball touches the cylinder's wall from inside along its
//! equator. The ball is cadrs's sphere primitive, its poles on the z axis; the band is a circle
//! on the plane z = −3 extruded 6.
use crate::Refusal;
use crate::bench::{ball, cylinder, p3, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let band = cylinder(k, p3(0.0, 0.0, -3.0), v3(0.0, 0.0, 1.0), 5.0, 6.0)?;
    union(k, b, band)
}

//! A solid torus about the z axis, its tube's middle circle of radius 10 in the xy plane and the
//! tube of radius 3, united with a drum, the cylinder of radius 13 on the z axis from z = −1 to
//! 1: the torus touches the drum's wall from inside along its outer equator. The torus is
//! cadrs's torus primitive; the drum a circle extruded.
use crate::Refusal;
use crate::bench::{cylinder, p3, torus, union, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let up = v3(0.0, 0.0, 1.0);
    let ring = torus(k, p3(0.0, 0.0, 0.0), up, 10.0, 3.0)?;
    let drum = cylinder(k, p3(0.0, 0.0, -1.0), up, 13.0, 2.0)?;
    union(k, ring, drum)
}

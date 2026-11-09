//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, the circular edge around its top
//! chamfered with the same distance 5 on both faces, the cylinder's own radius: the top shrinks
//! to its centre and the chamfer is the cone from the circle of radius 5 at z = 5 up to its apex
//! at (0, 0, 10).
//!
//! The cylinder is a circle extruded; its top rim is picked by a point on it and chamfered,
//! equal distance 5.
use crate::Refusal;
use crate::bench::{cylinder, edge_at, p3, v3};
use crate::shared::chamfer::chamfer;
use cadrs_kernel::{BodyId, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let rim = edge_at(k, c, p3(5.0, 0.0, 10.0))?;
    chamfer(k, c, &[rim], ChamferSpec::EqualDistance(5.0))
}

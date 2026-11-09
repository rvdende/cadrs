//! A cylinder of radius 5 on the z axis from z = −1 to z = 10 (a circle extruded), with
//! everything below the plane z = 0 cut away and then everything above the plane z = x tan 30°,
//! which meets it along the y axis: the hoof over the half disc where x ≥ 0.
use crate::Refusal;
use crate::bench::{V3, cylinder, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, -1.0), V3::z(), 5.0, 11.0)?;
    let floor = cut_beyond(k, c, p3(0.0, 0.0, 0.0), -V3::z())?;
    let t = 30f64.to_radians().tan();
    cut_beyond(k, floor, p3(0.0, 0.0, 0.0), v3(-t, 0.0, 1.0))
}

//! A cylinder of radius 5 on the z axis from z = 0 to z = 10, bored through by a hole of radius 4
//! about the line x = 0.999999, y = 0: a circle extruded from z = −1 to z = 11, cut away. The wall
//! left at (5, 0) is 10⁻⁶ thick. The hole's circle is sketched with its x axis along −x, so that
//! its seam is on the far side of the hole, not beside the cylinder's own seam at (5, 0).
use crate::Refusal;
use crate::bench::{V3, cut, cylinder, extrude, p3, plane, round, sketch};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let tube = cylinder(k, p3(0.0, 0.0, 0.0), V3::z(), 5.0, 10.0)?;
    let on = plane(p3(0.999999, 0.0, -1.0), -V3::x(), V3::z());
    let hole = extrude(k, &sketch(on, vec![round((0.0, 0.0), 4.0)]), 12.0)?;
    cut(k, tube, hole)
}

//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) grooved all round by a
//! wheel with a sharp V rim (the triangle with corners (4, 0, 10) and (6.5, 0, 10) ± 2.5 tan 30°·w
//! turned about the axis through (14, 0, 10) along w = (0, sin 20°, cos 20°)) turned a whole turn
//! about the z axis.
//!
//! The wheel is its triangle revolved; what it passes through is cadrs's sweep of a solid (turned
//! whole turns about the z axis: the solid of revolution it generates), cut from the rod.
use crate::Refusal;
use crate::bench::{axis, cut, p3, plane, polygon, revolve, sketch, v3};
use crate::shared::sweep::first;
use crate::shared::swept::rod;
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, SolidMotion};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let t = 20f64.to_radians();
    let (o, w) = (p3(14.0, 0.0, 10.0), v3(0.0, t.sin(), t.cos()));
    // Sketch x from the wheel's axis toward the rim's dip (−x), sketch y along w.
    let x = v3(-1.0, 0.0, 0.0);
    let on = plane(o, x, x.cross(&w));
    let h = 2.5 * 30f64.to_radians().tan();
    let triangle = polygon(&[(10.0, 0.0), (7.5, -h), (7.5, h)]);
    let wheel = revolve(k, &sketch(on, vec![triangle]), axis(o, w), None)?;
    let motion = SolidMotion::Turn {
        axis: crate::shared::swept::z_axis(),
    };
    let swept = first(k.sweep_solid(wheel, &motion)?)?;
    let r = rod(k)?;
    cut(k, r, swept)
}

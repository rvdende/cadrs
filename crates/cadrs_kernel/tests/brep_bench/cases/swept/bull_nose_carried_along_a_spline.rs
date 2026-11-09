//! A block (the box from (−6, −6, −8) to (36, 26, 0)) grooved by a bull-nose mill leaning 30°
//! toward +x (a cylinder of radius 2 along a = (sin 30°, 0, cos 30°) from c = (0, 0, −1) to c + 3a,
//! its end corner rounded to radius 0.5, in the frame riding the curve) carried along the cubic
//! Bézier curve on (0, 0, 0), (10, 0, 0), (20, 20, 0), (30, 20, 0) by a frame along its tangent,
//! upright.
//!
//! The mill is its half section turned about its axis; what it passes through is cadrs's sweep of
//! a solid along a path (meshed), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, plane, v3};
use crate::shared::geometry::Section;
use crate::shared::swept::{bezier_path, carried};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-6.0, -6.0, -8.0), p3(36.0, 26.0, 0.0))?;
    let a = v3(30f64.to_radians().sin(), 0.0, 30f64.to_radians().cos());
    let c = p3(0.0, 0.0, -1.0);
    let radial = v3(1.0, 0.0, 0.0) - a * a.x;
    let on = plane(c, radial, radial.cross(&a));
    let section = Section::on(on)
        .line((0.0, 0.0), (1.5, 0.0))
        .conic((1.5, 0.0), (2.0, 0.0), 2f64.sqrt() / 2.0, (2.0, 0.5))
        .line((2.0, 0.5), (2.0, 3.0))
        .line((2.0, 3.0), (0.0, 3.0))
        .line((0.0, 3.0), (0.0, 0.0));
    let mill = section.turned(k, c, a)?;
    let path = bezier_path([(0.0, 0.0), (10.0, 0.0), (20.0, 20.0), (30.0, 20.0)]);
    let swept = carried(k, mill, path)?;
    cut(k, b, swept)
}

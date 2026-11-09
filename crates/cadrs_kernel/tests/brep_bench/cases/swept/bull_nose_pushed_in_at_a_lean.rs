//! A block (the box from (−8, −8, −8) to (8, 8, 0)) bored by a bull-nose mill leaning 30° toward +x
//! (a cylinder of radius 2 along a = (sin 30°, 0, cos 30°) from c = (0, 0, 4) to c + 3a, its end
//! corner rounded to radius 0.5) pushed straight down 8, stopping wholly inside it.
//!
//! The mill is its half section turned about its axis (the round a quarter circle, drawn as the
//! rational quadratic it is); what it passes through is cadrs's sweep of a solid (one slide), cut
//! from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, plane, v3};
use crate::shared::geometry::Section;
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-8.0, -8.0, -8.0), p3(8.0, 8.0, 0.0))?;
    let a = v3(30f64.to_radians().sin(), 0.0, 30f64.to_radians().cos());
    let c = p3(0.0, 0.0, 4.0);
    // The half section in the plane through the axis and y = 0: sketch x along the radius
    // (square to a in that plane), sketch y along a.
    let radial = v3(1.0, 0.0, 0.0) - a * a.x;
    let on = plane(c, radial, radial.cross(&a));
    let section = Section::on(on)
        .line((0.0, 0.0), (1.5, 0.0))
        .conic((1.5, 0.0), (2.0, 0.0), 2f64.sqrt() / 2.0, (2.0, 0.5))
        .line((2.0, 0.5), (2.0, 3.0))
        .line((2.0, 3.0), (0.0, 3.0))
        .line((0.0, 3.0), (0.0, 0.0));
    let mill = section.turned(k, c, a)?;
    let swept = slid(k, mill, v3(0.0, 0.0, -8.0))?;
    cut(k, b, swept)
}

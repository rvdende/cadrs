//! A block (the box from (−10, −10, −8) to (30, 25, 0)) grooved by an upright bull-nose mill (a
//! cylinder of radius 2 from z = −1.5 to 5, its bottom corner rounded to radius 0.5) run along the
//! cubic Bézier curve on (0, 0, 0), (30, 20, 0), (−10, 20, 0), (20, 0, 0), which loops over itself.
//!
//! The mill is its half section turned about its axis; what it passes through is cadrs's sweep of
//! a solid along a path (meshed), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::{bezier_path, carried};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-10.0, -10.0, -8.0), p3(30.0, 25.0, 0.0))?;
    let w = 2f64.sqrt() / 2.0;
    let section = Section::on(xz(0.0))
        .line((0.0, -1.5), (1.5, -1.5))
        .conic((1.5, -1.5), (2.0, -1.5), w, (2.0, -1.0))
        .line((2.0, -1.0), (2.0, 5.0))
        .line((2.0, 5.0), (0.0, 5.0))
        .line((0.0, 5.0), (0.0, -1.5));
    let mill = section.turned(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let path = bezier_path([(0.0, 0.0), (30.0, 20.0), (-10.0, 20.0), (20.0, 0.0)]);
    let swept = carried(k, mill, path)?;
    cut(k, b, swept)
}

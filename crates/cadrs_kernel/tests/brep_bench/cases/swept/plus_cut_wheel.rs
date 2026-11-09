//! A wheel, the disk of radius 40 about the vertical line through (−50, 0) from z = −8 to −2,
//! cut by a plus (arms 30 long and 8 wide along ±x and ±y, from z = −10 to 0) turning about the
//! z axis by t while the wheel turns by −2t/3 about its own axis, over t from 0° to 540°.
//!
//! The wheel is a circle extruded. Seen from it, the plus turns by t about the z axis and then,
//! with the frame, by 2t/3 about the wheel's axis: cadrs's `Kernel::sweep_solid` with that planar
//! motion (its sides' envelopes and its corners' paths, the region they bound extruded through
//! the plus's height) gives what the plus fills, cut from the wheel.
use crate::Refusal;
use crate::bench::{cut, extrude, p3, polygon, round, sketch, xy};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, PlanarStep, SolidMotion};
use std::f64::consts::PI;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let wheel = extrude(k, &sketch(xy(-8.0), vec![round((-50.0, 0.0), 40.0)]), 6.0)?;
    let plus = [
        (30.0, -4.0),
        (30.0, 4.0),
        (4.0, 4.0),
        (4.0, 30.0),
        (-4.0, 30.0),
        (-4.0, 4.0),
        (-30.0, 4.0),
        (-30.0, -4.0),
        (-4.0, -4.0),
        (-4.0, -30.0),
        (4.0, -30.0),
        (4.0, -4.0),
    ];
    let tool = extrude(k, &sketch(xy(-10.0), vec![polygon(&plus)]), 10.0)?;
    let motion = SolidMotion::Planar {
        normal: crate::bench::unit(crate::bench::v3(0.0, 0.0, 1.0)),
        steps: vec![
            PlanarStep::Turn {
                about: p3(0.0, 0.0, 0.0),
                from: 0.0,
                to: 3.0 * PI,
            },
            PlanarStep::Turn {
                about: p3(-50.0, 0.0, 0.0),
                from: 0.0,
                to: 2.0 * PI,
            },
        ],
    };
    let swept = k.sweep_solid(tool, &motion)?;
    let swept = swept
        .bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the sweep made no body"))?;
    cut(k, wheel, swept)
}

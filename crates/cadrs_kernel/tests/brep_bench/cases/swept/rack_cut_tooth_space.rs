//! A gear blank, the disk of radius 22 about the y axis from y = 0 to y = 6, with one tooth space
//! cut by a rack tooth (module 2, 20° pressure angle) rolled on its pitch circle of radius 20
//! from 60° on one side to 60° on the other.
//!
//! The blank is a circle on the plane y = 0 extruded 6; the tooth is its trapezoid (corners
//! (18, ±0.842856) and (24, ±3.026666) in (x, z)) on the plane y = −2 extruded 10. Seen from the
//! blank the rack rolls on the pitch circle: slid 20φ along its pitch line (along z) and turned
//! φ about the y axis together, φ from −60° to 60° (its pitch point at rest on the circle).
//! cadrs's `Kernel::sweep_solid` with that planar motion gives what the tooth fills, cut from the
//! blank.
use crate::Refusal;
use crate::bench::{cut, extrude, p3, plane, polygon, round, sketch, v3};
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, PlanarStep, SolidMotion};
use std::f64::consts::PI;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // Planes square to y with sketch x along the model's x and sketch y along its z.
    let at = |y: f64| plane(p3(0.0, y, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, -1.0, 0.0));
    let blank = extrude(k, &sketch(at(0.0), vec![round((0.0, 0.0), 22.0)]), -6.0)?;
    let tooth = [
        (18.0, -0.842856),
        (24.0, -3.026666),
        (24.0, 3.026666),
        (18.0, 0.842856),
    ];
    let rack = extrude(k, &sketch(at(-2.0), vec![polygon(&tooth)]), -10.0)?;
    let (f0, f1) = (-PI / 3.0, PI / 3.0);
    let motion = SolidMotion::Planar {
        normal: crate::bench::unit(v3(0.0, 1.0, 0.0)),
        steps: vec![
            PlanarStep::Slide {
                from: v3(0.0, 0.0, 20.0 * f0),
                to: v3(0.0, 0.0, 20.0 * f1),
            },
            PlanarStep::Turn {
                about: p3(0.0, 0.0, 0.0),
                from: f0,
                to: f1,
            },
        ],
    };
    let swept = k.sweep_solid(rack, &motion)?;
    let swept = swept
        .bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the sweep made no body"))?;
    cut(k, blank, swept)
}

//! The solid a cube (the box from (2.4, −0.6, 1.4) to (3.6, 0.6, 2.6)) fills as it turns by t/4
//! about the upright through (2, 0, 0) and then by −t about the line through (0, 0.5, 0.5) along
//! x, as t runs from −75° to 75°.
//!
//! cadrs's sweep of a solid (`SolidMotion::Rigid`, its two turns applied in order at each
//! instant).
use crate::Refusal;
use crate::bench::{axis, block, p3, v3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, RigidStep, SolidMotion};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(2.4, -0.6, 1.4), p3(3.6, 0.6, 2.6))?;
    let t = 75f64.to_radians();
    let steps = vec![
        RigidStep::Turn {
            axis: axis(p3(2.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)),
            from: -t / 4.0,
            to: t / 4.0,
        },
        RigidStep::Turn {
            axis: axis(p3(0.0, 0.5, 0.5), v3(1.0, 0.0, 0.0)),
            from: t,
            to: -t,
        },
    ];
    first(k.sweep_solid(cube, &SolidMotion::Rigid { steps })?)
}

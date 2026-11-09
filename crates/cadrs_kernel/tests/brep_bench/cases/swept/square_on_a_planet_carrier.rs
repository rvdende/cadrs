//! The solid a square bar (the prism from z = 0 to 2 on the square of side 2 about (5, 0)) fills
//! as it is carried round the z axis a whole turn while turning about its own centre by −2t, as a
//! planet gear on its carrier.
//!
//! cadrs's sweep of a solid in its planes (`SolidMotion::Planar`, its steps applied in order at
//! each instant): a turn of −3t about the bar's centre, then the carrier's turn of t about the z
//! axis; a prism along the motion's normal fills the region its section sweeps, extruded.
use crate::Refusal;
use crate::bench::{block, p3, v3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, Kernel, PlanarStep, SolidMotion};
use std::f64::consts::TAU;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let bar = block(k, p3(4.0, -1.0, 0.0), p3(6.0, 1.0, 2.0))?;
    let steps = vec![
        PlanarStep::Turn {
            about: p3(5.0, 0.0, 0.0),
            from: 0.0,
            to: -3.0 * TAU,
        },
        PlanarStep::Turn {
            about: p3(0.0, 0.0, 0.0),
            from: 0.0,
            to: TAU,
        },
    ];
    let motion = SolidMotion::Planar {
        normal: crate::bench::unit(v3(0.0, 0.0, 1.0)),
        steps,
    };
    first(k.sweep_solid(bar, &motion)?)
}

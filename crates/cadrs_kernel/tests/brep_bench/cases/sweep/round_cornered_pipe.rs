//!
//! cadrs's Sweep of the disk of radius 3 on the Right plane at (−30, 0, 0) (square to the first
//! run) along the two runs drawn on the Top plane, its corner round (`SweepCorners::Round`: the
//! disk turned about the corner's axis, the z axis, from square to the first run to square to the
//! second).
use crate::Refusal;
use crate::bench::{line, p2, round, sketch, xy, yz};
use crate::shared::sweep::{on, sweep_round_corners};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let disk = sketch(yz(-30.0), vec![round((0.0, 0.0), 3.0)]);
    let path = vec![
        on(xy(0.0), line(p2(-30.0, 0.0), p2(0.0, 0.0))),
        on(xy(0.0), line(p2(0.0, 0.0), p2(0.0, 30.0))),
    ];
    sweep_round_corners(k, &disk, path)
}

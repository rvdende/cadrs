//! A plate (the box from (−15, −15, 0) to (15, 15, 4)) with a hole punched through it by a coil
//! spring (what a ball of radius 0.4 fills as its centre runs three turns of the helix of radius 2
//! about the z axis from (2, 0, −12), rising 1.5 a turn) slid along (3, 1, 18).
//!
//! The spring is its circular section swept along the helix (cadrs's Sweep) with a ball at each
//! end (united); what it passes through is cadrs's sweep of a solid (one slide), cut from the
//! plate.
use crate::Refusal;
use crate::bench::{ball, block, cut, p3, round, sketch, square_to, union_each, v3};
use crate::shared::sweep::{helix, helix_at, sweep};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-15.0, -15.0, 0.0), p3(15.0, 15.0, 4.0))?;
    let o = p3(0.0, 0.0, -12.0);
    let (start, tangent) = helix_at(o, 2.0, 1.5, 0.0, 0.0);
    let section = sketch(square_to(start, tangent), vec![round((0.0, 0.0), 0.4)]);
    let coil = sweep(k, &section, helix(o, 2.0, 1.5, 0.0, 3.0))?;
    let first = ball(k, p3(2.0, 0.0, -12.0), 0.4)?;
    let last = ball(k, p3(2.0, 0.0, -7.5), 0.4)?;
    let spring = union_each(k, coil, &[first, last])?;
    let swept = slid(k, spring, v3(3.0, 1.0, 18.0))?;
    cut(k, plate, swept)
}

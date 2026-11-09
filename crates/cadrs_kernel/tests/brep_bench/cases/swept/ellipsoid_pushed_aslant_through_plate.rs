//! A plate (the box from (−15, −15, 0) to (15, 15, 4)) holed by an ellipsoid (semi-axes 3, 2 and
//! 1.5 along x, y and z, centered at (−2, −1.5, −4)) pushed along (4, 3, 12), wholly through it.
//!
//! The ellipsoid is a unit ball (cadrs's sphere primitive) stretched along the axes (as
//! `geometry/ellipsoid`) and moved; what it passes through is cadrs's sweep of a solid (one
//! slide), cut from the plate.
use crate::Refusal;
use crate::bench::{ball, block, cut, moved, p3, v3};
use crate::shared::sweep::first;
use crate::shared::swept::slid;
use cadrs_kernel::Kernel;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-15.0, -15.0, 0.0), p3(15.0, 15.0, 4.0))?;
    let unit = ball(k, p3(0.0, 0.0, 0.0), 1.0)?;
    let stretched = first(k.scale_axes(unit, p3(0.0, 0.0, 0.0), v3(3.0, 2.0, 1.5))?)?;
    let e = moved(k, stretched, v3(-2.0, -1.5, -4.0))?;
    let swept = slid(k, e, v3(4.0, 3.0, 12.0))?;
    cut(k, plate, swept)
}

//! A plate (the box from (−15, −15, 0) to (15, 15, 4)) holed by a tilted box (4 × 3 × 2 along
//! e₁ = (2, 1, 2)/3, e₂ = (−2, 2, 1)/3 and e₃ = (1, 2, −2)/3, centered at (−2, −1, −8)) pushed along
//! (4, 2, 18), wholly through it.
//!
//! The box is a block about the origin carried by the rotation whose columns are e₁, e₂, e₃ and
//! moved to its center (cadrs's affine transform); what it passes through is cadrs's sweep of a
//! solid (one slide), cut from the plate.
use crate::Refusal;
use crate::bench::{block, cut, p3, v3};
use crate::shared::sweep::first;
use crate::shared::swept::slid;
use cadrs_kernel::Kernel;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-15.0, -15.0, 0.0), p3(15.0, 15.0, 4.0))?;
    let local = block(k, p3(-2.0, -1.5, -1.0), p3(2.0, 1.5, 1.0))?;
    let m = nalgebra::Matrix3::from_columns(&[
        v3(2.0, 1.0, 2.0) / 3.0,
        v3(-2.0, 2.0, 1.0) / 3.0,
        v3(1.0, 2.0, -2.0) / 3.0,
    ]);
    let tilted = first(k.transform_affine(local, m, v3(-2.0, -1.0, -8.0))?)?;
    let swept = slid(k, tilted, v3(4.0, 2.0, 18.0))?;
    cut(k, plate, swept)
}

//! A block (the box from (−8, −8, −8) to (8, 8, 0)) bored by a ring (the torus about (0, 0, 4), its
//! axis leaning 35° from upright toward +x, radii 3 and 1) pushed straight down 8, stopping wholly
//! inside it.
//!
//! The ring is cadrs's torus primitive; what it passes through is cadrs's sweep of a solid (one
//! slide), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, torus, v3};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-8.0, -8.0, -8.0), p3(8.0, 8.0, 0.0))?;
    let lean = 35f64.to_radians();
    let ring = torus(
        k,
        p3(0.0, 0.0, 4.0),
        v3(lean.sin(), 0.0, lean.cos()),
        3.0,
        1.0,
    )?;
    let swept = slid(k, ring, v3(0.0, 0.0, -8.0))?;
    cut(k, b, swept)
}

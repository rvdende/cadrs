//! A block (the box from (−6, −6, −8) to (36, 26, 0)) grooved by a key (the box from
//! (−1.5, −0.8, −1.5) to (1.5, 0.8, 2.5) in the frame riding the curve) carried along the cubic
//! Bézier curve on (0, 0, 0), (10, 0, 0), (20, 20, 0), (30, 20, 0) by a frame along its tangent,
//! upright.
//!
//! What the key passes through is cadrs's sweep of a solid along a path (meshed: the region the
//! key's places hold, from its mesh's distance), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3};
use crate::shared::swept::{bezier_path, carried};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-6.0, -6.0, -8.0), p3(36.0, 26.0, 0.0))?;
    let key = block(k, p3(-1.5, -0.8, -1.5), p3(1.5, 0.8, 2.5))?;
    let path = bezier_path([(0.0, 0.0), (10.0, 0.0), (20.0, 20.0), (30.0, 20.0)]);
    let swept = carried(k, key, path)?;
    cut(k, b, swept)
}

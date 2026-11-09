//! A block (the box from (−6, −6, −8) to (36, 26, 0)) grooved by a ring (the torus about (0, 0, 0.5)
//! along the y axis, radii 1.5 and 0.5, in the frame riding the curve) carried along the cubic
//! Bézier curve on (0, 0, 0), (10, 0, 0), (20, 20, 0), (30, 20, 0) by a frame along its tangent,
//! upright.
//!
//! What the ring passes through is cadrs's sweep of a solid along a path (meshed: the region the
//! ring's places hold, from its distance in closed form), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, torus, v3};
use crate::shared::swept::{bezier_path, carried};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-6.0, -6.0, -8.0), p3(36.0, 26.0, 0.0))?;
    let ring = torus(k, p3(0.0, 0.0, 0.5), v3(0.0, 1.0, 0.0), 1.5, 0.5)?;
    let path = bezier_path([(0.0, 0.0), (10.0, 0.0), (20.0, 20.0), (30.0, 20.0)]);
    let swept = carried(k, ring, path)?;
    cut(k, b, swept)
}

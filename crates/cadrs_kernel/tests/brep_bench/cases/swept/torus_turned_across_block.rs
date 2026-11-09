//! A sector of a ring about the z axis (radii 8 to 12, z from −1 to 1, polar angle 0° to 60°),
//! grooved by a torus (center (5, 0, 0), radii 2.5 and 2, its axis leaning 25° from z toward +y)
//! turned rigidly about the z axis from −140° to +140°.
//!
//! The block is its section (r from 8 to 12, z from −1 to 1) revolved 60° about z; the torus is
//! cadrs's torus primitive, its turn cadrs's sweep of a solid over part of a turn
//! (`Kernel::cut_swept`, a rigid turn about the z axis from −140° to +140°), cut from the block.
use crate::Refusal;
use crate::bench::{axis, p3, rectangle, revolve, sketch, torus, v3, xz};
use crate::shared::swept::{cut_swept, swing, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let z = axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0));
    let ring = sketch(xz(0.0), vec![rectangle(8.0, -1.0, 12.0, 1.0)]);
    let block = revolve(k, &ring, z, Some(60.0))?;
    let (s, c) = 25f64.to_radians().sin_cos();
    let cutter = torus(k, p3(5.0, 0.0, 0.0), v3(0.0, s, c), 2.5, 2.0)?;
    cut_swept(k, block, cutter, &swing(z_axis(), -140.0, 140.0))
}

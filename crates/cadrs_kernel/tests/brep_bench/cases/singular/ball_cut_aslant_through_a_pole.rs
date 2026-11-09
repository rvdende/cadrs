//! A ball of radius 5 centered on the origin, cut by the plane x + z = 5, which leans at 45° and
//! passes through (0, 0, 5) and (5, 0, 0): the cap where x + z > 5 cut away, the rest kept. The
//! ball's poles are on the z axis, so the rim of the cut passes through the pole at (0, 0, 5),
//! crossing every meridian there.
//!
//! The ball is cadrs's sphere primitive, its poles at (0, 0, ±5). The cap is cut with a block
//! sketched on the plane x + z = 5 (through (2.5, 0, 2.5), normal (1, 0, 1)) and extruded away
//! from the center.
use crate::Refusal;
use crate::bench::{ball, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    cut_beyond(k, b, p3(2.5, 0.0, 2.5), v3(1.0, 0.0, 1.0))
}

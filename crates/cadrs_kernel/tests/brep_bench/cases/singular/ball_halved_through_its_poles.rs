//! A ball of radius 5 centered on the origin, with everything on the side x < 0 of the yz plane
//! cut away: the half ball on the side of +x. The ball's poles are on the z axis, at (0, 0, ±5),
//! both in the cutting plane, so the cut runs along a meridian from one pole to the other.
//!
//! The ball is cadrs's sphere primitive, its poles on the z axis. The half x < 0 is cut with a
//! block sketched on the yz plane and extruded toward −x.
use crate::Refusal;
use crate::bench::{ball, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    cut_beyond(k, b, p3(0.0, 0.0, 0.0), v3(-1.0, 0.0, 0.0))
}

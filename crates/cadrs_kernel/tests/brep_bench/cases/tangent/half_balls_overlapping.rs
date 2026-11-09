//! Two halves of the ball of radius 5 about the origin, united: the half with z ≥ 0 of a ball whose
//! poles are on the z axis, and the half with x ≥ 0 of one whose poles are on the x axis. Each
//! ball is cadrs's sphere primitive, its poles on that axis, each half cut from it by a block on
//! the plane through the origin.
use crate::Refusal;
use crate::bench::{V3, p3, union};
use crate::shared::singular::{ball_about, cut_beyond};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let o = p3(0.0, 0.0, 0.0);
    let up = ball_about(k, o, V3::z(), 5.0)?;
    let up = cut_beyond(k, up, o, -V3::z())?;
    let side = ball_about(k, o, V3::x(), 5.0)?;
    let side = cut_beyond(k, side, o, -V3::x())?;
    union(k, up, side)
}

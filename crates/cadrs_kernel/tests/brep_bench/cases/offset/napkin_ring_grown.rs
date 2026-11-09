//! A napkin ring, a ball of radius 5 about the origin with a hole of radius 3 bored through it
//! along the z axis, grown by 1: the solid of every point within 1 of it. The sphere moves out to
//! radius 6, the bore narrows to radius 2, and each rim becomes a piece of a torus's tube of
//! radius 1.
//!
//! The ball is cadrs's sphere primitive, its poles on the z axis. The bore is a circle extruded
//! through it and cut. Then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{ball, cut, cylinder, p3, v3};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let bore = cylinder(k, p3(0.0, 0.0, -6.0), v3(0.0, 0.0, 1.0), 3.0, 12.0)?;
    let ring = cut(k, b, bore)?;
    grown(k, ring, 1.0)
}

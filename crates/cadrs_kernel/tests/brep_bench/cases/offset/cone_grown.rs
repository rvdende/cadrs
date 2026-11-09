//! A cone on the z axis with a base of radius 3 at z = 0 and its apex at (0, 0, 4), grown by 1:
//! the solid of every point within 1 of the cone. The base moves down to z = −1, the side out to
//! a cone of the same angle, the rim becomes a piece of a torus and the apex a cap of a ball.
//!
//! The cone is cadrs's cone primitive. Then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{cone, p3, v3};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cone(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0, 0.0, 4.0)?;
    grown(k, c, 1.0)
}

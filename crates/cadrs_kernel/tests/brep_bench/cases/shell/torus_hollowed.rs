//! A torus hollowed, its cavity closed. A solid torus about the z axis whose tube has radius 2 about a
//! middle circle of radius 6, hollowed to a wall 0.5 thick with no opening: the torus less the torus
//! of tube radius 1.5.
//!
//! The torus is cadrs's torus primitive; cadrs's Shell feature with Hollow set (no face removed).
use crate::Refusal;
use crate::bench::{V3, p3, torus};
use crate::shared::shell::hollow;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let t = torus(k, p3(0.0, 0.0, 0.0), V3::z(), 6.0, 2.0)?;
    hollow(k, t, 0.5)
}

//! A solid cone on the z axis, its base the disk of radius 5 in the xy plane centered on the
//! origin, its apex at (0, 0, 10), cut by the plane 4x + z = 10, which passes through the apex
//! and meets the base plane in the line x = 2.5: the side where 4x + z > 10, away from the axis,
//! cut away. The plane meets the cone's side only in the two rulings through the ends of the
//! chord x = 2.5 of the base, which meet at the apex.
//!
//! The cone is cadrs's cone primitive. The far side is cut with a block sketched on the plane
//! 4x + z = 10 (through (2.5, 0, 0), normal (4, 0, 1)) and extruded away from the axis.
use crate::Refusal;
use crate::bench::{cone, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cone(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 0.0, 10.0)?;
    cut_beyond(k, c, p3(2.5, 0.0, 0.0), v3(4.0, 0.0, 1.0))
}

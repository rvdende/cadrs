//! A solid cone on the z axis, its base the disk of radius 5 in the xy plane centered on the
//! origin, its apex at (0, 0, 10), with everything on the side x < 0 of the yz plane cut away.
//! The cutting plane holds the axis, so it passes through the apex and meets the side in two
//! rulings.
//!
//! The cone is cadrs's cone primitive. The half x < 0 is cut with a block sketched on the yz
//! plane and extruded toward −x.
use crate::Refusal;
use crate::bench::{cone, p3, v3};
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cone(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 0.0, 10.0)?;
    cut_beyond(k, c, p3(0.0, 0.0, 0.0), v3(-1.0, 0.0, 0.0))
}

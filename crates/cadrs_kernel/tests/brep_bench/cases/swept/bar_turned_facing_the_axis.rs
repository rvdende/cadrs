//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! cut by a bar leaning 20° out from upright in the plane y = 0 (its inner side the plane through
//! (10, 0, 0) square to (cos 20°, 0, −sin 20°), 3 thick, within 2 of y = 0 and 3 either way
//! along its length) turned about the z axis from −140° to +140°.
//!
//! The bar is a block turned 20° about the y axis and moved to (10, 0, 0); what it passes through
//! is cadrs's sweep of a solid (one turn), cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{axis, block, cut, moved, p3, turned, v3};
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let sector = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    // In its own frame: 3 thick along x (its inner side x = 0), 4 across along y, 6 long along z;
    // turned 20° about y, x goes to (cos 20°, 0, −sin 20°) and z to (sin 20°, 0, cos 20°).
    let local = block(k, p3(0.0, -2.0, -3.0), p3(3.0, 2.0, 3.0))?;
    let leaning = turned(k, local, axis(p3(0.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)), 20.0)?;
    let bar = moved(k, leaning, v3(10.0, 0.0, 0.0))?;
    let swept = swung(k, bar, z_axis(), -140.0, 140.0)?;
    cut(k, sector, swept)
}

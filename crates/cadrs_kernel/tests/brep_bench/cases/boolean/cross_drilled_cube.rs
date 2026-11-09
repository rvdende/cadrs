//! A cube from the origin to (30, 30, 30) drilled right through by fifteen holes of radius 2.5:
//! three along x and three along y at z = 15, and nine along z, on the lines 7.5, 15 and 22.5.
//! Each hole is a circle extruded from 1 outside one face to 1 outside the other, cut away one
//! after another.
use crate::Refusal;
use crate::bench::{V3, block, cut_each, cylinder, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(30.0, 30.0, 30.0))?;
    let at = [7.5, 15.0, 22.5];
    let mut holes = Vec::new();
    for &a in &at {
        holes.push(cylinder(k, p3(-1.0, a, 15.0), V3::x(), 2.5, 32.0)?);
        holes.push(cylinder(k, p3(a, -1.0, 15.0), V3::y(), 2.5, 32.0)?);
    }
    for &x in &at {
        for &y in &at {
            holes.push(cylinder(k, p3(x, y, -1.0), V3::z(), 2.5, 32.0)?);
        }
    }
    cut_each(k, cube, &holes)
}

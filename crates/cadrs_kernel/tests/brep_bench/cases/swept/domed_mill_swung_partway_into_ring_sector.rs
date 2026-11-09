//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! notched by a mill with a domed end (a cylinder of radius 1 about the upright through (10, 0) up
//! to z = 3, ended below by the cap of the sphere of radius 2 about (10, 0, 1.2)) turned about the
//! z axis from −140° to +30°.
//!
//! The mill is its half section turned about its axis: the sphere's 30° arc from its lowest point
//! (10, 0, −0.8) to the rim (11, 0, 1.2 − √3), drawn as the rational quadratic it is, and the
//! cylinder's side up to z = 3. What it passes through is cadrs's sweep of a solid (one turn),
//! cut from the block (cadrs's cylindrical box).
use crate::Refusal;
use crate::bench::{cut, p3, v3, xz};
use crate::shared::geometry::Section;
use crate::shared::swept::{ring_sector, swung, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    // The sphere's arc from angle −90° to −60° about (10, 1.2) in the (x, z) plane: its tangents
    // meet on the bisector at −75°, 2 / cos 15° out, its weight cos 15°.
    let half = 15f64.to_radians();
    let m = (
        10.0 + 2.0 / half.cos() * (-75f64).to_radians().cos(),
        1.2 + 2.0 / half.cos() * (-75f64).to_radians().sin(),
    );
    let rim = 1.2 - 3f64.sqrt();
    let section = Section::on(xz(0.0))
        .conic((10.0, -0.8), m, half.cos(), (11.0, rim))
        .line((11.0, rim), (11.0, 3.0))
        .line((11.0, 3.0), (10.0, 3.0))
        .line((10.0, 3.0), (10.0, -0.8));
    let mill = section.turned(k, p3(10.0, 0.0, 0.0), v3(0.0, 0.0, 1.0))?;
    let swept = swung(k, mill, z_axis(), -140.0, 30.0)?;
    cut(k, block, swept)
}

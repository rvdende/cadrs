//! A drum, the cylinder of radius 8 on the z axis from z = 0 to z = 6, with a flange around its
//! side, a ring from radius 8 to 11 and from z = 2.2 to z = 3.8, shrunk by 1: the solid of every
//! point at least 1 from its outside. The flange, thinner than 2, is taken away whole; the drum
//! moves in to radius 7 and z = 1 to 5, and the flange's inside edges become concave pieces of
//! tori meeting in a low ridge at radius 7.4, z = 3.
//!
//! The drum a circle extruded; the flange an annulus sketched at z = 2.2 and extruded 1.6,
//! united with it; then cadrs's offset inward.
use crate::Refusal;
use crate::bench::{cylinder, extrude, p3, round, sketch_with_holes, union, v3, xy};
use crate::shared::offset::shrunk;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 8.0, 6.0)?;
    let ring = sketch_with_holes(
        xy(2.2),
        round((0.0, 0.0), 11.0),
        vec![round((0.0, 0.0), 8.0)],
    );
    let flange = extrude(k, &ring, 1.6)?;
    let part = union(k, drum, flange)?;
    shrunk(k, part, 1.0)
}

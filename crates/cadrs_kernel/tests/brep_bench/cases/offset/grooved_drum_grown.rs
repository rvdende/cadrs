//! A drum, the cylinder of radius 10 on the z axis from z = 0 to z = 10, with a groove around its
//! side from z = 4.2 to z = 5.8, cut 3 deep to radius 7, grown by 1: the solid of every point
//! within 1 of it. The groove, narrower than 2, fills; the drum grows to radius 11 and z = −1 to
//! 11 with rounded rims, and the groove's outer rims become pieces of tori meeting in a shallow
//! V-shaped notch at radius 10.6, z = 5.
//!
//! The drum a circle extruded; the groove an annulus from radius 7 to beyond the drum sketched
//! at z = 4.2 and extruded 1.6, cut from it; then cadrs's offset with rounded edges.
use crate::Refusal;
use crate::bench::{cut, cylinder, extrude, p3, round, sketch_with_holes, v3, xy};
use crate::shared::offset::grown;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 10.0, 10.0)?;
    let ring = sketch_with_holes(
        xy(4.2),
        round((0.0, 0.0), 12.0),
        vec![round((0.0, 0.0), 7.0)],
    );
    let groove = extrude(k, &ring, 1.6)?;
    let part = cut(k, drum, groove)?;
    grown(k, part, 1.0)
}

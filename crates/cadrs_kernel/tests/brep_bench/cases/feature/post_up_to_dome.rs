//! A closed hollow dome with a post inside it. The dome: a plate, the disk of radius 20 about
//! the z axis from z = −2 to z = 0, and on it a hemispherical shell between 18 and 20 from the
//! origin, closing the cavity of the half ball of radius 18 above the plate. The post: the disk
//! of radius 3 on the cavity's floor extruded up until it meets the inside of the dome, the
//! sphere of radius 18, and joined to the dome.
//!
//! The dome is the plate (cadrs's cylinder primitive) united with the shell: its ball of radius 20
//! less its ball of radius 18, less a block below z = 0. The post is the disk sketched on
//! the floor's plane z = 0 and extruded up "up to face", the dome's inner sphere, so its top
//! takes the sphere's shape; the two are united.
use crate::Refusal;
use crate::bench::{ball, block, cut, cylinder, face_at, p3, round, sketch, union, v3, xy};
use crate::shared::feature::extrude_to;
use cadrs_kernel::{BodyId, DefaultKernel, ExtrudeEnd};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let z = v3(0.0, 0.0, 1.0);
    let origin = p3(0.0, 0.0, 0.0);
    let plate = cylinder(k, p3(0.0, 0.0, -2.0), z, 20.0, 2.0)?;
    let outer = ball(k, origin, 20.0)?;
    let inner = ball(k, origin, 18.0)?;
    let skin = cut(k, outer, inner)?;
    let below = block(k, p3(-21.0, -21.0, -21.0), p3(21.0, 21.0, 0.0))?;
    let shell = cut(k, skin, below)?;
    let dome = union(k, plate, shell)?;
    let inside = face_at(k, dome, p3(0.0, 0.0, 18.0))?;
    let disk = sketch(xy(0.0), vec![round((0.0, 0.0), 3.0)]);
    let end = ExtrudeEnd::UpToFace {
        body: dome,
        face: inside,
        offset: 0.0,
    };
    let post = extrude_to(k, &disk, z, end, &[])?;
    union(k, dome, post)
}

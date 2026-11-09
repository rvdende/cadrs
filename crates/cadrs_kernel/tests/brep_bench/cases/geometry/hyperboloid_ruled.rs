//! The solid bounded by two flat disks and the surface ruled between their rims, the rims turned
//! a quarter turn against each other: circles of radius 5 about the z axis in the planes z = −5
//! and z = 5, the point at angle θ on the lower joined by a straight line to the point at
//! θ + 90° on the upper.
//!
//! A loft of two sections, which cadrs skins with a ruled surface, joining the sections' pieces
//! end to end. The loft restarts a whole circle where it lines up with the section before, so
//! each rim is sketched as two half circles, and the joints between them say which points
//! correspond: on the plane z = −5 with its sketch x axis along x, the halves meet at angles 0°
//! and 180°; on the plane z = 5 with its sketch x axis along y, at 90° and 270°. Each ruling then
//! runs from angle θ below to θ + 90° above.
use crate::Refusal;
use crate::bench::{arc, chain, p2, p3, plane, sketch, v3};
use cadrs_kernel::{BodyId, BodyKind, DefaultKernel, Kernel, LoftEnd, LoftSection, LoftSpec};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // The circle of radius 5 about the sketch's origin, as two halves from its sketch x axis.
    let rim = || {
        let c = p2(0.0, 0.0);
        chain(vec![arc(c, 5.0, 0.0, 180.0), arc(c, 5.0, 180.0, 180.0)])
    };
    let z = v3(0.0, 0.0, 1.0);
    let lower = sketch(plane(p3(0.0, 0.0, -5.0), v3(1.0, 0.0, 0.0), z), vec![rim()]);
    let upper = sketch(plane(p3(0.0, 0.0, 5.0), v3(0.0, 1.0, 0.0), z), vec![rim()]);
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![LoftSection::Profile(lower), LoftSection::Profile(upper)],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    let r = k.loft_with(&spec)?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the loft made no body"))
}

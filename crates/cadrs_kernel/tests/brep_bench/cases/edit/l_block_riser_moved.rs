//! An L-block, the polygon through (x, z) = (0, 0), (40, 0), (40, 10), (15, 10), (15, 30),
//! (0, 30) extruded from y = 0 to y = 20; then its riser, the face x = 15 from z = 10 to 30,
//! moved out by 10 along +x to x = 25, the foot's top cut back and the upright's top run on to
//! meet it: the L whose section is the polygon through (0, 0), (40, 0), (40, 10), (25, 10),
//! (25, 30), (0, 30).
//!
//! The L is sketched on the Front plane and extruded along +y; then Move face translates the
//! riser by 10 along +x.
use crate::Refusal;
use crate::bench::{extrude, face_at, p3, polygon, sketch, v3, xz};
use crate::shared::edit::move_faces;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let l = polygon(&[
        (0.0, 0.0),
        (40.0, 0.0),
        (40.0, 10.0),
        (15.0, 10.0),
        (15.0, 30.0),
        (0.0, 30.0),
    ]);
    // the Front plane's normal is −y: extrude against it, along +y
    let block = extrude(k, &sketch(xz(0.0), vec![l]), -20.0)?;
    let riser = face_at(k, block, p3(15.0, 10.0, 20.0))?;
    move_faces(k, block, &[riser], v3(10.0, 0.0, 0.0))
}

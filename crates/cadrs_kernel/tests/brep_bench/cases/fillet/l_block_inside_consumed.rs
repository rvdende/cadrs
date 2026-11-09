//! An L-block, the polygon through (x, z) = (0, 0), (10, 0), (10, 3), (3, 3), (3, 10), (0, 10)
//! extruded from y = 0 to y = 10, its concave inside edge (x = 3, z = 3, along y) rounded to
//! radius 7, adding material: the round touches the faces at their far edges, consuming both,
//! and is a quarter of the cylinder of radius 7 on the line x = 10, z = 10.
//!
//! The L is sketched on the Front plane and extruded along +y; the inside edge is picked by a
//! point on it and filleted, radius 7.
use crate::Refusal;
use crate::bench::{edge_at, extrude, p3, polygon, sketch, xz};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let l = polygon(&[
        (0.0, 0.0),
        (10.0, 0.0),
        (10.0, 3.0),
        (3.0, 3.0),
        (3.0, 10.0),
        (0.0, 10.0),
    ]);
    // the Front plane's normal is −y: extrude against it, along +y
    let block = extrude(k, &sketch(xz(0.0), vec![l]), -10.0)?;
    let inside = edge_at(k, block, p3(3.0, 5.0, 3.0))?;
    fillet(k, block, &[inside], 7.0)
}

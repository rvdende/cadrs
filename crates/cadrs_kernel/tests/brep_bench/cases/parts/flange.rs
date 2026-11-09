//! A round mounting flange on the z axis: a plate of radius 32 from z = 0 to z = 6, a hub of
//! radius 15 on it up to z = 16, and a bore of radius 8 right through, turned as one section in
//! the xz plane revolved a whole turn about the z axis. Six bolt holes of radius 3 go through the
//! plate along z about (24, 0) and that point turned by 60°, 120°, … 300°, cut one after another:
//! the first a circle extruded from z = −1 to z = 7, past both faces, and the others it turned.
use crate::Refusal;
use crate::bench::{V3, axis, cut, cylinder, p3, polygon, revolve, sketch, turned, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // on the Front plane, sketch (x, y) is the model's (x, z)
    let section = polygon(&[
        (8.0, 0.0),
        (32.0, 0.0),
        (32.0, 6.0),
        (15.0, 6.0),
        (15.0, 16.0),
        (8.0, 16.0),
    ]);
    let z = axis(p3(0.0, 0.0, 0.0), V3::z());
    let mut flange = revolve(k, &sketch(xz(0.0), vec![section]), z, None)?;
    let first = cylinder(k, p3(24.0, 0.0, -1.0), V3::z(), 3.0, 8.0)?;
    for i in 0..6 {
        let hole = if i == 0 {
            first
        } else {
            turned(k, first, z, 60.0 * i as f64)?
        };
        flange = cut(k, flange, hole)?;
    }
    Ok(flange)
}

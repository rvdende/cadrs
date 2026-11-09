//! A curved block (the ring sector between radii 8 and 12, heights ±1, polar angles 0° to 60°)
//! whose floor is cut by the flat end of a leaning bar turned past it about the z axis from −140°
//! to +140°.
//!
//! The bar is its end (the rectangle 3 by 4 turned 5°, in the plane through (10, 0, 0) spanned by
//! u = (cos 20°, 0, −sin 20°) and v = (0, 1, 0)) extruded 3 along that plane's normal; what it
//! passes through (one turn) is cut from the block by cadrs's cut by a swept solid (the exact
//! sweep cut from it where it can be, else meshed).
use crate::Refusal;
use crate::bench::{extrude, p3, plane, polygon, sketch, v3};
use crate::shared::swept::{cut_swept, ring_sector, swing, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let block = ring_sector(k, (8.0, 12.0), (-1.0, 1.0), (0.0, 60.0))?;
    let t = 20f64.to_radians();
    let (u, n) = (v3(t.cos(), 0.0, -t.sin()), v3(t.sin(), 0.0, t.cos()));
    // (The sketch's y axis, n × u, is v.)
    let on = plane(p3(10.0, 0.0, 0.0), u, n);
    let f = 5f64.to_radians();
    let corner = |p: f64, q: f64| (p * f.cos() - q * f.sin(), p * f.sin() + q * f.cos());
    let end = polygon(&[
        corner(0.0, -2.0),
        corner(3.0, -2.0),
        corner(3.0, 2.0),
        corner(0.0, 2.0),
    ]);
    let bar = extrude(k, &sketch(on, vec![end]), 3.0)?;
    cut_swept(k, block, bar, &swing(z_axis(), -140.0, 140.0))
}

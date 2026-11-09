//! A lemon: the inner part of a spindle torus. In the xz plane, the part of the disk of radius 5
//! centered at (−3, 0, 0) with x ≥ 0, a circular segment cut off by the z axis as its chord, from
//! (0, 0, −4) to (0, 0, 4) and reaching out to (2, 0, 0), turned a whole turn about the z axis.
//!
//! On the Front (xz) plane, the segment: the arc of the circle of radius 5 about (−3, 0) from
//! (0, −4) through (2, 0) to (0, 4), closed by its chord on the z axis, revolved a whole turn
//! about z.
use crate::Refusal;
use crate::bench::{arc, axis, chain, line, p2, p3, revolve, sketch, v3, xz};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // The arc's ends (0, ±4) lie at angles ±arccos(3/5) about (−3, 0).
    let end = (3.0f64 / 5.0).acos().to_degrees();
    let segment = chain(vec![
        arc(p2(-3.0, 0.0), 5.0, -end, 2.0 * end),
        line(p2(0.0, 4.0), p2(0.0, -4.0)),
    ]);
    let s = sketch(xz(0.0), vec![segment]);
    revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)
}

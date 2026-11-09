//! A picture frame: the rabbeted molding, in (s, z) the polygon (0, 2), (3, 2), (3, 0), (8, 0),
//! (8, 6), (0, 6) with s the distance outward from the path, swept around the closed rectangle
//! with corners (±30, ±20, 0), mitered at its four corners.
//!
//! The section is drawn on the Right plane (x = 0), where it crosses the side y = −20: there
//! (s, z) is the point (0, −20 − s, z), the sketch's (y, z). cadrs's sweep mitres each corner
//! of its path, but swept once around the whole closed rectangle (a closed path starts where
//! the section crosses it) its pieces come out joined wrongly (a solid of 5535, not 9864), so
//! the frame is swept in two halves, each along the open path from the section's place around
//! two corners to the middle of the opposite side, (0, −20) to (±30, −20) to (±30, 20) to
//! (0, 20), drawn on the Top plane, and the two halves united where they meet.
use crate::Refusal;
use crate::bench::{line, p2, polygon, sketch, union, xy, yz};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let molding: Vec<(f64, f64)> = [
        (0.0, 2.0),
        (3.0, 2.0),
        (3.0, 0.0),
        (8.0, 0.0),
        (8.0, 6.0),
        (0.0, 6.0),
    ]
    .iter()
    .map(|&(s, z)| (-20.0 - s, z))
    .collect();
    let section = sketch(yz(0.0), vec![polygon(&molding)]);
    let mut halves = Vec::new();
    for side in [1.0, -1.0] {
        let c = [
            p2(0.0, -20.0),
            p2(30.0 * side, -20.0),
            p2(30.0 * side, 20.0),
            p2(0.0, 20.0),
        ];
        let path = (0..3).map(|i| on(xy(0.0), line(c[i], c[i + 1]))).collect();
        halves.push(sweep(k, &section, path)?);
    }
    union(k, halves[0], halves[1])
}

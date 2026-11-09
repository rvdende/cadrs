//! A plate (the box from (0, −5, 0) to (20, 5, 4)) pocketed from below by a bar of square section
//! stood on a corner (6 ≤ x ≤ 14, |y| + |z + 6| ≤ 1) pushed 8 along (0.3, 0, √0.91).
//!
//! The bar is its section sketched on the plane x = 6 and extruded 8; what it passes through is
//! cadrs's sweep of a solid (one slide: the bar where it starts, with what its leading faces
//! sweep), cut from the plate.
use crate::Refusal;
use crate::bench::{block, cut, extrude, p3, polygon, sketch, v3, yz};
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, -5.0, 0.0), p3(20.0, 5.0, 4.0))?;
    // On the plane x = 6, sketch (y, z).
    let section = polygon(&[(0.0, -7.0), (1.0, -6.0), (0.0, -5.0), (-1.0, -6.0)]);
    let bar = extrude(k, &sketch(yz(6.0), vec![section]), 8.0)?;
    let lean = v3(0.3, 0.0, (1.0f64 - 0.09).sqrt());
    let swept = slid(k, bar, lean * 8.0)?;
    cut(k, plate, swept)
}

//! Two balls of radius 5, one centered on the origin and one on (4.8, 0, 3.6), united. They meet
//! in a circle of radius 4 about the point halfway between the centers. Each ball's poles are on
//! the vertical through its center, so the circle passes through (0, 0, 5), a pole of the first,
//! and through (4.8, 0, −1.4), a pole of the second.
//!
//! Each ball is cadrs's sphere primitive, its poles on the vertical through its center; the two
//! are united.
use crate::Refusal;
use crate::bench::{ball, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let a = ball(k, p3(0.0, 0.0, 0.0), 5.0)?;
    let b = ball(k, p3(4.8, 0.0, 3.6), 5.0)?;
    union(k, a, b)
}

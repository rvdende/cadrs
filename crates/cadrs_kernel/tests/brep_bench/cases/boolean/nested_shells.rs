//! Five balls, each inside the one before: the first (radius 10 about the origin) less the second
//! (8 about (1, 0, 0)), joined with the third (6 about (1, 1, 0)) less the fourth (4 about
//! (1, 1, 1)), joined with the fifth (2 about (1, 1.5, 1.5)): three bodies, none touching, each in
//! the hollow of the one around it. Each ball is cadrs's sphere primitive.
use crate::Refusal;
use crate::bench::{ball, cut, p3, union};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let (b10, b8) = (
        ball(k, p3(0.0, 0.0, 0.0), 10.0)?,
        ball(k, p3(1.0, 0.0, 0.0), 8.0)?,
    );
    let (b6, b4) = (
        ball(k, p3(1.0, 1.0, 0.0), 6.0)?,
        ball(k, p3(1.0, 1.0, 1.0), 4.0)?,
    );
    let b2 = ball(k, p3(1.0, 1.5, 1.5), 2.0)?;
    let outer = cut(k, b10, b8)?;
    let middle = cut(k, b6, b4)?;
    let two = union(k, outer, middle)?;
    union(k, two, b2)
}

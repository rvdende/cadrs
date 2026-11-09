//! Twelve balls of radius 3, their centers every 30° on the circle of radius 10 about the z axis
//! (the first at (10, 0, 0)), joined one after another into a ring. Each ball is cadrs's sphere
//! primitive.
use crate::Refusal;
use crate::bench::{ball, p3, union_each};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut balls = Vec::new();
    for i in 0..12 {
        let a = (30.0 * i as f64).to_radians();
        balls.push(ball(k, p3(10.0 * a.cos(), 10.0 * a.sin(), 0.0), 3.0)?);
    }
    union_each(k, balls[0], &balls[1..])
}

//! The block from (−15, −15, 0) to (15, 15, 20), bored right through along the z axis by a hole
//! of radius 6. The hole's wall drafted 3°, pull +z, about the neutral plane z = 0: a cone through
//! the hole's circle at z = 0, widening toward the top.
//!
//! The rectangle extruded, the bore a circle extruded through it and cut; then cadrs's Draft of
//! the hole's wall. A hole that widens as it rises leans into the block's material, so the
//! angle is positive.
use crate::Refusal;
use crate::bench::{block, cut, cylinder, face_at, p3, v3};
use crate::shared::draft::drafted;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(-15.0, -15.0, 0.0), p3(15.0, 15.0, 20.0))?;
    let bore = cylinder(k, p3(0.0, 0.0, -1.0), v3(0.0, 0.0, 1.0), 6.0, 22.0)?;
    let bored = cut(k, b, bore)?;
    let wall = face_at(k, bored, p3(6.0, 0.0, 10.0))?;
    drafted(
        k,
        bored,
        vec![wall],
        3.0,
        v3(0.0, 0.0, 1.0),
        p3(0.0, 0.0, 0.0),
    )
}

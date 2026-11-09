//! A solid spindle torus centered on the origin, about the z axis: the circle through the middle
//! of its tube has radius 3 and the tube has radius 5, so the tube crosses the axis. The solid is
//! what the disk (x − 3)² + z² ≤ 25 fills as it turns, counted once: the turn of the disk's part
//! with x ≥ 0, bounded by the tube circle's outer arc, which meets the axis at (0, 0, ±4).
//!
//! cadrs's torus primitive (`Primitive::Torus`), its tube wider than its ring, the solid it fills counted once.
use crate::Refusal;
use crate::bench::{p3, torus, v3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    torus(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 3.0, 5.0)
}

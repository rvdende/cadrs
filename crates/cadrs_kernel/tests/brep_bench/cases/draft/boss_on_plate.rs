//! A square plate from (−20, −20, 0) to (20, 20, 5), and on it a cylindrical boss of radius 8
//! about the z axis, from z = 5 to z = 17, united with the plate. The boss's side drafted 4°,
//! pull +z, neutral plane z = 5: it becomes a cone narrowing as it rises, the top shrinking to
//! meet it.
//!
//! The plate is a rectangle extruded, the boss a circle sketched on its top and extruded,
//! united; then cadrs's Draft of the boss's side.
use crate::Refusal;
use crate::bench::{block, cylinder, face_at, p3, union, v3};
use crate::shared::draft::drafted;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(-20.0, -20.0, 0.0), p3(20.0, 20.0, 5.0))?;
    let boss = cylinder(k, p3(0.0, 0.0, 5.0), v3(0.0, 0.0, 1.0), 8.0, 12.0)?;
    let part = union(k, plate, boss)?;
    let side = face_at(k, part, p3(8.0, 0.0, 11.0))?;
    drafted(
        k,
        part,
        vec![side],
        4.0,
        v3(0.0, 0.0, 1.0),
        p3(0.0, 0.0, 5.0),
    )
}

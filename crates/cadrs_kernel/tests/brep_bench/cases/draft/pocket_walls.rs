//! The block from the origin to (40, 30, 15), with a rectangular pocket cut into its top: the box
//! from (8, 8, 5) to (32, 22, 15), its floor at z = 5. The pocket's four walls drafted 5°, pull
//! +z, about the neutral plane z = 5, each leaning away from the pocket's middle as it rises, so
//! the opening grows to meet them.
//!
//! The rectangle extruded; the pocket a rectangle sketched on the top face and extruded 10 down,
//! cut; then cadrs's Draft of the pocket's walls (leaning into the material: a positive angle).
use crate::Refusal;
use crate::bench::{block, cut, extrude, faces_at, p3, rectangle, sketch, v3, xy};
use crate::shared::draft::drafted;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 15.0))?;
    let pocket = extrude(
        k,
        &sketch(xy(15.0), vec![rectangle(8.0, 8.0, 32.0, 22.0)]),
        -10.0,
    )?;
    let part = cut(k, b, pocket)?;
    let walls = faces_at(
        k,
        part,
        &[
            p3(8.0, 15.0, 10.0),
            p3(32.0, 15.0, 10.0),
            p3(20.0, 8.0, 10.0),
            p3(20.0, 22.0, 10.0),
        ],
    )?;
    drafted(k, part, walls, 5.0, v3(0.0, 0.0, 1.0), p3(0.0, 0.0, 5.0))
}

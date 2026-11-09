//! The box from the origin to (20, 12, 10), its four sides drafted 5° the other way (a negative
//! draft), pull +z, about the neutral plane z = 0: each leans out as it rises, the top growing to
//! meet them. An upside-down frustum of a rectangular pyramid.
//!
//! The rectangle extruded, then cadrs's Draft of its four sides with a negative angle.
use crate::Refusal;
use crate::bench::{block, faces_at, p3, v3};
use crate::shared::draft::drafted;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 12.0, 10.0))?;
    let sides = faces_at(
        k,
        b,
        &[
            p3(0.0, 6.0, 5.0),
            p3(20.0, 6.0, 5.0),
            p3(10.0, 0.0, 5.0),
            p3(10.0, 12.0, 5.0),
        ],
    )?;
    drafted(k, b, sides, -5.0, v3(0.0, 0.0, 1.0), p3(0.0, 0.0, 0.0))
}

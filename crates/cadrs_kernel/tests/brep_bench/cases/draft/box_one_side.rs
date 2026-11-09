//! The box from the origin to (20, 12, 10), its side x = 20 drafted 10°, pull +z, about the
//! neutral plane z = 0: it turns about its bottom edge, leaning toward −x as it rises. The top
//! and the sides y = 0 and y = 12 are trimmed to it: a prism along y on a trapezoid.
//!
//! The rectangle extruded, then cadrs's Draft of the one side.
use crate::Refusal;
use crate::bench::{block, face_at, p3, v3};
use crate::shared::draft::drafted;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 12.0, 10.0))?;
    let side = face_at(k, b, p3(20.0, 6.0, 5.0))?;
    drafted(k, b, vec![side], 10.0, v3(0.0, 0.0, 1.0), p3(0.0, 0.0, 0.0))
}

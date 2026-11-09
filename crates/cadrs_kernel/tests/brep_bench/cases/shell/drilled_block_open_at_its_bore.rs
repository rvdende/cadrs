//! A drilled block shelled open at its bore. The block from the origin to (20, 20, 10) with a hole of
//! radius 3 bored through it along z about x = 10, y = 10, shelled inward to a wall 1 thick, open at
//! the bore: the six flat faces moved 1 inward make the cavity from (1, 1, 1) to (19, 19, 9), which
//! runs out to the bore's cylinder.
//!
//! The drilled block is the square with the hole's circle in it on the Top plane, extruded 10; cadrs's
//! Shell feature (inward), the bore picked as the face to remove.
use crate::Refusal;
use crate::bench::{extrude, p3, rectangle, round, sketch_with_holes, xy};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let square = rectangle(0.0, 0.0, 20.0, 20.0);
    let s = sketch_with_holes(xy(0.0), square, vec![round((10.0, 10.0), 3.0)]);
    let b = extrude(k, &s, 10.0)?;
    shell_inward(k, b, &[p3(13.0, 10.0, 5.0)], 1.0, Joins::Rounded)
}

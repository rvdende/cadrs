//! A drilled block shelled with its top open: the bore becomes a tube standing in the cup. The block
//! from the origin to (20, 20, 10) with a hole of radius 3 bored through it along z about x = 10,
//! y = 10, shelled inward to a wall 1 thick, open at its top: the bore moved 1 inward (into the
//! material) is the cylinder of radius 4, so a tube stands in the cup.
//!
//! The drilled block is the square with the hole's circle in it on the Top plane, extruded 10; cadrs's
//! Shell feature (inward), the top picked as the face to remove.
use crate::Refusal;
use crate::bench::{extrude, p3, rectangle, round, sketch_with_holes, xy};
use crate::shared::shell::{Joins, shell_inward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let square = rectangle(0.0, 0.0, 20.0, 20.0);
    let s = sketch_with_holes(xy(0.0), square, vec![round((10.0, 10.0), 3.0)]);
    let b = extrude(k, &s, 10.0)?;
    shell_inward(k, b, &[p3(2.0, 2.0, 10.0)], 1.0, Joins::Rounded)
}

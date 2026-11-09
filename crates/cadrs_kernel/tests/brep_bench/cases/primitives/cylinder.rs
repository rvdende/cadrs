//! A solid cylinder of radius 5 and height 10 whose axis is the z axis, standing on the xy plane:
//! a circle on the Top plane, extruded 10 up.
use crate::Refusal;
use crate::bench::{extrude, round, sketch, xy};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch(xy(0.0), vec![round((0.0, 0.0), 5.0)]);
    extrude(k, &s, 10.0)
}

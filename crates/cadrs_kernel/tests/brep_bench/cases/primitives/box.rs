//! A rectangular block 10 long in x, 20 in y and 30 in z, with one corner at the origin and the
//! opposite corner at (10, 20, 30): the rectangle on the Top plane, extruded 30 up.
use crate::Refusal;
use crate::bench::{extrude, rectangle, sketch, xy};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = sketch(xy(0.0), vec![rectangle(0.0, 0.0, 10.0, 20.0)]);
    extrude(k, &s, 30.0)
}

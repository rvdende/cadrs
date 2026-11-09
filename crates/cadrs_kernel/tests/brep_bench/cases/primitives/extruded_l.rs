//! An L-shaped plate: the polygon through (0, 0), (20, 0), (20, 5), (5, 5), (5, 15) and (0, 15)
//! sketched on the Top plane, extruded 8 along +z.
use crate::Refusal;
use crate::bench::{extrude, polygon, sketch, xy};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let l = polygon(&[
        (0.0, 0.0),
        (20.0, 0.0),
        (20.0, 5.0),
        (5.0, 5.0),
        (5.0, 15.0),
        (0.0, 15.0),
    ]);
    extrude(k, &sketch(xy(0.0), vec![l]), 8.0)
}

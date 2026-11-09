//! The drum of `geometry/saddle-drum` (a circle of radius 8 extruded, its top replaced by the
//! bilinear saddle z = 10 + xy/8), with everything above the plane z = 10 cut away by a block on
//! that plane reaching past the drum: the plane is tangent to the saddle at its saddle point.
use crate::Refusal;
use crate::bench::{V3, p3};
use crate::shared::geometry::saddle_drum;
use crate::shared::singular::cut_beyond;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let drum = saddle_drum(k)?;
    cut_beyond(k, drum, p3(0.0, 0.0, 10.0), V3::z())
}

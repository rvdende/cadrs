//! A sheet: the side of the cone frustum about the z axis whose radius runs from 6 at z = 0 to 3
//! at z = 4, one face bounded by those two circles, thickened by 1 along its normal away from the
//! axis.
//!
//! The slant line from (6, 0) to (3, 4) sketched on the Front plane (its (x, z)) and
//! surface-revolved a whole turn about z; cadrs's Thicken thickens it with its arrow flipped to
//! point away from the axis.
use crate::Refusal;
use crate::bench::{axis, p3, v3, xz};
use crate::shared::thicken::{open_chain, polyline, surface_revolve, thicken_toward};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let s = open_chain(xz(0.0), polyline(&[(6.0, 0.0), (3.0, 4.0)]));
    let sheet = surface_revolve(k, &s, axis(p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0)), None)?;
    // the point of the sheet at radius 4.5, z = 2, on the +y side
    thicken_toward(k, sheet, p3(0.0, 4.5, 2.0), v3(0.0, 1.0, 0.0), 1.0)
}

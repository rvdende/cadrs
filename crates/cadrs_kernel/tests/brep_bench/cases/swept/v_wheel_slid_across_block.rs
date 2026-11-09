//! A block (the box from (0, −5, 0) to (20, 5, 10)) grooved across its top by a wheel with a sharp
//! V rim (the triangle (0, 8), (−3 tan 30°, 11), (3 tan 30°, 11) of the plane x = −8, in (y, z),
//! turned about the line through (−8, 0, 14) along y) slid 36 along x.
//!
//! The wheel is its triangle turned about its axis; what it passes through is cadrs's sweep of a
//! solid (one slide), cut from the block.
use crate::Refusal;
use crate::bench::{block, cut, p3, v3, yz};
use crate::shared::geometry::Section;
use crate::shared::swept::slid;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, -5.0, 0.0), p3(20.0, 5.0, 10.0))?;
    let t = 3.0 * 30f64.to_radians().tan();
    let section = Section::on(yz(-8.0))
        .line((0.0, 8.0), (t, 11.0))
        .line((t, 11.0), (-t, 11.0))
        .line((-t, 11.0), (0.0, 8.0));
    let wheel = section.turned(k, p3(-8.0, 0.0, 14.0), v3(0.0, 1.0, 0.0))?;
    let swept = slid(k, wheel, v3(36.0, 0.0, 0.0))?;
    cut(k, b, swept)
}

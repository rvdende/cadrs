//! A hub with a stop, and a rib turned around the hub until it meets the stop. The hub: a
//! cylinder of radius 10 on the z axis from z = 0 to z = 10. The stop: everything within 25 of
//! the z axis between the half-planes at 120° and 135°, from z = 0 to z = 10, joined to the hub.
//! The rib: the rectangle 5 ≤ x ≤ 20, 0 ≤ z ≤ 4 in the plane y = 0, turned about the z axis from
//! 0° toward +y until it meets the stop's face at 120°, and joined to them.
//!
//! The hub is a circle extruded and the stop a sector (two lines and an arc) extruded, the two
//! united. The rib is the rectangle sketched on the Front plane and revolved about the z axis
//! "up to face", the stop's face at 120° (a plane through the axis: the revolve ends at its
//! angle); it is united with them.
use crate::Refusal;
use crate::bench::{
    arc, axis, chain, cylinder, extrude, face_at, line, p2, p3, rectangle, sketch, union, v3, xy,
    xz,
};
use crate::shared::feature::revolve_to;
use cadrs_kernel::{BodyId, DefaultKernel, RevolveEnd};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let z = v3(0.0, 0.0, 1.0);
    let hub = cylinder(k, p3(0.0, 0.0, 0.0), z, 10.0, 10.0)?;
    let (a, b) = (120f64.to_radians(), 135f64.to_radians());
    let sector = chain(vec![
        line(p2(0.0, 0.0), p2(25.0 * a.cos(), 25.0 * a.sin())),
        arc(p2(0.0, 0.0), 25.0, 120.0, 15.0),
        line(p2(25.0 * b.cos(), 25.0 * b.sin()), p2(0.0, 0.0)),
    ]);
    let stop = extrude(k, &sketch(xy(0.0), vec![sector]), 10.0)?;
    let base = union(k, hub, stop)?;
    let face = face_at(k, base, p3(17.0 * a.cos(), 17.0 * a.sin(), 5.0))?;
    // the Front plane's sketch (x, y) is the model's (x, z)
    let rect = sketch(xz(0.0), vec![rectangle(5.0, 0.0, 20.0, 4.0)]);
    let end = RevolveEnd::UpToFace {
        body: base,
        face,
        offset: 0.0,
    };
    let rib = revolve_to(k, &rect, axis(p3(0.0, 0.0, 0.0), z), end, &[])?;
    union(k, base, rib)
}

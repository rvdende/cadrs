//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by a bar whose
//! section (in the plane y = 0) is the closed uniform cubic B-spline on (3.5, 0), (4.5, −1.5),
//! (8, −1.5), (12, −1), (12, 1), (8, 1.5), (4.5, 1.5) in (x, z), carried from y = −8 to 8, screwed
//! about the z axis, rising 6 a turn, from a turn of −90° to 1350°.
//!
//! The bar is the periodic spline extruded (cadrs's Extrude); what it passes through (a turn and a
//! slide along its axis at once: a screw) is cut from the rod by cadrs's cut by a swept solid
//! (exactly half a turn at a time where it can be, else meshed); of the
//! bar, its part within 5.5 of the axis (the screw maps the cylinder about its axis onto itself, so
//! only that part reaches the rod).
use crate::Refusal;
use crate::bench::{extrude, p3, plane, v3};
use crate::shared::geometry::Section;
use crate::shared::swept::{cut_swept, near_axis, rod, screw, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // The plane y = 8, sketch x along +x and sketch y along +z (normal −y, the extrusion's way).
    let on = plane(p3(0.0, 8.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, -1.0, 0.0));
    // (The same loop started from (12, −1): its seam, where the periodic spline's first span
    // starts, then lies at its far end, out of the part near the axis.)
    let poles = [
        (12.0, -1.0),
        (12.0, 1.0),
        (8.0, 1.5),
        (4.5, 1.5),
        (3.5, 0.0),
        (4.5, -1.5),
        (8.0, -1.5),
    ];
    let section = Section::on(on).periodic(3, &poles);
    let bar = extrude(k, &section.sketch(), 16.0)?;
    let bar = near_axis(k, bar, 5.5)?;
    let r = rod(k)?;
    cut_swept(k, r, bar, &screw(z_axis(), 6.0, -90.0, 1350.0))
}

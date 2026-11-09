//! A sheet: the parabolic cylinder z = x²/8 for −4 ≤ x ≤ 4 and 0 ≤ y ≤ 10, a B-spline surface of
//! degree 2 in u and 1 in v with control points (−4, y, 2), (0, y, −2) and (4, y, 2) for y = 0 and
//! y = 10, thickened by 1 to its concave side, toward the parabola's focus.
//!
//! The parabola is the quadratic B-spline curve (one Bézier span) on those three control points at
//! y = 0, made a wire; the wire is extruded 10 along +y into the sheet (cadrs's extrude of a curve,
//! a sweep of the wire one kind higher). cadrs's Thicken thickens it with its arrow flipped to
//! point up, toward the focus.
use crate::Refusal;
use crate::bench::{p3, v3};
use crate::shared::thicken::thicken_toward;
use cadrs_kernel::{BodyId, BodySweep, Bspline3, DefaultKernel, Kernel, PathCurve};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let parabola = Bspline3 {
        degree: 2,
        knots: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        poles: vec![p3(-4.0, 0.0, 2.0), p3(0.0, 0.0, -2.0), p3(4.0, 0.0, 2.0)],
        weights: vec![],
    };
    let wire = k.create_wire(&[PathCurve::Bspline(parabola)])?.bodies[0];
    let sheet = k
        .sweep_body(wire, &BodySweep::Translate(v3(0.0, 10.0, 0.0)))?
        .bodies[0];
    // the parabola's vertex, (0, y, 0); its focus is above it
    thicken_toward(k, sheet, p3(0.0, 5.0, 0.0), v3(0.0, 0.0, 1.0), 1.0)
}

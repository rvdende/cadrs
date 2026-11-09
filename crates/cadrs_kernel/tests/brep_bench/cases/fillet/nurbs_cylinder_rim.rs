//! The cylinder of radius 5 on the z axis from z = 0 to z = 10, its side one rational B-spline
//! surface (the rational quadratic circle of nine control points times a line), its top rim
//! rounded to radius 1.
//!
//! cadrs makes no face from a bare surface, so the cylinder is made and its side converted:
//! a circle extruded, healed with `HealOp::ToBSpline` (every curved surface its exact B-spline:
//! the side the standard nine-point circle's net, its knots in radians, its seam on the half
//! plane y = 0, x ≥ 0). Its top rim is picked by a point on it and filleted.
use crate::Refusal;
use crate::bench::{cylinder, edge_at, p3, v3};
use crate::shared::fillet::fillet;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, DefaultKernel, HealOp, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let c = cylinder(k, p3(0.0, 0.0, 0.0), v3(0.0, 0.0, 1.0), 5.0, 10.0)?;
    let spline = first(Kernel::heal(k, c, &[HealOp::ToBSpline])?)?;
    let rim = edge_at(k, spline, p3(0.0, 5.0, 10.0))?;
    fillet(k, spline, &[rim], 1.0)
}

//! What the cadrs implementations of the fill cases share: Fill (Onshape's Fill) through sketch
//! curves, each on its own plane, and the flat walls under a patch's boundary curves.
//!
//! cadrs's Fill fills a planar loop with its plane and four curves with their bilinearly blended
//! Coons patch (sampled and interpolated by a bicubic B-spline, exact for boundaries of degree two
//! or less, whose patch is of degree two or less each way).
use crate::Refusal;
use crate::bench::{line, p2};
use crate::shared::geometry::bezier_knots;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, Curve2, DefaultKernel, FillCurve, FillSpec, Kernel, Plane};
use nalgebra::Point2;

/// The sheet Fill makes of the closed chain `curves` (each a curve sketched on its plane).
pub fn fill(k: &mut DefaultKernel, curves: &[(Plane, Curve2)]) -> Result<BodyId, Refusal> {
    let curves = curves
        .iter()
        .map(|(plane, curve)| FillCurve::Sketch {
            plane: *plane,
            curve: curve.clone(),
        })
        .collect();
    first(k.fill(&FillSpec { curves, source: 1 })?)
}

/// The sketch segment from `a` to `b`.
pub fn seg(a: (f64, f64), b: (f64, f64)) -> Curve2 {
    line(p2(a.0, a.1), p2(b.0, b.1))
}

/// The polynomial Bézier curve with these control points: a sketch spline of one span.
pub fn bezier(poles: &[(f64, f64)]) -> Curve2 {
    let degree = poles.len() - 1;
    Curve2::Spline {
        degree,
        knots: bezier_knots(degree),
        poles: poles.iter().map(|&(x, y)| Point2::new(x, y)).collect(),
        weights: vec![],
        source: None,
    }
}

/// The flat wall on `plane` under its curve `top` (from `a` to `b`, each a sketch point (s, h)
/// with h its height): down from `b` to the floor h = 0, along it back under `a`, and up to `a`.
pub fn wall(
    k: &mut DefaultKernel,
    plane: Plane,
    top: Curve2,
    a: (f64, f64),
    b: (f64, f64),
) -> Result<BodyId, Refusal> {
    fill(
        k,
        &[
            (plane, top),
            (plane, seg(b, (b.0, 0.0))),
            (plane, seg((b.0, 0.0), (a.0, 0.0))),
            (plane, seg((a.0, 0.0), a)),
        ],
    )
}

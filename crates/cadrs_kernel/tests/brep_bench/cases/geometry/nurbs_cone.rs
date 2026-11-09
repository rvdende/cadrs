//! A solid cone standing on the disk of radius 5 about the z axis in the plane z = 0, its apex at
//! (0, 0, 10), whose side is one rational B-spline surface with a pole at the apex: rational
//! quadratic around the axis and linear up it, a 9 × 2 net whose first row is the base circle as
//! a rational quadratic (the nine points 5cᵢ, weights 1, √2/2, …, knots
//! (0, 0, 0, ¼, ¼, ½, ½, ¾, ¾, 1, 1, 1)) and whose second row is nine copies of the apex; closed
//! by a flat disk at its base.
//!
//! The side is cadrs's face on a B-spline surface given by its net (`Kernel::bspline_face`), which
//! bounds it by its base circle, its seam and its pole at the apex; the base is the Fill of that
//! circle (`Kernel::fill`), a flat disk; the two are sewn into a solid (`Kernel::sew_solid`).
use crate::Refusal;
use crate::bench::{edge_at, p3};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, BsplineSurface, DefaultKernel, FillCurve, FillSpec, Kernel};
use nalgebra::Point3;

/// The control points cᵢ of the unit circle as a rational quadratic, and their weights.
fn circle() -> ([(f64, f64); 9], [f64; 9]) {
    let s = std::f64::consts::FRAC_1_SQRT_2;
    (
        [
            (1.0, 0.0),
            (1.0, 1.0),
            (0.0, 1.0),
            (-1.0, 1.0),
            (-1.0, 0.0),
            (-1.0, -1.0),
            (0.0, -1.0),
            (1.0, -1.0),
            (1.0, 0.0),
        ],
        [1.0, s, 1.0, s, 1.0, s, 1.0, s, 1.0],
    )
}

/// The knots of [`circle`].
const AROUND: [f64; 12] = [
    0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0,
];

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let (c, w) = circle();
    let surface = BsplineSurface {
        degree_u: 2,
        degree_v: 1,
        knots_u: AROUND.to_vec(),
        knots_v: vec![0.0, 0.0, 1.0, 1.0],
        poles: c
            .iter()
            .map(|&(x, y)| {
                vec![
                    Point3::new(5.0 * x, 5.0 * y, 0.0),
                    Point3::new(0.0, 0.0, 10.0),
                ]
            })
            .collect(),
        weights: w.iter().map(|&wi| vec![wi, wi]).collect(),
    };
    let side = first(k.bspline_face(&surface, None)?)?;
    let rim = edge_at(k, side, p3(-5.0, 0.0, 0.0))?;
    let base = first(k.fill(&FillSpec {
        curves: vec![FillCurve::Edge {
            body: side,
            edge: rim,
        }],
        source: 1,
    })?)?;
    first(k.sew_solid(&[side, base], 1e-7)?)
}

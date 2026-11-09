//! A ball of radius 5 centered on the origin, its whole surface one rational B-spline surface,
//! the standard biquadratic sphere: a 9 × 5 net, the tensor product of the unit circle's nine
//! control points around the axis (weights 1, √2/2, …) and the semicircle's five from the south
//! pole to the north, whose first and last rows each collapse to a pole and whose edges u = 0
//! and u = 1 are the same meridian, its seam.
//!
//! The face is cadrs's face on a B-spline surface given by its net (`Kernel::bspline_face`), which
//! bounds it by its seam and its poles; the closed sheet is sewn into a solid
//! (`Kernel::sew_solid`).
use crate::Refusal;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, BsplineSurface, DefaultKernel, Kernel};
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
    let s = std::f64::consts::FRAC_1_SQRT_2;
    let (c, w) = circle();
    // The semicircle from the south pole to the north: (ρⱼ, zⱼ) and their weights.
    let m = [(0.0, -5.0), (5.0, -5.0), (5.0, 0.0), (5.0, 5.0), (0.0, 5.0)];
    let v = [1.0, s, 1.0, s, 1.0];
    let surface = BsplineSurface {
        degree_u: 2,
        degree_v: 2,
        knots_u: AROUND.to_vec(),
        knots_v: vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0],
        poles: c
            .iter()
            .map(|&(x, y)| {
                m.iter()
                    .map(|&(r, z)| Point3::new(r * x, r * y, z))
                    .collect()
            })
            .collect(),
        weights: w
            .iter()
            .map(|&wi| v.iter().map(|&vj| wi * vj).collect())
            .collect(),
    };
    let sheet = first(k.bspline_face(&surface, None)?)?;
    first(k.sew_solid(&[sheet], 1e-7)?)
}

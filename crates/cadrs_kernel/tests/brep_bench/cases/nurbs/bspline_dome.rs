//! A block standing on the square 0 ≤ x, y ≤ 20, its floor flat at z = 0, its four sides
//! upright, and its top a clamped non-rational bicubic B-spline surface of 6 × 5 control points,
//! on the knots (0, 0, 0, 0, 2/5, 2/5, 1, 1, 1, 1) along u (a double knot) and
//! (0, 0, 0, 0, 1/3, 1, 1, 1, 1) along v, the control point Pᵢⱼ at (xᵢ, yⱼ, zᵢⱼ) with
//! x = 0, 3, 7, 11, 16, 20 and y = 0, 5, 10, 15, 20.
//!
//! The surface is the kernel's `NurbsSurface` from those knots and points. The block is the
//! square extruded 10, and its flat top replaced by the surface (Replace face), which trims the
//! four sides to the surface's edges.
use crate::Refusal;
use crate::shared::nurbs::{domed_block, spline_surface};
use cadrs_kernel::{BodyId, DefaultKernel};

const X: [f64; 6] = [0.0, 3.0, 7.0, 11.0, 16.0, 20.0];
const Y: [f64; 5] = [0.0, 5.0, 10.0, 15.0, 20.0];
const Z: [[f64; 5]; 6] = [
    [3.0, 5.0, 6.0, 5.0, 4.0],
    [5.0, 9.0, 11.0, 8.0, 5.0],
    [6.0, 12.0, 15.0, 10.0, 6.0],
    [4.0, 10.0, 18.0, 9.0, 5.0],
    [5.0, 8.0, 9.0, 7.0, 4.0],
    [2.0, 4.0, 5.0, 4.0, 3.0],
];

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let points: Vec<Vec<[f64; 3]>> = (0..6)
        .map(|i| (0..5).map(|j| [X[i], Y[j], Z[i][j]]).collect())
        .collect();
    let ku = vec![0.0, 0.0, 0.0, 0.0, 0.4, 0.4, 1.0, 1.0, 1.0, 1.0];
    let kv = vec![0.0, 0.0, 0.0, 0.0, 1.0 / 3.0, 1.0, 1.0, 1.0, 1.0];
    let top = spline_surface(3, 3, ku, kv, &points, None)?;
    domed_block(k, (0.0, 0.0), (20.0, 20.0), 10.0, top)
}

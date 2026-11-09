//! A block standing on the square 0 ≤ x, y ≤ 20, its floor flat at z = 0, its four sides upright,
//! and its top a non-rational Bézier patch of degree 15 each way: the control point Pᵢⱼ is
//! (20i/15, 20j/15, zᵢⱼ), zᵢⱼ = 4 + ((3i + 5j) mod 7), for i, j = 0 … 15. The patch is the graph
//! of a height over the square, at least 4 above the floor, and its four edges are the sides' top
//! edges.
//!
//! A rectangle on the Top plane extruded 12 (above the patch's highest control point, 10), its top
//! face then replaced by the patch, made from its control net (cadrs's `NurbsSurface`): the sides
//! are trimmed where the patch meets them, along its edges.
use crate::Refusal;
use crate::bench::{block, p3};
use crate::shared::geometry::{bezier_knots, pole, replace_face};
use cadrs_kernel::native::{NurbsSurface, Surface};
use cadrs_kernel::{BodyId, DefaultKernel};
use std::sync::Arc;

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let net = (0..16)
        .map(|i| {
            (0..16)
                .map(|j| {
                    let z = 4.0 + ((3 * i + 5 * j) % 7) as f64;
                    pole(20.0 * i as f64 / 15.0, 20.0 * j as f64 / 15.0, z, 1.0)
                })
                .collect()
        })
        .collect();
    let patch = NurbsSurface::new(15, 15, bezier_knots(15), bezier_knots(15), net)?;
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 20.0, 12.0))?;
    replace_face(k, b, p3(10.0, 10.0, 12.0), Surface::Nurbs(Arc::new(patch)))
}

//! What the cadrs implementations of the nurbs cases share: the cases' B-spline surfaces, made
//! with the kernel's own `NurbsSurface` from the control points, knots and weights the cases
//! give, and a block whose top is one of them.
//!
//! cadrs builds a part from sketches, so a block with a free-form top is a block extruded from
//! its rectangle whose flat top is then given the surface with Replace face
//! (`Kernel::replace_faces`, Onshape's Replace face): the top takes the new surface over its
//! whole parameter range, and the four sides are trimmed (or extended) to meet it.
use crate::Refusal;
use crate::bench::{block, face_at, p3};
use cadrs_kernel::native::{NurbsSurface, Surface};
use cadrs_kernel::{BodyId, DefaultKernel, FaceReplacement, Kernel};
use nalgebra::Vector4;
use std::sync::Arc;

/// A clamped B-spline surface of degrees `du` × `dv` on the knots `ku` × `kv` through the
/// control points `points[i][j]` (i along u) with weights `weights[i][j]` (1 throughout for a
/// polynomial surface).
pub fn spline_surface(
    du: usize,
    dv: usize,
    ku: Vec<f64>,
    kv: Vec<f64>,
    points: &[Vec<[f64; 3]>],
    weights: Option<&[Vec<f64>]>,
) -> Result<Surface, Refusal> {
    let cps = points
        .iter()
        .enumerate()
        .map(|(i, row)| {
            row.iter()
                .enumerate()
                .map(|(j, p)| {
                    let w = weights.map_or(1.0, |w| w[i][j]);
                    Vector4::new(p[0] * w, p[1] * w, p[2] * w, w)
                })
                .collect()
        })
        .collect();
    Ok(Surface::Nurbs(Arc::new(NurbsSurface::new(
        du, dv, ku, kv, cps,
    )?)))
}

/// The bicubic Bézier patch of `nurbs/bezier-dome` (and the drilled and sliced domes): its
/// control points stand over the even 4 × 4 grid of the square 0 ≤ x, y ≤ 20.
pub fn bezier_dome_surface() -> Result<Surface, Refusal> {
    const HEIGHTS: [[f64; 4]; 4] = [
        [4.0, 6.0, 7.0, 5.0],
        [6.0, 14.0, 12.0, 7.0],
        [5.0, 13.0, 16.0, 8.0],
        [3.0, 6.0, 8.0, 6.0],
    ];
    let points: Vec<Vec<[f64; 3]>> = (0..4)
        .map(|i| {
            (0..4)
                .map(|j| [20.0 * i as f64 / 3.0, 20.0 * j as f64 / 3.0, HEIGHTS[i][j]])
                .collect()
        })
        .collect();
    let bezier = vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0];
    spline_surface(3, 3, bezier.clone(), bezier, &points, None)
}

/// The rational barrel vault of `nurbs/groin-vault` and `nurbs/vault-tangent-plane`: degree 1
/// along x (x = −12 to 12) and 2 along y, each row the rational quadratic arc through
/// (x, −12, 10), (x, 0, 26) and (x, 12, 10) with weights 1, 3/5 and 1.
pub fn barrel_vault_surface() -> Result<Surface, Refusal> {
    let points: Vec<Vec<[f64; 3]>> = [-12.0, 12.0]
        .iter()
        .map(|&x| vec![[x, -12.0, 10.0], [x, 0.0, 26.0], [x, 12.0, 10.0]])
        .collect();
    let weights = vec![vec![1.0, 0.6, 1.0]; 2];
    spline_surface(
        1,
        2,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &points,
        Some(&weights),
    )
}

/// The block over the rectangle [x0, x1] × [y0, y1] from the floor z = 0, its top the surface
/// `top` over its parameter range [0, 1] × [0, 1]: the rectangle extruded `height` (a height the
/// surface's edges cross the sides near), then its top face replaced by `top`.
pub fn domed_block(
    k: &mut DefaultKernel,
    (x0, y0): (f64, f64),
    (x1, y1): (f64, f64),
    height: f64,
    top: Surface,
) -> Result<BodyId, Refusal> {
    let b = block(k, p3(x0, y0, 0.0), p3(x1, y1, height))?;
    let face = face_at(k, b, p3((x0 + x1) / 2.0, (y0 + y1) / 2.0, height))?;
    let r = k.replace_faces(
        b,
        &[FaceReplacement {
            face,
            surface: top,
            domain: Some(((0.0, 1.0), (0.0, 1.0))),
        }],
    )?;
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("Replace face made no body"))
}

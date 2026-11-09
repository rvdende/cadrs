//! Two slabs touching along the plane z = 4, united: the box from (0, 0, 0) to (10, 10, 4), its
//! top a bicubic Bézier patch lying flat in that plane (control points on x = 0, 2, 7, 10 and
//! y = 0, 4, 5, 10), and the box from (5, 3, 4) to (15, 13, 7), its bottom a biquadratic Bézier
//! patch lying flat in it (x = 5, 12, 15 and y = 3, 4, 13).
//!
//! Each slab is a block whose flat face is given its patch with Replace face (the patch's
//! parameters 0 to 1 each way); the two united.
use crate::Refusal;
use crate::bench::{block, face_at, p3, union};
use crate::shared::geometry::bezier_knots;
use crate::shared::nurbs::spline_surface;
use crate::shared::sweep::first;
use cadrs_kernel::native::Surface;
use cadrs_kernel::{BodyId, DefaultKernel, FaceReplacement, Kernel};

/// The flat Bézier patch in the plane z = 4 whose control points stand over the grid `xs` × `ys`.
fn flat_patch(xs: &[f64], ys: &[f64]) -> Result<Surface, Refusal> {
    let points: Vec<Vec<[f64; 3]>> = xs
        .iter()
        .map(|&x| ys.iter().map(|&y| [x, y, 4.0]).collect())
        .collect();
    let (du, dv) = (xs.len() - 1, ys.len() - 1);
    spline_surface(du, dv, bezier_knots(du), bezier_knots(dv), &points, None)
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let lower = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 4.0))?;
    let top = face_at(k, lower, p3(5.0, 5.0, 4.0))?;
    let lower = first(k.replace_faces(
        lower,
        &[FaceReplacement {
            face: top,
            surface: flat_patch(&[0.0, 2.0, 7.0, 10.0], &[0.0, 4.0, 5.0, 10.0])?,
            domain: Some(((0.0, 1.0), (0.0, 1.0))),
        }],
    )?)?;
    let upper = block(k, p3(5.0, 3.0, 4.0), p3(15.0, 13.0, 7.0))?;
    let bottom = face_at(k, upper, p3(10.0, 8.0, 4.0))?;
    let upper = first(k.replace_faces(
        upper,
        &[FaceReplacement {
            face: bottom,
            surface: flat_patch(&[5.0, 12.0, 15.0], &[3.0, 4.0, 13.0])?,
            domain: Some(((0.0, 1.0), (0.0, 1.0))),
        }],
    )?)?;
    union(k, lower, upper)
}

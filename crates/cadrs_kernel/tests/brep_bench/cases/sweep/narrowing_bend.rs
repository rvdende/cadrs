//! A hollow square duct narrowing as it bends a quarter turn about the z axis along the arc of
//! radius 30 from (30, 0, 0) to (0, 30, 0): its section's outer half-width runs linearly with the
//! angle from 9 to 6, the hole's from 7 to 4, the wall 2 thick.
//!
//! The outside is a square of half-width 9 swept along the arc scaled down to 6 at its end
//! (cadrs's `Kernel::sweep_with_law`: the profile scaled about the path's point in proportion to
//! the length travelled, here to the angle); the hole, a square of half-width 7 scaled to 4, is
//! swept the same way along the arc run on a degree past each end (its law run on with it), so
//! it cuts through the outside's flat ends.
use crate::Refusal;
use crate::bench::{arc, cut, p2, p3, plane, rectangle, sketch, v3, xy};
use crate::shared::sweep::{on, sweep_law};
use cadrs_kernel::{BodyId, DefaultKernel};

/// The square of half-width `h` about the arc's point at `start` degrees, in the vertical plane
/// through the z axis there, swept along the arc to `end` degrees, its half-width running
/// linearly with the angle as 9 − 3θ/90° − `inset`.
fn square_along(
    k: &mut DefaultKernel,
    start: f64,
    end: f64,
    inset: f64,
) -> Result<BodyId, Refusal> {
    let h = |deg: f64| 9.0 - 3.0 * deg / 90.0 - inset;
    let (s, c) = start.to_radians().sin_cos();
    // The plane through the z axis at `start`, its sketch x out from the axis and y up z.
    let radial = v3(c, s, 0.0);
    let on_plane = plane(p3(0.0, 0.0, 0.0), radial, radial.cross(&v3(0.0, 0.0, 1.0)));
    let hs = h(start);
    let profile = sketch(on_plane, vec![rectangle(30.0 - hs, -hs, 30.0 + hs, hs)]);
    let path = vec![on(xy(0.0), arc(p2(0.0, 0.0), 30.0, start, end - start))];
    sweep_law(k, &profile, path, 0.0, h(end) / hs)
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let outside = square_along(k, 0.0, 90.0, 0.0)?;
    let hole = square_along(k, -1.0, 91.0, 2.0)?;
    cut(k, outside, hole)
}

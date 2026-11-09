//! A rod (the cylinder of radius 5 about the z axis from z = 0 to z = 20) threaded by a bar (the box
//! 4 ≤ x ≤ 12, |y| ≤ 8, |z| ≤ 1) turned by θ about the z axis while rising 6s + 0.3s², s = θ/360°,
//! as θ runs from −90° to 1260°.
//!
//! That motion is a frame riding the curve (cos θ, sin θ, 6s + 0.3s²) upright (its x axis along
//! the curve's level tangent turns by θ, its origin rises with it): the curve as cubic Hermite
//! pieces, 32 a turn. What the bar (drawn where the motion starts) passes through is cut from the
//! rod by cadrs's cut by a swept solid along a path (meshed); of the bar, its part within 5.5 of the axis
//! (the motion's turns and rises map the cylinder about the axis onto itself).
use crate::Refusal;
use crate::bench::{block, moved, p3, turned, v3};
use crate::shared::swept::{carry, cut_swept, near_axis, rod, z_axis};
use cadrs_kernel::{BodyId, DefaultKernel, PathCurve};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let rise = |t: f64| {
        let s = t / 360.0;
        6.0 * s + 0.3 * s * s
    };
    let at = |t: f64| {
        let r = t.to_radians();
        p3(r.cos(), r.sin(), rise(t))
    };
    // d/dθ (per degree) of the curve.
    let tangent = |t: f64| {
        let r = t.to_radians();
        let k = std::f64::consts::PI / 180.0;
        v3(-r.sin() * k, r.cos() * k, (6.0 + 0.6 * t / 360.0) / 360.0)
    };
    let (from, to) = (-90.0f64, 1260.0f64);
    let n = (((to - from) / 360.0) * 32.0).ceil() as usize;
    let path: Vec<PathCurve> = (0..n)
        .map(|i| {
            let (a, b) = (
                from + (to - from) * i as f64 / n as f64,
                from + (to - from) * (i + 1) as f64 / n as f64,
            );
            let h = b - a;
            PathCurve::Bezier3 {
                poles: vec![
                    at(a),
                    at(a) + tangent(a) * (h / 3.0),
                    at(b) - tangent(b) * (h / 3.0),
                    at(b),
                ],
                weights: vec![],
            }
        })
        .collect();
    let bar = block(k, p3(4.0, -8.0, -1.0), p3(12.0, 8.0, 1.0))?;
    let bar = near_axis(k, bar, 5.5)?;
    let bar = turned(k, bar, z_axis(), from)?;
    let bar = moved(k, bar, v3(0.0, 0.0, rise(from)))?;
    let r = rod(k)?;
    cut_swept(k, r, bar, &carry(path))
}

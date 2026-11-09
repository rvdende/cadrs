//! What the cadrs implementations of the sweep cases share: a solid sweep of a sketch along a
//! path, the path's pieces (sketch lines and arcs, Bézier curves in space), and a helix as
//! cadrs's Part Studio draws one.
//!
//! cadrs's kernel has no helical curve of its own: its Part Studio's Helix feature hands the
//! kernel the helix as rational Bézier pieces, sixteen a turn, each a rational quadratic lying on
//! the helix's cylinder (its arc of the circle, the height rising with it), and the kernel's
//! sweep recognises a path that follows a helix and sweeps the profile along it by the helix's
//! exact screw motion. [`helix`] draws it the same way.
use crate::Refusal;
use crate::bench::{P3, V3};
use cadrs_kernel::{
    BodyId, BodyKind, Curve2, DefaultKernel, Kernel, PathCurve, Plane, Profile, SweepControl,
    SweepSpec,
};
use std::f64::consts::TAU;

/// The first body of an operation's result.
pub fn first(r: cadrs_kernel::OpResult) -> Result<BodyId, Refusal> {
    r.bodies
        .first()
        .copied()
        .ok_or_else(|| Refusal::from("the operation made no body"))
}

/// The solid `profile`'s regions sweep along `path` (cadrs's Sweep, Solid, no profile control:
/// the profile keeps its angle to the path, corners mitred).
pub fn sweep(
    k: &mut DefaultKernel,
    profile: &Profile,
    path: Vec<PathCurve>,
) -> Result<BodyId, Refusal> {
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        corners: Default::default(),
        faces: vec![],
    };
    first(k.sweep_with(profile, &spec)?)
}

/// [`sweep`], round the path's sharp corners by turning the profile about each corner (not
/// mitred).
pub fn sweep_round_corners(
    k: &mut DefaultKernel,
    profile: &Profile,
    path: Vec<PathCurve>,
) -> Result<BodyId, Refusal> {
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        corners: cadrs_kernel::SweepCorners::Round,
        faces: vec![],
    };
    first(k.sweep_with(profile, &spec)?)
}

/// A sketch curve of a path on its plane.
pub fn on(plane: Plane, curve: Curve2) -> PathCurve {
    PathCurve::Sketch { plane, curve }
}

/// The conical helix about the axis through `origin` along +z, its radius going from `r0` to
/// `r1` as it rises `height`, starting at angle `start` (radians, from +x) and turning
/// right-handed (from +x toward +y) `turns` turns, radius, angle and height each in proportion:
/// as cadrs's Helix feature draws one on a cone, sixteen quintic Bézier pieces a turn, each
/// matching the helix's points and first and second derivatives at its ends.
pub fn conical_helix(
    origin: P3,
    r0: f64,
    r1: f64,
    height: f64,
    start: f64,
    turns: f64,
) -> Vec<PathCurve> {
    let n = ((turns.abs() * 16.0).ceil() as usize).max(1);
    let at2 = |s: f64| {
        let th = start + TAU * turns * s;
        let dth = TAU * turns;
        let (r, dr) = (r0 + (r1 - r0) * s, r1 - r0);
        let (c, sn) = (th.cos(), th.sin());
        let (radial, tangential) = (V3::new(c, sn, 0.0), V3::new(-sn, c, 0.0));
        let z = V3::new(0.0, 0.0, 1.0);
        (
            origin + z * (height * s) + radial * r,
            z * height + radial * dr + tangential * (r * dth),
            tangential * (2.0 * dr * dth) - radial * (r * dth * dth),
        )
    };
    (0..n)
        .map(|k| {
            let (s0, s1) = (k as f64 / n as f64, (k + 1) as f64 / n as f64);
            let h = s1 - s0;
            let (p0, d0, a0) = at2(s0);
            let (p5, d5, a5) = at2(s1);
            PathCurve::Bezier3 {
                poles: vec![
                    p0,
                    p0 + d0 * (h / 5.0),
                    p0 + d0 * (2.0 * h / 5.0) + a0 * (h * h / 20.0),
                    p5 - d5 * (2.0 * h / 5.0) + a5 * (h * h / 20.0),
                    p5 - d5 * (h / 5.0),
                    p5,
                ],
                weights: vec![],
            }
        })
        .collect()
}

/// A Bézier curve in space through `poles` (a polynomial one).
pub fn bezier(poles: &[P3]) -> PathCurve {
    PathCurve::Bezier3 {
        poles: poles.to_vec(),
        weights: vec![],
    }
}

/// The helix about the axis through `origin` along +z of `radius`, rising `pitch` a turn,
/// starting at angle `start` (radians, from +x) and turning right-handed (from +x toward +y)
/// `turns` turns: as cadrs's Helix feature draws it, sixteen rational quadratic Bézier pieces
/// a turn, each the arc of the circle on its cylinder (its middle pole at the corner of the
/// tangents, weighted by the cosine of half its turn), the height rising with it.
pub fn helix(origin: P3, radius: f64, pitch: f64, start: f64, turns: f64) -> Vec<PathCurve> {
    let n = ((turns.abs() * 16.0).ceil() as usize).max(1);
    let height = pitch * turns;
    let at = |t: f64, r: f64, z: f64| origin + V3::new(r * t.cos(), r * t.sin(), z);
    (0..n)
        .map(|i| {
            let (s0, s1) = (i as f64 / n as f64, (i + 1) as f64 / n as f64);
            let (t0, t1) = (start + TAU * turns * s0, start + TAU * turns * s1);
            let half = (t1 - t0) / 2.0;
            let (z0, z1) = (height * s0, height * s1);
            PathCurve::Bezier3 {
                poles: vec![
                    at(t0, radius, z0),
                    at(t0 + half, radius / half.cos(), (z0 + z1) / 2.0),
                    at(t1, radius, z1),
                ],
                weights: vec![1.0, half.cos(), 1.0],
            }
        })
        .collect()
}

/// The point at angle `t` (radians, from +x) of the helix about the z axis through `origin` of
/// `radius` rising `pitch` a turn from angle `start`, and its tangent there.
pub fn helix_at(origin: P3, radius: f64, pitch: f64, start: f64, t: f64) -> (P3, V3) {
    let z = pitch * (t - start) / TAU;
    (
        origin + V3::new(radius * t.cos(), radius * t.sin(), z),
        V3::new(-radius * t.sin(), radius * t.cos(), pitch / TAU),
    )
}

/// The solid `profile`'s regions sweep along `path`, twisting by `twist` (radians,
/// right-handed about the path) and scaling by `scale` (its size at the end over the start)
/// about the path's point, both in proportion to the length travelled (cadrs's
/// `Kernel::sweep_with_law`).
pub fn sweep_law(
    k: &mut DefaultKernel,
    profile: &Profile,
    path: Vec<PathCurve>,
    twist: f64,
    scale: f64,
) -> Result<BodyId, Refusal> {
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::None,
        corners: Default::default(),
        faces: vec![],
    };
    first(k.sweep_with_law(profile, &spec, &cadrs_kernel::SweepLaw { twist, scale })?)
}

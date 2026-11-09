//! A block (the box from (−10, −10, −10) to (40, 40, 0)) grooved by an upright bull-nose mill (a
//! cylinder of radius 2 from z = −1.5 to 5, its bottom corner rounded to radius 0.5) run along a
//! line from (0, 0) to (20, 0), a quarter turn about (20, 10) to (30, 10), and a line to (30, 30).
//!
//! An upright solid of revolution moved across in its own planes fills, at each height, the
//! region within its radius there of the path: its section through its axis square to the path,
//! carried along it upright (cadrs's Sweep along the path, the profile's plane kept holding the
//! z axis), with the mill's halves beyond the path's ends (its half section turned half a turn,
//! cadrs's Revolve). The groove is the block less the three.
use crate::Refusal;
use crate::bench::{arc, axis, chain, cut, line, p2, p3, plane, revolve, v3, xy};
use crate::shared::geometry::Section;
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, BodyKind, DefaultKernel, Kernel, PathCurve, SweepControl, SweepSpec};

/// The mill's axial section in the plane through `foot` (on its axis at the block's top) square to
/// `across` (horizontal): sketch x along `across`, sketch y up.
fn section(foot: crate::bench::P3, across: crate::bench::V3) -> Section {
    let on = plane(foot, across, across.cross(&v3(0.0, 0.0, 1.0)));
    let w = 2f64.sqrt() / 2.0;
    Section::on(on)
        .line((-1.5, -1.5), (1.5, -1.5))
        .conic((1.5, -1.5), (2.0, -1.5), w, (2.0, -1.0))
        .line((2.0, -1.0), (2.0, 5.0))
        .line((2.0, 5.0), (-2.0, 5.0))
        .line((-2.0, 5.0), (-2.0, -1.0))
        .conic((-2.0, -1.0), (-2.0, -1.5), w, (-1.5, -1.5))
}

/// The mill about the upright through `foot`, its half on the side of the plane through `foot`
/// square to `ahead` that `ahead` points to: its half section turned half a turn about the
/// upright (cadrs's Revolve), from the side square to `ahead` round through `ahead`.
fn half_mill(
    k: &mut DefaultKernel,
    foot: crate::bench::P3,
    ahead: crate::bench::V3,
) -> Result<BodyId, Refusal> {
    let up = v3(0.0, 0.0, 1.0);
    // Sketch x out to the side (a quarter turn about the upright before `ahead`), sketch y up.
    let side = ahead.cross(&up);
    let on = plane(foot, side, side.cross(&up));
    let w = 2f64.sqrt() / 2.0;
    let half = Section::on(on)
        .line((0.0, -1.5), (1.5, -1.5))
        .conic((1.5, -1.5), (2.0, -1.5), w, (2.0, -1.0))
        .line((2.0, -1.0), (2.0, 5.0))
        .line((2.0, 5.0), (0.0, 5.0))
        .line((0.0, 5.0), (0.0, -1.5));
    revolve(k, &half.sketch(), axis(foot, up), Some(180.0))
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = crate::bench::block(k, p3(-10.0, -10.0, -10.0), p3(40.0, 40.0, 0.0))?;
    let path = chain(vec![
        line(p2(0.0, 0.0), p2(20.0, 0.0)),
        arc(p2(20.0, 10.0), 10.0, -90.0, 90.0),
        line(p2(30.0, 10.0), p2(30.0, 30.0)),
    ]);
    let path: Vec<PathCurve> = path
        .curves
        .into_iter()
        .map(|c| PathCurve::Sketch {
            plane: xy(0.0),
            curve: c,
        })
        .collect();
    // The section square to the path where it starts (along +x): across it is +y.
    let profile = section(p3(0.0, 0.0, 0.0), v3(0.0, 1.0, 0.0)).sketch();
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::LockDirection(crate::bench::unit(v3(0.0, 0.0, 1.0))),
        corners: Default::default(),
        faces: vec![],
    };
    let swept = first(k.sweep_with(&profile, &spec)?)?;
    let start = half_mill(k, p3(0.0, 0.0, 0.0), v3(-1.0, 0.0, 0.0))?;
    let end = half_mill(k, p3(30.0, 30.0, 0.0), v3(0.0, 1.0, 0.0))?;
    let b = cut(k, b, swept)?;
    let b = cut(k, b, start)?;
    cut(k, b, end)
}

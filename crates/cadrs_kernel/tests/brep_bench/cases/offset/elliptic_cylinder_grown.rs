//! The elliptic cylinder of `geometry/elliptic-cylinder` (x²/25 + y²/9 ≤ 1 from z = 0 to 10) grown
//! by 1: every point within 1 of it.
//!
//! cadrs's rounded offset unites a tube round each rim with the faces' slabs, and the tube round
//! an ellipse touches the side's slab along the rim, which its boolean leaves open. So the grown
//! solid is built as what it is: the ellipse extruded from z = −1 to 11 (the slabs of the ends and
//! the body), united with the band outside it, the section {0 ≤ w ≤ 1, 0 ≤ z ≤ 10} with a quarter
//! circle of radius 1 below and above it (w out along the ellipse's normal) swept round the
//! ellipse upright (cadrs's Sweep, the section's plane kept holding the z axis): its outer face
//! the side's offset, its rounds the rims' tubes' outer quarters.
use crate::Refusal;
use crate::bench::{extrude_along, p3, plane, sketch, union, unit, v3, xy};
use crate::shared::geometry::{Section, ellipse};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, BodyKind, DefaultKernel, Kernel, PathCurve, SweepControl, SweepSpec};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let d = 1.0;
    let s = sketch(xy(-d), vec![ellipse((0.0, 0.0), 5.0, 3.0)]);
    let prism = extrude_along(k, &s, v3(0.0, 0.0, 1.0), 10.0 + 2.0 * d)?;
    // The band's section where the ellipse crosses +x, (5, 0): in the plane y = 0, sketch x out
    // along +x (the ellipse's normal there), sketch y up.
    let on = plane(p3(5.0, 0.0, 0.0), v3(1.0, 0.0, 0.0), v3(0.0, -1.0, 0.0));
    let w = 2f64.sqrt() / 2.0;
    let band = Section::on(on)
        .conic((0.0, -d), (d, -d), w, (d, 0.0))
        .line((d, 0.0), (d, 10.0))
        .conic((d, 10.0), (d, 10.0 + d), w, (0.0, 10.0 + d))
        .line((0.0, 10.0 + d), (0.0, -d));
    let path: Vec<PathCurve> = ellipse((0.0, 0.0), 5.0, 3.0)
        .curves
        .into_iter()
        .map(|c| PathCurve::Sketch {
            plane: xy(0.0),
            curve: c,
        })
        .collect();
    let spec = SweepSpec {
        body: BodyKind::Solid,
        path,
        control: SweepControl::LockDirection(unit(v3(0.0, 0.0, 1.0))),
        corners: Default::default(),
        faces: vec![],
    };
    let swept = first(k.sweep_with(&band.sketch(), &spec)?)?;
    union(k, prism, swept)
}

//! A pipe of radius 2 swept along three edges of the cube of side 30, (0, 0, 0) to (30, 0, 0)
//! to (30, 30, 0) to (30, 30, 30), mitered at its two corners: the circle drawn on the Right
//! plane (x = 0) about the origin, swept along the three lines, the first two drawn on the Top
//! plane and the third on the plane y = 30 (cadrs's sweep mitres each corner of its path).
use crate::Refusal;
use crate::bench::{line, p2, round, sketch, xy, xz, yz};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = sketch(yz(0.0), vec![round((0.0, 0.0), 2.0)]);
    let path = vec![
        on(xy(0.0), line(p2(0.0, 0.0), p2(30.0, 0.0))),
        on(xy(0.0), line(p2(30.0, 0.0), p2(30.0, 30.0))),
        // on the plane y = 30, its sketch (x, z)
        on(xz(30.0), line(p2(30.0, 0.0), p2(30.0, 30.0))),
    ];
    sweep(k, &section, path)
}

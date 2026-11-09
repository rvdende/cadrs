//! A square bar 10 × 10 swept along two straight runs, (0, 0, 0) to (40, 0, 0) and on, turned
//! 120° to the left, to (20, 20√3, 0), mitered at the corner: the square drawn on the Right
//! plane (x = 0) centered on the origin, swept along the two lines drawn on the Top plane
//! (cadrs's sweep mitres a corner of its path).
use crate::Refusal;
use crate::bench::{line, p2, rectangle, sketch, xy, yz};
use crate::shared::sweep::{on, sweep};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let section = sketch(yz(0.0), vec![rectangle(-5.0, -5.0, 5.0, 5.0)]);
    let corner = p2(40.0, 0.0);
    let path = vec![
        on(xy(0.0), line(p2(0.0, 0.0), corner)),
        on(xy(0.0), line(corner, p2(20.0, 20.0 * 3f64.sqrt()))),
    ];
    sweep(k, &section, path)
}

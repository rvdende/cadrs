//!
//! cadrs's Loft with guides: the square at z = 0 sketched on the Top plane and the apex, along the
//! four ridges, each a Bézier curve through its control points.
use crate::Refusal;
use crate::bench::{p3, rectangle, sketch, xy};
use crate::shared::loft::guided_loft;
use crate::shared::sweep::bezier;
use cadrs_kernel::{BodyId, DefaultKernel, LoftSection};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let apex = p3(0.0, 0.0, 24.0);
    // the ridge through the corner with x and y of signs (sx, sy)
    let ridge = |sx: f64, sy: f64| {
        vec![bezier(&[
            p3(9.0 * sx, 9.0 * sy, 0.0),
            p3(15.0 * sx, 15.0 * sy, 8.0),
            p3(12.0 * sx, 12.0 * sy, 19.0),
            apex,
        ])]
    };
    let foot = sketch(xy(0.0), vec![rectangle(-9.0, -9.0, 9.0, 9.0)]);
    let ridges = vec![
        ridge(1.0, 1.0),
        ridge(-1.0, 1.0),
        ridge(-1.0, -1.0),
        ridge(1.0, -1.0),
    ];
    guided_loft(
        k,
        vec![LoftSection::Profile(foot), LoftSection::Point(apex)],
        ridges,
    )
}

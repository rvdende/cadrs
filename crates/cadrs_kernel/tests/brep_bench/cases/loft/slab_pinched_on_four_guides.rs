//!
//! cadrs's Loft with guides: the rectangle at z = 0 and the one at z = 24, each sketched on its
//! plane, along the four guides, each a Bézier curve through its control points.
use crate::Refusal;
use crate::bench::{p3, rectangle, sketch, xy};
use crate::shared::loft::guided_loft;
use crate::shared::sweep::bezier;
use cadrs_kernel::{BodyId, DefaultKernel, LoftSection};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the guide through the corner with x and y of signs (sx, sy)
    let guide = |sx: f64, sy: f64| {
        vec![bezier(&[
            p3(12.0 * sx, 3.2 * sy, 0.0),
            p3(18.0 * sx, -2.8 * sy, 8.0),
            p3(15.0 * sx, 0.2 * sy, 19.0),
            p3(3.0 * sx, 12.2 * sy, 24.0),
        ])]
    };
    let foot = sketch(xy(0.0), vec![rectangle(-12.0, -3.2, 12.0, 3.2)]);
    let head = sketch(xy(24.0), vec![rectangle(-3.0, -12.2, 3.0, 12.2)]);
    let guides = vec![
        guide(1.0, 1.0),
        guide(-1.0, 1.0),
        guide(-1.0, -1.0),
        guide(1.0, -1.0),
    ];
    guided_loft(
        k,
        vec![LoftSection::Profile(foot), LoftSection::Profile(head)],
        guides,
    )
}

//! What the cadrs implementations of the singular cases share: cutting away everything on one
//! side of a plane.
use crate::Refusal;
use crate::bench::{P3, V3, cut, extrude, rectangle, sketch, square_to};
use cadrs_kernel::{BodyId, DefaultKernel};

/// `body` less everything on the side `normal` points to of the plane through `point`: cut
/// with a block sketched as a square of side 60 on that plane, centred on `point`, and extruded
/// 30 along `normal` (the bodies cut here lie within 20 of the origin).
pub fn cut_beyond(
    k: &mut DefaultKernel,
    body: BodyId,
    point: P3,
    normal: V3,
) -> Result<BodyId, Refusal> {
    let s = sketch(
        square_to(point, normal),
        vec![rectangle(-30.0, -30.0, 30.0, 30.0)],
    );
    let tool = extrude(k, &s, 30.0)?;
    cut(k, body, tool)
}

/// The ball of `radius` about `center` with its poles on the axis through `center` along `dir`
/// (cadrs's sphere primitive, its seam on the side the sketch plane through that axis would put
/// it).
pub fn ball_about(
    k: &mut DefaultKernel,
    center: P3,
    dir: V3,
    radius: f64,
) -> Result<BodyId, Refusal> {
    let seam = crate::bench::through_axis(center, dir).x_dir.into_inner();
    crate::bench::ball_oriented(k, center, radius, dir, seam)
}

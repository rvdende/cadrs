//! The pyramid on the rectangle from (−10, −6, 0) to (10, 6, 0) with its apex at (0, 0, 12), all
//! eight edges rounded to radius 3/2 in one fillet (its apex splits: a short round along y = 0
//! between the long sides, closed by sphere patches).
//!
//! The rectangle sketched on the Top plane and lofted to the apex. cadrs's Fillet closes the apex
//! as four rounds meeting in one ball, which this apex has none of, so the solid is built as the
//! case composes it: the pyramid shrunk by 3/2 (its faces moved in, the apex a short ridge: a hip
//! roof), then grown by 3/2 (every point within 3/2 of it).
use crate::Refusal;
use crate::bench::{p3, polygon, sketch, xy};
use crate::shared::offset::{grown, offset};
use crate::shared::sweep::first;
use cadrs_kernel::{BodyId, BodyKind, DefaultKernel, Kernel, LoftEnd, LoftSection, LoftSpec};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let base = sketch(
        xy(0.0),
        vec![polygon(&[
            (-10.0, -6.0),
            (10.0, -6.0),
            (10.0, 6.0),
            (-10.0, 6.0),
        ])],
    );
    let spec = LoftSpec {
        body: BodyKind::Solid,
        sections: vec![
            LoftSection::Profile(base),
            LoftSection::Point(p3(0.0, 0.0, 12.0)),
        ],
        start: LoftEnd::default(),
        end: LoftEnd::default(),
        source: 1,
    };
    let pyramid = first(k.loft_with(&spec)?)?;
    let roof = offset(k, pyramid, -1.5, true)?;
    grown(k, roof, 1.5)
}

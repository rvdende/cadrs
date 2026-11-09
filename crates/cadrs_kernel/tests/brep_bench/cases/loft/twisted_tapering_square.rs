//!
//! cadrs's Loft with guides: the square of side 30 sketched at z = 0 and the one of side 10 at
//! z = 40, along the four spirals through their corners, each a conical helix drawn as cadrs's
//! Helix feature draws one on a cone (from radius 15√2 to 5√2, half a turn).
use crate::Refusal;
use crate::bench::{p3, rectangle, sketch, xy};
use crate::shared::loft::guided_loft;
use crate::shared::sweep::conical_helix;
use cadrs_kernel::{BodyId, DefaultKernel, LoftSection};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let foot = sketch(xy(0.0), vec![rectangle(-15.0, -15.0, 15.0, 15.0)]);
    let head = sketch(xy(40.0), vec![rectangle(-5.0, -5.0, 5.0, 5.0)]);
    let r = 2f64.sqrt();
    let guides = [45.0f64, 135.0, 225.0, 315.0]
        .iter()
        .map(|a| {
            conical_helix(
                p3(0.0, 0.0, 0.0),
                15.0 * r,
                5.0 * r,
                40.0,
                a.to_radians(),
                0.5,
            )
        })
        .collect();
    guided_loft(
        k,
        vec![LoftSection::Profile(foot), LoftSection::Profile(head)],
        guides,
    )
}

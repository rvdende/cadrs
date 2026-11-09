//!
//! cadrs's Loft with guides: the square sketched at z = 0 and the circle at z = 30, along straight
//! guides pairing them, two from each corner of the square to the points of the circle nearest
//! the sides beside it (so the wall between those two is the cone from the corner to the quarter
//! circle, and between neighbouring corners' guides the triangle to one point).
use crate::Refusal;
use crate::bench::{p3, rectangle, round, sketch, xy};
use crate::shared::loft::guided_loft;
use crate::shared::sweep::bezier;
use cadrs_kernel::{BodyId, DefaultKernel, LoftSection};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    // the square's corners counter-clockwise from (20, −20), and the circle's points nearest its
    // sides from (0, −15): corner k lies between points k and k + 1
    let square = [(20.0, -20.0), (20.0, 20.0), (-20.0, 20.0), (-20.0, -20.0)];
    let points = [(0.0, -15.0), (15.0, 0.0), (0.0, 15.0), (-15.0, 0.0)];
    let foot = sketch(xy(0.0), vec![rectangle(-20.0, -20.0, 20.0, 20.0)]);
    let top = sketch(xy(30.0), vec![round((0.0, 0.0), 15.0)]);
    let mut guides = Vec::new();
    for (i, &(x, y)) in square.iter().enumerate() {
        for &(u, v) in [points[i], points[(i + 1) % 4]].iter() {
            guides.push(vec![bezier(&[p3(x, y, 0.0), p3(u, v, 30.0)])]);
        }
    }
    guided_loft(
        k,
        vec![LoftSection::Profile(foot), LoftSection::Profile(top)],
        guides,
    )
}

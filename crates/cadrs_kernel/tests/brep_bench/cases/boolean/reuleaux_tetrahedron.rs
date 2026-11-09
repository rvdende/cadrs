//! A Reuleaux tetrahedron: the part common to four balls of radius 10 centered at the vertices
//! of the regular tetrahedron of side 10, (0, 0, 0), (10, 0, 0), (5, 5√3, 0) and
//! (5, 5√3/3, 10√(2/3)).
//!
//! Each ball is cadrs's sphere primitive; their common part is cadrs's boolean, one ball at a
//! time.
use crate::Refusal;
use crate::bench::{ball, common, p3};
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let r3 = 3f64.sqrt();
    let centers = [
        p3(0.0, 0.0, 0.0),
        p3(10.0, 0.0, 0.0),
        p3(5.0, 5.0 * r3, 0.0),
        p3(5.0, 5.0 * r3 / 3.0, 10.0 * (2.0f64 / 3.0).sqrt()),
    ];
    let mut solid = ball(k, centers[0], 10.0)?;
    for &c in &centers[1..] {
        let b = ball(k, c, 10.0)?;
        solid = common(k, solid, b)?;
    }
    Ok(solid)
}

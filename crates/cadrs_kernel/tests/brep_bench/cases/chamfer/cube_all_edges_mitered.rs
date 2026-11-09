//! A cube of side 10 between the origin and (10, 10, 10), all twelve edges chamfered with the
//! same distance 2 on both faces, the corners mitered: each edge its own chamfer, made one after
//! another, ending on the faces at its ends (the chamfers made before it among them), so the
//! three chamfers at each corner meet in a point.
//!
//! The cube is a square extruded; then twelve Chamfer features, one per edge, each edge picked
//! again on the body the chamfers before it left, by the point at its middle.
use crate::Refusal;
use crate::bench::{block, edge_at, p3};
use crate::shared::chamfer::chamfer;
use cadrs_kernel::{BodyId, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut body = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    // The middle of each edge: four along x, four along y, four along z.
    let mut middles = Vec::new();
    for (a, b) in [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)] {
        middles.push(p3(5.0, a, b));
        middles.push(p3(a, 5.0, b));
        middles.push(p3(a, b, 5.0));
    }
    for m in middles {
        let edge = edge_at(k, body, m)?;
        body = chamfer(k, body, &[edge], ChamferSpec::EqualDistance(2.0))?;
    }
    Ok(body)
}

//! The octahedron |x| + |y| + |z| ≤ 10, all twelve edges chamfered with the same distance √6 on
//! both faces (the chamfer of the edge from (10, 0, 0) to (0, 10, 0) the plane x + y = 8), the
//! corners mitered: each edge its own chamfer, made one after another, ending on the faces at
//! its ends (the chamfers made before it among them), so the four chamfers at each corner meet
//! in a point.
//!
//! The octahedron is two square pyramids, each the square through its corners in z = 0 lofted
//! to a point, united; then twelve Chamfer features, one per edge, each edge picked again on the
//! body the chamfers before it left, by the point at its middle.
use crate::Refusal;
use crate::bench::{edge_at, p3};
use crate::shared::chamfer::{chamfer, octahedron};
use cadrs_kernel::{BodyId, ChamferSpec, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let mut body = octahedron(k, 10.0)?;
    let middles = [
        p3(5.0, 5.0, 0.0),
        p3(-5.0, 5.0, 0.0),
        p3(-5.0, -5.0, 0.0),
        p3(5.0, -5.0, 0.0),
        p3(5.0, 0.0, 5.0),
        p3(-5.0, 0.0, 5.0),
        p3(0.0, 5.0, 5.0),
        p3(0.0, -5.0, 5.0),
        p3(5.0, 0.0, -5.0),
        p3(-5.0, 0.0, -5.0),
        p3(0.0, 5.0, -5.0),
        p3(0.0, -5.0, -5.0),
    ];
    for m in middles {
        let edge = edge_at(k, body, m)?;
        body = chamfer(k, body, &[edge], ChamferSpec::EqualDistance(6f64.sqrt()))?;
    }
    Ok(body)
}

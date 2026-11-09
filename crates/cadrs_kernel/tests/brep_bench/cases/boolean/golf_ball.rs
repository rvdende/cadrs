//! A ball of radius 10 at the origin with 62 dimples, each cut by its own ball of radius 1.2
//! centered 10.6 out toward a vertex, an edge's midpoint or a face's center of the icosahedron
//! with vertices (0, ±1, ±φ), (±1, ±φ, 0) and (±φ, 0, ±1). Every ball is cadrs's sphere
//! primitive, its poles on the vertical through its center, as the case asks. The dimples are
//! cut in one boolean with all 62 balls as its tools.
use crate::Refusal;
use crate::bench::{V3, ball, boolean, p3};
use cadrs_kernel::{BodyId, BoolOp, DefaultKernel};

/// The 62 directions, as unit vectors: the icosahedron's 12 vertices, its 30 edges' midpoints and
/// its 20 faces' centers.
fn directions() -> Vec<V3> {
    let phi = (1.0 + 5f64.sqrt()) / 2.0;
    let mut vertices = Vec::new();
    for a in [-1.0, 1.0] {
        for b in [-phi, phi] {
            vertices.extend([V3::new(0.0, a, b), V3::new(a, b, 0.0), V3::new(b, 0.0, a)]);
        }
    }
    // neighbors are an edge, 2, apart
    let edge = |i: usize, j: usize| ((vertices[i] - vertices[j]).norm() - 2.0).abs() < 1e-9;
    let mut out: Vec<V3> = vertices.iter().map(|v| v.normalize()).collect();
    for i in 0..12 {
        for j in i + 1..12 {
            if edge(i, j) {
                out.push((vertices[i] + vertices[j]).normalize());
            }
        }
    }
    for i in 0..12 {
        for j in i + 1..12 {
            for l in j + 1..12 {
                if edge(i, j) && edge(j, l) && edge(i, l) {
                    out.push((vertices[i] + vertices[j] + vertices[l]).normalize());
                }
            }
        }
    }
    out
}

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let golf = ball(k, p3(0.0, 0.0, 0.0), 10.0)?;
    let dimples = directions()
        .into_iter()
        .map(|d| ball(k, p3(0.0, 0.0, 0.0) + d * 10.6, 1.2))
        .collect::<Result<Vec<_>, _>>()?;
    // (One cut by all the dimples' balls at once: they are apart.)
    boolean(k, BoolOp::Subtract, golf, &dimples)
}

//! A plate (the origin to (40, 30, 5)) with a square boss on it ((12, 7, 5) to (28, 23, 15)): the
//! boss's four upright edges rounded to radius 3, then its root (one tangent chain round the boss
//! once the uprights are round) rounded to radius 2, concave.
//!
//! Two Fillet features one after another, as the case composes the corners: the four upright
//! edges, then the root, picked by one straight edge (tangent propagation takes the rest).
use crate::Refusal;
use crate::bench::{block, edge_at, edges_at, p3, union};
use crate::shared::fillet::fillet;
use cadrs_kernel::{BodyId, DefaultKernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let plate = block(k, p3(0.0, 0.0, 0.0), p3(40.0, 30.0, 5.0))?;
    let boss = block(k, p3(12.0, 7.0, 5.0), p3(28.0, 23.0, 15.0))?;
    let b = union(k, plate, boss)?;
    let uprights = edges_at(
        k,
        b,
        &[
            p3(12.0, 7.0, 10.0),
            p3(28.0, 7.0, 10.0),
            p3(28.0, 23.0, 10.0),
            p3(12.0, 23.0, 10.0),
        ],
    )?;
    let rounded = fillet(k, b, &uprights, 3.0)?;
    let root = edge_at(k, rounded, p3(20.0, 7.0, 5.0))?;
    fillet(k, rounded, &[root], 2.0)
}

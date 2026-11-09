//! A cube of side 10 between the origin and (10, 10, 10), the edge where its faces x = 10 and
//! y = 10 meet rounded with a radius that grows linearly along it, r = 1 + z/10: 1 at the bottom
//! and 2 at the top, its sections square to the edge circular.
//!
//! The cube is a square extruded; the edge is picked by the point at its middle and given a
//! variable fillet, radius 1 at its end on the bottom and 2 at its end on the top (cadrs
//! interpolates the radius linearly between the two).
use crate::Refusal;
use crate::bench::{block, edge_at, p3};
use crate::shared::fillet::fillet_variable;
use cadrs_kernel::{BodyId, DefaultKernel, FilletLaw, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let cube = block(k, p3(0.0, 0.0, 0.0), p3(10.0, 10.0, 10.0))?;
    let edge = edge_at(k, cube, p3(10.0, 10.0, 5.0))?;
    let info = k
        .edges(cube)?
        .into_iter()
        .find(|e| e.id == edge)
        .ok_or("the edge is gone")?;
    // the law runs from the edge's start to its end
    let radii = if info.start.z < info.end.z {
        vec![(0.0, 1.0), (1.0, 2.0)]
    } else {
        vec![(0.0, 2.0), (1.0, 1.0)]
    };
    fillet_variable(k, cube, &[FilletLaw { edge, radii }])
}

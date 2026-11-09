//! A box from the origin to (20, 10, 6), the four edges around its top face z = 6 chamfered
//! with distance 1 across the top and 2 down the side: the box cut by those four planes, the
//! chamfers of neighbouring edges meeting along slanting lines from the box's corners at z = 4
//! to the top face's corners.
//!
//! The box is a rectangle extruded; the four top edges go into one Chamfer feature of two
//! distances, 1 then 2. cadrs puts the first distance on the face whose loop runs along the edge
//! (as Onshape does), so each edge whose first face is not the top is flipped on its own (the
//! dialog's direction override): the top's loop runs along an edge when the top lies on the left
//! of the edge's direction, seen from above.
use crate::Refusal;
use crate::bench::{block, edges_at, p3, v3};
use crate::shared::chamfer::chamfer_with;
use cadrs_kernel::{BodyId, ChamferOpts, ChamferSpec, DefaultKernel, Kernel};

pub fn build(k: &mut DefaultKernel) -> Result<BodyId, Refusal> {
    let b = block(k, p3(0.0, 0.0, 0.0), p3(20.0, 10.0, 6.0))?;
    let top_center = p3(10.0, 5.0, 6.0);
    let picks = [
        p3(10.0, 0.0, 6.0),
        p3(20.0, 5.0, 6.0),
        p3(10.0, 10.0, 6.0),
        p3(0.0, 5.0, 6.0),
    ];
    let edges = edges_at(k, b, &picks)?;
    let mut opts = ChamferOpts::new(ChamferSpec::TwoDistances(1.0, 2.0));
    let infos = k.edges(b)?;
    for info in infos.iter().filter(|i| edges.contains(&i.id)) {
        // the top lies on the left of the edge when up × tangent points into it
        let left = v3(0.0, 0.0, 1.0).cross(&info.start_tangent);
        if left.dot(&(top_center - info.mid)) < 0.0 {
            opts.flipped.push(info.id);
        }
    }
    chamfer_with(k, b, &edges, &opts)
}
